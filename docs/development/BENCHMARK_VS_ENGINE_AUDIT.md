# Benchmark vs Engine Performance Audit

## The Gap

| Metric | Benchmark (8K) | Engine (10K) | Gap |
|--------|----------------|--------------|-----|
| FPS | 5,406 | ~45 | **120x slower** |
| Stutter | None | 1-second pauses | **Unusable** |
| Draw calls | ~1 (instanced) | ~10,000 | **10,000x** |

## Root Cause Comparison Table

| Optimization | Benchmark | Engine | Impact | Fix Priority |
|---|---|---|---|---|
| **Mesh sharing** | 1 shared `Cuboid` handle for all N entities | `asset_server.load("parts/block.glb#Mesh0/Primitive0")` per entity — Bevy deduplicates by path but still resolves N times | Medium | P1 |
| **Material sharing** | 1 shared `StandardMaterial` handle — enables GPU instancing | `resolve_material()` creates per-entity materials (each entity gets unique color) — 10K unique materials = 10K draw calls, zero batching | **Critical** | P0 |
| **Unlit rendering** | `unlit: true` — skips all PBR lighting calculations (no normal matrix, no shadow sampling, no light loop) | Full PBR with roughness, metallic, reflectance, specular transmission, emissive — every fragment runs the full light evaluation | High | P1 |
| **Alpha mode** | `AlphaMode::Opaque` — opaque-only fast path, no transparency sorting | Per-entity alpha from `base_color.alpha() * (1.0 - transparency)` — triggers `AlphaMode::Blend` for any transparency > 0, forces back-to-front sorting | Medium | P2 |
| **Components per entity** | 4 components: `Transform, Mesh3d, MeshMaterial3d, Velocity` — single archetype, perfect cache coherence | 12+ components: `Transform, Mesh3d, MeshMaterial3d, Visibility, Instance, BasePart, Part, PartEntity, Attributes, Tags, InstanceFile, Name` + optional `Collider, RigidBody, MaterialProperties, ThermodynamicState, ElectrochemicalState` — archetype fragmentation, cache misses | Medium | P2 |
| **Physics** | None — zero physics plugins | `avian3d::PhysicsPlugins::default()` — full broadphase + narrowphase + solver runs every frame on ALL entities with Collider component | **Critical** | P0 |
| **Spawn method** | `commands.spawn_batch()` — single batch allocation for all N entities, one archetype move | `commands.spawn()` + `commands.entity(e).insert()` per entity in a loop — N individual spawns + N archetype moves | High | P1 |
| **File watcher** | None | `notify` recursive watcher on Space directory — fires spurious Modify events for 10K pre-existing files on startup (fixed: 5s grace period) | High | Done |
| **TOML write-back** | None | `write_instance_changes_system` — `Changed<Transform>` fires for ALL newly-spawned entities on first frame, triggering 10K `read_to_string` + 10K `fs::write` = **20K synchronous disk I/O ops on main thread** | **CRITICAL — This is the 1-second stutter** | P0 |
| **Logging** | None | `info!()` per entity spawn — 10K log lines with string formatting (fixed: downgraded to `debug!`) | Medium | Done |
| **Hot-reload** | None | File watcher re-parses and re-inserts transforms for all 10K TOMLs on startup (fixed: grace period) | High | Done |
| **Slint UI sync** | None | `sync_bevy_to_slint` runs every frame — pushes FPS, entity data, explorer tree to Slint software renderer | Medium | P2 |
| **Explorer tree** | None | Rebuilds full entity tree model every frame for Slint Explorer panel | Medium | P2 |
| **Frustum culling** | Bevy default (basic per-entity AABB) | Bevy default (same) — but 10K unique materials means culled entities still have unique draw calls for visible ones | Low | P3 |
| **GPU indirect rendering** | Bevy 0.18 `GpuPreprocessingMode::PreprocessAndCull` — automatic GPU-driven instancing when materials are shared | Same Bevy 0.18 path available BUT unique materials prevent it from merging any draw calls | **Critical** — unlocked by material dedup | P0 |
| **MoE sparse gate** | 10% active experts (Changed<Transform>), 90% dormant — 5-10x speedup on per-frame work | No MoE gate — every system iterates ALL entities every frame | High | P1 |
| **Pipelined rendering** | Not enabled (headless) | Not enabled — simulation blocks on render | Medium | P2 |

## The 3 Critical Fixes (P0)

### 1. `write_instance_changes_system` — The 1-Second Stutter

**Root cause:** Bevy marks ALL newly-inserted components as `Changed`. When `spawn_instance` creates 10K entities, `Changed<Transform>` fires for all 10K on the **very next frame**. The system then does 10K × (`fs::read_to_string` + `toml::from_str` + `toml::to_string_pretty` + `fs::write`) = **20K synchronous disk I/O operations on the main thread**.

**Fix:** Skip entities whose `Transform` hasn't actually been modified by the user. Add a `SpawnedThisSession` marker component during `spawn_instance` and filter it out in the write-back system. Only write back transforms that were modified via the gizmo/properties panel.

### 2. Material Deduplication — Enabling GPU Instancing

**Root cause:** Each of 10K entities gets a unique `Handle<StandardMaterial>` because `resolve_material` creates a new material for every call. Bevy's automatic GPU instancing requires **identical material handles** to merge draw calls. With 10K unique handles, the GPU submits 10K separate draw calls.

**Fix:** Already implemented `MaterialCacheKey` + `dedup_cache` in `material_loader.rs`. Entities with the same quantized (color, preset, transparency, reflectance) share one handle. For the benchmark grid (all same color), this collapses 10K materials → 1, enabling full GPU instancing.

### 3. Physics Broadphase — Avian3d on 10K Static Entities

**Root cause:** `avian3d::PhysicsPlugins::default()` runs a full broadphase + narrowphase + constraint solver every frame. Even with `RigidBody::Static`, Avian3d still maintains broadphase data structures and runs collision detection passes.

**Fix:** Already implemented conditional collider insertion (`can_collide` gate). For benchmark parts with `can_collide = false`, no `Collider` or `RigidBody` is added, removing them from Avian3d entirely.

## Expected Impact After P0 Fixes

| Fix | FPS Impact |
|-----|-----------|
| Write-back skip (stutter fix) | Eliminates 1-second freezes |
| Material dedup → GPU instancing | 10-50x fewer draw calls |
| Physics skip for static parts | 20-40% CPU savings |
| **Combined** | **Target: 200+ FPS at 10K** |

The benchmark achieves 5,406 FPS because it's a minimal Bevy app with 1 material, 1 mesh, no physics, no UI, no file I/O, headless rendering. The engine will never match that number because it runs 40+ plugins, but 200+ FPS at 10K is achievable by eliminating the critical bottlenecks.
