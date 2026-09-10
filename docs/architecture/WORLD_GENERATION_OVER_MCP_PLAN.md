# World Generation Over MCP: Evaluation and Plan

**Date:** 2026-08-16
**Scope:** what it takes for "ask Claude for a world" to produce a world in a
running Eustress session, starting simple and getting more detailed as budget
and score allow, in two views (Abstract and Realistic) that share one state and
one set of colliders.
**Companion docs:** `WORLD_MODEL_SIMULATOR_ROADMAP.md` (the 50 Ways this plan
sequences), `WORLD_MODEL_STATUS_2026_08_15.md` (the verified ledger this plan
starts from).

Every "exists" claim below was checked against the tree with the same test the
ledger used: does it reach a running binary. A module that compiles and is called
by nothing is recorded as **orphaned**, because that is the defect class this
codebase produces most often.

---

## 0. The promise, stated so it can fail

A user types a brief. Within one minute a blocked-out world exists in the live
engine: layout, terrain, sky, lighting, spawn point, correct classes, correct
units, colliders that work. Every further request, or every further unit of
budget, makes it more detailed along a fixed ladder: proportions and materials,
then real geometry, then appearance (textures, reflections, splats), then
physical verification. At every rung the world is a durable checkpoint the user
can stop at, share, or revert to.

Two views of the same world are always available. **Abstract** shows the state
channel: the simple geometry that is authoritative, that Avian collides with, that
raycasts hit, that scripts see. **Realistic** shows the appearance channel derived
from that state: PBR materials with real textures, mesh detail, Gaussian splats
where a captured or baked surface exists, atmosphere, reflections, and the post
stack. Realism comes from geometry, materials and light transport. There is no
neural upscaler, no image model in the render path, and no per-scene "training"
except the per-scene optimisation that 3DGS capture already is. The same world
looks better on a stronger machine because a stronger machine runs a higher
quality tier of the same deterministic pipeline, not because it draws from a
different source.

Three things must be true for this to count as done, each of which can be
observed to be false:

1. The same brief plus the same seed produces a byte-identical WorldDb digest on
   two runs (determinism, the house thesis).
2. Every rung's efficacy gates pass before its effectiveness score is even
   computed (a broken world cannot outscore a working one).
3. Raycast, collision and measurement return identical results in Abstract and
   Realistic view (appearance never leaks into state).

---

## 1. Verified starting position

### What Claude can already do over MCP today

| Capability | Tool | Evidence |
|---|---|---|
| Create Part / Model with shape, size, position, material, colour, parent, unit | `create_entity` | `tools/src/entity_tools.rs:38`; 6 primitives, 19 material presets exposed |
| Live-engine routing of entity CRUD | `entity.create/update/delete` via `BridgeEntityTool` | `mcp-server/src/shared_registry.rs:71-75`; disk TOML fallback when the engine is closed |
| Query / find / inspect the live scene, Morton-cell digest, partition for multi-agent | `inspect_scene`, `scene_overview`, `partition_scene`, `query_entities` | `bridge_tools.rs` |
| Parametric geometry | `cad_create_part`, `cad_export_glb` | `tools/src/cad_tools.rs` (truck kernel) |
| Image reference to scene, iterative generate-render-verify | `image_to_geometry` (VIGA) | `engine/src/viga/` 2,688 lines: `agent`, `generator`, `verifier`, `pipeline`, `max_iterations` |
| Observe | `capture_viewport`, `ai_camera_set_pose/orbit/frame/capture` | bridge verbs; the AI camera has no Exposure, so judged frames use `capture_viewport` |
| Sense | `scene_raycast`, `sim_step` | Phase 2 verbs |
| Provenance | `oplog_tail` | `active_db.rs:142`; `MutationActor::Mcp` exists in the schema |
| Persist | WorldDb authoritative (C1 done); `world-db` in the default tier | `engine/Cargo.toml` core tier; `instance_loader.rs:1486` |

So a blocky world can be built into the live engine by an agent **today**. That
is a real head start and it sets the shape of the plan: the first stage is not
"build a generator", it is "make the existing surface complete, batched,
schema-valid and checkpointed, then measure it".

### What exists but reaches nothing (the orphans)

| Piece | Where | Why it matters here |
|---|---|---|
| `eustress-spatial-llm` (1,515 lines): `EnvironmentGenerator` from prompts, `GeneratedContent { entities }` | `crates/spatial-llm` | Zero dependents. This is a text-to-entities generator that no binary links. |
| `GenerativeMode { Text, Image, Model, Code, Soul, Abstract }` | `engine/src/generative_pipeline.rs` | The enum is used nowhere; the plugin's `build` is empty. The Abstract mode has had a vestigial home for months. |
| `genesis::ingest`: `GenerationBackend`, `GeneratedAsset`, `IngestSource`, `AssetKind::{GaussianSplats, Mesh, Voxels, PointCloud}` | `crates/genesis/src/ingest.rs` | Linked since 2026-08-16 (the engine now depends on genesis) but uncalled. It is the vendor-agnostic backend contract Way 42 asks for. |
| Terrain worldgen: seed-deterministic coarse-to-fine (noise, hydrology, erosion, climate), seamless regions | `common/src/terrain/worldgen/` | Not reachable over MCP or the bridge. It already implements "starts simple, refines deterministically" for terrain. |
| `texture-gen`: procedural PBR maps (base colour, normal, metallic-roughness), hash-seeded | `crates/texture-gen/src/main.rs` | A standalone binary that writes 8 fixed materials at 512 px. Not a library, not parameterised, not callable at runtime. |
| `.echk` chunk exporter | `worlddb/src/bake.rs:89` | No consumer; blocks city-scale streaming (Way 35). |

### What is missing outright

| Gap | Roadmap home |
|---|---|
| Dual-channel scene contract: `LinkedState` / `LinkedAppearance`, `PrimaryRepresentation`, state-drives-appearance sync | Way 21 (status: new) |
| Any global quality tier. Only `SsrQuality` and `DeformationQuality` exist. `adapter_info` is read in `slint_bevy_adapter.rs:99` and used for nothing. | Way 32 |
| Runtime view switching. `photoreal.rs` makes every camera stage a **startup** switch because `SharedLightingPlugin` builds one `mesh_view_bind_group` layout for all `Camera3d`; a mid-session change aborts wgpu with a binding-count mismatch. | Way 32; constraint in section 8 |
| Mesh to splat synthesis. `radiance` renders imported PLY/gcloud/glTF and extracts colliders; nothing produces splats from engine geometry. | Way 21/23 conversion graph |
| `scene.measure`, `scene.observe`, `scene.affordances` | Phase 2 |
| A batch create verb. One MCP call per entity is the current cost model. | new |
| Class coverage in `create_entity`. 78 class schemas exist under `common/assets/class_schema/`; the tool handles Part and Model and passes any other class string through unvalidated. | Way 37 |
| Checkpoints. `worlddb/src/branch.rs` has a `BranchHandle` and `commit`; no checkpoint / revert verb exposes it. | Way 9 |

---

## 2. Architecture: one state, two projections, one collider

This is Way 21 made concrete, with the render-tier question (Way 32) attached.

```
                 brief + seed + budget
                          |
                   [ World Spec ]            durable, per-Space, class-valid
                          |
        +---------- STATE CHANNEL -----------+       authoritative; WorldDb rkyv core
        | Part/Model/CSG/Terrain heightfield |       Avian colliders live HERE
        | classes, units, transforms, tags   |       raycast / measure / scripts read HERE
        +------------------+-----------------+
                           |  deterministic derivation (seeded)
        +----------- APPEARANCE CHANNEL -----+       never authoritative
        | PBR materials + procedural textures|       QualityTier picks how much of it
        | mesh detail / HLOD proxies         |       is resident and at what resolution
        | splats: baked from state, or       |
        |   captured (.ply + PPISP)          |
        | lights, atmosphere, probes, post   |
        +------------------------------------+
                  |                    |
            Abstract view        Realistic view
       (appearance hidden;     (appearance shown at
        state drawn flat)       the active tier)
```

**Rules that make the two views honest:**

- Appearance entities carry `LinkedAppearance(state_entity)` and are excluded from
  every physics, raycast, query and measurement path. The regression test is the
  one Way 21 already names: a raycast hits the mesh proxy, never a splat.
- Abstract is not "low quality". It is the state channel drawn as-is: flat shaded
  primitives, terrain heightfield, wire for constraints and attachments. It is
  the view an agent should judge *structure* in, and it is cheap on any machine.
- Realistic is the appearance channel at the active `QualityTier`. Switching
  views toggles appearance visibility (a runtime-safe operation on entities).
  It does **not** change camera view features, which is the thing the shared
  bind-group layout forbids mid-session.
- Colliders are derived from state once and shared by both views. Splat clouds
  that arrive by capture get their collider from `radiance::extract_colliders`
  and are then bound to that collider as appearance, so a captured world and a
  built world obey the same contract.

**QualityTier** is a resource decided **before the first camera spawns** (a probe
on `adapter_info` plus a user override), so the startup-switch constraint is
respected rather than fought:

| Tier | Meshes | Textures | Splats | Lighting / post |
|---|---|---|---|---|
| T0 Abstract-only | primitives + HLOD proxies | none (flat material colour) | hidden | no post |
| T1 | LOD1 | 512 px procedural | floater-culled, budgeted | SMAA |
| T2 | full | 1024 to 2048 px | full | SMAA + bloom + GTAO + probes/SSR |
| T3 | full + CAD detail | 4096 px where authored | full, PPISP-corrected | as T2 plus RT/GI once the hardware and Bevy's raytraced lighting are proven on 0.19 |

Every tier renders the same WorldDb state. A tier is a projection, not a fork.

---

## 3. The authoring ladder (detail rises with budget)

Each rung is a durable checkpoint. The agent can stop at any rung; the user can
revert to any rung. Each rung has a hard efficacy gate that must pass before the
rung's effectiveness is scored.

| Rung | What gets authored | Existing pieces | New pieces |
|---|---|---|---|
| **A0 Spec** | A structured World Spec from the brief: intent, scale, unit, seed, regions, the class instances the world will need, constraints (must-haves, forbiddens). Stored in the Space. | class_schema (78 classes), `units`, `Dataset` class | `world.plan` verb; spec schema; schema-validated instance list |
| **A1 Blocking** | Terrain region(s) at the coarse pass, `Sky`/`Atmosphere`/`Clouds`, lighting preset, `SpawnLocation`, Models and primitive Parts for every major mass, tags | `create_entity`, worldgen (coarse), `EngineTerrainPlugin`, Sky/Atmosphere classes | `entities.create_batch`; `terrain.generate(seed, region, pass=coarse)`; spatial-llm's `EnvironmentGenerator` wired as one proposer |
| **A2 Dressing** | Materials and colours from the library, proportions, decals, point/spot lights, constraints, particle/beam emitters, seats, GUIs | 22-material library, `suggest_swap_template`, `query_material`, light classes | `create_entity` widened to all 78 classes with schema validation; rotation, transparency, reflectance, attributes |
| **A3 Geometry** | CAD features for hero objects, CSG unions, mesh-edit passes, terrain fine passes (hydrology, erosion, roads), water | `cad_create_part`, `mesh-edit`, `UnionOperation`, worldgen fine passes, `road.rs`, `water.rs` | `terrain.generate(pass=fine)`; a CSG grammar for repeated structures (colonnades, facades) |
| **A4 Appearance** | Parameterised procedural PBR textures per material and seed, reflection probes, baked splats for organic or soft surfaces, captured splats with PPISP correction where a real reference exists, HLOD proxies | `texture-gen` (as a bin), `ReflectionProbe`, `radiance`, `ppisp`, HLOD | `texture-gen` refactored into a library and exposed as `texture.generate`; `splat.bake(entity)` mesh-to-splat; `ingest_capture` (Way 24) |
| **A5 Verification** | Collision closure, settle test, structural check for load-bearing models, navigability from spawn, unit sanity, script smoke run | `sim_step`, `scene_raycast`, `GenerativeArchPlugin` FEA gate, determinism pins | `scene.measure/observe/affordances`; `world.verify`; gate results into the ledger |

The ladder is how "starts simple and gets more detailed as tokens increase"
becomes a mechanism instead of a hope: budget buys rungs, and each rung is paid
for only after the previous one is verified.

---

## 4. EEE: effectiveness, efficiency, efficacy

The score has the same shape as the generative-architecture loop that landed on
2026-08-16, and it inherits that loop's most important lesson: **an ungated
score optimises toward nonsense.** `HillClimb` drove every member to zero area
because nothing said "must stand up". A world generator with only a visual judge
will drive toward pretty captures of worlds you fall through.

**Efficacy (the gate, evaluated first, binary):**
- every instance validates against its class schema and unit contract;
- a raycast straight down from `SpawnLocation` hits state geometry within a bound;
- `sim_step(120)` from a settled start produces no NaN, no body below the floor
  plane, no tunnelling on the test probes;
- load-bearing Models tagged as structures pass the FEA gate;
- scripts attached at A2 and later compile and run one frame without error;
- Abstract and Realistic raycasts agree on the probe set.

If any gate fails, the rung's score is zero and the agent is told which gate.
No aesthetic score is computed.

**Effectiveness (judged, 0 to 1):** Claude scores a fixed set of captures
(`ai_camera_frame` on the spec's regions, plus the spawn view) against the World
Spec with a rubric: presence of must-haves, absence of forbiddens, scale
plausibility, layout intent, and at A4 and later, material and lighting
plausibility. VIGA's verifier already does this against an image; the text-brief
variant is a rubric over the spec rather than a pixel comparison.

**Efficiency (measured):** tokens consumed, wall time, and GPU ms per frame at
the active tier, per rung. The stop rule is marginal: advance a rung only while
the gain in effectiveness per unit of cost stays above a threshold the user can
set. This is what lets the same pipeline serve a two-minute sketch and an
overnight build.

**Ledger and flywheel:** a `WorldGenLedger` resource (mirroring
`GenerativeArchLedger` and `SimBindingsLedger`) keeps per-rung records: spec
hash, seed, tier, gate results, effectiveness, cost, WorldDb digest. Records go
to the Polars sidecar (Way A5/48) so the synthetic-data flywheel gets labelled
worlds, and the op-log carries `MutationActor::Mcp` for every entity the agent
touched (today the actor field is a placeholder; threading it is a Phase 1 gap
already on the ledger).

---

## 5. The three representations

**Meshes.** Primitives and Models today; CAD parts via truck; CSG unions;
mesh-edit for extrude and inset. New: a small CSG grammar for repeated
architectural structure, and HLOD proxies generated at A4 so the T0 and T1 tiers
stay cheap on large worlds. All of it is state channel, all of it collides.

**Textures.** The material library is the base. `texture-gen` becomes a library
crate (`eustress-texture-gen` with a `lib.rs`, the bin kept as a thin wrapper)
exposing `generate(material, params, seed, resolution) -> PbrMaps`. Deterministic
by construction (it already uses a hash-seeded lattice). The `texture.generate`
verb lets the agent author a material variant per brief without leaving the
deterministic path. The tier selects the resolution.

**3DGS.** Three sources, one contract:
1. **Baked from state** (`splat.bake`): sample the state mesh surface, one
   Gaussian per sample with colour from the resolved material and texture,
   scale from local sample spacing, orientation from the surface normal. This
   is deterministic, needs no images, and gives soft or organic surfaces a look
   PBR meshes struggle with. The collider is the source mesh's, unchanged.
2. **Captured** (`ingest_capture`): `.ply` in, PPISP photometric correction,
   `radiance::extract_colliders` for the state channel, provenance
   `IngestSource::Captured`. This is the path that already mostly exists.
3. **Vendor** (optional, off by default): a `GenerationBackend` impl behind the
   contract already in `genesis::ingest`, returning `GeneratedAsset` with
   `IngestSource::Vendor(name)`, then re-derived to state (Way 41/43) so it gets
   colliders and classes like everything else. This is where a model would enter,
   and only as an *asset author* with provenance, never as the renderer. A
   `synthetic_only` policy flag keeps a world entirely in-house when that matters.

---

## 6. MCP surface: what to add, and the two traps

New verbs (bridge method plus MCP tool, in the four-step pattern:
`MethodName` enum, `deserialize_method` arm, `handlers::` fn, dispatch arm):

| Verb | Purpose | Rung |
|---|---|---|
| `world.plan` | brief to World Spec (schema-validated, stored in the Space) | A0 |
| `entities.create_batch` | N instances in one call, one op-log record per instance | A1 and later |
| `terrain.generate` | expose worldgen: seed, region, pass level | A1, A3 |
| `world.checkpoint` / `world.revert` | over `worlddb::branch` | every rung |
| `texture.generate` | procedural PBR maps by material, params, seed, resolution | A4 |
| `splat.bake` | mesh to splat, bound as appearance | A4 |
| `ingest_capture` | `.ply` with PPISP correction and collider extraction | A4 |
| `scene.measure` / `scene.observe` / `scene.affordances` | the sense verbs Phase 2 still lacks; the judge needs them | A5 |
| `world.verify` | run the efficacy gates, return per-gate results | A5 |
| `world.score` | effectiveness rubric run plus ledger write | A5 |
| `view.set` (`abstract` / `realistic`) | toggle appearance visibility; runtime-safe | any |
| `quality.get` | report the active tier and why it was chosen | any |

`create_entity` itself is widened: rotation, transparency, reflectance,
attributes, and any of the 78 classes with properties validated against the
class schema before the write.

**Trap 1:** every new MCP tool must be added to
`tools/src/capability.rs::capability_of()` or it is refused at runtime. This has
already bitten once.

**Trap 2:** `tools/list` is cached at connect. A new tool needs the MCP client
to reconnect before it is visible. Live verification of any new verb starts with
a reconnect, not a retry.

---

## 7. Sequenced stages, each with an exit that can fail

**Stage 1: complete and measure the surface that exists (foundation).**
- `entities.create_batch`; `create_entity` widened with schema validation;
  `world.checkpoint` / `world.revert` over `branch.rs`; `WorldGenLedger`;
  wire `spatial-llm::EnvironmentGenerator` as the first A1 proposer behind
  `world.plan`.
- Exit: one brief, built into the **live** engine by Claude over MCP, produces a
  world of 30 or more instances; the same brief and seed replay to a
  byte-identical WorldDb digest; the efficacy gates above all pass; Claude's
  rubric scores the capture above a threshold; the whole run is in the op-log
  with `MutationActor::Mcp`. Any of those failing fails the stage.

**Stage 2: two views, one state (Way 21 plus the tier).**
- `LinkedState` / `LinkedAppearance` / `PrimaryRepresentation`; appearance
  excluded from physics, raycast, query, measure; `QualityTier` decided from a
  startup probe before camera spawn; `view.set`; `quality.get`.
- Exit: the raycast-hits-proxy-not-splat regression test; toggling views changes
  zero state bytes; the same world reports different frame time and an identical
  digest across two tiers.

**Stage 3: procedural detail (A2 to A3).**
- `terrain.generate` over worldgen at coarse and fine passes; `texture-gen` as
  a library plus `texture.generate`; the CSG grammar; A2 dressing via the widened
  class surface.
- Exit: the ladder runs A0 to A3 on one brief with per-rung cost recorded, and
  effectiveness is non-decreasing across rungs while every gate stays green.

**Stage 4: appearance (A4).**
- `splat.bake`; `ingest_capture` with PPISP; HLOD proxy generation; probes at
  T2.
- Exit: a baked splat world raycasts identically to its mesh source on the probe
  set; frame time at T1 stays within budget on the reference low-end machine;
  floater cull leaves no orphan Gaussians above a count threshold.

**Stage 5: verification and the flywheel (A5).**
- `scene.measure/observe/affordances`; `world.verify`; `world.score`; ledger to
  Polars; the structural gate via `GenerativeArchPlugin` for tagged structures.
- Exit: a deliberately broken world (floor removed) scores zero and names the
  gate; a working world scores above zero; the flywheel table has one row per
  rung per run.

**Stage 6: the top rung (research).**
- Bevy raytraced lighting on 0.19 / wgpu 29 as T3, relightable splats (Way 22),
  `.echk` streaming and impostors for city scale (Way 35), vendor backends behind
  the ingest contract with the `synthetic_only` policy.
- Exit: T3 renders the Stage 4 world with measured GI and a documented cost, or
  the stage reports why it cannot yet.

C4 (the 10,270 lines of duplicated Luau/Rune bindings) is deliberately not on
this path. Scripts enter at A2 through the existing runtimes; unifying them is
its own pass.

---

## 8. Constraints stated plainly

- **Camera stages are per-launch.** `photoreal.rs` documents why: one shared
  `mesh_view_bind_group` layout per process. The plan respects this by choosing
  the tier before camera spawn and by making Abstract/Realistic an *appearance
  visibility* toggle. Lifting the constraint (per-camera layouts, or a camera
  respawn path) is a separate render task and is not assumed anywhere above.
- **"Indistinguishable from reality" is bounded by light transport and asset
  fidelity, not by the pipeline.** Deterministic PBR plus SSR, probes, GTAO and
  atmosphere gets far. Path-traced GI is the frontier and is Stage 6. The claim
  the plan can defend is the one in section 0: the same world looks better on a
  stronger machine because it runs a higher tier of the same deterministic
  pipeline.
- **Splats from text without images do not exist as a technique.** The in-house
  route is bake-from-state. Anything richer is a vendor backend with provenance,
  and it is opt-in.
- **Token economics are real.** Per-entity MCP calls are the wrong unit for a
  200-instance world; the batch verb and `partition_scene` (multi-agent by
  Morton cell) are on Stage 1 for that reason.
- **The efficacy gates are the product.** The temptation will be to weaken a gate
  that keeps failing. The generative-architecture loop shows exactly what an
  ungated score optimises toward.
- **The tree is currently broken by concurrent work**
  (`realism/deformation/systems.rs:412`, a missing `mut`, edited 21:30 on
  2026-08-16 by another session). Any build-verified stage waits on that or on a
  clean branch.

---

## 9. First concrete slice

Stage 1, in one session: `entities.create_batch` plus `world.checkpoint` plus
the `WorldGenLedger`, then drive one brief through Claude over MCP into the live
engine and record the three exit measurements (digest replay, gates, rubric).
That produces the first row of the flywheel and the first number for
efficiency, and it is the smallest change that makes the promise in section 0
testable rather than aspirational.
