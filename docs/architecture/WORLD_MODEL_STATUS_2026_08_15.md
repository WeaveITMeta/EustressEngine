# World-Model Simulator: Verified Status Ledger

**Date:** 2026-08-15
**Method:** every row below was checked against the tree with a *discriminating*
test: not "does the symbol exist" but "does it reach a running binary". A module
that compiles, self-tests, and is called by nothing is recorded as **orphaned**,
not as **exists**.

Companion to `WORLD_MODEL_SIMULATOR_ROADMAP.md`. Where the two disagree, this
file is the newer measurement.

---

## 1. Headline: four things that reach nothing

The recurring defect class in this codebase is authored value that no running
binary consumes. Four live instances:

| # | What | Evidence | Consequence |
|---|---|---|---|
| 1 | **`eustress-genesis`** (752 LOC): the Phase 4/5/6 spine (1D FEA, candidate schema, fitness, optimizer, ingest contracts) | Workspace member (`Cargo.toml:41`), so it builds. `eustress-genesis` appeared in exactly one `Cargo.toml` line in the entire workspace: its own `name =`. Zero dependents. | **NOW WIRED.** `engine/src/generative_arch.rs` hosts `GenerativeArchPlugin`, registered in `app_core.rs`. See §7. |
| 2 | **`sync_workspace_gravity_to_avian`** (`crates/common/src/services/workspace.rs:152`): the canonical, unit-converting gravity system | Scheduled by nothing. The only two references in the tree were its own definition and a doc mention at `workspace.rs:33`. | **NOW WIRED.** Scheduled from `plugins/workspace_plugin.rs`. See §7. |
| 3 | **`bake_to_echk`** (`crates/worlddb/src/bake.rs:89`): the Fjall-tree to `.echk` chunk exporter | No caller anywhere in `crates/engine`. The only engine mention is a comment at `editor_settings.rs:633` noting the export "is needed". | `.echk` live streaming (Phase 7) has an exporter and no consumer. |
| 4 | **WorldState DTO**: Phase 1's "one serializer for bridge/MCP/Properties" | No such type exists. The only `WorldState` hits are an unrelated networking protocol variant (`eustress-networking/src/protocol.rs:548`) and the `sandbox` module's external-solver *trait* (`common/src/sandbox/mod.rs`), a different concept. | Bridge, MCP and Properties each serialize state their own way. |

Item 2 was the sharpest: the orphaned function's own doc comment asserted the
opposite of the truth, claiming that `eustress-runtime` and `eustress-networking`
both scheduled it "instead of each defining their own". Neither did, and neither
could: both of those modules are compiled out (see C6 in §3). That doc comment
has been corrected.

The live defect the claim concealed was in a third place entirely, the engine's
own Rune bridge. Chasing the documented duplicates would have missed it. Where a
doc asserts a consolidation, verify the consolidated callsite exists before
trusting that the duplicates are the problem.

---

## 2. Corrections to the existing record

Both the roadmap's §6 ledger and the `project_world_model_phases` memory are
stale in **both directions**. Some work is further along than recorded; some
recorded-as-done work never landed.

### Recorded as pending, actually landed

| Item | Old record | Verified now |
|---|---|---|
| **C1** (TOML demotion / DB authority) | Roadmap: "Critical, pending". Memory: "the ONE holdout". | **RESOLVED.** `world-db` is in the `core` tier which is in `default` (`crates/engine/Cargo.toml:389`). `write_instance_definition` (`instance_loader.rs:1486`) short-circuits: if `active_db::put_instance` accepts the instance it returns `Ok` *without writing TOML*. That is the authority flip. |
| **C12** (WorldDb + GS gated off default) | Roadmap: "Med, pending". | **RESOLVED.** Both `world-db` and `gaussian-splatting` are in the default `core` tier. |
| **Causal op-log producers** | Module doc (`worlddb/src/mutations.rs:11`) still says "SCAFFOLD ... wiring the live producers is staged". | **Producers are wired.** `active_db.rs:142` calls `record_mutation` from real engine paths. The module doc is stale. See §4 for the real remaining gap. |

### Recorded as done, actually not

| Item | Old record | Verified now |
|---|---|---|
| **C5** (gravity unit boundary) | Memory: "Phase 0 DONE ... gravity units". | **WAS NOT RESOLVED; NOW FIXED.** The live unconverted writer was `sync_rune_gravity` in the engine (`soul/physics_bridge.rs`), which wrote Avian `Gravity` straight from a Rune thread-local. It now writes `Workspace.gravity` instead, and the canonical converting system is scheduled from `WorkspacePlugin`. |
| **C6** (duplicate gravity sync) | Memory: "Phase 0 DONE". | **NO RACE EXISTED.** The `runtime` and `eustress-networking` gravity systems are behind `#[cfg(feature = "physics")]` (`runtime/src/lib.rs:47`, `eustress-networking/src/lib.rs:55`) on a feature commented out of both manifests (`runtime/Cargo.toml:28`, `eustress-networking/Cargo.toml:48`), in crates whose `avian3d` dependency is also commented out. Neither module has ever compiled. Both have now been deleted. |
| **WorldState DTO** | Memory: "Phase 1 ... WorldState DTO" landed. | **Never existed.** See §1 item 4. |
| **Phase 4 `realism::fea`** | Roadmap Phase 4 targets `realism::fea`. | **No FEA in `realism` at all.** `ls crates/common/src/realism/` shows 20+ modules, none of them `fea`. The only FEA in the tree is `genesis/src/fea.rs`, which is orphaned. |

### Recorded as a fixed size, actually grew

| Item | Old record | Verified now |
|---|---|---|
| **C4** (duplicated VM bindings) | Roadmap: Luau 5,102 + Rune 3,745 = **8,847 LOC**. | **10,270 LOC.** `common/src/luau/runtime.rs` = 5,253; `engine/src/soul/rune_ecs_module.rs` = 5,017. The duplication grew ~1,400 LOC since the roadmap was written. The shared `common/src/scripting/` layer (4,778 LOC) is **types-only** by its own module doc: data types, Instance API, events, services, plugin, datastore. It is not the behavior/ECS-binding facade C4 asks for. |

---

## 3. Contention items, corrected

| # | Item | Status | Evidence |
|---|---|---|---|
| C1 | State authority is TOML not DB | **RESOLVED** | `Cargo.toml:389` default tier; `instance_loader.rs:1486` short-circuit |
| C2 | No physics determinism config | **RESOLVED (engine only)** | `app_core.rs:224-236`: fixed 60 Hz, `SubstepCount(6)`, `SolverConfig`, `DeterminismPlugin`. Real test at `common/tests/determinism.rs`. **Caveat:** not uniform across binaries. `client/src/main.rs:84` pins `Time<Fixed>` 60 Hz but *not* `SubstepCount`/`SolverConfig`; `eustress-networking/src/physics.rs:266` runs 120 Hz. |
| C3 | Bridge TCP accept unverified | **RESOLVED** | `engine_bridge/self_test.rs` pings the bridge at startup, spawned at `mod.rs:528`. The 500 ms `recv_timeout` race is documented as removed at `mod.rs:432`. |
| C4 | Two parallel scripting stacks | **NOT RESOLVED, WORSE** | 10,270 LOC, shared layer is types-only. See §2. |
| C5 | Gravity bypasses unit boundary | **RESOLVED** | Canonical converting system scheduled in `plugins/workspace_plugin.rs`; `soul/physics_bridge.rs` `sync_rune_gravity` now writes `Workspace.gravity` behind a `Local` change-detector instead of writing `Gravity` directly |
| C6 | Duplicate gravity-sync systems | **NEVER APPLIED** | The two "duplicates" sat behind a `physics` feature commented out of both manifests, so neither ever compiled. No last-writer-wins race existed. Dead code now removed |
| C7 | run_simulation determinism is Monte-Carlo not physics | **RESOLVED** | `GlobalRngSeed` registered by `DeterminismPlugin`; `scenarios/engine.rs:160` derives from it |
| C8 | Duplicate StudioState | **RESOLVED** | Exactly one `pub struct StudioState` (`ui/mod.rs:253`); `ui/webview.rs:219` imports `super::StudioState` |
| C9 | GS to collider extraction is a TODO | **RESOLVED (Tier A)** | `radiance/src/collider.rs:61` `extract_colliders` is implemented, no TODO remains. Landed further via commits `12f4e845`, `1aad549f`, `3cf0e30f`, `6ebd4f68`. |
| C10 | No true FEA / multi-physics coupling | **NOT RESOLVED** | No `realism::fea`. No `PhysicsSet` type anywhere (Way 26 coupling contract absent). No golden/verification suites in `realism`. |
| C11 | FileAssetReader swaps root per Space | **RESOLVED by another route** | Two sources registered: `space://` (swappable, `app_core.rs:57`) and `bundled://` (fixed, via avatar boot registrar). External clouds were solved instead by `UnapprovedPathMode` permissive absolute paths (commit `6ebd4f68`), not by a third named source. |
| C12 | WorldDb + GS gated off default | **RESOLVED** | Both in default `core` tier |
| C13 | Monolithic engine crate | **PARTIAL** | The bin was thinned, but `crates/engine/src` is still **556 files / 190,126 LOC**. Crate decomposition has not happened. |
| C14 | light_cull needs 0.19 retune | **UNVERIFIED** | Present in `lib.rs`, `main.rs`, `plugins/lighting_plugin.rs`. Retune status not determinable statically; needs a GPU measurement. |
| C15 | P2P disabled on 0.19 | **UNCHANGED** | Off the world-model critical path by design |

---

## 4. Phase status, corrected

### Phase 0: Foundation
**5 of 7 clear.** C2, C3, C7, C8 resolved; C14 unverified; **C5 and C6 still
open**. Determinism is pinned in the engine binary only, not in client or
networking.

### Phase 1: State authoritative
**Substantially landed, two gaps.**
- WorldDb authoritative: **done** (C1/C12).
- Causal op-log: **partially done**. The storage API and producers are wired,
  but two real gaps remain:
  - **No `Update` records.** `record_semantic` is called from exactly three
    sites (`active_db.rs:177`, `:752`, `:796`), covering Create (FileWatcher +
    System) and Delete (System). `MutationOp::Update` is never produced, so
    property edits do not appear in the op-log and cannot be replayed.
  - **Causality fields are placeholders.** Every record is written with
    `tx_id: 0`, `parent_tx: None`, `reason: None`, `before: None`
    (`active_db.rs:115-140`). It is a durable, ordered mutation *stream*, not
    yet a *causal* one.
- WorldState DTO: **not started**.
- Units: length, accel and velocity converters exist (`units.rs:377`, `:400`,
  `:407`). **No mass units.** No `Dimension`/`Quantity` type (Way 40).

### Phase 2: Agent loop (POMDP)
**Roughly half.**
- Bridge verbs present: `sim.step`, `scene.raycast`, `scene.overview`,
  `oplog.tail`, `sim.bindings`, `data.bind`/`bindings`/`unbind`, plus the full
  entity CRUD, tool, selection, action and ai_camera surface
  (`engine_bridge/protocol.rs:218-253`).
- Bridge verbs **absent**: `scene.measure`, `scene.observe`,
  `scene.affordances`. There is no affordance model in the tree at all; every
  `affordance` hit is UI prose.
- Headless runner: the **host exists and is real** (`engine/src/bin/headless.rs`,
  declared `Cargo.toml:58-60`): loads a Space, ticks the real stack at a
  deterministic 60 Hz, advertises the bridge, exits 0. But it has **no episode
  semantics**: zero hits for sandbox, budget, manifest or episode.
- `Substrate` facade + three projections: **not started**.
- C4: **not started** (and regressed).

### Phase 3: Representation and Gaussian Splatting
**The most advanced phase.** C9 resolved to Tier A and pushed well beyond it
since (dynamic collider extraction for any imported splat, click-selection,
per-splat floater cull, PPISP toggles, one-button `.ply` import).
`gaussian-splatting` is default-on. Remaining: dual-channel scene contract,
representation capability matrix.

### Phase 4: Multi-physics and FEA
**First rung now reaches a binary.** The 1D FEA MVP is no longer inert: it is
wired as the yield gate inside `engine/src/generative_arch.rs` (see §7). Still
absent: `realism::fea` proper, `PhysicsSet`, verification harness, golden suites,
law cards, `Quantity` type. Deformation is still the vertex-displacement
approximation and has not been reclassified as visual-only in any DTO (there is
no DTO). The FEA that landed is axial-only and yield-only: no buckling, no
self-weight, one scalar DOF per node.

### Phase 5: Architecture-generation loop
**Now live.** Candidate schema, closed-form fitness, `Optimizer` trait, hill
climb and `run_loop` all run inside `GenerativeArchPlugin`. See §7 for what the
wiring had to fix before the loop was worth running.

### Phase 6: Ingest-and-surpass
**Still orphaned.** `GenerationBackend`, `GeneratedAsset`, `IngestSource` in
`genesis/src/ingest.rs` remain reachable from nothing; the engine dependency now
exists, but nothing calls the ingest half. Note the interchange half is
independently real: `.ply` and glTF import work today via `radiance`.

### Phase 7: Scale and web
**More landed than recorded, one orphan.**
- Morton: **in use** in the binary-core path (`active_db.rs:455`,
  `promote.rs:59-194`), not merely available.
- HLOD: **landed** (`space/hlod.rs`, wired into `world_db_binary` and
  `lighting_plugin`).
- Collider streaming / residency: **landed** (`physics/collider_streaming.rs`,
  Morton-cell locality by camera).
- `.echk`: **exporter orphaned**, no live streaming.
- wasm carve: not started. C13 crate decomposition not started.
- Open and owned: `deny.toml` present at repo root; **SPDX headers on 2 files**
  out of 556 in the engine crate alone.

### Cross-cutting
`symbolic/causal.rs` present. `worlddb/src/branch.rs` present (versioned-worlds
substrate). Ontology reconciliation and the falsifiable predict-then-check loop
not started.

---

## 5. What this implies for sequencing

The roadmap's build order is law-fixed: Foundation, then State/Determinism, then
Agent Loop, then the rest. Measured against the tree, the honest position is:

1. **Phase 0 is now clear**, once C14 is measured on a GPU. C5 is fixed and C6
   turned out never to have been a live defect. See §7.
2. **Phase 1 has two concrete gaps**, both small and well-defined: op-log
   `Update` coverage, and the WorldState DTO.
3. **Phase 2 is the real frontier**, and is the phase whose milestone
   ("an agent drives, inspects and learns over MCP in one live session") most
   directly serves the AI-substrate goal. The natural next step is a bridge verb
   over `GenerativeArchLedger`, which would add an agent-facing verb and give the
   Phase 5 loop an agent driver in one change.
4. **Phase 6 remains written and unplugged.** The engine now depends on
   `genesis`, so wiring `IngestSource` and `GenerationBackend` is a much shorter
   step than it was.

C4 (10,270 LOC of duplicated bindings) is the largest single item anywhere in
the plan and is growing. It deserves its own dedicated pass rather than being
folded into Phase 2 delivery.

---

## 6. Not verified here

- C14 (light_cull retune) needs a GPU measurement, not a source read.
- The audit itself was static verification against source. The changes in §7 are
  build-verified separately.

---

## 7. Changes made off the back of this audit

### C5: one converting writer of Avian `Gravity`

The live defect was not where the documentation pointed. `sync_rune_gravity`
(`engine/src/soul/physics_bridge.rs`) wrote Avian's `Gravity` directly from a
Rune thread-local, unconverted and unordered against anything else.

- `plugins/workspace_plugin.rs` now schedules the canonical
  `sync_workspace_gravity_to_avian`, the only system that writes `Gravity`.
- `sync_rune_gravity` now writes `Workspace.gravity` instead, so the flow is
  Rune to `Workspace` to converting system to Avian.
- It gates on a `Local<Option<f64>>` change-detector rather than comparing the
  thread-local against `Workspace.gravity`. Comparing would let the stale Rune
  default (9.80665) clobber a scene-authored gravity every frame; the detector
  writes only when a script actually changed the value, and adopts the boot value
  silently.
- Behaviour on a normal boot is neutral: `Workspace::default().gravity` is
  already `-9.80665` and the meter conversion is an identity path, so the guard
  does not even log.
- The dead `runtime` and `eustress-networking` gravity systems were deleted.

### Phase 5: `GenerativeArchPlugin`

`engine/src/generative_arch.rs` wraps `eustress-genesis`. Two defects in the
crate had to be fixed first, or wiring it would have shipped a crash vector and a
loop that produces nonsense:

1. **Panic on malformed input.** `ClosedFormFitness::score` calls
   `efficiency_heuristic` (which reaches `total_mass()`, indexing
   `materials[m.material]` and `nodes[m.from]` unchecked) *before*
   `compliance_heuristic`, the only bounds check in the crate. An agent-supplied
   candidate with an out-of-range index killed the process instead of scoring 0.
   `validate_candidate` now rejects it first.
2. **The optimizer had no strength term.** `HillClimb` tunes only `Member::area`;
   stability and compliance are area-invariant and efficiency is `load / mass`,
   which rises monotonically as area shrinks. An ungated loop drives every member
   to the `1e-6` floor while reporting a rising score. `FeaGatedFitness` wires the
   crate's own 1D FEA solver in as a hard yield gate, which is precisely the seam
   the `Fitness` trait doc designates. The two orphaned pieces resolve each other.

Honesty bounds on that gate: yield-only, no buckling, no self-weight, and
`Fea1d` carries one scalar DOF per node, so it is exact only for an axial chain.
`verify_fea` reports `colinear: false` and declines to produce numbers for any
other topology rather than fabricating them.

The plugin is inert until a `RunGenerativeArchEvent` arrives or
`EUSTRESS_GENESIS_ITERS` is set. The `Gizmos` overlay is a separate editor-tier
plugin because the headless shell registers no `GizmoPlugin`.
