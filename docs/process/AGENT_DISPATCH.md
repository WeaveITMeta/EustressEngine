# Agent Dispatch — Multi-Agent Implementation Protocol

**Status**: Live process doc. Every dispatched worker agent reads this before starting.
**Owner**: orchestrator session
**Last revised**: 2026-05-26

> This document is the **contract** between the orchestrator and worker agents.
> It defines roles, lifecycle, file ownership, verification gates, and the
> dangerous feedback loops every implementation must avoid. The principles
> here trace back to Forrester systems-dynamics thinking: every change
> ripples through stocks, flows, and feedback loops in the live engine.

---

## Roles

### Orchestrator
- Singular Claude session driving the wave
- Reads spec docs (`docs/architecture/*.md`) as source of truth
- Dispatches worker agents with tight prompts
- **Owns**: `eustress/Cargo.toml`, `Cargo.lock`, `.forge/`, this doc, all `docs/architecture/*.md`
- Runs verification gates pre-merge
- Commits + pushes (per-task granular)
- Maintains TaskList kanban as shared memory

### Worker
- Background agent (`subagent_type: general-purpose`)
- Runs in isolated git worktree (`isolation: "worktree"`)
- Receives: task spec, OWNS/READS/MUST-NOT-TOUCH manifest, deliverable criteria
- Returns: branch name + change summary + test results
- **Does NOT push or merge**

### Integrator (sub-role of orchestrator)
- Runs after each worker returns
- `cargo check` inside the worktree before merging
- Runs the verification gate suite
- Merges to main (or rejects + re-dispatches)

---

## Dispatch Lifecycle

```
1. Orchestrator creates Task in TaskList (status: pending)
2. Orchestrator reads .forge/ownership.toml to confirm no conflicts
3. Orchestrator launches worker (Agent tool, run_in_background: true)
4. Worker reads this doc + the spec section + its OWNS manifest
5. Worker claims task (TaskUpdate → in_progress) — optional
6. Worker implements in its worktree
7. Worker returns: branch name + diff summary + test output
8. Orchestrator runs integrator pass:
   - cargo check in the worktree
   - touched-crate tests
   - log greps for known failure modes
9. Orchestrator merges to main (or rejects + amends + re-dispatches)
10. User runs the 4-click verification (Explorer ▶ / tag × / add tag / F2)
11. Orchestrator commits + pushes if green
12. Task marked completed
```

---

## File-Ownership Manifest

`.forge/ownership.toml` is the **machine-readable** source of truth.
Every dispatched agent reads it before claiming files. Violations fail
the task at integrator gate.

Three categories:

- **`[orchestrator_only]`** — only the orchestrator may modify. Workers
  that need changes here file an "ADD DEP" / "WORKSPACE CHANGE" task and
  the orchestrator handles serially.

- **`[immutable_during_wave]`** — pre-existing systems too risky to touch
  this wave. Workers may read but not write. Touching = task failure.

- **`[per_wave.<task_id>]`** — explicit OWNS list per active task.
  Files claimed by one task cannot be claimed by another running in parallel.

---

## Agent Prompt Template

Every worker agent prompt follows this exact shape — no more, no less:

```
TASK ID: <id from TaskList>
TASK: <one-paragraph description>

SPEC: <docs/architecture/SPEC.md#section> — the ONLY contract you need

OWNS (you may write these):
  - path/to/file/here
  - path/to/other/file

READS (you may read but not write):
  - reference/files

MUST NOT TOUCH (anything not listed above):
  - Cargo.toml family — orchestrator owns
  - any .slint file unless explicitly in OWNS
  - any other agent's OWNS files

DEPENDS ON: <prior task IDs that must be completed first>

DELIVERABLE:
  - <what the orchestrator verifies before merging>
  - <e.g. "cargo check passes", "test X passes", "no new resource validation failures">

PROTOCOL:
  - Read docs/process/AGENT_DISPATCH.md FIRST
  - Work in your assigned git worktree (auto-provisioned via isolation: worktree)
  - Do NOT push; do NOT commit; orchestrator handles git
  - Return: branch name + diff summary + cargo check output
```

---

## The 8 Dangerous Feedback Loops + Breakers

Every implementation must respect these. The breakers are non-negotiable.

### LOOP 1 — UUID write-back feedback (catastrophic)
Migration writes uuid to TOML → file_watcher fires "TOML changed" → re-import → potentially regenerates uuid → loop.
**BREAKER**: file_watcher compares mtime + content hash before re-import. OR migration pauses watcher with global mutex held until `migrated_to_uuid_at` is stamped.

### LOOP 2 — Spawner edit → sync → write-back → respawn (cosmetic but expensive)
Spawner.apply_edit writes to ECS → Changed<T> fires → sync writes TOML → watcher fires → reload → if "props unchanged" check is wrong → respawn → loop.
**BREAKER**: PropertyBag equality check before re-spawn. Sync writes TOML only on DEBOUNCED edits (file_watcher already debounces). Spawner uses Changed<T> not Added<T> for re-detection.

### LOOP 3 — LOD demotion drops collider mid-physics-tick
LOD switcher demotes Hero→Active → removes Collider, RigidBody::Dynamic stays → Avian solver runs next frame, body has no collider → falls through floor → scene corruption.
**BREAKER**: LOD demotion must remove RigidBody AND Collider atomically. OR LOD only changes shadow_enabled + Visibility, never physics — physics LOD is a separate Wave 4 system. **Wave 2/3 LOD touches visual components ONLY.**

### LOOP 4 — Parallel agents both edit Cargo.toml (build break)
Two worktrees both add deps to Cargo.toml → merge conflict → manual fix required.
**BREAKER**: Cargo.toml is `[orchestrator_only]`. Workers needing deps file an "ADD DEP" task; orchestrator handles serially BEFORE dispatching dependent agents.

### LOOP 5 — New resource breaks drain_slint_actions silently (lesson-of-the-session)
Spawner adds new component → drain signature adds ResMut<NewResource> → init_resource missed in active plugin → Bevy skips drain every frame silently → ALL Slint UI dead.
**BREAKER**: Wave 2.3 ships a startup-time assertion: a system that scans the active plugin's drain signature and panics in debug builds if any `Res`/`ResMut` parameter isn't matched by an `init_resource` in the same plugin. Built ONCE, pays dividends forever.

### LOOP 6 — Cargo feature / dependency convergence failure (user-added)
Agent A adds `dep = { features = ["foo"] }`, Agent B (parallel) adds same dep with `features = ["bar"]` → merge conflict OR incompatible features activated → runtime panics or silent misbehavior.
**BREAKER**: Orchestrator owns ALL Cargo.toml changes. Workers request via "ADD DEP" task with explicit feature set. Orchestrator runs `cargo tree -d` after every dep change to verify no duplicate-version warnings beyond the existing baseline.

### LOOP 7 — Reflection / Properties panel registration mismatch (user-added)
New component added via spawner → not registered with `bevy_reflect` → Properties panel shows blank or crashes on edit → user forces manual TOML edit → file_watcher → potential re-spawn with partial data.
**BREAKER**: Every `ClassSpawner` registration MUST include a `register_reflect_for_class()` hook. ClassRegistry enforces this — registration panics if `Reflect` impl missing on any of the spawner's components. Built in Wave 2.3.

### LOOP 8 — Selection / Gizmo state desync on spawn (user-added)
Spawner creates entity with Transform + new components while gizmo is active on another entity → selection manager doesn't know about the new entity → gizmo attaches to wrong thing OR gizmo on despawned entity OOPs.
**BREAKER**: Spawners NEVER touch selection state directly. SpawnCtx exposes an opt-in `defer_selection_to: Entity` callback that the selection manager consumes one frame after spawn. Default: no selection change.

---

## Pre-existing Systems (READ-ONLY for Wave 2)

These are stocks the dispatched agent does not own. Touching them = task failure. Reading is fine and encouraged.

| System | Location | Risk if touched |
|---|---|---|
| Slint UI | `crates/engine/ui/slint/**/*.slint` | Wave 2 is Rust-only — UI changes belong in Wave 3+ |
| file_watcher | `crates/engine/src/space/file_watcher.rs` | The chokepoint for TOML→ECS — Wave 2.1 coordinates via mutex, doesn't modify watcher itself |
| Avian physics integration | `crates/engine/src/physics/` | Physics LOD is Wave 4 — Wave 2/3 doesn't touch |
| Atmosphere/Sun celestial path | `crates/engine/src/plugins/lighting_plugin.rs` | Wave 3 lighting spawners do NOT register for Star/Sun/Moon |
| StudioUiPlugin (legacy) | `crates/engine/src/ui/slint_ui.rs:1050` | Legacy — DO NOT add resources here (see LOOP 5 lesson) |
| MCP bridge tool handlers | `crates/engine/src/engine_bridge/protocol/handlers/` | Wave 2 adds uuid lookups via NEW functions, doesn't modify existing |
| Audit log + history panel | `crates/engine/src/stream/` | Stream events stay entity-bits-keyed in Wave 2 — uuid migration of audit log is Wave 3 |
| The drain_slint_actions function body | `crates/engine/src/ui/slint_ui.rs::drain_slint_actions` | Signature change requires the LOOP-5 assertion to pass first |

---

## DO-NOT-BREAK Invariants → Verification Gates

Every merge passes through these:

| Invariant | Check | Run by |
|---|---|---|
| `cargo check --workspace` clean | no new errors, no new warnings vs main | Integrator (pre-merge) |
| Touched-crate tests pass | `cargo test -p <crate>` for every crate in OWNS | Integrator |
| No Bevy resource validation failures | grep engine log for "failed validation: Resource does not exist" → empty | Integrator (post-launch) |
| No drain regression | grep log for "drain_slint_actions failed" → empty | Integrator |
| Existing scenes load | engine boots Universe1/Space1 with ≥ 600 entities present | Integrator (bridge query) |
| Slint UI callbacks work | 4-click test: Explorer ▶ / tag × / add tag / F2 | **User** (manual) |
| FPS no worse than baseline | benchmark space ≥ current FPS reading (currently 2 FPS in MindMap — that's the floor) | Integrator (bridge query) |
| TOML round-trip preserves custom sections | edit one entity, save, diff TOML = only the edit | Integrator (automated) |
| Path-keyed lookups still work | MCP `find_entity --path "..."` returns entity | Integrator |
| UUID-keyed lookups work (post Wave 2.1) | MCP `find_entity --uuid "..."` returns same entity | Integrator |

Any gate failure → reject the worker's output → re-dispatch with the error log attached.

---

## 6-Agent Parallel Ceiling

Strict. The orchestrator enforces it programmatically, not via human discipline:

```
if dispatched_agents_running >= 6:
    queue_dispatch(task) and wait
```

Rationale:
- The user's machine: ~6 concurrent rustc/cargo processes before disk thrashing
- Orchestrator's review work (merging worktrees + running tests) becomes bottleneck above 6
- Most parallel batches naturally have ≤ 6 disjoint work items

---

## Dry-Run Mode

Before merging, the orchestrator can request a worker to re-export its diff as a `.patch` file without committing. This lets the orchestrator:
- Simulate the merge against current main
- Detect conflicts with other in-flight workers
- Validate file-ownership compliance machine-readably

Dry-run is opt-in per dispatch; the orchestrator turns it on for risky merges.

---

## Rollback Protocol

| Failure scope | Response |
|---|---|
| Single task's commit broke something | `git revert <commit>`; re-dispatch with the issue |
| A whole wave broke verification | `git revert <commit>..<commit>`; bisect; surface the breaking task |
| Production engine session crashes during user testing | User reports; orchestrator immediately reverts the last commit; engine restart |
| Catastrophic Fjall corruption | Restore `world.fjalldb.bak-*` backup (already present per the live `Space1/`); migration's resume-from-checkpoint absorbs the work loss |

---

## TaskList as Shared Memory

The TaskList is the kanban board visible to every agent + the user:

```
status: pending     — claimable
status: in_progress — owned by a running agent
status: completed   — merged + verified
status: deleted     — withdrawn (rare)
```

Workers check the list before starting (defensive — orchestrator should already have routed them around conflicts). Orchestrator updates status as the lifecycle progresses.

---

## Continuous Improvement Loop

After each wave lands, the orchestrator updates this document with:
- What worked: dispatch patterns that finished cleanly
- What didn't: tasks that needed re-dispatch, ownership ambiguities, spec gaps
- What changes: new loops to break, new gates to add, prompt template refinements

This is the meta-feedback loop — the dispatch process itself is a stock that improves each wave.
