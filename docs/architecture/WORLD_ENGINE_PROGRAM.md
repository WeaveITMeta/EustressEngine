# Eustress World-Engine Program

**North star.** The persistent, deterministic, Morton-indexed Fjall WorldDB is *one substrate seen two ways*: the **world model** an AI reasons over **and** the **document** the studio edits. One change-stream serves both undo/replay; one spatial index serves both render and query; one hero/cold split serves both edit-a-planet and model-resolution. We build toward an engine that beats Roblox (capped instance budget, Lua-only, no world model), Unreal (RAM scene + edit→build + no native AI/world-model), and Unity (DOTS still materializes everything) on throughput, render distance, and AI-native creation.

This program is **measure-first and increment-based**: every step is shippable + reversible in isolation and gated on a real metric from the existing profilers (`EUSTRESS_PROFILE` → `eustress_profile_phases.txt`; `--features profiling` inferno; `space/load_phase.rs` LOAD-PHASE timings; bridge `inspect_scene` FPS).

## Ground-truth corrections (verified against the code — read first)
1. **`render_cascade` is hard-filtered `With<StreamingInstanceRef>`** (`render_cascade.rs:384`); `spawn_binary_core` inserts no `RenderTier` (`residency.rs:14-16` self-documents this). Vehicle Simulator is ≈99% `BinaryEcsInstance` → **any LOD/visibility win must target binary entities**, or it measures ~0. This reshapes pillar P1.
2. **The cold `ArchInstanceCore` is already packed POD** (`worlddb/src/rkyv_values.rs:228`). The fat type is the **hero `BasePart` (~260 B)** (`classes.rs:1161`). So the compression win is **hero-pack only**.
3. **The substrate bottleneck is concurrency, not compile time** — 15 agent worktrees share one `target/`. This raises P4 (crate decomposition) leverage.
4. The reconcile 57 s was **re-reading 161K TOMLs every open** (fixed → mtime-gate, 57 s → ~23 s); the residual is the **serial directory walk**.
5. **Eager-spawn is WORK-bound** (~37 s resolving entity components/meshes) — bursting the spawn budget did nothing; the lever is **parallel bake (P3) + spawning fewer (P2)**.

## The seven pillars
- **P1 — GPU-driven virtualized render.** Persistent per-cell instance buffers, GPU compute cull (frustum + Hi-Z), indirect multi-draw, per-cell LOD pyramid baked into the DB (near hero / mid merged-instanced / far impostor / horizon card). Draw cost scales with screen complexity, not entity count. *Not Nanite* — domain-fit for millions of simple primitives.
- **P2 — Two-tier entities.** HERO (near, interactive, full ECS, packed `BasePart` ~256→~16 B) vs COLD (never an ECS entity — packed POD in DB + GPU buffers, bulk-drawn, promoted on interaction). Live ECS stays in the low tens of thousands at any world size.
- **P3 — DB-as-spatial-index load.** No runtime filesystem walk (TOML → import/export only); spatial index returns camera-local cells instantly; parallel rkyv-decode + parallel component-bake off-main-thread; render-first (<1 s to pixels).
- **P4 — Substrate.** Decompose the monolithic engine crate (render/sim/streaming/persistence) for parallel compile + clean boundaries; hot loop off-main-thread; bit-deterministic fixed-timestep sim.
- **P5 — World-model layer.** Determinism → forward-simulate + branch counterfactuals via the change-stream; `embedvec` semantic spatial query; AI perception-action API (ARC-AGI-3) over the persistent DB.
- **P6 — Studio/collab.** Editor scales to a planet via two-tier streaming; AI-as-first-class co-author (the bridge driving tools live); change-stream → per-event undo + git-style world history + Morton-locality real-time multi-user co-edit; git-friendly TOML export.
- **Unification** — the same deterministic Morton-indexed DB is the world model *and* the editor's document.

## Sequenced roadmap (gates are real profiler metrics)
- **M0 — Diagnostic truth (~1 day, no risk; GATE for everything).** Profile VS @ 350m/150m; log live entity-count-by-type (`BinaryEcsInstance` vs `StreamingInstanceRef` vs total); micro-time `spawn_binary_core` (`decode` vs `arch_to_instance` vs `spawn`). *Build nothing heavy until this baseline exists.*
- **M1 — World-model spatial query (P5.1, ~2-3 days, LOW; ship first).** Build on `active_db::iter_instance_cores_in_region` (exists, line 422) + `world_to_cell` + a bridge tool. **Gate:** 100 queries around the camera, p90 < 100 ms, sorted by distance, in-radius.
- **M2 — Binary-entity cull + tier (corrected P1.1, ~5-7 days, MEDIUM; ⟸ M0).**
  - **M2a (~2 days):** in `sys_residency_evict`, despawn `BinaryEcsInstance` beyond a new `cull_radius` (env `EUSTRESS_RESIDENCY_CULL`, default = `load_radius`) instead of `evict_radius` (1.4×). **Gate (decisive):** does render+present ms drop *with* entity count? → **draw-call/CPU-bound** (proceed to batching/M3) vs **pixel-bound** (jump to GPU Hi-Z occlusion). This one measurement decides the entire P1 arc.
  - **M2b (~3-5 days):** assign `RenderTier` to binary entities at spawn; unify them under the LOD ladder (hysteresis + LRU caps).
- **M3 — GPU-driven render (P1.2-4, 6-9 weeks, HIGH; ⟸ M2 gate).** Compute frustum+Hi-Z cull → indirect multi-draw for cold cells; per-cell LOD pyramid baked at import (meshopt); impostor/horizon tiers. **Gate:** ≥30% render-ms drop on the densest cell per sub-step or stop and re-diagnose.
- **M4 — Two-tier entities (P2; ⟸ M0).**
  - **M4.1 (~1.5 days, LOW):** `version: u8` on `ArchInstanceCore` as an **append-only tail field, serde default 0** (mandatory — never required, or every VS core fails to decode) + `spawn_cap_per_boot` (env `EUSTRESS_RESIDENCY_SPAWN_CAP`). **Gate:** VS opens identically on the OLD binary before & after.
  - **M4.2 (~1-2 weeks):** hero `BasePart` pack — intern `material_name`→u16, quantize color→`[u8;4]`, bitflags, drop identity `pivot_offset`. **Gate:** `Changed<…>`-sweep systems drop on inferno.
  - **M4.3 (heavier):** cold-render path (POD → GPU buffers, promote on interaction).
- **M5 — DB-as-spatial-index load (P3; ⟸ M0 spawn-cost breakdown).** Parallel decode/bake off-main-thread; replace eager file scan with DB spatial query for the camera cell; render-first.
- **M6 — Substrate: crate decomp (P4 pragmatic half; ⟸ nothing; run in PARALLEL from week 1).** Extract one clean leaf subsystem first (biggest compile-time win, lowest coupling); determinism harness (seeded fixed-timestep + replay-equality test) second.
- **M7 — Collab/undo (P6.0, ~1 day, LOW):** a `MessageReader<WorldDbCommit>` consumer (independent cursor — confirmed safe) feeding unified per-event undo/history that already works for streamed entities.

## Minimum set that is already clearly best-in-class
**M1 + M2a + M4.1 + M7 (P6.0)** — ~6-8 engineer-days, all LOW/MEDIUM risk, all reversible, each shippable + measurable on the *existing* profiler. Delivers: an AI-native persistent-world query API no incumbent has; a real FPS win on the 164K-part VS; bounded live-ECS for planet-scale editing; a unified change-stream undo/history. Everything heavier (M3 GPU, M4.3 cold-render, P4 determinism) is a force-multiplier on top of an already-differentiated base — sequenced *behind the M0/M2a measurement gates so we never sink quarters into the wrong-bound bottleneck.*

## Honest effort/risk
- **Weeks:** M1, M2(a/b), M4.1/4.2, M6 (first crate), M7.
- **Quarters:** M3 (GPU-driven), M4.3 (cold-render), P4 determinism/multiplayer, M5 full load rewrite.
- **Biggest risk:** M3 — de-risk early with the M2a measurement (don't build GPU batching if pixel-bound) and a single-cell merged-instanced spike before committing.

*Each increment is built against the inferno + LOAD-PHASE profilers already in the tree, so the program advances on measured fact, not assertion.*
