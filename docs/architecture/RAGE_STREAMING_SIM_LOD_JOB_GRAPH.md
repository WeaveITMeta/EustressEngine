# Predictive streaming, simulation LOD and the coupled frame

What Eustress takes from Rockstar's RAGE, mapped onto the code that exists.
Written 2026-11-19 for McKale's direction:

> "Eustress already has SpatialChunkGrid (DashMap + R-tree, tiers, dirty
> write-back) and velocity in InstanceBin. Learn from RAGE by adding
> predictive prefetch from velocity vectors, simulation LODs (full
> Avian/scripts near, simplified far), and a multi-threaded job graph that
> couples physics, AI and streaming as one system. Keep the ownership model;
> accumulate specialized tools the same way Rockstar did."

Status: phase 1 and the core of phase 2 are written into the live tree; the
first slice of phase 3 is written; nothing has been compiled or run yet (the
orchestrator compiles). Every new test is named `rage_*`.

Paths below are relative to `eustress/crates/` unless they start with `docs/`.
Line numbers are as of this writing.

---

## 1. What RAGE does that applies

Public descriptions of RAGE (GTA IV and V, Red Dead Redemption 2) and the
talks around it describe four habits. These are the general techniques, not
claims about Rockstar's internals.

1. **Stream for where the player will be.** Requests go out from the
   player's and the camera's velocity, a few seconds ahead, ordered by when
   each block is needed. A fast car never drives into an unloaded block,
   because the block was requested before the car got there.
2. **LOD for simulation, not only for rendering.** Peds and vehicles near
   the player run full physics and full AI; farther away they run cheaper
   models ("dummy" physics, simplified AI, fewer updates) or freeze; past
   that they are not simulated at all. The same distance and screen-size
   reasoning that picks a mesh LOD picks a simulation LOD.
3. **One job graph per frame.** Streaming, physics, AI and animation are
   jobs with dependencies on worker threads. The frame is a graph, its
   critical path is known, and I/O never waits on the main thread.
4. **Purpose-built tools for every system.** Each system ships with its own
   inspector (streaming visualisers, LOD overlays, frame timelines), and the
   tools accumulate over years. A system nobody can see into does not get
   tuned.

## 2. What Eustress already has

| RAGE habit | Eustress today | Where |
|---|---|---|
| Chunked spatial store | `SpatialChunkGrid`: `chunks: DashMap<ChunkCoord, Chunk>`, `rtree: RwLock<RTree<ChunkEntry>>`, `id_to_chunk: DashMap<InstanceId, ChunkCoord>` | `common/src/streaming/chunk_grid.rs:129-137` |
| Storage tiers | `Tier { Cold, Hot, Active }`, `InstanceRecord { tier, dirty, version, entity }` | `common/src/streaming/types.rs:98`, `:110-131` |
| Velocity per instance | `InstanceBin.velocity: f32`, a **magnitude**, not a vector (used as the MoE "is moving" gate) | `common/src/streaming/types.rs:80` |
| Hysteresis gate | `HysteresisRadiusGate` promote/demote by camera distance | `common/src/streaming/radius_gate.rs` |
| Dirty write-back | `DirtyBitFlusher` background thread | `common/src/streaming/dirty_flusher.rs` |
| Render LOD | `RenderTier` Hero/Active/Streamed with hysteresis and caps (visual only, by LOOP 3) | `common/src/streaming/render_cascade.rs` |
| Large-Space streaming | Residency: Fjall cells scanned and spawned around the camera, keep box, cull radius, mirror-before-evict | `engine/src/space/residency.rs`, chain at `engine/src/space/world_db_binary.rs:1425-1440` |
| Camera prediction | `CameraMotion` EMA velocity, `predicted_focus`, `cell_priority` (seconds to reach), course-change replan | `engine/src/space/stream_predict.rs`, used at `residency.rs:379-449` |
| Far render | HLOD: one merged proxy per far Morton cell, built on `AsyncComputeTaskPool`, hidden (never despawned) when near | `engine/src/space/hlod.rs:1-90`, `:840-942` |
| Spatial keys | Morton cells, `world_to_cell`, `cell_prefix` range scans | `worlddb/src/keys.rs:237` |
| Physics | Avian 0.7 (patched), 60 Hz fixed, 6 substeps, island sleeping | `engine/src/app_core.rs:269-272`, `eustress/.patches/avian3d-0.7.0/PATCH.md` |
| Weld groups | One hub body per group; members keep `RigidBody` but carry `RigidBodyDisabled` | `play-runtime/src/assemblies.rs:1-60` |
| Play frame | `PlayScriptSet::{Pull, Scripts, Apply, End}`; `PlayRole::{Authority, Replica}` | `common/src/play_session.rs:21`, `:167` |
| Body ownership by role | `body_for`: anchored Static; unanchored Dynamic on the Authority, Kinematic on a replica | `play-runtime/src/apply.rs:143` |
| AI | NPC humanoids (`Move`, `MoveTo`, `Jump`), Authority only | `play-runtime/src/npc.rs`, `play-runtime/src/lib.rs` `add_simulated_characters` |
| Scripts | One Luau VM per session (`PlayMlua`); Rune frame in `PlayScriptSet::Scripts` | `common/src/mlua/play/mod.rs`, `engine/src/play_mode.rs:1949` |
| Replication | Host motion lane at 30 Hz: bodies physics moved, and a reliable resting pose for each that stopped | `engine/src/net_replicate.rs:43`, `:456-496` |
| Frame stagger | `stagger(period, slot)`; only `sync_portal_markers` qualified | `common/src/utils.rs`, memory `project_frame_stagger.md` |

So Eustress already had RAGE habit 1 for the camera in the Fjall streamer,
and habit 2 for rendering. It did not have: prediction for anything other
than the order-0 camera (players, vehicles, a headless host); a pin set that
stops eviction of predicted cells; any measure of whether prediction works;
simulation LOD; an explicit frame order joining streaming, LOD, AI and
physics; or inspectors for any of it.

## 3. The ownership model

This is what "keep the ownership model" means in this code. Every new piece
below fits inside it.

**In five lines:**

1. **The record** is the truth at rest: the world database (`entities`
   partition rkyv core keyed by Morton cell and `stored_id`, plus the
   `tree` partition TOML tail), or for a disk Space the `.toml` file with
   `SpatialChunkGrid`'s `InstanceRecord` as its hot cache; Play never
   writes records (`outside_play_session`, `world_db_plugin.rs:1141`).
2. **The ECS entity's existence** belongs to exactly one streamer: residency
   for `BinaryEcsInstance`, the chunk grid's radius gate for
   `StreamingInstanceRef`, or the file loader for a small Space; edits are
   mirrored before eviction, and a selected entity is never evicted.
3. **The chunk or cell** belongs to its grid: `SpatialChunkGrid` owns its
   chunks (membership derived from position), residency's keep box owns
   which Fjall cells are resident, and HLOD owns far-cell proxies (hidden,
   never despawned).
4. **The physics body** belongs to Avian on the Authority: pose, velocities,
   sleep and islands; the body's type is decided once by role
   (`body_for`), a weld group's hub owns the group's body, and members are
   disabled.
5. **During Play the tree is the truth** on the Authority (pull writes
   physics into it, scripts write it, apply writes it to the ECS); a replica
   draws what the host sends and never simulates (Kinematic bodies, no NPC,
   no seats, no joints).

**How the new pieces respect it:**

| New piece | Owns | Never touches |
|---|---|---|
| Prefetch planner | A pin set and load *requests* | Existence: the streamer still spawns and despawns; records |
| Chunk-grid gate with prefetch | Promotes admitted chunks, holds pinned ones | Anything outside its own `StreamingInstanceRef` chunks |
| Residency observer | A report | Residency's queue, resident set or eviction (observe only) |
| Simulation LOD | `SimTier`, `SimLodApplied`, a scaled `SleepThreshold`, and a `Sleeping` it inserted | Body type, `RigidBodyDisabled`, colliders, joints, weld members, avatars, foci, replicas, records |
| `__SimTier` tree property | Written quietly on transition, on the Authority | Never dirty, never replicated, never heard by a script as a change |
| Frame timeline | Timings | Order of existing systems (markers only) |

Tiers are never persisted and never replicated. What replicates is the
effect: a Dormant body stops moving, so the motion lane sends its resting
pose once, as it does for any body that comes to rest.

## 4. System 1: predictive prefetch (phase 1, built)

### Model

A cell is **needed** while its centre is within `need_radius` of a focus
(the streamer's load ball). A focus at `p` moving at `v` reaches that
distance of a cell centre `c` at the earliest root of
`|p + v t - c| = need_radius`. `eta_within` solves it exactly
(`common/src/world_sim/prefetch.rs:127`). Every cell with an ETA inside the
lead time is predicted; the ETA is both its deadline and its priority. The
predicted region is a capsule swept along the path, sorted by arrival, then
by cell key, so the output is deterministic.

The horizon is `min(lead_time, max_lead_distance / speed)`, so a very fast
focus never requests a corridor across the world, and a stationary one
predicts only its need ball (`effective_horizon`, `prefetch.rs:117`). A box
larger than `max_candidates` cells falls back to the need ball and is
flagged `truncated`.

### Foci and velocity

Foci are every active `Camera3d` and every entity with `StreamFocus`
(`common/src/world_sim/focus.rs:23`). The play runtime tags every
`AvatarBody` (`play-runtime/src/sim_lod.rs:80`), so the host's character and
each joined player's character are foci, and a headless host with no camera
still predicts. Velocity is **measured** from each focus's own positions
(`FocusTracker`, EMA with a teleport reset), not read from `InstanceBin`:
that field is a scalar magnitude (`types.rs:80`), and widening the 48-byte
`Pod` layout would change the sidecar format. Measuring also covers kinematic
replica avatars and free cameras, which have no Avian velocity.

### Planner

`PrefetchPlanner` (`prefetch.rs:361`) folds each plan in:

- **Hysteresis.** A predicted cell stays pinned for `linger_secs` (1.5 s)
  after it leaves the plan, so heading noise never flaps it.
- **Never evict what is in use.** `is_pinned` (`prefetch.rs:480`) is what an
  evictor asks.
- **I/O budget.** A token bucket in caller units (`budget_units_per_sec`,
  `budget_burst`) plus `max_loads_per_tick`. A cell needed now is always
  admitted, up to the per-tick cap, even into debt; a predicted cell waits
  for tokens. Prefetch never starves demand.
- **Retry.** A request that has not made the cell resident within
  `retry_secs` is sent again.
- **Counters.** Hit (resident when first needed), prefetch hit (a hit an
  ahead request produced, with the lead it achieved), miss (not resident
  when needed, with how late it became resident), waste (requested ahead,
  expired never needed), deferrals by budget.

Env knobs: `EUSTRESS_PREFETCH_LEAD_SECS`, `EUSTRESS_PREFETCH_MAX_LEAD`,
`EUSTRESS_PREFETCH_BUDGET`, `EUSTRESS_PREFETCH_MAX_LOADS`.

### Wiring

- **Chunk grid (acts).** `sys_predictive_prefetch`
  (`common/src/streaming/predictive.rs:63`) runs in `WorldSimSet::Predict`,
  plans over an `XzGrid` of `chunk_size`, prices a load by the chunk's
  not-yet-active instance count (so the budget is spawns per second), and
  hands the gate two sets. `HysteresisRadiusGate::evaluate_all_with_prefetch`
  (`radius_gate.rs:224`) promotes an admitted chunk before it is in range
  (under the same active cap) and holds every pinned chunk against
  demotion. With both sets empty it is exactly the old pass (tested).
- **Residency (observes, phase 1b).** `sys_residency_prefetch_observe`
  (`engine/src/space/residency.rs:665`) runs the same predictor over an
  `XyzGrid` with residency's own camera, `CameraMotion` velocity, load
  radius and lead time, and counts hits and misses against residency's
  resident set. It discards its requests: residency's behaviour is
  unchanged. This measures the prediction that already exists before
  anything replaces it.

Important limit: for a migrated `.eustress` world the chunk grid's disk scan
is deliberately idle (`plugin.rs`, `sys_initial_disk_scan`), so on those
Spaces phase 1 acts only through what residency already does, and the
observer is the measurement. Moving residency onto the shared planner is
phase 1c (section 9).

### Found while reading: culled entities in resident cells never return

`sys_residency_evict` despawns an entity beyond the cull radius
(`residency.rs:613`, directional since the camera prediction landed) but
leaves its cell in `resident_cells`; `sys_residency_load` only queues cells
not in that set (`residency.rs:426`) and forgets a cell only when it leaves
the keep box (`residency.rs:449`). A cell whose centre is inside the 350 m
load ball holds entities out to about 570 m (a 256 m cell's half diagonal is
221 m). Those beyond the cull radius are spawned, culled on the next evict
pass, and do not come back as the camera approaches, until the cell leaves
the keep box. Behind the camera the cull is tighter (300 m), so a turn shows
the same gap. Not fixed here: it is residency's behaviour and the M2a cull
knob depends on it. Fix options are in section 11. The residency observer's
miss counter will not see it (it counts cells, not entities); the measurement
for it is in section 12.

## 5. System 2: simulation LOD (phase 2 core, built)

### Tiers

| Tier | Physics | Scripts and AI | State |
|---|---|---|---|
| **Full** | Avian as always | Every frame | Live |
| **Reduced** | Avian every step it is awake; sleeps at 4x its own `SleepThreshold`, so nearly still far bodies rest sooner | Every frame (see follow-ups) | Live |
| **Dormant** | Its island is put to sleep (`Sleeping`) | NPC AI skips an idle Dormant NPC; scripts see `__SimTier` | Pose, velocities, joints kept exactly |

### Policy (pure, `common/src/world_sim/sim_tier.rs`)

- Distance to the nearest focus, with in and out bands: Full to Reduced past
  200 m, back inside 160 m; Reduced to Dormant past 700 m, back inside 600 m
  (`classify_tier`, `:144`). Env: `EUSTRESS_SIM_LOD` (`0` off),
  `EUSTRESS_SIM_LOD_REDUCED_M`, `EUSTRESS_SIM_LOD_DORMANT_M`.
- Screen space: a body covering at least 5% of a view's half-height stays
  Full, at least 1% stays Reduced, with a 0.8x band to leave
  (`screen_fraction`, `:133`). A headless host has no view; distance rules.
- Speed gate: a body faster than 3 m/s is never Dormant. This is the
  conservative subset: nothing in motion is frozen.
- Full cap (4,000): the farthest overflow drops to Reduced; closest first,
  ties on entity bits.
- Islands: every body takes its island's best tier, so a jointed or
  touching group is never split between asleep and awake; an island with a
  `SleepingDisabled` member is never Dormant (`plan_tiers`, `:232`).
- Cadence: every 8 frames.

### Applying it to Avian (`play-runtime/src/sim_lod.rs`)

- `evaluate_sim_tiers` (`:103`): on the cadence frame, samples dynamic
  bodies (not weld members, not avatars, not foci), plans, inserts
  `SimTier` only where it changed, and writes `__SimTier` quietly into the
  tree for each transition.
- `apply_sim_tiers` (`:203`): reacts to `Changed<SimTier>`. Full restores
  the body's own threshold and lifts only a sleep LOD inserted; Reduced
  scales the threshold; Dormant scales it and inserts `Sleeping` unless the
  body is already asleep or may never sleep. `SimLodApplied` records exactly
  what was changed.
- Physics wins: when a contact or a script's push wakes a body LOD put to
  sleep, Avian removes `Sleeping`; the next pass counts it in
  `woken_by_physics` and leaves it awake.
- `release_sim_lod` (`:257`): when the session ends, LOD is turned off, or
  the app is a replica, every body is given back as it was.
- Authority only: `sim_lod_active` requires a `PlayDataModel`, LOD enabled,
  and `PlayRole::Authority` (absent counts as Authority, as in Studio).
- Registered by `PlayRuntimePlugin` (the Player, `play-runtime/src/lib.rs:126`)
  and by Studio's `PlayDataModelPlugin` (`engine/src/play_datamodel/mod.rs:162`).

NPC change (`play-runtime/src/npc.rs:120-160`): the driver writes an NPC's
velocity only when it changes. Before, it rewrote an idle NPC's zero every
frame, and Avian wakes a sleeping body on any velocity write, so idle NPCs
(and everything touching them) could never sleep. An idle Dormant NPC is
skipped entirely; a command still reaches it, because the write then changes
its velocity and wakes it.

### What this deliberately does not do, and why

- **No `RigidBodyDisabled`.** Weld groups already use it on merged members
  (`assemblies.rs`); a LOD that removed it on promotion would re-enable a
  merged part. It also switches attached joints off.
- **No Dynamic to Kinematic switch.** A kinematic body keeps its velocity
  with no gravity (it drifts), has infinite mass in joints, and the type
  change re-inserts solver bodies and wakes islands.
- **No per-body reduced step rate.** Avian steps every awake body every
  substep. Skipping steps for a body slows its physical time, which is
  wrong physics, not cheaper physics.
- **No collider removal.** LOOP 3 (`render_cascade.rs` module docs): a
  dynamic body without a collider falls through the world.
- **No freezing of movers.** See decision D1.
- **Scripts.** The Luau VM is one per session (`common/src/mlua/play/mod.rs`);
  throttling a Dormant model's `Heartbeat` connections needs a change in the
  VM's dispatch, which the mlua session owns. LOD publishes `__SimTier` so
  that change can read it; until then scripts run every frame in every tier.

## 6. System 3: the coupled job graph (phase 3)

### The frame today, made explicit

Bevy's main schedule already orders the big pieces. The first slice names
the world-simulation stages and measures all three:

```text
First            frame_open
PreUpdate        input
RunFixedMainLoop [physics_open] Avian PhysicsSchedule x 0..n at 60 Hz, 6 substeps [physics_close]
Update           [world_sim_open]
                   WorldSimSet::Predict   sys_predictive_prefetch (foci, plan, admit)
                   WorldSimSet::Stream    file changes -> radius gate -> transform sync -> index
                   WorldSimSet::Tier      tag foci -> evaluate tiers -> apply tiers
                 [world_sim_close]
                 residency chain (load -> HLOD plan -> mirror -> evict -> HLOD visibility)
                 [play_open]
                   PlayScriptSet::Pull    poses, collisions, characters, mass
                   PlayScriptSet::Scripts Luau, Rune, VehicleSeat input
                   PlayScriptSet::Apply   apply tree -> ECS, NPC AI, camera, particles
                   PlayScriptSet::End     event trim
                 [play_close]
PostUpdate       transform propagation, render extract, overlay
Last             frame_close
```

Dependencies that matter: physics for frame N runs before Update N, so
tiers and NPC velocities decided in Update N take effect in the physics of
N+1, which is the order Avian's own schedule expects. Prefetch plans before
the gate in the same frame, so an admitted chunk is promoted the frame it is
admitted.

Built (`common/src/world_sim/frame_graph.rs`): `WorldSimSet` chained;
`FrameGraphTimeline` with marker systems at each seam (`:172`); per-stage
last, EMA and max; the frame's longest stage as the critical path and its
share of the frame. Markers are wall clock at seams, so under the
multi-threaded executor a span includes whatever ran beside it: an upper
bound, which is what a budget needs.

### Where real parallelism comes from

Two designs were considered.

- **A. Bevy's executor, explicit sets, async I/O.** Systems keep running on
  Bevy's multi-threaded executor, which already runs non-conflicting
  systems in parallel on the `ComputeTaskPool`. Avian already parallelises
  inside its step (the `parallel` feature). What is serial today is I/O and
  decode on the main thread: residency calls
  `active_db::iter_instance_cores_in_region` synchronously per cell
  (`residency.rs` load loop; `active_db.rs:1297`, a process-wide
  `RwLock`, so it is safe to call from a worker). HLOD already reads the DB
  on `AsyncComputeTaskPool` (`hlod.rs:840-942`); residency should do the
  same: spawn one scan task per admitted cell, poll finished tasks at the
  top of `Predict`, spawn their cores under the spawn budget in `Stream`.
  I/O then overlaps physics and scripts.
- **B. A dedicated task graph.** A RAGE-style job system on
  `ComputeTaskPool` with explicit job nodes for broadphase, solver islands,
  AI and streaming. It would fight Avian's own schedule (Avian owns its
  `PhysicsSchedule` and its parallel loops), duplicate Bevy's executor, and
  need `World` access split by hand.

**Recommendation: A.** It keeps the ownership model (systems still own
what they own), coexists with Avian unchanged, and moves the only real
serial cost (main-thread I/O) off the critical path. B is not worth its
cost while Avian is the physics owner.

### Next slices (proposals for review, not built)

1. **Async residency scans** (as above), with a frame budget: at most
   `max_cell_loads_per_tick` scans in flight, decoded cores capped by the
   existing spawn budget. Needs residency's owner.
2. **Run conditions instead of early returns** for the world-simulation
   systems, so a cadence-skipped system costs nothing (Bevy evaluates run
   conditions every frame, but skips the system's parameter fetch).
3. **Physics-adjacent AI.** NPC velocities are written in `Apply` and read
   by the next physics step; a later slice moves NPC steering into
   `RunFixedMainLoopSystems::BeforeFixedMainLoop` so AI and physics share a
   tick on a host running more than one fixed step per frame.
4. **Hosted server budget mode.** On a headless host, the timeline drives
   the cadence: if `physics + world_sim + play` exceeds the 16.6 ms floor,
   raise LOD cadence and lower the prefetch budget before anything misses a
   tick.

## 7. How the three compose

- Prefetch decides **what exists** ahead of need; simulation LOD decides
  **how much of what exists is simulated**; the frame graph decides **when**
  each runs and shows what it costs.
- The same foci drive both. A player's character is a focus for streaming
  and for LOD; a joined player brings the world to life around them on the
  host.
- Dormant is the in-memory answer between Full and evicted: a Dormant body
  keeps its full state at a sleeping body's cost, so the streamer need not
  despawn simulated bodies during Play to save CPU. (Residency today can
  despawn a simulated body during Play if the camera leaves; with LOD the
  right order is Full, Reduced, Dormant, and only then eviction, which
  should happen only outside Play or with a saved state.)

## 8. Determinism and authority

- **One decider.** Tiers are computed only where `PlayRole::Authority`
  holds. Replicas never tier; their bodies are Kinematic and follow the
  motion lane. A Dormant body's last motion is its resting pose, which the
  host sends reliably.
- **Deterministic decisions.** The cadence counts frames, not time; the
  plan sorts by distance then entity bits; islands take the minimum tier.
  Same inputs give the same tiers. LOD changes physics outcomes relative to
  a run without LOD (a far body sleeps sooner), so compare runs with the
  same LOD config.
- **`sim.step`.** Manual fixed stepping drives `FixedMain` without `Update`,
  so tiers are frozen during stepped runs; stepping stays reproducible.
- **Joins.** A joining player becomes a focus as soon as their avatar
  carries `StreamFocus`; bodies near them return to Full within one cadence
  (8 frames, about 133 ms). Bodies were kept exactly, so nothing jumps.
- **Host migration.** Tiers are not state: a new host starts every body
  untiered (treated as Full) and computes its own. The worst case is one
  frame of full cost.
- **No focus, no change.** With no camera and no player, the evaluator
  changes nothing, so an empty server never freezes the world.

## 9. Budgets

| Item | Budget | How it is held |
|---|---|---|
| Frame | 16.6 ms at the 60 Hz server floor (hosted sessions, 5 Hz idle) | Timeline reports physics, world_sim and play; critical stage named |
| Prefetch plan | Under 0.2 ms for 8 foci | About 40 cells per focus at 256 m cells and 500 m radius; capped by `max_candidates` |
| LOD pass | Under 0.5 ms per 10,000 dynamic bodies, on 1 frame in 8 | One linear pass plus an O(n log n) cap sort only when over the cap; reported as `last_pass_ms` |
| LOD reactor | Transitions only | `Changed<SimTier>` |
| Physics | Proportional to awake bodies | Reduced and Dormant reduce awake bodies; `woken_by_physics` shows churn |
| Chunk-grid spawns | `EUSTRESS_PREFETCH_BUDGET` instances per second, 8 chunks per tick | Token bucket; demand first |
| Residency I/O | 8 cells per tick (unchanged) | Existing `max_cell_loads_per_tick` |

## 10. Risks

| Risk | Mitigation |
|---|---|
| A forced sleep of a body something near depends on | Island tiering takes the best member; physics wakes win and are counted |
| A floating object far away (frozen mid-air) | Speed gate: only near-still bodies go Dormant |
| `Sleeping` left on bodies after Stop | `release_sim_lod` restores on session end; `SimLodApplied` records exactly what changed |
| LOD toggling a weld group | Members are never sampled; the hub is |
| An idle NPC rewriting its velocity kept islands awake | Fixed (write on change) |
| Prefetch thrash at a boundary | Pins linger; requests retry only after `retry_secs` |
| Prefetch starving demand | Needed cells bypass the token check |
| A huge plan (tiny cells, huge radius) | `max_candidates` fallback, flagged `truncated` |
| Two sources double-counting the same focus velocity | Only the chunk-grid prefetch samples `FocusTracker`; LOD reads positions only |
| Set ordering cycles | `WorldSimSet` is not ordered against Play sets; markers only |
| Untested build | Every pure piece has `rage_*` tests designed to fail if the logic goes; Avian-facing tests run without Avian's plugins, so the first real check is a Play session (section 12) |

## 11. Phased plan

| Phase | Content | State |
|---|---|---|
| 1 | Predictor, planner, chunk-grid gate with prefetch, inspector | Written |
| 1b | Residency observer (hit and miss for the existing camera prediction) | Written |
| 1c | Residency on the shared planner: avatars as foci, pin set honoured by evict, and the culled-entity fix (D3) | Proposal |
| 2 | Tier policy, Avian application, NPC sleep fix, inspector | Written |
| 2b | Script throttling in the Luau VM from `__SimTier` (mlua session) | Proposal |
| 2c | Freeze far movers, or a cheap "dummy" mode for vehicles (D1) | Decision |
| 3 | `WorldSimSet`, frame timeline, critical stage | Written |
| 3b | Async residency scans on `AsyncComputeTaskPool` | Proposal |
| 3c | Hosted budget mode driven by the timeline | Proposal |

## 12. Tools, Rockstar style

Each system ships with its inspector. Built:

- **Engine bridge `sim.inspect`** (`engine/src/engine_bridge/protocol.rs:600`):
  every prefetch source (foci with velocity and horizon, predicted cells with
  ETA, centre and residency, pins, budget, counters and hit rate), LOD's
  report (counts, transitions, cap and island stats, forced sleeps, physics
  wakes, the last 64 transitions) and config, per-body tiers with names and
  positions, and the frame timeline with its critical stage.
- **MCP tool `inspect_world_sim`** (`mcp-server/src/bridge_tools.rs:602`):
  a one-paragraph summary plus the full JSON.
- **Studio overlay** (`common/src/world_sim/overlay.rs`), `EUSTRESS_SIM_OVERLAY=1`:
  predicted cells as squares on the ground (cyan resident, orange loading,
  brighter when sooner), each focus's predicted path as a line, and each
  tiered body as a sphere (green Full, yellow Reduced, blue Dormant).
- **Residency source in the same report**, so the existing camera predictor
  is measured beside the new one.

To accumulate next:

- A Studio panel (Slint) that charts the timeline and hit rate over time.
- `inspect_world_sim` filters: one cell, one entity's tier history.
- A replay of a recorded camera path through the planner offline, to tune
  lead time and budget without running Studio.
- A "why is this asleep" query: entity to tier, distance, island, and the
  rule that decided it.
- Per-cell I/O latency histograms once scans are async.

## 13. Decisions for McKale

- **D1. Far movers.** Options: (a) keep the speed gate, nothing moving is
  ever frozen (built; safest; saves less on busy worlds); (b) freeze
  everything past 700 m regardless of speed, as RAGE does with far traffic
  (largest saving; a far falling or driving body hangs until someone comes
  near); (c) build a cheap "dummy" mode for vehicles and NPCs (kinematic
  path following on a spline, rejoining physics when near; most work, best
  result).
- **D2. Script throttling.** Options: (a) scripts read `__SimTier`
  themselves (no VM change; only scripts written for it benefit); (b) the
  VM skips `Heartbeat` and `Stepped` handlers of scripts under a Dormant
  model and runs them at a quarter rate under a Reduced one (automatic;
  needs the mlua session; changes timing for those scripts).
- **D3. Residency's culled-entity gap.** Options: (a) un-mark a cell
  resident when the cull despawns any of its entities, so it re-scans when
  wanted (simple; re-scans cells straddling the cull radius on each replan);
  (b) skip spawning cores beyond the cull radius and keep the cell partially
  resident with a per-cell distance watermark (no churn; more state);
  (c) leave it (the M2a measurement knob keeps its exact meaning).
- **D4. Phase 3 shape.** Options: (a) design A, Bevy's executor plus async
  I/O (recommended); (b) design B, a dedicated job graph (large, and it
  competes with Avian's scheduling).
- **D5. Defaults.** LOD is on by default in Play with the conservative
  subset. Options: keep it on, or ship it off behind `EUSTRESS_SIM_LOD=1`
  until the measurements below are in.

## 14. Tests and measurements

Tests (all `rage_*`, not run yet):

- `cargo test -p eustress-common --lib rage_`: focus tracker (3),
  prefetch geometry and planner (11), gate with prefetch (4), tier policy
  (8), frame graph (3), inspector (1).
- `cargo test -p eustress-play-runtime --lib rage_`: Avian application (8),
  NPC velocity gate (1).
- `cargo test -p eustress-engine --lib rage_` (release only, given the debug
  link ceiling): residency cell mapping (1).

Measurements after the build:

1. Vehicle Simulator in TerrainBenchmark, Play, drive a fixed route at
   speed: `inspect_world_sim` residency source hit rate, miss latency max,
   and waste, with `EUSTRESS_RESIDENCY_LEAD_SECS` at 0, 2 and 4.
2. The same route with the overlay on: are predicted cells ahead of the
   car, and resident before it arrives.
3. A physics-heavy Space with many loose bodies: frame `physics` stage avg
   and max with `EUSTRESS_SIM_LOD=0` and `=1`; LOD counts and
   `woken_by_physics`; `last_pass_ms`.
4. NPC idle cost: a Space with 200 idle NPCs, physics stage before and after
   the write-on-change fix.
5. Timeline critical stage on Box Head and Vehicle Simulator, Studio and a
   hosted session, to choose the first phase 3b target.
6. Residency gap (D3): count `BinaryEcsInstance` within 350 m of the camera
   against the cores the DB holds in that ball, before and after a 300 m
   drive.
