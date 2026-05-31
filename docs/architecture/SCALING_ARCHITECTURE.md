# Eustress Scaling Architecture — 10M Entities · 60 FPS · Photorealistic

**Status:** Living plan. Supersedes `THE_LAST_GAME_ENGINE.md` and `ENHANCEMENT_PIPELINE.md` (both now deprecated stubs).
**Owner:** Engine core.  **Last revised:** 2026-05-30.
**Target:** A `.eustress` world holding **10,000,000 persisted entities** that renders at **≥60 FPS (16.6 ms)** with a **photorealistic** look on a single high-end GPU.

---

## 0. Read this first — the one idea the old docs got wrong

The previous plans (`THE_LAST_GAME_ENGINE.md`, `ENHANCEMENT_PIPELINE.md`) described a Python client/server that turned RON-scene primitives into photoreal assets over HTTP with FLUX + TripoSR. None of that is how the engine works today, and more importantly **it does not address the actual hard problem.** Turning a cube into a pretty temple is an *asset* problem. Drawing ten million of anything at 60 FPS is a *systems* problem, and the two are solved in completely different places in the frame.

The single load-bearing realization:

> **"10M entities" is a persistence claim, never a live-ECS claim.**

Iterating 10M Bevy ECS entities once per frame, at an optimistic **5 ns each**, costs **50 ms** — that is **20 FPS before a single triangle is drawn or a single collider is stepped.** There is no renderer, no GPU, and no amount of instancing that buys this back, because the cost is paid on the CPU in ECS iteration and scheduling before rendering even begins.

Therefore the architecture is, end to end:

| Layer | Population | Lives where | Touched per frame? |
|---|---|---|---|
| **Persisted** | 10,000,000 | Fjall `entities_uuid` partition (rkyv cores, Morton-keyed) | No |
| **Resident (warm)** | ~250–500K | RAM core cache near camera | Indexed, not iterated |
| **Live ECS (active)** | **≤ ~100K** | Bevy World (Transform + render/physics) | Yes |
| **Drawn** | whatever the GPU culls to | GPU indirect draw buffers | On GPU |

Every subsystem below exists to defend that table. The data layer's job is to keep **live ECS ≤ ~100K** while 10M sit on disk. The renderer's job is to draw that live set photorealistically and let the **GPU** decide what's actually visible. TOML's job is to stay out of the hot path entirely.

---

## 0.5 Design constraints (non-negotiable)

Two invariants gate every phase below. They are not goals to trade off against performance — they are the shape performance work must take.

### C1 — Default insert uses the most scalable representation

Every entity-creation surface (Insert menu, Model ribbon, Toolbox, MCP `create_entity`, paste, drag-drop import) must **default to the scalable representation**, not the filesystem one.

The policy already exists and is correct: [`representation_for_part`](eustress/crates/engine/src/space/representation.rs) classifies a bare Part / primitive (no attached file, primitive `parts/*.glb` mesh) as **`BinaryEcs`** (rkyv `ArchInstanceCore` in Fjall, scalable to millions) and only routes file-natured classes, custom/relative meshes, or folders with attached artifacts to **`FileSystem`** (TOML). It is pure, unit-tested, and carries the V-Cell guard (`mesh_requires_filesystem`) that keeps custom meshes off the binary path.

**What is missing is the wiring.** That module's own doc states the routing *"only honors `BinaryEcs` once the K2 codec swap and the entities-partition load path are wired — until then everything stays FileSystem (current behavior)."* Today the canonical [`create_instance`](eustress/crates/common/src/instance_create.rs) unconditionally writes *"exactly one folder + one `_instance.toml`."* So **100% of inserts currently produce the non-scalable form.** Closing this — the long-deferred "create-flip" — is **P1, first-class**, not a later optimization.

The flip is risky precisely because the TOML create path is load-bearing and well-tested. It must preserve, verified against a binary-created part:
- **Identity**: mint the UUID (`fresh_uuid_for_create`) and populate `entities_uuid`, `path_to_uuid`, `uuid_to_path`, `class_index` — a binary part must be findable/selectable/undoable exactly like a TOML one.
- **Surfaces**: Explorer tree, Properties panel, selection, gizmos, Cut/Dup/Group, and undo must behave identically (entities carry a `BinaryEcsInstance` marker; surfaces must not assume a disk folder exists).
- **Promotion**: the instant a binary part gains a file artifact (drop a `.pptx`/custom mesh in), it promotes to FileSystem — the router's documented `BinaryEcs → FileSystem` path. Demotion (last artifact removed) folds it back. Both must be real, not "architecture only."
- **No silent custom-mesh leak**: `mesh_requires_filesystem` must gate the create site so a V-Cell-style custom mesh never lands in the entities partition (the exact failure mode that lost the V-Cell mesh before).

Bulk surfaces (Roblox import, procedural spawners) likewise write rkyv cores directly — never N TOML files — as the benchmark generator already does.

### C2 — The material system must not break

[`material_sync.rs`](eustress/crates/engine/src/material_sync.rs) is the authoritative visual-property pipeline and must keep producing **pixel-identical** results. Its semantics, which any scaling refactor must reproduce exactly:
- registry-material lookup by `material_name` (custom `.mat.toml`) then enum preset; texture-Repeat patching (seam fix);
- `color`+`transparency` → `base_color` alpha + `AlphaMode::Blend`; `reflectance` → boosted `metallic` and reduced `perceptual_roughness`; `Neon` → emissive; `Glass` → specular/diffuse transmission + IOR;
- per-axis `uv_transform` tiling from `BasePart.size` (and `texture_repeat` override);
- `cast_shadow`/transparency-driven `NotShadowCaster` opt-out — a first-class perf knob at 50K+ parts.

**The tension, stated plainly:** sync currently does `materials.add(cloned)` — **one unique `StandardMaterial` (and bind group) per entity.** That is correctness-fine but is exactly the cost the scaling plan must remove. So C1 and C2 are coupled: defaulting inserts to `BinaryEcs` (C1) only pays off if the material path is **instanced** (bindless / GPU material-index — §5 A-render-1/2). The refactor:
1. keeps `BasePart` as the authoring source of truth and reuses sync's tint math **as the producer of a small per-instance material-params record**, uploaded to a GPU buffer/array — *not* a cloned per-entity `StandardMaterial`;
2. keeps registry base materials (textures) as the bindless texture set;
3. ships **additively behind a dual path** and is only allowed to replace the clone-per-entity path after a **visual-parity gate** (reference renders match the current pipeline). Until parity is proven, the existing material system stays the renderer.

Binary-created parts already satisfy correctness today (they carry `BasePart` + `MeshMaterial3d`, so sync runs unchanged) — verify the rkyv round-trip preserves every `BasePart` material field (`material`, `material_name`, `color`, `reflectance`, `transparency`, `texture_repeat`, `cast_shadow`) as part of C1's acceptance.

---

## 1. Frame budget (the contract everything is measured against)

16.6 ms total. A defensible allocation at 60 FPS with ~100K live entities:

| Slice | Budget | Owner |
|---|---|---|
| ECS scheduling + change detection over live set | 2.0 ms | binary-ECS, streaming |
| Streaming: spatial query + spawn/despawn batch | 1.5 ms (amortized, off-thread feeds it) | WorldDb streaming |
| Physics (Avian, active-tier only) | 3.0 ms | physics LOD |
| GPU-driven cull + indirect draw build | 1.5 ms (mostly GPU) | render Track A |
| Shadow passes (capped casters) | 2.5 ms | render |
| Main opaque + deferred lighting | 3.0 ms | render |
| GI / reflections / post (GTAO, SSR/Solari, TAA, bloom, tonemap) | 2.5 ms | photoreal stack |
| Headroom | 0.6 ms | — |

Two hard rules fall out of this table:

1. **No system may iterate all live entities more than once per frame**, and no system may touch resident-but-inactive cores per frame at all. Change-detection (`Changed<T>`) and spatial gating are mandatory, not optimizations.
2. **The DB and asset I/O never run on the render thread.** All Fjall reads/writes for streaming go through `AsyncComputeTaskPool`; results are applied as batched commands.

---

## 2. Where we are today (grounded baseline)

What actually exists, with file pointers, so this plan builds on reality rather than aspiration.

### Data layer — strong foundation, missing the streaming link
- **Fjall WorldDb**, 9 partitions: `entities`, `meta`, `tree`, `datastore`, `datastore_ord`, `entities_uuid`, `path_to_uuid`, `uuid_to_path`, `class_index` — [fjall_backend.rs](eustress/crates/worlddb/src/fjall_backend.rs).
- **Two key encoders** in [keys.rs](eustress/crates/worlddb/src/keys.rs): `FlatKeyEncoder` (12 B, **current default**) and `MortonKeyEncoder` (20 B, 21-bit-per-axis 3D Morton, `chunk_size 256.0` — **built but not the default**).
- **rkyv cores**: `ArchInstanceCore` zero-copy archives with an extensible `EusValue` tail — [rkyv_values.rs](eustress/crates/worlddb/src/rkyv_values.rs).
- **Binary-ECS load/save**: [world_db_binary.rs](eustress/crates/engine/src/space/world_db_binary.rs) — **all-or-nothing boot load** via `iter_instance_cores()`; per-frame `Changed<>` mirror back with value-gating and Morton-move delete+write.
- **`.echk` chunk bake** exists ([bake.rs](eustress/crates/worlddb/src/bake.rs)) but is **not wired to live streaming**.
- **Benchmark generator**: [generate_benchmark_map.rs](eustress/crates/engine/src/bin/generate_benchmark_map.rs) `--binary-ecs N`. Ceiling tested ≈ **2.1M** entities. Measured: DashMap insert 37 ms, R-tree radius query 9.3 ms, eviction 4.7 ms @ 2.1M.

### TOML instancing — correctly scoped, keep it that way
- `_instance.toml` + 300 ms-debounced file watcher; canonical creation through [`create_instance`](eustress/crates/common/src/instance_create.rs); representation router (`representation_for_part` / `mesh_requires_filesystem`) already splits **BinaryEcs (scale path)** vs **FileSystem (custom/special path)**.

### Rendering — stock Bevy 0.18, the scale features are absent
- **Bevy 0.18 deferred PBR**, custom `EustressAtmosphere` (Bevy's atmosphere was removed in 0.18), fog/skybox/sun-moon — [lighting_plugin.rs](eustress/crates/common/src/plugins/lighting_plugin.rs).
- **Per-instance *color* instancing** only — [instanced_pbr.rs](eustress/crates/engine/src/rendering/instanced_pbr.rs). Not geometry instancing, not GPU-driven.
- **Distance-band LOD cascade** with hysteresis + LRU caps — [render_cascade.rs](eustress/crates/common/src/streaming/render_cascade.rs). **Visibility-only reactor today**; impostors (W3.9+), physics LOD, per-tier shadow caps (W3.6) deferred.
- **Material sync** to `StandardMaterial`, per-material bind groups (not bindless) — [material_sync.rs](eustress/crates/engine/src/material_sync.rs).

### Explicit gaps (this is the work)
GPU frustum/occlusion culling · indirect draw · bindless/virtual texturing · impostors/HLOD · physics LOD · **live spatial streaming of cores from Fjall by camera position** · async DB I/O for streaming · GI/reflections/AO/TAA photoreal stack.

The good news: the *data model* (Morton + rkyv + tiered streaming types) and the *LOD policy* (render_cascade) are already designed for this. The missing pieces are mostly *wiring* and *GPU work*, not redesign.

---

## 3. Subsystem A — Data layer: stream 10M cores by camera locality

This is the keystone. Without it, nothing else matters, because the live ECS set stays unbounded.

### A1. Make Morton the default key encoding
Flip `MortonKeyEncoder` to the default for the `entities_uuid`/`INSTANCE_CORE` core store. Spatially-near entities then share key prefixes → they land in the same Fjall SSTable blocks → a camera-region query touches **few blocks, not the whole keyspace.** This is the property the LSM tree must have for streaming to be cheap.
- Keep `path_to_uuid` / `uuid_to_path` as-is (identity lookups are point-gets, order-independent).
- Bump `WorldSchemaVersion` in `header.bin`; provide a one-shot re-key migration (read FlatKey cores, rewrite Morton). Migration must be additive + verified + reversible (snapshot-before), per project convention.

### A2. Replace boot-load with a streaming residency manager
Today: `load_binary_ecs_instances` loads *everything* at boot. Replace with a **residency manager** keyed off camera position:

```
camera moves ─► compute desired resident Morton-cell set (radius R, ring expansion)
            ─► diff against currently-resident set
            ─► enqueue cell-range scans (load) and cell evictions (despawn)
                on AsyncComputeTaskPool
            ─► background: prefix-scan Morton range, rkyv-decode cores → InstanceDefinition batch
            ─► main thread: spawn_batch (load) / despawn batch (evict), capped per frame
```

- **Hysteresis on cell load/unload** (separate load radius vs keep radius) to stop thrash at boundaries — mirror the dead-zone pattern already in `render_cascade.rs`.
- **Per-frame spawn/despawn cap** (e.g. ≤ 4K spawns/frame) so a fast camera never blows the 1.5 ms streaming budget; backpressure the queue, don't drop work.
- **Bevy `spawn_batch` with pre-sized archetypes.** Per-entity `commands.spawn` at 100K is death by command-buffer overhead; batch by class so archetype moves are amortized.
- This subsumes `render_cascade`'s tiers: residency = "exists as ECS entity at all"; the existing tier cascade then governs mesh-LOD/visibility/physics *within* the resident set.

### A3. rkyv read path — kill the realign copy at scale
Currently decode copies past the 1-byte tag into a 16-byte-aligned buffer because Fjall returns unaligned bytes and the tag offsets the archive. At ~100K cores/second of streaming this is real cost.
- **Store the tag out-of-band** (or pad the value to 16-byte alignment) so `rkyv::access()` can read the archive in place, zero-copy, directly from the Fjall block cache.
- Decode on the task pool, not the main thread; hand finished `InstanceDefinition` batches across a channel.

### A4. Write path — one WriteBatch per frame
The per-frame `Changed<>` mirror must coalesce into a **single Fjall `WriteBatch`** (atomic), not N point-writes. Morton-move (position changed → delete-old + put-new) goes in the same batch so a core is never momentarily absent. Keep the existing value-gate that suppresses Avian's same-value storm.

### A5. Tune Fjall for the spatial-scan access pattern
- Size the **block cache** to hold the active region's cores (~250–500K × core size) so re-streaming a revisited area is RAM-speed.
- Tune block size so a Morton range scan reads contiguous blocks.
- Spatial locality from Morton keys minimizes read amplification across LSM levels; verify with a level-touch counter in the `active_db.rs` tally.

### A6. `.echk` vs live `entities` — settle the two roles
- **`entities_uuid` (Morton)** = the live, mutable, edited world. The streaming residency manager reads/writes here.
- **`.echk` chunks** = immutable, delta-hashed snapshots for *network/offline delivery* (R2, multiplayer cold-start, distribution). Bake from `entities`; never the live read path. Document this so they don't get conflated again.

---

## 4. Subsystem B — TOML instancing: an authoring boundary, not a runtime

This section is the storage side of constraint **C1** (§0.5): the create-flip routes scalable inserts to binary and keeps TOML a minority. The invariant to protect: **a 10M-entity space contains essentially zero `_instance.toml` files.** Ten million files would destroy any filesystem (inode pressure, directory enumeration, the watcher). TOML stays a *minority, special-case* representation.

- **BinaryEcs is the scale path** (rkyv in Fjall). **FileSystem (TOML)** is only for: custom/relative meshes, file-natured classes (SoulScript, Document, GUI), or folders with attached artifacts — exactly what `representation_for_part` already decides. Keep that router authoritative.
- **Bulk creation never writes N TOML files.** Roblox import (Wave 4), benchmark generation, procedural spawners → write rkyv cores directly (the `--binary-ecs` path), as already done in the benchmark generator. Add the same direct-to-binary path to the Roblox importer's materializer.
- **Finish the demotion path.** Promotion (add attachment → FileSystem) works; demotion (remove last attachment on a scalable class → back to BinaryEcs) is "architecture only" today. Implement it so the population can't leak permanently into the slow representation.
- **Watcher scope invariant.** The file watcher only ever covers the FileSystem minority (it already ignores `world.fjalldb/`, `.git/`, `.eustress/`). Document that it must *never* be pointed at a directory whose size scales with entity count. TOML is for hand-edits, diffs, and human-readable export — not the million-entity hot loop.

---

## 5. Subsystem C — Rendering: GPU decides what's visible

The renderer must stop assuming the CPU enumerates draws. With ≤100K live entities feeding it, even Bevy's stock per-entity visibility checks become a bottleneck, and per-material bind-group churn caps the draw rate. Two tracks, phased, with a hard cutover gate.

### Track A — Pragmatic GPU-driven (ship first, gets us to 60 FPS / near-photoreal on Bevy 0.18)

**A-render-1 · Geometry instancing + GPU material array.**
Extend the existing per-instance-color path ([instanced_pbr.rs](eustress/crates/engine/src/rendering/instanced_pbr.rs)) into **full per-instance transform + material-index** instancing. One draw per (mesh × LOD), thousands of instances each. Move material params into a GPU storage buffer indexed per instance. **This is the C2 reconciliation point**: `material_sync`'s tint math (§0.5) becomes the producer of that per-instance params record instead of `materials.add(cloned)` — built dual-path and held behind the visual-parity gate before the clone path is retired.

**A-render-2 · Bindless / virtual texturing.**
Replace per-material bind groups with a **bindless texture array** (or sparse virtual texture) so adding a unique material costs an array slot, not a pipeline state change. This is what lets 10M *distinct-looking* things share a handful of draws and bounded VRAM. (Gate on Bevy 0.18 / wgpu 27 bindless support; sparse VT is the fallback if bindless is immature.)

**A-render-3 · GPU-driven culling + indirect draw.**
Two-phase culling in compute:
1. **Frustum cull** instance AABBs on the GPU.
2. **Occlusion cull** against a **Hierarchical Z-Buffer (HZB)** built from last frame's depth — the standard Ubisoft/temporal-reprojection scheme.
Output a compacted indirect-draw buffer; the CPU issues a handful of `multi_draw_indirect` calls and never sees the per-instance visibility decision. This is the single biggest 60 FPS lever once the live set is bounded.

**A-render-4 · HLOD + impostors (finish render_cascade's Streamed/Horizon tiers).**
- **Impostors**: bake octahedral impostor atlases at asset-bake time; the Streamed tier swaps mesh → impostor billboard (the cascade already reserves `MeshLodTier::Lod3`/impostor and a billboard pipeline exists).
- **HLOD**: merge a whole distant Morton cell's meshes into one proxy mesh/material so a far city block is a single instanced draw, not thousands. This is what makes a 5 km horizon affordable.

**A-render-5 · Photoreal stack (pragmatic).**
On top of the existing deferred PBR + atmosphere: **GTAO** (ground-truth AO), **screen-space reflections** + a small set of **reflection probes** for off-screen reflection, **cascaded shadow maps with per-tier caster caps** (W3.6), **screen-space contact shadows**, **TAA**, **bloom**, **auto-exposure** (EV100 hook already present), **ACES/AgX tonemapping**, and **volumetric fog** integrated with `EustressAtmosphere`.

**A-render-6 · Physics LOD.**
Avian colliders/rigidbodies only on the Hero+Active tiers; Streamed/Horizon carry none. The cascade explicitly avoids touching `RigidBody`/`Collider` today (the "LOOP 3" breaker) — Track A is where that gate is lifted *safely*, driven by residency tier, with restore-on-promote verified.

### Track B — Virtual geometry + ray-traced GI (the photoreal ceiling, later)

Begin only after the **cutover gate** (below) is green.
- **Meshlet virtual geometry** (Nanite-style): meshopt-generated meshlet clusters with a continuous cluster-LOD DAG; GPU cluster culling replaces discrete mesh-LOD swaps. Evaluate Bevy 0.18's experimental `meshlet` feature first; extend rather than fork if viable.
- **Virtual Shadow Maps** to match meshlet density (discrete CSM won't keep up with cluster geometry).
- **Bevy Solari** (experimental ray-traced GI + reflections) as the GI path, with the Track-A screen-space stack as the non-RT-hardware fallback.

### Cutover gate (Track A → Track B)
Do **not** start Track B until **all** hold on the 2.1M-class benchmark map:
1. Live ECS set provably bounded ≤ ~100K under a sweeping camera (residency manager shipped).
2. GPU-driven cull + indirect draw shipped; CPU draw-submit < 1.5 ms.
3. ≥ 60 FPS sustained with the Track-A photoreal stack on the reference GPU.
4. Streaming hitches < 2 ms p99 under fast camera motion.

Track B raises the visual ceiling; it does **not** fix scaling. Starting it before the gate just builds beauty on a 20 FPS foundation.

---

## 6. Subsystem D — Photorealism & the asset-bake pipeline (folds in the old ENHANCEMENT_PIPELINE)

The old "enhance a primitive into a photoreal mesh at runtime over HTTP" idea is kept **only as a bake-time, offline path** — never per-frame.

- **AI asset generation moves to bake time.** Texture/mesh/material generation (the FLUX/TripoSR-class idea, or any future model) runs offline or on a background bake worker, writing into `.eustress/assets/` with the existing **SHA256 content cache** (never regenerate the same prompt). The runtime only ever *loads* finished, optimized assets.
- **The baker produces the scale-ready forms**, because runtime cannot: per-mesh **LOD chain** (meshopt simplify — the cascade already names Lod0/Lod1/Lod3), **meshlet clusters** (for Track B), **octahedral impostor atlas** (for Track A Streamed tier), **mip-complete textures** packed for the bindless/virtual-texture system, and an entry in `assets/manifest.toml`.
- **Photorealism = renderer (Section 5) + asset quality (here).** Neither alone is enough. A photoreal material on a stock-culled scene tanks FPS; GPU-driven culling on flat-shaded boxes isn't photoreal. The plan advances both in lockstep, gated by the frame budget in Section 1.
- **Quest-graph / LLM narrative** (the other half of the old doc) is orthogonal to scaling and stays a gameplay-layer concern — out of scope for this performance plan, not deleted.

---

## 7. Phased roadmap

Each phase ends with a measured gate on the benchmark generator, scaled toward 10M.

| Phase | Theme | Key work | Exit gate |
|---|---|---|---|
| **P1** | Morton + streaming residency + **create-flip (C1)** | A1 default Morton + migration; A2 residency manager; A3 zero-copy decode; A4 batched writes; **wire create sites to honor `representation_for_part` → write rkyv core for `BinaryEcs`, populate identity indices, keep promote/demote real** | Live ECS ≤ 100K under sweeping camera over a 2.1M map; no boot-time all-load; **default insert lands as binary core with full Explorer/Properties/undo/material parity, custom meshes still FileSystem** |
| **P2** | Async + Fjall tuning | A2 off-thread I/O + spawn caps; A5 block-cache/compaction tuning; A6 echk/live split documented | Streaming p99 hitch < 2 ms; revisit-region load at RAM speed |
| **P3** | GPU-driven render + **material instancing (C2)** | A-render-1 instancing; A-render-3 cull + indirect draw; A-render-6 physics LOD; **port `material_sync` tint math to per-instance material-params (dual path)** | CPU draw-submit < 1.5 ms; 60 FPS @ 2.1M; **material-instanced renders pixel-match the current `StandardMaterial` pipeline (parity gate)** |
| **P4** | Scale to 10M (persistence) | Generate + stream a 10M-core map; verify Fjall size/read-amp; raise benchmark ceiling past 2.1M | 10M persisted, 60 FPS sustained, live set bounded |
| **P5** | Photoreal pragmatic | A-render-2 bindless/VT; A-render-4 HLOD/impostors; A-render-5 photoreal stack; D baker LOD/impostor output | Near-photoreal @ 60 FPS @ 10M — **Track A cutover gate** |
| **P6** | Virtual geometry (Track B) | Meshlets, VSM, Solari GI — only after P5 gate green | Photoreal ceiling raised, gate-1..4 still hold |

---

## 8. Acceptance benchmarks

Extend [generate_benchmark_map.rs](eustress/crates/engine/src/bin/generate_benchmark_map.rs) past its current 2.1M ceiling toward 10M, and add a **moving-camera flythrough** harness (not a static snapshot — static frames hide streaming cost). Record, per phase:

- Live ECS entity count over time (must stay bounded).
- Frame time p50 / p99; streaming hitch p99.
- DB: range-scan ms, SSTable levels touched, block-cache hit rate, mirror WriteBatch ms.
- GPU: draw-call count, culled-vs-submitted ratio, VRAM.

These plug into the existing `active_db.rs` tally and stream-event telemetry so runs are comparable (`compare_runs`).

---

## 9. Top risks

1. **Bevy 0.18 / wgpu 27 maturity** for bindless, indirect draw, meshlets, Solari. *Mitigation:* Track A degrades gracefully (sparse VT instead of bindless; discrete LOD if meshlets aren't ready); Track B is explicitly gated behind Track A success.
2. **Streaming thrash / pop-in** under fast cameras. *Mitigation:* hysteresis dead-zones (proven in render_cascade), ring pre-fetch, per-frame caps with backpressure, impostor coverage at the Streamed tier so anything not-yet-resident still has *something* to draw.
3. **Migration safety** flipping to Morton keys on existing `.eustress` worlds. *Mitigation:* additive, verified, reversible re-key with snapshot-before; schema-version gate in `header.bin`.
4. **Physics LOD correctness** (the LOOP 3 hazard). *Mitigation:* tier-driven add/remove of colliders with restore-on-promote, value-gated, behind the residency manager — never ad-hoc.
5. **Asset VRAM at 10M distinct looks.** *Mitigation:* virtual texturing + HLOD proxies + impostors cap resident texel/triangle budgets regardless of persisted count.

---

## 10. The one-paragraph summary

Persist 10M entities as Morton-keyed rkyv cores in Fjall. Stream only the camera-local working set (≤~100K) into the live Bevy ECS on a background pool, batched in and out with hysteresis. Keep TOML as a small authoring/export representation that never scales with entity count. Let the GPU — not the CPU — decide what's visible via frustum + HZB-occlusion culling into indirect draws, with HLOD and impostors collapsing the distance. Bake photoreal assets (LODs, meshlets, impostors, bindless textures) offline with a SHA256 cache, and layer a GTAO/SSR/TAA/volumetrics stack now, ray-traced GI later — only after the scaling foundation is proven at 60 FPS.
