# 04 — CROSS-PACK FILE OWNERSHIP

**Status:** Normative. Binding on every item in `docs/PROMPTS/packs/`.
**Companions:** `00_MASTER_PROTOCOL.md` (authority), `03_PROMPT_SCHEMA.md` (authoring format).
**Scope of authority:** This file decides who may edit a source file that more than one pack claims.
Where a pack's scope list conflicts with this file, this file wins.

---

## 1. The problem this file solves

`03_PROMPT_SCHEMA.md` §4.3 makes an item's in-scope list contractual: the executing agent may edit
those files and may not edit anything else. Each pack was authored with a closed, acyclic dependency
graph, so two items *inside* one pack are always ordered relative to each other.

Nothing ordered two items in *different* packs. Thirty-two source paths are claimed in-scope by
items in more than one pack. `eustress/crates/engine/src/bin/headless.rs` alone is claimed by seven
items across three packs; `eustress/crates/engine/Cargo.toml` by ten items across six.

That matters more here than in a normal repository because of a constraint in
`00_MASTER_PROTOCOL.md` §2.0: the workspace shares one `target/` directory, so **only one L1 may be
in a build-consuming state at a time**. Cross-pack work on a shared file is therefore not merged —
it is serialised, in whatever order the queue happens to produce, with each agent measuring against
a file shape the next agent is about to change.

The remedy is ownership plus an edge. Every contested path has exactly one owner. Every claimant in
another pack carries `depends_on: [<owner>]`, so the graph — not the queue — decides the order.

---

## 2. How to read a row

| Column | Meaning |
|---|---|
| **Path** | The contested path, exactly as it appears in the claiming items' scope lists |
| **Disposition** | `EXCLUSIVE`, `APPEND-ONLY`, or `DIRECTORY` — see below |
| **Owner** | The single item that holds the edit right at the point of contention |
| **Other claimants** | Every item in another pack that claimed the path |
| **Edge** | The `depends_on` entry each of those claimants now carries |

**Owner** names one item. Where the owning pack has several co-claimants, they are already ordered
by that pack's own closed acyclic graph; the Owner column names that pack's **entry item** for the
path — the earliest item in the owning pack that may edit it. Contention *inside* a pack is not this
file's business.

The three dispositions are not stylistic. They describe what kind of file it is:

**`EXCLUSIVE`** — a behaviour file. Two packs editing it produce two different behaviours, and
whichever lands second silently invalidates the first one's measurement. The path has been **removed
from every other pack's in-scope list and added to its out-of-scope list.** A claimant that cannot
reach its exit criterion without editing the file escalates to L0 under `FILE-OWNERSHIP`. It does
not edit the file.

**`APPEND-ONLY`** — a registry: a `Cargo.toml`, a message-type enum, a tool-schema table, a `mod`
list. Several items each need their own entry and none needs another's. Removing these files from
every non-owner's scope would make most of the program unexecutable, and it would not fix anything,
because the failure mode here is not two items wanting the same stanza — it is one item rewriting a
stanza another item depends on. So the entry stays in scope and the **out-of-scope list forbids the
specific thing that breaks: altering, reordering, or removing an entry the claimant did not add.**

**`DIRECTORY`** — a directory claimed by two packs that work on different files inside it. There is
no file-level collision to resolve, so unscoping a pack from its own crate would be a scope error,
not a fix. The owner owns the directory's **layout**; other claimants keep file-level edit rights
and may not move, rename, or delete a file another item owns.

Three rows carry no owner. Those are items claiming an entire source tree
(`eustress/crates/engine/src/`, `eustress/crates/mcp-server/src/`,
`eustress/crates/engine/src/space/`). A contract over "everything" binds nothing, and the real
contention in each case is a named file that has its own row. Those rows are recorded, not enforced;
an edit to a file that has an owner is governed by that file's row regardless of the broader claim.

**`depends_on` is authoritative.** `03_PROMPT_SCHEMA.md` §3 makes `blocks` informational. The
`blocks` lists on owning items have been updated to match, but an L1 sequences from `depends_on`.

---

## 3. The ownership table

| Path | Disposition | Owner | Other claimants | Edge |
|---|---|---|---|---|
| `eustress/crates/engine/src/bin/headless.rs` | EXCLUSIVE | `G7.03` (T4) | `G2.03` (T2), `G7.31` (T5), `G7.32` (T5), `G7.39` (T5), `G7.41` (T5) | `depends_on: [G7.03]` |
| `eustress/crates/common/src/simulation/clock.rs` | EXCLUSIVE | `G1.07` (G1) | `G2.04` (T2), `G2.05` (T2), `G7.15` (T4) | `depends_on: [G1.07]` |
| `eustress/crates/common/src/simulation/recorder.rs` | EXCLUSIVE | `G1.07` (G1) | `G2.03` (T2), `G7.15` (T4) | `depends_on: [G1.07]` |
| `eustress/crates/engine/src/simulation/plugin.rs` | EXCLUSIVE | `G1.07` (G1) | `G2.04` (T2), `G2.05` (T2), `G2.14` (T2), `G7.15` (T4) | `depends_on: [G1.07]` |
| `eustress/crates/engine/src/ai_camera.rs` | EXCLUSIVE | `G1.05` (G1) | `G3.01` (T1), `G3.04` (T1), `G3.06` (T1), `G3.07` (T1), `G3.08` (T1), `G4.02` (T1), `G4.03` (T1), `G7.03` (T4), `G7.09` (T4) | `depends_on: [G1.05]` |
| `eustress/crates/engine/src/engine_bridge/protocol.rs` | APPEND-ONLY | `G1.05` (G1) | `G3.01` (T1), `G4.01` (T1), `G7.07` (T4), `G7.09` (T4), `G7.13` (T4), `G7.16` (T4), `G7.17` (T4) | `depends_on: [G1.05]` |
| `eustress/crates/mcp-server/src/bridge_tools.rs` | APPEND-ONLY | `G1.05` (G1) | `G3.01` (T1), `G7.02` (T4), `G7.05` (T4) | `depends_on: [G1.05]` |
| `eustress/crates/engine/Cargo.toml` | APPEND-ONLY | `G1.03` (G1) | `G0.03` (G0), `G3.01` (T1), `G4.02` (T1), `G2.01` (T2), `G5.23` (T2), `G7.03` (T4), `G7.37` (T5) | `depends_on: [G1.03]` |
| `eustress/Cargo.toml` | APPEND-ONLY | `G1.01` (G1) | `G0.03` (G0), `G3.02` (T1), `G7.01` (T4) | `depends_on: [G1.01]` |
| `eustress/crates/common/Cargo.toml` | APPEND-ONLY | `G2.02` (T2) | `G2.30` (B1) | `depends_on: [G2.02]` |
| `eustress/spaces/harness/` | EXCLUSIVE | `G1.03` (G1) | `G3.01` (T1) | `depends_on: [G1.03]` |
| `eustress/crates/engine/src/frame_diagnostics.rs` | EXCLUSIVE | `G1.08` (G1) | `G4.01` (T1), `G5.21` (T2), `G7.40` (T5) | `depends_on: [G1.08]` |
| `eustress/crates/engine/src/profiler.rs` | EXCLUSIVE | `G1.08` (G1) | `G5.21` (T2), `G7.40` (T5) | `depends_on: [G1.08]` |
| `eustress/crates/common/src/physics/determinism.rs` | EXCLUSIVE | `G2.02` (T2) | `G1.11` (G1), `G7.14` (T4) | `depends_on: [G2.02]` |
| `eustress/crates/engine/src/simulation/electrochemistry.rs` | EXCLUSIVE | `G2.08` (T2) | `G2.32` (B1) | `depends_on: [G2.08]` |
| `eustress/crates/engine/src/ui/slint_ui.rs` | EXCLUSIVE | `G6.13` (T3) | `G7.42` (T5) | `depends_on: [G6.13]` |
| `eustress/crates/engine/src/ui/` | DIRECTORY | `G6.04` (T3) | `G6.31` (B1) | `depends_on: [G6.04]` |
| `eustress/crates/engine/src/main.rs` | EXCLUSIVE | `G7.31` (T5) | `G6.02` (T3) | `depends_on: [G7.31]` |
| `eustress/crates/engine/src/app_core.rs` | EXCLUSIVE | `G7.30` (T5) | `G7.03` (T4) | `depends_on: [G7.30]` |
| `eustress/crates/engine/src/lib.rs` | APPEND-ONLY | `G1.04` (G1) | `G6.02` (T3) | `depends_on: [G1.04]` |
| `eustress/crates/engine/src/mesh_import.rs` | EXCLUSIVE | `G2.34` (B1) | `G5.02` (T1) | `depends_on: [G2.34]` |
| `eustress/crates/engine/src/space/load_phase.rs` | EXCLUSIVE | `G7.39` (T5) | `G3.12` (T1) | `depends_on: [G7.39]` |
| `eustress/crates/tools/src/cad_tools.rs` | APPEND-ONLY | `G2.35` (B1) | `G7.18` (T4) | `depends_on: [G2.35]` |
| `eustress/crates/tools/src/simulation_tools.rs` | EXCLUSIVE | `G7.08` (T4) | `G0.11` (G0), `G5.22` (T2) | `depends_on: [G7.08]` |
| `eustress/crates/common/src/simulation/watchpoint.rs` | EXCLUSIVE | `G2.04` (T2) | `G7.10` (T4) | `depends_on: [G2.04]` |
| `eustress/crates/worlddb/src/` | DIRECTORY | `G7.08` (T4) | `G7.37` (T5), `G7.38` (T5) | `depends_on: [G7.08]` |
| `eustress/crates/agent-eval/` | DIRECTORY | `G7.01` (T4) | `G0.01` (G0), `G0.02` (G0), `G0.07` (G0), `G0.10` (G0), `G0.11` (G0) | already transitive to `G7.01` |
| `eustress/crates/tools/src/` | DIRECTORY | `G7.02` (T4) | `G0.01` (G0), `G0.02` (G0) | already transitive to `G7.02` |
| `eustress/crates/engine/src/bin/` | DIRECTORY | `G2.01` (T2) | `G7.30` (T5), `G7.40` (T5), `G7.41` (T5), `G7.42` (T5) | — *(no edge required; no file-level collision)* |
| `eustress/crates/engine/src/` | DIRECTORY | (none — over-broad claim) | `G7.20` (T4), `G7.31` (T5), `G7.32` (T5) | — *(recorded, not enforced)* |
| `eustress/crates/mcp-server/src/` | DIRECTORY | (none — over-broad claim) | `G0.01` (G0), `G0.02` (G0), `G5.22` (T2) | — *(recorded, not enforced)* |
| `eustress/crates/engine/src/space/` | DIRECTORY | (none — over-broad claim) | `G2.03` (T2), `G6.05` (T3), `G7.41` (T5) | — *(recorded, not enforced)* |

### 3.1 Why each owner

**`eustress/crates/engine/src/bin/headless.rs`** — `G7.03`. G7.03 replaces the 291-line MinimalPlugins shell with a render tier; every other claimant reads or extends what G7.03 leaves behind. T4's pack header already splits this file between G7.03 (render tier) and G7.14 (a narrow `--tick-rate` licence); that intra-pack split stands.

**`eustress/crates/common/src/simulation/clock.rs`** — `G1.07`. G1.07 adds the independent physics-step counter that G2.04's compression instrumentation and G7.15's dropped-tick accounting both read. The counter must exist before either can be honest about a dropped tick.

**`eustress/crates/common/src/simulation/recorder.rs`** — `G1.07`. Same change as clock.rs: G1.07 exports the step counter into the recording. Splitting the counter and its export across two packs guarantees one of them measures a field the other has not written yet.

**`eustress/crates/engine/src/simulation/plugin.rs`** — `G1.07`. The third file in G1.07's single change — the counter is incremented from the physics schedule here. G1.06 also edits it, intra-pack, sequenced by G1's own graph.

**`eustress/crates/engine/src/ai_camera.rs`** — `G1.05`. G1.05's exit criterion IS this file — `AI_CAM_WIDTH`/`AI_CAM_HEIGHT` are hardcoded to 1280/720 at ai_camera.rs:43-44 and `request_capture` at ai_camera.rs:158 takes only a path. Nine downstream items assume a parameterised capture; exactly one may build it.

**`eustress/crates/engine/src/engine_bridge/protocol.rs`** — `G1.05`. A message-type registry, not a behaviour file: nine items each need their own request/response variant and none needs to change another's. G1.05 lands first because the capture parameters are the shallowest change and everything else in G1 is gated behind them.

**`eustress/crates/mcp-server/src/bridge_tools.rs`** — `G1.05`. The MCP-side mirror of protocol.rs — one input schema per tool, additively. G1.05 owns the `ai_camera_capture` and `capture_viewport` schemas; G7.02 and G7.05 own their own tools' schemas.

**`eustress/crates/engine/Cargo.toml`** — `G1.03`. A manifest. G1.03 registers the first harness `[[bin]]`; every later claimant appends its own `[[bin]]`, dependency, or feature. The failure mode is not two packs wanting the same stanza — it is one pack rewriting the `bevy` feature list under another. bevy_anti_alias and bevy_post_process are declared inert at engine/Cargo.toml:133-134; nothing here may drop them.

**`eustress/Cargo.toml`** — `G1.01`. G1.01's entire exit criterion is the `[workspace] members` list — it may remove `"crates/backend"` or implement the four missing `Database` methods, and must record which. No other item may touch that list until the decision is recorded; appending a new member crate afterwards is safe.

**`eustress/crates/common/Cargo.toml`** — `G2.02`. `eustress/crates/common/tests/determinism.rs` is gated behind a non-default `physics` feature, so it never runs. G2.02 owns the feature and `[[test]]` wiring that makes it run at all; G2.30 appends its own dev-dependencies on top.

**`eustress/spaces/harness/`** — `G1.03`. The harness scenes are generator output, never hand-authored, and every archived comparison is keyed to them. G1.03 owns the seeded generator; a second writer makes every earlier bundle incomparable.

**`eustress/crates/engine/src/frame_diagnostics.rs`** — `G1.08`. G1.08's artifact is the per-frame tick-indexed frame-time series. G4.01, G5.21 and G7.40 all consume that series; if any of them also produces it, the three packs measure three different things and call them all `ft_p99_ms`.

**`eustress/crates/engine/src/profiler.rs`** — `G1.08`. G1.08 adds the raw-series export and the output-path parameter; G7.40 then builds the machine-readable format and the perf-assert gate on top of that export. Reversing the order would have G7.40 define a format against a profiler that cannot yet emit one.

**`eustress/crates/common/src/physics/determinism.rs`** — `G2.02`. G2.02 is the item that makes this file compile and run by default at all. G1.11's claim is narrow (seeding one RNG consumer) and G7.14's is narrower still, but neither is testable until the `physics` feature gate is resolved.

**`eustress/crates/engine/src/simulation/electrochemistry.rs`** — `G2.08`. T2 rebuilds this file from a lumped 0-D model through SPM to P2D across G2.08-G2.13, sequenced by T2's own graph. B1's G2.32 declares a validity envelope for the lumped model — an envelope authored against a model T2 is mid-replacement is stale before it is written.

**`eustress/crates/engine/src/ui/slint_ui.rs`** — `G6.13`. 23,103 lines with exactly one drain registration. G6.13 splits it so no studio UI file exceeds 3,000 lines. G7.42's typed recovery surfaces must be written against the split files, not against a file scheduled to stop existing in its current shape.

**`eustress/crates/engine/src/ui/`** — `G6.04`. `StudioState` is defined exactly once, at eustress/crates/engine/src/ui/mod.rs:253, and the drain contract runs through it. G6.04 owns the drain-cost work that reshapes this directory; G6.31's dead-control sweep is a survey of the surface G6.04 leaves behind.

**`eustress/crates/engine/src/main.rs`** — `G7.31`. main.rs is a thin process shell — the app lives in app_core.rs. The only things that belong in the shell are the log-stream and panic-hook installs, which are G7.31's and G7.32's. G6.02's latency instrument belongs in the UI path, not the entry point.

**`eustress/crates/engine/src/app_core.rs`** — `G7.30`. Both claims are registration-only and both are one line in the same plugin list. G7.30 has no dependencies and is T5's item zero, so it lands first at no cost to T4's schedule.

**`eustress/crates/engine/src/lib.rs`** — `G1.04`. A module-declaration file. Each claimant adds its own `mod` line and nothing else; the failure mode is a removed declaration, not a contested one.

**`eustress/crates/engine/src/mesh_import.rs`** — `G2.34`. G2.34 adds the STEP import path and its measured-geometry floors. G5.02's round-trip budget is measured across import and export together, so it must be measured against the importer G2.34 ships, not against the one it replaces.

**`eustress/crates/engine/src/space/load_phase.rs`** — `G7.39`. The unfinished-phase watchdog is this file, and G7.39's exit criterion is a machine-readable stuck-phase record emitted from it. G3.12's opening-frame work reads load phases; it does not need to define them.

**`eustress/crates/tools/src/cad_tools.rs`** — `G2.35`. A tool-handler file. G2.35 lands the STEP round-trip handlers, G7.18 appends the outcome-scoring handlers; neither needs to alter the other's.

**`eustress/crates/tools/src/simulation_tools.rs`** — `G7.08`. G7.08 makes agent mutation transactional here (fork, rehearse, commit, zero residue). An egress guard (G0.11) or an experiment-throughput change (G5.22) written against the pre-transactional shape has to be rewritten once G7.08 lands.

**`eustress/crates/common/src/simulation/watchpoint.rs`** — `G2.04`. Subsystems that record straight into the watchpoint registry are invisible to anything reading only `SimValuesResource`. G2.04 owns the registry's shape; G7.10's failure proposals read it.

**`eustress/crates/worlddb/src/`** — `G7.08`. G7.08 is the structural change — fork and commit semantics. G7.37's round-trip fidelity floor and G7.38's integrity verifier are measurements of whatever shape the store ends in, so a floor measured before G7.08 lands is measured against a store that is about to change.

**`eustress/crates/agent-eval/`** — `G7.01`. G7.01 creates the crate. Every G0 claimant already reaches G7.01 transitively through G0.01's `depends_on: [G7.01, G7.19]`, so no new edge is required — only the recorded rule that G0 items add files here and never restructure the crate.

**`eustress/crates/tools/src/`** — `G7.02`. G7.02 owns the handler contract that makes a silent no-op impossible; G0.01 and G0.02 add the capability gate in front of it. Both G0 items reach G7.02 transitively (G0.01 -> G7.19 -> G7.18 -> G7.13 -> G7.02), so no new edge is required.

**`eustress/crates/engine/src/bin/`** — `G2.01`. Not a real collision: each claimant adds its own binary under this directory. The one file two packs genuinely contend for is headless.rs, which has its own row. Two packs naming the same new binary is an escalation, not a merge.

**`eustress/crates/engine/src/`** — (none — over-broad claim). Three items claim the whole engine source tree. That is a scoping defect, not an ownership question: 03_PROMPT_SCHEMA.md §4.3 makes the list contractual, and a contract over 'everything' binds nothing. Contention resolves at the named-file rows above; an edit to a file with an owner is governed by that file's row regardless of this claim.

**`eustress/crates/mcp-server/src/`** — (none — over-broad claim). `tools/call` at eustress/crates/mcp-server/src/main.rs:463-521 never reads `requires_approval`; G0.01 and G0.02 close that at the dispatch boundary, while G5.22 works on experiment throughput. Different files. bridge_tools.rs is the one contended file and has its own row.

**`eustress/crates/engine/src/space/`** — (none — over-broad claim). Three packs, three different files. load_phase.rs is the one contended file and has its own row.

---

## 4. Edges that are not file-ownership rows

Two ordering edges are required for the ruling above to hold, and neither comes from a shared path.

| Claimant | Now depends on | Why |
|---|---|---|
| `G3.01` | `G1.11` | `G3.01`'s determinism criterion covers `ai_camera.capture` alone. `00_MASTER_PROTOCOL.md` §1.1 D2 requires byte-identical **frames and recordings**, and `G1.11` is the item that verifies both. Without this edge T1's item zero would certify a narrower property than the one the program's definition of done needs. |
| `G7.32` | `G7.31` | `G7.31` and `G7.32` are siblings under `G7.30` and both install into `eustress/crates/engine/src/main.rs` — the log-stream init and the panic hook. Siblings are unordered, so the shared file had no owner inside T5 until this edge existed. |

---

## 5. What changed in the packs

The ruling is applied, not merely recorded. In `docs/PROMPTS/packs/`:

- **44 items** gained one or more cross-pack `depends_on` entries.
- **19 owning items** had `blocks` updated to match.
- **52 scope lists** were edited: `EXCLUSIVE` paths moved from in-scope to out-of-scope, and
  `APPEND-ONLY` / `DIRECTORY` paths gained a bounded out-of-scope entry naming the owner.
- The combined graph across all nine packs is **acyclic**, and every `depends_on` target resolves to
  an item that exists. Seven items remain dependency-free: `G1.01`, `G1.02` (the harness roots),
  `G0.06`, `G1.30`, `G1.40`, `G6.01`, `G7.30`.

Each pack header carries a **Cross-pack file ownership** section listing its own items' new edges.

---

## 6. Escalating a FILE-OWNERSHIP block

An executing agent that hits an out-of-scope entry naming this file does **not** edit the file and
does **not** work around it. It emits a `00_MASTER_PROTOCOL.md` §5.3 decision packet to L0 with:

```
FILE-OWNERSHIP  <item-id>
─────────────────────────────────────────────────────────────────
PATH            : <the contested path>
OWNER           : <owning item id>
WHY BLOCKED     : <the exit criterion that cannot be reached without this edit, and the number it misses>
MINIMAL EDIT    : <the smallest change that would unblock, as a diff sketch>
─────────────────────────────────────────────────────────────────
DECISION REQUESTED : exactly ONE of —
   (a) DEFER  this item behind <owner>, and fold the edit into <owner>'s scope
   (b) GRANT  a bounded exception: <the exact lines>, recorded as a new row in
              docs/PROMPTS/04_FILE_OWNERSHIP.md
   (c) REASSIGN ownership of the path to this item, and re-scope <owner>
   (d) KILL   this item
RECOMMENDATION     : <one of a-d> because <one sentence>
```

Option (b) is the only one that lets the item proceed, and only L0 may grant it. An agent that
grants itself a bounded exception has edited a file outside its declared scope, which
`00_MASTER_PROTOCOL.md` §2.2 forbids outright.

---

## 7. Adding a row

A new contested path is discovered whenever two packs' scope lists name the same file. To add it:

1. Name one owner, using the rule in §2 — the earliest item in the owning pack that may edit it.
2. Pick the disposition from the file's *kind*, not from convenience. If in doubt between
   `EXCLUSIVE` and `APPEND-ONLY`, ask whether two items could each add an entry without reading the
   other's. If they could not, it is `EXCLUSIVE`.
3. Add `depends_on: [<owner>]` to every claimant in another pack, and check the combined graph is
   still acyclic.
4. Edit the claimants' scope lists to match the disposition.
5. Add the row here, with the rationale. A row without a rationale is a preference, not a ruling.
