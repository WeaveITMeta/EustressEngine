# T5 — System Integration, Robustness, Diagnostics + Final Cohesion

**Owns:** the feeling that the whole thing is rock-solid, and the final end-to-end integration gate.
Crash-free session measurement, error handling and recovery, world-container durability, save/load
round-trip fidelity, the load-phase watchdog, profiling truth, enterprise-grade diagnostics, on-prem
vs cloud reliability, the CI gate, startup/shutdown correctness, structured logging — and then the
scripted ten-minute journey through the entire product that is judged as one artifact.

**Phase tag.** Every item in this pack carries `phase: G7` (the terminal gauntlet phase, whose exit
condition in `docs/PROMPTS/00_MASTER_PROTOCOL.md` §3.2 is *"the full observe→act→judge loop executes
headless in CI with no desktop session"*). The robustness half of this pack is what makes that
sentence survivable in front of a buyer; the cohesion half is where the verdict is rendered.

**Workloads its evidence feeds:** primarily **W3 (Trust & Verifiability)** — byte-identical reruns,
green CI, reproducible diagnostics — with **W6 (Operator Leverage)** on the diagnostics and fleet
items, **W4 (Vertical Proof)** on the on-prem and journey items, and **W1 (Provable Quality)** on the
final cohesion judgement.

**ITEM ZERO: `G7.30`.** Nothing in this pack may start before it. Today the engine emits no session
start record, no session end reason, and no crash record — `eustress/crates/engine/src/usage_telemetry.rs`
writes its outbox only from `flush_on_exit`, which reacts to `AppExit`. A process that dies hard
therefore emits *nothing at all*, which makes a crashed session indistinguishable from a session that
never happened. The crash-free-session rate is not merely unknown; it is currently **unmeasurable**.
G7.30 makes it measurable and records the baseline. You cannot improve what you have not measured.

---

## Cross-pack file ownership

Items in this pack share source files with items in other packs. `docs/PROMPTS/04_FILE_OWNERSHIP.md` names one owner per contested path and is normative; where it conflicts with an item's scope list, it wins. This pack's own dependency-graph table records its internal edges together with the program-gate edges of `02_QUEUE.md` §8.2 and §8.4; the cross-pack edges arising from contested paths are these:

| Item | Now depends on | Contested path | Effect on this item's scope |
|---|---|---|---|
| `G7.31` | `G7.03` (T4) | `eustress/crates/engine/src/bin/headless.rs` | may no longer edit it |
| `G7.32` | `G7.03` (T4) | `eustress/crates/engine/src/bin/headless.rs` | may no longer edit it |
| `G7.32` | `G7.31` (T5) | — | G7.31 and G7.32 are siblings under G7.30 and both install into main.rs; G7.31 first |
| `G7.37` | `G1.03` (G1) | `eustress/crates/engine/Cargo.toml` | may append to but not alter it |
| `G7.37` | `G7.08` (T4) | `eustress/crates/worlddb/src/` | may not restructure it |
| `G7.38` | `G7.08` (T4) | `eustress/crates/worlddb/src/` | may not restructure it |
| `G7.39` | `G7.03` (T4) | `eustress/crates/engine/src/bin/headless.rs` | may no longer edit it |
| `G7.40` | `G1.08` (G1) | `eustress/crates/engine/src/frame_diagnostics.rs` | may no longer edit it |
| `G7.40` | `G1.08` (G1) | `eustress/crates/engine/src/profiler.rs` | may no longer edit it |
| `G7.41` | `G7.03` (T4) | `eustress/crates/engine/src/bin/headless.rs` | may no longer edit it |
| `G7.42` | `G6.13` (T3) | `eustress/crates/engine/src/ui/slint_ui.rs` | may no longer edit it |
| `G7.45` | `G1.13` (G1) | — | supplies the clean-environment substitute this item runs in |

An item blocked by one of these entries emits a `FILE-OWNERSHIP` decision packet to L0 (`docs/PROMPTS/04_FILE_OWNERSHIP.md` §6). It does not edit the file and does not work around it.

---


## Dependency graph

| ID | Title | Tier | Workload | `depends_on` |
|---|---|---|---|---|
| **G7.30** | Session lifecycle beacon and measured crash-free baseline | M | W3 | G1.01 *(ITEM ZERO)* |
| G7.31 | Structured JSON log stream and `eustress diag` support bundle | M | W6 | G7.30 |
| G7.32 | Typed panic classifier and durable crash record | M | W3 | G7.30 |
| G7.33 | CI truth ledger — what `ci.yml` gates vs what it must gate | S | W3 | G7.30 |
| G7.34 | `cargo test` runs in CI and is green | M | W3 | G7.33 |
| G7.35 | The client binary is built and smoke-tested in CI | M | W3 | G7.33 |
| G7.36 | Clippy and rustfmt gate on a named crate set | M | W3 | G7.34 |
| G7.37 | Property-based Space save/load round-trip | L | W3 | G7.34 |
| G7.38 | World-container integrity verifier and git-portability truth | M | W3 | G7.37 |
| G7.39 | Machine-readable stuck-phase record, headless-safe | M | W3 | G7.31 |
| G7.40 | Microprofiler machine-readable output and `perf-assert` | M | W3 | G7.31 |
| G7.41 | Startup and shutdown correctness contract | M | W3 | G7.32 |
| G7.42 | Fault injection and typed user-facing recovery | L | W6 | G7.32, G7.39, G1.12 |
| G7.43 | Always-on regression fleet re-running every earlier phase gate | L | W3 | G7.34, G7.35, G7.40, G7.36, G7.37, G7.39, G7.41 |
| G7.44 | On-prem / air-gapped vs cloud reliability profile | M | W3 | G7.31, G7.38 |
| G7.45 | The ten-minute cohesion journey | XL | W4 | all of G7.30–G7.44 except G7.33 and G7.36, which it inherits transitively |

Ladder shape: **G7.30–G7.33** are cheap diagnostics and inventory that establish the baseline.
**G7.34–G7.40** are the work that baseline makes measurable. **G7.41–G7.44** harden the seams.
**G7.45** is the proof artifact and the only item that renders a verdict.

**Standing out-of-scope entries, inherited by every item below** (repeated in each `## 3. Scope`):
anything under `.github/workflows/` may not be edited to make a gate pass;
`docs/PROMPTS/01_CRITIC_RUBRIC.md` is never readable or editable by an executing agent; and any
capture already hashed into a provenance manifest is frozen.

**Declared deviation: T5 is the CI-owning pack.** `docs/PROMPTS/03_PROMPT_SCHEMA.md` §4.3 names
`.github/workflows/` a standing out-of-scope entry for every executing agent. Four items here
deliberately override it, because the phase exit condition this pack is measured against —
*"the full observe→act→judge loop executes headless in CI with no desktop session"* — cannot be
satisfied by an agent forbidden from touching CI. The override is bounded:

| Item | What it may touch | Direction |
|---|---|---|
| `G7.34` | `.github/workflows/ci.yml` | ADD the `cargo test` gate |
| `G7.35` | `.github/workflows/ci.yml` | ADD the client build + smoke gate |
| `G7.36` | `.github/workflows/ci.yml` | ADD the clippy/rustfmt gate |
| `G7.43` | a **new** workflow file for the regression fleet | ADD fleet gates; existing workflows untouched |

`G7.33` inventories CI and edits nothing. Every other item in this pack, and every item in every
other pack, inherits the standing out-of-scope entry unchanged. The override is one-directional in
all four: an item may only ADD a gate, and a diff that weakens, disables, or removes an existing
gate fails that item outright. Each of the four restates this under its `## 4. Approach constraints`
and carries a matching `## 9. Definition of NOT done` entry, so the exception is auditable from the
item file alone — an L2 receiving only that file sees the licence and its limit.

---

---
id: G7.30
title: Session lifecycle beacon and measured crash-free session baseline
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G1.01]
blocks: [G7.31, G7.32, G7.33, G7.41, G7.45, G7.03]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.30/crash_free_baseline.json
escalation: >
  If making the beacon durable requires writing to disk on the hot path more than once per 30 s of
  wall time, or adds any measurable startup cost above 20 ms, STALL rather than trading startup
  latency for observability.
status: DRAFT
notes: >
  ITEM ZERO for pack T5. Tier M rather than S because it touches a compiled crate and needs real
  sessions run against it, but the change itself is small and one build should validate it.
---

## 1. Objective

The engine records, for every session, that the session started and how it ended. A session that
ends by a hard crash, a kill, or a power loss is afterwards distinguishable from a session that
ended cleanly and from a session that never happened. From that record, a **measured**
crash-free-session rate exists for the first time, written to a single JSON artifact with the
session count it was computed from.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. It is never described as a game engine. The licence is PolyForm Shield 1.0.0;
say **source-available**, never open source. The physics engine is Avian, never Rapier. `.slint`
files compile to Rust, so Slint *is* Rust. Units are meter-native; studs are a display unit only.

**What exists today.** `eustress/crates/engine/src/usage_telemetry.rs` (467 lines) is a live,
end-to-end telemetry pipeline: click events buffer in memory, flush to daily JSONL under the user's
`Eustress/telemetry/` folder, and at session end the per-tool **totals** are written to an `outbox/`
file and posted to `https://api.eustress.dev/api/telemetry/usage` on a background thread. A failed
post is retried by `startup_drain` at next launch. The endpoint is overridable with
`EUSTRESS_TELEMETRY_URL`. The plugin is registered at `eustress/crates/engine/src/app_core.rs:145`
(`UsageTelemetryPlugin`). This pipeline works and is the right rail to extend — do not build a
second one.

**The exact gap.** In that file, `flush_on_exit` is a Bevy system reading `AppExit`:

```rust
fn flush_on_exit(mut exits: MessageReader<AppExit>, mut telemetry: ResMut<UsageTelemetry>) {
    if exits.read().next().is_some() {
        let n = telemetry.flush();
        ...
        telemetry.write_outbox();
        std::thread::spawn(drain_outbox);
    }
}
```

`write_outbox` (same file, around line 201) is therefore reached **only on a graceful `AppExit`**.
There is no session-start record anywhere in the file — `grep -n "session" eustress/crates/engine/src/usage_telemetry.rs`
returns only `session_counts`, `session_mode`, and prose. Consequences, all of them true today:

- A hard crash writes no outbox file, so the session leaves no trace. Crashed sessions are invisible.
- Because crashed sessions are invisible, the denominator of "crash-free sessions" is unknowable, so
  the ratio cannot be computed at all. This is a 0% capability, not a partial one.
- The uploaded aggregate carries `install`, `mode`, and `counts` only — no duration, no end reason,
  no engine version, no platform.

**The related panic path.** `eustress/crates/engine/src/main.rs:607` wraps `app.run()` in
`std::panic::catch_unwind`. A panic whose message contains any of several substrings ("swap chain",
"Acquiring a texture", "unrecoverable", "None value" plus "uniform_buffer"/"bevy_render", "Buffer"
plus "invalid") is classified as a lost GPU surface and the process calls `std::process::exit(0)` —
**a clean exit code for what may well have been a real fault**. Everything else is
`resume_unwind`'d. G7.32 replaces that substring sieve; G7.30 only needs to know it exists, because
`std::process::exit(0)` skips Bevy's `AppExit` path and therefore skips `flush_on_exit` too.

**Privacy posture you must preserve.** The module's documented posture is: anonymous random install
UUID never joined to a Bliss/KYC account; no scene content, file paths, or entity names; events
readable as plain JSONL by the user; on by default with one toggle in Settings ▸ Notifications ▸
Privacy that stops capture at the source. Your beacon must satisfy all four. A session record may
carry: install id, session id, start/end timestamps, end reason, engine version, OS/platform string,
and a monotonic session sequence number. It may **not** carry a Space name, a file path, a universe
name, a user name, or any free text.

**Build reality.** A full engine build takes 10–15 minutes. Only one cargo build at a time — the
workspace shares a single `eustress/target/`, and concurrent builds produce link failures. Never
kill a build mid-compile. Validate with `cargo run`, not `cargo check`: `cargo check` will not catch
the plugin-registration failure this item is most likely to produce.

**Where the workspace root is.** The repository root has no `Cargo.toml`. All cargo commands run
from `eustress/`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/usage_telemetry.rs`
- `eustress/crates/engine/src/app_core.rs` — registration only
- A new binary under `eustress/crates/engine/src/bin/` for computing the baseline from records

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/main.rs` — the panic classifier is G7.32's item
- `eustress/crates/engine/src/editor_settings.rs` — reuse the existing opt-out flag, do not add a second
- The layout of `eustress/crates/engine/src/bin/` — the directory is owned by `G2.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Excluding a session category from the denominator, counting only sessions longer than some
  duration, treating an unknown end reason as clean, or computing the rate over a hand-picked window
  are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The start record must be durable **before** any Space loads. A start record written after load
  cannot observe a crash during load, which is precisely the crash class the load-phase watchdog
  (`eustress/crates/engine/src/space/load_phase.rs`) exists for.
- Detect the crash on the *next* launch, not the current one: a start record with no matching end
  record, discovered at startup, is an orphan and is counted as an unclean session. Do not attempt
  to catch every signal — you cannot catch a power loss, and a design that pretends to is worse than
  one that reconciles orphans honestly.
- Do not regress the privacy posture. Reviewers will grep the emitted JSONL for path separators.
- The baseline must be computed from **at least 20 real sessions** on the founder's machine, mixing
  clean exits, deliberate hard kills, and at least one induced panic. State the mix in the artifact.

## 5. Exit criterion

### Criterion
A single command reads the local session records and emits a JSON object whose `sessions_total` is
**≥ 20**, whose `sessions_unclean` is **≥ 3**, and whose `crash_free_rate` equals
`1 - sessions_unclean / sessions_total` to within 1e-9 — with every unclean session carrying a
non-empty `end_reason` drawn from a closed enum, none of them `"unknown"` by default.

### Measurement

Command:

    cd eustress && cargo run --release --bin session-baseline -- \
        --telemetry-dir "$HOME/Eustress/telemetry" \
        --out ../docs/PROMPTS/artifacts/G7.30/crash_free_baseline.json ; echo "EXIT=$?"

Then, to read the emitted value rather than observe the file:

    python -c "import json,sys; d=json.load(open('docs/PROMPTS/artifacts/G7.30/crash_free_baseline.json')); \
      print('total',d['sessions_total'],'unclean',d['sessions_unclean'],'rate',d['crash_free_rate']); \
      sys.exit(0 if d['sessions_total']>=20 and d['sessions_unclean']>=3 and \
      abs(d['crash_free_rate']-(1-d['sessions_unclean']/d['sessions_total']))<1e-9 else 1)" ; echo "EXIT=$?"

Expected output shape:

    EXIT=0
    total 24 unclean 4 rate 0.8333333333333334
    EXIT=0

Pass condition:

    Both `EXIT=0` markers present, AND sessions_total >= 20, AND sessions_unclean >= 3,
    AND the arithmetic identity holds.

Grep the `EXIT=` marker. A file appearing on disk is not a pass.

## 6. Critic gate

`critic_gate: []`. This item is mechanical: either a session ledger exists with a computable rate,
or it does not. The replacement for a Critic pass is the exit criterion in §5, which is unusually
tight on purpose — it requires a specific minimum sample, a specific minimum unclean count (so the
detector is proven to fire, not merely to compile), and an arithmetic identity that cannot be
satisfied by a hardcoded number.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — extend UsageTelemetry with start/end records + orphan reconcile
   -> if still failing, MANDATORY approach change. Renaming a field or changing the flush
      interval is NOT an approach change; moving from in-process reconcile to a separate
      sidecar record file written at startup IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where sessions_unclean stays 0 (the detector
                  never fires) despite deliberate hard kills
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: beacon writes to disk more than once per 30 s of wall time, or startup
                   cost rises above 20 ms (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the sample floor to a stated
value with the stated consequence; FUND a specific approach D; DEFER behind a named item; or KILL.

## 8. Artifact

`docs/PROMPTS/artifacts/G7.30/crash_free_baseline.json`

A reader finds: `sessions_total`, `sessions_unclean`, `crash_free_rate`, the histogram of
`end_reason` values, the date range the sessions span, the engine commit each session ran, the OS
string, and the deliberate-kill mix used to prove the detector fires. This file is the W3 evidence
for the item and is the denominator every later robustness claim in this pack cites.

## 9. Definition of NOT done

- The rate is computed but `sessions_unclean` is 0 because no crash was ever induced. An untested
  detector is not a detector.
- The start record is written after the Space finishes loading, so a hang or crash during load —
  the single most likely long-tail failure, and the reason
  `eustress/crates/engine/src/space/load_phase.rs` has a watchdog at all — is recorded as "never
  started".
- Every unclean session lands in `end_reason: "unknown"`. A closed enum with one populated member is
  a placeholder.
- The session record carries a Space path, a universe name, or any free text, breaking the module's
  documented privacy posture.
- The rate is computed from the uploaded aggregates on the Worker side instead of the local records,
  so the artifact cannot be reproduced on a machine with no network.
- A second telemetry pipeline is introduced alongside the existing one, so the opt-out toggle in
  Settings ▸ Notifications ▸ Privacy stops one but not the other.

---

---
id: G7.31
title: Structured JSON log stream and one-command `eustress diag` support bundle
workload: W6
workload_secondary: [W3]
phase: G7
depends_on: [G7.30, G7.03]
blocks: [G7.39, G7.40, G7.42, G7.44, G7.45, G6.02, G7.32]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.31/diag_bundle_contract.json
escalation: >
  If producing the bundle requires the engine to be running (i.e. it cannot be produced after a
  crash, from the artifacts a crashed session left behind), STALL — a support bundle you can only
  collect while healthy is worthless to a support engineer.
status: DRAFT
notes: >
  Tier M. The engine change is a log-layer addition; the bundle collector is a new binary in the CLI
  crate and needs no engine rebuild once the log format is fixed.
---

## 1. Objective

An enterprise support engineer with no access to the founder can run one command on a customer
machine and obtain a single self-describing archive containing everything needed to diagnose a
failure: structured machine-parseable logs, the session ledger from G7.30, the last crash record,
hardware and build identity, active feature flags, and the environment knobs that were set. The
bundle is produced **after** a crash, from what the crashed session left on disk, not by
instrumenting a healthy process.

## 2. Context you need (self-contained)

**Project invariants.** Eustress is an AI-native simulation substrate, never a game engine. Licence
is PolyForm Shield 1.0.0 — say source-available. Physics is Avian. Slint is Rust. Units are
meter-native. Builds take 10–15 minutes, one at a time, shared `eustress/target/`; never kill a
build mid-compile; validate with `cargo run`, not `cargo check`. Cargo commands run from `eustress/`
(the repo root has no `Cargo.toml`).

**Logging today.** The engine uses Bevy's `LogPlugin`, configured at
`eustress/crates/engine/src/main.rs:215`. `eustress/crates/engine/src/profiler.rs` (702 lines)
already demonstrates the extension point you need: it installs a `tracing_subscriber::Layer` into
the *same* global subscriber Bevy builds, via `LogPlugin`'s `custom_layer` hook (see the module docs
at the top of that file, and `custom_layer` around line 288–312). It deliberately uses Bevy's
re-exported `bevy::log::tracing_subscriber` rather than a direct dependency, to avoid a version
skew. Follow that precedent exactly.

The headless binary configures logging separately: `eustress/crates/engine/src/bin/headless.rs:230`
adds `bevy::log::LogPlugin::default()`. Both surfaces must emit the same structured stream, or a
headless reproduction of a windowed bug produces logs a support engineer cannot diff.

**What a support engineer cannot do today.** There is no `eustress diag`, no log file with a stable
schema, and no single place that records which cargo features a shipped binary was built with. The
CLI (`eustress/crates/cli/src/main.rs`, 1,398 lines, binary name `eustress`) exposes exactly six
top-level subcommands: `Bridge`, `Run`, `Server`, `Publish`, `Sim`, `Fork`. There is no diagnostics
subcommand. Adding one is in scope.

**Feature flags matter more here than usual.** `eustress/crates/engine/Cargo.toml` has
`default = ["core", "data"]`, and `core` is a long list that currently enables nearly everything —
including `worlddb` and deliberately **excluding** the `toml` write-back feature. That exclusion is
the documented cause of a real user-visible behaviour (`docs/AUDIT/02_STUDIO_ENGINE.md`: the
Properties panel does not persist edits in the default build). A support engineer looking at a
"my edit vanished" report cannot resolve it without knowing which features the binary carries. The
bundle must record the compiled-in feature set, not the features listed in a file.

**Environment knobs that change behaviour and must be captured** (all verified present in the
codebase): `EUSTRESS_PROFILE`, `EUSTRESS_PROFILE_FRAMES`, `EUSTRESS_PHASE_WATCHDOG_SECS`,
`EUSTRESS_LOAD_SPAWN_BUDGET`, `EUSTRESS_RESIDENCY_*`, `EUSTRESS_HLOD_RADIUS`,
`EUSTRESS_SHADOW_DISTANCE`, `EUSTRESS_SPLAT_BUDGET`, `EUSTRESS_CAPTURE`, `EUSTRESS_CAPTURE_DIR`,
`EUSTRESS_TELEMETRY_URL`.

**Privacy.** The same posture G7.30 preserved applies, with one deliberate difference: a support
bundle is produced by explicit user action for a specific support interaction, so it *may* contain
paths and Space names — but it must say so plainly on stdout before writing, and it must offer
`--redact` which replaces every absolute path with a stable hash. Never upload the bundle. Writing
it locally and telling the user where it is, is the whole feature.

**Prerequisite already satisfied.** G7.30 delivered the session ledger. The bundle includes it; do
not rebuild it.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/` — a new structured-log layer module and its registration
- `eustress/crates/engine/src/main.rs` — `LogPlugin` configuration only
- `eustress/crates/cli/src/main.rs` — the new `diag` subcommand
- `eustress/crates/cli/Cargo.toml` — only if a dependency is genuinely required; say why

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/profiler.rs` — read it as the precedent; G7.40 owns changes to it
- `eustress/crates/engine/src/usage_telemetry.rs` — G7.30 owns it; consume its output
- `eustress/crates/engine/src/bin/headless.rs` — owned by `G7.03` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Declaring a field optional
  so the completeness check passes, or shrinking the required-field list, is a measurement change.
  If a field genuinely cannot be obtained, report `EXIT_CRITERION_UNMEASURABLE` with evidence.
- The structured stream is **additive**. Human-readable console logging must keep working exactly as
  it does now; a support format that makes day-to-day development worse will be reverted.
- Log lines must be one JSON object per line (JSONL), each with at minimum `ts`, `level`, `target`,
  `msg`, and a `span` array. A pretty-printed multi-line format is not parseable by `jq` line-wise
  and fails this item.
- The bundle must be produced from disk artifacts only. Test this by killing the engine with an
  uncatchable kill and *then* running the collector.
- Do not add a network call. The bundle is written locally and its path is printed.

## 5. Exit criterion

### Criterion
After the engine is started, made to load a Space, and then killed uncatchably, a single
`eustress diag` invocation produces an archive that contains **all 9** required members and whose
log member parses as JSONL with **zero** malformed lines out of at least 200.

### Measurement

Command (bash; run from the repo root):

    cd eustress && cargo build --release --package eustress-engine --package eustress-cli && cd ..
    ./eustress/target/release/eustress-engine &
    ENGINE_PID=$!
    sleep 45
    kill -9 $ENGINE_PID
    sleep 2
    ./eustress/target/release/eustress diag --out /tmp/diag.zip --redact ; echo "EXIT=$?"
    python - <<'PY' ; echo "EXIT=$?"
    import json, zipfile, sys
    REQUIRED = {"manifest.json","logs/engine.jsonl","sessions/ledger.jsonl","crash/last_crash.json",
                "build/features.json","build/commit.txt","host/hardware.json","host/env.json",
                "phases/last_load_phases.json"}
    z = zipfile.ZipFile("/tmp/diag.zip")
    names = set(z.namelist())
    missing = REQUIRED - names
    lines = z.read("logs/engine.jsonl").decode("utf-8").splitlines()
    bad = 0
    for ln in lines:
        if not ln.strip():
            continue
        try:
            o = json.loads(ln)
            if not {"ts","level","target","msg","span"} <= set(o):
                bad += 1
        except Exception:
            bad += 1
    print("missing:", sorted(missing), "lines:", len(lines), "malformed:", bad)
    sys.exit(0 if not missing and len(lines) >= 200 and bad == 0 else 1)
    PY

Expected output shape:

    EXIT=0
    missing: [] lines: 1462 malformed: 0
    EXIT=0

Pass condition:

    Both `EXIT=0` markers present, missing == [], lines >= 200, malformed == 0.

On Windows, substitute `Stop-Process -Id $p.Id -Force` for `kill -9` and run the Python block from a
file; the assertion is identical and the same `EXIT=` markers must be grepped.

## 6. Critic gate

`critic_gate: []`. Diagnostics quality is not a perceptual property; it is a completeness property.
The mechanical criterion in §5 replaces the Critic and is deliberately strict: nine named members,
a minimum line count so an empty log cannot pass, and zero tolerance on malformed lines.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — custom_layer JSONL sink + CLI collector reading known paths
   -> if still failing, MANDATORY approach change. Adding a field is NOT an approach change;
      moving from an in-process layer to a file-rotating sidecar writer IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same member still missing
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the bundle cannot be produced from a crashed session's on-disk leftovers
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.31/diag_bundle_contract.json`

A reader finds: the nine required bundle members with a one-line description and source path for
each, the JSONL line schema with every field typed, the redaction rule, the measured line count and
malformed count from the passing run, the commit, and the exact command a support engineer is told
to run. This file is the contract a support organisation is trained against.

## 9. Definition of NOT done

- The bundle is complete only when collected from a healthy engine, and is missing the crash and
  phase members after a hard kill — which is the only case that matters.
- The JSONL stream exists in the windowed engine but not in `eustress-headless`, so a headless
  reproduction cannot be diffed against the customer's logs.
- `build/features.json` lists the features from `Cargo.toml` rather than the features actually
  compiled into the running binary, so a build with `toml` write-back off is indistinguishable from
  one with it on — the exact ambiguity behind "my Properties edit vanished".
- `--redact` hashes paths but the log `msg` bodies still contain absolute paths verbatim.
- Console output changes format, breaking the founder's existing habits and any log-grep muscle
  memory, because the structured layer replaced rather than joined the human layer.
- The collector uploads the bundle somewhere. It must not touch the network.

---

---
id: G7.32
title: Typed panic classifier and durable crash record
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.30, G7.03, G7.31]
blocks: [G7.41, G7.42, G7.45]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.32/panic_classification.json
escalation: >
  If any panic class cannot be distinguished without parsing the panic message string, STALL and
  report which class — a classifier that is still substring matching has not solved the problem it
  was funded to solve.
status: DRAFT
notes: >
  Tier M. One engine build validates the classifier and all induced panic cases if the fault
  injection points are added in the same pass.
---

## 1. Objective

Every panic that reaches the top of the engine is classified by a typed rule, written to a durable
crash record before the process leaves, and reported to the session ledger with an end reason that
is not `unknown`. A lost GPU surface no longer exits with code 0 indistinguishably from a clean
shutdown.

## 2. Context you need (self-contained)

**Project invariants.** AI-native simulation substrate, never a game engine. PolyForm Shield 1.0.0,
source-available. Avian, never Rapier. Slint is Rust. Meter-native units. 10–15 minute builds, one
at a time, shared `eustress/target/`, never killed mid-compile. `cargo run`, not `cargo check`.
Cargo runs from `eustress/`.

**The exact code you are replacing.** `eustress/crates/engine/src/main.rs:607` wraps `app.run()`:

```rust
let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { app.run(); }));
```

and the handler that follows classifies by substring on the downcast payload:

```rust
let is_gpu_surface_panic =
    msg.contains("swap chain")
    || msg.contains("Acquiring a texture")
    || msg.contains("unrecoverable")
    || msg.contains("operation unrecoverable")
    || (msg.contains("None value") && (msg.contains("uniform_buffer") || msg.contains("bevy_render")))
    || msg.contains("Buffer") && msg.contains("invalid");
```

On a match it prints a warning and calls `std::process::exit(0)`. Everything else is
`resume_unwind`'d. Three defects follow directly:

1. **`exit(0)` is a lie.** A lost surface is reported to the operating system, to any supervising
   process, and to CI as success. It also bypasses Bevy's `AppExit`, so `flush_on_exit` in
   `eustress/crates/engine/src/usage_telemetry.rs` never runs and the session's telemetry is lost.
2. **`msg.contains("unrecoverable")` is far too broad.** Any panic anywhere in any dependency whose
   message happens to contain that word is silently reclassified as a transient GPU event and
   swallowed.
3. **Nothing is written down.** There is no crash record, so a user reporting "it just closed" hands
   a support engineer nothing.

**What a "typed rule" means here.** Classification must be driven by something structural — the
panic location's module path, a typed error carried in the payload, a downcast to a known error
type, or an explicit marker set by the subsystem that is about to panic — not by the human-readable
message text. Where a class genuinely cannot be identified structurally, say so explicitly in the
artifact and classify it `unclassified` rather than guessing; an honest `unclassified` bucket with a
count is worth more than a wrong label.

**Exit code contract you are establishing.** `0` = clean shutdown only. Non-zero for everything
else, with a distinct code per class, documented in the artifact. Note that
`.github/workflows/release.yml` currently runs a Windows startup smoke test that starts the engine,
sleeps 8 seconds, and fails only if the process has already exited — it does not inspect the exit
code, so an `exit(0)` on a GPU-surface panic passes it today. You may not edit that workflow in this
item (G7.35 may), but your exit codes must make that smoke test meaningful when it is tightened.

**Prerequisite already satisfied.** G7.30 delivered the session ledger with a closed `end_reason`
enum. Extend that enum; do not create a parallel one.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/main.rs` — the catch_unwind block and its handler
- A new module under `eustress/crates/engine/src/` for the classifier and crash-record writer
- `eustress/crates/engine/src/usage_telemetry.rs` — only to widen the `end_reason` enum

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/cad/src/eval.rs` — it has its own deliberate `catch_unwind` around truck; leave it
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/bin/headless.rs` — owned by `G7.03` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Reducing the induced-panic
  set, marking a class "not reachable in practice", or asserting on the log line instead of the exit
  code are measurement changes. Report `EXIT_CRITERION_UNMEASURABLE` with evidence if the
  measurement is genuinely wrong.
- No substring matching on panic messages survives in the final classifier. A reviewer will grep for
  `.contains("` inside the classifier module and expect zero hits against panic text.
- The crash record must be written from a `panic::set_hook` handler, before unwinding completes —
  a record written after `catch_unwind` returns cannot capture an abort or a panic in a non-main
  thread.
- Preserve the genuinely useful behaviour: a truly transient lost surface should still not show the
  user a scary crash dialog. Change the *exit code* and the *record*, not the user-facing calm.
- Batch verification. Six builds is the whole budget for three approaches.

## 5. Exit criterion

### Criterion
For each of **5** induced panic classes, the engine writes a crash record whose `class` matches the
induced class and exits with the documented non-zero code for that class; and a clean quit writes no
crash record and exits `0`. Six cases, six correct outcomes, zero substring matches on panic text in
the classifier module.

### Measurement

Command (bash, from the repo root; the harness binary is authored by this item):

    cd eustress && cargo build --release --package eustress-engine && cd ..
    ./eustress/target/release/eustress-engine --panic-drill all \
        --out docs/PROMPTS/artifacts/G7.32/panic_classification.json ; echo "EXIT=$?"
    grep -c '\.contains("' eustress/crates/engine/src/crash_classify.rs ; echo "GREP_EXIT=$?"
    python -c "import json,sys; d=json.load(open('docs/PROMPTS/artifacts/G7.32/panic_classification.json')); \
      cases=d['cases']; ok=[c for c in cases if c['observed_class']==c['induced_class'] and c['observed_exit_code']==c['expected_exit_code']]; \
      print(len(ok),'/',len(cases)); \
      sys.exit(0 if len(cases)==6 and len(ok)==6 and d['substring_matches_on_panic_text']==0 else 1)" ; echo "EXIT=$?"

`--panic-drill all` runs each induced class in a fresh child process, collects the child's exit code
and the crash record it wrote, and emits the JSON. Adjust the classifier module path in the `grep`
if you name it differently; the artifact must record the path you used and its match count.

Expected output shape:

    EXIT=0
    0
    GREP_EXIT=1
    6 / 6
    EXIT=0

(`grep -c` printing `0` with exit 1 is the pass: zero substring matches.)

Pass condition:

    Final `EXIT=0`, 6/6 cases correct, and substring_matches_on_panic_text == 0.

## 6. Critic gate

`critic_gate: []`. The exit criterion is a six-case truth table plus a source-level assertion that
the old technique is gone. Both are mechanically checkable and neither can be satisfied by prose.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — panic::set_hook + typed payload/location classification
   -> if still failing, MANDATORY approach change. Adding another rule to the same rule table
      is NOT an approach change; moving classification to explicit subsystem-set markers IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations stuck at the same case count
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a class is only separable by parsing panic message text (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.32/panic_classification.json`

A reader finds: the six cases with induced class, observed class, expected and observed exit code;
the full exit-code table with one line of prose per code; the classifier module path and its
substring-match count; the list of classes that are honestly `unclassified` and why; and the commit.

## 9. Definition of NOT done

- The classifier is typed for four classes and still falls back to substring matching for the fifth.
- The crash record is written from the `catch_unwind` handler, so a panic on a Bevy task-pool thread
  or an abort produces no record.
- Exit codes are distinct but the lost-surface case still returns `0`, so a supervising process and
  CI both still read a crash as success.
- The panic hook is installed in `main.rs` only, so `eustress-headless` — the surface CI will
  actually run — has no classifier at all.
- The user-facing behaviour regresses: a transient minimised-window surface loss now shows a crash
  dialog, trading a real usability property for a bookkeeping one.
- The drill passes because the drill induces panics through a code path that only exists in the
  drill, and no production call site can reach any of the five classes.

---

---
id: G7.33
title: CI truth ledger — what ci.yml gates today versus what it must gate
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.30]
blocks: [G7.34, G7.35, G7.36, G7.43]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.33/ci_truth_ledger.json
escalation: >
  If any currently-claimed gate cannot be verified as passing or failing by reading the workflow
  files alone, STALL rather than recording an assumption — an inventory containing one guess is
  worse than no inventory.
status: DRAFT
notes: >
  Tier S with max_builds 0. This item compiles nothing. Its whole value is that the three CI items
  that follow (G7.34, G7.35, G7.36) and the regression fleet (G7.43) all cite one agreed inventory
  instead of re-deriving it three times.
---

## 1. Objective

A machine-readable ledger exists that names every check the repository's CI performs today, every
check a buyer would reasonably assume it performs, and the exact delta between the two — with each
row citing a file and line. Downstream CI items consume this ledger rather than re-reading the
workflows.

## 2. Context you need (self-contained)

**Project invariants.** Eustress is an AI-native simulation substrate, never a game engine. Licence
PolyForm Shield 1.0.0 — source-available, never open source. Physics is Avian. Slint is Rust. Units
are meter-native. Cargo runs from `eustress/`; the repo root has no `Cargo.toml`.

**The three workflow files, and nothing else.** `ls .github/workflows/` returns exactly
`ci.yml`, `linux-engine.yml`, `release.yml`. There are no other workflows.

**What `ci.yml` actually contains.** Three jobs, verified by reading the file:

1. `security` — installs `cargo-deny` and runs
   `cargo deny --config ../deny.toml check advisories bans sources` from `eustress/`. It generates a
   fresh lockfile first because `eustress/Cargo.lock` is not committed (`eustress/.gitignore`
   contains `Cargo.lock`).
2. `shader-validate` — installs `naga-cli` and validates `eustress/crates/**/*.wgsl`, but the loop
   explicitly **skips any file containing naga_oil preprocessor directives**
   (`#import`, `#ifdef`, `#{`), because bare naga cannot parse them. Bevy shaders are preprocessed by
   naga_oil, so in practice the real engine shaders are skipped, not validated.
3. `data-graph-default` — runs `cargo tree -p eustress-engine -e normal,build` and greps for
   ` eustress-data v[0-9]`, asserting the Data Platform leaf is in the default graph. Metadata only;
   nothing compiles.

**What `linux-engine.yml` contains.** One meaningful step: `cargo check --package eustress-engine`.
Its triggers are `workflow_dispatch` and pushes to the branch `ci/linux-build` only — so it does
**not** run on pushes to `main` or on pull requests.

**What `release.yml` contains.** A `verify-core` gate asserting the tag is an ancestor of `Core`,
then per-platform jobs. `grep -rn "cargo build" .github/workflows/` returns exactly three lines, all
in `release.yml` (lines 67, 143, 226), all `--package eustress-engine`. The Windows job then runs a
best-effort startup smoke test that launches the exe, sleeps 8 seconds, and fails only if the
process has already exited; the job's own comment states this is "a floor, not a full guarantee" and
that a graceful exit caused by no GPU adapter is not distinguished from success.

**Three facts that follow, all verifiable with one command each:**

- `grep -rn "cargo test" .github/workflows/` returns nothing. **No test runs in CI.**
- `grep -rn "clippy" .github/workflows/` returns nothing. **No lint runs in CI.**
- `grep -rn "eustress-client" .github/workflows/` returns nothing. `eustress/crates/client/Cargo.toml`
  declares `[[bin]] name = "eustress-client"`. **The client binary has never been built by CI.**

And a fourth: `grep -rc "#\[test\]" --include=*.rs eustress/crates | awk -F: '{s+=$2} END {print s}'`
returns **2061** (MEASURED, 2026-08-06, this repository at branch `main`). Two thousand and sixty-one
test functions exist and zero of them execute in CI.

**Why this is a business item, not a hygiene item.** `docs/AUDIT/12_INFRASTRUCTURE.md` Feature 1
records CI build state as "partial (no desktop engine)". An enterprise buyer's technical due
diligence reads `.github/workflows/` directly. The delta between what the repository implies and
what it enforces is a trust liability, and every later item in this pack is an argument that the
delta is being closed on purpose.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/G7.33/ci_truth_ledger.json` — the artifact itself

### Out of scope — do not edit
- Anything under `.github/workflows/` — this item **inventories** CI, it does not change it. Never
  modify CI to make a gate pass.
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- Any Rust source. This item compiles nothing (`max_builds: 0`).

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Recording a gate as
  "effectively covered" because a human runs it locally is a measurement change. A gate either runs
  in CI on the default branch or it does not.
- Every row must cite `file:line`. A row without a citation is inadmissible.
- Do not editorialise inside the ledger. It is data. The recommendation column holds one of exactly
  four values: `add`, `tighten`, `keep`, `remove`.
- Do not propose gates that cannot run on a GitHub-hosted runner without a GPU. Note the constraint
  in the row instead; G7.43 owns the self-hosted question.
- Count `#[test]` functions with the command given above and record the exact command in the
  artifact, so a stranger can re-derive the number.

## 5. Exit criterion

### Criterion
The ledger contains **at least 18** rows; every row has a non-empty `evidence` field matching
`^[A-Za-z0-9_./-]+:[0-9]+$` or the literal `absent`; the four "absent" facts above
(`cargo test`, `clippy`, `eustress-client`, workspace build) each appear as a row with
`present_today: false`; and `tests_defined` equals the number produced by the counting command.

### Measurement

Command (bash, from the repo root):

    python - <<'PY' ; echo "EXIT=$?"
    import json, re, subprocess, sys
    d = json.load(open("docs/PROMPTS/artifacts/G7.33/ci_truth_ledger.json"))
    rows = d["rows"]
    pat = re.compile(r"^[A-Za-z0-9_./-]+:[0-9]+$")
    bad = [r["id"] for r in rows if not (pat.match(r.get("evidence","")) or r.get("evidence")=="absent")]
    ids = {r["id"] for r in rows if r.get("present_today") is False}
    need = {"cargo_test","clippy","client_binary_build","workspace_build"}
    out = subprocess.run("grep -rc '#\\[test\\]' --include=*.rs eustress/crates | awk -F: '{s+=$2} END {print s}'",
                         shell=True, capture_output=True, text=True)
    counted = int(out.stdout.strip())
    print("rows",len(rows),"badevidence",bad,"missingabsent",sorted(need-ids),
          "tests_defined",d["tests_defined"],"counted",counted)
    sys.exit(0 if len(rows)>=18 and not bad and not (need-ids) and d["tests_defined"]==counted else 1)
    PY

Expected output shape:

    rows 21 badevidence [] missingabsent [] tests_defined 2061 counted 2061
    EXIT=0

Pass condition:

    `EXIT=0`, rows >= 18, badevidence == [], missingabsent == [], and tests_defined == counted.

## 6. Critic gate

`critic_gate: []`. An inventory is right or wrong, not beautiful. The mechanical criterion replaces
the Critic and is strict on the one thing inventories fail at: uncited rows. Every row carries a
`file:line` or admits the thing is absent.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — read all three workflow files line by line and enumerate
   -> if still failing, MANDATORY approach change. Adding rows is NOT an approach change;
      switching from prose reading to parsing the workflow YAML programmatically IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same rows still failing the evidence pattern
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: a claimed gate cannot be verified from the workflow files alone
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.33/ci_truth_ledger.json`

A reader finds: `rows` (each with `id`, `description`, `present_today`, `evidence`, `blocking_for`,
`recommendation`, `runner_constraint`), `tests_defined` with the exact counting command,
`workflow_files` with their SHA-256 at the time of inventory, and the commit. G7.34, G7.35, G7.36
and G7.43 all cite this file rather than re-reading the workflows.

## 9. Definition of NOT done

- Rows exist but several cite a file with no line number, so a reader cannot check them.
- The ledger says "tests are not run in CI" but does not carry the count, so the scale of the gap
  (2,061 functions) is invisible.
- `shader-validate` is recorded as present and passing without recording that it **skips** every
  naga_oil shader — which is every real Bevy shader — so a reader concludes shaders are validated.
- `linux-engine.yml` is recorded as a gate without recording that its triggers exclude `main` and
  pull requests, so a reader concludes Linux is checked on every change.
- The Windows startup smoke test is recorded as a crash gate without recording that it never
  inspects the exit code, so an `exit(0)` on a GPU-surface panic reads as a pass.
- The recommendation column contains prose instead of one of the four allowed values, so the ledger
  cannot be consumed programmatically by the items that depend on it.

---

---
id: G7.34
title: cargo test runs in CI on the default branch and is green
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.33]
blocks: [G7.36, G7.37, G7.43]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.34/test_gate.json
escalation: >
  If making the suite green requires marking more than 5% of the 2,061 test functions #[ignore],
  STALL — a green suite achieved by hiding a twentieth of it is a worse signal than a red one.
status: DRAFT
notes: >
  Tier M. The wall-clock risk is CI runner time, not local builds; keep the local build count low by
  running the suite locally once and iterating on the workflow against that known-good state.
---

## 1. Objective

A CI job runs the Rust test suite on every push to the default branch and on every pull request, and
it is green. The number of test functions that execute is recorded, every excluded test is named
with a reason, and a red suite blocks the merge.

## 2. Context you need (self-contained)

**Project invariants.** AI-native simulation substrate, never a game engine. PolyForm Shield 1.0.0,
source-available. Avian, never Rapier. Slint is Rust. Meter-native units. Builds take 10–15 minutes,
one at a time on a shared `eustress/target/`; never kill a build mid-compile. Validate with
`cargo run`, not `cargo check`. Cargo runs from `eustress/`.

**Starting state, measured.** `grep -rn "cargo test" .github/workflows/` returns nothing — no test
has ever run in this repository's CI. `grep -rc "#\[test\]" --include=*.rs eustress/crates | awk -F: '{s+=$2} END {print s}'`
returns **2061** (MEASURED, 2026-08-06, branch `main`). Integration `tests/` directories exist at
`eustress/crates/cad/tests`, `eustress/crates/common/tests`, and `eustress/crates/data/tests`;
`eustress/crates/common/tests/determinism.rs` is one of them.

**A known, documented hazard you must respect.** The `worlddb` test suite aborts on Windows when run
multi-threaded; the established mitigation in this project is `--test-threads=1` for database tests,
with the heavier DB tests gated behind `#[ignore]`. Do not discover this the hard way. Decide
explicitly whether the CI job runs the whole workspace single-threaded or partitions DB tests into
their own job, and record the decision.

**A second hazard: what actually compiles on a Linux runner.** The only Linux CI that exists today
(`linux-engine.yml`) runs `cargo check --package eustress-engine` and installs a long list of system
packages to do it: `libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev libvulkan-dev
libx11-dev libxi-dev libxcursor-dev libxrandr-dev libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev
libssl-dev pkg-config mold clang`. A workspace-wide `cargo test` on Ubuntu needs at least that set.
Reuse the list verbatim rather than rediscovering it.

**A third hazard: no committed lockfile.** `eustress/.gitignore` contains `Cargo.lock`, so CI
resolves fresh every run. `ci.yml`'s `security` job already handles this with an explicit
`cargo generate-lockfile` step. Do the same, or the job's first failure will be a resolution error
misread as a test failure.

**What "green" must mean.** Not "the job exits 0". It must mean: a recorded number of tests ran, that
number is at least a stated floor, and the set of skipped tests is enumerated. A suite that silently
compiles zero test targets exits 0 and proves nothing.

**Prerequisite already satisfied.** G7.33 produced `docs/PROMPTS/artifacts/G7.33/ci_truth_ledger.json`,
which names the `cargo_test` row with `present_today: false`. Update that row's status as part of
this item's artifact, not by editing G7.33's file.

## 3. Scope

### In scope — files this item may edit
- `.github/workflows/ci.yml` — **only to add a stricter gate**, never to weaken one
- Rust test code anywhere under `eustress/crates/` that is genuinely broken, to make it pass honestly
- `eustress/crates/*/Cargo.toml` — dev-dependency additions if a test needs one

### Out of scope — do not edit
- `.github/workflows/release.yml` and `.github/workflows/linux-engine.yml`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G7.33/ci_truth_ledger.json` — it is a prior item's frozen evidence
- Production (non-test) logic, except where a test exposes a real bug — and then say which, in the
  artifact, with the `path:line`

## 4. Approach constraints

- **This item deliberately overrides the standing out-of-scope entry on `.github/workflows/`**
  (`docs/PROMPTS/03_PROMPT_SCHEMA.md` §4.3). It owns the CI gate it is named for, so it may edit
  `ci.yml` — but in one direction only: it may **ADD** a gate. Weakening or removing any existing
  gate fails the item outright, whatever the resulting CI status.
- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Marking a failing test `#[ignore]`, deleting a test, narrowing the job to one crate, or setting
  `continue-on-error` are all measurement changes. If a test is genuinely wrong, fix the test and
  say so in the artifact with its path — do not hide it.
- Every `#[ignore]` you add must carry a reason string and appear in the artifact's `excluded` list.
  The cap is 5% of 2,061 (see the escalation trigger).
- Do not disable a whole crate to get green. If a crate cannot be tested on a hosted runner, record
  the reason in `blocked_crates` and keep it out of the count floor — but it must be named.
- The job must fail loudly on a resolution error rather than reporting it as a test failure; follow
  the `security` job's `cargo generate-lockfile` precedent.
- Keep local builds down. Run the suite locally once to establish the true baseline, then iterate on
  workflow syntax without rebuilding.

## 5. Exit criterion

### Criterion
The new CI job, run on the default branch, reports **at least 1800** executed test functions with
**0** failures, and the item's artifact enumerates every excluded test such that
`executed + excluded + blocked == 2061`.

### Measurement

Command (bash, run locally to establish the same numbers CI will produce):

    cd eustress && cargo generate-lockfile && \
      cargo test --workspace --no-fail-fast -- --test-threads=1 2>&1 | tee /tmp/testlog.txt ; \
      echo "EXIT=${PIPESTATUS[0]}"
    cd .. && python - <<'PY' ; echo "EXIT=$?"
    import json, re, sys
    log = open("/tmp/testlog.txt", encoding="utf-8", errors="replace").read()
    passed = sum(int(m) for m in re.findall(r"test result: ok\. (\d+) passed", log))
    failed = sum(int(m) for m in re.findall(r"(\d+) failed", log))
    d = json.load(open("docs/PROMPTS/artifacts/G7.34/test_gate.json"))
    total = d["executed"] + len(d["excluded"]) + d["blocked_count"]
    print("passed",passed,"failed",failed,"executed",d["executed"],"sum",total)
    sys.exit(0 if passed >= 1800 and failed == 0 and d["executed"] == passed and total == 2061 else 1)
    PY

Then confirm the gate is wired, not just locally green:

    grep -n "cargo test" .github/workflows/ci.yml ; echo "GREP_EXIT=$?"

Expected output shape:

    EXIT=0
    passed 1904 failed 0 executed 1904 sum 2061
    EXIT=0
    142:        run: cargo test --workspace --no-fail-fast -- --test-threads=1
    GREP_EXIT=0

Pass condition:

    Final `EXIT=0` with passed >= 1800, failed == 0, and the accounting identity
    executed + excluded + blocked == 2061; AND `GREP_EXIT=0` proving the job exists in ci.yml.

## 6. Critic gate

`critic_gate: []`. A test gate is binary. The mechanical criterion replaces the Critic and is tight
in the way test gates are usually gamed: it demands an execution count floor, zero failures, and a
full accounting identity so that "green" cannot be achieved by shrinking the suite.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — one workspace-wide single-threaded job on ubuntu-latest
   -> if still failing, MANDATORY approach change. Adding a system package is NOT an approach
      change; partitioning into per-crate jobs, or moving DB tests to their own job, IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the executed count moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: green requires marking more than 5% of 2,061 tests #[ignore]
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.34/test_gate.json`

A reader finds: `executed`, `excluded` (each entry with `path`, `test_name`, `reason`),
`blocked_crates` with reasons and `blocked_count`, the runner OS and system-package list, the
threading decision and why, the workflow job name and `ci.yml` line, any production bug the suite
exposed with its `path:line`, and the commit.

## 9. Definition of NOT done

- The job exists and exits 0 because no test target compiled. Zero executed tests is not green.
- Green is achieved by `--package eustress-common` only, so 2,061 becomes a number nobody has to
  face.
- `continue-on-error: true` appears anywhere in the job.
- The DB tests abort the runner intermittently and the fix is a retry loop rather than the
  documented single-threaded execution.
- The accounting identity is satisfied by inflating `blocked_count` to absorb everything
  inconvenient, with `blocked_crates` reasons that are one word.
- The job runs on `workflow_dispatch` only — like `linux-engine.yml` does today — so it never
  actually gates a merge.
- The item's CI licence was used in the forbidden direction: an existing gate in `ci.yml` was
  weakened, disabled, or removed. This item may only ADD; a diff that subtracts a gate fails it
  outright.

---

---
id: G7.35
title: The client binary is built and startup-smoke-tested in CI on all three platforms
workload: W3
workload_secondary: [W4]
phase: G7
depends_on: [G7.33]
blocks: [G7.43, G7.45]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.35/client_build_gate.json
escalation: >
  If the client cannot be built on a hosted runner for a reason that is not a missing system
  package — a genuine platform dependency, a GPU requirement at link time, a missing asset — STALL
  and name the reason rather than adding a self-hosted runner as a workaround.
status: DRAFT
notes: >
  Tier M. The historical failure mode here is that the client is assumed to build because the engine
  does. Build it first, locally, before touching any workflow.
---

## 1. Objective

`eustress-client` is compiled by CI on Windows, macOS, and Linux, and each build is startup-smoke-tested
with an exit-code assertion. A change that breaks the client is caught by CI rather than by a user
downloading a release.

## 2. Context you need (self-contained)

**Project invariants.** AI-native simulation substrate, never a game engine. PolyForm Shield 1.0.0,
source-available. Avian, never Rapier. Slint is Rust. Meter-native units. 10–15 minute builds, one
at a time, shared `eustress/target/`, never killed mid-compile. `cargo run`, not `cargo check`.
Cargo runs from `eustress/`.

**Starting state, measured.** `grep -rn "eustress-client" .github/workflows/` returns nothing.
`grep -rn "cargo build" .github/workflows/` returns exactly three lines — `release.yml:67`, `:143`,
`:226` — all `--package eustress-engine`. `eustress/crates/client/Cargo.toml` declares
`[package] name = "eustress-client"` and `[[bin]] name = "eustress-client", path = "src/main.rs"`.
The client binary has therefore **never been built by CI**, on any platform, in this repository's
history. That is a 0% capability and this item starts there.

**Why it matters commercially.** The client is the surface a player touches. A binary that CI has
never compiled is a binary whose breakage is discovered by whoever downloads it. Every consumer-funnel
claim rests on it.

**The precedent to follow and to improve on.** `release.yml`'s Windows job already has a startup
smoke test: it launches the exe, sleeps 8 seconds, and fails only if the process has already exited.
Its own comment concedes this is "a floor, not a full guarantee" because `windows-latest` has no
real GPU and a graceful exit for that reason is not distinguished from success. Your smoke test must
be better in exactly one way: **it asserts on the exit code**, using the codes G7.32 established. A
process that exits `0` because there is no GPU adapter must be distinguishable from a process that
exits `0` because it shut down cleanly — if it is not, the client must exit with a documented
non-zero "no adapter" code and the smoke test must accept that code explicitly as a headless-runner
outcome, while rejecting every crash code.

**System packages on Linux.** Reuse the list `linux-engine.yml` already installs:
`libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev libvulkan-dev libx11-dev libxi-dev
libxcursor-dev libxrandr-dev libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev libssl-dev
pkg-config mold clang`. The client may need more or fewer; record the final list.

**No committed lockfile.** `eustress/.gitignore` contains `Cargo.lock`. Follow `ci.yml`'s `security`
job precedent and generate one explicitly.

**Prerequisites already satisfied.** G7.33 produced the CI ledger naming `client_binary_build` as
absent. G7.32 established the exit-code table your smoke test asserts against — read
`docs/PROMPTS/artifacts/G7.32/panic_classification.json` for the codes rather than inventing new ones.

## 3. Scope

### In scope — files this item may edit
- `.github/workflows/ci.yml` — **only to add a stricter gate**
- `eustress/crates/client/` — source and `Cargo.toml`, only to make the build genuinely succeed
- `eustress/crates/client/src/main.rs` — to add the documented no-adapter exit code if absent

### Out of scope — do not edit
- `.github/workflows/release.yml` — packaging the client for release is a separate item
- `.github/workflows/linux-engine.yml`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/` — if the client fails to build because of an engine change, that is an
  escalation, not a licence to edit the engine

## 4. Approach constraints

- **This item deliberately overrides the standing out-of-scope entry on `.github/workflows/`**
  (`docs/PROMPTS/03_PROMPT_SCHEMA.md` §4.3). It owns the CI gate it is named for, so it may edit
  `ci.yml` — but in one direction only: it may **ADD** a gate. Weakening or removing any existing
  gate fails the item outright, whatever the resulting CI status.
- **Changing the measurement instead of the artifact fails this item.** Building the client with
  `--no-default-features` to dodge a broken feature, dropping a platform from the matrix, or
  replacing the exit-code assertion with a "process still alive" check are measurement changes.
- Build the client **locally first**, on Windows, before writing any workflow. Discovering a
  compile error through a 30-minute CI cycle is a budget failure.
- The smoke test asserts the exit code. "Process is alive after N seconds" alone does not pass.
- Do not add a self-hosted runner. If the client genuinely cannot build hosted, that is the
  escalation in the front matter.
- The matrix must be `windows-latest`, `macos-14`, `ubuntu-latest` — the same three platforms
  `release.yml` already targets.

## 5. Exit criterion

### Criterion
A CI job builds `eustress-client` in release mode on all **3** platforms and runs a startup smoke
test on each whose observed exit code is in the documented accept set; the artifact records all
three build durations and all three observed exit codes, and **0** platforms are skipped.

### Measurement

Local verification (bash, from the repo root) — the same assertion CI performs:

    cd eustress && cargo generate-lockfile && \
      cargo build --release --package eustress-client ; echo "BUILD_EXIT=$?"
    cd .. && ./eustress/target/release/eustress-client --smoke-exit ; echo "SMOKE_EXIT=$?"

CI verification — after the workflow run completes on the default branch:

    python - <<'PY' ; echo "EXIT=$?"
    import json, sys
    d = json.load(open("docs/PROMPTS/artifacts/G7.35/client_build_gate.json"))
    plats = d["platforms"]
    accept = set(d["accept_exit_codes"])
    ok = [p for p in plats if p["built"] and p["smoke_exit_code"] in accept]
    print("platforms", len(plats), "ok", len(ok), "skipped", d["skipped_count"])
    sys.exit(0 if len(plats) == 3 and len(ok) == 3 and d["skipped_count"] == 0 else 1)
    PY
    grep -n "eustress-client" .github/workflows/ci.yml ; echo "GREP_EXIT=$?"

Expected output shape:

    BUILD_EXIT=0
    SMOKE_EXIT=0
    platforms 3 ok 3 skipped 0
    EXIT=0
    168:        run: cargo build --release --package eustress-client
    GREP_EXIT=0

Pass condition:

    `EXIT=0` with 3 platforms, 3 ok, 0 skipped; AND `GREP_EXIT=0`. `accept_exit_codes` must be a
    closed list that excludes every crash code from G7.32's table — a set containing every integer
    fails the item.

## 6. Critic gate

`critic_gate: []`. Building a binary is not a perceptual property. The mechanical criterion is tight
where this class of gate is normally gamed: the accept set is closed and must exclude crash codes,
and the skipped-platform count must be exactly zero.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — matrix job mirroring release.yml's three platforms
   -> if still failing, MANDATORY approach change. Adding a system package is NOT an approach
      change; splitting the client into a headless-verifiable entry point IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same platform still failing to compile
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a hosted-runner build failure whose cause is not a missing system package
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.35/client_build_gate.json`

A reader finds: the three platforms with `built`, `build_seconds`, `smoke_exit_code`, and runner
image; `accept_exit_codes` with one line of prose per code explaining why it is acceptable on a
GPU-less runner; the system-package list used on Linux; `skipped_count`; the `ci.yml` job name and
line; any client source change required, with `path:line`; and the commit.

## 9. Definition of NOT done

- The job builds the client on Linux only, because that runner was easiest, and the matrix is
  recorded as "3 platforms, 2 skipped".
- The smoke test passes because it asserts the process is alive, so a client that will exit with a
  crash code one second later is recorded green.
- `accept_exit_codes` includes a crash code from G7.32's table, making the assertion vacuous.
- The client builds only with `--no-default-features`, so what CI proves is not what a user runs.
- The job is added to `release.yml` instead of `ci.yml`, so it runs on tags only and never gates a
  merge.
- Building the client required editing the engine, and that edit was made silently rather than
  escalated.
- The item's CI licence was used in the forbidden direction: an existing gate in `ci.yml` was
  weakened, disabled, or removed. This item may only ADD; a diff that subtracts a gate fails it
  outright.

---

---
id: G7.36
title: Clippy and rustfmt gate at deny-warnings on a named crate set with a written expansion ladder
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.34]
blocks: [G7.43]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.36/lint_gate.json
escalation: >
  If reaching zero warnings on the named crate set requires more than 25 #[allow] attributes, STALL —
  an allow-riddled gate teaches the team that the gate is decorative.
status: DRAFT
notes: >
  Tier M. Deliberately scoped to a named crate set rather than the whole workspace: a 39-crate
  deny-warnings sweep is an XL item and would consume the pack's build budget. The expansion ladder
  is the deliverable that makes the narrow start honest.
---

## 1. Objective

`cargo clippy -- -D warnings` and `cargo fmt --check` run in CI over a named set of crates and pass
with zero warnings, and a written ladder states which crates join the gate next and what must be
true before each joins. The gate is narrow on purpose and says so.

## 2. Context you need (self-contained)

**Project invariants.** AI-native simulation substrate, never a game engine. PolyForm Shield 1.0.0,
source-available. Avian, never Rapier. Slint is Rust — `.slint` files compile to Rust, so
Slint-generated code is Rust code and must be excluded from `fmt --check` deliberately, with the
exclusion recorded. Meter-native units. 10–15 minute builds, one at a time, shared
`eustress/target/`, never killed mid-compile. `cargo run`, not `cargo check`. Cargo runs from
`eustress/`.

**Starting state, measured.** `grep -rn "clippy" .github/workflows/` returns nothing. No lint gate
has ever existed in this repository's CI. The workspace at `eustress/crates/` contains 39 crate
directories.

**Why the whole workspace is the wrong first target.** `eustress/crates/engine/src/ui/slint_ui.rs`
is **23,103 lines** (MEASURED via `wc -l`, 2026-08-06) — the single largest file in the repository
and, per `docs/PROMPTS/00_MASTER_PROTOCOL.md`'s G6 row, a known structural liability with its own
item. A deny-warnings sweep that includes it will consume this item's entire budget in that one
file and produce a gate nobody can keep green. Start with crates whose surface is small and whose
correctness matters most, and write down the ladder for the rest.

**Suggested starting set** (you may justify a different one in the artifact, but it must be at least
five crates and must include the first two): `eustress-common`, `eustress-worlddb`,
`eustress-cad`, `eustress-tools`, `eustress-cli`. Rationale to record: `common` and `worlddb` are
where a silent data-loss defect costs the most; `tools` is the MCP surface an external agent drives;
`cli` is what a support engineer runs.

**Generated code.** Some source in this tree is generated rather than authored — `tool_metadata.rs`
and the `tools/*.svg` set are known generated artifacts. Generated files must be excluded from
`fmt --check` explicitly and by path, and the exclusion recorded in the artifact. Reformatting a
generated file guarantees the gate goes red the next time the generator runs.

**Prerequisite already satisfied.** G7.34 delivered a green test job in `ci.yml`. Add the lint job
beside it and reuse its caching and lockfile-generation steps rather than inventing new ones.

## 3. Scope

### In scope — files this item may edit
- `.github/workflows/ci.yml` — **only to add a stricter gate**
- Source under the named crate set, to fix real lints
- `rustfmt.toml` or `.rustfmt.toml` at `eustress/`, if formatting policy needs stating
- `clippy.toml` at `eustress/`, if a lint threshold needs stating

### Out of scope — do not edit
- `.github/workflows/release.yml` and `.github/workflows/linux-engine.yml`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/ui/slint_ui.rs` — out of scope by design; it has its own item
- Any generated file. Exclude it; do not reformat it.

## 4. Approach constraints

- **This item deliberately overrides the standing out-of-scope entry on `.github/workflows/`**
  (`docs/PROMPTS/03_PROMPT_SCHEMA.md` §4.3). It owns the CI gate it is named for, so it may edit
  `ci.yml` — but in one direction only: it may **ADD** a gate. Weakening or removing any existing
  gate fails the item outright, whatever the resulting CI status.
- **Changing the measurement instead of the artifact fails this item.** Adding a crate-level
  `#![allow(clippy::all)]`, downgrading `-D warnings` to `-W`, or excluding a file because it is
  noisy are measurement changes. Report `EXIT_CRITERION_UNMEASURABLE` with evidence if a lint is
  genuinely wrong for this codebase — and then encode that judgement in `clippy.toml`, once, with a
  comment, rather than scattering `#[allow]`.
- Every `#[allow]` you add must be narrowly scoped (item level, not crate level) and must carry a
  one-line reason comment. They are counted; the cap is 25 (front matter).
- Fixing a lint must not change behaviour. If a lint reveals a real bug, fix the bug, and record it
  in the artifact with `path:line` — that is the most valuable output this item can produce.
- The ladder is not optional prose. Each future crate gets a row with a named precondition.
- Do not run clippy across the workspace "just to see" more than once; each full pass is expensive.

## 5. Exit criterion

### Criterion
`cargo clippy -p <each crate in the set> --all-targets -- -D warnings` exits **0** for every crate
in a set of **at least 5**, `cargo fmt --check` exits **0** over the same set, the added `#[allow]`
count is **at most 25**, and the ladder covers **every** remaining workspace crate directory with a
named precondition.

### Measurement

Command (bash, from the repo root):

    cd eustress && cargo generate-lockfile
    FAIL=0
    for c in eustress-common eustress-worlddb eustress-cad eustress-tools eustress-cli; do
      cargo clippy -p "$c" --all-targets -- -D warnings || FAIL=1
    done
    cargo fmt --check -p eustress-common -p eustress-worlddb -p eustress-cad \
                      -p eustress-tools -p eustress-cli || FAIL=1
    echo "LINT_EXIT=$FAIL"
    cd .. && python - <<'PY' ; echo "EXIT=$?"
    import json, subprocess, sys
    d = json.load(open("docs/PROMPTS/artifacts/G7.36/lint_gate.json"))
    gated_dirs = set(d["gated_crate_dirs"])
    ladder_dirs = {r["dir"] for r in d["ladder"]}
    out = subprocess.run("ls eustress/crates", shell=True, capture_output=True, text=True)
    dirs = {x for x in out.stdout.split() if x}
    uncovered = dirs - gated_dirs - ladder_dirs
    print("gated", len(gated_dirs), "allows", d["added_allow_count"],
          "ladder_rows", len(d["ladder"]), "crate_dirs", len(dirs), "uncovered", sorted(uncovered))
    sys.exit(0 if len(gated_dirs) >= 5 and d["added_allow_count"] <= 25
             and all(r.get("precondition") for r in d["ladder"])
             and not uncovered else 1)
    PY
    grep -n "clippy" .github/workflows/ci.yml ; echo "GREP_EXIT=$?"

Expected output shape:

    LINT_EXIT=0
    gated 5 allows 11 ladder_rows 34 crate_dirs 39 uncovered []
    EXIT=0
    191:        run: cargo clippy -p eustress-common --all-targets -- -D warnings
    GREP_EXIT=0

Pass condition:

    `LINT_EXIT=0`, `EXIT=0` with gated >= 5, allows <= 25, every ladder row carrying a non-empty
    precondition, and no uncovered crate directory; AND `GREP_EXIT=0`.

## 6. Critic gate

`critic_gate: []`. Lint cleanliness is mechanical. The criterion is strict on the two ways this gate
is normally faked: the `#[allow]` count is capped and counted, and the ladder must account for
*every* crate directory so that "we gated five" cannot quietly mean "we ignored thirty-four".

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — fix lints crate by crate in the named set
   -> if still failing, MANDATORY approach change. Adding another #[allow] is NOT an approach
      change; encoding a codebase-wide judgement once in clippy.toml IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the remaining warning count moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: more than 25 #[allow] attributes required (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.36/lint_gate.json`

A reader finds: `gated_crate_dirs` with the rationale for each; `added_allow_count` and every allow
with `path:line` and its reason; `generated_exclusions` by path; `ladder` — one row per remaining
crate directory with `dir`, `precondition`, and `estimated_tier`; any real bug the lints exposed
with `path:line`; the `ci.yml` job name and line; and the commit.

## 9. Definition of NOT done

- The gate is green because `-D warnings` was quietly softened to `-W warnings` in the workflow.
- A crate-level `#![allow(clippy::all)]` appears anywhere in the gated set.
- The ladder exists but half its rows have an empty or one-word precondition, so it is a list of
  crate names rather than a plan.
- A generated file was reformatted to satisfy `fmt --check`, and the next generator run will turn
  the gate red with no code change.
- A lint revealed a genuine bug and it was silenced rather than fixed and recorded — the single
  highest-value output of this item, thrown away.
- The job runs on `workflow_dispatch` only and never gates a merge.
- The item's CI licence was used in the forbidden direction: an existing gate in `ci.yml` was
  weakened, disabled, or removed. This item may only ADD; a diff that subtracts a gate fails it
  outright.

---

---
id: G7.37
title: Property-based Space save/load round-trip with a measured fidelity floor
workload: W3
workload_secondary: [W1]
phase: G7
depends_on: [G7.34, G1.03, G7.08]
blocks: [G7.38, G7.43, G7.45]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.37/roundtrip_property.json
escalation: >
  If the generator finds a class of round-trip loss that cannot be fixed inside this item's scope —
  a property that is lost because the on-disk schema has no field for it — STALL immediately with
  the minimal shrunk counterexample rather than narrowing the generator to avoid it.
status: DRAFT
notes: >
  Tier L. Property testing is new to this workspace (no proptest or quickcheck dependency exists
  anywhere today), so the item carries both the harness and the first real property.
---

## 1. Objective

A property-based test asserts that a Space survives a save/load round-trip with no loss of authored
state. It generates randomised scene graphs — nesting, transforms, class variety, name collisions —
runs each through the real save and load paths, and compares the reloaded graph against the
original. Any divergence produces a shrunk, reproducible counterexample.

## 2. Context you need (self-contained)

**Project invariants.** Eustress is an AI-native simulation substrate, never a game engine. Licence
PolyForm Shield 1.0.0 — source-available. Physics is Avian, never Rapier. Slint is Rust. **Units are
meter-native; studs are a display unit only** — this matters here, because a round-trip that
converts to studs and back is exactly the kind of silent precision loss this item exists to catch.
Builds take 10–15 minutes, one at a time, shared `eustress/target/`, never killed mid-compile.
`cargo run`, not `cargo check`. Cargo runs from `eustress/`.

**Starting state, measured.** `grep -rn "proptest\|quickcheck" --include=Cargo.toml eustress/crates`
returns **nothing**. There is no property-testing dependency anywhere in this workspace. This
capability is 0% and this item starts there.

**The two persistence layers, and which one is authoritative.** `eustress/crates/engine/Cargo.toml`
sets `default = ["core", "data"]`, and the `core` tier includes `worlddb`. Its own comment states the
model plainly: Fjall is **authoritative**; TOML seeds the database on first open and **is not kept in
sync afterwards**. The `toml` write-back feature is deliberately **not** in the `core` tier, so on a
normal build `write_instance_definition` skips the disk write for every instance
`active_db::put_instance` accepts. `eustress/crates/engine/src/space/world_db_plugin.rs:25` carries
the same statement in its module docs. A round-trip property that only exercises TOML therefore
tests a path a shipped build does not take.

**The save path.** `eustress/crates/engine/src/space/space_ops.rs:352` defines
`pub fn save_space(world: &mut World)`. It walks `Instance` + `BasePart` entities and writes each
back to its `.part.toml` / `_instance.toml`. Its own comments record a real historical defect and
its fix: writing the **global** transform instead of the **local** one made every nested
save-then-reload drift the part by its parent's transform, so a grouped or folder-nested part
accumulated error while a top-level part under an identity-transform `Workspace` did not. That is
precisely the shape of bug a property test finds and an example-based test does not — because it
only appears under nesting.

**Migration state matters.** `eustress/crates/engine/src/space/space_ops.rs:41` defines
`space_is_migrated(space_root) -> bool`, keyed on a `migrated_at` stamp in `header.bin`. Migrated and
non-migrated Spaces take different load paths. Your generator must cover both, or you have tested
half the product.

**What a Space directory is.** A `.eustress` world container is a directory holding `world.fjalldb/`
(the Fjall log-structured-merge-tree database — live entity-component-system state), `header.bin`
(world identity and schema version), human-editable `schema/` and service folders, and after a
publish bake `chunks/*.echk` plus `manifest.toml`.

**Prerequisite already satisfied.** G7.34 put `cargo test --workspace` in CI and green. Your property
test must run inside that job — a property test that only runs locally is a private opinion.

## 3. Scope

### In scope — files this item may edit
- A new integration test directory/file under `eustress/crates/engine/tests/` or
  `eustress/crates/worlddb/tests/`
- `eustress/crates/engine/Cargo.toml` and/or `eustress/crates/worlddb/Cargo.toml` — dev-dependencies
  only (a property-testing crate)
- `eustress/crates/engine/src/space/space_ops.rs` — only to fix a defect the property test finds
- `eustress/crates/worlddb/src/` — only to fix a defect the property test finds

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/ui/` — the UI is not on the round-trip path
- Anything under `eustress/crates/common/src/physics/`
- Any existing entry in `eustress/crates/engine/Cargo.toml` — the file is owned by `G1.03`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.
- The layout of `eustress/crates/worlddb/src/` — the directory is owned by `G7.08`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Shrinking the generator's depth or class variety, excluding a component from the comparison,
  loosening a float tolerance beyond the one declared below, or reducing the case count are all
  measurement changes. If a property is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with
  the shrunk counterexample and stop.
- Float comparison uses one declared tolerance, stated in the artifact, applied uniformly. Positions
  and sizes are meters. A tolerance looser than `1e-4 m` must be justified in the artifact with the
  storage precision that forces it.
- The generator must produce, at minimum: nesting depth 0–5; sibling name collisions; non-identity
  parent rotations *and* scales (the historical drift bug needs both); at least six distinct classes;
  negative scale on at least one part; and both migrated and non-migrated Spaces.
- Round-trip through the **authoritative** path a default build uses. If you also test the TOML path,
  label it separately; it does not substitute.
- Every failure must reproduce from a recorded seed. A flaky property test is worse than none.
- Batch your builds. Twelve is the whole budget for three approaches.

## 5. Exit criterion

### Criterion
The property test runs **at least 1000** generated cases with **0** failures, over a generator whose
recorded coverage includes all six required shapes above; and the artifact records at least one
shrunk counterexample that the generator found and the item fixed, with its `path:line`.

### Measurement

Command (bash, from the repo root):

    cd eustress && PROPTEST_CASES=1000 cargo test --package eustress-engine \
      --test space_roundtrip -- --test-threads=1 --nocapture 2>&1 | tee /tmp/rt.txt ; \
      echo "EXIT=${PIPESTATUS[0]}"
    cd .. && python - <<'PY' ; echo "EXIT=$?"
    import json, sys
    d = json.load(open("docs/PROMPTS/artifacts/G7.37/roundtrip_property.json"))
    need = {"nesting_depth_5","sibling_name_collision","parent_rotation","parent_scale",
            "class_variety_6","negative_scale","migrated_space","unmigrated_space"}
    cov = set(d["generator_coverage"])
    print("cases",d["cases_run"],"failures",d["failures"],
          "missing_coverage",sorted(need-cov),"fixed_defects",len(d["defects_found"]))
    sys.exit(0 if d["cases_run"] >= 1000 and d["failures"] == 0
             and not (need - cov) and len(d["defects_found"]) >= 1
             and all(x.get("evidence") for x in d["defects_found"]) else 1)
    PY

Expected output shape:

    EXIT=0
    cases 1000 failures 0 missing_coverage [] fixed_defects 2
    EXIT=0

Pass condition:

    Both `EXIT=0` markers, cases_run >= 1000, failures == 0, no missing coverage, and at least one
    recorded defect carrying a `path:line` evidence field.

If the generator genuinely finds no defect after 1,000 cases across all eight coverage shapes, that
is a legitimate outcome — record `defects_found` with a single entry of kind `none_found` carrying
the seed range searched as its evidence, and say so plainly in the artifact.

## 6. Critic gate

`critic_gate: []`. Round-trip fidelity is an exact property, not a perceptual one. The mechanical
criterion is unusually tight to compensate: a case-count floor, a zero-failure requirement, an
enumerated coverage set the generator must actually hit, and a requirement that the item report what
it found rather than only that it passed.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — proptest strategies over a scene-graph model, round-trip via the
                 authoritative WorldDb path
   -> if still failing, MANDATORY approach change. Raising the case count is NOT an approach
      change; switching from a generated scene graph to a generated *mutation sequence* replayed
      against a reference model IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same shrunk counterexample unfixed
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a loss class with no on-disk field to carry it (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.37/roundtrip_property.json`

A reader finds: `cases_run`, `failures`, the seed and generator version, `generator_coverage` as the
list of shapes actually exercised, the float tolerance and its justification, `defects_found` — each
with a shrunk counterexample, the `path:line` of the fix, and whether it was a save-side or load-side
loss — the test path and the CI job it runs in, and the commit.

## 9. Definition of NOT done

- The property passes because the generator never nests deeper than one level, so the historical
  parent-transform drift class cannot be reached.
- The round-trip goes through TOML on a build with the `toml` feature enabled, so the path a shipped
  default build takes is untested.
- Only migrated Spaces are generated, so the `space_is_migrated` branch is half-covered.
- The comparison ignores a component "because it is derived", and that component is exactly where
  the loss is.
- The test is marked `#[ignore]` so the G7.34 job stays green, making it a local-only opinion.
- The float tolerance was widened from `1e-4 m` to something forgiving in order to pass, with no
  storage-precision justification.
- A failure was observed once, could not be reproduced, and was recorded as flaky rather than
  pinned to a seed.

---

---
id: G7.38
title: World-container integrity verifier and the truth about a Space in git
workload: W3
workload_secondary: [W4]
phase: G7
depends_on: [G7.37, G7.08]
blocks: [G7.44, G7.45]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.38/container_integrity.json
escalation: >
  If a Space cannot be made portable through git without changing the on-disk storage format, STALL
  and state that plainly with the evidence — do not ship a partial export that silently drops live
  state, which is the exact failure this item exists to end.
status: DRAFT
notes: >
  Tier M. The verifier is small. The value is that it converts a folklore failure ("committing a
  Space loses your edits") into a command that says so, out loud, before the user loses anything.
---

## 1. Objective

A command inspects a `.eustress` world container and reports its integrity: whether the Fjall
database opens, whether `header.bin` is readable and its migration stamp consistent with what is on
disk, whether the TOML seed and the authoritative database agree, and — critically — whether the
container as it sits in a git working tree would carry its live state to another machine. The
answer, whichever way it goes, is stated and recorded, not left to folklore.

## 2. Context you need (self-contained)

**Project invariants.** AI-native simulation substrate, never a game engine. PolyForm Shield 1.0.0,
source-available. Avian, never Rapier. Slint is Rust. Meter-native units. 10–15 minute builds, one
at a time, shared `eustress/target/`, never killed mid-compile. `cargo run`, not `cargo check`.
Cargo runs from `eustress/`.

**What a world container is.** A `.eustress` directory holds `world.fjalldb/` (the Fjall
log-structured-merge-tree key-value store — the live entity-component-system state), `header.bin`
(world identity and schema version, carrying the `migrated_at` stamp), human-editable `schema/` and
service folders, `Workspace/` with `_instance.toml` seed files, and after a publish bake
`chunks/*.echk` plus `manifest.toml`.

**Which layer is authoritative.** `eustress/crates/engine/Cargo.toml`'s `core` feature tier states
it directly: Fjall is authoritative; TOML seeds the database on first open and is **not** kept in
sync afterwards; the `toml` write-back feature is deliberately excluded from `core`.
`eustress/crates/engine/src/space/world_db_plugin.rs:25` repeats it. Therefore: **the TOML you can
read in the working tree is the seed, not the state.**

**The git question, measured.** `git ls-files | grep -ci "fjalldb"` returns **0** in this repository
(MEASURED, 2026-08-06, branch `main`). Not one Fjall database file is tracked. Separately, the root
`.gitignore` contains broad patterns — `*.log`, `logs/`, `**/debug/`, `*.tmp`, `tmp/`, `temp/`,
`.cache/` — that a log-structured-merge-tree's internal file names can collide with. Do not assert a
mechanism you have not verified: **this item's job is to determine, by experiment, exactly which
files of a real `world.fjalldb/` git would and would not carry**, and to make the verifier report it.
Run `git check-ignore -v` against every file in a real container and record the result.

**Why this is a durability item and not a git-hygiene item.** If a user commits a Space, pushes, and
clones it elsewhere, and the authoritative store did not travel, then the clone silently reconstructs
from the stale TOML seed and every edit made since the first open is gone — with no error, because
seeding from TOML is a legitimate first-open path. The user experiences it as "my work vanished".
A verifier that says so before the push is the fix; a verifier that says so after is still worth
having.

**Reconcile-on-open exists and must not be confused with a repair tool.**
`eustress/crates/engine/src/space/load_phase.rs:297` shows the phase structure — `space-open`
encloses `db-reconcile` — and `eustress/crates/engine/src/space/world_db_plugin.rs` runs the
reconcile. Reconcile makes the database consistent with what it finds. It cannot resurrect state
that never arrived.

**Prerequisite already satisfied.** G7.37 established that a save/load round-trip preserves authored
state within a declared tolerance. This item asks the next question: does the container survive being
*moved*.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/cli/src/main.rs` — a new `space verify` subcommand
- `eustress/crates/worlddb/src/` — a read-only integrity check API
- `.gitattributes` or `.gitignore` — only if the experiment shows a fix belongs there, and only
  additively

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/space/world_db_plugin.rs` — reconcile behaviour is not this item's
- Any user's real Space. Operate on a container this item creates.
- The layout of `eustress/crates/worlddb/src/` — the directory is owned by `G7.08`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Declaring the container
  "portable" because the TOML seed travels is a measurement change: the seed is not the state.
  Report `EXIT_CRITERION_UNMEASURABLE` with evidence if the check is genuinely wrong.
- The verifier is **read-only**. It must never write to, compact, or repair the database it is
  inspecting. A verifier that mutates is a corruption risk during an incident.
- Determine the git behaviour by experiment (`git check-ignore -v` over every container file), not by
  reading `.gitignore` and reasoning. Record the raw result.
- The portability verdict is one of exactly three values: `portable`, `seed_only`, `corrupt`. No
  fourth value, no prose verdict.
- The command must work with the engine **not running**. An integrity check you can only perform from
  inside the process cannot be used during an incident.

## 5. Exit criterion

### Criterion
Against **3** deliberately constructed containers — one healthy, one whose authoritative store was
removed to simulate a git round-trip, and one with a truncated `header.bin` — the verifier returns
the correct verdict for each (`portable`, `seed_only`, `corrupt` respectively) with a **non-zero,
distinct exit code** for the two unhealthy cases, and the artifact records the raw `git check-ignore`
result for every file in the healthy container.

### Measurement

Command (bash, from the repo root):

    cd eustress && cargo build --release --package eustress-cli --package eustress-engine && cd ..
    ./eustress/target/release/eustress space verify --space /tmp/tc_healthy   --json ; echo "V1=$?"
    ./eustress/target/release/eustress space verify --space /tmp/tc_seedonly  --json ; echo "V2=$?"
    ./eustress/target/release/eustress space verify --space /tmp/tc_corrupt   --json ; echo "V3=$?"
    python - <<'PY' ; echo "EXIT=$?"
    import json, sys
    d = json.load(open("docs/PROMPTS/artifacts/G7.38/container_integrity.json"))
    cases = {c["name"]: c for c in d["cases"]}
    want = {"healthy": "portable", "seedonly": "seed_only", "corrupt": "corrupt"}
    ok = all(cases.get(k, {}).get("verdict") == v for k, v in want.items())
    codes = {cases[k]["exit_code"] for k in ("seedonly", "corrupt")}
    print("verdicts_ok", ok, "unhealthy_codes", sorted(codes),
          "healthy_code", cases["healthy"]["exit_code"],
          "checkignore_rows", len(d["git_check_ignore"]))
    sys.exit(0 if ok and 0 not in codes and len(codes) == 2
             and cases["healthy"]["exit_code"] == 0
             and len(d["git_check_ignore"]) > 0 else 1)
    PY

The three containers are built by this item; record the exact construction steps in the artifact so a
stranger can rebuild them.

Expected output shape:

    V1=0
    V2=3
    V3=4
    verdicts_ok True unhealthy_codes [3, 4] healthy_code 0 checkignore_rows 214
    EXIT=0

Pass condition:

    `EXIT=0`: all three verdicts correct, the healthy case exits 0, the two unhealthy cases exit
    with two distinct non-zero codes, and the `git check-ignore` evidence is non-empty.

## 6. Critic gate

`critic_gate: []`. Container integrity is a three-valued fact. The mechanical criterion replaces the
Critic and is tight where this class of tool is normally weak: it requires the *unhealthy* cases to
be detected and distinguished, not merely the healthy one to pass.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — read-only open + header parse + seed/state divergence check
   -> if still failing, MANDATORY approach change. Adding a check is NOT an approach change;
      moving from opening the database to inspecting its on-disk file set without opening it IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the seed_only case is still misreported
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: portability requires an on-disk format change (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.38/container_integrity.json`

A reader finds: the three cases with `verdict`, `exit_code`, and the construction steps that produced
each; `git_check_ignore` — the raw per-file result over a real healthy container; the exit-code table
with one line of prose per code; whether any fix was applied and where; an explicit statement of what
a user should do today to move a Space between machines; and the commit.

## 9. Definition of NOT done

- The verifier reports `portable` for the seed-only container because the TOML is present and
  readable — the exact confusion this item exists to eliminate.
- The verdict is correct but every case exits 0, so no script or CI job can act on it.
- The git behaviour is asserted from reading `.gitignore` rather than measured with
  `git check-ignore`, so the claim is a deduction and not evidence.
- The verifier opens the database read-write and compacts it as a side effect, so running it during
  an incident changes the thing being investigated.
- The tool requires a running engine, so it is unusable in the situation it was built for.
- The artifact says a Space is not portable but gives the user no statement of what to do instead.

---

---
id: G7.39
title: Machine-readable stuck-phase record, reported safely with no desktop session
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.31, G7.03]
blocks: [G7.42, G7.43, G7.45, G3.12]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.39/stuck_phase_record.json
escalation: >
  If any load phase cannot be given a matching end() on every exit path — so that a legitimate early
  return is permanently reported as stuck — STALL and name the phase rather than raising the
  threshold to hide it.
status: DRAFT
notes: >
  Tier M. The watchdog already exists and works; this item makes its output machine-readable and
  safe to run where there is no display, which is where CI runs it.
---

## 1. Objective

When a Space load phase runs past its threshold, the engine writes a structured record naming the
phase, its detail string, and its elapsed time — to disk, on every surface, including
`eustress-headless` where no window system exists. The record is a member of the G7.31 support
bundle, and the native dialog is used only where a display is present.

## 2. Context you need (self-contained)

**Project invariants.** AI-native simulation substrate, never a game engine. PolyForm Shield 1.0.0,
source-available. Avian, never Rapier. Slint is Rust. Meter-native units. 10–15 minute builds, one
at a time, shared `eustress/target/`, never killed mid-compile. `cargo run`, not `cargo check`.
Cargo runs from `eustress/`.

**What already exists and works — do not rebuild it.**
`eustress/crates/engine/src/space/load_phase.rs` implements an always-armed load-phase watchdog. Its
design, verbatim from its own module comments, is deliberate on three points: it is **always armed**
(not gated on `EUSTRESS_PROFILE`, because "a hang the user is staring at must report itself without
them having known to set an env var beforehand"); it is **off-thread** ("a dedicated watchdog thread
owns the timing, because the thread that would normally notice is the blocked one"); and it is
**escalating** — log first, then a queued toast, then a native dialog.

Concrete details you will need:
- `const WATCHDOG_DEFAULT_SECS: u64 = 30` (CONFIG DEFAULT), overridable by
  `EUSTRESS_PHASE_WATCHDOG_SECS`, where `0` disables the watchdog entirely
  (`watchdog_threshold()`, around line 188).
- `begin(name, detail)` and `end(name)` bracket a phase. Re-entering the same phase name **replaces**
  the older entry rather than stacking, so a re-opened Space cannot leak a permanently-stuck ghost.
- The doc comment on `begin` states the contract plainly: pair with `end` on **every** exit path, and
  an early `return` that skips `end` is itself reported as a stuck phase — "which is the intended
  behaviour (a phase that silently bailed is unfinished work)".
- Phases overlap: `space-open` encloses `db-reconcile` (see the comment near line 297).
- `show_stuck_dialog` (around line 384) builds a human message and calls `rfd::MessageDialog`, on its
  own thread because `show()` blocks, with a `DIALOG_OPEN` latch cleared on dismissal.

**The two gaps this item closes.**

1. **Nothing machine-readable is produced.** The report is a log line, a toast, and a modal. None of
   them is a file another tool can read, so the stuck-phase signal cannot enter the G7.31 support
   bundle, cannot be asserted on in a test, and cannot be counted over time.
2. **`rfd::MessageDialog` needs a display.** `eustress/crates/engine/src/bin/headless.rs` is
   `MinimalPlugins` plus `ScheduleRunnerPlugin` — no winit, no GPU, no Slint (its own module docs say
   so). On a CI runner or a container there is no window system at all. A watchdog whose loudest
   escalation cannot run on the surface CI uses is silent exactly where nobody is watching.

**Prerequisite already satisfied.** G7.31 defined the support bundle and named
`phases/last_load_phases.json` as one of its nine required members. This item produces that file.
Read `docs/PROMPTS/artifacts/G7.31/diag_bundle_contract.json` for its expected shape rather than
inventing one.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/space/load_phase.rs`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/ui/` — the toast surface stays as it is
- The default threshold. Thirty seconds is a deliberate choice ("a false popup is worse than a late
  one"); changing it is not this item's business.
- `eustress/crates/engine/src/bin/headless.rs` — owned by `G7.03` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Raising the threshold,
  disabling the watchdog in headless mode, or suppressing a phase because it is "known slow" are
  measurement changes.
- Preserve all three existing properties: always armed, off-thread, escalating. A record written
  from the blocked main thread is worthless — the whole point is that the main thread is stuck.
- Detect display availability rather than assuming it. Selecting the dialog path by `cfg` on the
  binary is acceptable only if you also handle a windowed binary launched with no display; state
  which mechanism you used.
- The record must be written **while the phase is still stuck**, not after it completes. A record
  written on resolution cannot be collected from a process that never resolves.
- Exactly one record per stuck phase, matching the existing `reported` latch semantics — a watchdog
  that writes a file every tick fills a disk during an incident.

## 5. Exit criterion

### Criterion
With `EUSTRESS_PHASE_WATCHDOG_SECS=2` and a deliberately stalled phase, both the windowed engine and
`eustress-headless` write a stuck-phase record containing the phase name, the detail string, and an
`elapsed_s` of **at least 2.0**; the headless run writes it **without a display** and does **not**
attempt a native dialog; and exactly **1** record is written for a phase stuck for 10 seconds.

### Measurement

Command (bash, from the repo root; `--stall-phase` is a debug flag this item adds):

    cd eustress && cargo build --release --package eustress-engine && cd ..
    rm -rf /tmp/phaserec && mkdir -p /tmp/phaserec
    EUSTRESS_PHASE_WATCHDOG_SECS=2 EUSTRESS_PHASE_RECORD_DIR=/tmp/phaserec \
      ./eustress/target/release/eustress-headless --space /tmp/tc_healthy --ticks 1 \
      --stall-phase db-reconcile=10 ; echo "HEADLESS_EXIT=$?"
    python - <<'PY' ; echo "EXIT=$?"
    import json, glob, sys
    files = sorted(glob.glob("/tmp/phaserec/*.json"))
    recs = [json.load(open(f)) for f in files]
    stuck = [r for r in recs if r["phase"] == "db-reconcile"]
    print("files", len(files), "stuck_records", len(stuck),
          "elapsed", [round(r["elapsed_s"], 2) for r in stuck],
          "dialog_attempted", any(r.get("dialog_attempted") for r in stuck))
    sys.exit(0 if len(stuck) == 1 and stuck[0]["elapsed_s"] >= 2.0
             and stuck[0].get("detail") and not stuck[0].get("dialog_attempted") else 1)
    PY

Then repeat the same run against the windowed binary, where `dialog_attempted` **must** be `true`,
and record both outcomes in the artifact.

Expected output shape:

    HEADLESS_EXIT=0
    files 1 stuck_records 1 elapsed [10.01] dialog_attempted False
    EXIT=0

Pass condition:

    `EXIT=0`: exactly one record, `elapsed_s >= 2.0`, a non-empty detail string, and
    `dialog_attempted == false` on the headless surface — with the windowed run recorded separately
    as `dialog_attempted == true`.

## 6. Critic gate

`critic_gate: []`. This is an observability contract, not a perceptual one. The mechanical criterion
is tight on the two failure modes that matter: exactly one record (not zero, not one per tick), and
no dialog attempt where there is no display.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — extend the existing watchdog thread to write a JSON record before
                 escalating, with a display-capability check gating the dialog
   -> if still failing, MANDATORY approach change. Moving the write earlier in the same function
      is NOT an approach change; moving the record to a separate always-running reporter that the
      watchdog only signals IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the headless run still writes zero records
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a phase with no reachable end() on some exit path (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.39/stuck_phase_record.json`

A reader finds: the record schema with every field typed; the headless and windowed runs side by
side with their `dialog_attempted` values; the display-capability mechanism used and why; the
one-record-per-stuck-phase proof (file count for a 10-second stall); the phase names currently
bracketed by `begin`/`end` with their `load_phase.rs` line numbers; and the commit.

## 9. Definition of NOT done

- The record is written when the phase **finishes**, so a process that hangs forever — the only case
  worth reporting — produces nothing.
- The record is written from the main thread, so it never executes while the main thread is blocked.
- Headless writes a record but still calls into `rfd`, which blocks or aborts on a runner with no
  display, turning a diagnostic into a hang.
- A ten-second stall produces sixty records because the `reported` latch was not honoured.
- The watchdog was silenced in headless mode instead of being given a headless reporting path,
  removing the signal from the one surface CI can run.
- The record omits the detail string, so it says a phase is stuck without saying on what.

---

---
id: G7.40
title: Machine-readable microprofiler output and a perf-assert gate that can fail
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.31, G1.08]
blocks: [G7.43, G7.45]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.40/perf_assert_baseline.json
escalation: >
  If arming the profiler changes the measured frame time of the profiled run by more than 3%, STALL —
  a profiler that perturbs what it measures cannot be the metric everything else is judged against.
status: DRAFT
notes: >
  Tier M. The profiler already works; this item adds a machine-readable sibling output and a threshold
  assertion binary. Keep the profiling cargo feature switched off — it forces a full Bevy rebuild.
---

## 1. Objective

The always-compiled phase profiler emits a machine-readable JSON alongside its human outputs, and a
`perf-assert` binary reads that JSON and exits non-zero when a named phase exceeds a declared
threshold. A performance regression can fail a gate instead of being noticed by someone squinting at
a flamegraph.

## 2. Context you need (self-contained)

**Project invariants.** AI-native simulation substrate, never a game engine. PolyForm Shield 1.0.0,
source-available. Avian, never Rapier. Slint is Rust. Meter-native units. 10–15 minute builds, one
at a time, shared `eustress/target/`, never killed mid-compile. `cargo run`, not `cargo check`.
Cargo runs from `eustress/`.

**What already exists, verbatim from `eustress/crates/engine/src/profiler.rs` (702 lines).** There
are two profilers, and only the first is relevant here:

- **Phase profiler — always compiled, the default.** It needs no `tracing` spans, so it is in every
  build (debug, release, `run-studio`) and adds "only a handful of cheap systems per frame". It is
  dormant until the `EUSTRESS_PROFILE` environment variable is set to any non-empty value; the arming
  check is a single `OnceLock` read. It attributes the frame to its **six top-level phases**, and the
  dominant phase is the bottleneck's location. `EUSTRESS_PROFILE_FRAMES` sets the window length in
  frames (**default 120**, CONFIG DEFAULT). Outputs land in the working directory:
  `eustress_profile.txt` (a ranked table), `eustress_profile.svg` (an `inferno` flamegraph), and
  `eustress_profile_phases.txt` (ranked phases).
- **Per-system trace layer — cargo feature `profiling`, opt-in deep dive.** This is the *only* path
  that enables `bevy_ecs/trace`, which recompiles the whole Bevy stack. The module's own advice is to
  use it deliberately and ideally with its own `--target-dir`. **Do not enable it in this item** — it
  will consume your entire build budget.

`eustress/crates/engine/src/frame_diagnostics.rs` (151 lines) is the companion frame-time source.

**The rule this item enforces.** In this project the microprofiler is the only trustworthy
performance metric. Frame counters observed by eye, numbers read off a UI overlay, and wall-clock
timings taken around a command that also writes to a terminal are all known to mislead — piping
engine output through a terminal-capturing shell pipeline measurably stalls the engine, so a timing
taken that way measures the pipeline. Any performance claim in this program must trace to a profiler
artifact.

**The gap.** All three outputs are for humans. There is no JSON, so no test, no CI job, and no
regression fleet can assert on a phase time. A performance regression is therefore only detectable by
a person who happens to re-run the profiler and compare two text files by eye.

**Existing benchmark binaries you may cite but must not modify.** `eustress/benches/instance-capacity/`
declares two binaries in its `Cargo.toml`: `instance-capacity` and `avian-physics-bench`. They are
separate measurement rigs with their own reporting; this item does not replace them.

**Prerequisite already satisfied.** G7.31 defined the support bundle and the structured JSONL log
format. The profiler JSON should reuse that item's conventions for hardware and build identity rather
than inventing a second schema — read `docs/PROMPTS/artifacts/G7.31/diag_bundle_contract.json`.

## 3. Scope

### In scope — files this item may edit
- A new binary under `eustress/crates/engine/src/bin/` for `perf-assert`
- A threshold file, checked in, that `perf-assert` reads

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/benches/instance-capacity/` — separate rigs, separate items
- The `profiling` cargo feature and anything it gates
- `eustress/crates/engine/src/frame_diagnostics.rs` — owned by `G1.08` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- `eustress/crates/engine/src/profiler.rs` — owned by `G1.08` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- The layout of `eustress/crates/engine/src/bin/` — the directory is owned by `G2.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Raising a threshold to make the assert pass, shortening the frame window, excluding the slowest
  phase, or taking the minimum instead of the declared statistic are all measurement changes.
- The JSON is **additive**. `eustress_profile.txt`, `eustress_profile.svg`, and
  `eustress_profile_phases.txt` must keep their current content and location.
- Report a distribution, not a mean. At minimum: `p50`, `p95`, `p99`, and `max` per phase, plus the
  frame count the window actually captured. A regression that only shows in the tail is the common
  case and a mean hides it.
- The profiler must stay dormant when `EUSTRESS_PROFILE` is unset. Verify the dormant-path cost did
  not change; the front-matter escalation is a hard 3% ceiling on observer effect.
- Thresholds live in a checked-in file with a comment per entry saying what hardware the number came
  from. A threshold with no provenance is a wish.
- Every number recorded is labelled `MEASURED` with the command and hardware, `TARGET`, or
  `CONFIG DEFAULT`.

## 5. Exit criterion

### Criterion
A profiled run emits a JSON with all **6** top-level phases, each carrying `p50`/`p95`/`p99`/`max`
and a captured frame count of at least the configured window; `perf-assert` exits **0** against a
threshold file the run satisfies and exits **non-zero** against a threshold file deliberately set
0.5 ms below the measured `p95` of the dominant phase; and the profiler's observer effect on total
frame time is at most **3%**.

### Measurement

Command (bash, from the repo root):

    cd eustress && cargo build --release --package eustress-engine && cd ..
    rm -f eustress_profile.json
    EUSTRESS_PROFILE=1 EUSTRESS_PROFILE_FRAMES=120 \
      ./eustress/target/release/eustress-headless --space /tmp/tc_healthy --ticks 600 ; echo "RUN_EXIT=$?"
    ./eustress/target/release/perf-assert \
      --profile eustress_profile.json --thresholds docs/PROMPTS/harness/perf_thresholds.toml ; echo "PASS_EXIT=$?"
    ./eustress/target/release/perf-assert \
      --profile eustress_profile.json --thresholds /tmp/thresholds_too_tight.toml ; echo "FAIL_EXIT=$?"
    python - <<'PY' ; echo "EXIT=$?"
    import json, sys
    p = json.load(open("eustress_profile.json"))
    d = json.load(open("docs/PROMPTS/artifacts/G7.40/perf_assert_baseline.json"))
    phases = p["phases"]
    have = all(all(k in v for k in ("p50","p95","p99","max")) for v in phases.values())
    print("phases", len(phases), "stats_complete", have, "frames", p["frames_captured"],
          "observer_effect_pct", d["observer_effect_pct"],
          "pass_exit", d["perf_assert_pass_exit"], "fail_exit", d["perf_assert_fail_exit"])
    sys.exit(0 if len(phases) == 6 and have and p["frames_captured"] >= 120
             and d["observer_effect_pct"] <= 3.0
             and d["perf_assert_pass_exit"] == 0 and d["perf_assert_fail_exit"] != 0 else 1)
    PY

`/tmp/thresholds_too_tight.toml` is generated by this item from the measured run; record how.

Expected output shape:

    RUN_EXIT=0
    PASS_EXIT=0
    FAIL_EXIT=2
    phases 6 stats_complete True frames 600 observer_effect_pct 1.4 pass_exit 0 fail_exit 2
    EXIT=0

Pass condition:

    `EXIT=0`: six phases with complete statistics, at least the configured frame window captured,
    observer effect at most 3%, `perf-assert` exiting 0 on a satisfiable threshold file and non-zero
    on a deliberately tight one. An assert that never fails is not an assert.

## 6. Critic gate

`critic_gate: []`. Performance measurement is numeric by construction. The mechanical criterion is
tight where perf gates are usually hollow: the assert must be *demonstrated to fail* against a
threshold the run cannot meet, and the profiler's own perturbation is bounded and recorded.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — emit a JSON sibling from the existing phase-dump path + a reader binary
   -> if still failing, MANDATORY approach change. Adding a statistic is NOT an approach change;
      moving from an end-of-window dump to a streaming per-frame record IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the deliberately-tight threshold still passes
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: observer effect above 3% (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.40/perf_assert_baseline.json`

A reader finds: the six phases with `p50`/`p95`/`p99`/`max`, all labelled `MEASURED` with the exact
command, the hardware (CPU, GPU, driver, RAM, OS), and the Space used; `frames_captured`;
`observer_effect_pct` with the paired armed/unarmed runs it was computed from; the threshold file
path and every entry's provenance; `perf_assert_pass_exit` and `perf_assert_fail_exit`; and the
commit.

## 9. Definition of NOT done

- The JSON reports a mean per phase, so a tail regression that doubles `p99` while leaving the mean
  flat passes silently.
- `perf-assert` has never been observed to fail, so nobody knows whether it can.
- The thresholds were derived from the same single run they are then asserted against, making the
  gate a tautology — state in the artifact how the threshold was chosen and from how many runs.
- Adding the JSON changed the human outputs' format, breaking the founder's existing reading habits.
- The `profiling` cargo feature was enabled to get richer data, consuming the build budget and
  producing numbers that do not correspond to a shipped build.
- A number appears in the artifact without a `MEASURED` / `TARGET` / `CONFIG DEFAULT` label.

---

---
id: G7.41
title: Startup and shutdown correctness contract with measured cold-start and clean-exit proof
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.32, G7.03]
blocks: [G7.43, G7.45]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.41/lifecycle_contract.json
escalation: >
  If a clean shutdown cannot release the world-container lock within 5 seconds on any surface, STALL —
  a container still locked after exit means the next launch fails and the user's only recourse is a
  reboot.
status: DRAFT
notes: >
  Tier M. Cold start is measured, not improved, in this item; the deliverable is the contract and the
  proof that exit is clean, which is what makes a later start-time item measurable.
---

## 1. Objective

The engine's startup and shutdown behaviour is a written contract with measured numbers behind it:
cold start to interactive is measured on a named machine and Space; a normal quit releases the
world-container lock, flushes telemetry, and exits `0`; every abnormal termination path exits with the
documented non-zero code from G7.32; and no orphaned process, thread, or lock file survives a clean
exit.

## 2. Context you need (self-contained)

**Project invariants.** AI-native simulation substrate, never a game engine. PolyForm Shield 1.0.0,
source-available. Avian, never Rapier. Slint is Rust. Meter-native units. 10–15 minute builds, one
at a time, shared `eustress/target/`, never killed mid-compile. `cargo run`, not `cargo check`.
Cargo runs from `eustress/`.

**Why shutdown is load-bearing here.** Three separate mechanisms depend on a clean exit path:

1. `eustress/crates/engine/src/usage_telemetry.rs` — `flush_on_exit` reacts to Bevy's `AppExit` and
   is the only thing that calls `write_outbox()`. An exit path that bypasses `AppExit` loses the
   session's telemetry, which is exactly what G7.30's session ledger was built to stop.
2. The world container. `world.fjalldb/` is a Fjall log-structured-merge-tree store opened by the
   process. A process that exits without releasing it leaves a lock that the next launch must
   contend with.
3. The engine bridge. `eustress/crates/engine/src/bin/headless.rs` advertises the bridge on
   `<universe>/.eustress/engine.port`; a stale port file after exit points the MCP server and the
   `eustress` CLI at a process that is gone.

**What exists today on the exit path.** `eustress/crates/engine/src/main.rs:607` wraps `app.run()`
in `catch_unwind` and, on a panic it classifies as a lost GPU surface, calls `std::process::exit(0)`
— which bypasses `AppExit` entirely and therefore bypasses `flush_on_exit`. G7.32 replaced that
classifier and established a documented exit-code table; read
`docs/PROMPTS/artifacts/G7.32/panic_classification.json` for the codes and do not invent new ones.

**What exists today on the startup path.** `eustress/crates/engine/src/space/load_phase.rs` brackets
load phases with `begin`/`end` and reports any phase running past its threshold (default 30 seconds,
CONFIG DEFAULT, `EUSTRESS_PHASE_WATCHDOG_SECS`). The phase names it already brackets — including
`space-open` enclosing `db-reconcile` — are the natural segments of a cold-start measurement. Reuse
them; do not add a parallel timing scheme.

Relevant startup knobs that change what "cold start" means and must be recorded with any number:
`EUSTRESS_LOAD_SPAWN_BUDGET`, `EUSTRESS_RESIDENCY_*`, `EUSTRESS_HLOD_RADIUS`,
`EUSTRESS_SHADOW_DISTANCE`, `EUSTRESS_SPLAT_BUDGET`.

**No cold-start number exists in this repository today.** This item produces the first one. It is a
baseline, not a target — do not attempt to improve it here.

**Prerequisite already satisfied.** G7.32 delivered the exit-code table and a durable crash record.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/main.rs` — shutdown ordering and exit-path correctness
- `eustress/crates/engine/src/space/` — lock release on shutdown only
- A new binary under `eustress/crates/engine/src/bin/` for the lifecycle drill

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/usage_telemetry.rs` — consume its behaviour, do not change it
- Load performance. Measuring cold start is in scope; making it faster is a different item.
- `eustress/crates/engine/src/bin/headless.rs` — owned by `G7.03` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- The layout of `eustress/crates/engine/src/bin/` — the directory is owned by `G2.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Measuring cold start on an
  empty Space, warming the file cache first without saying so, excluding the reconcile phase, or
  declaring "interactive" at a moment before the window responds are all measurement changes.
- Define "interactive" once, precisely, in the artifact, in terms of an observable event — not a
  feeling. Whatever definition you choose, the same definition must be used for every recorded run.
- Report cold start as a distribution over at least 5 runs (`p50` and `max`), with the file cache
  state stated for each.
- Clean exit means all four: exit code `0`, telemetry outbox written, container lock released, port
  file removed. Assert all four, not the exit code alone.
- Every abnormal path must be exercised, not reasoned about: at minimum a window-close, a `SIGTERM`
  or platform equivalent, and a hard kill.

## 5. Exit criterion

### Criterion
Over **5** cold-start runs on a named Space, `p50` and `max` time-to-interactive are recorded with
the cache state; and a lifecycle drill demonstrates that a clean exit satisfies all **4** clean-exit
conditions with the container lock released within **5.0 s**, while a hard kill leaves a crash
record and a non-zero exit code from the G7.32 table.

### Measurement

Command (bash, from the repo root):

    cd eustress && cargo build --release --package eustress-engine && cd ..
    ./eustress/target/release/lifecycle-drill \
        --space /tmp/tc_healthy \
        --cold-runs 5 \
        --out docs/PROMPTS/artifacts/G7.41/lifecycle_contract.json ; echo "DRILL_EXIT=$?"
    python - <<'PY' ; echo "EXIT=$?"
    import json, sys
    d = json.load(open("docs/PROMPTS/artifacts/G7.41/lifecycle_contract.json"))
    cold = d["cold_start"]
    clean = d["clean_exit"]
    kill = d["hard_kill"]
    print("runs", cold["runs"], "p50", cold["p50_s"], "max", cold["max_s"],
          "lock_release_s", clean["lock_release_s"], "conditions",
          [clean[k] for k in ("exit_code_zero","outbox_written","lock_released","port_file_removed")],
          "kill_code", kill["exit_code"], "kill_record", kill["crash_record_written"])
    sys.exit(0 if cold["runs"] >= 5
             and all(clean[k] for k in ("exit_code_zero","outbox_written","lock_released","port_file_removed"))
             and clean["lock_release_s"] <= 5.0
             and kill["exit_code"] != 0 and kill["crash_record_written"] else 1)
    PY

Expected output shape:

    DRILL_EXIT=0
    runs 5 p50 8.4 max 11.2 lock_release_s 0.31 conditions [True, True, True, True] kill_code 5 kill_record True
    EXIT=0

Pass condition:

    `EXIT=0`: at least 5 cold runs recorded, all four clean-exit conditions true, lock released
    within 5.0 s, and the hard kill producing both a non-zero exit code and a crash record.

The cold-start `p50` and `max` are **baseline measurements**, not thresholds — the item does not fail
for being slow. It fails for not knowing.

## 6. Critic gate

`critic_gate: []`. Lifecycle correctness is a checklist of observable facts. The mechanical criterion
replaces the Critic and is strict where lifecycle work is usually hand-waved: all four clean-exit
conditions must hold simultaneously, and the abnormal path must be exercised rather than described.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — drill binary spawning the engine as a child and asserting on artifacts
   -> if still failing, MANDATORY approach change. Adding a sleep is NOT an approach change;
      moving lock release from Drop-based cleanup to an explicit shutdown phase IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same clean-exit condition still false
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: container lock not released within 5 s on any surface (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.41/lifecycle_contract.json`

A reader finds: the definition of "interactive" and the observable event that marks it;
`cold_start` with `runs`, `p50_s`, `max_s`, per-run cache state, the Space used and its entity count,
the hardware, and every startup env knob's value; `clean_exit` with the four boolean conditions and
`lock_release_s`; `hard_kill` with its exit code and crash-record path; the per-phase startup
breakdown taken from the `load_phase` names; and the commit. Every number carries a `MEASURED` /
`TARGET` / `CONFIG DEFAULT` label.

## 9. Definition of NOT done

- Cold start is measured once, so the number has no distribution and the next run will disagree
  with it.
- "Interactive" is defined as "the window appears", which happens long before the Space is loaded
  and makes the number meaningless.
- The clean exit asserts only the exit code, so a run that exits `0` while leaving the container
  locked is recorded as clean — and the next launch fails.
- The hard-kill case is described in prose rather than executed, so the crash-record path is
  untested on the surface that matters.
- The drill passes on the windowed engine only, leaving `eustress-headless` — the surface CI runs —
  with an unproven shutdown path and a stale `engine.port` file.
- Cold start was "improved" during this item, so the baseline records a tuned configuration nobody
  ships.

---

---
id: G7.42
title: Fault injection and typed, actionable recovery for every user-facing failure
workload: W6
workload_secondary: [W3]
phase: G7
depends_on: [G7.32, G7.39, G6.13, G1.12]
blocks: [G7.45]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D4, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G7_error_surface.json
artifact: docs/PROMPTS/artifacts/G7.42/fault_matrix.json
escalation: >
  If any injected fault produces no user-visible signal at all — the action simply does nothing —
  STALL and name it, because a silent no-op is the worst outcome in this matrix and cannot be traded
  against progress elsewhere.
status: DRAFT
notes: >
  Tier L: the fault-injection harness plus a Critic-gated capture loop. This is the only robustness
  item in the pack with a perceptual gate, because "the error told me what to do" is a craft
  judgement as much as a mechanical one.
---

## 1. Objective

Every fault in a declared matrix of realistic failures produces a typed, user-visible, actionable
outcome: the user is told what failed, in their own terms, and what to do next. No fault produces a
silent no-op, and no fault produces an unrecoverable state that requires restarting the application.

## 2. Context you need (self-contained)

**Project invariants.** Eustress is an AI-native simulation substrate, never a game engine. Licence
PolyForm Shield 1.0.0 — source-available. Physics is Avian, never Rapier. **Slint is Rust** — the
`.slint` files under `eustress/crates/engine/ui/slint/` compile to Rust, so an error surface built in
Slint is Rust code, not a separate technology. Units are meter-native. Builds take 10–15 minutes, one
at a time, shared `eustress/target/`, never killed mid-compile. `cargo run`, not `cargo check`. Cargo
runs from `eustress/`.

**The failure class this item exists to kill.** The studio's UI action path is a `SlintAction` queue
drained by a single system (`SlintSystems::Drain`) in
`eustress/crates/engine/src/ui/slint_ui.rs`, which is **23,103 lines** (MEASURED via `wc -l`,
2026-08-06) — the largest file in the repository. The known failure mode in that path is that one
missing required parameter causes the drain to skip, and **every** subsequent UI click silently does
nothing. The user sees an application that has stopped responding to input with no error anywhere.
This is the archetype: not a crash, not a message, just silence. A fault matrix that does not include
a drain-skip case has missed the point.

**The two mechanisms you build on, both already delivered.**
- G7.32 gave you a typed panic classifier, a durable crash record, and an exit-code table. Read
  `docs/PROMPTS/artifacts/G7.32/panic_classification.json`.
- G7.39 gave you a machine-readable stuck-phase record that is written while the phase is still
  stuck, on every surface including headless. Read
  `docs/PROMPTS/artifacts/G7.39/stuck_phase_record.json`.

Neither of those covers a *recoverable* fault, which is what this item is about. A stuck phase is
reported; a rejected file is not.

**Fault classes the matrix must cover, at minimum.** Each is a realistic failure a user or an agent
will hit:
1. A required action parameter is missing (the drain-skip archetype).
2. A Space directory is opened that is not a valid world container.
3. A world container is opened whose authoritative store is absent — the `seed_only` verdict from
   G7.38's verifier.
4. A disk write fails (target full or read-only).
5. A script fails to compile — both Rune and Luau are supported scripting surfaces.
6. An MCP or engine-bridge call arrives with a malformed parameter object.
7. An asset import is given a file whose format is unsupported.
8. The engine bridge port file exists but no process is listening.

**What "actionable" means for the gate.** The user-visible message must name the thing that failed
and state a next step the user can actually take. "An error occurred" is not actionable. "Failed to
open Space: no world database found at `<path>/world.fjalldb`. This Space may have been copied
without its database — run `eustress space verify` for details." is.

**Recovery, not just reporting.** After each injected fault, the application must remain usable: the
next unrelated action must succeed. A fault that reports beautifully and then leaves the UI dead
fails this item.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/ui/slint/notifications.slint` and the error-surface `.slint` files
- `eustress/crates/engine/src/ui/notifications.rs` and sibling error-surface modules
- Error types and their conversions in the crates the matrix touches
- A new fault-injection harness binary under `eustress/crates/engine/src/bin/`
- `docs/PROMPTS/harness/recipes/G7_error_surface.json` (create if absent, following the recipe schema in `docs/PROMPTS/02_CAPTURE_HARNESS.md` §8) — this item's own `capture_recipe`, and this is the only item permitted to author it

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- A general refactor of `slint_ui.rs`. Splitting that file is a separate, larger item; touching its
  error path is in scope, restructuring it is not.
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/ui/slint_ui.rs` — owned by `G6.13` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- The layout of `eustress/crates/engine/src/bin/` — the directory is owned by `G2.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Removing a fault class from the matrix, replacing a real injection with a mock that takes a
  different code path, or scoring a generic message as actionable are all measurement changes. If a
  fault genuinely cannot be injected, report `EXIT_CRITERION_UNMEASURABLE` with evidence.
- Inject faults through the **real** call path. A test that constructs the error value directly and
  hands it to the notification system proves the notification renders, not that the fault reaches it.
- Every matrix entry must record all four outcomes: `user_visible` (bool), `message` (the exact
  string), `actionable` (bool with the named next step), and `recovered` (bool — did the next
  unrelated action succeed).
- Do not add a modal dialog for a recoverable fault. Modals are for the cases G7.39 already handles.
- Batch your builds. Twelve is the whole budget for three approaches; one build should validate
  several matrix entries.

## 5. Exit criterion

### Criterion
For all **8** fault classes: `user_visible` is true, `recovered` is true, and the recorded message
names both the failing thing and a next step; **0** entries have `user_visible: false`; and the
blinded error-surface capture scores at least **8.0** on both gated Critic dimensions.

### Measurement

Command (bash, from the repo root):

    cd eustress && cargo build --release --package eustress-engine && cd ..
    ./eustress/target/release/fault-drill \
        --matrix all \
        --out docs/PROMPTS/artifacts/G7.42/fault_matrix.json ; echo "DRILL_EXIT=$?"
    python - <<'PY' ; echo "EXIT=$?"
    import json, sys
    d = json.load(open("docs/PROMPTS/artifacts/G7.42/fault_matrix.json"))
    e = d["entries"]
    silent = [x["id"] for x in e if not x["user_visible"]]
    unrecovered = [x["id"] for x in e if not x["recovered"]]
    vague = [x["id"] for x in e if not x["actionable"] or not x.get("next_step")]
    print("entries", len(e), "silent", silent, "unrecovered", unrecovered, "vague", vague)
    sys.exit(0 if len(e) >= 8 and not silent and not unrecovered and not vague else 1)
    PY

Then produce the blinded capture bundle for the Critic:

    cargo run --release --bin eustress-capture -- \
        --recipe docs/PROMPTS/harness/recipes/G7_error_surface.json \
        --subject HEAD \
        --control 71ccf6fe \
        --out docs/PROMPTS/artifacts/bundles \
        --trials 5 \
        --verify-determinism ; echo "CAP_EXIT=$?"

If `docs/PROMPTS/harness/recipes/G7_error_surface.json` does not yet exist, this item authors it,
following the recipe format the capture harness defines, and captures the `error_surface` interaction
sequence (trigger a known-invalid action, error appears, error dismissed) at 1920x1080, 2560x1440,
and 3840x2160.

Expected output shape:

    DRILL_EXIT=0
    entries 8 silent [] unrecovered [] vague []
    EXIT=0
    CAP_EXIT=0

Pass condition:

    `EXIT=0` with at least 8 entries and empty silent/unrecovered/vague lists, `CAP_EXIT=0`, and
    both gated Critic dimensions at or above 8.0.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)** and **D6 (overall coherence)**, floor **8.0 each**. The mean is
irrelevant — either dimension below 8.0 fails the item.

D4 is here because an error surface is a designed surface: alignment, timing, dismissal behaviour,
and whether the message reads as written by someone who understood the user's situation. D6 is here
because eight error surfaces that each look reasonable but disagree with each other on placement,
tone, and dismissal read as eight features rather than one system.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_error_surface.json`.

Note that the Critic never sees anything you write about your own work, and every score it gives must
cite a specific frame or measured value. Design the change so the improvement is legible in a single
still frame — an error whose helpfulness only emerges after reading a log will not be credited.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — typed error enum per subsystem, converted at the drain boundary,
                 rendered through the existing notification surface
   -> if still failing, MANDATORY approach change. Rewording a message is NOT an approach change;
      moving from post-hoc conversion at the drain to errors carrying their own remediation from
      the point of failure IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND the
                  silent-entry count unchanged
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: any injected fault produces no user-visible signal at all (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.42/fault_matrix.json`

A reader finds: `entries` — one per fault class, each with `id`, `injection_point` as `path:line`,
`user_visible`, the exact `message`, `actionable`, `next_step`, `recovered`, and the surface it was
observed on; the recipe path and bundle hash for the blinded capture; the Critic scorecard reference;
and the commit.

## 9. Definition of NOT done

- The drain-skip archetype is absent from the matrix, so the one failure this item was named for is
  untested.
- A fault is injected by constructing the error value directly rather than through the real call
  path, so the test proves the renderer works and not that the fault reaches it.
- Messages are visible but generic — "Operation failed" for six of eight entries — and `actionable`
  was marked true anyway.
- The error appears and the application is thereafter unusable, so `recovered` should be false and
  was not checked.
- Eight surfaces were built independently: three toasts, two modals, two console lines, and one
  status-bar flash. D6 fails on coherence even if each is individually fine.
- The Critic passes D4 but refuses the wow gate, citing that errors are now polite while the
  underlying action still gives no progress feedback. That is a legitimate refusal; the item is not
  done.

---

---
id: G7.43
title: Always-on regression fleet that re-runs every earlier phase gate on a schedule
workload: W3
workload_secondary: [W6]
phase: G7
depends_on: [G7.34, G7.35, G7.40, G7.36, G7.37, G7.39, G7.41]
blocks: [G7.45]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.43/fleet_manifest.json
escalation: >
  If any earlier phase's exit gate cannot be re-run without a desktop session or a GPU, STALL and
  name that gate — the fleet's whole purpose is that no phase can be silently broken by a later one,
  and a gate the fleet cannot run is a phase that can be silently broken.
status: DRAFT
notes: >
  Tier L. The fleet is the structural answer to an ordered gauntlet: phase 7 must be able to re-run
  phase 1's gates. Without it, every passed phase decays the moment the next one starts.
---

## 1. Objective

A scheduled, always-on fleet re-runs every gauntlet phase's exit gate that can be executed without a
human, records each gate's pass or fail with the measured value it produced, and fails loudly when a
previously-passing gate goes red. A later phase can no longer silently break an earlier one.

## 2. Context you need (self-contained)

**Project invariants.** Eustress is an AI-native simulation substrate, never a game engine. Licence
PolyForm Shield 1.0.0 — source-available. Physics is Avian, never Rapier. Slint is Rust. Units are
meter-native. Builds take 10–15 minutes, one at a time, shared `eustress/target/`; never kill a build
mid-compile. Validate with `cargo run`, not `cargo check`. Cargo runs from `eustress/`.

**Why this item exists.** The gauntlet is **ordered**: `docs/PROMPTS/00_MASTER_PROTOCOL.md` §3.2
states that a phase may not open until its predecessors' hard dependencies are `PASSED`. An ordered
programme with no re-run mechanism has a structural defect: a phase that passed in week two can be
broken in week six by an unrelated change, and nothing notices, because the gate that would have
caught it ran once and was never run again. The seven phase exit conditions, verbatim from that
document, are:

| Phase | Exit condition |
|---|---|
| G1 Capture & Measurement Harness | the harness runs from a single command, on a machine that is not the founder's, and produces a content-addressed bundle with a valid provenance manifest |
| G2 Determinism & Numerical Trust | two runs byte-identical; the time-compression step-drop instrumented and surfaced; the Watchman cooldown uses sim time |
| G3 Render Fidelity | Critic at or above 8.0 on materials/lighting from the S1–S3 scene set |
| G4 Motion & Temporal Stability | Critic at or above 8.0 on motion; measured frame-time variance floor met on the fixed camera path |
| G5 Scale & Streaming | a measured entity count at a measured frame rate on the fixed harness |
| G6 Studio UI Craft | Critic at or above 8.0 on UI craftsmanship; the drain-skip failure class has a regression test that runs in CI |
| G7 Agent Loop Closure | the full observe→act→judge loop executes headless in CI with no desktop session |

Some of those are Critic-gated and cannot run unattended — a blinded human-or-Critic scoring pass is
not a cron job. The fleet's honest scope is therefore: **every gate whose measurement is mechanical**,
plus a recorded, explicit list of the gates that are not mechanical and therefore require a scheduled
human or Critic invocation. Both halves belong in the manifest; a fleet that quietly omits the
Critic-gated phases is claiming coverage it does not have.

**What already exists to build on, all delivered earlier in this pack.**
- G7.34 put `cargo test --workspace` in `ci.yml` and green, with a recorded executed-test floor.
  Read `docs/PROMPTS/artifacts/G7.34/test_gate.json`.
- G7.35 put the client binary build and an exit-code-asserting smoke test in CI across three
  platforms. Read `docs/PROMPTS/artifacts/G7.35/client_build_gate.json`.
- G7.40 gave the profiler a machine-readable JSON and a `perf-assert` binary that has been
  *demonstrated to fail* against a threshold the run cannot meet. Read
  `docs/PROMPTS/artifacts/G7.40/perf_assert_baseline.json`.

**Hard constraints on where the fleet can run.** GitHub-hosted runners have no GPU.
`eustress/crates/engine/src/bin/headless.rs` is `MinimalPlugins` plus `ScheduleRunnerPlugin` — no
winit, no GPU, no Slint — and its own module docs state that `ai_camera` and `viewport.capture` need
a future `--render gpu` tier that does not exist. Any gate requiring a rendered frame therefore
cannot run on a hosted runner today. Record that as a constraint per gate; do not pretend otherwise
and do not solve it by adding a self-hosted runner in this item.

**Cost discipline.** A workspace build is 10–15 minutes. A fleet that rebuilds everything on every
push will exhaust the runner budget and be switched off, which is worse than not having it. Schedule
it; do not attach it to every push.

## 3. Scope

### In scope — files this item may edit
- `.github/workflows/` — **a new workflow file for the fleet only**, and only to add gates
- A fleet runner binary or script under `eustress/crates/cli/src/` or `scripts/`
- `docs/PROMPTS/harness/` — the fleet's gate registry file

### Out of scope — do not edit
- `.github/workflows/ci.yml`, `release.yml`, `linux-engine.yml` — the fleet is additive; never
  modify an existing workflow to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- Any earlier item's artifact under `docs/PROMPTS/artifacts/` — the fleet reads them, it does not
  rewrite them
- The gates themselves. If a gate is wrong, that is an escalation, not a licence to loosen it.

## 4. Approach constraints

- **This item deliberately overrides the standing out-of-scope entry on `.github/workflows/`**
  (`docs/PROMPTS/03_PROMPT_SCHEMA.md` §4.3). The fleet is a workflow, so it may add one — but in one
  direction only: it may **ADD** a gate. Weakening or removing any existing gate, in the new file or
  in `ci.yml`, `release.yml`, or `linux-engine.yml`, fails the item outright, whatever the resulting
  CI status.
- **Changing the measurement instead of the artifact fails this item.** Dropping a gate from the
  registry because it is slow or flaky, marking a gate `advisory` so a red result does not fail the
  run, or reducing a gate's threshold to keep the fleet green are all measurement changes.
- Every registry entry declares: the gate id, the phase it belongs to, the literal command, the pass
  condition, whether it is `mechanical` or `requires_human_or_critic`, and its runner constraint
  (`hosted_ok`, `needs_gpu`, `needs_desktop`).
- A red gate must fail the workflow run. A fleet that reports red in a log nobody reads is a
  dashboard, not a gate.
- The fleet must record the **measured value**, not just pass/fail, for every mechanical gate — a
  gate that is drifting toward its threshold is the signal worth having before it goes red.
- Schedule, do not attach to push. State the cadence and the estimated runner-minutes per run.
- Every `requires_human_or_critic` gate must carry a named cadence and a named owner role. "TBD" is
  a placeholder and fails the item.

## 5. Exit criterion

### Criterion
The fleet registry covers **all 7** gauntlet phases with at least one gate each; at least **5** gates
are `mechanical` and execute in a single scheduled run; the run records a measured value for every
mechanical gate; and a deliberately broken gate causes the workflow run to **fail** rather than to
report and continue.

### Measurement

Command (bash, from the repo root):

    ./scripts/fleet-run.sh --registry docs/PROMPTS/harness/fleet_gates.toml \
        --out docs/PROMPTS/artifacts/G7.43/fleet_manifest.json ; echo "FLEET_EXIT=$?"
    python - <<'PY' ; echo "EXIT=$?"
    import json, sys
    d = json.load(open("docs/PROMPTS/artifacts/G7.43/fleet_manifest.json"))
    gates = d["gates"]
    phases = {g["phase"] for g in gates}
    mech = [g for g in gates if g["kind"] == "mechanical"]
    novalue = [g["id"] for g in mech if g.get("measured_value") in (None, "")]
    human = [g for g in gates if g["kind"] == "requires_human_or_critic"]
    nocadence = [g["id"] for g in human if not g.get("cadence") or not g.get("owner_role")]
    print("phases", sorted(phases), "mechanical", len(mech),
          "missing_values", novalue, "human_gates_without_cadence", nocadence,
          "red_gate_fails_run", d["red_gate_fails_run_verified"])
    sys.exit(0 if len(phases) == 7 and len(mech) >= 5 and not novalue
             and not nocadence and d["red_gate_fails_run_verified"] is True else 1)
    PY

`red_gate_fails_run_verified` must be set by actually breaking one gate, observing the run fail, and
restoring it — record the run id and the restored commit in the manifest. Setting it true by hand
without that evidence fails the item.

Expected output shape:

    FLEET_EXIT=0
    phases ['G1', 'G2', 'G3', 'G4', 'G5', 'G6', 'G7'] mechanical 6 missing_values [] human_gates_without_cadence [] red_gate_fails_run True
    EXIT=0

Pass condition:

    `EXIT=0`: all seven phases represented, at least five mechanical gates with recorded measured
    values, every human/Critic gate carrying a cadence and an owner role, and the red-gate failure
    demonstrated rather than asserted.

## 6. Critic gate

`critic_gate: []`. A regression fleet is infrastructure; it is correct or it is not. The mechanical
criterion replaces the Critic and is strict where fleets are usually hollow: the red-gate path must
be *demonstrated*, and every gate that cannot be automated must be named with a cadence and an owner
rather than quietly dropped.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — a registry file + a runner script invoked by one scheduled workflow
   -> if still failing, MANDATORY approach change. Adding a gate is NOT an approach change;
      moving from one monolithic scheduled run to per-phase jobs with a rollup gate IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same phase still unrepresented in the registry
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a phase exit gate that cannot be re-run without a desktop session or a GPU
                   (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.43/fleet_manifest.json`

A reader finds: `gates` — one entry per gate with `id`, `phase`, `kind`, the literal `command`, the
`pass_condition`, `runner_constraint`, and for mechanical gates the `measured_value` from the latest
run; the schedule and estimated runner-minutes per run; `red_gate_fails_run_verified` with the run id
and the gate that was broken; the list of phases whose gates are not automatable with the reason and
the named cadence and owner role for each; and the commit.

## 9. Definition of NOT done

- The fleet runs and every gate is green because the only gates in the registry are the three that
  already ran in `ci.yml`. Seven phases must be represented, including the ones that are
  inconvenient.
- Gates report pass/fail with no measured value, so a gate drifting from 3 ms toward a 10 ms
  threshold looks identical to one sitting at 3 ms.
- A red gate is recorded in the manifest and the workflow still exits 0, so nothing is blocked.
- The Critic-gated phases are omitted entirely and the manifest claims seven-phase coverage.
- `red_gate_fails_run_verified` is `true` because someone wrote `true`, with no run id and no broken
  gate behind it.
- The fleet is attached to every push, consumes the runner budget in a week, and is disabled — a
  gate that exists only in git history.
- The item's CI licence was used in the forbidden direction: an existing gate was weakened,
  disabled, or removed. This item may only ADD; a diff that subtracts a gate fails it outright.

---

---
id: G7.44
title: On-prem and air-gapped reliability profile measured against the cloud-connected profile
workload: W3
workload_secondary: [W4]
phase: G7
depends_on: [G7.31, G7.38]
blocks: [G7.45]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G7.44/deployment_profile.json
escalation: >
  If any core authoring or simulation capability is found to hard-fail with no network — as opposed
  to degrading with a stated message — STALL and name it, because that converts every on-prem
  conversation into a product change rather than a deployment note.
status: DRAFT
notes: >
  Tier M. This is a measurement item, not a feature item. Its value is that an enterprise
  conversation about air-gapped deployment can be answered with a matrix instead of a guess.
---

## 1. Objective

A measured matrix states, capability by capability, what Eustress does with no network at all, what
it does with a restricted network, and what it does fully connected — and for every degraded
capability, whether the degradation is announced to the user or silent. An enterprise buyer asking
"does this work inside our network" gets a document, not an opinion.

## 2. Context you need (self-contained)

**Project invariants.** AI-native simulation substrate, never a game engine. PolyForm Shield 1.0.0 —
say **source-available**, never open source; this matters directly here, because a licence
conversation always follows an on-prem conversation. Physics is Avian, never Rapier. Slint is Rust.
Units are meter-native. Builds take 10–15 minutes, one at a time, shared `eustress/target/`; never
kill a build mid-compile. `cargo run`, not `cargo check`. Cargo runs from `eustress/`.

**Honest starting position on hosted infrastructure.** `docs/AUDIT/12_INFRASTRUCTURE.md` records the
operational backbone as incomplete in ways that bear directly on this item: Vault is referenced in
Nomad job specs but the cluster is **not deployed**; the `infrastructure/forge/consul/` directory is
**empty**; Prometheus, Grafana and alerting are at 0%; multi-region Terraform modules do not exist;
there are no backup or disaster-recovery runbooks; Windows authenticode signing is 0% and macOS
notarisation is 0%. This item does not fix any of that. It measures what the *desktop application*
does without a network, which is a different and much more tractable question — and it is the
question an on-prem buyer actually asks first.

**Network dependencies that exist in the shipped desktop application.** These are the surfaces to
test, each verified present in the codebase:
- **Usage telemetry.** `eustress/crates/engine/src/usage_telemetry.rs` posts session aggregates to
  `https://api.eustress.dev/api/telemetry/usage` on a background thread, overridable with
  `EUSTRESS_TELEMETRY_URL`, with an outbox retried by `startup_drain` at next launch. Its documented
  design already anticipates being offline. Verify that it degrades silently *and harmlessly* — an
  offline install must not accumulate an unbounded outbox.
- **Publish.** The `eustress` CLI (`eustress/crates/cli/src/main.rs`) exposes a `Publish` subcommand
  that pushes to Cloudflare R2 via Wrangler. This is inherently online; the question is whether it
  fails with a clear message.
- **Engine bridge and MCP.** The bridge is advertised on `<universe>/.eustress/engine.port` and is a
  local TCP JSON-RPC surface. It should work fully with no external network; confirm it does.
- **Asset and media fetch.** The importer pipeline fetches remote media with a negative cache.
  Confirm the negative cache prevents a stall rather than retrying forever.

**Prerequisites already satisfied.**
- G7.31 delivered `eustress diag` and the structured JSONL log stream. Use the bundle to capture each
  run's evidence rather than transcribing observations by hand. Read
  `docs/PROMPTS/artifacts/G7.31/diag_bundle_contract.json`.
- G7.38 delivered `eustress space verify` with a three-valued portability verdict. A world container
  is local, so authoring and simulation should be fully available offline — prove it rather than
  assuming it.

**The three profiles to measure.** `offline` (no network interface reachable), `restricted`
(loopback and a local network only; no egress to the public internet), and `connected`. Use a real
network condition, not a mocked HTTP client — a mock proves the code path, not the product.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/G7.44/deployment_profile.json` — the artifact
- `eustress/crates/engine/src/usage_telemetry.rs` — only to bound the outbox if the measurement shows
  it grows without limit offline
- `eustress/crates/cli/src/main.rs` — only to improve a network-failure message the measurement shows
  is unclear

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- Anything under `infrastructure/` — hosted infrastructure is not this item's subject
- New features. If a capability is genuinely unavailable offline, record it; do not build it here.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Simulating offline by
  pointing `EUSTRESS_TELEMETRY_URL` at a dead host, excluding a capability because it is "obviously
  online", or recording a degradation as announced when the message only appears in a log file are
  all measurement changes.
- Every capability row must be exercised in all three profiles. A row with two of three measured is
  incomplete.
- "Announced" means visible to a user in the application. A line in `engine.jsonl` is evidence for a
  support engineer, not an announcement to a user.
- Record the exact network condition used for each profile and how it was imposed, so a stranger can
  reproduce it.
- Do not report an unbounded-growth risk without measuring it: run the offline profile long enough to
  produce at least 5 sessions and record the outbox directory's file count and total bytes.

## 5. Exit criterion

### Criterion
The matrix covers **at least 12** capabilities across **3** profiles — 36 measured cells, **0**
unmeasured — with every degraded cell carrying an `announced` boolean and the exact user-visible
message where true; and the offline profile's telemetry outbox is shown to be bounded after at least
5 offline sessions.

### Measurement

Command (bash, from the repo root):

    python - <<'PY' ; echo "EXIT=$?"
    import json, sys
    d = json.load(open("docs/PROMPTS/artifacts/G7.44/deployment_profile.json"))
    caps = d["capabilities"]
    profiles = ("offline", "restricted", "connected")
    unmeasured = [(c["id"], p) for c in caps for p in profiles
                  if c.get(p, {}).get("status") in (None, "", "unmeasured")]
    degraded = [(c["id"], p) for c in caps for p in profiles
                if c.get(p, {}).get("status") == "degraded"]
    unannounced_missing = [(cid, p) for cid, p in degraded
                           if "announced" not in next(c for c in caps if c["id"] == cid)[p]]
    ob = d["offline_outbox"]
    print("caps", len(caps), "cells", len(caps) * 3, "unmeasured", unmeasured,
          "degraded", len(degraded), "missing_announced", unannounced_missing,
          "outbox_sessions", ob["sessions"], "outbox_files", ob["file_count"],
          "outbox_bytes", ob["total_bytes"], "bounded", ob["bounded"])
    sys.exit(0 if len(caps) >= 12 and not unmeasured and not unannounced_missing
             and ob["sessions"] >= 5 and ob["bounded"] is True else 1)
    PY

Each cell's evidence is a `eustress diag` bundle produced under that profile; the artifact records the
bundle path per cell.

Expected output shape:

    caps 14 cells 42 unmeasured [] degraded 9 missing_announced [] outbox_sessions 6 outbox_files 6 outbox_bytes 4812 bounded True
    EXIT=0

Pass condition:

    `EXIT=0`: at least 12 capabilities, zero unmeasured cells, every degraded cell carrying an
    `announced` field, at least 5 offline sessions measured, and the outbox demonstrated bounded.

## 6. Critic gate

`critic_gate: []`. A deployment matrix is a factual inventory. The mechanical criterion replaces the
Critic and is strict on the one thing such matrices always fudge: no cell may be left unmeasured, and
a degraded cell must state whether the user was told.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — run the full capability list under three real network conditions,
                 collecting a diag bundle per cell
   -> if still failing, MANDATORY approach change. Adding a capability row is NOT an approach
      change; moving from manual runs to a scripted profile harness that imposes the network
      condition and drives the capability IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same cells still unmeasured
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a core authoring or simulation capability hard-fails offline (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G7.44/deployment_profile.json`

A reader finds: `capabilities` — one row per capability, each with `id`, a one-line description, and
three profile cells carrying `status` (`full` / `degraded` / `unavailable`), `announced`, the exact
user-visible message where announced, and the diag-bundle path that evidences it; how each network
condition was imposed; `offline_outbox` with `sessions`, `file_count`, `total_bytes`, and `bounded`;
an explicit statement of what an air-gapped install can and cannot do; and the commit. This file is
the answer to the first question an on-prem buyer asks.

## 9. Definition of NOT done

- Offline was simulated by pointing the telemetry URL at an unreachable host, so every other network
  dependency was never actually offline.
- Nine of fourteen capabilities were measured and the rest marked "expected full" from reading the
  code.
- A degraded cell is recorded as announced because the failure appears in `engine.jsonl` — evidence
  for a support engineer, invisible to the user.
- The outbox was never measured over multiple offline sessions, so the one genuine unbounded-growth
  risk in the offline profile is unassessed.
- The matrix concludes "works offline" without stating which capabilities are gone, which is the only
  part of the answer a buyer needs.
- A missing offline capability was built during this item, so the matrix describes a product that has
  not shipped.

---

---
id: G7.45
title: The ten-minute cohesion journey — cold install to publish, recorded and judged as one artifact
workload: W4
workload_secondary: [W1, W3]
phase: G7
depends_on: [G1.13, G7.30, G7.31, G7.32, G7.34, G7.35, G7.37, G7.38, G7.39, G7.40, G7.41, G7.42, G7.43, G7.44]
blocks: []
tier: XL
token_envelope: 1200000
wallclock_envelope: 5d
max_builds: 20
critic_gate: [D1, D4, D5, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G7_cohesion_journey.json
artifact: docs/PROMPTS/artifacts/G7.45/journey_record.json
escalation: >
  If any journey step cannot be completed at all in the clean-environment substitute G1.13 defines —
  as opposed to completing with a stated caveat — STALL immediately with that step named. A journey
  with a hole in it cannot be judged as one artifact, and patching the hole by skipping the step is
  the one outcome this item exists to prevent. Procuring a physically separate machine as RM-2 is a
  human decision under 00_MASTER_PROTOCOL.md section 6; escalate for it, never assume it.
status: DRAFT
notes: >
  Tier XL and the terminal item of pack T5. It renders the verdict. Everything upstream in this pack
  exists so that this run is honest rather than lucky.
---

## 1. Objective

A single scripted journey — cold install, first launch, author, simulate, inspect, agent-drive,
publish — runs end to end in the clean-environment substitute `G1.13` defines, completes inside ten
minutes of wall-clock, is recorded as one continuous artifact, and is judged blind as one product
rather than as seven capable parts. Every step either succeeds or fails with a stated, actionable
message; no step is skipped and no step silently does nothing.

## 2. Context you need (self-contained)

**What Eustress is, and how it must be described in anything this item produces.** Eustress is an
AI-native simulation substrate — a world model an AI reasons over and a document a human edits. It is
**never** called a game engine; 3D rendering and the entity-component-system are implementation
details in service of that goal. The licence is **PolyForm Shield 1.0.0**: say **source-available**,
never open source. The physics engine is **Avian**, never Rapier. `.slint` files compile to Rust, so
**Slint is Rust**. Units are **meter-native**; studs are a display unit only. These are not stylistic
preferences — a recorded journey that shows or narrates any of them wrongly is a defective artifact.

**Build reality.** A full engine build takes 10–15 minutes. Only one cargo build at a time; the
workspace shares a single `eustress/target/` and concurrent builds produce link failures. Never kill
a build mid-compile. Validate with `cargo run`, not `cargo check`. Cargo runs from `eustress/` — the
repository root has no `Cargo.toml`.

**The seven journey steps, with the concrete surface each exercises.**

1. **Cold install.** The Windows installer is built by Inno Setup from `installer/windows/eustress-engine.iss`
   (invoked by `.github/workflows/release.yml`). **The binaries are unsigned.**
   `docs/launch/PUBLIC_ALPHA_CHECKLIST.md` line 25 records code-signing the installer and executable
   as a `[SHOULD]`, not yet done, and notes that unsigned binaries produce operating-system warnings.
   The journey must record the SmartScreen or Gatekeeper prompt as part of the recorded experience —
   it is what a real first user sees, and hiding it makes the artifact a lie.
2. **First launch.** On a blank profile, with no `%LOCALAPPDATA%` state.
   `docs/launch/PUBLIC_ALPHA_CHECKLIST.md` also requires that the telemetry first-run privacy notice
   appears exactly once (line 39); the journey must show it appearing and must show the opt-out being
   reachable.
3. **Author.** Create a Space and author content in the studio. The studio UI is 60 `.slint` files
   under `eustress/crates/engine/ui/slint/` driven from `eustress/crates/engine/src/ui/`.
4. **Simulate.** Enter play, run the simulation, stop, and observe the recorded result. The
   simulation kernel is `eustress/crates/common/src/simulation/` with Bevy integration at
   `eustress/crates/engine/src/simulation/plugin.rs`. **Time compression must be 1.0 for the entire
   journey.** `eustress/crates/common/src/simulation/clock.rs` advances `simulation_time_s` by the
   full compressed delta but caps physics ticks at `max_ticks_per_frame` and zeroes the accumulator on
   saturation (lines 100–102), so under compression the clock reports time that was never stepped.
   Any compressed-time claim in this journey would be unfounded.
5. **Inspect.** Open the Properties panel on an authored object, change a value, and — critically —
   **reload and re-read it**. `docs/AUDIT/02_STUDIO_ENGINE.md` records that the Properties panel does
   not persist edits in the default build: the legacy TOML write-back sits behind an opt-in `toml`
   cargo feature that `eustress/crates/engine/Cargo.toml`'s `core` tier deliberately excludes, and the
   Fjall mirror writes only `Transform`. The journey must include the re-read step and must record
   truthfully what happens. Do not build the persistence fix inside this item; record the result.
6. **Agent-drive.** Drive the running engine over the MCP tool surface: at minimum `scene_overview`,
   `create_entity`, `run_simulation`, `get_sim_value`, and a capture call. The engine bridge is
   advertised on `<universe>/.eustress/engine.port`; the `eustress` CLI's `bridge` subcommand
   (`eustress/crates/cli/src/main.rs`) speaks the same protocol as the MCP tools, one call per
   JSON-RPC round trip.
7. **Publish.** `eustress publish` pushes a Space to Cloudflare R2 via Wrangler
   (`Commands::Publish` in `eustress/crates/cli/src/main.rs`). Note honestly:
   `docs/launch/PUBLIC_ALPHA_CHECKLIST.md` line 15 still lists provisioning the
   `releases.eustress.dev` R2 bucket as an unchecked `[BLOCKER]`. If publish cannot succeed, the
   journey records the exact failure and its message — and that message must be actionable, per
   G7.42's contract. A publish step that hangs or silently no-ops fails this item.

**What every upstream item in this pack gives you, so you do not rebuild it.**
G7.30 the session ledger and crash-free denominator · G7.31 `eustress diag` and the structured JSONL
log · G7.32 the typed panic classifier and exit-code table · G7.34 a green `cargo test` in CI ·
G7.35 the client binary built and smoke-tested on three platforms · G7.37 the property-based
save/load round-trip · G7.38 `eustress space verify` and the container portability verdict · G7.39 the
machine-readable stuck-phase record · G7.40 the machine-readable profiler and `perf-assert` · G7.41
the startup/shutdown contract and cold-start baseline · G7.42 the fault matrix and typed recovery ·
G7.43 the regression fleet · G7.44 the deployment profile. Read their artifacts under
`docs/PROMPTS/artifacts/`; cite them; do not duplicate them.

**Recording constraint — no desktop-free shortcut.** Steps 1–5 need a real desktop session.
`eustress/crates/engine/src/bin/headless.rs` is `MinimalPlugins` plus `ScheduleRunnerPlugin` — no
winit, no GPU, no Slint — and its own module docs state that `ai_camera` and `viewport.capture` need
a future `--render gpu` tier that does not exist. The journey is therefore recorded on a real
machine. Record the machine.

**Blinding.** The Critic never sees your self-report, your commit messages, or any caption. It sees
the recording, the measured numbers, and `path:line` citations. Anything you want credited must be
visible in the artifact itself.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/harness/recipes/G7_cohesion_journey.json` — the journey recipe this item authors
- A journey-driver script or binary under `scripts/` or `eustress/crates/cli/src/`
- `docs/PROMPTS/artifacts/G7.45/` — the artifact directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- **Any product source code.** This item measures and records; it does not fix. A defect the journey
  exposes is reported with its `path:line` and escalated, not patched here. Patching product code
  inside the item that judges the product destroys the artifact's value.
- `docs/AUDIT/` and `docs/launch/PUBLIC_ALPHA_CHECKLIST.md` — cited, not edited
- Any prior item's artifact under `docs/PROMPTS/artifacts/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever the recording looks
  like.** Removing a step, pre-warming the machine and calling it cold, editing the recording to cut
  a wait, re-running only the steps that went well, or narrating over a failure are all measurement
  changes. If a step is genuinely unmeasurable, report `EXIT_CRITERION_UNMEASURABLE` with evidence
  and stop.
- **One continuous recording.** No cuts, no splices, no speed-up. A ten-minute product must survive
  ten unedited minutes.
- **The clean-environment substitute, exactly as `G1.13` defines it.** `G1.13` amended the "not the
  founder's machine" condition to a reproducible one, because "a machine that has never run Eustress"
  is not a specification an agent can satisfy or falsify — the founder's box has run Eustress and
  procuring another is not an agent's decision. The substitute is: a fresh clone of the repository at
  a named commit into a new directory (not a `git clean` of the working tree); `HOME` and
  `USERPROFILE` redirected to an empty temporary directory; every `EUSTRESS_*` environment variable
  enumerated and cleared; a checkout-local `CARGO_TARGET_DIR`; and no `%LOCALAPPDATA%/Eustress` state.
  Record the preparation transcript, and record what the substitute cannot prove: a
  GPU-driver-dependent, driver-version-dependent, or OS-build-dependent difference survives it
  undetected, because it is the same physical box. `G1.02` pinned that box as `RM-1`.
- **Procuring a second machine is a human decision, not an approach change.** If the residual risk
  above is judged unacceptable, escalate under `00_MASTER_PROTOCOL.md` §6 with the two options
  `G1.13` already frames: PROCURE or borrow a second machine, register it as `RM-2` in
  `docs/PROMPTS/harness/reference_machines.json`, and re-run this item against it; or ACCEPT the
  substitute and record the residual risk in the phase report. An agent may not buy, rent, or borrow
  hardware, and may not spend iterations trying to simulate a second machine on the first.
- **The clock starts at the first user action of the install** and stops when the publish step
  returns — success or stated failure. Record the wall-clock elapsed time per step and in total.
- Time compression stays at 1.0 for the whole run (see §2, step 4).
- Run the journey at least **3** times. A single successful run is an anecdote; the artifact reports
  all three, including the ones that went badly.
- Do not fix what you find. Record it, cite it, escalate it.

## 5. Exit criterion

### Criterion
Across **3** independent journey runs in the `G1.13` clean-environment substitute: all **7** steps
are attempted in every run with **0** skipped; every step either succeeds or fails with a recorded,
actionable message; **0** steps produce a silent no-op or an unrecoverable state; the median total
wall-clock is at most **10.0 minutes**; and the blinded recording scores at least **8.0** on each of
D1, D4, D5 and D6.

### Measurement

Command (bash, from the repo root):

    ./scripts/journey-run.sh \
        --recipe docs/PROMPTS/harness/recipes/G7_cohesion_journey.json \
        --runs 3 \
        --out docs/PROMPTS/artifacts/G7.45/journey_record.json ; echo "JOURNEY_EXIT=$?"
    python - <<'PY' ; echo "EXIT=$?"
    import json, statistics, sys
    d = json.load(open("docs/PROMPTS/artifacts/G7.45/journey_record.json"))
    runs = d["runs"]
    STEPS = ["cold_install","first_launch","author","simulate","inspect","agent_drive","publish"]
    skipped, silent, stuck, unactionable = [], [], [], []
    for r in runs:
        by = {s["id"]: s for s in r["steps"]}
        for sid in STEPS:
            s = by.get(sid)
            if s is None or s.get("attempted") is not True:
                skipped.append((r["id"], sid)); continue
            if s["outcome"] == "silent_noop":   silent.append((r["id"], sid))
            if s.get("unrecoverable") is True:  stuck.append((r["id"], sid))
            if s["outcome"] == "failed" and not s.get("actionable_message"):
                unactionable.append((r["id"], sid))
    totals = [r["total_minutes"] for r in runs]
    print("runs", len(runs), "skipped", skipped, "silent", silent,
          "unrecoverable", stuck, "unactionable", unactionable,
          "median_minutes", round(statistics.median(totals), 2))
    sys.exit(0 if len(runs) >= 3 and not skipped and not silent and not stuck
             and not unactionable and statistics.median(totals) <= 10.0 else 1)
    PY

Then produce the blinded bundle for the Critic:

    cargo run --release --bin eustress-capture -- \
        --recipe docs/PROMPTS/harness/recipes/G7_cohesion_journey.json \
        --subject HEAD \
        --control 71ccf6fe \
        --out docs/PROMPTS/artifacts/bundles \
        --trials 5 \
        --verify-determinism ; echo "CAP_EXIT=$?"

Expected output shape:

    JOURNEY_EXIT=0
    runs 3 skipped [] silent [] unrecoverable [] unactionable [] median_minutes 8.7
    EXIT=0
    CAP_EXIT=0

Pass condition:

    `EXIT=0` with three runs, no skipped steps, no silent no-ops, no unrecoverable states, every
    failure carrying an actionable message, and median total wall-clock at or under 10.0 minutes;
    `CAP_EXIT=0`; and the Critic at or above 8.0 on each of D1, D4, D5 and D6.

A step that **fails with a clear, actionable message** does not fail this item. A step that is
skipped, hangs, silently does nothing, or leaves the product unusable does.

## 6. Critic gate

Gated on **D1, D4, D5, D6**, floor **8.0** on each. The mean is irrelevant — one dimension below
floor fails the item. **D7 is deliberately not gated here**; see below.

- **D1 (first-three-seconds impact)** — what a stranger concludes in the first 3.0 seconds of the
  recording, before any explanation. The opening is the install prompt on an unsigned binary; that is
  the real first impression and it is being judged.
- **D4 (UI craftsmanship)** — whether the studio surface was designed or assembled. The `inspect`
  step's re-read is deliberately in the journey; a value that appears to save and does not persist is
  a craftsmanship finding, not a hidden one.
- **D5 (simulation believability and numerical trust)** — whether the `simulate` step both looks
  right and would survive a domain expert reading the numbers. Time compression is pinned at 1.0
  precisely so this dimension has something honest to score.
- **D6 (overall coherence)** — the point of the item. Seven steps that each work but disagree with
  each other on vocabulary, feedback, and pacing read as seven products.
**Why D7 is not on this item.** The capture command below runs `--subject HEAD --control 71ccf6fe`.
Both sides are Eustress; `71ccf6fe` is this pack's own predecessor commit. D7 asks a judge which of
two artifacts it would rather use, and a forced choice between a build and its own parent does not
answer that question — it answers "did this pack change anything visible", which the other four
dimensions already measure with citations. The floor makes it worse rather than better: at 4-of-5
against a coin, a self-comparison clears the floor by chance alone about 19 times in 100, so a D7
pass here would carry roughly the evidentiary weight of a coin landing heads four times.

D7 requires a control the judge could plausibly prefer — something a buyer already trusts. Procuring
one is `G1.14`, which is marked HUMAN-EXECUTED for a reason: `00_MASTER_PROTOCOL.md` §4.1 R2 forbids
an agent to select the product, acquire the artifact, perform the capture, or set
`publication_rights`. Until `G1.14` lands a licensed external control with a complete provenance
manifest, no item in this library can honestly gate on D7, and this item does not pretend to. The
`71ccf6fe` control stays — R4 makes our own prior build the default comparison and it is always legal
to publish — but it is scored on D1/D4/D5/D6, where a self-comparison is exactly the right instrument.

Capture recipe: `docs/PROMPTS/harness/recipes/G7_cohesion_journey.json`, authored by this item.

The Critic can refuse to pass a run that clears every number, and can never pass one that misses a
number. Treat the numeric floor as the beginning of the argument.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — scripted driver over the real desktop, one continuous recording per run
   -> if still failing, MANDATORY approach change. Re-recording the same script is NOT an approach
      change; restructuring the journey's step ordering or its narrative spine IS.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND the
                  median total wall-clock moving < 5%
  - Budget      : 1.8M tokens or 30 builds consumed (150% of the XL envelope)
  - Item-specific: any step cannot be completed at all on a never-run machine (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the wall-clock ceiling or a
named dimension floor to a stated value with the stated consequence; FUND a specific approach D with
an estimate and why it is materially different; DEFER behind a named blocking item; or KILL with a
statement of what the programme loses. A packet asking the human to "review the situation" is
malformed.

## 8. Artifact

`docs/PROMPTS/artifacts/G7.45/journey_record.json`

A reader finds: `runs` — three entries, each with the machine preparation, the hardware, the commit,
`total_minutes`, and `steps` (per step: `id`, `attempted`, `outcome`, `elapsed_seconds`,
`actionable_message` where failed, `unrecoverable`, and the recording timestamp range it occupies);
the recording paths and their hashes; the blinded bundle hash and the Critic scorecard reference;
every defect the journey exposed with its `path:line` and the item it was escalated to; the citation
list to every upstream artifact this run relied on; and the explicit statement of what the product
does **not** do, discovered in the run. This file plus its recordings are the W4 evidence for the
whole pack and the artifact the phase report cites.

## 9. Definition of NOT done

- The journey completes in eight minutes because the install step was performed off-camera on a
  machine that already had the runtime dependencies.
- A step failed, was fixed inside this item, and the journey was re-recorded — so the artifact
  records a product that did not exist when the run began.
- The `inspect` step omits the reload-and-re-read, so the known Properties-persistence behaviour is
  invisible and the artifact overstates the product.
- The publish step is skipped because the R2 bucket is not provisioned. Skipping is the one thing
  this item forbids; attempt it and record the message.
- The recording is spliced to remove a 40-second load wait. The wait is the product.
- Time compression was raised during the `simulate` step to make the result arrive sooner, so every
  number in that step rests on the clock's step-drop behaviour and D5 is unfounded.
- All numeric floors are met and the Critic refuses the wow gate, citing that the seven steps read as
  seven separate tools sharing a window. That is a legitimate refusal; the item is not done.
- The artifact narrates the product as "a game engine", or says "open source", or names Rapier, or
  treats studs as the native unit. Any one of those makes the artifact unusable regardless of the
  numbers.
- The recording clears D1, D4, D5 and D6 against `71ccf6fe` and the result is reported as a
  preference win. It is not one. Both sides are Eustress, and no judge was asked which product
  they would rather use. D7 needs the licensed external control `G1.14` procures.
- D7 is added back to `critic_gate` so the item can claim a preference result. Re-adding a gate
  the item was scoped without is a measurement change and fails the item outright.
- A second physical machine is procured, rented, or borrowed to satisfy the cold-install
  condition. That is a spend and a human decision under `00_MASTER_PROTOCOL.md` §6; the agent
  escalates for `RM-2` and runs the `G1.13` substitute in the meantime.
