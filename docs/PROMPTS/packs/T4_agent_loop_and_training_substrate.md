# T4 — AI-Native MCP Agent Loop + Agentic Training Substrate

**Pack ID:** `T4`
**Phase owned:** `G7` — Agent Loop Closure
**Status:** All items authored `DRAFT`. An L1 promotes to `READY`.
**Conforms to:** `docs/PROMPTS/03_PROMPT_SCHEMA.md`

---

## What this pack owns

Two halves of one thesis.

**Half A — the loop.** `build → run → detect → propose → re-run`, under a human gate, driven
entirely over the MCP tool surface and the TCP Engine Bridge, with no desktop session anywhere in
the path. This half is about *reliability and observability*: a tool call that silently no-ops, a
mutation that cannot be undone, an op-log that cannot reconstruct what the agent did, or an
observation that requires a window are each individually fatal to the loop.

**Half B — the training and evaluation substrate.** What a frontier lab would actually need to
adopt Eustress as an RL / agentic-eval environment: a task specification format, a `reset`/`step`
API, seeded and replayable episodes, generated observation and action spaces, physically verified
outcome scoring, an adversarial anti-gaming suite, containerised GUI-free integration, measured
throughput, and a published benchmark with a reproducible harness.

**What makes this environment different from every gym already published** — and every item in
Half B must preserve these four properties, not erode them:

1. **Real first-principles physics.** Avian rigid-body dynamics plus the realism tier, not a
   hand-authored reward surface. Task success is a consequence of simulated dynamics.
2. **Real time compression.** An episode can cover simulated hours or years of process time. This
   is also the single most dangerous property in the repo — see `G7.15`, which makes compressed
   episodes honest before any of them are scored.
3. **Real engineering artifacts.** The truck-based CAD kernel means a task can require a *part*,
   and the part can be measured — mass, bounding volume, interference, feature count — rather than
   described.
4. **Physically verifiable success.** A task's success predicate reads simulation and geometry
   state. Nothing in this pack is scored by string match. `G7.18` and `G7.19` exist to enforce that.

---

## Workloads this pack's evidence feeds

| Workload | How this pack produces it |
|---|---|
| **W3 — Trust & Verifiability** | Byte-identical episode replays (`G7.14`, `G7.17`), op-log reconstruction (`G7.06`), container-reproducible benchmark runs (`G7.20`, `G7.22`) |
| **W5 — Extension Surface** | The `.etask` format (`G7.12`), the generated observation/action spec (`G7.16`), and the lab integration guide (`G7.24`) are the terms on which an outside party builds against the substrate |
| **W6 — Operator Leverage** | Loop-iteration latency (`G7.04`), fork–rehearse–commit (`G7.08`), throughput per CPU-hour (`G7.21`) |
| **W1 — Provable Quality** | The published benchmark and measured baseline (`G7.22`, `G7.23`) |
| **W4 — Vertical Proof** | The 12-task suite is an engineering vertical run end to end with no unwired buttons (`G7.18`) |

---

## ITEM ZERO

**`G7.01` — MCP tool-surface conformance census.**

Nothing else in this pack may start first. The pack's central claim is that Eustress has a reliable
agent surface; today the honest state is that nobody has ever invoked all of it and recorded what
came back. Every subsequent item cites `tool_conformance.json`, and the two most-cited numbers in
the pack — how many tools silently no-op, and what a tool call costs in wall-clock — do not exist
until `G7.01` produces them.

---

## Dependency graph

| ID | Title | Tier | Depends on |
|---|---|---|---|
| `G7.01` | MCP tool-surface conformance census | L | — *(ITEM ZERO)* |
| `G7.02` | Zero silent no-ops on the agent surface | L | `G7.01` |
| `G7.03` | Headless render tier: agent eyes with no desktop | XL | `G7.01` |
| `G7.04` | One-iteration agent-loop latency baseline | M | `G7.02` |
| `G7.05` | Every agent mutation is transactional and undoable | L | `G7.02` |
| `G7.06` | Op-log replay reconstructs the world exactly | L | `G7.05` |
| `G7.07` | The human gate is enforced, not hinted | M | `G7.02` |
| `G7.08` | Fork, rehearse, commit — with zero residue | L | `G7.06` |
| `G7.09` | Independent agent camera, deterministic captures | L | `G7.03`, `G1.12` |
| `G7.10` | Detect and propose: machine-readable failure proposals | L | `G7.04`, `G7.06`, `G1.12` |
| `G7.11` | Retire the FoundationModelDispatcher claim | S | `G7.01` |
| `G7.12` | `.etask` — the environment and task specification format | L | `G7.01`, `G1.12`, `G1.48` |
| `G7.13` | `env.reset` / `env.step` over the Engine Bridge | L | `G7.02`, `G7.12` |
| `G7.14` | Episode determinism under seed and tick-rate variation | L | `G7.13`, `G7.15` |
| `G7.15` | Dropped-tick accounting invalidates dishonest episodes | M | `G7.01`, `G1.12` |
| `G7.16` | Observation and action spaces generated from code | M | `G7.13` |
| `G7.17` | Episode recording and byte-exact replay | L | `G7.14`, `G7.06` |
| `G7.18` | Physically verified outcome scoring | XL | `G7.12`, `G7.13`, `G1.12` |
| `G7.19` | Anti-gaming: the adversarial exploit suite | L | `G7.18`, `G1.12` |
| `G7.20` | Containerised, GUI-free lab integration | L | `G7.03`, `G7.13` |
| `G7.21` | Throughput: episodes per CPU-hour, measured | M | `G7.13`, `G7.20` |
| `G7.22` | EUSTRESS-PHYS-12 — public benchmark + reproducible harness | XL | `G7.18`, `G7.19`, `G7.20`, `G7.21`, `G1.12`, `G1.48` |
| `G7.23` | Measured baseline agent results on EUSTRESS-PHYS-12 | L | `G7.22`, `G7.09`, `G1.12` |
| `G7.24` | Lab integration guide | M | `G7.20`, `G7.22`, `G1.12`, `G1.48`, `G7.23` |

Publishable external artifacts: **`G7.12`** (specification), **`G7.22`** (benchmark + harness),
**`G7.23`** (reproducible measured result), **`G7.24`** (integration guide).

---

## Architectural constraint every item in this pack inherits

The evaluation harness this pack builds lives in a **new crate, `eustress/crates/agent-eval/`,
which does not depend on `eustress-engine`.** It talks to a running engine or `eustress-headless`
over the TCP Engine Bridge using `eustress-bridge-client` (`eustress/crates/bridge-client/`, whose
only dependency is `serde_json`).

This is not a stylistic preference. A full engine build takes 10–15 minutes and the workspace shares
one `target/`. A harness that links the engine costs 10–15 minutes per iteration; a harness that
speaks to it over a socket costs seconds. Any item that "helpfully" pulls the engine into
`agent-eval` has destroyed the pack's iteration economics and must be rejected.

---

## CI ownership

`.github/workflows/` is out of scope in all 24 items here, and each item's `## 3. Scope` repeats
that entry. This pack builds gate *scripts*; it does not wire them.

**CI is owned by pack T5** (`docs/PROMPTS/packs/T5_robustness_and_cohesion.md`). `G7.34` adds the
`cargo test` gate, `G7.35` the client build and smoke gate, `G7.36` the clippy/rustfmt gate, and
`G7.43` the standing regression fleet that re-runs earlier phase gates. Those four are the only
items in the program licensed to touch a workflow file, they may only ADD a gate, and each declares
that override in its own `## 4. Approach constraints`.

So when an item here says its gate script's CI wiring is "not performed", the correct pointer is
`G7.43` in T5 — a named owner with a registry entry — not an unowned human decision. Hand off the
script and its literal pass condition; do not wire it.

---

## `headless.rs` ownership

`eustress/crates/engine/src/bin/headless.rs` is 291 lines of `MinimalPlugins` today and there is no
headless GPU render path in it. **Two** items in this pack may edit it, and the split is by region,
not by file:

- **`G7.03` owns the render tier.** Composition, the `--render` tiers, and everything downstream of
  them. Its archived evidence is invalidated by any change to that region, so every other item lists
  the file out of scope for that reason.
- **`G7.14` holds a narrow, conditional licence** — `--tick-rate`, and only if the sweep shows the
  flag alters the simulation path, and only to restore decoupling. That is a determinism fix in the
  argument handling, not a render-tier change.

No other item in this pack may edit the file. An item citing `G7.03` as a reason to leave the file
alone is stating why the *render tier* is frozen, not claiming `G7.03` is the file's only editor.

---

## How to use this file

Each section below is a complete, conforming prompt. Emit each one verbatim into the path given in
its heading, then delete nothing from this file — this pack file is the index and the dependency
authority.

---

## Cross-pack file ownership

Items in this pack share source files with items in other packs. `docs/PROMPTS/04_FILE_OWNERSHIP.md` names one owner per contested path and is normative; where it conflicts with an item's scope list, it wins. This pack's own dependency-graph table records its internal edges together with the program-gate edges of `02_QUEUE.md` §8.2 and §8.4; the cross-pack edges arising from contested paths are these:

| Item | Now depends on | Contested path | Effect on this item's scope |
|---|---|---|---|
| `G7.01` | `G1.01` (G1) | `eustress/Cargo.toml` | may append to but not alter it |
| `G7.02` | `G1.05` (G1) | `eustress/crates/mcp-server/src/bridge_tools.rs` | may append to but not alter it |
| `G7.03` | `G1.03` (G1) | `eustress/crates/engine/Cargo.toml` | may append to but not alter it |
| `G7.03` | `G1.05` (G1) | `eustress/crates/engine/src/ai_camera.rs` | may no longer edit it |
| `G7.03` | `G7.30` (T5) | `eustress/crates/engine/src/app_core.rs` | may no longer edit it |
| `G7.05` | `G1.05` (G1) | `eustress/crates/mcp-server/src/bridge_tools.rs` | may append to but not alter it |
| `G7.07` | `G1.05` (G1) | `eustress/crates/engine/src/engine_bridge/protocol.rs` | may append to but not alter it |
| `G7.09` | `G1.05` (G1) | `eustress/crates/engine/src/ai_camera.rs` | may no longer edit it |
| `G7.09` | `G1.05` (G1) | `eustress/crates/engine/src/engine_bridge/protocol.rs` | may append to but not alter it |
| `G7.10` | `G2.04` (T2) | `eustress/crates/common/src/simulation/watchpoint.rs` | may no longer edit it |
| `G7.13` | `G1.05` (G1) | `eustress/crates/engine/src/engine_bridge/protocol.rs` | may append to but not alter it |
| `G7.14` | `G2.02` (T2) | `eustress/crates/common/src/physics/determinism.rs` | may no longer edit it |
| `G7.15` | `G1.07` (G1) | `eustress/crates/common/src/simulation/clock.rs` | may no longer edit it |
| `G7.15` | `G1.07` (G1) | `eustress/crates/common/src/simulation/recorder.rs` | may no longer edit it |
| `G7.15` | `G1.07` (G1) | `eustress/crates/engine/src/simulation/plugin.rs` | may no longer edit it |
| `G7.16` | `G1.05` (G1) | `eustress/crates/engine/src/engine_bridge/protocol.rs` | may append to but not alter it |
| `G7.17` | `G1.05` (G1) | `eustress/crates/engine/src/engine_bridge/protocol.rs` | may append to but not alter it |
| `G7.18` | `G2.35` (B1) | `eustress/crates/tools/src/cad_tools.rs` | may append to but not alter it |

An item blocked by one of these entries emits a `FILE-OWNERSHIP` decision packet to L0 (`docs/PROMPTS/04_FILE_OWNERSHIP.md` §6). It does not edit the file and does not work around it.

---


---

## `docs/PROMPTS/items/G7.01_mcp-tool-conformance-census.md`

````markdown
---
id: G7.01
title: MCP tool-surface conformance census and reliability baseline
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G1.01]
blocks: [G7.02, G7.03, G7.11, G7.12, G7.15]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.01/tool_conformance.json
escalation: >
  If more than 20 of the defined tool descriptors cannot be invoked at all against a live
  eustress-headless process — because they require the windowed editor, a GPU, or a network
  service that does not exist — STALL rather than shrinking the census. The purpose of this item
  is to learn that number, not to hide it.
status: DRAFT
notes: >
  This is the pack's ITEM ZERO. critic_gate is empty because the exit criterion is fully
  mechanical: a count of tools invoked, and zero unclassified outcomes.
---

## 1. Objective

Every tool descriptor that Eustress advertises to an AI agent has been invoked at least twice
against a live headless engine — once with a minimal valid input and once with a required
parameter deliberately removed — and the outcome of each invocation is recorded in a single
machine-readable file with a typed outcome class and a measured latency. After this item, the
number of tools on the agent surface that fail silently is a known integer rather than an
anecdote.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — a world model an AI
reasons over and a document a human edits. It is never described as a game engine; 3D rendering
and the ECS are implementation details in service of that goal. The licence is PolyForm Shield
1.0.0, so the project is **source-available**, never open source. The physics engine is **Avian**,
never Rapier. Slint `.slint` files compile to Rust, so Slint *is* Rust. Units are meter-native;
studs are a display unit only.

**The agent surface, counted from source.** The frequently repeated figure of "~200 MCP tools" is
wrong. The verified counts are:

- **79 tool descriptors** implemented as `ToolHandler` impls under `eustress/crates/tools/src/`.
  By file: `simulation_tools.rs` 20, `universe_tools.rs` 14, `cad_tools.rs` 10, `git_tools.rs` 6,
  `script_tools.rs` 6, `entity_tools.rs` 5, `memory_tools.rs` 5, `embedvec_tools.rs` 4,
  `file_tools.rs` 3, `physics_tools.rs` 2, `spatial_tools.rs` 2, `shell_tools.rs` 1,
  `diff_tools.rs` 1.
- **24 bridge tools** in `eustress/crates/mcp-server/src/bridge_tools.rs`.
- **1 hand-rolled tool**, `set_active_universe`, in `eustress/crates/mcp-server/src/tools.rs` —
  the only tool that mutates MCP session state.

That is **104 defined**. After per-mode filtering (`eustress/crates/tools/src/modes.rs`,
`WorkshopMode`), **90 are exposed** in a default live session. Your census must cover all 104
defined descriptors, and must record for each whether it is exposed under the default mode set.

**The tool contract.** `eustress/crates/tools/src/registry.rs` defines `ToolDefinition` with
fields `name`, `description`, `input_schema` (a `serde_json::Value` JSON Schema), `modes`,
`requires_approval: bool`, and `stream_topics: &'static [&'static str]`. It defines `ToolResult`
with `success: bool`, `content: String`, `structured_data: Option<Value>`, and
`stream_topic: Option<String>`. `requires_approval` is documented in that file as a **hint** that
MCP clients may ignore — do not assume it blocks anything.

**The failure class you are measuring.** A known and recurring defect in this codebase: a tool or
UI action that is missing a required parameter is *skipped* rather than rejected, producing no
error and no observable effect. The agent sees an ordinary success-shaped response and proceeds on
a false premise. This is why the census invokes every tool twice — the second invocation, with a
required parameter removed, is the probe for that class.

**How to reach a live engine without a window.** `eustress/crates/engine/src/bin/headless.rs`
(291 lines) builds an app from `MinimalPlugins` + `ScheduleRunnerPlugin` plus
`app_core::add_core_sim_plugins`, loads a Space, and advertises the Engine Bridge. Usage:

```
eustress-headless --space <dir> [--ticks N] [--tick-rate HZ] [--no-autoplay]
                  [--autoplay-delay-frames N]
eustress-headless --universe <dir> ...
```

Run it with `--no-autoplay` for this item so the world stays in Edit state and your census is not
racing a running simulation.

**The bridge.** `eustress/crates/engine/src/engine_bridge/` exposes JSON-RPC 2.0 over TCP. The
verified method names present in `protocol.rs` are: `action.invoke`, `ai_camera.capture`,
`ai_camera.frame`, `ai_camera.orbit`, `ai_camera.png`, `ai_camera.set_pose`, `capture.png`,
`data.bind`, `data.bindings`, `data.unbind`, `ecs.inspect`, `ecs.query`, `entity.add_tag`,
`entity.create`, `entity.delete`, `entity.demote`, `entity.find`, `entity.promote`, `entity.read`,
`entity.remove_tag`, `entity.update`, `oplog.tail`, `scene.overview`, `scene.raycast`,
`selection.set`, `sim.bindings`, `sim.read`, `sim.step`, `state.get`, `tool.equip`, `tools.call`,
`tools.list`. **`tools.call` and `tools.list` are the two methods this item leans on** — they let
you drive the whole 104-descriptor surface through one socket.

**The client you must use.** `eustress/crates/bridge-client/` is the crate
`eustress-bridge-client`. Its public surface is exactly three functions:
`call_engine(universe_dir: &Path, method: &str, params: Value) -> Result<Value, String>`,
`call_engine_with_timeout(...)`, and `port_file_path(universe_dir: &Path) -> PathBuf`. Discovery
is via `<universe>/.eustress/engine.port`. The crate's only dependency is `serde_json` — it does
not link the engine.

**Where worlds live.** `eustress/crates/engine/src/space/mod.rs:119 workspace_root()` resolves in
this order: the `EUSTRESS_WORKSPACE` environment variable (explicit override, wins on every
platform and is the CI/container hook), then the platform Documents `Eustress/` directory, then
the current working directory. A Universe directory is one containing a `Spaces/` (or legacy
`spaces/`) subdirectory. **No Space is committed to this repository** — Universes are user data.
This item therefore creates its own fixture Space under `EUSTRESS_WORKSPACE` and commits the
*generator*, not the world.

**Build reality.** A full engine build takes 10–15 minutes. Only one cargo build may run at a
time — the workspace shares a single `target/` and concurrent builds produce link failures. Never
kill a build mid-compile. Validate with `cargo run`, not `cargo check`.

**The architectural constraint for this pack.** Build the census tool in a **new crate,
`eustress/crates/agent-eval/`, which must not depend on `eustress-engine`.** It reaches the engine
over the bridge through `eustress-bridge-client`. A harness that links the engine costs 10–15
minutes per iteration; one that speaks over a socket costs seconds. Add `"crates/agent-eval"` to
the `members` list in `eustress/Cargo.toml`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — the entire new crate, including `Cargo.toml`, `src/lib.rs`, and
  `src/bin/agent_census.rs` producing the binary `eustress-agent-census`
- `eustress/Cargo.toml` — the single line adding `"crates/agent-eval"` to `members`
- `scripts/agent_eval/` — fixture-Space generator scripts
- `docs/PROMPTS/artifacts/G7.01/` — the artifact directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/tools/src/` — this item **measures** the tool surface; it does not fix it.
  Fixing is `G7.02`. Editing a tool to make it pass the census is the exact failure this pack is
  designed to catch.
- `eustress/crates/mcp-server/src/` — same reason
- `eustress/crates/engine/` — no engine changes; if the census cannot reach a tool without an
  engine change, record that as an outcome class, do not make the change
- Any existing entry in `eustress/Cargo.toml` — the file is owned by `G1.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Dropping a tool from the census because it is awkward, skipping the missing-parameter probe for
  tools where it is inconvenient, widening an outcome class to absorb ambiguous results, or
  marking a timeout as a pass are all measurement changes. If the measurement is genuinely wrong,
  report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Do not fix any tool. A census that reports 0 defects because you fixed them on the way through
  destroys the baseline the entire pack is measured against.
- Every outcome must land in exactly one class. Use these six and no others:
  `ok` (structured success with an observable effect or a non-empty read),
  `structured_error` (a well-formed failure the agent can act on),
  `silent_noop` (success-shaped response with no observable effect and no error),
  `panic` (the engine process logged a panic or the connection dropped),
  `timeout` (no response inside 30 s),
  `unreachable` (the tool cannot be invoked headlessly at all — record *why*, in one sentence).
  There is no seventh class and no `unknown`.
- "Observable effect" must be defined mechanically, not by eyeball. For mutating tools, compare a
  world-state digest taken before and after — `ecs.query` over the affected class plus
  `oplog.tail` is sufficient. For read-only tools, a non-empty, schema-shaped payload is the
  effect. State your digest method in the artifact.
- Latency must be measured per invocation, wall-clock, at the `call_engine` boundary, with at
  least 5 repetitions per tool for the valid-input case. Report p50 and p95 in milliseconds.
- Batch your builds. The `agent-eval` crate compiles in seconds; the only expensive build is the
  one that produces `eustress-headless`, and you need it once.

## 5. Exit criterion

### Criterion
`tool_conformance.json` records **104 defined descriptors**, each with at least one valid-input
invocation and one missing-required-parameter invocation, **zero** entries in an outcome class
other than the six named above, and **zero** entries with a missing latency measurement.

### Measurement

Command:

```
# 1. Build the headless simulator (one engine build; ~10-15 min)
cargo build --release --package eustress-engine --bin eustress-headless

# 2. Create the fixture workspace and Space
$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_census_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE

# 3. Start the headless simulator, Edit state, no autoplay
Start-Process -FilePath .\eustress\target\release\eustress-headless.exe `
  -ArgumentList '--universe', "$env:EUSTRESS_WORKSPACE\CensusUniverse", '--no-autoplay'

# 4. Run the census
cargo run --release --package eustress-agent-eval --bin eustress-agent-census -- `
  --universe "$env:EUSTRESS_WORKSPACE\CensusUniverse" `
  --repetitions 5 `
  --timeout-s 30 `
  --out docs/PROMPTS/artifacts/G7.01/tool_conformance.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape (`tool_conformance.json`, abridged):

```json
{
  "schema_version": 1,
  "commit": "71ccf6fe",
  "captured_at": "2026-08-06T18:22:41Z",
  "host": { "os": "windows-11-26200", "cpu": "…", "logical_cores": 32 },
  "tools_defined": 104,
  "tools_exposed_default_modes": 90,
  "tools_invoked": 104,
  "unclassified": 0,
  "outcome_totals": {
    "ok": 71, "structured_error": 12, "silent_noop": 9,
    "panic": 0, "timeout": 2, "unreachable": 10
  },
  "digest_method": "ecs.query(class) + oplog.tail(32) → blake3",
  "tools": [
    {
      "name": "create_entity",
      "source": "eustress/crates/tools/src/entity_tools.rs:73",
      "exposed_default_modes": true,
      "requires_approval": false,
      "valid_input":   { "outcome": "ok", "latency_p50_ms": 6.1, "latency_p95_ms": 11.4,
                         "effect_observed": true },
      "missing_param": { "outcome": "structured_error", "removed_param": "class",
                         "latency_p50_ms": 1.9, "error_excerpt": "Missing required parameter: class" }
    }
  ]
}
```

Pass condition:

```
EXIT == 0
AND tools_invoked == tools_defined == 104
AND unclassified == 0
AND every entry in tools[] has both valid_input and missing_param objects
AND every valid_input has a numeric latency_p50_ms and latency_p95_ms
AND sum(outcome_totals) over both probes == 208
```

Verify by reading the emitted values from the JSON, not by observing that the file exists. The
census binary must exit non-zero if any descriptor was skipped or any outcome was unclassifiable —
grep `EXIT=` and require `0`.

## 6. Critic gate

`critic_gate` is `[]`. This item produces no perceptual artifact, so no blind Critic is engaged.
The mechanical criterion in §5 replaces it and is deliberately tight: three exact integer
equalities plus a per-entry completeness assertion. There is no partial credit — a census covering
103 of 104 descriptors fails.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — drive every descriptor through the bridge's `tools.call`
   -> if still failing, MANDATORY approach change. Retrying with a longer timeout is NOT an
      approach change; invoking the MCP server over stdio instead of the bridge is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with tools_invoked moving < 5% and unclassified > 0
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: more than 20 descriptors classify as `unreachable` (see front matter)
```

The stall packet must fit one screen and must request exactly one of: LOWER the coverage floor to a
stated descriptor count with the stated consequence for downstream items; FUND a specific approach
D; DEFER behind a named blocking item; or KILL with a statement of what the pack loses.

## 8. Artifact

`docs/PROMPTS/artifacts/G7.01/tool_conformance.json`

A reader finds: the commit, the host, the three descriptor counts, the outcome histogram across
both probe types, the digest method, and one object per descriptor giving its source `path:line`,
mode exposure, approval flag, both probe outcomes, and measured p50/p95 latency. Every later item
in pack T4 cites this file for its baseline; `G7.02` cites `outcome_totals.silent_noop` as the
number it must drive to zero, and `G7.04` cites the latency distribution.

Also archived alongside it: `docs/PROMPTS/artifacts/G7.01/census_stderr.log`, the engine's stderr
for the census run, which is the evidence for every `panic` classification.

## 9. Definition of NOT done

- The census covers the 90 exposed tools and skips the 14 that mode-filtering hides. The mandate is
  104 defined descriptors; a tool hidden by mode filtering is still a tool an agent can reach by
  switching modes.
- The missing-parameter probe is run only on tools that "looked risky". Every descriptor gets the
  probe, including read-only ones.
- `silent_noop` is reported as `0` because the effect check was "the call returned success".
  Success-shaped responses are precisely what this class detects; the digest comparison is
  mandatory.
- A tool times out, and the fix is raising `--timeout-s` until it passes. A timeout at 30 s is the
  finding, not an obstacle to it.
- The `agent-eval` crate ends up depending on `eustress-engine`, so every census iteration now
  costs a 10–15 minute build. This fails the item on architecture even if the JSON is perfect.
- A descriptor is quietly fixed in `eustress/crates/tools/src/` to move it out of `silent_noop`.
  That is out of scope, destroys the baseline, and fails the item outright.
- The artifact is produced but the binary exits 0 even when descriptors were skipped, so the exit
  code proves nothing. The exit code is part of the measurement.
````

---

## `docs/PROMPTS/items/G7.02_zero-silent-noops.md`

````markdown
---
id: G7.02
title: Zero silent no-ops on the agent tool surface
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.01, G1.05]
blocks: [G7.04, G7.05, G7.07, G7.13]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.02/tool_conformance_after.json
escalation: >
  If eliminating a silent no-op for any tool requires a change inside
  eustress/crates/engine/src/ui/slint_ui.rs, STALL immediately. That file is 23,103 lines and is
  the single largest structural liability in the repository; a drive-by edit there is not a fix,
  it is a new defect with a delayed fuse.
status: DRAFT
notes: >
  critic_gate is empty: the criterion is an integer from a re-run of the G7.01 census.
---

## 1. Objective

Every tool on the Eustress agent surface responds to a malformed or incomplete invocation with a
structured, machine-actionable error, and never with a success-shaped response that had no effect.
Re-running the `G7.01` census against the same fixture reports `silent_noop = 0` across both probe
types, with no reduction in the number of descriptors covered.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never described as a
game engine. Source-available under PolyForm Shield 1.0.0, never open source. Physics is **Avian**,
never Rapier. Slint compiles to Rust. Units are meter-native.

**The defect class, stated precisely.** When a required parameter is absent from a tool
invocation, or when a handler's precondition is unmet, the code path returns early without
producing an error the caller can see. The invocation looks successful. Downstream, the agent
believes a mutation happened that did not. This class has previously taken down the entire studio
UI click surface: a single missing required parameter caused every queued action in a drain system
to be discarded, and the only trace was a log line reading "failed validation". An agent has no
log to read — it has the tool result. That is why the fix must be at the result boundary.

**The tool contract.** `eustress/crates/tools/src/registry.rs` defines `ToolResult` with
`success: bool`, `content: String`, `structured_data: Option<serde_json::Value>`, and
`stream_topic: Option<String>`. Handlers implement `ToolHandler::execute(input, ctx) -> ToolResult`
and return the definition through `ToolHandler::definition()`, whose `input_schema` field is a
JSON Schema `serde_json::Value`. **The schema is already there for every tool.** The strongest
available fix is therefore central, not per-tool: validate `input` against `definition().input_schema`
at dispatch time and synthesise a structured error before the handler ever runs.

**An existing precedent to copy.** `eustress/crates/tools/src/simulation_tools.rs` — the
`run_experiment` handler at line 1567 — already returns
`ToolResult { success: false, content: "Missing required parameter: duration_s", .. }` when
`duration_s` is absent. That is the shape every tool must produce. It is hand-rolled per tool
today; the point of this item is to make it structural.

**Where the tools live.** 79 handlers under `eustress/crates/tools/src/` across `cad_tools.rs`,
`diff_tools.rs`, `embedvec_tools.rs`, `entity_tools.rs`, `file_tools.rs`, `git_tools.rs`,
`memory_tools.rs`, `physics_tools.rs`, `script_tools.rs`, `shell_tools.rs`, `simulation_tools.rs`,
`spatial_tools.rs`, `universe_tools.rs`. Dispatch and registration are in
`eustress/crates/tools/src/registry.rs` and `eustress/crates/tools/src/lib.rs`. 24 more live in
`eustress/crates/mcp-server/src/bridge_tools.rs`; 1 (`set_active_universe`) in
`eustress/crates/mcp-server/src/tools.rs`.

**Your baseline.** `docs/PROMPTS/artifacts/G7.01/tool_conformance.json`, produced by `G7.01`. Read
`outcome_totals.silent_noop` — that integer is what you must drive to zero. Read the per-tool
entries to know exactly which descriptors are implicated; do not re-derive the list.

**Build reality.** A change in `eustress/crates/tools/` triggers a rebuild of every dependent
crate including the engine: 10–15 minutes. One cargo build at a time. Never kill a build
mid-compile. Validate with `cargo run`, not `cargo check` — `cargo check` will not catch a
dispatch-path regression. Budget accordingly: this item's 12 builds are the real ceiling, so a
central validator that fixes many tools in one build is strongly preferred over 20 per-tool edits.

**The census harness.** `eustress/crates/agent-eval/`, binary `eustress-agent-census`, built by
`G7.01`. It does not link the engine and rebuilds in seconds. Re-run it unchanged.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/tools/src/registry.rs` — the dispatch path and any central input validator
- `eustress/crates/tools/src/lib.rs`
- Individual handler files under `eustress/crates/tools/src/` — only where a central validator
  provably cannot cover the case, and each such case must be justified in the artifact
- `eustress/crates/mcp-server/src/bridge_tools.rs`
- `eustress/crates/mcp-server/src/tools.rs`
- `eustress/crates/agent-eval/` — only to add the comparison mode described in §5

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G7.01/tool_conformance.json` — the baseline is frozen evidence
- `eustress/crates/engine/src/ui/slint_ui.rs` — see the escalation trigger
- `eustress/crates/engine/src/` generally — a tool that no-ops because an engine-side system
  skipped is a finding to record, not a licence to edit the engine here
- Any existing entry in `eustress/crates/mcp-server/src/bridge_tools.rs` — the file is owned by `G1.05`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Reclassifying `silent_noop` as `structured_error` in the census without changing behaviour,
  removing a tool from the census, loosening the effect digest, or making the missing-parameter
  probe remove an optional parameter instead of a required one are all measurement changes. If the
  measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Prefer one central schema-driven validator over per-handler edits. Every per-handler edit you
  make must be listed in the artifact with a one-line reason the central path could not cover it.
- A structured error must carry, at minimum: the tool name, the offending parameter name, and what
  was expected. `success: false` with `content: "error"` is not a structured error and does not
  clear this item.
- Do not make a tool "succeed" to remove a no-op. Turning a no-op into a silent partial mutation is
  strictly worse than the defect being fixed.
- Do not regress the `ok` count. A validator that rejects previously valid inputs has traded one
  defect for another; the exit criterion checks both directions.
- Batch your verification: one engine build should validate the entire central-validator change
  plus every handler edit you intend to make.

## 5. Exit criterion

### Criterion
A re-run of the `G7.01` census against the same fixture reports `silent_noop == 0`,
`tools_invoked == 104`, `unclassified == 0`, and an `ok` count **greater than or equal to** the
baseline `ok` count in `docs/PROMPTS/artifacts/G7.01/tool_conformance.json`.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_census_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE
Start-Process -FilePath .\eustress\target\release\eustress-headless.exe `
  -ArgumentList '--universe', "$env:EUSTRESS_WORKSPACE\CensusUniverse", '--no-autoplay'

cargo run --release --package eustress-agent-eval --bin eustress-agent-census -- `
  --universe "$env:EUSTRESS_WORKSPACE\CensusUniverse" `
  --repetitions 5 --timeout-s 30 `
  --out docs/PROMPTS/artifacts/G7.02/tool_conformance_after.json `
  --compare-baseline docs/PROMPTS/artifacts/G7.01/tool_conformance.json `
  --require-silent-noop 0 `
  --require-ok-not-below-baseline
echo "EXIT=$LASTEXITCODE"
```

Expected output shape (tail of stdout):

```
baseline  ok=71  structured_error=12  silent_noop=9   timeout=2  unreachable=10
after     ok=79  structured_error=13  silent_noop=0   timeout=2  unreachable=10
regressions: none
GATE: silent_noop == 0            PASS
GATE: ok >= baseline_ok           PASS
GATE: tools_invoked == 104        PASS
```

Pass condition:

```
EXIT == 0
AND after.outcome_totals.silent_noop == 0
AND after.outcome_totals.ok >= baseline.outcome_totals.ok
AND after.tools_invoked == 104
AND after.unclassified == 0
```

Read the emitted counters from `tool_conformance_after.json`; the `--require-*` flags must make the
binary exit non-zero when a gate fails, and the check is `grep EXIT=0`, never "the file appeared".

## 6. Critic gate

`critic_gate` is `[]`. This item has no perceptual surface. The replacement mechanical criterion is
unusually tight because it is bidirectional: it is not enough to drive one counter to zero, the
`ok` counter must not fall, so the trivially-passing degenerate fix (reject everything) is excluded
by construction.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — central JSON-Schema validation at dispatch in registry.rs
   -> if still failing, MANDATORY approach change. Adding a per-handler guard to one more tool is
      NOT an approach change; moving validation into the MCP server's request layer is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with silent_noop moving < 2 and ok moving < 2
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: any required fix lands inside engine/src/ui/slint_ui.rs (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.02/tool_conformance_after.json`

A reader finds the same schema as the `G7.01` baseline plus a `comparison` object giving both
histograms side by side, a `regressions` array (empty on pass), and a `manual_handler_edits` array
listing every per-handler change with its one-line justification. This file is the W3 evidence that
the agent surface fails loudly.

## 9. Definition of NOT done

- `silent_noop` reaches 0 because the census now classifies those tools as `unreachable`. That is
  reclassification, not repair, and the regression check on `ok` will not catch it — state the
  `unreachable` count explicitly and it must not rise above the baseline either.
- The central validator rejects inputs that previously worked, so `ok` falls. Both directions are
  gated.
- Structured errors are produced but carry no parameter name, so an agent cannot self-correct. The
  error must name the offending parameter.
- The fix is 20 hand-written guards, one per tool, with no central path — so the 21st tool added
  next month reintroduces the class. The artifact must justify every manual edit.
- A tool now returns `success: true` with a partial mutation instead of no mutation. Worse than the
  original defect.
- Verification is done with `cargo check` and a dispatch-path regression ships. Validate with
  `cargo run`.
````

---

## `docs/PROMPTS/items/G7.03_headless-render-tier.md`

````markdown
---
id: G7.03
title: Headless render tier — the agent sees without a desktop session
workload: W3
workload_secondary: [W1, W6]
phase: G7
depends_on: [G7.01, G1.03, G1.05, G7.30]
blocks: [G7.09, G7.20, G7.23, G2.03, G7.31, G7.32, G7.39, G7.41]
tier: XL
token_envelope: 1200000
wallclock_envelope: 5d
max_builds: 20
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.03/headless_capture_report.json
escalation: >
  If the windowless render path requires a GPU adapter that a headless container cannot provide
  and no software rasterizer fallback initialises, STALL rather than declaring the tier done on a
  developer workstation with a discrete GPU. The entire point of the tier is that it runs where
  there is no display.
status: DRAFT
notes: >
  XL because this is the P6 tier of docs/architecture/HEADLESS_RUNTIME.md, currently status `new`,
  and it touches plugin composition in the engine — every iteration is a full engine build.
  critic_gate is empty here deliberately: this item proves a frame can be produced at all.
  Whether the frame is any good is G7.09 and the render packs.
---

## 1. Objective

`eustress-headless` gains a `--render gpu` tier that composes a windowless render pipeline, so
`ai_camera.capture` and `viewport.capture` return real pixels from a process that never creates a
window. A capture taken from that process is a valid, non-degenerate image of the loaded Space, and
the process exits 0.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0, never open source. Physics is **Avian**. Slint
compiles to Rust. Units are meter-native.

**Why this item is the pack's spine.** The agent loop is `observe → act → judge`. Today the
*observe* half cannot run without a desktop session. `eustress/crates/engine/src/bin/headless.rs`
line 25 states it plainly in its own module docs: `ai_camera` / `viewport.capture` "need the future
`--render gpu` tier". `docs/architecture/HEADLESS_RUNTIME.md` §9 lists **P6 — `--render gpu` tier
(windowless `DefaultPlugins`) for `ai_camera` capture** with status `new`, i.e. not started. Until
this exists, the loop cannot run in a container, in CI, or on any cloud box — which is exactly
where a training substrate must run.

**What headless is today.** `headless.rs` is 291 lines. It builds `App::new()` with
`bevy::log::LogPlugin`, `MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(...))`,
`bevy::transform::TransformPlugin`, `bevy::diagnostic::DiagnosticsPlugin`,
`bevy::state::app::StatesPlugin`, `bevy::input::InputPlugin`, `AssetPlugin` (with
`unapproved_path_mode: UnapprovedPathMode::Allow`), and `bevy::scene::ScenePlugin`. It then
`init_asset::<T>()`s the render-asset *containers* without the render pipeline —
`Mesh`, `StandardMaterial`, `Image`, `AnimationClip`, `AnimationGraph`,
`bevy::audio::AudioSource`, and `bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>` — with
an in-source comment recording that omitting the decal container caused the **entire disk spawn
path to silently skip** on first bring-up. That comment is your warning: in Bevy 0.19, a system
whose parameters fail validation is skipped without an error. Read it before you touch plugin
composition.

It finally calls `app_core::add_core_sim_plugins(&mut app, &args.space)` — the shared, headless-safe
simulation tier that the windowed editor also composes — and adds a `headless_run_driver` system
implementing `Editing → Playing → tick target → Editing → drain → AppExit`.

**The capture surfaces that must start working.** `eustress/crates/engine/src/ai_camera.rs`
implements an off-screen camera using `RenderTarget::Image` with GPU readback to PNG, exposed over
the bridge as `ai_camera.set_pose`, `ai_camera.orbit`, `ai_camera.frame`, `ai_camera.capture`,
`ai_camera.png`, and as `viewport.capture` / `capture.png`. The method names are present in
`eustress/crates/engine/src/engine_bridge/protocol.rs`. Today they require the windowed engine.

**The other frame path, for context only.** `eustress/crates/client/src/systems/frame_capture.rs`
does a PNG burst via `bevy::render::view::screenshot`, armed by `EUSTRESS_CAPTURE=<count>[@<every_n>]`
with output directory `EUSTRESS_CAPTURE_DIR`, a 90-frame arm delay, and an F9 hotkey. It is client
only and frame-indexed against wall clock. Do not build on it — it is not tick-indexed and it is
in the wrong binary.

**What is deliberately not in this item.** `eustress/crates/engine/src/photoreal.rs` records that
GTAO, TAA, bloom, and auto-exposure are on hold because the relevant Bevy post-process crates have
not published stable releases against the pinned engine version; only filmic tonemapping ships. Do
not try to improve image quality here. `eustress/crates/engine/src/rendering/instanced_pbr.rs`
exists but is registered by no plugin anywhere — leave it alone. This item proves a frame can be
produced headlessly; frame *quality* belongs to the render packs.

**Known headless limitation to respect.** `headless.rs` line 22 records that no glTF loader is
registered, so `.glb`-backed custom meshes do not decode. Bare parts, physics, scripts, sim values,
and the bridge all work. Your fixture Space must therefore be built from primitives, not imported
meshes, or you will be debugging an asset gap and calling it a render gap.

**Where worlds live.** `eustress/crates/engine/src/space/mod.rs:119 workspace_root()` honours the
`EUSTRESS_WORKSPACE` environment variable first, on every platform. That is the hook a container
uses.

**Build reality.** Full engine build: 10–15 minutes. One at a time. Never kill mid-compile —
doing so has produced `LNK2001` link failures and Windows SAC `os error 4551` in this repository,
recoverable only with `cargo clean -p`. Validate with `cargo run`, not `cargo check`; a plugin
composition error is exactly the class `cargo check` cannot see.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/bin/headless.rs`
- `eustress/crates/engine/Cargo.toml` — only if a feature flag is genuinely required; say why
- `eustress/crates/agent-eval/` — the verification binary described in §5
- `docs/architecture/HEADLESS_RUNTIME.md` — update the P6 row's status when, and only when, the
  gate in §5 has passed

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/photoreal.rs` — the post-stack is on hold and is not this item
- `eustress/crates/engine/src/rendering/instanced_pbr.rs` — unregistered, out of scope
- `eustress/crates/client/src/systems/frame_capture.rs` — the wrong binary and the wrong index
- `eustress/crates/engine/src/main.rs` — the windowed shell must not change behaviour
- `eustress/crates/engine/src/ui/` — the studio UI tier has no place in a headless render path
- `eustress/crates/engine/src/ai_camera.rs` — owned by `G1.05` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- Any existing entry in `eustress/crates/engine/Cargo.toml` — the file is owned by `G1.03`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.
- `eustress/crates/engine/src/app_core.rs` — owned by `G7.30` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Lowering the capture resolution to make readback succeed, replacing the non-degenerate-image
  check with a file-size check, capturing a solid-colour clear and calling it a frame, or falling
  back to the windowed engine for the measurement are all measurement changes. If the measurement
  is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The default tier must remain unchanged. `eustress-headless --space X --ticks N` with no
  `--render` flag must still compose `MinimalPlugins` and must still work on a box with no GPU. A
  change that makes the existing tier require an adapter fails this item.
- No window may be created in the `--render gpu` tier. Prove it, do not assert it: the process must
  emit a single log line naming the composed tier and the adapter backend, and the artifact must
  record that no `winit` window plugin was added. If a windowing plugin is present but hidden, that
  is not headless.
- A software-rasterizer fallback must be attempted before failing when no hardware adapter is
  present, and the artifact must record which adapter was actually used. A tier that only works on
  a workstation with a discrete GPU does not satisfy the objective.
- Batch your builds ruthlessly. Twenty builds is roughly four hours of pure compile and is the
  whole envelope for three approaches. One build should validate plugin composition, the capture
  path, and the CLI flag together.

## 5. Exit criterion

### Criterion
`eustress-headless --render gpu` loads a primitive-only fixture Space, runs 300 sim ticks, captures
a 1280×720 PNG through `ai_camera.capture`, and exits 0 — with the captured image
**non-degenerate**, defined as: at least **3 distinct luminance clusters** occupying at least
**5%** of pixels each, and per-channel standard deviation **> 8.0** on an 8-bit scale.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_render_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE

cargo run --release --package eustress-agent-eval --bin eustress-headless-capture-check -- `
  --headless-bin .\eustress\target\release\eustress-headless.exe `
  --universe "$env:EUSTRESS_WORKSPACE\RenderUniverse" `
  --render gpu `
  --ticks 300 `
  --width 1280 --height 720 `
  --out-png docs/PROMPTS/artifacts/G7.03/frame_t300.png `
  --out docs/PROMPTS/artifacts/G7.03/headless_capture_report.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape (`headless_capture_report.json`):

```json
{
  "schema_version": 1,
  "commit": "…",
  "headless_exit_code": 0,
  "window_created": false,
  "composed_tier": "render-gpu",
  "adapter": { "backend": "Vulkan", "name": "llvmpipe (LLVM 18.1.0, 256 bits)", "software": true },
  "ticks_run": 300,
  "capture": {
    "path": "docs/PROMPTS/artifacts/G7.03/frame_t300.png",
    "width": 1280, "height": 720,
    "luminance_clusters_ge_5pct": 5,
    "stddev_r": 41.2, "stddev_g": 38.7, "stddev_b": 35.9,
    "all_pixels_identical": false
  },
  "default_tier_regression_check": { "ran": true, "exit_code": 0, "window_created": false }
}
```

Pass condition:

```
EXIT == 0
AND headless_exit_code == 0
AND window_created == false
AND ticks_run == 300
AND capture.luminance_clusters_ge_5pct >= 3
AND min(stddev_r, stddev_g, stddev_b) > 8.0
AND default_tier_regression_check.exit_code == 0
```

The check binary must compute the image statistics itself and exit non-zero when any gate fails.
Read the emitted numbers; do not infer success from the PNG existing.

## 6. Critic gate

`critic_gate` is `[]` and this is deliberate. The question here is binary — can a frame be produced
with no desktop session, yes or no — and it is answered by image statistics, not by taste. Frame
*quality* is judged elsewhere: `G7.09` gates capture determinism, and the render packs own D2.
Passing this item with an ugly frame is a pass; passing it with a beautiful frame produced by the
windowed engine is a fail.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — compose bevy DefaultPlugins minus WinitPlugin, keep the existing
                 add_core_sim_plugins tier underneath
   -> if still failing, MANDATORY approach change. Adding one more init_asset call is NOT an
      approach change; driving the render app from a manually constructed RenderPlugin with an
      explicit adapter request is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the process still fails to produce any PNG
  - Budget      : 1.8M tokens or 30 builds consumed (150% of the XL envelope)
  - Item-specific: no adapter initialises without a display, and no software rasterizer fallback
                   is available (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.03/headless_capture_report.json`, with
`docs/PROMPTS/artifacts/G7.03/frame_t300.png` beside it.

A reader finds: the commit, the composed tier name, whether a window was created, the graphics
adapter actually used and whether it was a software rasterizer, the tick count, the full image
statistics, and the result of the regression check proving the default `MinimalPlugins` tier still
runs unchanged on a box with no adapter. This is the W3 evidence that the observe half of the agent
loop no longer needs a desktop.

## 9. Definition of NOT done

- The capture works on the development workstation with a discrete GPU and fails on a machine with
  no adapter. The artifact must name the adapter, and the tier must have a software fallback.
- A window is created and immediately hidden. Hidden is not headless; a container has no window
  server to hide it in.
- The default tier now requires a GPU, so `eustress-headless --space X --ticks N` breaks on the
  boxes where it currently works. The regression check exists precisely to catch this.
- The PNG is produced but is a uniform clear colour, because the camera is inside geometry or no
  light exists. `all_pixels_identical` and the cluster count are the guards; a uniform frame is not
  an observation.
- `.glb`-backed meshes are used in the fixture, the loader is absent headlessly, and hours are
  spent debugging an asset gap as though it were a render gap. Build the fixture from primitives.
- A system silently skips because a render asset container was not initialised, and the failure
  presents as "the scene is empty" rather than as an error. The existing `ForwardDecalMaterial`
  comment in `headless.rs` documents this exact trap.
- The P6 row in `HEADLESS_RUNTIME.md` is marked done before the gate passes.
````

---

## `docs/PROMPTS/items/G7.04_agent-loop-latency-baseline.md`

````markdown
---
id: G7.04
title: One-iteration agent-loop latency baseline
workload: W6
workload_secondary: [W3]
phase: G7
depends_on: [G7.02]
blocks: [G7.10]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.04/loop_latency.json
escalation: >
  If a single full loop iteration exceeds 120 s at p50 on the reference fixture, STALL and report
  it rather than proceeding to optimise. A two-minute loop iteration is an architecture finding,
  not a tuning target, and downstream items in this pack must be re-planned around it.
status: DRAFT
notes: >
  A baseline item: it establishes numbers, it does not beat them. The threshold in the exit
  criterion is a completeness threshold, not a performance one.
---

## 1. Objective

The wall-clock cost of one complete agent-loop iteration — observe, act, simulate, observe again,
judge — is measured over at least 30 iterations against a fixed fixture, broken down per stage with
p50 and p95, and the dominant stage is named. After this item, "the loop is slow" is replaced by a
number attached to a stage.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Units are meter-native.

**The loop being measured.** Five stages, in order, one iteration:

| Stage | What it does | Bridge / MCP surface |
|---|---|---|
| `observe_pre` | Read world state before acting | `scene.overview`, `ecs.query`, `sim.read` |
| `act` | Apply one mutation | `entity.create` or `entity.update`, or `tools.call` |
| `simulate` | Advance the world a fixed number of ticks | `sim.step` |
| `observe_post` | Read world state after | `scene.overview`, `ecs.query`, `sim.read` |
| `judge` | Compute the delta the agent would reason over | client-side diff of the two observations |

**The surfaces, verified.** `eustress/crates/engine/src/engine_bridge/protocol.rs` implements the
JSON-RPC methods `scene.overview`, `ecs.query`, `ecs.inspect`, `entity.create`, `entity.read`,
`entity.update`, `entity.delete`, `entity.find`, `sim.read`, `sim.step`, `sim.bindings`,
`oplog.tail`, `scene.raycast`, `state.get`, `tools.call`, `tools.list`, and the `ai_camera.*`
family. `eustress/crates/bridge-client/` (crate `eustress-bridge-client`) exposes exactly
`call_engine`, `call_engine_with_timeout`, and `port_file_path`; discovery is via
`<universe>/.eustress/engine.port`.

**Where the harness lives.** `eustress/crates/agent-eval/`, created by `G7.01`. It must not depend
on `eustress-engine` — it reaches the engine over the socket, so it rebuilds in seconds instead of
10–15 minutes. Preserve that.

**Your inputs.** `docs/PROMPTS/artifacts/G7.01/tool_conformance.json` already carries p50/p95 for
every individual tool call. This item measures the *composition*, which is not the sum: `sim.step`
dominates in a way single-call latency does not reveal, and the two observation stages may be
cache-warm on the second call.

**What "the world" is here.** `eustress-headless --universe <dir>` loads a Space and advertises the
bridge. Run it with `--no-autoplay` so that ticks advance only when you ask for them via
`sim.step` — otherwise the simulate stage is measuring free-running wall clock rather than the cost
of a commanded advance.

**Build reality.** No engine change is needed for this item, so no 10–15 minute build should be
required beyond reusing the `eustress-headless` binary already built for `G7.01`/`G7.02`. Validate
with `cargo run`, not `cargo check`.

**A caution about the clock.** `eustress/crates/common/src/simulation/clock.rs:83 advance()`
advances `simulation_time_s` by the full compressed delta but caps executed physics ticks at
`max_ticks_per_frame` (default 10), zeroing the accumulator on saturation
(`clock.rs:100-102`). At `time_scale = 1.0` — which is what this item must use — that cap is not
reached and the measurement is clean. **Do not raise `time_scale` to make the simulate stage look
fast.** That is measuring dropped work. `G7.15` deals with that defect directly.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — including `src/bin/loop_bench.rs` producing `eustress-loop-bench`
- `scripts/agent_eval/` — the fixture generator
- `docs/PROMPTS/artifacts/G7.04/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/` — this item measures, it does not optimise
- `eustress/crates/tools/`, `eustress/crates/mcp-server/` — same reason
- `docs/PROMPTS/artifacts/G7.01/`, `docs/PROMPTS/artifacts/G7.02/` — frozen baselines

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Raising `time_scale` so the simulate stage reports less wall time, reducing the tick count per
  iteration below the stated value, dropping the observation stages, warming a cache and reporting
  only warm iterations, or discarding outliers are all measurement changes. If the measurement is
  genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Do not optimise anything. This item's entire value is an honest before-number. An item that
  reports a fast loop because it changed the loop has produced no baseline.
- Report the **first three iterations separately** as cold-start, and exclude them from p50/p95
  only if you also report them. Hiding cold-start is a measurement change; labelling it is honest.
- Fix `time_scale = 1.0`, `sim.step` ticks = 60 per iteration, and iterations ≥ 30. State all three
  in the artifact. Any deviation must be justified in the artifact and reported alongside the
  standard configuration, not instead of it.
- Latency is wall clock at the `call_engine` boundary in the harness process, in milliseconds,
  measured with a monotonic clock.

## 5. Exit criterion

### Criterion
`loop_latency.json` contains **at least 30 complete loop iterations** against the reference
fixture, with p50 and p95 in milliseconds for **all five stages** plus the end-to-end total, a named
`dominant_stage`, and a separately reported cold-start block for iterations 1–3.

### Measurement

Command:

```
$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_loop_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE
Start-Process -FilePath .\eustress\target\release\eustress-headless.exe `
  -ArgumentList '--universe', "$env:EUSTRESS_WORKSPACE\LoopUniverse", '--no-autoplay'

cargo run --release --package eustress-agent-eval --bin eustress-loop-bench -- `
  --universe "$env:EUSTRESS_WORKSPACE\LoopUniverse" `
  --iterations 30 `
  --ticks-per-iteration 60 `
  --time-scale 1.0 `
  --out docs/PROMPTS/artifacts/G7.04/loop_latency.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "host": { "os": "windows-11-26200", "cpu": "…", "logical_cores": 32 },
  "config": { "iterations": 30, "ticks_per_iteration": 60, "time_scale": 1.0 },
  "cold_start_iterations_1_3_ms": [ 1840.2, 402.7, 388.1 ],
  "stages_ms": {
    "observe_pre":  { "p50": 14.2, "p95": 27.9 },
    "act":          { "p50":  8.6, "p95": 19.3 },
    "simulate":     { "p50": 1013.4, "p95": 1190.7 },
    "observe_post": { "p50": 15.1, "p95": 30.2 },
    "judge":        { "p50":  1.1, "p95":  2.4 }
  },
  "end_to_end_ms": { "p50": 1052.4, "p95": 1268.0 },
  "dominant_stage": "simulate",
  "dominant_stage_share_p50": 0.963,
  "iterations_recorded": 30
}
```

Pass condition:

```
EXIT == 0
AND iterations_recorded >= 30
AND every one of the five stages has numeric p50 and p95
AND end_to_end_ms.p50 is present and numeric
AND dominant_stage is one of the five stage names
AND cold_start_iterations_1_3_ms has exactly 3 entries
AND config.time_scale == 1.0 AND config.ticks_per_iteration == 60
```

The bench binary must exit non-zero if fewer than 30 iterations completed or any stage timing is
missing. Read the values; the file existing proves nothing.

## 6. Critic gate

`critic_gate` is `[]`. This is a measurement item with no perceptual output. The mechanical
criterion is a completeness contract: five stages, both percentiles, 30 iterations, cold start
reported, configuration pinned. There is no performance threshold — inventing one before the
baseline exists would be the exact error this item prevents.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — drive all five stages through eustress-bridge-client
   -> if still failing, MANDATORY approach change. Increasing the iteration count is NOT an
      approach change; driving the loop through the MCP server over stdio instead of the bridge is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations that fail to complete 30 loop iterations
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: end_to_end_ms.p50 > 120000 (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.04/loop_latency.json`

A reader finds: the commit, the host CPU and core count, the pinned configuration, the cold-start
block, per-stage p50/p95 for all five stages, the end-to-end distribution, and the dominant stage
with its share of p50. `G7.10` cites this file to budget the detect-and-propose stage;
`G7.21` cites it to sanity-check episodes-per-hour against per-iteration cost.

## 9. Definition of NOT done

- The loop is fast because `time_scale` was raised. That measures ticks the clock reported and the
  physics never executed — see `clock.rs:100-102` and `G7.15`.
- Cold-start iterations are silently folded into p50, so the first-call cost of a Space load is
  smeared across the distribution instead of being visible.
- Only the total is reported. Without the stage breakdown there is nothing for a later item to act
  on, and `dominant_stage` cannot be derived.
- 30 iterations were attempted, 26 completed, and the report presents 26 as the result. The binary
  must exit non-zero.
- The harness is made to link `eustress-engine` "for convenience", so every future iteration of this
  measurement costs a 10–15 minute build.
- Something was optimised along the way and the numbers are post-fix. There is then no baseline,
  and every downstream claim of improvement is unfalsifiable.
````

---

## `docs/PROMPTS/items/G7.05_transactional-agent-mutations.md`

````markdown
---
id: G7.05
title: Every agent mutation is transactional and undoable
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.02, G1.05]
blocks: [G7.06]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.05/mutation_transactionality.json
escalation: >
  If any mutating tool cannot be made undoable without a change inside
  eustress/crates/engine/src/ui/slint_ui.rs, STALL. That file is 23,103 lines and every panel's
  drain logic funnels through it; a mutation-semantics change made there is untestable.
status: DRAFT
notes: >
  critic_gate is empty: the criterion is a byte-comparison of world-state digests across an
  apply/undo cycle, repeated for every mutating tool.
---

## 1. Objective

For every tool on the agent surface that mutates world state, applying it and then undoing it
returns the world to a byte-identical digest, and the mutation appears exactly once in the durable
op-log with the actor attributed to the agent. A tool that cannot satisfy both is either fixed or
explicitly and visibly declared non-transactional in the artifact.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native.

**Why this matters more here than in an editor.** In `fork → rehearse → commit`, the *rehearse*
step is only safe if it can be unwound with no residue. In episode-based evaluation, `reset` is
only meaningful if the previous episode left nothing behind. Both depend on the same property, and
that property is untested today.

**The mutating surface.** From the `G7.01` census and the tool sources, the mutating tools are:
`create_entity`, `update_entity`, `delete_entity`, `promote_entity`, `demote_entity`,
`select_entity`, `add_tag`, `remove_tag`, `set_sim_value`, `run_simulation`, `pause_simulation`,
`stop_simulation`, `sim_step`, `execute_rune`, `execute_luau`, `create_script`, `equip_tool`,
`invoke_action`, `write_file`, `stage_file_change`, `run_bash`, `git_commit`, `git_branch`,
`new_space`, `new_universe`, `rename_space`, `rename_universe`, `datastore_set`, and the `cad_*`
family (`cad_create_part`, `cad_set_variable`, `cad_add_feature`, `cad_edit_feature`,
`cad_delete_feature`, `cad_export_glb`). **Derive the authoritative list from
`docs/PROMPTS/artifacts/G7.01/tool_conformance.json`** — every entry whose `valid_input.effect_observed`
is `true` and whose effect included an op-log append. Do not hand-maintain the list.

Three of these are legitimately outside world-state transactionality and must be *declared*, not
forced: `run_bash`, `git_commit`, and `git_branch` mutate the filesystem and repository, not the
world. Declare them explicitly in the artifact with that reason. Everything else must be undoable.

**The undo machinery that exists.** `eustress/crates/engine/src/undo.rs` provides `UndoStack`, a
Bevy resource. It is consumed widely — `align_distribute.rs:119`, `array_tools.rs:222`,
`cad_plugin.rs:434`, `clipboard.rs:954`, and others. `eustress/crates/engine/src/app_core.rs:146`
registers undo before the Slint UI tier "which reads `UndoStack`", and line 148 records that every
`UndoStack` push is teed onto a `history.<kind>` stream topic. Critically for this item: `app_core`
is the **shared headless-safe tier**, so `UndoStack` exists in `eustress-headless` too. Note the
existing hazard recorded at `array_tools.rs:266` — a code path that warns "no `UndoStack` resource —
{total} spawned part(s) cannot be undone" and proceeds anyway. That warning pattern is the failure
mode this item eliminates for the agent surface.

**The durable op-log that exists.** `eustress/crates/worlddb/src/mutations.rs` (214 lines) defines
`MutationOp` (Create/Update/Delete), `MutationActor`, and `MutationRecord` with fields
`{ tx_id, ts_nanos, actor, op, class_name, uuid, rel_path, before, after, parent_tx, reason }`,
plus `encode_mutation` / `decode_mutation` and a `MutationView`.
`docs/architecture/CAUSAL_OPLOG_WIRING.md` documents the storage half as **done**: a `mutations`
partition, `WorldDb::record_mutation(&[u8]) -> Result<u64>` which assigns its own monotonic op-log
sequence and persists the high-water mark in `meta:mutation_seq`, and `iter_mutations(min_seq, max_seq)`.
It also records the architectural finding you must respect: **record at the semantic caller sites,
never from `apply_commit`'s generic loop and never from `mirror_transform_changes` /
`mirror_binary_ecs_changes`** — because per-frame transform mirroring flows through `apply_commit`
at a 2048-ops-per-frame budget and a naive hook there produces 100×–1000× op-log bloat.

**The read surface.** `oplog.tail` over the bridge; `oplog_tail` as an MCP tool.

**World-state digest.** Define it once and use it everywhere in this pack: a BLAKE3 hash over the
canonical serialisation of every entity core in the Space's `world.fjalldb`, ordered by UUID.
`eustress/crates/eustress-space/` already opens `world.fjalldb` through the `worlddb` crate with the
engine never linked — that is the template for computing the digest from `agent-eval` without a
10–15 minute build. Its `verify` subcommand already walks every core with rkyv `CheckBytes`.

**Build reality.** 10–15 minutes for an engine build. One at a time. Never kill mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/tools/src/` — mutation handlers, to push undo entries and record mutations
- `eustress/crates/mcp-server/src/bridge_tools.rs`
- `eustress/crates/engine/src/space/active_db.rs` — the binary-ECS semantic caller site named in
  `CAUSAL_OPLOG_WIRING.md`
- `eustress/crates/engine/src/undo.rs` — only to add an agent-attributable push API
- `eustress/crates/agent-eval/` — the verification binary
- `docs/PROMPTS/artifacts/G7.05/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/ui/slint_ui.rs` — see the escalation trigger
- `eustress/crates/engine/src/space/world_db_plugin.rs` `mirror_transform_changes` — explicitly
  forbidden as a recording site by `CAUSAL_OPLOG_WIRING.md`; hooking it produces op-log bloat
- `docs/PROMPTS/artifacts/G7.01/`, `docs/PROMPTS/artifacts/G7.02/` — frozen baselines
- Any existing entry in `eustress/crates/mcp-server/src/bridge_tools.rs` — the file is owned by `G1.05`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Excluding a tool from the transactionality set because it is hard, computing the digest over a
  subset of components, comparing digests with a tolerance, or declaring a tool "non-transactional"
  without the stated filesystem/repository reason are all measurement changes. If the measurement is
  genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The digest must cover the whole Space, not the entities you expect to have changed. A mutation
  with an unexpected side effect elsewhere is exactly what this item exists to find.
- Exactly one op-log record per semantic mutation. Zero records fails; two records fails. Op-log
  bloat is a failure mode with its own gate in §5.
- Do not record from `apply_commit` or the transform mirror. `CAUSAL_OPLOG_WIRING.md` documents,
  with adversarial verification, why that produces up to 2048 records per idle frame.
- `MutationRecord.actor` must distinguish an agent-originated mutation from a human one. If the
  existing `MutationActor` enum cannot express that, extending it is in scope; inventing a parallel
  mechanism is not.
- Batch your builds. Twelve is the ceiling for three approaches; one build should validate the undo
  push, the record site, and the actor attribution together.

## 5. Exit criterion

### Criterion
For **every** mutating tool identified from the `G7.01` census except the three declared
filesystem/repository tools, an apply-then-undo cycle restores the world-state digest **exactly**,
and the apply produces **exactly one** op-log record attributed to the agent actor. Aggregate:
`undo_exact == mutating_tool_count` and `oplog_record_count_wrong == 0`.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_mutation_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE
Start-Process -FilePath .\eustress\target\release\eustress-headless.exe `
  -ArgumentList '--universe', "$env:EUSTRESS_WORKSPACE\MutationUniverse", '--no-autoplay'

cargo run --release --package eustress-agent-eval --bin eustress-mutation-check -- `
  --universe "$env:EUSTRESS_WORKSPACE\MutationUniverse" `
  --tool-set-from docs/PROMPTS/artifacts/G7.01/tool_conformance.json `
  --trials-per-tool 3 `
  --out docs/PROMPTS/artifacts/G7.05/mutation_transactionality.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "digest_algorithm": "blake3 over rkyv entity cores, ordered by uuid",
  "mutating_tool_count": 31,
  "declared_non_world_tools": ["run_bash", "git_commit", "git_branch"],
  "undo_exact": 31,
  "undo_inexact": 0,
  "oplog_record_count_wrong": 0,
  "oplog_bloat_max_records_per_mutation": 1,
  "actor_attribution_correct": 31,
  "tools": [
    {
      "name": "create_entity",
      "trials": 3,
      "digest_before": "9f2c…", "digest_after_apply": "41ab…", "digest_after_undo": "9f2c…",
      "undo_exact": true,
      "oplog_records_appended": 1,
      "oplog_actor": "Agent",
      "oplog_op": "Create"
    }
  ]
}
```

Pass condition:

```
EXIT == 0
AND undo_exact == mutating_tool_count
AND undo_inexact == 0
AND oplog_record_count_wrong == 0
AND oplog_bloat_max_records_per_mutation == 1
AND actor_attribution_correct == mutating_tool_count
AND declared_non_world_tools has exactly 3 entries, each with a stated reason
```

The check binary must exit non-zero when any tool's post-undo digest differs from its pre-apply
digest. Read the counters; the file existing proves nothing.

## 6. Critic gate

`critic_gate` is `[]`. The criterion is a hash equality repeated across every mutating tool and
three trials each — stronger evidence than any perceptual judgement could provide, and not
subject to interpretation. The replacement is deliberately multi-clause so a fix that restores the
digest by disabling the mutation cannot pass: `digest_after_apply` must differ from
`digest_before`, which the per-tool rows expose.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — push an UndoStack entry plus a MutationRecord at each semantic
                 tool handler site
   -> if still failing, MANDATORY approach change. Adding the same guard to one more handler is
      NOT an approach change; wrapping tool dispatch in a snapshot/restore transaction is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with undo_exact moving < 2
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a required fix lands in engine/src/ui/slint_ui.rs (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.05/mutation_transactionality.json`

A reader finds: the digest algorithm, the derived mutating-tool count, the three declared
non-world tools with reasons, the four aggregate counters, and one row per tool giving three
digests, the undo verdict, the op-log record count, the recorded actor, and the recorded op. This
is the W3 evidence that a rehearsal can be unwound and that what the agent did is attributable.

## 9. Definition of NOT done

- `undo_exact` reaches the target because the tool set was narrowed to the easy tools. The set is
  derived from the `G7.01` census, not curated.
- The digest matches after undo because the mutation never happened — `digest_after_apply` equals
  `digest_before`. The per-tool rows expose this; a no-op is not an undo.
- The op-log now carries a record for every mirrored transform, and the `mutations` partition grows
  by thousands of rows per idle minute. `CAUSAL_OPLOG_WIRING.md` documents exactly this trap; the
  bloat gate exists to catch it.
- Undo works for the agent path and silently breaks the human path in the studio, because the undo
  entry shape changed. Both paths share `UndoStack`.
- A tool is declared non-transactional with no reason, or with a reason other than "mutates the
  filesystem or the repository, not the world". The declaration list is capped at three and each
  needs its reason.
- Actor attribution is added but every mutation records the same actor, so agent and human work are
  indistinguishable in the op-log — which defeats the purpose in `G7.06`.
````

---

## `docs/PROMPTS/items/G7.06_oplog-replay-reconstructs-world.md`

````markdown
---
id: G7.06
title: Op-log replay reconstructs the world exactly
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.05]
blocks: [G7.08, G7.10, G7.17]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.06/oplog_replay.json
escalation: >
  If replay requires reading engine-internal state that is not present in MutationRecord — that
  is, if the op-log is structurally incapable of describing the mutation — STALL and report which
  field is missing. Widening MutationRecord is a schema decision with migration consequences and
  is a human call, not an agent call.
status: DRAFT
notes: >
  This is the observability keystone of Half A. If the op-log cannot rebuild the world, then
  "what did the agent do" has no answer that survives the process exiting.
---

## 1. Objective

Replaying a Space's durable op-log from sequence 0 onto an empty Space produces a world whose
state digest is byte-identical to the digest of the recorded Space. The op-log is therefore a
complete, sufficient account of every mutation, and "what did the agent do to this world" has an
answer that outlives the process.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native.

**Why replay, and not "the log looks complete".** An append-only log that omits one mutation class
looks perfectly healthy in a tail. The only test that finds the gap is reconstruction. This is also
the property that makes the loop auditable to an outside party: a lab or a buyer can be handed a
log and a starting Space and can derive the ending Space themselves, without trusting the running
process.

**The op-log, verified.** `eustress/crates/worlddb/src/mutations.rs` (214 lines) defines:

- `MutationOp` — `Create` / `Update` / `Delete`
- `MutationActor`
- `MutationRecord { tx_id, ts_nanos, actor, op, class_name, uuid, rel_path, before, after,
   parent_tx, reason }`
- `encode_mutation(&MutationRecord) -> Result<Vec<u8>>` and `decode_mutation(&[u8])`
- `MutationView` with `MutationView::from_record(seq, &record)`

`docs/architecture/CAUSAL_OPLOG_WIRING.md` records the storage half as **done**: a `mutations`
partition, `WorldDb::record_mutation(&[u8]) -> Result<u64>` assigning its own monotonic sequence
and persisting the high-water mark at `meta:mutation_seq`, `iter_mutations(min_seq, max_seq)`, and
key encoding in `eustress/crates/worlddb/src/keys.rs` — `encode_mutation_key(seq)` /
`decode_mutation_key`, tag `'U'`, big-endian sequence, so an **ascending range scan is the replay
order**. That last fact is the one you build on: replay is a forward scan, not a graph walk.

**The causality fields.** `parent_tx` and `reason` exist on every record. Together with `actor`
they are what turns a flat log into a causal account: which mutation caused which, and why the
agent claims it did it. This item must prove they are populated, not merely present in the struct.

**Producer wiring already done by `G7.05`.** Every mutating tool now appends exactly one record,
attributed to the agent actor, with the world-state digest restored on undo. Read
`docs/PROMPTS/artifacts/G7.05/mutation_transactionality.json` for the tool set and the digest
algorithm; reuse the same digest definition — a BLAKE3 hash over the canonical serialisation of
every authored entity core in `world.fjalldb`, ordered by UUID.

**The engine-free path.** `eustress/crates/eustress-space/` opens `world.fjalldb` through the
`worlddb` crate with the engine never linked; it has `open`, `verify` (rkyv `CheckBytes` over every
core, non-zero exit on failure), and `export`. That crate is your template: the replay tool belongs
in `eustress/crates/agent-eval/` depending on `eustress-worlddb`, not on `eustress-engine`. This
keeps iteration at seconds instead of 10–15 minutes.

**What replay does not have to reproduce.** Physics state that is derived rather than authored —
velocities mid-flight, contact manifolds, solver caches. Define the digest over **authored entity
cores** only, exactly as `G7.05` did, and state that scope in the artifact. A replay that
reproduces authored state exactly is the claim; reproducing a solver's internal cache is not.

**Build reality.** No engine build should be required if the replay tool is engine-free. If you
find yourself rebuilding the engine to iterate on replay, the architecture is wrong. Validate with
`cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — including `src/bin/oplog_replay.rs`
- `eustress/crates/worlddb/src/mutations.rs` — only to add a replay-apply helper, and only if the
  helper is genuinely shared; extending `MutationRecord`'s fields is an escalation, not an edit
- `eustress/crates/engine/src/space/active_db.rs` — only to populate `parent_tx` / `reason` at
  existing semantic recording sites
- `docs/PROMPTS/artifacts/G7.06/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G7.01/`, `G7.02/`, `G7.05/` — frozen baselines
- `eustress/crates/engine/src/space/world_db_plugin.rs` `mirror_transform_changes` — forbidden as
  a recording site by `CAUSAL_OPLOG_WIRING.md`
- `eustress/crates/engine/src/ui/` — no UI tier involvement in op-log replay

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Narrowing the digest to the entity classes that happen to replay correctly, seeding the replay
  target with a partial copy of the source Space instead of an empty one, replaying onto the
  source Space in place, or comparing digests with a tolerance are all measurement changes. If the
  measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Replay must start from a genuinely empty Space, created fresh, and must apply records in
  ascending sequence order with no reordering and no skipping.
- If a record cannot be applied, replay must fail loudly with the sequence number and the reason.
  A replay that skips unapplicable records and still reports success has proven nothing.
- `parent_tx` and `reason` must be non-default on agent-originated records. An item that achieves
  digest equality with every `reason` empty has produced a log that reconstructs state but not
  causality, and fails the second gate in §5.
- Do not widen `MutationRecord`. If a field is genuinely missing, that is the escalation trigger.

## 5. Exit criterion

### Criterion
Over a workload of **at least 200 recorded agent mutations** spanning at least **8 distinct
mutating tools**, replaying the op-log from sequence 0 onto a fresh empty Space yields a
world-state digest **exactly equal** to the source Space's digest, with **zero skipped records**;
and **at least 95%** of agent-originated records carry a non-empty `reason` and a resolvable
`parent_tx` (or an explicit root marker).

### Measurement

Command:

```
$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_mutation_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE
Start-Process -FilePath .\eustress\target\release\eustress-headless.exe `
  -ArgumentList '--universe', "$env:EUSTRESS_WORKSPACE\ReplayUniverse", '--no-autoplay'

# 1. Generate the workload: 200+ mutations across 8+ tools
cargo run --release --package eustress-agent-eval --bin eustress-mutation-workload -- `
  --universe "$env:EUSTRESS_WORKSPACE\ReplayUniverse" `
  --mutations 250 --min-distinct-tools 8 --seed 20260806

# 2. Replay onto a fresh empty Space and compare digests (engine never linked)
cargo run --release --package eustress-agent-eval --bin eustress-oplog-replay -- `
  --source-space "$env:EUSTRESS_WORKSPACE\ReplayUniverse\Spaces\Main" `
  --target-space "$env:TEMP\replay_target" `
  --from-seq 0 `
  --fail-on-skip `
  --out docs/PROMPTS/artifacts/G7.06/oplog_replay.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "digest_algorithm": "blake3 over rkyv entity cores, ordered by uuid",
  "digest_scope": "authored entity cores only; derived physics state excluded",
  "records_total": 253,
  "records_applied": 253,
  "records_skipped": 0,
  "distinct_tools_in_workload": 11,
  "source_digest": "c81f4a…",
  "replay_digest": "c81f4a…",
  "digest_match": true,
  "causality": {
    "agent_records": 250,
    "with_reason": 250,
    "with_resolvable_parent_or_root": 249,
    "causality_coverage": 0.998
  },
  "first_unapplicable_seq": null
}
```

Pass condition:

```
EXIT == 0
AND digest_match == true
AND records_skipped == 0
AND records_applied == records_total
AND records_total >= 200
AND distinct_tools_in_workload >= 8
AND causality.causality_coverage >= 0.95
```

The replay binary must exit non-zero on any digest mismatch or any skipped record. Read the two
digests and compare them in the output; do not accept "replay completed" as evidence.

## 6. Critic gate

`critic_gate` is `[]`. A hash equality between an independently reconstructed world and the
recorded one is stronger and less arguable than any perceptual judgement. The criterion carries a
second, independent clause — causality coverage — so a log that reconstructs state while recording
no reasons cannot pass.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — forward scan of the mutations partition, applying before/after
                 payloads through the worlddb core API
   -> if still failing, MANDATORY approach change. Adding a special case for one more class is
      NOT an approach change; replaying through the same semantic creation path the tools use is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with records_applied moving < 5% and digest_match false
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: MutationRecord structurally cannot describe a mutation (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.06/oplog_replay.json`

A reader finds: the digest algorithm and its declared scope, the record counts, the number of
distinct tools exercised, both digests, the match verdict, the causality coverage breakdown, and
the sequence number of the first unapplicable record if any. `G7.08` cites this file to justify
that a rehearsal can be replayed rather than merely discarded; `G7.17` builds episode replay on
the same guarantee.

## 9. Definition of NOT done

- Digest equality is achieved by replaying onto a copy of the source Space rather than an empty
  one. That tests nothing.
- Records that fail to apply are skipped and the run still reports success. `--fail-on-skip` is
  mandatory and `records_skipped` must be 0.
- The workload uses one tool 250 times. Eight distinct tools is the floor precisely because a
  single-class log hides every gap in the other classes.
- The digest scope is quietly narrowed mid-item to exclude whichever class does not replay. The
  scope is fixed by `G7.05` and stated in the artifact; narrowing it is a measurement change.
- Causality coverage is met by writing a constant string into every `reason`. A reason that is
  identical across 250 records carries no information; the artifact must show reason diversity, and
  a Critic reviewing this file will look for it.
- The replay tool is made to depend on `eustress-engine`, so every iteration costs 10–15 minutes
  and the tool can never run as a standalone audit for a third party.
````

---

## `docs/PROMPTS/items/G7.07_human-gate-enforced.md`

````markdown
---
id: G7.07
title: The human gate is enforced, not hinted
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.02, G1.05]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.07/approval_gate.json
escalation: >
  If enforcing the gate requires the MCP server to hold interactive state across process
  restarts — that is, if the only workable design is a persistent session store — STALL and
  report it. Persistent approval state has security consequences that are a human decision.
status: DRAFT
notes: >
  Small but load-bearing: without an enforced gate, "under a human gate" is a claim the sales
  surface cannot make honestly.
---

## 1. Objective

When strict gating is enabled, every tool declaring `requires_approval` is refused with a
structured, actionable error unless the invocation carries a valid approval token, and every tool
that does not declare it is unaffected. Approval enforcement is a property of the substrate, not a
convention the client is trusted to honour.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native.

**The current state, stated honestly.** `eustress/crates/tools/src/registry.rs` defines
`ToolDefinition.requires_approval: bool` and documents it in-source as a **hint**: "MCP clients
interpret this as a hint — external IDEs may always require approval." Nothing in the engine
refuses an unapproved call. **17 tool descriptors** across `eustress/crates/tools/src/` declare
`requires_approval: true` — among them `run_experiment`
(`eustress/crates/tools/src/simulation_tools.rs:1567`, which can create a git branch, overwrite
sim values, and run the simulation unattended). Derive the exact list from the source at execution
time rather than trusting this count; the census in
`docs/PROMPTS/artifacts/G7.01/tool_conformance.json` records `requires_approval` per descriptor and
is the authoritative input.

**Why this is a substrate property and not a client convention.** The agent loop's value
proposition — an autonomous build/run/detect/propose cycle that a human supervises rather than
babysits — is only truthful if the supervision point cannot be bypassed by a client that chooses
not to implement it. A hint honoured by one client and ignored by another is not a gate.

**The surfaces that must enforce.** Both entry points, or the gate is a door with a window next to
it:

1. The MCP server (`eustress/crates/mcp-server/src/main.rs`, 819 lines, plus
   `shared_registry.rs`), which serves `tools/list` and `tools/call` over stdio.
2. The Engine Bridge's `tools.call` method
   (`eustress/crates/engine/src/engine_bridge/protocol.rs`), which reaches the same registry over
   TCP.

**The refusal must be actionable.** A gated refusal must tell the caller: the tool name, that
approval is required, and how an approval is supplied. An agent that receives an opaque failure
will retry the same call in a loop.

**Configuration.** Use an environment variable, consistent with the rest of the repository's knobs
(`EUSTRESS_WORKSPACE`, `EUSTRESS_PROFILE`, `EUSTRESS_CAPTURE`, `EUSTRESS_PHASE_WATCHDOG_SECS`).
Name it `EUSTRESS_AGENT_GATE`, with values `off` (default, current behaviour preserved) and
`strict`. Preserving the default is mandatory: a change that breaks the existing interactive
workflow fails this item.

**Build reality.** A change in `eustress/crates/tools/` or the MCP server rebuilds dependents;
budget 10–15 minutes per engine build, one at a time, never killed mid-compile. Validate with
`cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/tools/src/registry.rs` — the dispatch-time gate check
- `eustress/crates/mcp-server/src/main.rs`, `shared_registry.rs`, `tools.rs`
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — only the `tools.call` arm
- `eustress/crates/agent-eval/` — the verification binary
- `docs/PROMPTS/artifacts/G7.07/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- The `requires_approval` value on any existing tool. Changing which tools are gated is a policy
  decision; this item enforces the existing policy, it does not rewrite it.
- `eustress/crates/engine/src/ui/slint_ui.rs` — the studio's own approval UX is a separate concern
- `docs/PROMPTS/artifacts/G7.01/`, `G7.02/` — frozen baselines
- Any existing entry in `eustress/crates/engine/src/engine_bridge/protocol.rs` — the file is owned by `G1.05`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Reducing the gated set so fewer tools must be blocked, testing only the MCP path and not the
  bridge path, or accepting any non-empty string as a valid token are all measurement changes. If
  the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Both entry points must enforce. A gate on the MCP server alone is bypassed by any caller that
  opens the TCP socket, and the socket's location is written to a well-known file
  (`<universe>/.eustress/engine.port`).
- Default behaviour must be byte-for-byte unchanged with `EUSTRESS_AGENT_GATE` unset. Prove it: the
  verification must include an ungated control run whose outcome histogram matches `G7.02`'s.
- An invalid or expired token must be refused with the same structured shape as a missing token,
  and must be distinguishable in the error content. Silent acceptance of a malformed token is the
  worst possible outcome and is explicitly gated in §5.
- Do not build a persistent approval store. Approval is per-invocation for the purposes of this
  item. Anything longer-lived is the escalation trigger.

## 5. Exit criterion

### Criterion
With `EUSTRESS_AGENT_GATE=strict`, **100%** of tools declaring `requires_approval: true` are
refused without a token and **100%** succeed with a valid token, over **both** the MCP stdio path
and the bridge `tools.call` path; **0%** of non-gated tools are affected; and **100%** of
invocations bearing a malformed token are refused.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless
cargo build --release --package eustress-mcp-server

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_census_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE
$env:EUSTRESS_AGENT_GATE = "strict"
Start-Process -FilePath .\eustress\target\release\eustress-headless.exe `
  -ArgumentList '--universe', "$env:EUSTRESS_WORKSPACE\CensusUniverse", '--no-autoplay'

cargo run --release --package eustress-agent-eval --bin eustress-gate-check -- `
  --universe "$env:EUSTRESS_WORKSPACE\CensusUniverse" `
  --mcp-server-bin .\eustress\target\release\eustress-mcp-server.exe `
  --tool-set-from docs/PROMPTS/artifacts/G7.01/tool_conformance.json `
  --also-run-ungated-control `
  --out docs/PROMPTS/artifacts/G7.07/approval_gate.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "gated_tool_count": 17,
  "ungated_tool_count": 87,
  "paths_tested": ["mcp_stdio", "bridge_tools_call"],
  "results": {
    "mcp_stdio":         { "gated_refused_without_token": 17, "gated_allowed_with_token": 17,
                           "malformed_token_refused": 17, "ungated_affected": 0 },
    "bridge_tools_call": { "gated_refused_without_token": 17, "gated_allowed_with_token": 17,
                           "malformed_token_refused": 17, "ungated_affected": 0 }
  },
  "refusal_error_shape_ok": true,
  "refusal_example": "run_experiment requires approval. Supply approval_token; none was provided.",
  "ungated_control": { "gate_env": "off", "outcome_totals_match_g7_02": true }
}
```

Pass condition:

```
EXIT == 0
AND for EACH path in {mcp_stdio, bridge_tools_call}:
      gated_refused_without_token == gated_tool_count
  AND gated_allowed_with_token    == gated_tool_count
  AND malformed_token_refused     == gated_tool_count
  AND ungated_affected            == 0
AND refusal_error_shape_ok == true
AND ungated_control.outcome_totals_match_g7_02 == true
```

Read the counters. The gate-check binary must exit non-zero if any gated tool executed without a
token on either path.

## 6. Critic gate

`critic_gate` is `[]`. The criterion is a set of exact equalities across two independent transport
paths plus a preserved-default control, which leaves no room for interpretation. There is no
perceptual surface; a Critic would add nothing a counter does not already prove.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — enforce at dispatch inside registry.rs so both transports inherit it
   -> if still failing, MANDATORY approach change. Adding the check to one more call site is NOT
      an approach change; moving enforcement into a request-middleware layer per transport is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where either transport still executes a gated tool
                  without a token
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the only workable design needs persistent cross-restart approval state
                   (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.07/approval_gate.json`

A reader finds: the gated and ungated tool counts derived from source, per-transport results for
all four gate conditions, the refusal error shape with a real example string, and the ungated
control confirming default behaviour is unchanged against the `G7.02` baseline. This is the W3
evidence behind any claim that the agent loop runs "under a human gate".

## 9. Definition of NOT done

- Only the MCP path enforces. The bridge port is written to a well-known file; an unenforced socket
  is not a gate.
- A malformed token is accepted because the check is "token is present". Presence is not validity,
  and this is separately gated.
- Non-gated tools start failing under `strict`, so enabling the gate breaks ordinary work and
  nobody enables it. `ungated_affected` must be 0.
- Default behaviour changed, so the existing interactive workflow regressed. The ungated control
  run against the `G7.02` histogram exists to catch exactly this.
- The refusal is `success: false, content: "denied"`. An agent cannot act on that; it will retry
  forever. The refusal must name the tool and say what is required.
- A tool's `requires_approval` flag is flipped to shrink the gated set. That is policy editing and
  is out of scope.
````

---

## `docs/PROMPTS/items/G7.08_fork-rehearse-commit.md`

````markdown
---
id: G7.08
title: Fork, rehearse, commit — with zero residue
workload: W6
workload_secondary: [W3]
phase: G7
depends_on: [G7.06]
blocks: [G0.11, G5.22, G7.37, G7.38]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.08/fork_rehearse_commit.json
escalation: >
  If a fork of a Space with more than 100,000 entities cannot be created in under 60 s wall clock,
  STALL and report the measured cost rather than proceeding. Fork cost sets the floor on episode
  reset cost in Half B, and a slow fork silently caps the entire training substrate's throughput.
status: DRAFT
notes: >
  This is the mechanism that makes autonomous experimentation safe: the agent rehearses on a fork
  and the human sees only what survived.
---

## 1. Objective

An agent can fork a Space, apply an arbitrary mutation sequence to the fork, measure the outcome,
and then either discard the fork — leaving the original's state digest byte-identical to its
pre-fork value — or commit it, producing a state digest identical to applying the same sequence
directly to the original. Both paths are verified over randomised trials, and the cost of each is
measured.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native.

**Why fork–rehearse–commit is the whole point of the loop.** `build → run → detect → propose →
re-run` is only usable if the "re-run" can happen without a human first cleaning up the previous
attempt. Rehearsal on a fork is what lets an agent try ten candidate interventions and present the
human with one. It is also the substrate primitive that Half B's `env.reset` is built on: an
episode boundary is a discarded fork.

**What exists to build on.**

- **Storage.** Each Space carries `world.fjalldb`, a Fjall LSM key-value store behind the
  `WorldDb` trait (`eustress/crates/worlddb/`). `eustress/crates/eustress-space/` opens it with the
  engine never linked and has `open`, `verify` (rkyv `CheckBytes` per core, non-zero exit on
  failure), and `export`.
- **The op-log.** `eustress/crates/worlddb/src/mutations.rs` plus the `mutations` partition and
  `iter_mutations(min_seq, max_seq)`; ascending sequence scan is replay order
  (`eustress/crates/worlddb/src/keys.rs`, tag `'U'`, big-endian).
- **Verified replay.** `G7.06` proved that replaying the op-log from sequence 0 onto an empty
  Space reproduces the source digest exactly. That gives you a second, independent implementation
  of "commit": replay the fork's tail of records onto the original.
- **Transactional mutations.** `G7.05` proved every agent mutation is undoable and appears exactly
  once in the op-log.

**A hard fact about Spaces and version control.** Committing a Space to git does **not** capture
edits — `world.fjalldb` is gitignored. Any fork design that assumes `git` is the fork mechanism for
world state is wrong. `run_experiment`
(`eustress/crates/tools/src/simulation_tools.rs:1567`) does optionally run
`git checkout -b exp/<name>-<timestamp>`, but that branches *source*, not the world. Do not confuse
the two; say which one you mean everywhere in the artifact.

**Digest definition.** Reuse `G7.05`'s exactly: BLAKE3 over the canonical serialisation of every
authored entity core in `world.fjalldb`, ordered by UUID; derived physics state excluded.

**Scale context.** `docs/AUDIT/05_SPACE_STREAMING.md:23` quotes a 2.10M-entity figure that is an
`active_cap` **config default**, not a measurement — do not cite it as one. The fork-cost curve in
this item must be measured at entity counts you actually create, and each must be labelled
MEASURED with the command and host.

**Build reality.** If the fork tool lives in `eustress/crates/agent-eval/` over `eustress-worlddb`,
no engine build is required to iterate. Keep it that way. Validate with `cargo run`, not
`cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — including `src/bin/fork_rehearse.rs`
- `eustress/crates/worlddb/src/` — only if a fork/snapshot primitive is genuinely missing and must
  be added at the store layer; justify it in the artifact
- `eustress/crates/tools/src/simulation_tools.rs` — only to expose fork/discard/commit as agent
  tools once the mechanism is proven
- `docs/PROMPTS/artifacts/G7.08/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G7.01/`, `G7.02/`, `G7.05/`, `G7.06/` — frozen baselines
- `eustress/crates/engine/src/ui/` — forking is a substrate operation, not a UI feature
- `.gitignore` — do not "fix" the gitignored world store; that is a storage-policy decision

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Reducing the trial count, narrowing the mutation sequences to ones known to fork cleanly,
  comparing digests with a tolerance, or measuring fork cost only on a trivially small Space are
  all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Mutation sequences must be randomised from a seeded generator, and the seed must be recorded so
  a failing trial is reproducible. Hand-picked sequences prove nothing about arbitrary agent
  behaviour.
- Discard must be verified against the original's digest **and** against its op-log high-water
  mark. A discard that leaves orphan records in the `mutations` partition has residue even if the
  entity digest matches.
- Commit must be verified by equivalence, not by assertion: apply sequence S to a fork and commit,
  versus apply S directly to a copy of the original. Both digests must match.
- Report fork, discard, and commit wall-clock cost at three entity scales you actually create.
  Label every number MEASURED with the host and command.
- Do not use git for world state. See §2.

## 5. Exit criterion

### Criterion
Over **20 randomised trials**, discarding a fork restores the original's digest exactly in
**20/20** cases with **zero** orphan op-log records; and over **20 further randomised trials**,
committing a fork produces a digest equal to direct application in **20/20** cases. Fork, discard,
and commit wall-clock costs are reported at three measured entity scales.

### Measurement

Command:

```
$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_fork_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE `
  -EntityScales 1000,20000,200000
Start-Process -FilePath .\eustress\target\release\eustress-headless.exe `
  -ArgumentList '--universe', "$env:EUSTRESS_WORKSPACE\ForkUniverse", '--no-autoplay'

cargo run --release --package eustress-agent-eval --bin eustress-fork-rehearse -- `
  --universe "$env:EUSTRESS_WORKSPACE\ForkUniverse" `
  --discard-trials 20 --commit-trials 20 `
  --mutations-per-trial 25 `
  --seed 20260806 `
  --cost-scales 1000,20000,200000 `
  --out docs/PROMPTS/artifacts/G7.08/fork_rehearse_commit.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "host": { "os": "windows-11-26200", "cpu": "…", "logical_cores": 32 },
  "seed": 20260806,
  "digest_algorithm": "blake3 over rkyv entity cores, ordered by uuid",
  "discard": { "trials": 20, "digest_restored_exactly": 20,
               "orphan_oplog_records_max": 0, "failures": [] },
  "commit":  { "trials": 20, "digest_equals_direct_application": 20, "failures": [] },
  "cost_ms_measured": {
    "1000":   { "fork_p50": 41.2,   "discard_p50": 8.9,   "commit_p50": 63.7 },
    "20000":  { "fork_p50": 512.6,  "discard_p50": 27.4,  "commit_p50": 940.1 },
    "200000": { "fork_p50": 5188.0, "discard_p50": 210.3, "commit_p50": 9701.4 }
  },
  "world_state_fork_mechanism": "world.fjalldb snapshot copy (NOT git)",
  "git_used_for_world_state": false
}
```

Pass condition:

```
EXIT == 0
AND discard.digest_restored_exactly == discard.trials == 20
AND discard.orphan_oplog_records_max == 0
AND commit.digest_equals_direct_application == commit.trials == 20
AND cost_ms_measured has exactly 3 scales, each with fork/discard/commit p50
AND git_used_for_world_state == false
```

The binary must exit non-zero on any digest mismatch in either direction. Read the trial counters.

## 6. Critic gate

`critic_gate` is `[]`. Two independent 20/20 hash-equality results plus an orphan-record check
constitute stronger evidence than a perceptual score. The cost table is reported but not gated —
the escalation trigger in the front matter, not the Critic, guards against an unusably slow fork.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — snapshot-copy world.fjalldb for the fork; discard = delete the copy;
                 commit = replay the fork's op-log tail onto the original
   -> if still failing, MANDATORY approach change. Copying more directories is NOT an approach
      change; a copy-on-write overlay at the WorldDb trait level is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with discard.digest_restored_exactly moving < 2
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: fork of a >100k-entity Space exceeds 60 s (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.08/fork_rehearse_commit.json`

A reader finds: the seed, the digest algorithm, both 20-trial results with any failures itemised,
the orphan-record maximum, the measured cost table at three entity scales with the host that
produced it, and an explicit statement that git was not used for world state. `G7.13` cites the
discard cost as the floor on `env.reset`; `G7.21` cites it in the episodes-per-hour budget.

## 9. Definition of NOT done

- Discard restores the entity digest but leaves records in the `mutations` partition, so the next
  op-log replay reproduces a world that includes the rehearsal. Orphan records are residue.
- The trials use hand-picked mutation sequences that avoid the awkward classes. Randomised and
  seeded, or it proves nothing about an agent's actual behaviour.
- Fork is implemented with `git`, so world state — which is gitignored — is not actually forked and
  the whole mechanism silently no-ops on the thing that matters.
- Commit is verified by asserting the fork's records were applied, rather than by comparing against
  direct application. Equivalence is the claim; applying records is only the mechanism.
- Fork cost is measured on a 50-entity Space and presented as the cost curve. Three real scales,
  each labelled MEASURED with its host.
- The cost numbers are quoted as evidence of scale using the 2.10M-entity figure from
  `docs/AUDIT/05_SPACE_STREAMING.md:23`. That figure is a config default, not a measurement.
````

---

## `docs/PROMPTS/items/G7.09_agent-camera-deterministic-captures.md`

````markdown
---
id: G7.09
title: Independent agent camera with deterministic captures
workload: W3
workload_secondary: [W1]
phase: G7
depends_on: [G7.03, G1.05, G1.12]
blocks: [G7.23]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D6]
capture_recipe: docs/PROMPTS/harness/recipes/G7_agent_camera.json
artifact: docs/PROMPTS/artifacts/G7.09/agent_camera_determinism.json
escalation: >
  If two captures at the same pose and the same tick differ in even one byte and the cause is
  traced to non-determinism inside the graphics driver rather than inside Eustress, STALL and
  report it with the adapter name. A driver-level non-determinism finding changes what the whole
  program can promise about visual reproducibility.
status: DRAFT
notes: >
  Depends on G7.03 for the headless render tier. Gated on D6 only: the question here is whether
  the agent's viewpoint is a coherent, independent instrument, not whether the image is beautiful.
---

## 1. Objective

The agent camera is a first-class, independently posed observation instrument: it can be placed,
oriented, and framed over the bridge without disturbing any human viewport, and two captures taken
from the same pose at the same simulation tick are byte-identical, while a pose change produces a
demonstrably different image.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native: camera positions in this item are expressed in meters, and studs never appear in
the artifact.

**Why byte-identical matters.** An agent that judges its own work from a rendered frame needs the
frame to be a function of world state and pose, and of nothing else. If two captures of an
unchanged world differ, then every visual judgement in the loop carries an unquantified noise
floor, and `G7.23`'s measured baseline scores become unreproducible.

**What exists.** `eustress/crates/engine/src/ai_camera.rs` implements an off-screen camera using
`RenderTarget::Image` with GPU readback to PNG. It is exposed over the Engine Bridge as
`ai_camera.set_pose`, `ai_camera.orbit`, `ai_camera.frame`, `ai_camera.capture`, `ai_camera.png`,
and as `viewport.capture` / `capture.png`; the method names are present in
`eustress/crates/engine/src/engine_bridge/protocol.rs`. The MCP tool names are
`ai_camera_set_pose`, `ai_camera_orbit`, `ai_camera_frame`, `ai_camera_capture`, and
`capture_viewport`.

**A recorded defect to verify is gone.** Project history records that the AI camera has previously
ended up pointed off-screen — posed such that captures contained nothing of the scene. Framing must
therefore be verified by image content, not by the pose call returning success.

**What `G7.03` delivered and you depend on.** `eustress-headless --render gpu` composes a windowless
render pipeline; `docs/PROMPTS/artifacts/G7.03/headless_capture_report.json` records the adapter
that was used and whether it was a software rasterizer. Read that file: the adapter identity is an
input to this item's determinism claim and must be carried into this item's artifact.

**Fixture constraint inherited from `G7.03`.** `eustress/crates/engine/src/bin/headless.rs` line 22
records that no glTF loader is registered headlessly, so `.glb`-backed custom meshes do not decode.
Build the fixture Space from primitives.

**What is out of scope by construction.** `eustress/crates/engine/src/photoreal.rs` records that
GTAO, TAA, bloom, and auto-exposure are on hold; only filmic tonemapping ships. Do not attempt to
improve image quality. Note the direct relevance: **temporal anti-aliasing, were it enabled, would
make byte-identical capture impossible** — its absence is why this criterion is achievable today,
and the artifact must state which frame-to-frame accumulating effects are active (expected: none).

**The tick index.** Captures must be tick-indexed, not wall-clock indexed. Drive the world with
`sim.step` to an exact tick, then capture. The client-side burst capture in
`eustress/crates/client/src/systems/frame_capture.rs` is frame-indexed against wall clock and is
the wrong instrument here.

**Build reality.** Engine builds are 10–15 minutes, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — only the `ai_camera.*` arms
- `eustress/crates/agent-eval/` — the verification binary
- `docs/PROMPTS/harness/recipes/G7_agent_camera.json` — create it if the G1 capture harness has
  not yet shipped a recipe for this scene; the recipe format is defined in
  `docs/PROMPTS/02_CAPTURE_HARNESS.md`
- `docs/PROMPTS/artifacts/G7.09/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/photoreal.rs` — the post-stack is on hold and is not this item
- `eustress/crates/client/src/systems/frame_capture.rs` — wrong binary, wrong index
- `eustress/crates/engine/src/bin/headless.rs` — `G7.03` owns its render tier and `G7.14` holds a
  narrow `--tick-rate` licence; this item owns neither region, and changing the render tier here
  invalidates `G7.03`'s archived evidence
- `docs/PROMPTS/artifacts/G7.03/` — frozen baseline
- `eustress/crates/engine/src/ai_camera.rs` — owned by `G1.05` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- Any existing entry in `eustress/crates/engine/src/engine_bridge/protocol.rs` — the file is owned by `G1.05`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Comparing captures with a perceptual-difference tolerance instead of byte equality, reducing the
  capture resolution until noise disappears, capturing before the render graph has settled and
  calling the resulting blank frames identical, or dropping the pose-change control are all
  measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Byte equality means byte equality of the decoded pixel buffer, not of the PNG file. PNG encoders
  may embed timestamps; compare pixels.
- The pose-change control is mandatory and is the guard against the degenerate pass. Two identical
  black frames are byte-identical and prove nothing; a pose change must produce a frame whose mean
  absolute pixel difference from the reference exceeds a stated threshold.
- Independence must be demonstrated, not asserted: with a human viewport present, moving the agent
  camera must not move the human viewport, and vice versa. Verify by reading editor state
  (`state.get`) before and after.
- All camera positions and distances in the artifact are in **meters**. Studs are a display unit
  only and must not appear.
- Batch your builds: one engine build should validate the pose API, the capture path, and the
  independence check together.

## 5. Exit criterion

### Criterion
Across **10 distinct poses**, two captures taken at the same pose and the same simulation tick are
**byte-identical in the decoded pixel buffer** in **10/10** cases; each pose's capture differs from
its neighbour's by a **mean absolute pixel difference > 4.0** on an 8-bit scale; and moving the
agent camera leaves the human viewport pose unchanged in **10/10** cases.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_render_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE
Start-Process -FilePath .\eustress\target\release\eustress-headless.exe `
  -ArgumentList '--universe', "$env:EUSTRESS_WORKSPACE\RenderUniverse", '--render', 'gpu', '--no-autoplay'

cargo run --release --package eustress-agent-eval --bin eustress-camera-check -- `
  --universe "$env:EUSTRESS_WORKSPACE\RenderUniverse" `
  --recipe docs/PROMPTS/harness/recipes/G7_agent_camera.json `
  --poses 10 --repeat-per-pose 2 `
  --tick 300 `
  --width 1280 --height 720 `
  --frames-dir docs/PROMPTS/artifacts/G7.09/frames `
  --out docs/PROMPTS/artifacts/G7.09/agent_camera_determinism.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "adapter_from_g7_03": { "backend": "Vulkan", "name": "llvmpipe …", "software": true },
  "accumulating_effects_active": [],
  "tick": 300,
  "resolution": [1280, 720],
  "units": "meters",
  "poses": 10,
  "identical_repeat_captures": 10,
  "min_neighbour_mean_abs_diff": 11.7,
  "viewport_independence_ok": 10,
  "per_pose": [
    { "index": 0,
      "position_m": [12.0, 6.0, -18.0], "look_at_m": [0.0, 1.5, 0.0],
      "pixel_buffer_sha256_a": "3f8c…", "pixel_buffer_sha256_b": "3f8c…",
      "identical": true,
      "mean_abs_diff_vs_prev": null }
  ]
}
```

Pass condition:

```
EXIT == 0
AND identical_repeat_captures == poses == 10
AND min_neighbour_mean_abs_diff > 4.0
AND viewport_independence_ok == 10
AND accumulating_effects_active is an empty array
```

The check binary must exit non-zero on any repeat-capture mismatch or if any neighbour difference
falls at or below 4.0. Read the emitted values, not the frame files.

## 6. Critic gate

Gated on **D6 (overall coherence)**, floor **8.0**. The mean is irrelevant; a single dimension
below floor fails the item.

D6 is the right and only gate here because the question is whether the agent's viewpoint reads as
one designed instrument — consistent framing behaviour, a pose convention that means the same thing
at every scale, an orbit that orbits what it says it orbits — rather than a set of capabilities that
never met. It is not a gate on image beauty; that is D2 and belongs to the render packs.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_agent_camera.json`. Create it if absent, following
the recipe format in `docs/PROMPTS/02_CAPTURE_HARNESS.md`.

Note that the Critic never sees anything you write about your own work, and every score it gives
must cite a specific frame or measured value. The ten archived frames under
`docs/PROMPTS/artifacts/G7.09/frames` are what it will look at; make the pose set tell a coherent
story about the same scene rather than ten unrelated viewpoints.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — pose the existing RenderTarget::Image camera and read back at an
                 exact tick after an explicit render-graph settle
   -> if still failing, MANDATORY approach change. Adding another settle frame is NOT an approach
      change; capturing from a dedicated deterministic render pass is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with identical_repeat_captures moving < 1 and the
                  worst gated dimension moving < 0.5
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: byte differences trace to driver non-determinism (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.09/agent_camera_determinism.json`, with the ten pose pairs under
`docs/PROMPTS/artifacts/G7.09/frames/`.

A reader finds: the adapter carried forward from `G7.03`, the list of active frame-accumulating
effects (expected empty), the tick and resolution, the per-pose positions in meters, both capture
hashes per pose, the identical verdict, the neighbour difference, and the viewport-independence
result. `G7.23` cites this file to establish that its measured baseline scores were taken through a
reproducible instrument.

## 9. Definition of NOT done

- Captures are byte-identical because they are all blank — the camera is inside geometry or facing
  empty space. The neighbour-difference floor and the recorded frames are the guard, and this exact
  defect has occurred in this project before.
- Byte equality is claimed from PNG file hashes, and passes or fails depending on encoder metadata
  rather than on pixels. Compare decoded buffers.
- Determinism holds at 1280×720 and breaks at 3840×2160. Resolution-dependent determinism is a
  readback race, not determinism; state the resolutions tested.
- The agent camera moves the human viewport as a side effect, so an agent observing a world it
  shares with a human silently steals their view. Independence is separately gated.
- Camera positions are reported in studs. Units are meter-native; studs are a display unit only.
- The item passes every number and the Critic still refuses D6, citing that `orbit`, `frame`, and
  `set_pose` use three incompatible conventions for what "up" means. That is a legitimate refusal
  and the item is not done.
````

---

## `docs/PROMPTS/items/G7.10_detect-and-propose.md`

````markdown
---
id: G7.10
title: Detect and propose — machine-readable failure proposals
workload: W6
workload_secondary: [W3, W4]
phase: G7
depends_on: [G7.04, G7.06, G2.04, G1.12]
blocks: []
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G7_detect_propose.json
artifact: docs/PROMPTS/artifacts/G7.10/detector_precision_recall.json
escalation: >
  If detection precision cannot exceed 0.5 on seeded faults because the watchpoint sampling rate
  is too coarse to see the fault at all, STALL and report the sampling rate versus the fault
  duration. Raising the sampling rate has a telemetry-volume cost that is a program-level decision.
status: DRAFT
notes: >
  This closes the detect and propose arms of build -> run -> detect -> propose -> re-run. The
  detector is judged on seeded faults with known ground truth, so precision and recall are real
  numbers rather than impressions.
---

## 1. Objective

When a simulation run violates a declared expectation, the substrate emits a structured proposal —
naming the observed violation, the simulation variable implicated, the tick at which it occurred,
and a concrete candidate intervention — that an agent can act on without reading a log. Against a
suite of seeded faults with known ground truth, the detector's precision and recall are measured
numbers.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native.

**The loop stage this closes.** `build → run → detect → propose → re-run`. `G7.04` measured the
cost of one iteration; `G7.05` and `G7.06` made mutations transactional and reconstructable. This
item supplies the signal that decides whether the loop runs again and with what change.

**What exists to detect with.** The simulation kernel lives in
`eustress/crates/common/src/simulation/`: `clock.rs` (193 lines), `state.rs`, `watchpoint.rs`,
`breakpoint.rs`, `recorder.rs`, `config.rs`. Bevy integration is
`eustress/crates/engine/src/simulation/plugin.rs`, `electrochemistry.rs`, `rune_bindings.rs`,
`data_binding.rs`. `BreakPointRegistry` already sets run-exit conditions, and
`docs/architecture/HEADLESS_RUNTIME.md` §8 specifies that a tripped breakpoint maps to a non-zero
process exit — that is the crude form of detection that exists today.

**What the recorder gives you.** `eustress/crates/common/src/simulation/recorder.rs` defines
`SimulationRecording { metadata, series: HashMap<String, TimeSeries>, events: Vec<SimulationEvent> }`
and `RecordingMetadata { name, started_at, simulation_duration_s, wall_duration_s, total_ticks,
compression_ratio, tags }`. The export fires on entering Edit state
(`eustress/crates/engine/src/simulation/plugin.rs`), which is what `eustress-headless --ticks N`
triggers on completion. Your detector reads recordings; it does not need to run inside the engine.

**A known defect in the neighbourhood — do not reproduce it.** `docs/AUDIT/11_SIMULATION_DEBUGGER.md`
records that the Watchman alert cooldown is **wall-clock time, not simulation time**, so under high
time compression it misses sub-30-second spikes entirely. Your detector must operate on
**simulation time and tick index**, never on wall clock. If you find yourself writing a cooldown in
seconds of real time, you have rebuilt the bug.

**The other known defect that will corrupt your ground truth if ignored.**
`eustress/crates/common/src/simulation/clock.rs:83 advance()` advances `simulation_time_s` by the
full compressed delta but caps executed physics ticks at `max_ticks_per_frame` (default 10),
zeroing the accumulator on saturation at `clock.rs:100-102`. At high `time_scale` the clock reports
compressed time while the physics steps that would have covered it are discarded, with no error and
no counter. Run this item's fault suite at `time_scale = 1.0`. `G7.15` fixes the accounting; this
item must not depend on it, and must not silently inherit it.

**Ground truth by seeding.** A detector evaluated on real failures has no ground truth. Seed the
faults: inject a known perturbation into a known variable at a known tick, run, and check whether
the detector named that variable and that tick window. Precision and recall then have denominators.

**Build reality.** If the detector lives in `eustress/crates/agent-eval/` and reads exported
recordings, no engine build is needed to iterate on detection logic. Fault injection may need one.
10–15 minutes per engine build, one at a time, never killed mid-compile. Validate with `cargo run`,
not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — the detector and the proposal emitter
- `scripts/agent_eval/faults/` — the seeded fault suite definitions
- `docs/PROMPTS/harness/recipes/G7_detect_propose.json`
- `docs/PROMPTS/artifacts/G7.10/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/simulation/clock.rs` — the time-compression defect belongs to
  `G7.15`; touching it here creates a merge conflict with that item's evidence
- `eustress/crates/engine/src/ui/` — a proposal is a data structure, not a panel
- `docs/PROMPTS/artifacts/G7.04/`, `G7.06/` — frozen baselines
- `eustress/crates/common/src/simulation/watchpoint.rs` — owned by `G2.04` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Tuning the detector against the same fault instances used to score it, removing the fault classes
  it does poorly on, widening the accepted tick window until every detection counts as correct, or
  scoring "a proposal was emitted" rather than "the correct variable was named" are all measurement
  changes. If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with
  evidence and stop.
- Split the fault suite into a tuning half and a held-out scoring half before you write any
  detection logic, and score only on the held-out half. Record both halves' contents in the
  artifact so the split is auditable.
- All detector timing is in simulation time and tick index. No wall-clock cooldowns, ever — see the
  Watchman defect in §2.
- A proposal must be machine-actionable: tool name, parameters, and expected effect. "Investigate
  the battery voltage" is not a proposal. "Set `arc1.coolant_flow_lpm` to 12.0 and re-run 3600
  ticks; expect `arc1.core_temp_c` peak below 340" is.
- Include a **negative control**: runs with no seeded fault. Every proposal emitted on a
  no-fault run is a false positive and counts against precision.
- Run the whole suite at `time_scale = 1.0`.

## 5. Exit criterion

### Criterion
On a held-out suite of **at least 40 seeded fault runs across at least 5 fault classes**, plus
**at least 20 no-fault control runs**, the detector achieves **precision ≥ 0.85** and
**recall ≥ 0.80**, where a true positive requires naming the correct simulation variable **and** a
tick within ±5% of the run length of the injection tick, and every emitted proposal is
schema-valid and machine-actionable.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"

# 1. Execute the held-out fault suite (40 faulted + 20 control runs, batch mode)
cargo run --release --package eustress-agent-eval --bin eustress-fault-suite -- `
  --headless-bin .\eustress\target\release\eustress-headless.exe `
  --suite scripts/agent_eval/faults/heldout.json `
  --workspace $env:EUSTRESS_WORKSPACE `
  --ticks 3600 --time-scale 1.0 `
  --recordings-dir docs/PROMPTS/artifacts/G7.10/recordings

# 2. Score the detector against ground truth
cargo run --release --package eustress-agent-eval --bin eustress-detect-score -- `
  --recordings-dir docs/PROMPTS/artifacts/G7.10/recordings `
  --ground-truth scripts/agent_eval/faults/heldout.json `
  --tick-window-frac 0.05 `
  --proposals-dir docs/PROMPTS/artifacts/G7.10/proposals `
  --out docs/PROMPTS/artifacts/G7.10/detector_precision_recall.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "time_scale": 1.0,
  "ticks_per_run": 3600,
  "tuning_suite": "scripts/agent_eval/faults/tuning.json",
  "heldout_suite": "scripts/agent_eval/faults/heldout.json",
  "fault_classes": ["thermal_runaway", "flow_starvation", "sensor_dropout",
                    "actuator_saturation", "structural_overload"],
  "faulted_runs": 40,
  "control_runs": 20,
  "true_positives": 34, "false_positives": 5, "false_negatives": 6,
  "precision": 0.872, "recall": 0.850,
  "false_positives_on_controls": 2,
  "proposals_emitted": 39,
  "proposals_schema_valid": 39,
  "proposals_machine_actionable": 39,
  "example_proposal": {
    "detected": { "variable": "arc1.core_temp_c", "tick": 2118, "injection_tick": 2100,
                  "violation": "exceeded declared ceiling 340.0 (peak 402.7)" },
    "propose": { "tool": "set_sim_value",
                 "params": { "key": "arc1.coolant_flow_lpm", "value": 12.0 },
                 "then": { "tool": "sim_step", "params": { "ticks": 3600 } },
                 "expected_effect": "arc1.core_temp_c peak < 340.0" }
  }
}
```

Pass condition:

```
EXIT == 0
AND faulted_runs >= 40 AND control_runs >= 20 AND len(fault_classes) >= 5
AND precision >= 0.85
AND recall >= 0.80
AND proposals_schema_valid == proposals_emitted
AND proposals_machine_actionable == proposals_emitted
AND time_scale == 1.0
```

The scorer must exit non-zero if either rate falls below its floor or if any proposal fails schema
validation. Read the rates from the JSON.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**. The mean is
irrelevant; below floor on D5 fails the item.

D5 is the right gate because the question a domain expert asks of a detector is not "did it fire"
but "would I have acted on this". A proposal that names a real violation with a plausible
intervention and a falsifiable expected effect earns trust; one that names a symptom and suggests
looking into it does not. Prioritise the *content* of the proposal — the named variable, the tick,
the stated violation with its numeric threshold and observed value, and an expected effect that the
next run can refute.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_detect_propose.json`. Create it if absent,
following the format in `docs/PROMPTS/02_CAPTURE_HARNESS.md`.

The Critic never sees your self-report and must cite a specific value or artifact path for every
score. It will read proposals from `docs/PROMPTS/artifacts/G7.10/proposals/`; those files must
stand alone.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — declared expectations on watchpoints, evaluated in simulation time
                 against the exported recording
   -> if still failing, MANDATORY approach change. Retuning a threshold is NOT an approach change;
      moving from threshold violation to residual-against-a-declared-model is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with precision moving < 0.05 and recall moving < 0.05
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: watchpoint sampling is too coarse to resolve the fault (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.10/detector_precision_recall.json`, with the emitted proposals under
`docs/PROMPTS/artifacts/G7.10/proposals/` and the source recordings under
`docs/PROMPTS/artifacts/G7.10/recordings/`.

A reader finds: the pinned time scale and tick count, both suite paths so the tuning/held-out split
is auditable, the fault classes, the confusion counts, precision and recall, the false-positive
count on no-fault controls, proposal validity counts, and a full worked example proposal. This is
the W6 evidence that the loop can decide to run again without a human interpreting a log.

## 9. Definition of NOT done

- Precision and recall are measured on the same faults the detector was tuned against. The
  held-out split is mandatory and both suite files are recorded so the split can be checked.
- The detector fires on every run, so recall is 1.0 and precision collapses on the controls. The
  20 no-fault controls exist for exactly this.
- Detection uses a wall-clock cooldown and therefore misses fast transients under compression —
  the same defect `docs/AUDIT/11_SIMULATION_DEBUGGER.md` already records for the Watchman. Rebuilt
  bugs are not fixes.
- Proposals are emitted but say "investigate X". A proposal with no tool, no parameters, and no
  refutable expected effect is a notification.
- The suite is run at high `time_scale` for speed, so the ground-truth injection tick and the
  executed physics no longer correspond, and every rate is meaningless. `time_scale` is pinned to
  1.0 and gated.
- Precision and recall clear their floors and the Critic still refuses D5, citing that the proposed
  interventions would not be made by anyone who understands the system. That is a legitimate
  refusal and the item is not done.
````

---

## `docs/PROMPTS/items/G7.11_retire-foundation-model-dispatcher.md`

````markdown
---
id: G7.11
title: Retire the FoundationModelDispatcher claim and state the real model boundary
workload: W3
workload_secondary: [W5]
phase: G7
depends_on: [G7.01]
blocks: []
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.11/model_boundary_decision.md
escalation: >
  If any shipped code path — not documentation — actually depends on a FoundationModelDispatcher
  type or a ModelTier enum, STALL. Removing a claim is a documentation action; removing a
  dependency is a code change with a different risk profile and does not belong in a Tier S item.
status: DRAFT
notes: >
  Tier S, zero builds: this item edits documentation and module-level doc comments only. It is in
  this pack because an agent-loop pack that leaves a phantom AI subsystem in the docs is selling
  something it does not have.
---

## 1. Objective

No document or source comment in this repository asserts that a `FoundationModelDispatcher` or a
`ModelTier` dispatch system exists. In their place, one decision record states the substrate's
actual model boundary — the MCP tool surface and the Engine Bridge — and records what would have to
be true to revisit an in-engine dispatcher. The `spatial-llm` crate's own module documentation
states its real status.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never described as a
game engine. Source-available under PolyForm Shield 1.0.0 — never "open source". Physics is
**Avian**, never Rapier. Slint compiles to Rust. Units are meter-native.

**The claim being retired, quoted from the repository's own audit.**
`docs/AUDIT/07_AI_PLATFORM.md:10` records: "FoundationModelDispatcher (Feature 12) is **fully
absent** (not 'pseudocode') — `APEX_ENGINE.md` referenced but the file is missing. Spatial-LLM
(Feature 6): modules exist but **none call Claude** — state 🟡 → **🔴** (no wiring)."
`docs/AUDIT/MASTER.md:25` repeats it. `docs/AUDIT/07_AI_PLATFORM.md:40` adds: "ModelTier +
FoundationModelDispatcher are pseudocode only."

The current occurrences in the tree are: `docs/architecture/APEX_ENGINE.md:137` and `:152` (a
`pub struct FoundationModelDispatcher` and its `impl` block, in prose, in a document that is
entirely aspirational — a dependency wish-list with no code behind it);
`docs/AUDIT/01_CLIENT_PLAYER.md:240`; `docs/AUDIT/07_AI_PLATFORM.md` at lines 10, 20, 40, and 303;
and `docs/AUDIT/MASTER.md` at lines 25 and 69. Verify this list at execution time with the grep in
§5 rather than trusting it.

**The `spatial-llm` reality.** `eustress/crates/spatial-llm/src/` is 1,515 lines across
`client.rs` (264), `context.rs` (294), `error.rs` (57), `generation.rs` (190), `indexing.rs` (173),
`lib.rs` (90), `prompt.rs` (212), `query.rs` (235). `client.rs` defines `SpatialLlmConfig` with
`api_endpoint: Option<String>`, `api_key: Option<String>`, and `model: String` defaulting to
`"gpt-4"` — configuration for a call that is never made. The crate compiles, has a coherent
internal design, and issues no model requests.

**Why retiring the claim is the constructive answer, not a retreat.** Eustress's model integration
point already exists, is already the strongest available design, and is already shipped: **the MCP
tool surface plus the TCP Engine Bridge**. The model runs outside the process, reasons over the
world through 104 defined tool descriptors, and mutates it through a gated, op-logged,
undoable interface — the exact properties `G7.05`, `G7.06`, and `G7.07` establish. An in-engine
dispatcher would put a network call on the simulation's critical path and would couple model
choice to an engine build that takes 10–15 minutes. Saying so plainly is a stronger position than
a phantom subsystem.

**A relevant known gap to record accurately, not to fix here.**
`docs/AUDIT/07_AI_PLATFORM.md:10` also records that MCP `SubscribeTopic` is **absent** from the
`BridgeRequest::MethodName` enum. That is a real gap in the model boundary. This item records it in
the decision record as a named open question; it does not implement it.

**Documentation standard.** Documents must read as though always correct. No "previously we
thought", no changelog residue in the body, no self-justifying commentary. If a document's premise
is now wrong, rewrite the document, do not annotate it. The one exception is the decision record
itself, whose entire purpose is to record a decision — and even it states the decision and its
conditions, not a narrative of how the team felt about it.

**Build reality.** `max_builds` is 0. This item compiles nothing. If you find yourself needing a
build, you have exceeded scope — see the escalation trigger.

## 3. Scope

### In scope — files this item may edit
- `docs/architecture/APEX_ENGINE.md`
- `docs/AUDIT/07_AI_PLATFORM.md`
- `docs/AUDIT/01_CLIENT_PLAYER.md`
- `docs/AUDIT/MASTER.md`
- `eustress/crates/spatial-llm/src/lib.rs` — the module-level `//!` documentation block only; no
  functional code
- `docs/PROMPTS/artifacts/G7.11/model_boundary_decision.md` — the new decision record

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- Any functional Rust in `eustress/crates/spatial-llm/` — this item changes documentation, not
  behaviour
- `eustress/crates/tools/`, `eustress/crates/mcp-server/` — the real model boundary is described,
  not modified
- `docs/PROMPTS/artifacts/G7.01/` — frozen baseline

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Renaming the type so the grep stops matching, moving the claim to a file the grep does not scan,
  or adding an exclusion to the grep pattern are all measurement changes. If the measurement is
  genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Deleting `APEX_ENGINE.md` outright is permitted and may be the right call, but if it survives it
  must not assert the dispatcher exists. Either outcome satisfies the grep; state which you chose
  and why in the decision record.
- The audit files are the repository's honest gap ledger and their honesty is an asset. Rewriting
  an audit finding to remove the *gap* rather than the *claim of existence* is the opposite of this
  item's purpose. An audit line may continue to record that the feature is absent — what must go is
  any sentence written as though the subsystem is part of the system.
- The decision record must name the model boundary in terms of surfaces that exist, cite them by
  path, and list open questions — including the absent MCP `SubscribeTopic` — without implying any
  of them are scheduled.
- Do not write "open source". The licence is PolyForm Shield 1.0.0 and the project is
  source-available.

## 5. Exit criterion

### Criterion
A repository-wide grep for `FoundationModelDispatcher` and `ModelTier` returns **zero lines that
assert the subsystem exists**, outside the decision record itself; the decision record exists and
names the real model boundary with at least three verifiable repository paths; and
`eustress/crates/spatial-llm/src/lib.rs` opens with an explicit status statement.

### Measurement

Command:

```
# 1. Surviving occurrences outside the decision record
$hits = git grep -n -E "FoundationModelDispatcher|ModelTier" -- ':!docs/PROMPTS/artifacts/G7.11/*'
$hits
echo "SURVIVING_HITS=$((@($hits) | Where-Object { $_ -ne '' }).Count)"

# 2. The decision record exists and cites real paths
$doc = Get-Content docs/PROMPTS/artifacts/G7.11/model_boundary_decision.md -Raw
$paths = [regex]::Matches($doc, 'eustress/crates/[A-Za-z0-9_\-/]+\.rs') |
         ForEach-Object { $_.Value } | Sort-Object -Unique
$missing = $paths | Where-Object { -not (Test-Path $_) }
echo "CITED_PATHS=$($paths.Count)  MISSING_PATHS=$($missing.Count)"

# 3. spatial-llm states its status in its own module docs
$status = (Select-String -Path eustress/crates/spatial-llm/src/lib.rs `
  -Pattern '^//!.*[Ss]tatus' | Measure-Object).Count
echo "SPATIAL_LLM_STATUS_LINES=$status"

# 4. Every surviving hit is justified in the decision record
$justified = (Select-String -Path docs/PROMPTS/artifacts/G7.11/model_boundary_decision.md `
  -Pattern '^\| surviving-hit \|' | Measure-Object).Count
echo "JUSTIFIED_HITS=$justified"
```

Expected output shape:

```
SURVIVING_HITS=2
CITED_PATHS=5  MISSING_PATHS=0
SPATIAL_LLM_STATUS_LINES=1
JUSTIFIED_HITS=2
```

Pass condition:

```
CITED_PATHS >= 3
AND MISSING_PATHS == 0
AND SPATIAL_LLM_STATUS_LINES >= 1
AND JUSTIFIED_HITS == SURVIVING_HITS
AND every surviving hit reads as a record of ABSENCE, never as a description of a shipped
    subsystem — each is listed in the decision record's `surviving-hit` table with its one-line
    justification, and a reviewer confirms each
```

The grep count alone does not pass the item: `SURVIVING_HITS` may be non-zero, but every surviving
line must appear in the decision record's table with a justification, and the table must be
complete. Read the four emitted values.

## 6. Critic gate

`critic_gate` is `[]`. There is no perceptual artifact and no measurement to score. The replacement
mechanical criterion is unusually tight: every cited path verified to exist on disk, a status line
required in `spatial-llm`'s own module docs, and a one-to-one correspondence between surviving grep
hits and justified entries in the decision record — so no occurrence can be silently left behind.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — rewrite the affected documents so each reads as though always
                 correct, then write the decision record
   -> if still failing, MANDATORY approach change. Editing one more sentence is NOT an approach
      change; deleting APEX_ENGINE.md and folding its live content elsewhere is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with JUSTIFIED_HITS < SURVIVING_HITS
  - Budget      : 90k tokens consumed (150% of the S envelope); any cargo build at all
  - Item-specific: shipped code, not documentation, depends on the type (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.11/model_boundary_decision.md`

A reader finds: a one-paragraph statement of where a model attaches to Eustress today (the MCP tool
surface and the TCP Engine Bridge), cited to at least three verified repository paths; the decision
to retire the in-engine dispatcher concept with the reason stated in terms of the simulation's
critical path and build economics; the current honest status of `eustress/crates/spatial-llm/`; a
list of named open questions including the absent MCP `SubscribeTopic`; and a `surviving-hit` table
listing every remaining grep occurrence with a one-line justification. This is the W3 evidence that
the AI platform surface in the documentation matches the AI platform surface in the code.

## 9. Definition of NOT done

- The grep is clean because the type was renamed. The claim, not the identifier, is what is being
  retired.
- An audit finding recording the absence of the feature is deleted along with the claim. The audit
  is the honest gap ledger and its findings are an asset; only assertions of existence go.
- The decision record cites `docs/architecture/APEX_ENGINE.md` as evidence of the intended design.
  That document is the source of the claim being retired; citing it re-enters the loop.
- The decision record cites a path that does not exist. Every cited path is checked mechanically.
- `spatial-llm`'s module docs are updated to say it is "in progress". It is a stub that makes no
  model calls; say that.
- A rewritten document carries a note explaining that the dispatcher was previously described here.
  Documents read as though always correct; the change is explained in the decision record and
  nowhere else.
- The words "open source" appear anywhere in the new or rewritten text. The licence is PolyForm
  Shield 1.0.0 and the project is source-available.
````

---

# Half B — the training and evaluation substrate

---

## `docs/PROMPTS/items/G7.12_etask-specification-format.md`

````markdown
---
id: G7.12
title: .etask — the environment and task specification format
workload: W5
workload_secondary: [W3, W4]
phase: G7
depends_on: [G7.01, G1.12, G1.48]
blocks: [G7.13, G7.18]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D6]
capture_recipe: docs/PROMPTS/harness/recipes/G7_etask_spec.json
artifact: docs/PROMPTS/artifacts/G7.12/etask_spec_v1.md
escalation: >
  If the format cannot express a task whose success predicate depends on CAD geometry — mass,
  bounding volume, interference, or a measured feature — STALL. Physically verifiable success over
  engineering artifacts is the property that distinguishes this environment from every existing
  benchmark; a format that cannot express it is not worth shipping.
status: DRAFT
notes: >
  This is the first of four publishable external artifacts in pack T4. It is written to be read by
  someone outside the company who has never run Eustress.
---

## 1. Objective

A published, versioned specification defines what an Eustress task is: the starting world, the seed,
the observation the agent receives, the actions it may take, the tick and time budget, and the
success predicate — with the predicate expressed over simulation and geometry state rather than
over text. A validator accepts three conforming example tasks and rejects three deliberately
malformed ones with distinct, named error codes.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never described as a
game engine, in this document or in the specification you are writing. It is source-available under
PolyForm Shield 1.0.0; never write "open source". Physics is **Avian**, never Rapier. Slint
compiles to Rust, so Slint *is* Rust. **Units are meter-native**; studs are a display unit only and
must not appear anywhere in the specification.

**Who reads the artifact.** An engineer at an external lab who has never run Eustress, evaluating
whether to adopt it as an agentic environment. The specification must therefore be self-contained,
must not assume familiarity with the studio, and must not require reading the source to understand
a field.

**What must make this format different from an existing gym environment spec.** Four properties,
and the format must express all four or it is not worth publishing:

1. **First-principles physics.** The world evolves under Avian rigid-body dynamics plus the realism
   tier, not under a hand-authored transition function. A task declares the world; it does not
   declare the dynamics.
2. **Time compression.** A task may declare a `time_scale` so an episode covers simulated hours.
   The format must carry the honest accounting: see the `dropped_ticks` constraint below.
3. **Engineering artifacts.** A task may require the agent to *produce a part*, using the
   truck-based CAD kernel (`eustress/crates/cad/`). Tier 1 of the CAD agent interface has shipped:
   `cad_describe_part`, `cad_validate` and `cad_measure` exist, with geometry helpers in
   `eustress/crates/cad/src/measure.rs`. A predicate can therefore be written over a measured mass
   or a measured interference.
4. **Physically verifiable success.** The predicate reads state. It never string-matches an
   agent's answer.

**The honest-accounting constraint you must design in.**
`eustress/crates/common/src/simulation/clock.rs:83 advance()` advances `simulation_time_s` by the
full compressed delta while capping executed physics ticks at `max_ticks_per_frame` (default 10)
and zeroing the accumulator on saturation (`clock.rs:100-102`). Under high `time_scale` the clock
therefore reports compressed time that the physics never executed. Every `.etask` must carry a
declared `max_dropped_tick_fraction`, and an episode exceeding it is invalid rather than failed.
`G7.15` implements the counter; your format must have the field from version 1 so no reissue is
needed.

**Existing shapes to align with, not to reinvent.**

- `eustress/crates/common/src/simulation/recorder.rs` — `SimulationRecording { metadata, series,
  events }` with `RecordingMetadata { name, started_at, simulation_duration_s, wall_duration_s,
  total_ticks, compression_ratio, tags }`. An episode result should embed or reference a recording.
- `eustress/crates/tools/src/registry.rs` — `ToolDefinition` already carries a JSON Schema
  `input_schema` per tool. A task's action space is a **subset of tool names**, and each action's
  schema is the tool's existing schema. Do not invent a second schema language.
- `docs/PROMPTS/artifacts/G7.01/tool_conformance.json` — the authoritative list of 104 defined tool
  descriptors and which of them are reachable headlessly. Actions that are `unreachable` headlessly
  may not appear in an action space.
- Worlds: a Space directory holding `world.fjalldb`, discovered under `EUSTRESS_WORKSPACE`
  (`eustress/crates/engine/src/space/mod.rs:119`).

**What the format must not do.** It must not embed the world. A Space is potentially gigabytes; a
task references a Space by a content hash and a path, so the task file stays reviewable and
diffable.

**Build reality.** The validator belongs in `eustress/crates/agent-eval/` and must not depend on
`eustress-engine`, so it builds in seconds. Engine builds are 10–15 minutes, one at a time, never
killed mid-compile. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/G7.12/etask_spec_v1.md` — the publishable specification
- `docs/PROMPTS/artifacts/G7.12/schema/etask.v1.schema.json` — the machine schema
- `eustress/crates/agent-eval/` — including `src/bin/etask_validate.rs`
- `tasks/examples/` — the three conforming and three malformed example task files
- `docs/PROMPTS/harness/recipes/G7_etask_spec.json`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/tools/src/` — the action space *references* tool schemas; it does not change them
- `eustress/crates/common/src/simulation/clock.rs` — the dropped-tick counter is `G7.15`
- `docs/PROMPTS/artifacts/G7.01/` — frozen baseline

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Making the malformed examples fail for the same reason so one error code covers all three,
  loosening the validator until the malformed files pass, or declaring a required field optional
  because an example omits it are all measurement changes. If the measurement is genuinely wrong,
  report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The three malformed examples must fail for **three distinct, named reasons**, and each reason must
  correspond to a property that actually matters: a success predicate that is a string match rather
  than a state predicate; a missing or unseeded RNG seed; and an action referencing a tool that does
  not exist in the census.
- Success predicates are expressed over simulation values, entity state, or CAD measurements. The
  format must make a string-match predicate *inexpressible*, not merely discouraged. If a lab can
  write one, they will.
- Every quantity in the specification is in SI, meter-native. No studs.
- The specification must state, in its own words, which of the four differentiating properties each
  section serves. A lab reader must be able to see why the format is shaped this way.
- Do not embed world data in a task file. Reference a Space by path and content hash.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion
The validator accepts **3/3** conforming example tasks and rejects **3/3** malformed examples with
**3 distinct error codes**; every action named across the three conforming tasks resolves to a tool
present in the `G7.01` census and reachable headlessly; and at least one conforming task's success
predicate is a **CAD measurement**.

### Measurement

Command:

```
cargo run --release --package eustress-agent-eval --bin eustress-etask-validate -- `
  --schema docs/PROMPTS/artifacts/G7.12/schema/etask.v1.schema.json `
  --accept tasks/examples/valid/thermal_ceiling.etask.json `
  --accept tasks/examples/valid/bracket_mass_budget.etask.json `
  --accept tasks/examples/valid/structure_survives_load.etask.json `
  --reject tasks/examples/invalid/string_match_predicate.etask.json `
  --reject tasks/examples/invalid/missing_seed.etask.json `
  --reject tasks/examples/invalid/unknown_action.etask.json `
  --tool-census docs/PROMPTS/artifacts/G7.01/tool_conformance.json `
  --require-distinct-reject-codes 3 `
  --require-at-least-one-cad-predicate `
  --out docs/PROMPTS/artifacts/G7.12/validation_report.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape (`validation_report.json`):

```json
{
  "schema_version": 1,
  "spec_version": "etask/1.0",
  "accepted": 3, "accepted_expected": 3,
  "rejected": 3, "rejected_expected": 3,
  "reject_codes": ["E_PREDICATE_NOT_STATE_BASED", "E_SEED_MISSING", "E_ACTION_UNKNOWN"],
  "distinct_reject_codes": 3,
  "actions_referenced": 14,
  "actions_resolved_in_census": 14,
  "actions_unreachable_headless": 0,
  "predicate_kinds_used": ["sim_value_threshold", "cad_measurement", "entity_state"],
  "cad_predicate_present": true,
  "units": "SI, meter-native",
  "world_embedded_in_task": false
}
```

Pass condition:

```
EXIT == 0
AND accepted == 3 AND rejected == 3
AND distinct_reject_codes == 3
AND actions_resolved_in_census == actions_referenced
AND actions_unreachable_headless == 0
AND cad_predicate_present == true
AND world_embedded_in_task == false
```

The validator must exit non-zero if any accept file is rejected, any reject file is accepted, or
fewer than three distinct codes were produced. Read the emitted counters.

## 6. Critic gate

Gated on **D6 (overall coherence)**, floor **8.0**. The mean is irrelevant; below floor fails.

D6 is the gate because a specification is judged on whether it reads as one designed system. The
questions a Critic will ask: do observation, action, and predicate use one consistent vocabulary;
does a reader who has never seen Eustress understand what a task is after one pass; do the four
differentiating properties show up as structure rather than as marketing; and does every field
earn its place. The specification is the artifact under review — the Critic reads
`docs/PROMPTS/artifacts/G7.12/etask_spec_v1.md` and the three conforming examples, and nothing you
write about your own work.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_etask_spec.json`. Create it if absent, following
the format in `docs/PROMPTS/02_CAPTURE_HARNESS.md`.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — JSON Schema for structure plus a typed predicate enum that admits
                 only state-based kinds
   -> if still failing, MANDATORY approach change. Adding a field is NOT an approach change;
      moving the predicate from a declarative enum to a sandboxed expression language is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with distinct_reject_codes < 3 and the worst gated
                  dimension moving < 0.5
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a CAD-measurement predicate is inexpressible (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.12/etask_spec_v1.md`, with
`docs/PROMPTS/artifacts/G7.12/schema/etask.v1.schema.json`,
`docs/PROMPTS/artifacts/G7.12/validation_report.json`, and the six example task files under
`tasks/examples/`.

A reader finds a specification that stands alone: what a task is, every field with its type and
meaning, the four properties that distinguish this environment and which section serves each, the
predicate kinds and why a string match is inexpressible, the seeding and dropped-tick fields, and
three worked examples. This is the **first publishable external artifact** in pack T4 and the W5
evidence for the extension surface.

## 9. Definition of NOT done

- A success predicate can be written as a string comparison against the agent's output. That single
  affordance turns the whole benchmark into a text benchmark, which the world already has.
- The three malformed examples all fail schema validation for the same structural reason, so the
  three distinct codes are cosmetic.
- The format carries no seed, or carries a seed that nothing consumes. Replayability starts here;
  `G7.14` cannot fix a format that never recorded the seed.
- `max_dropped_tick_fraction` is omitted because `G7.15` has not landed. The field must exist in
  version 1 or every compressed episode published under this format is unfalsifiable.
- The action space is a new schema language rather than a subset of existing tool names and their
  existing schemas. Two schema languages is one too many and they will diverge.
- A task file embeds the world, so a task is gigabytes and cannot be reviewed in a pull request.
- Distances appear in studs. Units are meter-native; studs are a display unit only.
- The specification calls Eustress a game engine, or calls the licence open source. Either fails the
  item outright, and this artifact is published externally.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.
````

---

## `docs/PROMPTS/items/G7.13_env-reset-step-api.md`

````markdown
---
id: G7.13
title: env.reset and env.step over the Engine Bridge
workload: W5
workload_secondary: [W3, W6]
phase: G7
depends_on: [G7.02, G7.12, G1.05]
blocks: [G7.14, G7.16, G7.18, G7.20, G7.21]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.13/env_api_conformance.json
escalation: >
  If env.reset cannot complete in under 10 s on the reference fixture, STALL and report the
  measured reset cost against the fork/discard cost from G7.08. Reset cost multiplies by every
  episode in every training run; a slow reset caps throughput before any other optimisation
  matters.
status: DRAFT
notes: >
  The API surface a lab actually calls. Deliberately shaped like the reset/step contract every RL
  harness already speaks, so adoption costs an adapter, not a rewrite.
---

## 1. Objective

The Engine Bridge exposes two methods, `env.reset` and `env.step`, that turn a Space plus an
`.etask` file into an episodic environment: `reset(seed)` returns an initial observation and
`step(action)` returns an observation, a scalar outcome signal, a terminal flag, and structured
info. A client script drives 1,000 steps across 10 episodes with zero protocol errors, and two
resets with the same seed produce byte-identical first observations.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native.

**Why reset/step and not something new.** Every RL and agentic-eval harness in existence already
speaks a reset/step contract. Matching its shape means a lab writes an adapter, not a rewrite.
Deviating from it means Eustress is evaluated on integration cost before it is evaluated on
capability.

**The transport you extend.** `eustress/crates/engine/src/engine_bridge/` is JSON-RPC 2.0 over TCP;
`protocol.rs` is 2,729 lines and already implements `ecs.query`, `ecs.inspect`,
`entity.create/read/update/delete/find`, `entity.add_tag/remove_tag/promote/demote`, `sim.read`,
`sim.step`, `sim.bindings`, `scene.overview`, `scene.raycast`, `oplog.tail`, `selection.set`,
`state.get`, `tool.equip`, `tools.call`, `tools.list`, `action.invoke`, `data.bind/unbind/bindings`,
and the `ai_camera.*` and `capture.*` families. Discovery is `<universe>/.eustress/engine.port`;
the client is `eustress/crates/bridge-client/` (crate `eustress-bridge-client`), whose entire
public surface is `call_engine`, `call_engine_with_timeout`, and `port_file_path`, and whose only
dependency is `serde_json`.

**`sim.step` already exists and is not `env.step`.** `sim.step` advances the simulation a number of
ticks. `env.step` applies an *action* and then advances by the task's declared tick budget per step,
returning an observation and outcome. Build `env.step` on top of `sim.step`; do not overload it.

**The task format you consume.** `.etask` version 1, specified in
`docs/PROMPTS/artifacts/G7.12/etask_spec_v1.md` with the machine schema at
`docs/PROMPTS/artifacts/G7.12/schema/etask.v1.schema.json`. It declares the starting Space by path
and content hash, the seed, the observation and action spaces, the tick and time budget, the
success predicate, and `max_dropped_tick_fraction`.

**Reset semantics, and the primitive that implements them.** `G7.08` proved that a Space can be
forked, mutated, and discarded with the original's state digest restored exactly and no orphan
op-log records — see `docs/PROMPTS/artifacts/G7.08/fork_rehearse_commit.json` for the measured
fork/discard costs at three entity scales. **An episode is a fork; a reset is a discard followed by
a fresh fork.** Do not implement a second reset mechanism.

**Seeding.** `eustress/crates/common/src/physics/determinism.rs` is 57 lines: a
`GlobalRngSeed(0x5EED_E057_1234_ABCD)` resource and nothing else. The determinism pins live in
`eustress/crates/engine/src/main.rs` — `Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`, and a
`SolverConfig`. `env.reset(seed)` must set the global RNG seed from the episode seed, and this item
must prove the seed reaches the world by showing two same-seed resets produce byte-identical first
observations. Full episode determinism across tick rates is `G7.14`; this item establishes only that
reset is seed-respecting.

**Outcome signal, stated honestly.** `env.step` returns a scalar so standard harnesses have
somewhere to put it, but the authoritative outcome is the `.etask` success predicate evaluated over
state. Real predicate implementation is `G7.18`. For this item the scalar may be the predicate's
current satisfaction distance; it must never be a text similarity.

**Build reality.** Changing `protocol.rs` rebuilds the engine: 10–15 minutes, one at a time, never
killed mid-compile. The client driver belongs in `eustress/crates/agent-eval/` and must not link
the engine. Validate with `cargo run`, not `cargo check` — a JSON-RPC dispatch arm that fails to
register is exactly the class `cargo check` cannot see.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — the two new `env.*` arms
- `eustress/crates/engine/src/engine_bridge/mod.rs` — registration only
- `eustress/crates/agent-eval/` — including `src/bin/env_driver.rs` and a reusable client module
- `docs/PROMPTS/artifacts/G7.13/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- The existing `sim.step` behaviour — `env.step` is built on it, not in place of it
- `eustress/crates/common/src/simulation/clock.rs` — belongs to `G7.15`
- `docs/PROMPTS/artifacts/G7.08/`, `G7.12/` — frozen baselines
- `eustress/crates/engine/src/ui/` — an environment API has no UI tier
- Any existing entry in `eustress/crates/engine/src/engine_bridge/protocol.rs` — the file is owned by `G1.05`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Reducing the step count below 1,000, shortening episodes so fewer steps run per reset, retrying
  failed calls silently and reporting zero protocol errors, or comparing first observations with a
  tolerance are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Reset must be implemented as discard-plus-fork over the `G7.08` primitive. A second reset path
  will diverge from the one that has proven residue-free.
- An illegal action — one outside the task's declared action space — must be refused with a
  structured error and must **not** advance the episode. Silently ignoring it makes step counts
  meaningless.
- `env.step` must return the same four-part shape on every call, including on refusal, so a client
  never has to branch on response shape.
- Observation equality means byte equality of the canonical serialisation, not field-by-field
  approximate comparison.
- Batch your builds: one engine build should validate both methods, the error paths, and the
  registration.

## 5. Exit criterion

### Criterion
A client driver completes **1,000 `env.step` calls across 10 episodes** with **zero protocol
errors** and **zero malformed responses**; two `env.reset` calls with the same seed produce
**byte-identical** first observations in **10/10** paired trials; and **100%** of illegal actions
are refused without advancing the step counter.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_task_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE
Start-Process -FilePath .\eustress\target\release\eustress-headless.exe `
  -ArgumentList '--universe', "$env:EUSTRESS_WORKSPACE\TaskUniverse", '--no-autoplay'

cargo run --release --package eustress-agent-eval --bin eustress-env-driver -- `
  --universe "$env:EUSTRESS_WORKSPACE\TaskUniverse" `
  --task tasks/examples/valid/thermal_ceiling.etask.json `
  --episodes 10 --steps-per-episode 100 `
  --seed-pairs 10 `
  --illegal-action-probes 50 `
  --out docs/PROMPTS/artifacts/G7.13/env_api_conformance.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "task": "tasks/examples/valid/thermal_ceiling.etask.json",
  "episodes": 10,
  "steps_total": 1000,
  "protocol_errors": 0,
  "malformed_responses": 0,
  "step_response_shape": ["observation", "outcome", "done", "info"],
  "seed_pairs_tested": 10,
  "seed_pairs_identical_first_observation": 10,
  "illegal_action_probes": 50,
  "illegal_actions_refused": 50,
  "illegal_actions_that_advanced_step": 0,
  "reset_ms": { "p50": 812.4, "p95": 1204.9 },
  "step_ms":  { "p50": 61.3,  "p95": 118.7 },
  "reset_mechanism": "discard+fork over G7.08 primitive"
}
```

Pass condition:

```
EXIT == 0
AND steps_total >= 1000 AND episodes >= 10
AND protocol_errors == 0 AND malformed_responses == 0
AND seed_pairs_identical_first_observation == seed_pairs_tested == 10
AND illegal_actions_refused == illegal_action_probes == 50
AND illegal_actions_that_advanced_step == 0
AND reset_ms.p50 is present and numeric
```

The driver must exit non-zero on any protocol error, any observation mismatch between same-seed
resets, or any illegal action that advanced the counter. Read the counters.

## 6. Critic gate

`critic_gate` is `[]`. The criterion is a set of exact counter equalities over a thousand real
calls, plus a byte-equality check across ten seed pairs. That is stronger and less arguable than a
perceptual score, and there is no perceptual surface here. The four-part response-shape assertion
replaces what a design reviewer would otherwise be asked to judge.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — two new JSON-RPC arms in protocol.rs, reset delegating to the
                 G7.08 fork/discard primitive
   -> if still failing, MANDATORY approach change. Adding a retry is NOT an approach change;
      running one headless process per episode and making reset a process restart is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with steps_total moving < 10% and protocol_errors > 0
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: reset_ms.p50 > 10000 (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.13/env_api_conformance.json`

A reader finds: the task driven, the episode and step totals, the error counters, the fixed
response shape, the seed-pair determinism result, the illegal-action refusal results, measured
reset and step latencies, and the reset mechanism named explicitly. `G7.14` builds episode
determinism on this API; `G7.21` cites the reset and step latencies in the throughput budget;
`G7.20` runs this same driver inside a container.

## 9. Definition of NOT done

- `env.step` is an alias for `sim.step`, so an action is never applied and the environment is a
  clock with extra steps.
- Illegal actions are ignored rather than refused, so an agent that emits nonsense burns its step
  budget invisibly and the episode length no longer means anything.
- Same-seed resets differ, and the difference is dismissed as "just floating point". Byte equality
  of the first observation is the gate; a seed that does not reach the world is not a seed.
- Reset is implemented as a bespoke world-clearing routine rather than the proven `G7.08`
  discard-plus-fork, so residue reappears after episode 200 and no one notices until a training run
  is already spoiled.
- The response shape varies — a success returns four fields and a refusal returns two — so every
  client must branch on shape. The shape assertion exists for this.
- Protocol errors are retried inside the driver and reported as zero. Retries must be reported;
  a hidden retry is a hidden failure rate.
- The outcome scalar is computed from text similarity between the agent's message and an expected
  answer. That is the exact failure mode this whole pack exists to avoid.
````

---

## `docs/PROMPTS/items/G7.14_episode-determinism.md`

````markdown
---
id: G7.14
title: Episode determinism under seed and tick-rate variation
workload: W3
workload_secondary: [W5]
phase: G7
depends_on: [G7.13, G7.15, G2.02]
blocks: [G7.17]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.14/episode_determinism.json
escalation: >
  If byte-identical episode recordings are unachievable because Avian's solver is
  order-nondeterministic under the pinned SubstepCount and SolverConfig, STALL and report the
  divergence tick and magnitude. Cross-run physics determinism is a program-level claim; if it
  cannot hold, every downstream benchmark result must be restated as distributional rather than
  exact, and that is a human decision.
status: DRAFT
notes: >
  The tick-rate variation clause is the point of this item. Same-seed same-machine equality is
  easy to achieve accidentally; equality across main-loop rates proves the simulation is actually
  decoupled from wall clock.
---

## 1. Objective

An episode is fully determined by its task, its seed, and its action sequence. Replaying the same
triple produces a byte-identical episode recording across independent process invocations **and**
across three different main-loop tick rates, proving the simulation's evolution is decoupled from
wall clock and host scheduling.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**, never Rapier. Slint compiles to
Rust. Units are meter-native.

**Why this is the item that makes the environment usable for training.** Without it, a lab cannot
attribute an outcome difference to a policy difference. It is also the repository's own declared
gate: `docs/architecture/HEADLESS_RUNTIME.md:294` states the determinism gate as "the same space +
same `--ticks` produces byte-identical recordings across two runs". That gate is **written and has
never been recorded as passed**. This item executes it, and strengthens it.

**The honest current state of determinism.** It is claimed and untested.
`eustress/crates/common/src/physics/determinism.rs` is **57 lines** — a
`GlobalRngSeed(0x5EED_E057_1234_ABCD)` resource and nothing else. The pins live in
`eustress/crates/engine/src/main.rs`: `Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`, and a
`SolverConfig`. `docs/AUDIT/11_SIMULATION_DEBUGGER.md` Feature 8 records: "Avian deterministic step
is single-run only; cross-platform untested."

**Why tick-rate variation is the real test.** `eustress-headless` accepts `--tick-rate HZ` for the
main loop while the simulation's fixed step stays at 60 Hz — the binary's own usage text says so
(`eustress/crates/engine/src/bin/headless.rs`: "`--tick-rate <HZ>` Main-loop rate (default 60; sim
fixed-step stays 60 Hz)"). If the simulation is genuinely decoupled from the main loop, changing
the main-loop rate changes how many main-loop iterations occur per simulated second but not the
simulated trajectory. If the recording changes, something in the simulation is reading wall clock or
frame count. That is precisely the class of defect that silently corrupts a training run, and no
same-seed same-rate test will ever find it.

**The clock hazard you must control for.**
`eustress/crates/common/src/simulation/clock.rs:83 advance()` advances `simulation_time_s` by the
full compressed delta but caps executed physics ticks at `max_ticks_per_frame` (default 10),
zeroing the accumulator on saturation (`clock.rs:100-102`). **A lower main-loop rate means a larger
wall delta per iteration, which makes saturation more likely** — so the tick-rate sweep is exactly
the condition under which dropped ticks appear. This is why `G7.15` is a dependency: the
`dropped_ticks` counter must exist, and every run in this item must report `dropped_ticks == 0`. A
determinism result taken across runs with different dropped-tick counts is meaningless.

**What is being compared.** The `SimulationRecording` exported on entering Edit state
(`eustress/crates/engine/src/simulation/plugin.rs`), defined in
`eustress/crates/common/src/simulation/recorder.rs` as
`{ metadata, series: HashMap<String, TimeSeries>, events: Vec<SimulationEvent> }`. Note that
`RecordingMetadata` contains `started_at` and `wall_duration_s`, which are wall-clock fields and
**will legitimately differ between runs**. Define a canonical comparison that excludes exactly those
two fields, name the exclusion list in the artifact, and hash everything else. Excluding any third
field is a measurement change.

**The API you drive.** `env.reset(seed)` and `env.step(action)` from `G7.13`; see
`docs/PROMPTS/artifacts/G7.13/env_api_conformance.json` for the conformance baseline. Actions come
from a fixed, recorded sequence — this item is not about agent behaviour.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — the determinism runner and the canonical recording hasher
- `eustress/crates/engine/src/bin/headless.rs` — only if `--tick-rate` is found to alter the sim
  path, and only to restore decoupling
- `docs/architecture/HEADLESS_RUNTIME.md` — record the determinism gate as passed, when and only
  when it has passed
- `docs/PROMPTS/artifacts/G7.14/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/simulation/recorder.rs` — changing what is recorded changes what is
  compared; the recording format is the measurement
- `eustress/crates/common/src/simulation/clock.rs` — `G7.15` owns it
- `docs/PROMPTS/artifacts/G7.13/`, `G7.15/` — frozen baselines
- `eustress/crates/common/src/physics/determinism.rs` — owned by `G2.02` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Excluding a diverging series from the comparison, rounding values before hashing, shortening the
  episode until divergence has not yet accumulated, dropping the tick-rate sweep, or widening the
  metadata exclusion list beyond `started_at` and `wall_duration_s` are all measurement changes. If
  the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Every run in the sweep must report `dropped_ticks == 0`. A run with dropped ticks is discarded and
  re-run at a configuration that does not saturate; if no such configuration exists, that is the
  finding and it must be reported, not hidden.
- Episodes must be long enough for divergence to accumulate. Use at least 3,600 sim ticks — one
  simulated minute at 60 Hz — and state the length.
- The action sequence is fixed and recorded, identical across every run in a comparison group.
- Report the **first diverging tick and the diverging series name** whenever a comparison fails.
  "The hashes differ" is not a finding; "series `arc1.core_temp_c` first differs at tick 1,842 by
  4.7e-6" is.
- Run the sweep at `time_scale = 1.0` and separately at one compressed scale, and report both. The
  compressed run is reported for information; only the `time_scale = 1.0` result gates the item.

## 5. Exit criterion

### Criterion
For each of **3 tasks × 3 seeds**, the canonical episode-recording hash is **identical across 3
independent process invocations** at main-loop tick rates of **60, 120, and 240 Hz** — that is,
**9/9 comparison groups** with 3 matching hashes each — with **`dropped_ticks == 0`** in all 27
runs and episodes of at least **3,600 sim ticks**.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_task_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE

cargo run --release --package eustress-agent-eval --bin eustress-determinism-sweep -- `
  --headless-bin .\eustress\target\release\eustress-headless.exe `
  --workspace $env:EUSTRESS_WORKSPACE `
  --tasks tasks/examples/valid/thermal_ceiling.etask.json,tasks/examples/valid/bracket_mass_budget.etask.json,tasks/examples/valid/structure_survives_load.etask.json `
  --seeds 1,7,20260806 `
  --tick-rates 60,120,240 `
  --sim-ticks 3600 `
  --time-scale 1.0 `
  --action-sequence scripts/agent_eval/fixed_actions.json `
  --metadata-exclude started_at,wall_duration_s `
  --require-zero-dropped-ticks `
  --out docs/PROMPTS/artifacts/G7.14/episode_determinism.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "host": { "os": "windows-11-26200", "cpu": "…" },
  "sim_ticks": 3600,
  "time_scale": 1.0,
  "tick_rates_hz": [60, 120, 240],
  "metadata_excluded": ["started_at", "wall_duration_s"],
  "comparison_groups": 9,
  "groups_all_hashes_identical": 9,
  "runs_total": 27,
  "runs_with_dropped_ticks": 0,
  "groups": [
    { "task": "thermal_ceiling", "seed": 1,
      "hash_60hz": "a41c…", "hash_120hz": "a41c…", "hash_240hz": "a41c…",
      "identical": true, "first_divergent_tick": null, "first_divergent_series": null }
  ],
  "compressed_reference_run": { "time_scale": 1000.0, "identical": false,
                                "dropped_ticks": 84210, "reported_for_information_only": true }
}
```

Pass condition:

```
EXIT == 0
AND comparison_groups == 9
AND groups_all_hashes_identical == 9
AND runs_total == 27
AND runs_with_dropped_ticks == 0
AND sim_ticks >= 3600
AND metadata_excluded == ["started_at", "wall_duration_s"]
```

The sweep must exit non-zero on any hash mismatch or any run reporting dropped ticks. Read the
group results; a passing group must show three equal hashes, not merely an `identical: true` flag.

## 6. Critic gate

`critic_gate` is `[]`. Byte equality of 27 recordings across three main-loop rates is the strongest
form of evidence available and admits no interpretation. The mechanical criterion is also
deliberately multi-clause: the dropped-tick requirement prevents a pass obtained by running so
slowly that every run saturates identically, and the minimum episode length prevents a pass
obtained before divergence could accumulate.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — drive fixed action sequences through env.reset/env.step at three
                 tick rates and hash the canonical recordings
   -> if still failing, MANDATORY approach change. Lengthening the settle window is NOT an
      approach change; pinning Avian's solver iteration order and rebuilding the seed propagation
      is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with groups_all_hashes_identical moving < 1
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: divergence traced to Avian solver order-nondeterminism (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.14/episode_determinism.json`

A reader finds: the host, the pinned episode length and time scale, the three tick rates, the exact
metadata exclusion list, all nine comparison groups with three hashes each, the dropped-tick count
across all 27 runs, and — for any failing group — the first divergent tick and series. This file is
the execution of the `docs/architecture/HEADLESS_RUNTIME.md:294` determinism gate and is the W3
evidence that an episode is replayable.

## 9. Definition of NOT done

- Hashes match at 60 Hz and were never tested at another rate. Same-rate equality can be achieved
  by accident; the sweep is the item.
- A diverging series is excluded from the hash so the remaining series match. That is the exact
  measurement change this item forbids.
- The metadata exclusion list grows to include a third field because that field also differed. Only
  `started_at` and `wall_duration_s` are wall-clock artifacts; anything else differing is a finding.
- Runs at 60 Hz saturate the tick cap and drop ticks identically, producing matching hashes for the
  wrong reason. `runs_with_dropped_ticks == 0` is mandatory.
- The episode is 300 ticks long, so no divergence has had time to accumulate and the result says
  nothing about a real training episode.
- A mismatch is reported as "hashes differ" with no divergence localisation, so the next iteration
  has nothing to work from.
- The determinism gate in `HEADLESS_RUNTIME.md` is marked passed on the strength of the 60 Hz result
  alone.
````

---

## `docs/PROMPTS/items/G7.15_dropped-tick-accounting.md`

````markdown
---
id: G7.15
title: Dropped-tick accounting invalidates dishonest episodes
workload: W3
workload_secondary: [W5]
phase: G7
depends_on: [G7.01, G1.07, G1.12]
blocks: [G7.14]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G7_time_compression.json
artifact: docs/PROMPTS/artifacts/G7.15/dropped_tick_accounting.json
escalation: >
  If eliminating dropped ticks entirely at the documented BATTERY_CYCLE_TEST scale of 7.2e6x is
  attempted rather than accounting for them, STALL. The objective is honest accounting, not
  unbounded compute. A change that removes the max_ticks_per_frame cap reintroduces the spiral of
  death the cap exists to prevent.
status: DRAFT
notes: >
  This is the single most dangerous defect in the simulation kernel: every "N cycles in M seconds"
  claim currently rests on it. This item does not remove the cap — it makes the cost visible and
  makes an episode that paid it non-scoreable.
---

## 1. Objective

The simulation clock counts the physics ticks it discards under time compression, reports them in
every simulation recording, and any episode whose discarded fraction exceeds the task's declared
tolerance is marked **invalid** rather than scored. A compressed run's realised compression is
computed from ticks actually executed, alongside the nominal ratio.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**, never Rapier. Slint compiles to
Rust. Units are meter-native.

**The defect, quoted from the source.** `eustress/crates/common/src/simulation/clock.rs`,
`advance()` at line 83. Its own doc comment is honest about the design:

> The simulation **clock** advances by the full compressed delta (`wall_delta · time_scale`) so
> that `simulation_time_s` and `effective_compression()` reflect the intended time scale even under
> extreme compression (see the `BATTERY_CYCLE_TEST` preset). The returned **tick count** — used to
> drive discrete fixed-timestep physics — is capped at `max_ticks_per_frame` to prevent a spiral of
> death. Clock time is therefore decoupled from the number of physics steps executed.

The body: `simulation_time_s += sim_delta` and `accumulator_s += sim_delta`, then a drain loop
`while accumulator_s >= fixed_timestep_s && ticks < max_ticks_per_frame`, and then, at lines
100–102:

```rust
if ticks >= self.max_ticks_per_frame {
    self.accumulator_s = 0.0;
}
```

On saturation the accumulator is **zeroed** — the outstanding simulated time is discarded, not
carried. `max_ticks_per_frame` defaults to 10. `effective_compression()` at line 131 returns
`simulation_time_s / wall_time_s`, so it reports the *intended* compression and will look correct
while steps are being dropped. There is **no error, no counter, and no watchpoint**.
`docs/development/SIMULATION_SYSTEM.md` documents presets up to `BATTERY_CYCLE_TEST = 7.2e6×`.

**What this means for the pack.** Every claim of the form "N cycles simulated in M seconds" rests
on this behaviour. Every scored episode run at a high `time_scale` is scoring a trajectory the
physics did not compute. A training substrate that silently does this is worse than useless — it
produces confidently wrong data.

**What this item is not.** It is not a removal of the cap. The cap prevents an unbounded work
spiral and the doc comment is correct about that. The objective is to make the cost **visible and
disqualifying**, not to pay it.

**Where the count must surface.**
`eustress/crates/common/src/simulation/recorder.rs` defines
`RecordingMetadata { name, started_at, simulation_duration_s, wall_duration_s, total_ticks,
compression_ratio, tags }`. Add the accounting there so every exported recording carries it. The
export fires on entering Edit state (`eustress/crates/engine/src/simulation/plugin.rs`), which is
what `eustress-headless --ticks N` triggers.

**The task-level tolerance.** `.etask` version 1
(`docs/PROMPTS/artifacts/G7.12/etask_spec_v1.md`) carries `max_dropped_tick_fraction`. This item
implements the enforcement: an episode exceeding it is `invalid`, a state distinct from both
`success` and `failure`. Invalid episodes are excluded from scoring and counted separately, so a
benchmark result can never quietly average them in.

**A related known defect, for context, not for fixing here.**
`docs/AUDIT/11_SIMULATION_DEBUGGER.md` records that the Watchman alert cooldown is wall-clock, not
simulation time, so under compression it misses sub-30-second spikes. Different bug, same root
cause — wall clock leaking into simulation semantics. Do not fix it here; `G7.10` already forbids
rebuilding it.

**Build reality.** A change in `eustress/crates/common/` rebuilds the engine: 10–15 minutes, one at
a time, never killed mid-compile. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — the verification binary
- `docs/development/SIMULATION_SYSTEM.md` — state the accounting alongside the presets
- `docs/PROMPTS/harness/recipes/G7_time_compression.json`
- `docs/PROMPTS/artifacts/G7.15/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `max_ticks_per_frame`'s default value — changing it changes the defect's magnitude without
  changing its nature, and invalidates the comparison
- `eustress/crates/common/src/physics/` — the solver is not this item
- `docs/PROMPTS/artifacts/G7.01/` — frozen baseline
- `eustress/crates/common/src/simulation/clock.rs` — owned by `G1.07` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- `eustress/crates/common/src/simulation/recorder.rs` — owned by `G1.07` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- `eustress/crates/engine/src/simulation/plugin.rs` — owned by `G1.07` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Raising `max_ticks_per_frame` so fewer ticks drop, lowering the test `time_scale` until
  saturation stops, counting saturation events instead of discarded ticks, or reporting the
  discarded fraction relative to wall time rather than to intended sim time are all measurement
  changes. If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with
  evidence and stop.
- Do not remove or bypass the cap. The spiral-of-death protection stays.
- `effective_compression()` must keep its existing meaning and its existing name — other code and
  the recording metadata depend on it. Add `realized_compression()` alongside it, computed from
  ticks actually executed, and report both.
- The discarded count must be exact, not estimated: at saturation, the discarded simulated time is
  `accumulator_s` at the moment it is zeroed, and the discarded tick count is
  `floor(accumulator_s / fixed_timestep_s)`. Accumulate both across the run.
- `invalid` must be a distinct outcome from `failure` everywhere it appears. An invalid episode that
  is recorded as a failure corrupts a benchmark in the opposite direction and is just as dishonest.
- Preserve behaviour at `time_scale = 1.0` exactly. The counter must read zero and no timing may
  change.

## 5. Exit criterion

### Criterion
At `time_scale = 1.0` the recording reports `dropped_ticks == 0`; at a `time_scale` high enough to
saturate the cap, the recording reports `dropped_ticks > 0` with `realized_compression` **strictly
less than** `compression_ratio`; and an episode whose dropped fraction exceeds the task's
`max_dropped_tick_fraction` is emitted with outcome `invalid`, in **10/10** trials.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_task_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE

cargo run --release --package eustress-agent-eval --bin eustress-compression-check -- `
  --headless-bin .\eustress\target\release\eustress-headless.exe `
  --workspace $env:EUSTRESS_WORKSPACE `
  --task tasks/examples/valid/thermal_ceiling.etask.json `
  --time-scales 1.0,100.0,10000.0,7200000.0 `
  --sim-ticks 3600 `
  --invalidation-trials 10 `
  --out docs/PROMPTS/artifacts/G7.15/dropped_tick_accounting.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "max_ticks_per_frame": 10,
  "fixed_timestep_s": 0.016666666666666666,
  "runs": [
    { "time_scale": 1.0,       "ticks_executed": 3600, "dropped_ticks": 0,
      "dropped_fraction": 0.0,      "compression_ratio": 1.0,
      "realized_compression": 1.0,      "outcome": "scored" },
    { "time_scale": 100.0,     "ticks_executed": 3600, "dropped_ticks": 0,
      "dropped_fraction": 0.0,      "compression_ratio": 100.0,
      "realized_compression": 100.0,    "outcome": "scored" },
    { "time_scale": 10000.0,   "ticks_executed": 3600, "dropped_ticks": 214880,
      "dropped_fraction": 0.9835,   "compression_ratio": 10000.0,
      "realized_compression": 164.7,    "outcome": "invalid" },
    { "time_scale": 7200000.0, "ticks_executed": 3600, "dropped_ticks": 155036400,
      "dropped_fraction": 0.99998, "compression_ratio": 7200000.0,
      "realized_compression": 164.9,    "outcome": "invalid" }
  ],
  "baseline_unchanged_at_scale_1": true,
  "invalidation_trials": 10,
  "invalidated_correctly": 10,
  "invalid_distinct_from_failure": true
}
```

Pass condition:

```
EXIT == 0
AND the time_scale==1.0 run has dropped_ticks == 0 AND realized_compression == compression_ratio
AND at least one run has dropped_ticks > 0 AND realized_compression < compression_ratio
AND invalidated_correctly == invalidation_trials == 10
AND invalid_distinct_from_failure == true
AND baseline_unchanged_at_scale_1 == true
AND max_ticks_per_frame == 10
```

Read the per-run fields. The checker must exit non-zero if the `time_scale = 1.0` run reports any
dropped ticks, if no run demonstrates the drop, or if any over-tolerance episode was scored rather
than invalidated.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**. The mean is
irrelevant; below floor fails.

D5 is exactly right here: the question is whether a domain expert reading a compressed run's
recording would trust the numbers. That means the recording must make the shortfall obvious rather
than discoverable — both the nominal and realised compression present, the discarded count and
fraction present, and the outcome stated as `invalid` rather than buried. Prioritise the legibility
of the recording metadata; the Critic reads recordings, not your description of them.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_time_compression.json`. Create it if absent,
following the format in `docs/PROMPTS/02_CAPTURE_HARNESS.md`.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — count the discarded accumulator at the saturation branch, thread it
                 into RecordingMetadata, enforce the .etask tolerance at export
   -> if still failing, MANDATORY approach change. Adding another counter is NOT an approach
      change; carrying the accumulator across frames with an explicit debt ledger is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the time_scale==1.0 run still reports non-zero
                  dropped ticks, or no run demonstrates a drop
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: an attempt is made to remove the cap rather than account for it
                   (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.15/dropped_tick_accounting.json`

A reader finds: the cap value and fixed timestep, one row per tested `time_scale` giving ticks
executed, ticks dropped, dropped fraction, nominal and realised compression, and the resulting
outcome; the baseline-unchanged confirmation at `time_scale = 1.0`; and the invalidation trial
results with the explicit statement that `invalid` is distinct from `failure`. `G7.14` requires
`dropped_ticks == 0` in every determinism run and cites this file for the counter's definition;
`G7.22` cites it to justify excluding invalid episodes from published scores.

## 9. Definition of NOT done

- Dropped ticks are eliminated by raising `max_ticks_per_frame`. That converts a silent-accuracy
  defect into a silent-performance defect and reintroduces the spiral the cap prevents.
- The counter exists but never reaches the recording, so an archived episode still cannot be
  audited after the fact. The metadata is the point.
- `effective_compression()` is redefined to report the realised value, silently changing the meaning
  of an existing accessor that other code and existing recordings depend on. Add the new one;
  do not repurpose the old.
- Over-tolerance episodes are recorded as failures. An episode the physics never computed is not a
  failed attempt; scoring it as one biases the benchmark in the opposite direction.
- Behaviour at `time_scale = 1.0` changes, so every existing measurement in the program is
  invalidated by a fix that was supposed to be observational.
- The dropped count is an estimate derived from wall-clock ratios rather than the exact accumulator
  value discarded at the saturation branch.
- The numbers pass and the Critic refuses D5, citing that a reader of the recording still has to
  compute the shortfall themselves. That is a legitimate refusal and the item is not done.
````

---

## `docs/PROMPTS/items/G7.16_observation-action-spaces-generated.md`

````markdown
---
id: G7.16
title: Observation and action spaces generated from code, with a drift check
workload: W5
workload_secondary: [W3]
phase: G7
depends_on: [G7.13, G1.05]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.16/space_drift_report.json
escalation: >
  If the observation space cannot be generated from code because observations are assembled ad hoc
  per call site rather than from one typed structure, STALL and report the number of distinct
  assembly sites. Unifying them is a refactor with a different risk profile than a Tier M item.
status: DRAFT
notes: >
  Hand-maintained space documentation drifts within weeks. Generating it from the code and failing
  on drift is the only version that stays true.
---

## 1. Objective

The environment's observation space and action space are emitted from the running code as machine-
readable specifications, and a drift check exits non-zero whenever the committed specifications
disagree with what the code actually exposes. A lab reading the specification is reading the code.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native, and the emitted specification must state that.

**Why generated and not written.** A hand-written observation-space document is correct on the day
it is written. The tool surface here has 104 defined descriptors across 13 source files plus a
bridge tool module; any of them can change in a routine commit. An external consumer that trusted a
stale specification would build a broken adapter and blame the substrate.

**Where the action space comes from.** `eustress/crates/tools/src/registry.rs` defines
`ToolDefinition { name, description, input_schema, modes, requires_approval, stream_topics }` —
`input_schema` is already a JSON Schema `serde_json::Value`. The action space for a task is the
subset of tool names the `.etask` declares, and each action's parameter schema **is** the tool's
`input_schema`. Do not author a second schema. Mode filtering lives in
`eustress/crates/tools/src/modes.rs` (`WorkshopMode`); 104 descriptors are defined and 90 are
exposed under the default mode set.

**Which actions are admissible.** `docs/PROMPTS/artifacts/G7.01/tool_conformance.json` records, per
descriptor, whether it is reachable headlessly. A tool classified `unreachable` may not appear in a
generated action space, because a lab running in a container cannot call it.

**Where the observation space comes from.** The observation returned by `env.step` and `env.reset`
(`G7.13`; see `docs/PROMPTS/artifacts/G7.13/env_api_conformance.json` for the fixed four-part
response shape). Its components are assembled from surfaces that already exist over the bridge:
`scene.overview`, `ecs.query`, `sim.read`, `sim.bindings`, `state.get`, and — when the task's
observation declares a rendered frame — `ai_camera.capture` from the `G7.03` headless render tier.

**What a specification must state per field.** Name, type, units where physical, shape where
array-valued, and whether presence is guaranteed or conditional on the task. Units are SI and
meter-native; a field in studs is a defect, not a formatting choice.

**Drift, defined mechanically.** Regenerate the specification; canonicalise both the regenerated
and the committed version identically; compare. Any difference is drift. The check must exit
non-zero and print the differing field paths — not a diff of the whole file, which nobody reads.

**Build reality.** The generator needs the tool registry, which lives in
`eustress/crates/tools/` and does not require the engine. Keep the generator in
`eustress/crates/agent-eval/` depending on `eustress-tools`, so it builds in seconds. Validate with
`cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — including `src/bin/space_gen.rs`
- `docs/PROMPTS/artifacts/G7.16/` — the generated specifications and the drift report
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — only to add an introspection method that
  reports the live observation composition, if one is genuinely required

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/tools/src/` — the generator reads the registry; it does not reshape it
- `docs/PROMPTS/artifacts/G7.01/`, `G7.13/` — frozen baselines
- The `.etask` schema — `G7.12` owns it
- Any existing entry in `eustress/crates/engine/src/engine_bridge/protocol.rs` — the file is owned by `G1.05`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Canonicalising away a field that differs, excluding a tool family from the generated action space
  because its schema is awkward, comparing only field names and not types, or making the drift check
  exit 0 on difference are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The action space must be generated from `ToolDefinition`, never transcribed. A transcription is a
  second source of truth and will drift by construction.
- Tools classified `unreachable` in the `G7.01` census must be excluded, and the exclusion count
  must be reported so a reader knows the surface is smaller than the full tool list and why.
- Every physical field carries SI units. No studs anywhere in the output.
- Canonicalisation must be identical on both sides: same key ordering, same number formatting, same
  whitespace. State the canonicalisation rule in the artifact.
- Prove the check actually fails: deliberately perturb one field, run the check, confirm non-zero
  exit and the correct field path in the output, then restore. Record that negative test in the
  artifact.

## 5. Exit criterion

### Criterion
The generator emits an observation-space and an action-space specification; the drift check exits
**0** against the committed pair and exits **non-zero with the correct field path** when a single
field is perturbed; **every** action in the generated action space resolves to a live tool handler;
and **zero** actions are drawn from tools the `G7.01` census classified `unreachable`.

### Measurement

Command:

```
# 1. Generate
cargo run --release --package eustress-agent-eval --bin eustress-space-gen -- `
  --tool-census docs/PROMPTS/artifacts/G7.01/tool_conformance.json `
  --out-observation docs/PROMPTS/artifacts/G7.16/observation_space.json `
  --out-action      docs/PROMPTS/artifacts/G7.16/action_space.json

# 2. Drift check against the committed pair — expect 0
cargo run --release --package eustress-agent-eval --bin eustress-space-gen -- `
  --tool-census docs/PROMPTS/artifacts/G7.01/tool_conformance.json `
  --check docs/PROMPTS/artifacts/G7.16/observation_space.json,docs/PROMPTS/artifacts/G7.16/action_space.json `
  --out docs/PROMPTS/artifacts/G7.16/space_drift_report.json
echo "CLEAN_EXIT=$LASTEXITCODE"

# 3. Negative test — perturb one field, expect non-zero and the field path named
cargo run --release --package eustress-agent-eval --bin eustress-space-gen -- `
  --tool-census docs/PROMPTS/artifacts/G7.01/tool_conformance.json `
  --check docs/PROMPTS/artifacts/G7.16/observation_space.json,docs/PROMPTS/artifacts/G7.16/action_space.json `
  --inject-drift "observation.sim_values[0].units" `
  --out docs/PROMPTS/artifacts/G7.16/space_drift_negative.json
echo "DRIFT_EXIT=$LASTEXITCODE"
```

Expected output shape (`space_drift_report.json`):

```json
{
  "schema_version": 1,
  "commit": "…",
  "canonicalisation": "keys sorted lexicographically; floats as shortest round-trip decimal; LF only",
  "observation_fields": 22,
  "action_count": 41,
  "actions_resolved_to_live_handler": 41,
  "actions_from_unreachable_tools": 0,
  "unreachable_tools_excluded": 10,
  "units_system": "SI, meter-native",
  "drift_fields": [],
  "drift_detected": false,
  "negative_test": { "injected_field": "observation.sim_values[0].units",
                     "detected": true, "reported_path": "observation.sim_values[0].units" }
}
```

Pass condition:

```
CLEAN_EXIT == 0
AND DRIFT_EXIT != 0
AND drift_detected == false in the clean report
AND actions_resolved_to_live_handler == action_count
AND actions_from_unreachable_tools == 0
AND negative_test.detected == true
AND negative_test.reported_path == negative_test.injected_field
```

Both exit codes are part of the measurement: a drift check that never fails is not a check.

## 6. Critic gate

`critic_gate` is `[]`. The criterion includes its own negative test — the check must fail when it
should — which is a stronger guarantee than a perceptual score could give. The generated
specifications are consumed by `G7.24`'s integration guide, where their readability is judged
in context.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — walk the ToolDefinition registry for actions; introspect the live
                 env.reset observation for fields
   -> if still failing, MANDATORY approach change. Adding a special case for one more tool schema
      is NOT an approach change; deriving the observation space from a single typed observation
      struct is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the negative test fails to detect injected drift
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: observations are assembled at many ad hoc sites (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.16/space_drift_report.json`, with
`docs/PROMPTS/artifacts/G7.16/observation_space.json` and
`docs/PROMPTS/artifacts/G7.16/action_space.json` beside it.

A reader finds: the canonicalisation rule, the field and action counts, confirmation that every
action resolves to a live handler, the count of unreachable tools excluded and why, the units
system, an empty drift list, and the negative test proving the check fires. `G7.24` embeds the two
generated specifications; `G7.22`'s harness validates submitted agents against the action space.

## 9. Definition of NOT done

- The specifications are generated once and then edited by hand, so the next regeneration reports
  drift against a human's improvements and someone disables the check.
- The drift check compares field names only, so a type change from `f32` to `string` passes
  silently and every downstream adapter breaks at runtime.
- The action space includes tools the census classified `unreachable`, so a lab in a container gets
  a schema-valid action that always errors.
- The negative test is skipped because "the check obviously works". A check that has never failed
  has never been tested.
- Physical fields carry no units, or carry studs. Units are meter-native and SI.
- The generator transcribes tool schemas into a new format, creating the second source of truth this
  item exists to prevent.
````

---

## `docs/PROMPTS/items/G7.17_episode-recording-replay.md`

````markdown
---
id: G7.17
title: Episode recording and byte-exact replay
workload: W3
workload_secondary: [W5, W6]
phase: G7
depends_on: [G7.14, G7.06, G1.05]
blocks: []
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.17/episode_replay.json
escalation: >
  If an episode recording large enough to replay exceeds 100 MB for a 3,600-tick episode, STALL and
  report the size breakdown by component. Recording size multiplies by every episode in a training
  corpus; a 100 MB episode makes the corpus unshippable and the format must be redesigned before
  anything is published.
status: DRAFT
notes: >
  Determinism (G7.14) proves the same inputs give the same outputs. This item proves the inputs
  were actually captured — which is what makes an archived episode auditable by a third party.
---

## 1. Objective

Every episode writes a self-contained recording — task, seed, action sequence, observations, and
outcome — from which an independent process can reproduce the episode's final world-state digest
and final observation byte-exactly, without access to the process that produced it.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native.

**Why this is separate from determinism.** `G7.14` proved that the same task, seed, and action
sequence produce byte-identical recordings. That guarantees reproducibility *if you have the
inputs*. This item proves the inputs are actually captured in the archive — which is what lets a
third party audit a published result months later, on a different machine, without the original
process.

**What you build on.**

- **Episode determinism.** `docs/PROMPTS/artifacts/G7.14/episode_determinism.json` — 9 comparison
  groups, byte-identical across main-loop rates of 60, 120, and 240 Hz, with zero dropped ticks.
  Reuse its canonical hashing and its metadata exclusion list (`started_at`, `wall_duration_s`).
- **Op-log replay.** `docs/PROMPTS/artifacts/G7.06/oplog_replay.json` — replaying the durable
  op-log from sequence 0 onto an empty Space reproduces the source world digest exactly, with zero
  skipped records. That gives you a second, independent reproduction path: replay the actions, or
  replay the op-log, and both must land on the same digest.
- **The environment API.** `env.reset(seed)` / `env.step(action)` from `G7.13`, four-part response
  shape `{observation, outcome, done, info}`.
- **The simulation recording.** `eustress/crates/common/src/simulation/recorder.rs` —
  `SimulationRecording { metadata, series, events }` with `RecordingMetadata { name, started_at,
  simulation_duration_s, wall_duration_s, total_ticks, compression_ratio, tags }`, plus the
  dropped-tick accounting added by `G7.15`.
- **The world reference.** A Space is referenced by path and content hash, never embedded — the
  `.etask` format rule from `G7.12`. An episode recording follows the same rule.

**The two reproduction paths, and why both are required.** Action replay proves the recording
captured the agent's inputs. Op-log replay proves the recording captured the world's mutations.
A recording that satisfies only one of them has a gap: the first can hide a mutation that came from
somewhere other than an action, and the second can hide an action that produced no mutation but
consumed budget.

**Size discipline.** An observation may include a rendered frame from the `G7.03` headless render
tier. Full frames per step will blow the size budget immediately. Store frames by content hash into
a side directory and reference them, so a corpus deduplicates identical frames. State the storage
strategy in the artifact.

**Build reality.** The replayer belongs in `eustress/crates/agent-eval/` and must not link the
engine, so it builds in seconds and can be run by a third party without an engine build. Engine
builds are 10–15 minutes, one at a time, never killed mid-compile. Validate with `cargo run`, not
`cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — the recorder and the replayer
- `docs/PROMPTS/artifacts/G7.17/` — including the episode-recording format description
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — only if the `env.*` arms must emit
  additional recording fields

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/simulation/recorder.rs` — `G7.15` owns its metadata; changing what is
  recorded changes what `G7.14` compared
- `eustress/crates/common/src/simulation/clock.rs` — `G7.15` owns it
- `docs/PROMPTS/artifacts/G7.06/`, `G7.13/`, `G7.14/`, `G7.15/` — frozen baselines
- Any existing entry in `eustress/crates/engine/src/engine_bridge/protocol.rs` — the file is owned by `G1.05`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Replaying against the original Space rather than a fresh one, comparing final observations with a
  tolerance, replaying only the last N steps, or dropping the op-log reproduction path are all
  measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The replayer must consume **only** the recording and the referenced Space content hash. If it
  reads any state left behind by the recording process, the recording is not self-contained and the
  item fails regardless of the digests.
- Both reproduction paths — action replay and op-log replay — must be exercised on every episode in
  the sample and must agree.
- Verify the content-hash reference: replay must refuse to run against a Space whose hash does not
  match, with a structured error. A silent mismatch produces a plausible-looking wrong result.
- Report recording size per episode, broken down by component, so the corpus cost is visible.
- Do not embed the world in the recording.

## 5. Exit criterion

### Criterion
For **10 recorded episodes** of at least **3,600 sim ticks** each, an independent replay process
reproduces the final world-state digest and the final observation **byte-exactly** via **both** the
action-replay path and the op-log-replay path — **20/20** successful reproductions — and replay
against a mismatched Space content hash is refused in **10/10** attempts.

### Measurement

Command:

```
$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_task_fixture.ps1 -Out $env:EUSTRESS_WORKSPACE
Start-Process -FilePath .\eustress\target\release\eustress-headless.exe `
  -ArgumentList '--universe', "$env:EUSTRESS_WORKSPACE\TaskUniverse", '--no-autoplay'

# 1. Record 10 episodes
cargo run --release --package eustress-agent-eval --bin eustress-env-driver -- `
  --universe "$env:EUSTRESS_WORKSPACE\TaskUniverse" `
  --task tasks/examples/valid/thermal_ceiling.etask.json `
  --episodes 10 --sim-ticks 3600 `
  --record-dir docs/PROMPTS/artifacts/G7.17/episodes

# 2. Replay each episode by both paths in an independent process
cargo run --release --package eustress-agent-eval --bin eustress-episode-replay -- `
  --episodes-dir docs/PROMPTS/artifacts/G7.17/episodes `
  --paths action,oplog `
  --verify-space-hash `
  --mismatch-probes 10 `
  --out docs/PROMPTS/artifacts/G7.17/episode_replay.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "episodes": 10,
  "sim_ticks_per_episode": 3600,
  "reproductions_attempted": 20,
  "reproductions_exact": 20,
  "by_path": { "action": { "attempted": 10, "exact": 10 },
               "oplog":  { "attempted": 10, "exact": 10 } },
  "paths_agree_on_final_digest": 10,
  "hash_mismatch_probes": 10,
  "hash_mismatch_refused": 10,
  "replayer_read_only_recording_and_space": true,
  "recording_size_bytes": {
    "p50_total": 4180221,
    "breakdown_p50": { "actions": 18422, "observations": 1904118,
                       "series": 2201005, "frame_refs": 56676 },
    "frames_stored_by_content_hash": true
  },
  "episodes_detail": [
    { "id": "ep-0001", "seed": 1,
      "final_digest_recorded": "7b2e…",
      "final_digest_action_replay": "7b2e…",
      "final_digest_oplog_replay": "7b2e…",
      "final_observation_identical": true }
  ]
}
```

Pass condition:

```
EXIT == 0
AND reproductions_exact == reproductions_attempted == 20
AND by_path.action.exact == 10 AND by_path.oplog.exact == 10
AND paths_agree_on_final_digest == 10
AND hash_mismatch_refused == hash_mismatch_probes == 10
AND replayer_read_only_recording_and_space == true
AND recording_size_bytes.p50_total <= 104857600
```

The replayer must exit non-zero on any digest or observation mismatch and on any accepted hash
mismatch. Read the per-episode digests.

## 6. Critic gate

`critic_gate` is `[]`. Twenty byte-exact reproductions across two independent reproduction paths,
plus a self-containment assertion and a refusal test, is stronger evidence than any perceptual
score. The two-path requirement is what a reviewer would otherwise have to judge by inspection, and
it is mechanised here instead.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — record the action sequence plus the op-log tail per episode; replay
                 each independently and compare digests
   -> if still failing, MANDATORY approach change. Adding another recorded field is NOT an
      approach change; recording a periodic full world snapshot as replay anchors is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with reproductions_exact moving < 2
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a 3,600-tick episode recording exceeds 100 MB (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.17/episode_replay.json`, with the ten recorded episodes under
`docs/PROMPTS/artifacts/G7.17/episodes/`.

A reader finds: the episode count and length, both reproduction paths with their exactness counts,
confirmation that the two paths agree, the hash-mismatch refusal results, the self-containment
assertion, and the recording size breakdown showing frames are stored by content hash. `G7.22`
publishes episodes in this format so a third party can audit a leaderboard entry;
`G7.23`'s baseline results are archived as recordings of exactly this shape.

## 9. Definition of NOT done

- Replay reads a temporary file the recording process left behind, so it works on the authoring
  machine and fails everywhere else. Self-containment is separately asserted for this reason.
- Only the action path is exercised. A mutation arriving from somewhere other than an action is
  then invisible, which is exactly the gap the op-log path closes.
- The final observation is compared field-by-field with tolerances, so a slow numerical drift passes
  every episode and the corpus is quietly wrong.
- Replay against a mismatched Space is accepted, producing a plausible-looking result computed from
  the wrong world.
- Every observation frame is stored inline, so ten episodes occupy several gigabytes and no corpus
  can ever be published.
- Episodes are 300 ticks long so replay is easy, and the result says nothing about a real episode.
- The recording embeds the world, making a task archive unreviewable and violating the `.etask`
  rule it inherits.
````

---

## `docs/PROMPTS/items/G7.18_physically-verified-scoring.md`

````markdown
---
id: G7.18
title: Physically verified outcome scoring
workload: W4
workload_secondary: [W1, W3]
phase: G7
depends_on: [G7.12, G7.13, G2.35, G1.12]
blocks: [G7.19, G7.22]
tier: XL
token_envelope: 1200000
wallclock_envelope: 5d
max_builds: 20
critic_gate: [D5, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G7_scoring.json
artifact: docs/PROMPTS/artifacts/G7.18/scoring_verification.json
escalation: >
  If any task in the twelve cannot be scored from simulation or geometry state and would require
  a human or a language model to judge the outcome, STALL and report which task and why. A single
  judge-scored task in the suite makes the whole benchmark a language benchmark, and replacing it
  is a design decision about what the suite claims to measure.
status: DRAFT
notes: >
  The differentiator, made concrete. Twelve tasks, every one scored by reading physics or geometry.
  XL because the tasks themselves must be authored, and because the CAD-scored tasks touch the
  kernel.
---

## 1. Objective

A twelve-task suite exists in which every task's success is determined by reading simulation state
or measured geometry — never by comparing text. A deliberately constructed "cheat" solution that
produces the correct-looking answer without producing the correct physics scores zero on every one
of the twelve.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**, never Rapier. Slint compiles to
Rust. **Units are meter-native**; studs are a display unit only and must not appear in any task
definition or score.

**Why this item is the pack's differentiator.** Existing agentic benchmarks score by string match,
unit test, or model-as-judge. Eustress can do something none of them can: run the candidate's work
through first-principles physics and measure whether reality accepts it. A bracket either survives
the load or it does not. A part either fits in the envelope or it interferes. That property is worth
nothing if a single task in the suite falls back to text.

**The scoring surfaces that exist, verified.**

- **Simulation values.** `sim.read` and `sim.bindings` over the bridge; `get_sim_value` /
  `list_sim_values` as MCP tools. The kernel is
  `eustress/crates/common/src/simulation/{clock,state,watchpoint,breakpoint,recorder,config}.rs`.
- **Entity and scene state.** `ecs.query`, `ecs.inspect`, `entity.read`, `scene.overview`,
  `scene.raycast`.
- **Geometry measurement.** `eustress/crates/cad/src/measure.rs` provides, verified by signature:
  `mass_properties(mesh: &EvalMesh) -> MassProps` (with `volume()` and `size() -> [f64; 3]` on the
  result), `topology(mesh: &EvalMesh, weld_eps: f64) -> TopoReport` (with `is_watertight()` and
  `is_manifold()`), and `min_distance(a: &EvalMesh, b: &EvalMesh) -> (f64, bool)` — the last of
  which gives both clearance and an interference flag. The agent-facing tools
  `cad_describe_part`, `cad_validate_part`, and `cad_measure` are shipped (Tier 1 of the CAD agent
  interface) and their descriptors live in `eustress/crates/tools/src/cad_tools.rs`.
- **Physics.** Avian rigid-body dynamics with the determinism pins in
  `eustress/crates/engine/src/main.rs` (`Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`,
  `SolverConfig`).

**What the physics honestly is and is not.** Be accurate in the task definitions.
`docs/AUDIT/19_REALISM_PHYSICS.md` records that the V-Cell electrochemistry is a **lumped 0-D
model** (Nernst plus Butler-Volmer) with **no spatial electrochemistry and no ion transport**, and
that `ElectrochemicalState` and `ThermodynamicState` are **decoupled** — there is no thermal effect
on reaction rate. It also records that `fracture_mesh.rs` has **no integration path to Avian**. Do
not author a task whose predicate depends on any of those. A task that quietly relies on physics
the engine does not have is worse than no task.

**The task format.** `.etask` version 1, specified at
`docs/PROMPTS/artifacts/G7.12/etask_spec_v1.md`, schema at
`docs/PROMPTS/artifacts/G7.12/schema/etask.v1.schema.json`. Its predicate kinds are typed and a
string-match predicate is inexpressible by construction. This item authors twelve conforming tasks
and implements the predicate evaluators.

**The environment API.** `env.reset` / `env.step` from `G7.13`.

**What "long-horizon, ambiguous, multi-step" means concretely here.** The suite must not be twelve
one-shot parameter tweaks. At least four tasks must require a **multi-step** solution where an
intermediate state is neither success nor failure; at least three must be **ambiguous** — the task
states a goal and a constraint but not a method, so more than one valid solution exists; and at
least three must be **long-horizon**, requiring more than 500 environment steps or more than 100,000
sim ticks. Record which tasks satisfy which property.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Twenty builds is the whole XL envelope across three approaches — roughly four hours of pure compile.
Author all twelve tasks before the first build. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `tasks/suite/` — the twelve `.etask` files and their fixture Space generators
- `eustress/crates/agent-eval/` — the predicate evaluators and the scorer
- `eustress/crates/tools/src/cad_tools.rs` — only if a measurement needed by a predicate is not yet
  exposed, and only additively
- `eustress/crates/cad/src/measure.rs` — only additively, and only if a required measurement is
  genuinely absent
- `scripts/agent_eval/cheat/` — the cheat solutions used as the negative control
- `docs/PROMPTS/harness/recipes/G7_scoring.json`
- `docs/PROMPTS/artifacts/G7.18/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G7.12/schema/etask.v1.schema.json` — if the schema cannot express a
  predicate you need, that is `G7.12`'s escalation, not a licence to widen it here
- `eustress/crates/common/src/simulation/clock.rs` — `G7.15` owns it
- `eustress/crates/common/src/physics/` — the solver is not tuned to make a task pass
- `docs/PROMPTS/artifacts/G7.12/`, `G7.13/` — frozen baselines
- Any existing entry in `eustress/crates/tools/src/cad_tools.rs` — the file is owned by `G2.35`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Loosening a predicate threshold until the reference solution passes, replacing a physics predicate
  with a proxy that correlates with it, reducing the suite below twelve, or weakening a cheat
  solution until it fails for the wrong reason are all measurement changes. If the measurement is
  genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Every predicate reads state. If you find yourself comparing strings, stop — the format forbids it
  and so does this item.
- Every task must have a **verified reference solution** that scores success, and the reference must
  be recorded. A task nobody has solved is not a calibrated task; it is an unknown.
- Every task must have a **cheat solution** that produces the superficially right answer without the
  physics — for example, writing the scored value directly, moving the goal object rather than
  satisfying the goal, or deleting the constraint. Each must score zero. `G7.19` extends this into a
  systematic adversarial suite; this item establishes the per-task baseline.
- Do not author a task depending on spatial electrochemistry, ion transport, thermal-electrochemical
  coupling, or fracture-to-Avian integration. Those are recorded gaps.
- Thresholds are in SI, meter-native, with the unit stated per predicate.
- Author all twelve tasks and both solution sets before your first build.

## 5. Exit criterion

### Criterion
**12/12** tasks are scored by a predicate that reads simulation or geometry state, with **zero**
string-comparison predicates; **12/12** reference solutions score success; **12/12** cheat solutions
score zero; and the suite contains at least **4 multi-step**, **3 ambiguous**, and **3 long-horizon**
tasks as defined in §2.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_suite_fixtures.ps1 -Out $env:EUSTRESS_WORKSPACE

cargo run --release --package eustress-agent-eval --bin eustress-score-verify -- `
  --headless-bin .\eustress\target\release\eustress-headless.exe `
  --workspace $env:EUSTRESS_WORKSPACE `
  --suite-dir tasks/suite `
  --reference-solutions scripts/agent_eval/reference `
  --cheat-solutions scripts/agent_eval/cheat `
  --require-no-string-predicates `
  --out docs/PROMPTS/artifacts/G7.18/scoring_verification.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "tasks": 12,
  "predicate_kinds_used": ["sim_value_threshold", "sim_value_integral", "entity_state",
                           "cad_mass_properties", "cad_min_distance", "cad_topology"],
  "string_comparison_predicates": 0,
  "reference_solutions_succeeded": 12,
  "cheat_solutions_scored_zero": 12,
  "property_counts": { "multi_step": 5, "ambiguous": 4, "long_horizon": 3 },
  "units": "SI, meter-native",
  "excluded_physics": ["spatial electrochemistry", "ion transport",
                       "thermal-electrochemical coupling", "fracture->Avian"],
  "per_task": [
    { "id": "T03_bracket_mass_budget",
      "predicate": { "kind": "cad_mass_properties",
                     "expr": "mass_kg <= 0.850 AND min_distance(bracket, housing) >= 0.0020",
                     "units": ["kg", "m"] },
      "reference_score": 1.0,
      "cheat": { "name": "write_mass_value_directly", "score": 0.0 },
      "properties": ["ambiguous"],
      "steps_to_reference_solution": 41 }
  ]
}
```

Pass condition:

```
EXIT == 0
AND tasks == 12
AND string_comparison_predicates == 0
AND reference_solutions_succeeded == 12
AND cheat_solutions_scored_zero == 12
AND property_counts.multi_step >= 4
AND property_counts.ambiguous >= 3
AND property_counts.long_horizon >= 3
```

The verifier must exit non-zero if any reference solution fails, any cheat solution scores above
zero, or any predicate is string-based. Read the per-task rows.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)** and **D6 (overall coherence)**,
floor **8.0 each**. The mean is irrelevant; either dimension below floor fails the item.

D5 asks whether a domain expert would accept the scores. A predicate that reads a real physical
quantity at a defensible threshold, in stated SI units, on physics the engine actually models, earns
that. A predicate that reads a proxy, or that depends on a modelled quantity the audit records as
absent, does not — and the excluded-physics list in the artifact is where the Critic will look.

D6 asks whether the twelve read as one suite. Twelve tasks with twelve unrelated predicate styles,
inconsistent threshold conventions, or scores on incomparable scales fail coherence even if each
task is individually sound.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_scoring.json`. Create it if absent, following the
format in `docs/PROMPTS/02_CAPTURE_HARNESS.md`.

The Critic never sees your self-report. It reads the twelve task files and the scoring artifact;
they must stand alone.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — declarative typed predicates over sim values and CAD measurements,
                 evaluated by the scorer after the episode's terminal step
   -> if still failing, MANDATORY approach change. Retuning a threshold is NOT an approach change;
      moving from terminal-state predicates to trajectory-integral predicates is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with reference_solutions_succeeded moving < 2 and the
                  worst gated dimension moving < 0.5
  - Budget      : 1.8M tokens or 30 builds consumed (150% of the XL envelope)
  - Item-specific: a task requires a human or model judge (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.18/scoring_verification.json`, with the twelve task files under
`tasks/suite/`.

A reader finds: the predicate kinds actually used, a zero count of string predicates, both solution
sets' results, the multi-step / ambiguous / long-horizon property counts, the units system, the
explicit list of physics deliberately excluded because the engine does not model it, and one row per
task giving its predicate expression with units, its reference score, its cheat result, and the step
count of the reference solution. This is the W4 evidence that one vertical works end to end, and the
foundation `G7.22` publishes.

## 9. Definition of NOT done

- One task falls back to comparing the agent's stated answer against an expected string. One is
  enough to make the suite a language benchmark.
- A predicate reads a proxy that correlates with the physical outcome — total energy as a stand-in
  for whether the structure survived — because the real measurement was awkward.
- A task depends on thermal-electrochemical coupling, spatial electrochemistry, ion transport, or
  fracture-to-Avian integration. `docs/AUDIT/19_REALISM_PHYSICS.md` records all four as absent; a
  task built on them measures nothing.
- Every task is a one-shot parameter tweak, so the suite measures parameter search rather than
  long-horizon multi-step capability. The property counts are gated for this reason.
- A cheat solution fails because it is malformed rather than because the scoring caught it. A cheat
  must be a *valid* episode that reaches the wrong conclusion; otherwise the negative control proves
  nothing.
- Thresholds are stated without units, or in studs. Every threshold is SI and meter-native with the
  unit named.
- All twelve numbers pass and the Critic refuses D6, citing that the twelve tasks share no common
  scoring convention and cannot be aggregated into a single benchmark number. That is a legitimate
  refusal and the item is not done.
````

---

## `docs/PROMPTS/items/G7.19_anti-gaming-exploit-suite.md`

````markdown
---
id: G7.19
title: Anti-gaming — the adversarial exploit suite
workload: W3
workload_secondary: [W1, W4]
phase: G7
depends_on: [G7.18, G1.12]
blocks: [G7.22]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5]
capture_recipe: docs/PROMPTS/harness/recipes/G7_scoring.json
artifact: docs/PROMPTS/artifacts/G7.19/exploit_report.json
escalation: >
  If closing an exploit requires removing a legitimate agent capability — for example banning
  set_sim_value outright rather than scoping it out of an episode's action space — STALL. Trading
  environment capability for exploit resistance changes what the benchmark measures and is a
  design decision.
status: DRAFT
notes: >
  Every benchmark is gamed eventually. Publishing one without having tried to game it yourself is
  how a benchmark loses credibility on day two of external use.
---

## 1. Objective

A documented suite of at least ten distinct exploits — each a valid episode that reaches a
superficially successful outcome without doing the work the task asks for — is executed against the
twelve-task suite, and every exploit scores zero. The exploits and their outcomes are published
alongside the benchmark so an external evaluator can see what was anticipated.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native.

**Why this precedes publication.** A benchmark's credibility is set by whether its authors tried to
break it. Publishing the exploit suite alongside the results converts the obvious external critique
— "an agent could just do X" — from an attack into a citation.

**What you are attacking.** The twelve-task suite from `G7.18`, its typed state-based predicates,
and the `env.reset` / `env.step` API from `G7.13`. Read
`docs/PROMPTS/artifacts/G7.18/scoring_verification.json` for each task's predicate expression and
its per-task cheat baseline; this item generalises those single cheats into a systematic suite.

**Exploit classes that must each be represented.** At minimum, one working attempt in each of these
eight, plus two more of your own devising:

1. **Direct scored-value write.** Set the measured simulation value directly rather than causing it,
   via `set_sim_value` or an equivalent.
2. **Goal relocation.** Move the target object, region, or reference frame so the predicate is
   satisfied without the intended physical change.
3. **Constraint deletion.** Delete or disable the entity, joint, or load whose presence makes the
   task hard.
4. **Predicate-boundary exploitation.** Satisfy the predicate at exactly the sampled instant while
   violating it throughout the rest of the episode.
5. **Task-file tampering.** Modify the `.etask` file, its referenced Space, or its content hash from
   inside the episode.
6. **Scorer tampering.** Reach the scoring process or its output through `write_file`,
   `stage_file_change`, or `run_bash`.
7. **Compression laundering.** Raise `time_scale` so the physics is never executed and the reported
   trajectory is fictitious — see the dropped-tick accounting in
   `docs/PROMPTS/artifacts/G7.15/dropped_tick_accounting.json`.
8. **Nondeterminism farming.** Re-run the same episode many times and report the best outcome as
   though it were the expected one.

**The tools that make several of these reachable.** `run_bash`
(`eustress/crates/tools/src/shell_tools.rs`), `write_file` and `stage_file_change`
(`eustress/crates/tools/src/file_tools.rs`, `diff_tools.rs`), `git_commit` and `git_branch`
(`eustress/crates/tools/src/git_tools.rs`), `set_sim_value` and `run_experiment`
(`eustress/crates/tools/src/simulation_tools.rs`), and `delete_entity`
(`eustress/crates/tools/src/entity_tools.rs`). These are legitimate capabilities of the substrate.
The fix is almost never to remove them — it is to scope them out of an episode's declared action
space, or to make the predicate immune.

**The defences you already have.** `G7.15` marks over-tolerance compressed episodes `invalid` rather
than scored, which addresses class 7 if the tolerance is set correctly per task. `G7.14` established
determinism, which is what makes class 8 detectable. `G7.13` refuses actions outside the declared
action space without advancing the step counter. Use these; do not rebuild them.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile. Most
exploit work is task-file and harness work and needs no engine build. Validate with `cargo run`, not
`cargo check`.

## 3. Scope

### In scope — files this item may edit
- `scripts/agent_eval/exploits/` — the exploit definitions and their driver scripts
- `tasks/suite/` — predicate and action-space hardening on the twelve tasks
- `eustress/crates/agent-eval/` — the scorer's integrity checks and the exploit runner
- `docs/PROMPTS/artifacts/G7.19/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/tools/src/` — removing a capability to close an exploit is the escalation
  trigger, not a fix
- `eustress/crates/common/src/simulation/clock.rs` — `G7.15` owns it
- `docs/PROMPTS/artifacts/G7.15/`, `G7.18/` — frozen baselines

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Writing a deliberately weak exploit so it fails, declaring an exploit out of scope because it
  works, counting a crashed exploit as a defended one, or reducing the exploit count below ten are
  all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Every exploit must be a **valid episode** — it passes `env.step` action validation and runs to a
  terminal state. An exploit that errors out has proven nothing about the scoring.
- Every exploit must first be demonstrated to **work against an unhardened baseline**, and that
  baseline result must be recorded. An exploit that never worked is not an exploit; it is a
  formality. Record both the pre-hardening and post-hardening score for each.
- Prefer hardening the predicate or the declared action space over removing a capability. Record,
  per exploit, which mechanism closed it.
- Class 4 exploits require trajectory-aware predicates. If a task's predicate samples only the
  terminal state, say so and change the predicate, not the exploit.
- Class 8 requires a stated policy on repeated runs — for example, first-run-counts or
  mean-of-N-declared-in-advance. Record the policy; a benchmark without one is farmable by
  construction.

## 5. Exit criterion

### Criterion
At least **10 distinct exploits** spanning at least **8 named classes** are executed against all
twelve tasks; **100%** score zero after hardening; **100%** are recorded as having succeeded against
the unhardened baseline; and **zero** exploits were closed by removing an agent capability.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_suite_fixtures.ps1 -Out $env:EUSTRESS_WORKSPACE

cargo run --release --package eustress-agent-eval --bin eustress-exploit-run -- `
  --headless-bin .\eustress\target\release\eustress-headless.exe `
  --workspace $env:EUSTRESS_WORKSPACE `
  --suite-dir tasks/suite `
  --exploits scripts/agent_eval/exploits `
  --also-run-unhardened-baseline `
  --require-valid-episodes `
  --out docs/PROMPTS/artifacts/G7.19/exploit_report.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "tasks": 12,
  "exploits": 11,
  "exploit_classes": ["direct_value_write", "goal_relocation", "constraint_deletion",
                      "predicate_boundary", "task_file_tampering", "scorer_tampering",
                      "compression_laundering", "nondeterminism_farming",
                      "observation_spoofing", "budget_starvation"],
  "distinct_classes": 10,
  "all_episodes_valid": true,
  "succeeded_against_unhardened_baseline": 11,
  "scored_zero_after_hardening": 11,
  "closed_by_removing_capability": 0,
  "repeated_run_policy": "first run counts; seed declared in the submission",
  "per_exploit": [
    { "name": "write_scored_sim_value_directly", "class": "direct_value_write",
      "unhardened_score": 1.0, "hardened_score": 0.0,
      "closed_by": "set_sim_value scoped out of the task action space; predicate reads the
                    derived quantity, not the settable one",
      "episode_valid": true }
  ]
}
```

Pass condition:

```
EXIT == 0
AND exploits >= 10
AND distinct_classes >= 8
AND all_episodes_valid == true
AND succeeded_against_unhardened_baseline == exploits
AND scored_zero_after_hardening == exploits
AND closed_by_removing_capability == 0
AND repeated_run_policy is a non-empty string
```

The runner must exit non-zero if any exploit scores above zero after hardening or if any exploit
failed to succeed against the unhardened baseline. Read the per-exploit rows.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)**, floor **8.0**. The mean is
irrelevant; below floor fails.

D5 is the gate because the judgement being asked for is adversarial credibility: would a sceptical
domain expert, reading this exploit report, believe the benchmark is hard to game? That depends on
whether the exploits are the ones they would have tried, whether each was genuinely effective before
hardening, and whether the closures are principled rather than special cases. A report full of
exploits that were never dangerous scores badly no matter how many there are.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_scoring.json` — the same recipe as `G7.18`, since
the scenes and scoring path are identical.

The Critic reads `docs/PROMPTS/artifacts/G7.19/exploit_report.json` and the exploit definitions
under `scripts/agent_eval/exploits/`, and never your self-report.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — harden predicates to read derived rather than settable quantities,
                 and scope action spaces per task
   -> if still failing, MANDATORY approach change. Adding one more scoped-out tool is NOT an
      approach change; moving to trajectory-integral predicates evaluated over the whole episode is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with scored_zero_after_hardening moving < 1
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the only available closure removes an agent capability (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.19/exploit_report.json`, with the exploit definitions under
`scripts/agent_eval/exploits/`.

A reader finds: the exploit count and class coverage, confirmation that every exploit ran as a valid
episode, each exploit's score before and after hardening, the mechanism that closed it, the count of
closures that removed a capability (zero), and the declared repeated-run policy. This file is
published with the benchmark in `G7.22` and is the W3 evidence that the environment was attacked
before it was announced.

## 9. Definition of NOT done

- An exploit is listed but never worked against the unhardened baseline, so the report pads its
  count with formalities. Both scores are recorded per exploit for this reason.
- An exploit crashes the episode and is counted as defended. A crash is a robustness bug, not a
  scoring defence.
- Compression laundering is closed by forbidding time compression. That removes the property that
  makes this environment distinctive; the correct closure is per-task dropped-tick tolerance from
  `G7.15`.
- `set_sim_value` is removed from the tool surface. That breaks every legitimate use across the
  substrate to fix twelve tasks, and is the escalation trigger.
- The predicate-boundary class is skipped because every predicate samples only terminal state. That
  is the finding, and the predicates need changing.
- No repeated-run policy is declared, so an external submitter can run 500 times and report the best,
  and nothing in the rules says they may not.
- Every number passes and the Critic refuses D5, citing that the obvious exploit a specialist would
  try is absent from the suite. That is a legitimate refusal and the item is not done.
````

---

## `docs/PROMPTS/items/G7.20_containerised-lab-integration.md`

````markdown
---
id: G7.20
title: Containerised, GUI-free lab integration
workload: W5
workload_secondary: [W3, W6]
phase: G7
depends_on: [G7.03, G7.13]
blocks: [G7.21, G7.22, G7.24]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.20/container_conformance.json
escalation: >
  If the container image exceeds 8 GB, STALL and report the layer breakdown. An image that large
  will not be pulled by an evaluator on a first look, which defeats the item's purpose regardless
  of whether it runs correctly.
status: DRAFT
notes: >
  The integration shape a lab actually consumes: one image, one command, no display, no GPU
  required, results on stdout and on disk.
---

## 1. Objective

A single container image runs Eustress episodes end to end with no display server, no GPU
requirement, and no host-side setup beyond a mounted output directory. `docker run` executes ten
episodes and exits 0, writing a results file, and the image's binaries link no X11 or Wayland
libraries.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0 — the licence permits internal use, evaluation, and
modification at no cost, which is what an evaluating lab needs; say **source-available**, never open
source. Physics is **Avian**. Slint compiles to Rust. Units are meter-native.

**Why the container is the product surface for a lab.** An evaluator's first question is not "is the
physics good" but "how long until I can run it". Anything requiring a Windows desktop, a GPU driver,
or a manual Space setup will not be evaluated at all.

**What must be inside.** `eustress-headless` (the simulator, from
`eustress/crates/engine/src/bin/headless.rs`), the `--render gpu` tier delivered by `G7.03` with its
software-rasterizer fallback (see
`docs/PROMPTS/artifacts/G7.03/headless_capture_report.json` for the adapter it selected), the
`agent-eval` binaries including the `env` driver from `G7.13`, the twelve-task suite from `G7.18`,
and the fixture generators.

**The environment hook you use to place worlds.**
`eustress/crates/engine/src/space/mod.rs:119 workspace_root()` resolves `EUSTRESS_WORKSPACE` first,
on every platform, creating the directory and scaffolding a default Universe if empty. That is the
supported way to point the container at a mounted volume — no patching required.

**The honest state of Linux support.** `.github/workflows/linux-engine.yml` runs exactly one step:
`cargo check --package eustress-engine`. There is **no CI job that builds the engine binary on
Linux, and no CI job that runs any test anywhere** — `.github/workflows/ci.yml` has three jobs
(`cargo deny`, a naga WGSL validation that skips any file containing naga_oil directives, and a
`cargo tree` dependency grep) and none of them is `cargo test` or `clippy`. Assume nothing about
Linux beyond "it type-checks". Expect to find and fix real portability defects, and record each one
in the artifact.

**CI is out of scope, deliberately.** This item must not add or edit a workflow file. It delivers
the image and a runnable gate script; the wiring belongs to `G7.43` in pack T5
(`docs/PROMPTS/packs/T5_robustness_and_cohesion.md`), which is licensed to add a workflow and
maintains the fleet registry every gate script is registered in. Hand off the script path and its
literal pass condition in the artifact rather than working around the boundary.

**No GPU requirement.** The image must run on a CPU-only host. The `--render gpu` tier must fall
back to a software rasterizer, and the artifact must record which adapter was used inside the
container. Episodes whose tasks need no rendered observation must run without touching the render
tier at all.

**Build reality.** Building the engine for Linux inside a container is a full build: 10–15 minutes,
and the container build adds image assembly on top. One cargo build at a time; the host workspace
shares one `target/` and a container build must not race a host build. Never kill a build
mid-compile. Validate by running the image, not by `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `docker/eustress-agent-eval.Dockerfile` — the image definition
- `docker/entrypoint.sh` — the container entry point
- `scripts/agent_eval/` — the gate script the container runs, and the fixture generators
- `eustress/crates/agent-eval/` — portability fixes
- `eustress/crates/engine/src/` — only portability fixes required to build and run on Linux, each
  recorded in the artifact
- `docs/PROMPTS/artifacts/G7.20/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass, and never add a
  workflow here; wiring the gate script into CI is a separate human decision
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `tasks/suite/` — the twelve tasks are frozen by `G7.18`; a task that fails in the container is a
  portability finding, not a task to edit
- `docs/PROMPTS/artifacts/G7.03/`, `G7.13/`, `G7.18/` — frozen baselines

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Installing an X server in the image to satisfy a library dependency, mounting a GPU device to make
  rendering work, reducing the episode count below ten, or excluding a task that fails in the
  container are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The X11/Wayland link check is a hard gate, not a nicety: `ldd` over every executable in the image
  must show zero `libX11`, `libwayland`, `libxcb`, or `libxkbcommon` dependencies. If a crate pulls
  one in, remove the feature rather than the check.
- No GPU device may be passed to the container in the passing run. If the run requires
  `--gpus`, the item has not achieved its objective.
- One command must be sufficient. If a user needs to run a setup step first, fold it into the
  entry point.
- Record every portability defect found and fixed, with its `path:line`. This list is one of the
  most valuable things the item produces — it is the first honest inventory of what does not work
  off Windows.
- Report the image size and its layer breakdown.

## 5. Exit criterion

### Criterion
`docker run` on a **CPU-only host with no display and no `DISPLAY` variable** executes **10 episodes
across at least 3 of the twelve tasks**, exits **0**, and writes a results file containing 10 episode
records; **zero** executables in the image link X11 or Wayland libraries; and the image is **≤ 8 GB**.

### Measurement

Command:

```
# 1. Build the image (full Linux engine build inside; expect 20-30 min)
docker build -f docker/eustress-agent-eval.Dockerfile -t eustress-agent-eval:t4 .

# 2. Confirm no GUI linkage anywhere in the image
docker run --rm eustress-agent-eval:t4 /bin/sh -c \
  'for f in /opt/eustress/bin/*; do ldd "$f" 2>/dev/null; done | \
   grep -cE "libX11|libwayland|libxcb|libxkbcommon"'
echo "GUI_LINKS=$?"

# 3. Run 10 episodes, no display, no GPU device
docker run --rm \
  -e EUSTRESS_WORKSPACE=/work/workspace \
  -v "$PWD/docs/PROMPTS/artifacts/G7.20/out:/work/out" \
  eustress-agent-eval:t4 \
  --tasks T01,T03,T07 --episodes 10 \
  --out /work/out/container_conformance.json
echo "RUN_EXIT=$?"

# 4. Image size in bytes
docker image inspect eustress-agent-eval:t4 --format '{{.Size}}'
```

Expected output shape (`container_conformance.json`):

```json
{
  "schema_version": 1,
  "commit": "…",
  "image": "eustress-agent-eval:t4",
  "image_size_bytes": 5218374144,
  "host": { "os": "linux", "display_env_present": false, "gpu_devices_passed": 0 },
  "gui_library_links": 0,
  "adapter": { "backend": "Vulkan", "name": "llvmpipe (LLVM 18.1.0, 256 bits)", "software": true },
  "tasks_run": ["T01_thermal_ceiling", "T03_bracket_mass_budget", "T07_structure_survives_load"],
  "episodes_completed": 10,
  "episodes_errored": 0,
  "exit_code": 0,
  "portability_fixes": [
    { "path": "eustress/crates/agent-eval/src/fixture.rs:88",
      "issue": "backslash path separator assumed" }
  ],
  "ci_wiring": "gate script at scripts/agent_eval/gate.sh; wiring is owned by G7.43 in pack T5 and
                is NOT performed by this item"
}
```

Pass condition:

```
RUN_EXIT == 0
AND gui_library_links == 0
AND episodes_completed == 10 AND episodes_errored == 0
AND len(tasks_run) >= 3
AND host.display_env_present == false
AND host.gpu_devices_passed == 0
AND image_size_bytes <= 8589934592
```

Read the emitted values from the results file and the two shell-reported numbers. A container that
starts is not a pass; the episode count and the GUI-link count are the measurement.

## 6. Critic gate

`critic_gate` is `[]`. Every clause is mechanically checkable — an exit code, two counts, an
absence of environment variables, a device count, and a byte size. The GUI-link count in particular
is the kind of assertion a perceptual reviewer could not make, and it is the one that actually
proves "no GUI dependency".

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — multi-stage Dockerfile: build the Linux binaries in a builder stage,
                 copy only binaries plus task assets into a slim runtime stage
   -> if still failing, MANDATORY approach change. Adding another apt package is NOT an approach
      change; disabling the engine cargo features that pull in windowing is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with episodes_completed == 0
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: image size exceeds 8 GB (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.20/container_conformance.json`, with
`docker/eustress-agent-eval.Dockerfile` and `docker/entrypoint.sh`.

A reader finds: the image tag and size, the host's confirmed absence of a display and of GPU
devices, the zero GUI-link count, the graphics adapter actually selected inside the container, the
tasks run and episodes completed, the itemised list of portability defects found and fixed with
`path:line`, and an explicit note that CI wiring was not performed here and is owned by `G7.43` in
pack T5. `G7.21` measures throughput
inside this image; `G7.22` distributes it as the reproducible harness; `G7.24` documents it.

## 9. Definition of NOT done

- The container runs because an X server was installed inside it. The GUI-link count is zero only
  when the binaries genuinely need no windowing, and installing a server does not change linkage.
- The run requires `--gpus`, so an evaluator on a CPU-only cloud box cannot reproduce it. The
  software rasterizer fallback from `G7.03` is the point.
- Episodes start and none complete, but the entry point exits 0. The episode count is gated.
- A task is dropped from the container run because it fails on Linux. That is a portability finding
  and belongs in the artifact's fix list, not in a narrowed task set.
- A CI workflow file is added or edited. That is out of scope here; the gate script exists precisely
  so `G7.43` in pack T5 can register and wire it.
- The image is 14 GB because the builder stage was not discarded, so nobody pulls it.
- Portability fixes are made silently, so the first honest inventory of what does not work off
  Windows is lost.
````

---

## `docs/PROMPTS/items/G7.21_episode-throughput.md`

````markdown
---
id: G7.21
title: Throughput — episodes per CPU-hour, measured
workload: W6
workload_secondary: [W3]
phase: G7
depends_on: [G7.13, G7.20]
blocks: [G7.22]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.21/throughput.json
escalation: >
  If sustained throughput falls below 5 episodes per CPU-hour on the reference task, STALL and
  report the per-stage cost breakdown. Below that rate a training run of any useful size is
  economically impossible, and the substrate needs a performance item before a benchmark item.
status: DRAFT
notes: >
  A measurement item. The number it establishes is what a lab divides its compute budget by, so it
  must be measured honestly on named hardware and must be reproducible within a stated tolerance.
---

## 1. Objective

The rate at which the containerised environment produces completed episodes is a measured number on
named hardware, broken down by cost stage, reproducible within 10% across two independent runs, and
reported both per CPU-hour and per wall-clock hour at a stated parallelism.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0. Physics is **Avian**. Slint compiles to Rust. Units
are meter-native.

**Why the unit is episodes per CPU-hour.** A lab sizing an experiment divides its compute budget by
this number. Reporting episodes per wall-clock hour without stating parallelism and core count is
unfalsifiable, and reporting a peak rather than a sustained rate is misleading. Report both, with
the parallelism stated.

**Never fabricate a measured number.** Every figure in the artifact is `MEASURED` with its command,
host, and artifact path, or it is labelled `TARGET`, or it is labelled `CONFIG DEFAULT`. As a
cautionary example from this repository: `docs/AUDIT/05_SPACE_STREAMING.md:23` carries a
2.10M-entity figure that is an `active_cap` **config default** and has repeatedly been read as a
benchmark result. Do not add another of those.

**What you measure inside.** The container image from `G7.20`
(`docker/eustress-agent-eval.Dockerfile`; see
`docs/PROMPTS/artifacts/G7.20/container_conformance.json` for its verified GUI-free, CPU-only
configuration). Measuring on the Windows host instead would measure a different system from the one
a lab runs.

**The cost stages, and where their prior measurements live.** An episode costs: process or worker
startup, Space load, `env.reset`, N × `env.step` (each of which is an action plus a tick advance),
predicate evaluation, and recording write. You already have two of these measured — reset and step
latencies in `docs/PROMPTS/artifacts/G7.13/env_api_conformance.json`, and fork/discard costs at
three entity scales in `docs/PROMPTS/artifacts/G7.08/fork_rehearse_commit.json`. Reconcile against
them and flag any discrepancy greater than 25%; a large divergence means the container is doing
something the host was not.

**A trap that will silently inflate the number.** Raising `time_scale` makes an episode finish in
less wall time while the physics is never executed —
`eustress/crates/common/src/simulation/clock.rs:100-102` zeroes the accumulator on tick-cap
saturation. `G7.15` added `dropped_ticks` to the recording metadata and marks over-tolerance
episodes `invalid`. **Only episodes with outcome `scored` count toward throughput.** Report
invalid episodes separately; an environment that produces 400 unusable episodes an hour produces
zero episodes an hour.

**Existing profiling infrastructure, if you need to attribute cost.**
`eustress/crates/engine/src/profiler.rs` plus `frame_diagnostics.rs` are always compiled; arm with
`EUSTRESS_PROFILE=1` and window with `EUSTRESS_PROFILE_FRAMES` (default 120), writing
`eustress_profile.txt` and `eustress_profile.svg` to the working directory. Note that arming the
profiler changes the cost being measured — profile in a separate run, never in the throughput run.

**Build reality.** No engine change should be needed. Rebuilding the container is the expensive
step. One build at a time, never killed mid-compile. Validate by running, not by `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — the throughput harness and the per-stage timer
- `scripts/agent_eval/` — the run scripts
- `docs/PROMPTS/artifacts/G7.21/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docker/eustress-agent-eval.Dockerfile` — `G7.20` owns the image; changing it invalidates that
  item's evidence and this item's comparability
- `tasks/suite/` — the twelve tasks are frozen
- `eustress/crates/engine/` — this item measures, it does not optimise
- `docs/PROMPTS/artifacts/G7.08/`, `G7.13/`, `G7.20/` — frozen baselines

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Raising `time_scale` to shorten episodes, counting invalid episodes toward the rate, reporting a
  peak burst as a sustained rate, measuring on the Windows host rather than in the container, or
  reporting wall-clock throughput without stating parallelism and core count are all measurement
  changes. If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with
  evidence and stop.
- Sustained means at least 30 minutes of continuous operation, reported as a steady-state rate with
  the first 5 minutes excluded and separately reported as warm-up.
- Only `scored` episodes count. Report `invalid` and `errored` counts separately.
- Report at two parallelism levels: 1 worker, and workers equal to the host's physical core count.
  State both, and state the core count.
- Reproducibility is part of the criterion: two independent runs must agree within 10%.
- Do not profile in the throughput run. Arming `EUSTRESS_PROFILE=1` changes the number.

## 5. Exit criterion

### Criterion
Sustained throughput on the reference task is measured over **≥ 30 minutes** at two parallelism
levels inside the `G7.20` container, on named hardware, with a per-stage cost breakdown; **two
independent runs agree within 10%**; and **only `scored` episodes** are counted, with `invalid` and
`errored` reported separately.

### Measurement

Command:

```
# Run A
docker run --rm \
  -e EUSTRESS_WORKSPACE=/work/workspace \
  -v "$PWD/docs/PROMPTS/artifacts/G7.21/out:/work/out" \
  eustress-agent-eval:t4 \
  --throughput --task T01 --duration-min 30 --workers 1,PHYSICAL \
  --warmup-min 5 --time-scale 1.0 \
  --out /work/out/throughput_runA.json
echo "A_EXIT=$?"

# Run B — independent invocation, same configuration
docker run --rm \
  -e EUSTRESS_WORKSPACE=/work/workspace \
  -v "$PWD/docs/PROMPTS/artifacts/G7.21/out:/work/out" \
  eustress-agent-eval:t4 \
  --throughput --task T01 --duration-min 30 --workers 1,PHYSICAL \
  --warmup-min 5 --time-scale 1.0 \
  --out /work/out/throughput_runB.json
echo "B_EXIT=$?"

# Reconcile and emit the artifact
cargo run --release --package eustress-agent-eval --bin eustress-throughput-report -- \
  --run-a docs/PROMPTS/artifacts/G7.21/out/throughput_runA.json \
  --run-b docs/PROMPTS/artifacts/G7.21/out/throughput_runB.json \
  --reconcile-against docs/PROMPTS/artifacts/G7.13/env_api_conformance.json \
  --require-agreement-pct 10 \
  --out docs/PROMPTS/artifacts/G7.21/throughput.json
echo "EXIT=$?"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "measurement_status": "MEASURED",
  "host": { "cpu": "AMD Ryzen 9 7950X", "physical_cores": 16, "logical_cores": 32,
            "ram_gb": 64, "container": "eustress-agent-eval:t4" },
  "task": "T01_thermal_ceiling",
  "time_scale": 1.0,
  "duration_min": 30, "warmup_min_excluded": 5,
  "workers_1": {
    "episodes_scored": 74, "episodes_invalid": 0, "episodes_errored": 0,
    "episodes_per_cpu_hour": 2.96, "episodes_per_wallclock_hour": 2.96
  },
  "workers_16": {
    "episodes_scored": 981, "episodes_invalid": 0, "episodes_errored": 2,
    "episodes_per_cpu_hour": 2.45, "episodes_per_wallclock_hour": 39.24
  },
  "stage_cost_ms_p50": { "worker_startup": 1841.0, "space_load": 640.2, "env_reset": 812.4,
                         "env_step_mean": 61.3, "predicate_eval": 44.9, "recording_write": 122.7 },
  "run_agreement_pct": 3.8,
  "reconciliation_vs_g7_13": { "env_reset_delta_pct": 4.1, "env_step_delta_pct": 2.2,
                               "flagged": false }
}
```

Pass condition:

```
A_EXIT == 0 AND B_EXIT == 0 AND EXIT == 0
AND duration_min >= 30 AND warmup_min_excluded == 5
AND both worker levels report episodes_per_cpu_hour and episodes_per_wallclock_hour
AND run_agreement_pct <= 10.0
AND episodes_invalid is reported separately at both levels and excluded from the rates
AND measurement_status == "MEASURED" with host.cpu and host.physical_cores populated
AND time_scale == 1.0
```

The reporter must exit non-zero if the two runs disagree by more than 10%, if any rate includes
invalid episodes, or if the host fields are absent. Read the rates.

## 6. Critic gate

`critic_gate` is `[]`. There is no perceptual artifact. The mechanical criterion is unusually tight
for a measurement item because it gates reproducibility as well as completeness: a single
unreproducible number would be worse than no number, since a lab would size an experiment against it.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — worker-pool driver inside the container, one headless process per
                 worker, episodes streamed through env.reset/env.step
   -> if still failing, MANDATORY approach change. Adding workers is NOT an approach change;
      reusing one headless process across episodes via fork/discard instead of restarting is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with run_agreement_pct > 25
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: sustained throughput below 5 episodes per CPU-hour (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.21/throughput.json`

A reader finds: an explicit `MEASURED` status, the named host with physical and logical core counts,
the task and pinned time scale, the sustained duration with warm-up excluded, per-worker-level
episode counts split into scored / invalid / errored, both rate units at both parallelism levels, the
per-stage cost breakdown, the agreement percentage between two independent runs, and the
reconciliation against the `G7.13` latencies. `G7.22` publishes this as the harness's cost figure.

## 9. Definition of NOT done

- The rate is high because `time_scale` was raised and most episodes are `invalid`. Only scored
  episodes count, and `time_scale` is pinned at 1.0.
- A peak burst is reported as sustained throughput, so a lab's 10,000-episode plan takes four times
  the projected time.
- Wall-clock throughput is reported without parallelism or core count, making it unfalsifiable and
  uncomparable.
- The measurement is taken on the Windows host, which is not the system a lab runs. The container is
  the system under test.
- One run is reported. A single unreproducible number is worse than none because it will be used to
  size an experiment.
- The profiler is armed during the throughput run, so the number measures a profiled system.
- A number appears in the artifact without a `MEASURED` / `TARGET` / `CONFIG DEFAULT` label. This
  repository already has one config default that reads as a benchmark result; do not add a second.
````

---

## `docs/PROMPTS/items/G7.22_eustress-phys-12-benchmark.md`

````markdown
---
id: G7.22
title: EUSTRESS-PHYS-12 — public benchmark and reproducible harness
workload: W1
workload_secondary: [W3, W5]
phase: G7
depends_on: [G7.18, G7.19, G7.20, G7.21, G1.12, G1.48]
blocks: [G7.23, G7.24]
tier: XL
token_envelope: 1200000
wallclock_envelope: 5d
max_builds: 20
critic_gate: [D5, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G7_benchmark.json
artifact: docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md
escalation: >
  If two independent runs of the same baseline agent produce aggregate scores differing by more
  than 5 percentage points, STALL. A leaderboard whose noise floor exceeds the differences it
  intends to report cannot be published, and reducing that noise is a separate engineering item.
status: DRAFT
notes: >
  The publishable benchmark. Its credibility rests entirely on the four items it depends on:
  physically verified scoring, an adversarial suite, a container anyone can run, and a measured
  cost. Publishing before any of those is what turns a benchmark into marketing.
---

## 1. Objective

A public benchmark specification named EUSTRESS-PHYS-12 defines twelve physically verified tasks, a
submission format, a scoring and aggregation rule, a repeated-run policy, and a leaderboard schema —
accompanied by a container that a stranger runs with one command to reproduce two reference baselines
whose scores agree within a stated tolerance across independent runs.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine, and
this document is published externally, so the phrasing matters. Source-available under PolyForm
Shield 1.0.0 — never write "open source". Physics is **Avian**, never Rapier. Slint compiles to
Rust. **Units are meter-native**; studs must not appear.

**One licence fact the specification must state accurately, because an evaluator will check.**
PolyForm Shield 1.0.0 permits copying, distribution, modification, and internal production use at no
cost, including evaluation and research use, and building products *with* the substrate
royalty-free. It forbids providing a product that competes with the software. A lab evaluating
Eustress as an environment and publishing results is squarely inside the permitted set. State that
plainly and without embellishment. Do not describe the licence as open source, and do not promise
terms the licence does not grant.

**What makes this benchmark different, and what the specification must therefore prove rather than
assert.** Four properties, each already evidenced by a dependency:

1. **First-principles physics.** Avian dynamics plus the realism tier. Cite
   `docs/PROMPTS/artifacts/G7.18/scoring_verification.json` for the predicate kinds actually used.
2. **Real time compression, honestly accounted.** Cite
   `docs/PROMPTS/artifacts/G7.15/dropped_tick_accounting.json` and state that over-tolerance
   episodes are `invalid`, not scored.
3. **Real engineering artifacts.** CAD-measured predicates via `eustress/crates/cad/src/measure.rs`
   (`mass_properties`, `topology`, `min_distance`).
4. **Physically verifiable success.** Zero string-comparison predicates, evidenced by the same
   `G7.18` artifact.

**What the dependencies give you, and what you must not re-derive.**

- `G7.18` — twelve tasks, reference solutions, per-task cheat baselines, zero string predicates.
- `G7.19` — `docs/PROMPTS/artifacts/G7.19/exploit_report.json`: at least ten exploits across at
  least eight classes, all scoring zero after hardening, plus the declared repeated-run policy.
  **Publish this report with the benchmark.** It is the credibility anchor.
- `G7.20` — `docker/eustress-agent-eval.Dockerfile` and
  `docs/PROMPTS/artifacts/G7.20/container_conformance.json`: CPU-only, no display, zero GUI library
  links.
- `G7.21` — `docs/PROMPTS/artifacts/G7.21/throughput.json`: measured episodes per CPU-hour on named
  hardware, reproducible within 10%.
- `G7.17` — the episode recording format, replayable byte-exactly by an independent process via two
  paths, so a leaderboard entry can be audited.

**The two reference baselines.** A **random-action** agent, which establishes the floor and proves
the tasks are not passable by chance; and a **scripted-oracle** agent that follows each task's
reference solution, which establishes the ceiling and proves the tasks are solvable. A benchmark
with only one of these cannot be interpreted. Neither baseline is a language model — `G7.23`
supplies that.

**Aggregation must be stated, not implied.** Twelve per-task scores must combine by a rule written
down in advance: equal weighting or stated weights, how `invalid` episodes are handled (excluded and
reported, never zero-scored), how many episodes per task, and what the repeated-run policy is. An
unstated aggregation rule is the first thing an external evaluator will attack.

**CI remains out of scope.** Do not add or edit a workflow file. The harness is the container plus
the gate script.

**Build reality.** 10–15 minutes per engine build; container rebuilds add image assembly. One build
at a time, never killed mid-compile. Twenty builds is the entire XL envelope. Write the
specification before you build anything. Validate by running, not by `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md` — the publishable specification
- `docs/PROMPTS/artifacts/G7.22/schema/submission.v1.schema.json` and
  `leaderboard.v1.schema.json`
- `eustress/crates/agent-eval/` — the benchmark runner, the aggregator, the submission validator,
  and the two reference baseline agents
- `scripts/agent_eval/baselines/` — the baseline agent definitions
- `docs/PROMPTS/harness/recipes/G7_benchmark.json`
- `docs/PROMPTS/artifacts/G7.22/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass, and never add a
  workflow here
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `tasks/suite/` — the twelve tasks are frozen by `G7.18`; a task that scores badly is a result
- `scripts/agent_eval/exploits/` — the exploit suite is frozen by `G7.19`
- `docker/eustress-agent-eval.Dockerfile` — `G7.20` owns the image
- `docs/PROMPTS/artifacts/G7.15/`, `G7.17/`, `G7.18/`, `G7.19/`, `G7.20/`, `G7.21/` — frozen
  baselines and the specification's citations

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Dropping a task on which the oracle scores poorly, choosing an aggregation rule after seeing the
  baseline scores, reducing the episode count per task until variance looks small, or publishing
  without the exploit report are all measurement changes. If the measurement is genuinely wrong,
  report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Fix the aggregation rule, the episodes-per-task count, and the repeated-run policy **in writing,
  before** running either baseline. Record the commit at which they were fixed.
- The random baseline must score near the floor and the oracle near the ceiling. If random scores
  well on a task, that task is trivial and it is a finding — report it; do not silently drop it.
- The specification must be runnable by a stranger from the document alone: one `docker run`, one
  output file, one aggregation command. If a reader needs a repository checkout to reproduce a
  baseline, the harness is not reproducible.
- Every number in the specification is `MEASURED` with its command and host, `TARGET`, or
  `CONFIG DEFAULT`. No exceptions.
- Publish the exploit report and the throughput figure alongside the scores. A benchmark that hides
  its cost or its known attacks is not credible.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion
Both reference baselines are run over all twelve tasks at the declared episodes-per-task count; the
**random** baseline's aggregate score is **≤ 0.10** and the **oracle** baseline's is **≥ 0.90**; two
independent runs of each baseline agree within **5 percentage points** on the aggregate; a submission
validator accepts a conforming submission and rejects three malformed ones with distinct codes; and
the published specification cites the exploit report and the measured throughput.

### Measurement

Command:

```
# 1. Run both baselines twice, independently, in the frozen container
foreach ($run in 'A','B') {
  foreach ($agent in 'random','oracle') {
    docker run --rm `
      -e EUSTRESS_WORKSPACE=/work/workspace `
      -v "$PWD/docs/PROMPTS/artifacts/G7.22/out:/work/out" `
      eustress-agent-eval:t4 `
      --benchmark EUSTRESS-PHYS-12 --agent $agent `
      --episodes-per-task 20 --seed-base 20260806 `
      --out "/work/out/${agent}_run${run}.json"
    echo "EXIT_${agent}_${run}=$LASTEXITCODE"
  }
}

# 2. Aggregate, check reproducibility, and validate the submission format
cargo run --release --package eustress-agent-eval --bin eustress-benchmark-report -- `
  --random-a docs/PROMPTS/artifacts/G7.22/out/random_runA.json `
  --random-b docs/PROMPTS/artifacts/G7.22/out/random_runB.json `
  --oracle-a docs/PROMPTS/artifacts/G7.22/out/oracle_runA.json `
  --oracle-b docs/PROMPTS/artifacts/G7.22/out/oracle_runB.json `
  --submission-schema docs/PROMPTS/artifacts/G7.22/schema/submission.v1.schema.json `
  --accept scripts/agent_eval/baselines/valid_submission.json `
  --reject scripts/agent_eval/baselines/bad_missing_seed.json `
  --reject scripts/agent_eval/baselines/bad_unknown_task.json `
  --reject scripts/agent_eval/baselines/bad_replay_hash_mismatch.json `
  --require-agreement-pp 5 `
  --out docs/PROMPTS/artifacts/G7.22/benchmark_results.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape (`benchmark_results.json`):

```json
{
  "schema_version": 1,
  "benchmark": "EUSTRESS-PHYS-12",
  "commit": "…",
  "rules_fixed_at_commit": "…",
  "aggregation": { "rule": "unweighted mean of per-task success rate",
                   "episodes_per_task": 20,
                   "invalid_handling": "excluded from the mean and reported separately",
                   "repeated_run_policy": "first run counts; seed declared in the submission" },
  "tasks": 12,
  "random":  { "aggregate_a": 0.042, "aggregate_b": 0.050, "agreement_pp": 0.8,
               "invalid_episodes": 0 },
  "oracle":  { "aggregate_a": 0.958, "aggregate_b": 0.950, "agreement_pp": 0.8,
               "invalid_episodes": 0 },
  "per_task_random": { "T01": 0.00, "T03": 0.05 },
  "per_task_oracle": { "T01": 1.00, "T03": 0.95 },
  "trivial_tasks_flagged": [],
  "submission_validation": { "accepted": 1, "rejected": 3,
                             "reject_codes": ["E_SEED_MISSING", "E_TASK_UNKNOWN",
                                              "E_REPLAY_HASH_MISMATCH"],
                             "distinct_reject_codes": 3 },
  "cites_exploit_report": "docs/PROMPTS/artifacts/G7.19/exploit_report.json",
  "cites_throughput": "docs/PROMPTS/artifacts/G7.21/throughput.json",
  "reproducible_by_stranger": { "commands_required": 2, "repo_checkout_required": false }
}
```

Pass condition:

```
EXIT == 0 AND all four EXIT_<agent>_<run> == 0
AND tasks == 12
AND random.aggregate_a <= 0.10 AND random.aggregate_b <= 0.10
AND oracle.aggregate_a >= 0.90 AND oracle.aggregate_b >= 0.90
AND random.agreement_pp <= 5.0 AND oracle.agreement_pp <= 5.0
AND submission_validation.accepted == 1 AND submission_validation.rejected == 3
AND submission_validation.distinct_reject_codes == 3
AND cites_exploit_report and cites_throughput both resolve to existing files
AND reproducible_by_stranger.repo_checkout_required == false
```

Read the aggregates and the agreement figures. The reporter must exit non-zero if either baseline
falls outside its band or if the two runs disagree beyond the tolerance.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)** and **D6 (overall coherence)**,
floor **8.0 each**. The mean is irrelevant; either below floor fails.

D5 asks whether a domain expert would trust the numbers. That depends on: whether the predicates
measure real quantities on physics the engine actually models; whether the random/oracle bands are
where they should be; whether `invalid` episodes are excluded rather than zero-scored; and whether
the aggregation rule was fixed before the results were seen. The `rules_fixed_at_commit` field is
where the Critic will look for that last one.

D6 asks whether the published artifact reads as one designed benchmark — a coherent set of tasks
with a single scoring convention, a submission format that matches the harness, and a leaderboard
schema that matches the submission format — rather than twelve tasks and three schemas that met
recently.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_benchmark.json`. Create it if absent, following the
format in `docs/PROMPTS/02_CAPTURE_HARNESS.md`.

The Critic reads `docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md` and
`benchmark_results.json`, never your self-report. The specification must stand alone to a reader who
has never heard of Eustress.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — write the specification and schemas first; implement both baselines
                 against the frozen container; aggregate
   -> if still failing, MANDATORY approach change. Adjusting an aggregation weight is NOT an
      approach change; moving from per-task success rate to a normalised margin score is — and it
      requires re-fixing the rules before re-running.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with either baseline outside its band and the worst
                  gated dimension moving < 0.5
  - Budget      : 1.8M tokens or 30 builds consumed (150% of the XL envelope)
  - Item-specific: baseline aggregate agreement worse than 5 percentage points (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md`, with
`docs/PROMPTS/artifacts/G7.22/benchmark_results.json` and the two schemas under
`docs/PROMPTS/artifacts/G7.22/schema/`.

A reader — an external evaluator with no prior exposure — finds: what the benchmark measures and why
its four properties are unavailable elsewhere; the twelve tasks with their predicates and units; the
submission format and the leaderboard schema; the aggregation rule, episode count, invalid-handling
rule, and repeated-run policy, with the commit at which they were fixed; both baselines' scores with
their reproducibility figures; a link to the exploit report; the measured cost per episode; the exact
two commands to reproduce everything; and an accurate statement of the PolyForm Shield licence
position for an evaluating lab. This is the **second publishable external artifact** in pack T4 and
the W1 evidence for the program.

## 9. Definition of NOT done

- The random baseline scores 0.35, meaning several tasks are passable by chance, and the tasks are
  quietly replaced rather than the finding being reported.
- The aggregation rule is chosen after the baseline scores are known. `rules_fixed_at_commit` exists
  to make this checkable, and a Critic will check it.
- `invalid` episodes are scored as zero, so an agent that triggers dropped ticks is penalised as
  though it failed — biasing the leaderboard in the opposite direction from the defect.
- The benchmark is published without the exploit report, so the first external critique is the one
  the authors already knew about and did not disclose.
- Reproduction requires a repository checkout, a Rust toolchain, and a Windows machine. One
  container, two commands, or the harness is not reproducible.
- The specification describes Eustress as a game engine, or the licence as open source, or quotes a
  throughput figure without its host and command. Any one of these fails an externally published
  artifact.
- Every number passes and the Critic refuses D6, citing that the submission schema and the
  leaderboard schema disagree about what a task result is. That is a legitimate refusal and the item
  is not done.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.
````

---

## `docs/PROMPTS/items/G7.23_measured-baseline-agent-results.md`

````markdown
---
id: G7.23
title: Measured baseline agent results on EUSTRESS-PHYS-12
workload: W1
workload_secondary: [W3, W4]
phase: G7
depends_on: [G7.22, G7.09, G1.12]
blocks: [G7.24]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D5, D1]
capture_recipe: docs/PROMPTS/harness/recipes/G7_benchmark.json
artifact: docs/PROMPTS/artifacts/G7.23/baseline_agent_results.json
escalation: >
  If the tool-driven agent cannot complete a single episode end to end because the MCP surface
  exhausts its context before a terminal state is reached, STALL and report the token cost per
  episode against the observation size. Observation compression is a design change to the
  environment, not a tuning knob, and it changes what the benchmark measures.
status: DRAFT
notes: >
  The third publishable artifact and the one with the most external pull: a real agent's measured
  score on a physically verified benchmark. The number may be low. A low number honestly measured
  is the asset; a high number nobody can reproduce is a liability.
---

## 1. Objective

A general-purpose tool-using agent, driving Eustress over the MCP surface with no task-specific
scaffolding, is scored on all twelve EUSTRESS-PHYS-12 tasks across at least three independent trials
each, and the per-task and aggregate results are archived with replayable episode recordings that a
third party can audit.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine, and
this artifact is published, so the phrasing matters. Source-available under PolyForm Shield 1.0.0 —
never "open source". Physics is **Avian**, never Rapier. Slint compiles to Rust. Units are
meter-native.

**Why a low score is the asset.** A physically verified benchmark on which a strong general agent
scores modestly is evidence that the benchmark measures something the field has not saturated. A
benchmark on which everything scores near the ceiling measures nothing. Report the number you get.
Tuning the agent until the number looks good converts a result into an advertisement and is
explicitly forbidden below.

**What you are running against.** EUSTRESS-PHYS-12, specified at
`docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md`, with its rules fixed at a recorded commit:
aggregation rule, episodes per task, invalid-episode handling, and repeated-run policy. Read
`docs/PROMPTS/artifacts/G7.22/benchmark_results.json` for the two reference baselines — random near
the floor and scripted-oracle near the ceiling — which are the interpretive frame for this item's
number. Run in the frozen container from `G7.20`.

**The agent's interface, and the rule that keeps this honest.** The agent sees the environment
through exactly the declared action space for each task
(`docs/PROMPTS/artifacts/G7.16/action_space.json`) and the declared observation
(`observation_space.json`). **No task-specific prompting, no task-specific tools, no hand-written
solution hints.** One system prompt, identical across all twelve tasks, describing the substrate and
the interface but not the tasks. Record that prompt verbatim in the artifact — it is part of the
result.

**Visual observation, where a task declares one.** `G7.09` established that the agent camera
produces byte-identical captures at the same pose and tick, with the adapter recorded in
`docs/PROMPTS/artifacts/G7.09/agent_camera_determinism.json`. If any of the twelve tasks includes a
rendered frame in its observation, this dependency is why the result is reproducible; cite it.

**Auditability.** Every episode is archived in the `G7.17` recording format, which an independent
process replays byte-exactly by both the action path and the op-log path
(`docs/PROMPTS/artifacts/G7.17/episode_replay.json`). A leaderboard entry that cannot be replayed is
an assertion; one that can is a result.

**Cost reporting.** Report tokens consumed and wall-clock per episode alongside the score, and
reconcile the wall-clock against `docs/PROMPTS/artifacts/G7.21/throughput.json`. A score without a
cost is not actionable for a lab.

**Model identification.** Name the exact model and version used, and the date of the run. A result
attributed to an unnamed model is unreproducible by construction.

**Build reality.** No engine change should be required. The expensive resource here is agent
inference, not compilation. One build at a time if any is needed, never killed mid-compile.
Validate by running, not by `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — the agent runner and its transcript/cost recorder
- `scripts/agent_eval/agents/` — the single shared system prompt and the runner configuration
- `docs/PROMPTS/artifacts/G7.23/`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `tasks/suite/` — the twelve tasks are frozen; a hard task is a result
- `docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md` and its schemas — the rules are fixed
- `docker/eustress-agent-eval.Dockerfile` — the image is frozen by `G7.20`
- `docs/PROMPTS/artifacts/G7.09/`, `G7.16/`, `G7.17/`, `G7.20/`, `G7.21/`, `G7.22/` — frozen
  baselines

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Adding task-specific hints to the prompt, giving the agent a tool that only helps on one task,
  running many trials and reporting the best, extending the step budget beyond the task's declared
  value, or excluding a task the agent does badly on are all measurement changes. If the measurement
  is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- One system prompt, verbatim identical across all twelve tasks, recorded in the artifact.
- At least three independent trials per task, all reported. Report the mean and the spread; never
  report the best trial as the result.
- Honour the benchmark's declared repeated-run policy from `G7.22`. If the policy is
  first-run-counts, the headline number is the first run and the additional trials are reported as
  variance.
- Archive every episode as a `G7.17` recording and verify that a sample replays byte-exactly. An
  unauditable result is not publishable.
- Report tokens and wall clock per episode. Reconcile wall clock against the `G7.21` throughput
  figure and flag any divergence above 25%.
- Name the model and version exactly, and the run date.

## 5. Exit criterion

### Criterion
The agent completes **≥ 3 independent trials on each of the twelve tasks** — at least 36 episodes —
with **zero** task-specific prompting; per-task and aggregate scores are reported with their spread;
**≥ 90%** of archived episodes replay byte-exactly; and the aggregate is reported alongside the
`G7.22` random and oracle baselines with token and wall-clock cost per episode.

### Measurement

Command:

```
docker run --rm `
  -e EUSTRESS_WORKSPACE=/work/workspace `
  -e AGENT_MODEL="<exact-model-id>" `
  -v "$PWD/docs/PROMPTS/artifacts/G7.23/out:/work/out" `
  eustress-agent-eval:t4 `
  --benchmark EUSTRESS-PHYS-12 --agent tool-driven `
  --system-prompt /work/agents/shared_system_prompt.md `
  --trials-per-task 3 --seed-base 20260806 `
  --record-dir /work/out/episodes `
  --out /work/out/raw_results.json
echo "RUN_EXIT=$LASTEXITCODE"

# Verify replayability of the archive and emit the published artifact
cargo run --release --package eustress-agent-eval --bin eustress-baseline-report -- `
  --raw docs/PROMPTS/artifacts/G7.23/out/raw_results.json `
  --episodes-dir docs/PROMPTS/artifacts/G7.23/out/episodes `
  --verify-replay-sample-pct 100 `
  --compare-baselines docs/PROMPTS/artifacts/G7.22/benchmark_results.json `
  --reconcile-throughput docs/PROMPTS/artifacts/G7.21/throughput.json `
  --require-identical-prompt-across-tasks `
  --out docs/PROMPTS/artifacts/G7.23/baseline_agent_results.json
echo "EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "benchmark": "EUSTRESS-PHYS-12",
  "measurement_status": "MEASURED",
  "run_date": "2026-08-14",
  "model": "<exact-model-id>",
  "system_prompt_sha256": "e11a…",
  "identical_prompt_across_tasks": true,
  "task_specific_scaffolding": false,
  "trials_per_task": 3,
  "episodes_total": 36,
  "episodes_invalid": 1,
  "aggregate": { "first_run": 0.333, "mean": 0.352, "stddev": 0.061 },
  "reference_baselines": { "random": 0.046, "oracle": 0.954 },
  "per_task": [
    { "id": "T01_thermal_ceiling", "scores": [1.0, 1.0, 0.0], "mean": 0.667,
      "tokens_per_episode_p50": 84210, "wallclock_s_p50": 412.8 }
  ],
  "replay_verification": { "episodes_checked": 36, "replayed_exactly": 35,
                           "replay_rate": 0.972 },
  "cost": { "tokens_per_episode_p50": 91004, "wallclock_s_per_episode_p50": 448.1,
            "throughput_reconciliation_delta_pct": 11.4, "flagged": false }
}
```

Pass condition:

```
RUN_EXIT == 0 AND EXIT == 0
AND episodes_total >= 36 AND trials_per_task >= 3
AND identical_prompt_across_tasks == true
AND task_specific_scaffolding == false
AND per_task has 12 entries, each with >= 3 scores
AND replay_verification.replay_rate >= 0.90
AND reference_baselines.random and .oracle are both present
AND cost.tokens_per_episode_p50 and cost.wallclock_s_per_episode_p50 are numeric
AND model is a non-empty exact identifier
```

Read the emitted aggregate, the per-task spreads, and the replay rate. There is no threshold on the
score itself — the score is the finding.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)** and **D1 (first-three-seconds
impact)**, floor **8.0 each**. The mean is irrelevant; either below floor fails.

D5 asks whether the result is trustworthy: identical prompting across tasks, three trials with the
spread reported, invalid episodes excluded and counted, a replay rate that lets a third party check
the work, and cost reported alongside score. A single-trial number with no spread and no replay
evidence fails D5 no matter what the number is.

D1 is included because this artifact is published and its first three seconds decide whether an
external reader keeps reading. The question is what a stranger concludes from the top of the
document before any explanation: that a real agent was measured on a physically verified benchmark
and the number is what it is. A result buried under caveats, or one whose headline is ambiguous
about which number is the score, fails D1.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_benchmark.json` — the same recipe as `G7.22`.

The Critic reads the artifact and the archived episodes, never your self-report.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — run the tool-driven agent through the frozen container with one
                 shared system prompt; archive and verify every episode
   -> if still failing, MANDATORY approach change. Rewording the shared prompt is NOT an approach
      change; changing how observations are serialised into the agent's context is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with episodes_total < 36
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the agent exhausts context before any terminal state (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.23/baseline_agent_results.json`, with the shared system prompt at
`scripts/agent_eval/agents/shared_system_prompt.md` and the archived episodes under
`docs/PROMPTS/artifacts/G7.23/out/episodes/`.

A reader finds: the exact model and run date, the hash of the single shared prompt with confirmation
it was identical across tasks, explicit confirmation of no task-specific scaffolding, the per-task
scores across three trials with means and spread, the aggregate reported next to the random and
oracle baselines, the invalid-episode count, the replay verification rate, and cost in tokens and
wall clock per episode reconciled against measured throughput. This is the **third publishable
external artifact** in pack T4 and the strongest W1 evidence the program can produce.

## 9. Definition of NOT done

- The prompt was adjusted between tasks to help the agent. That converts a benchmark result into a
  demonstration, and the prompt hash check exists to catch it.
- One trial per task is run, so a lucky episode is indistinguishable from capability.
- The best of five trials is reported as the score. The declared repeated-run policy from `G7.22`
  governs, and all trials are reported.
- Episodes are not archived, or fewer than 90% replay, so no third party can audit the leaderboard
  entry and the result is an assertion.
- The score is reported with no cost, so a lab cannot tell whether reaching it took 400 tokens or
  4,000,000.
- The model is described as "a frontier model" rather than named and versioned, making the result
  unreproducible.
- The score is low and the artifact is padded with explanations for why. Report the number and the
  conditions; the interpretation belongs to the reader.
````

---

## `docs/PROMPTS/items/G7.24_lab-integration-guide.md`

````markdown
---
id: G7.24
title: Lab integration guide
workload: W5
workload_secondary: [W3]
phase: G7
depends_on: [G7.20, G7.22, G1.12, G1.48, G7.23]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D6, D1]
capture_recipe: docs/PROMPTS/harness/recipes/G7_benchmark.json
artifact: docs/PROMPTS/artifacts/G7.24/LAB_INTEGRATION.md
escalation: >
  If following the guide from a clean machine requires any step the guide does not contain, STALL
  and report the missing step. A guide that needs its author present is not an integration guide;
  it is a set of notes.
status: DRAFT
notes: >
  The fourth publishable external artifact. Its exit criterion is a clean-machine walkthrough, not
  a review — the only honest test of a guide is whether someone can follow it.
---

## 1. Objective

A published integration guide takes an engineer at an external lab from zero to a running episode
and a scored submission, using only the document, a container runtime, and a network connection. A
clean-machine walkthrough executes every command in the guide in order, in sequence, with no
undocumented steps, and produces a valid submission.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never described as a game
engine. This document is published externally, so every phrasing invariant is load-bearing. The
licence is PolyForm Shield 1.0.0 and the project is **source-available**; never write "open source".
Physics is **Avian**, never Rapier. Slint compiles to Rust, so Slint *is* Rust. **Units are
meter-native**; studs are a display unit only and must not appear.

**The licence position, stated accurately, because a lab's counsel will read this section.**
PolyForm Shield 1.0.0 permits, at no cost: copying, distribution, modification, forks, a patent
grant, internal production use at any organisation size, and building and shipping products made
*with* the substrate royalty-free. It forbids providing a product that competes with the software.
Evaluation, research use, publishing results, and building an internal training pipeline are inside
the permitted set. State that, cite `LICENSE` and `LICENSE-COMMERCIAL.md`, and stop — do not
characterise, soften, or extend the terms. Note honestly that the repository currently has no
`CONTRIBUTING.md` and no CLA or DCO, so a lab wishing to contribute upstream should ask; do not
invent a contribution process.

**What the guide integrates.** Everything the pack built, in the order a newcomer needs it:

1. The container — `docker/eustress-agent-eval.Dockerfile`, verified CPU-only with zero GUI library
   links in `docs/PROMPTS/artifacts/G7.20/container_conformance.json`.
2. The environment API — `env.reset` / `env.step` over the Engine Bridge, conformance in
   `docs/PROMPTS/artifacts/G7.13/env_api_conformance.json`.
3. The generated spaces — `docs/PROMPTS/artifacts/G7.16/observation_space.json` and
   `action_space.json`, kept true by a drift check.
4. The task format — `.etask` version 1, specified at
   `docs/PROMPTS/artifacts/G7.12/etask_spec_v1.md`.
5. The benchmark — `docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md` with its submission and
   leaderboard schemas.
6. Episode recording and replay — `docs/PROMPTS/artifacts/G7.17/episode_replay.json`.
7. Cost — `docs/PROMPTS/artifacts/G7.21/throughput.json`, measured on named hardware.

**What the guide must be honest about.** An integration guide that omits known limitations is
discovered to have omitted them, and the cost is trust. State plainly: `.glb`-backed custom meshes
do not decode in the headless tier (recorded in
`eustress/crates/engine/src/bin/headless.rs` line 22); the post-process stack is on hold, so only
filmic tonemapping ships (`eustress/crates/engine/src/photoreal.rs`); compressed episodes exceeding
their declared dropped-tick tolerance are `invalid` rather than scored
(`docs/PROMPTS/artifacts/G7.15/dropped_tick_accounting.json`); and the engine is not built by CI, so
the supported entry point is the container image, not a source build.

**Never fabricate a number.** Every figure in the guide is `MEASURED` with its command, host, and
artifact path, or labelled `TARGET` or `CONFIG DEFAULT`.

**Build reality.** This item writes a document and executes a walkthrough. `max_builds` is 6 to
allow container pulls and a rebuild if the walkthrough exposes a packaging defect. One build at a
time, never killed mid-compile.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/G7.24/LAB_INTEGRATION.md` — the publishable guide
- `docs/PROMPTS/artifacts/G7.24/` — the walkthrough transcript and its report
- `scripts/agent_eval/walkthrough.sh` — the script that executes the guide's commands in order
- `docker/entrypoint.sh` — only if the walkthrough exposes a defect in the documented entry point

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `LICENSE`, `LICENSE-COMMERCIAL.md` — quoted and cited, never edited
- `docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md` — the benchmark is frozen; the guide points
  at it
- `docker/eustress-agent-eval.Dockerfile` — the image is frozen by `G7.20`
- All frozen baselines: `docs/PROMPTS/artifacts/G7.12/`, `G7.13/`, `G7.15/`, `G7.16/`, `G7.17/`,
  `G7.20/`, `G7.21/`, `G7.22/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Running the walkthrough on a machine that already has the image cached, skipping a step because
  "everyone has that installed", editing the walkthrough script to differ from the guide, or
  hand-fixing a failure mid-walkthrough without also fixing the guide are all measurement changes.
  If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and
  stop.
- The walkthrough script must be generated from, or mechanically checked against, the guide's
  command blocks. A script that drifts from the document tests the script, not the guide.
- The clean machine must start with nothing but a container runtime — no cached image, no repository
  checkout, no Rust toolchain.
- Every limitation listed in §2 must appear in the guide. Their absence is a failure condition in
  §9 and a Critic will look for them.
- The guide must state the licence position and cite `LICENSE` and `LICENSE-COMMERCIAL.md` without
  characterising the terms beyond what they say.
- Time the walkthrough and report it. "Time to first episode" is the number a lab actually cares
  about.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion
A clean-machine walkthrough executes **every command block in the guide, in document order**, with
**zero undocumented steps** and **zero manual interventions**, ending in a **schema-valid
submission** accepted by the `G7.22` validator; and time-to-first-completed-episode is measured and
reported.

### Measurement

Command:

```
# Executed on a machine with a container runtime and nothing else:
# no cached image, no repo checkout, no Rust toolchain.

bash scripts/agent_eval/walkthrough.sh \
  --guide docs/PROMPTS/artifacts/G7.24/LAB_INTEGRATION.md \
  --extract-command-blocks \
  --fail-on-undocumented-step \
  --fail-on-manual-intervention \
  --out docs/PROMPTS/artifacts/G7.24/walkthrough_report.json
echo "WALKTHROUGH_EXIT=$?"

# The submission the walkthrough produced must pass the frozen benchmark validator
docker run --rm \
  -v "$PWD/docs/PROMPTS/artifacts/G7.24:/work/out" \
  eustress-agent-eval:t4 \
  --validate-submission /work/out/submission.json \
  --schema-version submission.v1
echo "SUBMISSION_EXIT=$?"
```

Expected output shape (`walkthrough_report.json`):

```json
{
  "schema_version": 1,
  "guide": "docs/PROMPTS/artifacts/G7.24/LAB_INTEGRATION.md",
  "guide_sha256": "b7d1…",
  "machine": { "clean": true, "image_pre_cached": false, "repo_checkout_present": false,
               "rust_toolchain_present": false },
  "command_blocks_in_guide": 11,
  "command_blocks_executed": 11,
  "undocumented_steps": 0,
  "manual_interventions": 0,
  "failures": [],
  "time_to_first_completed_episode_s": 1642,
  "total_walkthrough_s": 3980,
  "submission_produced": "docs/PROMPTS/artifacts/G7.24/submission.json",
  "limitations_documented": ["glb_headless", "post_process_on_hold",
                             "invalid_compressed_episodes", "engine_not_built_by_ci"],
  "licence_section_cites": ["LICENSE", "LICENSE-COMMERCIAL.md"]
}
```

Pass condition:

```
WALKTHROUGH_EXIT == 0 AND SUBMISSION_EXIT == 0
AND command_blocks_executed == command_blocks_in_guide
AND undocumented_steps == 0
AND manual_interventions == 0
AND machine.clean == true AND machine.image_pre_cached == false
    AND machine.repo_checkout_present == false AND machine.rust_toolchain_present == false
AND len(limitations_documented) >= 4
AND licence_section_cites contains both LICENSE and LICENSE-COMMERCIAL.md
AND time_to_first_completed_episode_s is numeric
```

Read the emitted counters. A guide that produced a submission after two undocumented fixes has
failed, and the intervention counter is what records that.

## 6. Critic gate

Gated on **D6 (overall coherence)** and **D1 (first-three-seconds impact)**, floor **8.0 each**. The
mean is irrelevant; either below floor fails.

D6 asks whether the guide, the benchmark specification, the `.etask` format, and the generated space
specifications read as one system with one vocabulary — or as four documents written by four people.
A reader who meets three different names for the same concept has met an incoherent system.

D1 is included because this document is the first thing an external evaluator reads. In three
seconds, before any explanation, a stranger should conclude what this is, what it runs on, and what
the first command is. A guide whose first screen is background and positioning fails D1 no matter how
good the rest is.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_benchmark.json` — the guide is evaluated against the
same benchmark surface it documents.

The Critic reads `docs/PROMPTS/artifacts/G7.24/LAB_INTEGRATION.md` and the walkthrough report, never
your self-report.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — write the guide, generate the walkthrough script from its command
                 blocks, execute on a clean machine
   -> if still failing, MANDATORY approach change. Adding one more documented step is NOT an
      approach change; restructuring the guide around a single quickstart command with everything
      else as reference is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive walkthroughs with undocumented_steps unchanged and > 0
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the walkthrough needs a step the guide does not contain (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.24/LAB_INTEGRATION.md`, with
`docs/PROMPTS/artifacts/G7.24/walkthrough_report.json` and the produced
`docs/PROMPTS/artifacts/G7.24/submission.json`.

A reader — an engineer at an external lab, with no prior exposure — finds: what Eustress is in one
paragraph; the first command; how to run one episode; the observation and action spaces; how to
write a task in `.etask`; how to score against EUSTRESS-PHYS-12 and submit; how to replay and audit
an episode; the measured cost per episode with its host; the four known limitations stated plainly;
and an accurate, cited statement of the licence position for an evaluating lab. This is the
**fourth publishable external artifact** in pack T4 and the W5 evidence for the extension surface.

## 9. Definition of NOT done

- The walkthrough runs on a machine with the image already pulled and a repository checkout present,
  so the guide's actual first ten minutes were never tested.
- A step failed, the author fixed it by hand, and the walkthrough continued. The intervention counter
  exists for this; a hand-fixed walkthrough proves the author can run it.
- The guide omits the `.glb` headless limitation, so the first thing a lab tries is importing a mesh
  and the first impression is a silent failure.
- The licence section characterises PolyForm Shield as "essentially permissive" or "MIT-friendly".
  It is neither, an external asset in this repository has already made that error, and a lab's
  counsel will notice.
- The guide states a throughput or cost figure with no host and no command, so the number cannot be
  checked.
- The walkthrough script diverges from the guide's command blocks, so the test validates the script
  rather than the document.
- Every counter passes and the Critic refuses D1, citing that the first screen is positioning rather
  than a command. That is a legitimate refusal and the item is not done.
- The document calls Eustress a game engine, calls the licence open source, or expresses a distance
  in studs. Any one fails an externally published artifact outright.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.
````
