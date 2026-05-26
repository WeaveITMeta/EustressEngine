# Render Cascade — the see-for-miles LOD ladder

> **Wave 1 SPEC ONLY.** No code in this pass.
>
> **Scope.** Define how EustressEngine renders 10km of content at 60+ FPS
> with 10M+ instances per Space. Builds on top of the existing
> chunked-streaming spec — [05_SPACE_STREAMING.md](../AUDIT/05_SPACE_STREAMING.md)
> covers chunk *delivery* (disk → RAM → ECS); this doc covers chunk
> *display* (ECS → tier-cascade → GPU).
>
> **Pass:** P1 (2026-05-26)
>
> **Not duplicated:** the hysteresis radius gate, `.echk` chunk format,
> `SpatialChunkGrid`, server dispatcher, async decode, and entity
> spawner are all the streaming spec's job. This doc reuses them and
> layers four *visual* tiers (Hero / Active / Streamed / Horizon) on
> top of the existing two *storage* tiers (Active / Hot — Cold isn't
> rendered).
>
> **Greenfield audit:** no `RenderTier` component exists in the
> workspace today. The closest existing systems are:
> 1. [`engine::mesh_optimizer::lod_switch_system`](../../eustress/crates/engine/src/mesh_optimizer.rs) —
>    per-mesh meshopt::simplify LODs picked by **screen-space-error**
>    (~1px target). Per-entity, per-frame. Useful raw material for
>    tier-2 mesh swaps; **not** a tier cascade.
> 2. [`common::realism::lod::SimLodTier`](../../eustress/crates/common/src/realism/lod.rs) —
>    per-entity **simulation** update-rate throttle (60 / 10 / 2 / 0 Hz
>    by distance). Orthogonal to rendering. Reuse for free.
> 3. [`common::terrain::lod`](../../eustress/crates/common/src/terrain/lod.rs) —
>    voxel-chunk **mesh regeneration** budget. Different pipeline.
>
> Nothing else exists. This spec is the unifying ladder.

---

## Table of contents

1. [Goal](#1-goal)
2. [Four-tier render cascade](#2-four-tier-render-cascade)
3. [Distance-band switcher system](#3-distance-band-switcher-system)
4. [Component bundles per tier](#4-component-bundles-per-tier)
5. [Auto-LOD mesh generation](#5-auto-lod-mesh-generation)
6. [Impostor billboard baker](#6-impostor-billboard-baker)
7. [Horizon panorama baker](#7-horizon-panorama-baker)
8. [Shadow-caster cap](#8-shadow-caster-cap)
9. [Origin rebasing](#9-origin-rebasing)
10. [Frustum culling](#10-frustum-culling)
11. [Occlusion culling (Phase 2)](#11-occlusion-culling-phase-2)
12. [Performance budget](#12-performance-budget)
13. [Mesh handle dedup + VRAM eviction](#13-mesh-handle-dedup--vram-eviction)
14. [Class-by-class LOD policy table](#14-class-by-class-lod-policy-table)
15. [Open questions](#15-open-questions)
16. [Risks and mitigations](#16-risks-and-mitigations)
17. [Wave 3 implementation order checklist](#17-wave-3-implementation-order-checklist)
18. [Citations and references](#18-citations-and-references)

---

## 1. Goal

> **One sentence.** A player standing in a Space sees **10 km** of
> rendered content (everything from boots on the ground out to the
> horizon haze) at a **stable 60+ FPS** on a mid-range desktop, with
> the Space carrying up to **10 million instances** on disk.

### Acceptance criteria

| # | Criterion | Measurement |
|--:|---|---|
| G1 | 60 fps minimum on RTX 3060 / 16 GB / Ryzen 5 5600 | `frametime_p99 ≤ 16.6 ms` over a 60 s flight path |
| G2 | 10 M instances per Space loadable from disk without OOM | `peak_rss ≤ 4 GB` during cold-load |
| G3 | 10 km draw distance, no frustum/fog hard wall | first-person panorama at 10 km altitude shows all four tiers |
| G4 | < 200 ms tier transition latency (no visible "pop" beyond LOD swap) | screenshot diff at 100 m / 500 m / 5 km boundaries |
| G5 | Mobile derate: same Space at 30 FPS, all tiers honored | Snapdragon 8 Gen 2 reference, see [15_MOBILE_PLATFORM](../AUDIT/15_MOBILE_PLATFORM.md) |

### Why four tiers (not three, not five)

Three tiers (Hero / Mid / Far) leaves a 5km-to-infinity gap that
either fogs out (cheating) or pays for far-field geometry every frame.
Five tiers add a switch you can't see — the human visual system stops
distinguishing detail roughly every log-decade of distance (0.1×,
1×, 10×, 100× near). The four chosen bands map to:

```
  0–100 m    ── reach / weapons / hands / interactables    (HERO)
  100–500 m  ── streetscape / encounter / NPCs             (ACTIVE)
  500 m–5 km ── landscape / "what's over there"            (STREAMED)
  5 km+      ── horizon / sky / weather                    (HORIZON)
```

This matches the player's three attention zones (action, awareness,
ambience) plus the irreducible far-field layer.

### Non-goals

- Sub-pixel anti-aliasing (handled by TAA elsewhere)
- VR / split-screen specifics (each eye runs its own tier assignment — see Risk R3)
- Procedural-detail generation (covered by [07_AI_PLATFORM](../AUDIT/07_AI_PLATFORM.md))
- LOD for non-visual data (covered by [realism::lod](../../eustress/crates/common/src/realism/lod.rs) — orthogonal)

---

## 2. Four-tier render cascade

Each entity is assigned exactly one `RenderTier` per frame based on
**camera-distance with hysteresis**. The component is a marker;
downstream systems gate behavior off it.

### Tier definitions

#### Tier 0 — Hero (0–100 m)

> The body-language zone. The player can walk up to it and touch it.

| Field | Value |
|---|---|
| Promote distance | ≤ 100 m |
| Demote distance (hysteresis) | > 120 m |
| Entity cap | 2 000 |
| Per-entity CPU budget | ≤ 4 µs |
| Per-entity GPU budget | full draw (LOD0 mesh, full shaders, shadow caster) |
| Mesh | **LOD0** (original GLB, meshopt-cache-optimised) |
| Shadows | **Yes** — counts against shadow-caster cap (§8) |
| Physics | **Full** — Avian dynamic + static colliders, per-frame solver |
| Lights | Live — `PointLight` / `SpotLight` casts shadows if enabled |
| Billboards (`BillboardGui`) | Live per-frame text/HP-bar updates |
| Animations | Per-frame skeletal update |
| Particles | Spawned + simulated at full rate |

**LRU policy.** When > `entity_cap` entities qualify for Hero, sort by
camera distance ascending; keep the closest 2 000; demote the rest
silently to Active.

#### Tier 1 — Active (100–500 m)

> The encounter zone. Visible at full motion but not interacted with.

| Field | Value |
|---|---|
| Promote distance | 100 m ≤ d ≤ 500 m |
| Demote-out distance | > 600 m (existing `evict_radius` from streaming — REUSE) |
| Demote-in distance | < 80 m (one tier-band hysteresis) |
| Entity cap | 20 000 |
| Per-entity CPU budget | ≤ 1.5 µs |
| Per-entity GPU budget | LOD1 or LOD2 mesh, single-pass PBR, **no shadow cast** |
| Mesh | **LOD1** at 100–250 m, **LOD2** at 250–500 m (per-mesh choice via meshopt-`simplify_sloppy` ratios 0.5 → 0.25) |
| Shadows | **No cast**, still receives (`NotShadowCaster` component inserted) |
| Physics | **Static colliders only** — kinematic + dynamic bodies demoted to "ghost-mode" (transform applied, no solver step) |
| Lights | **No-shadow** — `PointLight.shadows_enabled = false` always |
| Billboards | **Cached** text — re-render only on `Changed<BillboardText>` |
| Animations | 10 Hz (every 6 frames, via existing `SimLodTier::Mid`) |
| Particles | 25 % spawn rate, half-resolution simulation |

**Important.** The 500 m outer band of Active coincides with the
existing `StreamingConfig::active_radius` (§types.rs:252, default
500.0). The render-cascade promotion/demotion at this band MUST
happen *after* the streaming radius gate so we never tier-promote
an instance the streamer hasn't yet spawned. See §3.

#### Tier 2 — Streamed (500 m – 5 km)

> The landscape zone. Recognisable shapes; almost no per-frame cost.

| Field | Value |
|---|---|
| Promote distance | 500 m ≤ d ≤ 5 000 m |
| Demote-out distance | > 5 500 m |
| Demote-in distance | < 480 m |
| Entity cap | 200 000 |
| Per-entity CPU budget | ≤ 50 ns (a flat hash lookup + indirect draw command) |
| Per-entity GPU budget | LOD3 mesh **OR** impostor billboard (2 triangles, atlas texture sample) |
| Mesh | **LOD3** for buildings / large hand-authored items; **impostor billboard** (§6) for foliage / ambient props |
| Shadows | None (neither caster nor receiver) |
| Physics | **None** — no collider, no solver, no raycast hit |
| Lights | **Light probe contribution only** — bakes into the SH9 probe grid, no per-frame shader cost |
| Billboards (`BillboardGui`) | **Hidden** beyond 1 km (text unreadable; render budget waste) |
| Animations | 2 Hz (every 30 frames, `SimLodTier::Low`) |
| Particles | Replaced with a single "smoke-puff" decal at emitter position |

The Hero / Active / Streamed cascade is **per-entity**. Streamed
entities use Bevy's `RenderLayers` to be drawn to a dedicated
streamed-tier pass (so we can profile/cap them independently).

#### Tier 3 — Horizon (5 km+)

> The "what's the world beyond" zone. Pre-baked, zero per-frame cost.

| Field | Value |
|---|---|
| Promote distance | > 5 000 m |
| Demote-out distance | n/a (camera-anchored layer — see below) |
| Entity cap | **0 individual entities** — entire layer is one composited skybox |
| Per-entity CPU budget | 0 (no entities) |
| Per-entity GPU budget | ~0.2 ms total for the whole layer (1 fullscreen quad + skybox cubemap sample) |
| Mesh | **Per-chunk panorama** (§7) — pre-rendered offline, lives in `assets/horizon/<chunk_x>_<chunk_z>.ktx2` |
| Shadows | None |
| Physics | None |
| Lights | Already baked into the panorama |
| Billboards | None |
| Animations | None |
| Particles | Atmospheric haze (1 fullscreen pass) |

The Horizon layer is **not** a 3D scene — it's an inside-out skybox
re-anchored to the camera each frame. Per-chunk panoramas blend by
position (see §7 for the cross-fade math at chunk boundaries).

### Tier cascade — visual summary

```
              ─────────────────  Camera ─────────────────
                        ↓                 ↑
   distance:   0m ─── 100m ──── 500m ──── 5km ──── ∞
   tier:       HERO    ACTIVE   STREAMED  HORIZON
   ─────────────────────────────────────────────────────
   mesh LOD:   LOD0    LOD1/2   LOD3/imp   panorama
   shadows:    cast    receive  none       baked
   physics:    full    static   none       none
   lights:     shadow  no-shdw  probe      baked
   billboards: live    cached   hidden     none
   anim Hz:    60      10       2          0
   cap:        2k      20k      200k       0
   budget(ms): 8       3        1          0.5
   ─────────────────────────────────────────────────────
   hysteresis: ±0/+20m ±20/+100m ±20/+500m  one-way
```

---

## 3. Distance-band switcher system

A new Bevy system that runs on a **16-frame cadence** (≈ 4 Hz at
60 fps) and assigns `RenderTier` per entity. The cadence is the
single biggest knob: at 16 frames the worst-case lag in tier transition
is **267 ms**, well below human reaction time but cheap enough that
the system itself is invisible on the frame graph.

### Where it sits in the schedule

```
                   [Streaming radius gate]
                          ↓
                   spawns Bevy entities         (existing — sys_radius_gate)
                          ↓
                   [Distance-band switcher]     (NEW — this section)
                          ↓
                   assigns RenderTier
                          ↓
                   [Tier component reactor]     (NEW — §4 component bundles)
                          ↓
                   inserts/removes per-tier components
                          ↓
                   [Standard Bevy render]
```

The switcher runs in `Update` schedule, **chained after**
`streaming::sys_radius_gate` so we only ever tier an entity that's
been promoted to `Tier::Active` by the streamer.

### Pseudocode

```rust
// crates/common/src/streaming/render_cascade.rs  (new file)

use bevy::prelude::*;
use crate::streaming::SpatialChunkGrid;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTier {
    Hero,     // 0
    Active,   // 1
    Streamed, // 2
    Horizon,  // 3 — never on an entity; only the skybox layer
}

#[derive(Resource)]
pub struct RenderCascadeConfig {
    pub hero_in_m:       f32, // 100.0
    pub hero_out_m:      f32, // 120.0
    pub active_in_m:     f32, // 80.0   (re-entry from Streamed)
    pub active_out_m:    f32, // 600.0  (= streaming evict_radius — REUSE)
    pub streamed_in_m:   f32, // 480.0
    pub streamed_out_m:  f32, // 5500.0
    pub cadence_frames:  u32, // 16
    pub caps:            TierCaps, // 2000 / 20000 / 200000
}

#[derive(Resource, Default)]
pub struct RenderCascadeFrame(u64); // monotonic frame counter

fn sys_render_cascade(
    mut frame: ResMut<RenderCascadeFrame>,
    cfg: Res<RenderCascadeConfig>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut entities: Query<
        (Entity, &GlobalTransform, Option<&RenderTier>),
        With<crate::streaming::plugin::StreamingInstanceRef>,
    >,
    mut commands: Commands,
) {
    frame.0 = frame.0.wrapping_add(1);
    if frame.0 % cfg.cadence_frames as u64 != 0 {
        return; // cadence gate — only runs every N frames
    }

    let Some(cam_tf) = cameras.iter().next() else { return };
    let cam = cam_tf.translation();

    // First pass: compute candidate tier for every entity
    // (cheap — just a distance lookup with hysteresis)
    let mut candidates: Vec<(Entity, RenderTier, f32)> =
        Vec::with_capacity(entities.iter().len());

    for (e, tf, current) in entities.iter() {
        let d = cam.distance(tf.translation());
        let candidate = compute_tier_with_hysteresis(d, current, &cfg);
        candidates.push((e, candidate, d));
    }

    // Second pass: enforce LRU caps (closest-N wins per tier)
    enforce_caps(&mut candidates, &cfg.caps);

    // Third pass: apply tier changes (insert/remove component only on change)
    for (e, new_tier, _d) in candidates {
        commands.entity(e).insert(new_tier);
    }
}

fn compute_tier_with_hysteresis(
    d: f32,
    current: Option<&RenderTier>,
    cfg: &RenderCascadeConfig,
) -> RenderTier {
    // Hysteresis: only transition out at the "out" threshold;
    // re-enter at the "in" threshold. Prevents oscillation when a
    // stationary player is exactly at the band edge.
    match current {
        None | Some(RenderTier::Active) => {
            if d <= cfg.hero_in_m { RenderTier::Hero }
            else if d <= cfg.active_out_m { RenderTier::Active }
            else if d <= cfg.streamed_out_m { RenderTier::Streamed }
            else { RenderTier::Streamed } // streamer keeps it cold
        }
        Some(RenderTier::Hero) => {
            if d > cfg.hero_out_m { RenderTier::Active }
            else { RenderTier::Hero }
        }
        Some(RenderTier::Streamed) => {
            if d < cfg.active_in_m { RenderTier::Active }
            else if d > cfg.streamed_out_m {
                // streamer will Hot-demote at evict_radius=600, NOT here
                RenderTier::Streamed
            }
            else { RenderTier::Streamed }
        }
        Some(RenderTier::Horizon) => unreachable!(
            "Horizon is never on an entity; it's the skybox layer"
        ),
    }
}
```

### Interaction with the existing `HysteresisRadiusGate`

The streaming gate (existing) and render-cascade gate (new) are
**two layers of hysteresis on the same distance axis**:

```
                           CAMERA
                              │
   ────────────────────────────────────────────────────
   render-cascade tiers:     HERO  ACTIVE  STREAMED
   bands (m):           0───100───500────5000────────
                              │
   ────────────────────────────────────────────────────
   streaming gate tiers:    Active     Hot        Cold
   bands (m):           0──────500──────600───────2000
                              │
   ────────────────────────────────────────────────────
   storage:    ECS-spawned   ECS-spawned  RAM-cached  disk-cold
   ────────────────────────────────────────────────────
```

Three constraints:

- **C1.** A `RenderTier::Hero` or `RenderTier::Active` entity is
  always `Tier::Active` in the streamer. Render-cascade cannot promote
  what the streamer hasn't spawned.
- **C2.** A `RenderTier::Streamed` entity is `Tier::Active` in the
  streamer for the band [500m, 600m] (overlap zone where the streamer
  still has it spawned but the render-cascade is preparing to demote
  it). Beyond 600m the streamer despawns the entity entirely — the
  render-cascade never sees it.
- **C3.** For instances in the 600m-5km band, the *only* representation
  is the impostor billboard or LOD3 in **chunk-bulk** rendering — see §6.
  No per-entity Bevy ECS entity exists; the chunk is rendered as one
  instanced draw call.

In other words: render-cascade governs **per-entity** behaviour 0–600m,
and **per-chunk** behaviour 500m-5km. The 100m hysteresis overlap
between them is intentional.

### Tunables

All in `RenderCascadeConfig` (a Bevy `Resource`), loadable from
`<Space>/render_cascade.toml`:

```toml
[render_cascade]
hero_in_m       = 100.0
hero_out_m      = 120.0
active_in_m     = 80.0
active_out_m    = 600.0
streamed_in_m   = 480.0
streamed_out_m  = 5500.0
cadence_frames  = 16

[render_cascade.caps]
hero            = 2_000
active          = 20_000
streamed        = 200_000

[render_cascade.shadow]
caster_cap      = 4        # GPU shadow-map slots (§8)

[render_cascade.origin_rebase]
threshold_m     = 4_096.0  # §9
shift_quantum_m = 1_024.0
```

### Telemetry

Emit on the existing stream backbone (see [10_TELEMETRY](../AUDIT/10_TELEMETRY.md)):

```
render.cascade.tier_changed   (entity, from_tier, to_tier, distance_m)
render.cascade.cap_evict      (tier, evicted_count, cap)
render.cascade.frame_summary  (hero_count, active_count, streamed_count, ms)
```

---

## 4. Component bundles per tier

When `sys_render_cascade` assigns a tier, a reactor system inserts /
removes per-tier components on each entity. The reactor uses Bevy's
`Changed<RenderTier>` filter so it only fires on transitions.

### The four component archetypes

Bevy works best when we plan archetype membership explicitly. Each
tier corresponds to a deterministic component set:

| Component (Bevy) | Hero | Active | Streamed | Horizon |
|---|:-:|:-:|:-:|:-:|
| `Transform`, `GlobalTransform`, `Visibility` | yes | yes | yes | n/a |
| `Mesh3d<LOD0>` | yes | — | — | — |
| `Mesh3d<LOD1\|LOD2>` | — | yes | — | — |
| `Mesh3d<LOD3>` or `ImpostorQuad` | — | — | yes | — |
| `MeshMaterial3d<StandardMaterial>` | yes | yes | (impostor mat) | — |
| `bevy::light::NotShadowCaster` | — | yes | yes | — |
| `bevy::light::NotShadowReceiver` | — | — | yes | — |
| `avian3d::RigidBody::Dynamic` | yes | — | — | — |
| `avian3d::RigidBody::Static` | yes | yes | — | — |
| `avian3d::Collider` | yes | yes (static-only) | — | — |
| `bevy_render::view::NoFrustumCulling` | — | — | yes (chunk-level) | — |
| `crate::realism::SimLodTier::High` | yes | — | — | — |
| `crate::realism::SimLodTier::Mid` | — | yes | — | — |
| `crate::realism::SimLodTier::Low` | — | — | yes | — |
| `RenderLayers::layer(0)` (default) | yes | yes | — | — |
| `RenderLayers::layer(1)` (streamed pass) | — | — | yes | — |
| `RenderLayers::layer(2)` (horizon pass) | — | — | — | yes |
| `BillboardGui` enabled flag | true | true (cached) | false (< 1km) | n/a |

### The `ClassSpawner::lod_components(tier)` interface

A spawning helper that lives in `eustress_common::classes` and is
called by the streaming entity spawner when materialising an instance.
Each class implements a method that returns the bundle for the
requested tier — keeps tier-specific logic centralised per class.

> **Naming note.** The user-task description references
> `ClassSpawner::lod_components(tier)` and a "CLASS_REGISTRY.md" doc.
> No such doc exists in the workspace today
> ([`docs/classes/`](../classes/) holds `CLASS_EXTENSIBILITY.md` and a
> README — see audit at top). The class registry is currently the
> `.defaults.toml` directory + `ClassName` enum (per
> [CLASS_EXTENSIBILITY.md](../classes/CLASS_EXTENSIBILITY.md)). The
> `lod_components(tier)` API is **new** — proposed below — and lives
> alongside `ExtraSectionClaim`. **Open Q15.4** — should we keep
> `lod_components` on `ClassName` (centralised) or extend
> `ExtraSectionClaim` with an optional `lod_for(tier)` hook so
> plugins can override?

Pseudocode:

```rust
// crates/common/src/classes.rs (extension)

impl ClassName {
    pub fn lod_components(
        &self,
        tier: RenderTier,
        instance: &InstanceDefinition,
        mesh_cache: &MeshLodCache,
    ) -> Box<dyn Bundle> {
        match (self, tier) {
            // ── PARTS ──────────────────────────────────────────────
            (ClassName::Part, RenderTier::Hero)     => part_hero_bundle(instance),
            (ClassName::Part, RenderTier::Active)   => part_active_bundle(instance, mesh_cache),
            (ClassName::Part, RenderTier::Streamed) => part_streamed_bundle(instance, mesh_cache),
            (ClassName::Part, RenderTier::Horizon)  => unreachable!(),

            // ── LIGHTS ─────────────────────────────────────────────
            (ClassName::PointLight, RenderTier::Hero)     => point_light_with_shadows(instance),
            (ClassName::PointLight, RenderTier::Active)   => point_light_no_shadows(instance),
            (ClassName::PointLight, RenderTier::Streamed) => light_probe_contribution(instance), // §11
            (ClassName::PointLight, RenderTier::Horizon)  => empty_bundle(),

            // ── GUI ────────────────────────────────────────────────
            (ClassName::BillboardGui, RenderTier::Hero)     => billboard_live(instance),
            (ClassName::BillboardGui, RenderTier::Active)   => billboard_cached(instance),
            (ClassName::BillboardGui, RenderTier::Streamed) => empty_bundle(), // hidden
            (ClassName::BillboardGui, RenderTier::Horizon)  => empty_bundle(),

            // ── CONTAINERS (Folder, Model) ─────────────────────────
            // No mesh — only Transform + Visibility — same at all tiers.
            (ClassName::Folder | ClassName::Model, _) => container_bundle(instance),

            // ── FRAME (2D UI) ──────────────────────────────────────
            // Frames are ScreenGui-anchored, never world-positioned.
            // Render-cascade does not affect them.
            (ClassName::Frame | ClassName::ScreenGui, _) => unreachable!(
                "2D UI is not in the world render-cascade"
            ),

            // ── default catch-all ──────────────────────────────────
            (_, _) => default_bundle_for_tier(instance, tier),
        }
    }
}
```

### Wave-3 hooks

The tier reactor system (also new, alongside `sys_render_cascade`):

```rust
fn sys_apply_tier_change(
    cascade_cfg: Res<RenderCascadeConfig>,
    mesh_cache: Res<MeshLodCache>,
    mut commands: Commands,
    changed: Query<
        (Entity, &RenderTier, &StreamingInstanceRef),
        Changed<RenderTier>,
    >,
    // … plus a way to look up the class/instance metadata …
) {
    for (e, tier, inst_ref) in changed.iter() {
        // Look up class via inst_ref → InstanceRecord → class_name.
        let class = /* ... */;
        let instance_def = /* ... */;

        // Remove any old tier-specific components.
        commands.entity(e).remove::<TierExclusiveComponents>();

        // Add new bundle per the class's tier policy.
        commands.entity(e).insert(class.lod_components(*tier, &instance_def, &mesh_cache));
    }
}

#[derive(Component, Default)]
struct TierExclusiveComponents; // marker bundle for sweeping the old tier
```

> A cleaner Wave-3 implementation might split tier-exclusive components
> into one type-bundle per tier and use `Or<(With<HeroTier>, With<ActiveTier>, …)>`
> queries instead — that's an open design tradeoff (Q15.5).

---

## 5. Auto-LOD mesh generation

### Pipeline

```
   <ASSET>.glb   ──┐
                   ├──>  meshopt::simplify_sloppy(0.50)  ──>  <ASSET>.lod1.glb
                   ├──>  meshopt::simplify_sloppy(0.25)  ──>  <ASSET>.lod2.glb
                   ├──>  meshopt::simplify_sloppy(0.10)  ──>  <ASSET>.lod3.glb
                   └──>  impostor_baker (§6)             ──>  <ASSET>.impostor.png
                                                              + <ASSET>.impostor.toml
```

### Trigger points

| Trigger | Behaviour |
|---|---|
| Mesh imported (Studio "Import GLB" command) | Run bake synchronously, write all four variants alongside the source. Show progress. |
| Mesh modified outside Studio (file-watcher) | Schedule a background bake; old LODs serve until new ones land. |
| Manual "Rebake LODs" right-click in Asset panel | Force regeneration even if hashes match. |
| Build / publish | Verify every mesh in the manifest has matching LOD hashes; fail on missing. |

### CLI surface

```bash
eustress mesh bake <input.glb>           # one-shot bake of all LODs + impostor
eustress mesh bake <input.glb> --no-impostor
eustress mesh bake-all <dir>             # walk + bake everything missing
eustress mesh inspect <input.glb>        # show triangle counts + LOD hashes
```

### Algorithm (engine-side, offline pipeline)

```rust
fn bake_lods(input: &Path, output_dir: &Path) -> Result<LodSet> {
    let original = load_glb(input)?;
    let positions = original.attribute(POSITION)?;
    let indices = original.indices()?;

    let adapter = VertexDataAdapter::new(
        bytemuck::cast_slice(positions), 12, 0
    )?;

    let mut variants = Vec::new();
    for (ratio, label) in [(0.5, "lod1"), (0.25, "lod2"), (0.10, "lod3")] {
        let target = ((indices.len() / 3) as f32 * ratio) as usize * 3;
        let mut err = 0.0;

        // Use simplify_sloppy for ambient mesh (allows topology break).
        // Use simplify (locks borders) for hero/silhouette mesh — see Q15.6.
        let new_indices = meshopt::simplify_sloppy(
            &indices, &adapter, target, f32::MAX, Some(&mut err),
        );

        let mut variant = original.clone();
        variant.insert_indices(Indices::U32(new_indices));
        save_glb(&variant, &output_dir.join(format!("{}.glb", label)))?;
        variants.push(LodVariant { ratio, err, tri_count: target / 3 });
    }

    Ok(LodSet { source: input.to_owned(), variants })
}
```

### Manifest integration

The existing `.echk` manifest TOML (per
[CHUNKED_STORAGE.md](../development/CHUNKED_STORAGE.md)) gains LOD
hashes per mesh entry:

```toml
[meshes."assets/meshes/tree.glb"]
hash_lod0 = "blake3-of-lod0"
hash_lod1 = "blake3-of-lod1"
hash_lod2 = "blake3-of-lod2"
hash_lod3 = "blake3-of-lod3"
hash_impostor = "blake3-of-impostor.png"
tri_count_lod0 = 4_812
tri_count_lod1 = 2_406
tri_count_lod2 = 1_203
tri_count_lod3 = 481
```

Loader rule: if any LOD hash is missing, fall back to runtime
meshopt::simplify (existing `lod_switch_system` behaviour) and warn
in telemetry — never crash.

### Reuse of existing runtime code

The existing
[`engine::mesh_optimizer::lod_switch_system`](../../eustress/crates/engine/src/mesh_optimizer.rs)
already uses `meshopt::simplify` with `LockBorder` + 0.5/0.25/0.125
ratios at runtime. The offline pipeline shares the same algorithm
but writes to disk instead of `Assets<Mesh>` — refactor the inner
loop into a shared `meshopt_simplify_to_glb` helper in
`eustress-common::mesh` so both paths use one implementation.

---

## 6. Impostor billboard baker

### What an impostor is

A camera-facing **2-triangle quad** with a pre-rendered texture
showing what the object looks like from N angles. For grass tufts,
trees, rocks, and fences, an impostor at 1 km is visually
indistinguishable from LOD3 — at 1/100th the render cost.

### Atlas layout (16 angles)

```
   pitch = -15°    pitch = +15°
   ┌──┬──┬──┬──┬──┬──┬──┬──┐
   │ 0│ 1│ 2│ 3│ 4│ 5│ 6│ 7│   yaw bins (every 45°)
   ├──┼──┼──┼──┼──┼──┼──┼──┤
   │ 8│ 9│10│11│12│13│14│15│
   └──┴──┴──┴──┴──┴──┴──┴──┘

   Atlas: 4×4 grid of 256×256 px tiles → 1024×1024 atlas
   Format: KTX2 with BC7 compression (~256 KB on disk)
```

8 yaw × 2 pitch (low + high look-angle) is the empirical minimum
that hides "wrong-angle" flicker as the camera arcs around the
object. Star Citizen ships 4×3, Outerra 8×4 — 8×2 is a deliberate
compromise that ~halves VRAM vs Outerra.

### Pipeline

```
   <ASSET>.glb (already loaded)
        │
        ├──> spawn one-shot scene with:
        │      - lit by default skybox (no procedural sun yet)
        │      - camera at (radius, h, 0) orbiting in yaw+pitch
        │      - RenderTarget = 256×256 texture per shot
        │
        ├──> for each (yaw, pitch) in 8 × 2:
        │      ├──> orient camera
        │      ├──> render one frame to texture
        │      └──> blit into atlas tile
        │
        ├──> save atlas → assets/impostors/<ASSET>.png
        └──> save manifest → assets/impostors/<ASSET>.toml
```

### Atlas manifest

```toml
# assets/impostors/tree.toml

[impostor]
source_glb = "assets/meshes/tree.glb"
source_hash = "blake3..."   # invalidate if source changes

atlas_path = "tree.png"     # relative to this toml
atlas_size = [1024, 1024]
tile_size = [256, 256]
tile_grid = [4, 4]

[angles]
yaw_bins = 8                # 0, 45, 90, … 315°
pitch_bins = 2              # -15°, +15°
yaw_origin_deg = 0.0        # rotation at tile (0, 0)

[bounds]
# World-space bounds of the source mesh, used to size the quad in-world
size = [4.2, 6.8, 4.2]
pivot = [0.0, 0.0, 0.0]
```

### Render-time impostor selection

```rust
fn select_impostor_tile(
    cam_pos: Vec3,
    entity_pos: Vec3,
    manifest: &ImpostorManifest,
) -> (u32, u32) {
    let dir = (cam_pos - entity_pos).normalize();
    let yaw = dir.x.atan2(dir.z);
    let pitch = dir.y.asin();
    let yaw_bin = ((yaw / TAU * manifest.yaw_bins as f32) as u32) % manifest.yaw_bins;
    let pitch_bin = if pitch > 0.0 { 1 } else { 0 };
    (yaw_bin, pitch_bin)
}
```

Tile coordinates feed UV-offset into the impostor shader. The whole
chunk's impostors share **one draw call** via Bevy's instanced quad
pipeline + per-instance UV-offset attribute.

### Pop / flip issues

When the camera arcs at constant distance, the impostor tile changes
at each yaw boundary. Three mitigations:

- **Two-tile blend.** At yaw fraction f within a bin, sample tiles
  N and N+1, blend by f. Doubles texture fetches but kills the snap.
- **Octahedral encoding.** Pack 16 angles into a single procedural
  octahedron-mapped texture so the lookup is continuous. Defer to
  Phase 2 — more complex baker.
- **Aspect snap.** If the camera is roughly fronto-parallel and the
  yaw is changing rapidly (>30°/s), accept the snap; nobody notices
  when they're spinning.

### Trigger and on-demand re-bake

| Trigger | Behaviour |
|---|---|
| Mesh imported | Bake atlas as part of LOD pipeline (§5) |
| User toggles "needs impostor" off in Asset properties | Skip on next bake; clear file |
| Skybox/lighting baseline changes | Open question Q15.7 — re-bake everything or accept stale lighting? |
| Asset hash drifts | Auto-rebake on next access; warn |

---

## 7. Horizon panorama baker

### What it is

For everything beyond 5 km, we don't render per-object — we render
**one pre-baked panorama per chunk**, anchored to the camera each
frame. The player sees a recognisable mountain range / city skyline
from where they're standing, but pays zero per-entity cost.

### Output format

```
   assets/horizon/
   ├── chunk_0_0.ktx2        # 2048×1024 equirectangular HDR
   ├── chunk_0_0.toml        # metadata (subtraction bounds, view origin)
   ├── chunk_0_1.ktx2
   ├── chunk_0_1.toml
   └── …
```

One panorama per chunk on a coarse grid (`horizon_chunk_size`, e.g.
1024 m — 4× the streaming chunk size — so a 16km×16km world produces
256 panoramas of ~1 MB each = 256 MB total horizon budget).

### What's IN the panorama

The bake renders **everything ≥ 5 km from the chunk centre, viewed
from the chunk centre**. Specifically:

- All `RenderTier::Streamed`-capable entities at distances > 5 km
- Terrain heightmap rendered at LOD 4+
- Skybox + sun/moon/clouds at panorama-bake time of day

What's NOT in the panorama (must be subtracted before display):

- Anything within `radius_in_panorama_m` (= 4500 m, 500 m hysteresis
  with the Streamed boundary) — these objects re-render live as
  individual Streamed-tier entities
- Player/NPC characters (always live)
- Anything anchored to the player

### Bake pipeline

```
   for each <chunk_x, chunk_z> in world.coarse_grid:
       spawn_offline_world:
           - center camera at chunk centre, eye height 1.6 m
           - render world WITHOUT chunks within 4500 m of centre
           - render 6 cube faces at 1k each (or single 2k equirect)
           - HDR float32 → tonemap → BC6 compression
           - save to assets/horizon/chunk_x_z.ktx2
```

### Render time — anchoring to the camera

```rust
fn sys_horizon_pass(
    cam: Query<&GlobalTransform, With<Camera3d>>,
    horizon_cache: Res<HorizonCache>,
    horizon_chunk_grid: Res<HorizonChunkGrid>,
    // … renderer …
) {
    let cam_pos = cam.iter().next().unwrap().translation();
    let cur_chunk = HorizonChunkCoord::from_world(cam_pos);

    // Sample the 2x2 nearest panoramas and blend (bilinear-on-chunk-coord)
    let neighbors = horizon_chunk_grid.nearest_4(cur_chunk);
    let blend_weights = chunk_blend_weights(cam_pos, &neighbors);

    // Two-pass:
    //  1. Render skybox cube/sphere with this blended panorama
    //  2. Subtract per-chunk impostor / Streamed-tier draws in front
    //     (depth test handles this automatically — they have proper z)
    submit_horizon_draw(neighbors, blend_weights);
}
```

### Cross-fading at chunk boundaries

The naïve approach pops at the chunk boundary. The fix is
**bilinear blending**: sample the 4 nearest chunk panoramas weighted
by 2D distance to chunk centres. Cost: 4× texture samples in the
panorama shader, ~negligible.

### Why this is cheap

The horizon pass is **one full-screen quad with one HDR cube sample**
+ depth-write of zero. No vertex transform, no per-object iteration.
On the budget, < 0.5 ms regardless of world size.

### Trigger and re-bake

| Trigger | Behaviour |
|---|---|
| World published | Bake all horizon panoramas (slow — runs on the build farm or overnight in Studio) |
| Single chunk modified | Mark affected horizon chunks dirty (those whose centre is within 5 km of the modified streaming chunk); re-bake on idle |
| Time-of-day changed | Open Q15.8 — bake N times-of-day or sample dynamically? V1: one panorama per scene at noon TOD; sun/moon overlay is composited live |
| Sky / Atmosphere class changed | Re-bake all horizon panoramas (they bake the sky in) |

---

## 8. Shadow-caster cap

### Why

Bevy's default forward+ pipeline allocates a fixed number of shadow
maps. The current default is 4 (from `bevy_pbr::ClusteredForwardPlugin`
default config). A scene with 50 hero-tier `PointLight`s, all with
`shadows_enabled=true`, will silently fail to allocate maps for the
overflow — the shadows just don't render, with no error.

The render-cascade owns the "which N lights cast shadows this frame"
decision.

### Algorithm

```rust
fn sys_shadow_caster_cap(
    cfg: Res<RenderCascadeConfig>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    mut hero_lights: Query<
        (&GlobalTransform, &mut PointLight, &RenderTier),
        With<RenderTier>,
    >,
) {
    let cam_pos = cam.iter().next().unwrap().translation();
    let cap = cfg.shadow.caster_cap as usize;

    // 1. Gather all Hero-tier lights with shadows_enabled originally desired.
    let mut hero_shadow_lights: Vec<_> = hero_lights
        .iter()
        .filter(|(_, l, t)| **t == RenderTier::Hero && l.desired_shadows())
        .collect();

    // 2. Sort by distance ascending.
    hero_shadow_lights.sort_by(|a, b| {
        a.0.translation().distance(cam_pos)
            .partial_cmp(&b.0.translation().distance(cam_pos))
            .unwrap_or(Ordering::Equal)
    });

    // 3. Top N keep shadows; rest demoted silently.
    for (i, (_, mut light, _)) in hero_lights.iter_mut().enumerate() {
        light.shadows_enabled = i < cap;
        // also log demotion via stream backbone
    }
}
```

### Per-tier shadow policy

| Tier | Shadow caster | Shadow receiver |
|---|---|---|
| Hero | Yes (up to `caster_cap`) | Yes |
| Active | **No** (inserts `NotShadowCaster`) | Yes |
| Streamed | **No** (inserts `NotShadowCaster` + `NotShadowReceiver`) | No |
| Horizon | None (baked into panorama) | n/a |

### Per-class default `shadows_enabled`

Per [`PointLight`](../../eustress/crates/common/src/classes.rs) and
related, the default at line 6279 is `shadows_enabled: true`. The
render-cascade respects whatever the user set in TOML; it only
**demotes** when over the cap, never promotes a `shadows_enabled=false`
light to "yes".

### Telemetry

```
render.cascade.shadow_demoted   (light_entity, distance_m, rank)
```

If consistent demotion happens at the boundary, the artist needs to
raise `caster_cap` or split the light set.

### Default

`caster_cap = 4` (matches Bevy default). Configurable up to 8 on
desktop, force-clamp to 2 on mobile per
[15_MOBILE_PLATFORM](../AUDIT/15_MOBILE_PLATFORM.md).

---

## 9. Origin rebasing

### Problem

`Transform.translation` is `Vec3` (3× f32). f32 has ~7 significant
decimal digits of precision. Beyond ~4 km from origin, sub-millimetre
precision degrades into visible jitter:

| Distance from origin | Worst-case f32 precision |
|---|---|
| 1 km | 0.1 mm |
| 4 km | 0.5 mm |
| 16 km | 2 mm (visible jitter) |
| 100 km | 1 cm (unusable) |

(Source: [HYBRID_COORDINATES.md](../development/HYBRID_COORDINATES.md))

For a 10 km draw distance, the camera can comfortably reach 4 km
from origin and start hitting the wall.

### Existing infrastructure

The `HybridPosition` system from HYBRID_COORDINATES.md already
provides **DVec3 absolute + Vec3 relative** with auto-switching at
100km — but that's solar-system scale, not render-cascade scale.
It's overkill for "I'm 4km from where I started" and doesn't
trigger automatically at render-cascade distances.

### Solution: discrete origin shifts

When the camera exceeds `rebase_threshold_m` (4096 m) from the
"render origin", perform a one-shot quantised shift:

```
   shift = floor(cam_pos / shift_quantum) * shift_quantum
   for every visible-tier entity: transform.translation -= shift
   camera.translation -= shift
   render_origin += shift
   physics_world.notify_origin_shifted(shift)
   audio_world.notify_origin_shifted(shift)
   particle_system.translate_emitters(-shift)
```

`shift_quantum_m = 1024 m` so we shift in clean 1km increments —
makes debugging and replay trivial.

### When to shift

- Once per frame the cascade switcher runs (every 16 frames)
- Only between tier-switcher passes, never mid-frame
- Throttled: max 1 shift per second to prevent thrash if the player
  is exactly at the boundary

### What needs to know

| System | Receives shift via |
|---|---|
| Bevy `Transform`s of visible entities | iterating `Query<&mut Transform>` (use Bevy's existing `bevy_floating_origin` crate pattern) |
| Bevy `Camera` `Transform` | same — included in the iter |
| Avian3d physics world | `physics_world.shift_origin(shift)` — pose all rigid bodies, all colliders |
| Audio (Bevy `SpatialListener`) | event: re-anchor listener |
| Particle systems | iter emitters + active particles |
| The streaming `SpatialChunkGrid` | **NO shift** — its coordinates are world-absolute. The render-cascade subtracts `render_origin` only on display |
| Selection / picking math | reads from `render_origin`; ray cast uses local + add render_origin back |

### Inactive / asleep entities

`Tier::Hot` (in-RAM but no ECS entity) and `Tier::Cold` (disk-only)
records are NOT touched. The streamer's `bin.position` stays
world-absolute. When the streamer later promotes them to Active,
it subtracts the current `render_origin` then spawns.

### Reference

Star Citizen's "world origin shift" and Outerra's "moving origin"
both implement this. Bevy ecosystem has
`bevy_floating_origin` (third-party) as a working reference —
**Q15.10:** vendor it or roll our own integrated with Avian?

### Open: required for v1?

The naïve player movement (foot-walk) takes ~7 minutes to reach 4km
from spawn. The "show me 10 km" use case is mostly *looking*, not
*walking*. **Q15.11** — defer origin-rebase to Wave 5 (post-v1) and
ship v1 with a "spawn point ≤ 4km from any visible content"
constraint?

---

## 10. Frustum culling

### Confirm Bevy coverage

[`bevy_render::camera::Frustum`](https://docs.rs/bevy_render/latest/bevy_render/camera/struct.Frustum.html)
plus the `check_visibility` system already does:

- Per-entity `Aabb` test against six camera planes
- Sets `ViewVisibility::set(false)` for non-visible entities
- The standard `Mesh3d` rendering pipeline skips invisible entities

This is **sufficient for Hero and Active tiers** out of the box — no
custom work needed.

### Gaps for render-cascade

- **Streamed tier per-chunk culling.** A chunk's 10k impostors share
  one draw call. Bevy's per-entity AABB test happens per-impostor
  before culling decisions, which is wasteful. We need a per-chunk
  pre-filter: only run check_visibility on impostors in chunks whose
  bounding box intersects the frustum.

- **Horizon tier is camera-anchored.** Horizon never culls — the
  skybox is always behind everything. Set `NoFrustumCulling` on the
  horizon layer.

- **Billboards.** Already handled — see
  [`engine::billboard_pipeline`](../../eustress/crates/engine/src/billboard_pipeline.rs)
  line 152 inserts `NoFrustumCulling` because billboards have no
  meaningful static AABB. Reuse this pattern for impostors.

### Custom chunk-level frustum culling (Wave 3)

```rust
fn sys_chunk_frustum_cull(
    cam: Query<(&GlobalTransform, &Frustum), With<Camera3d>>,
    chunks: Query<(&ChunkBounds, &mut Visibility), With<StreamedChunkMarker>>,
) {
    let Some((_, frustum)) = cam.iter().next() else { return };
    for (bounds, mut vis) in chunks.iter_mut() {
        let visible = frustum.intersects_aabb(&bounds.aabb);
        *vis = if visible { Visibility::Inherited } else { Visibility::Hidden };
    }
}
```

A chunk-AABB miss skips all 10k impostors in that chunk in one go.

---

## 11. Occlusion culling (Phase 2)

Bevy 0.18 does not include GPU occlusion culling. Two options for
Phase 2 (post-v1):

| Option | Pros | Cons |
|---|---|---|
| **GPU occlusion queries** | Hardware-supported, conservative | One frame latency, hard to integrate with Bevy's render graph |
| **Software Hierarchical-Z** | No frame latency, predictable cost | CPU-side, doesn't scale to 1M entities |
| **Compute-shader portal/AABB** | Custom pipeline, integrates well | Significant engineering — Bevy plugin or upstream wgpu work |

### Hook point for Wave 4

Add an `OcclusionQuery` component that the renderer can pre-test
before submitting the draw. The `RenderTier::Streamed` chunk-level
AABB is the natural unit — if a chunk is occluded, skip its 10k
impostors.

V1 ships without occlusion culling. The frustum cull + tier cascade
+ shadow cap together hit the budget — occlusion is an optimisation,
not a correctness requirement.

---

## 12. Performance budget

### Per-frame ms allocation @ 60 fps (16.6 ms total)

```
   ┌──────────────────────────────────────────────────────┐
   │  Frame budget: 16.6 ms (60 FPS)                       │
   ├──────────────────────────────────────────────────────┤
   │  Render path total:           ≤ 12.0 ms              │
   │     Hero tier:                ≤  8.0 ms              │
   │     Active tier:              ≤  3.0 ms              │
   │     Streamed tier:            ≤  1.0 ms              │
   │     Horizon tier:             ≤  0.5 ms              │
   │     Tier switcher overhead:   ≤  0.2 ms (1 in 16 fr) │
   │     Frustum + shadow cap:     ≤  0.3 ms              │
   ├──────────────────────────────────────────────────────┤
   │  Application logic:           ≤  4.0 ms              │
   │     Scripts (Luau + Rune):    ≤  2.0 ms              │
   │     Physics (Avian):          ≤  1.5 ms              │
   │     Networking / replication: ≤  0.5 ms              │
   ├──────────────────────────────────────────────────────┤
   │  UI (Slint / egui):           ≤  0.6 ms              │
   ├──────────────────────────────────────────────────────┤
   │  Slack / GPU stall absorption: ~ 0.0 ms              │
   └──────────────────────────────────────────────────────┘
```

### Per-tier breakdown rationale

- **Hero ≤ 8 ms.** 2000 entities × 4 µs each = 8 ms. This is the
  expensive tier: full LOD0 mesh, shadow casters, full PBR.
  Doubling Hero-cap to 4000 means halving per-entity cost — only
  feasible for skeleton-light meshes.

- **Active ≤ 3 ms.** 20000 entities × 150 ns each = 3 ms (rendering
  only; tier-switcher logic counted separately). Static colliders,
  no shadow casters, LOD1/2 — the bulk of "visible at 100-500m".

- **Streamed ≤ 1 ms.** Most of this tier renders via instanced
  draws — 200000 impostor instances should issue as ~20 chunk-batches,
  each one draw call. The per-instance cost is dominated by
  GPU-side instance-buffer reads.

- **Horizon ≤ 0.5 ms.** One full-screen quad + one cubemap sample
  + an optional weather/cloud layer. Effectively fixed cost.

### Profiling hooks (telemetry)

Emit per-tier ms via the stream backbone:

```
render.cascade.frame_ms.hero
render.cascade.frame_ms.active
render.cascade.frame_ms.streamed
render.cascade.frame_ms.horizon
render.cascade.frame_ms.total
```

If a tier breaches budget, log a `render.cascade.budget_breach`
event with tier + measured ms + cap.

### Mobile derate

[15_MOBILE_PLATFORM](../AUDIT/15_MOBILE_PLATFORM.md) targets 30 FPS
(33 ms budget). Multiply all per-tier budgets by 2.0, but also halve
all caps: Hero 1000, Active 10000, Streamed 100000.

---

## 13. Mesh handle dedup + VRAM eviction

### Problem

A million-instance Space typically has ~50-100 *unique* meshes
(grass tuft × 1, tree × 5, building wall × 30, …). If the renderer
holds a `Handle<Mesh>` per instance and Bevy de-dupes by handle ID,
we're fine. If it accidentally loads the same mesh twice (different
paths, different handle IDs), VRAM explodes.

The bench-confirmed answer: **content-hash the GLB bytes**.

### Content-hash cache key

```rust
pub struct MeshLodCache {
    // key = blake3 hash of source GLB bytes
    // value = the 4 LOD handles + impostor atlas
    by_hash: HashMap<[u8; 32], MeshLodSet>,

    // soft cap — when exceeded, LRU-evict to disk
    vram_cap_bytes: u64,
    current_vram_bytes: u64,
    lru: LinkedHashMap<[u8; 32], Instant>,
}

pub struct MeshLodSet {
    pub lod0: Handle<Mesh>,
    pub lod1: Handle<Mesh>,
    pub lod2: Handle<Mesh>,
    pub lod3: Handle<Mesh>,
    pub impostor_atlas: Handle<Image>,
    pub vram_bytes: u64,
}
```

### Cache flow

```
   spawner asks: "give me the LOD set for mesh at path P"
        │
        ▼
   compute blake3 of P's contents
        │
        ▼
   look up in MeshLodCache.by_hash
        │
        ├── hit:  update LRU timestamp, return
        │
        └── miss:
              ├── load LOD0 from path
              ├── look up LOD1/2/3 hashes in manifest, load those
              ├── load impostor atlas
              ├── compute total VRAM bytes
              ├── if current_vram + new > vram_cap:
              │      evict LRU entries until under cap
              └── insert into cache + return
```

### VRAM accounting

Per-mesh approximate cost:

```
   tri × 3 vertices × (12 B pos + 12 B norm + 8 B UV + 16 B tan + 16 B color)
   = tri × 192 B  ─── upper bound
   + impostor atlas: 1024 × 1024 × 4 B (BC7) = 1 MB

   Typical 2000-tri mesh: 384 KB + 1 MB = 1.4 MB / unique mesh
   100 unique meshes:                       140 MB cache
   1000 unique meshes:                      1.4 GB ──── needs eviction
```

`vram_cap_bytes` default: 512 MB. Configurable per-project, query
GPU VRAM at startup and clamp to 1/4 of total.

### Eviction policy

LRU-by-access-time. When a tier-switcher refers to a handle, bump
its LRU timestamp. Evict the oldest until under cap. The evicted
mesh's `Handle<Mesh>` is dropped — Bevy frees the GPU buffer
asynchronously.

A re-request of an evicted mesh re-loads from disk in the next
streaming pass (50-150ms delay, but visually painted with the LOD3
or impostor immediately).

### Telemetry

```
mesh_cache.cache_size_mb
mesh_cache.hit_rate
mesh_cache.eviction_count
mesh_cache.thrashing_warning  // > 10 evicts/sec
```

---

## 14. Class-by-class LOD policy table

> **Status of the class list.** The canonical class registry is the
> `.defaults.toml` directory at
> [`eustress/crates/common/assets/class_schema/`](../../eustress/crates/common/assets/class_schema/)
> (per [CLASS_EXTENSIBILITY.md](../classes/CLASS_EXTENSIBILITY.md)).
> The list below mirrors the 49 classes I verified live there on
> 2026-05-26. Sub-classes of `Part` (e.g. `Seat`, `SpawnLocation`,
> `VehicleSeat`) inherit the `Part` row unless they override; new
> classes added later should add their row to this doc.

### Table

```
   Legend:
       LOD0 = full mesh, LOD1/2/3 = simplified, IMP = impostor billboard
       LIVE = per-frame update, CACHED = re-render on change, HIDDEN = no draw
       SHADOW = casts shadow, NOSHADOW = receives only, PROBE = SH9 light contribution
```

| ClassName | Hero (0–100 m) | Active (100–500 m) | Streamed (500m–5km) | Horizon (5 km+) |
|---|---|---|---|---|
| `Part` (Block/Ball/Cylinder/Wedge/Cone) | LOD0, SHADOW, full physics | LOD1, NOSHADOW, static collider | LOD3 or IMP, no physics | baked |
| `Seat` | LOD0, SHADOW, sit-trigger, physics | LOD1, NOSHADOW, no trigger | LOD3 or IMP, no trigger | baked |
| `VehicleSeat` | LOD0, SHADOW, drive-trigger, physics | LOD1, NOSHADOW, no trigger | LOD3 or IMP, no trigger | baked |
| `SpawnLocation` | LOD0, SHADOW, spawn-trigger | LOD1, NOSHADOW, no trigger | LOD3 or IMP, no trigger | baked |
| `MeshPart` / generic GLB | LOD0, SHADOW (if not flagged off), full physics | LOD1/2, NOSHADOW, static collider | LOD3 or IMP, no physics | baked |
| `SpecialMesh` (legacy mesh ref) | LOD0, SHADOW, no physics (decorative) | LOD2, NOSHADOW | IMP only | baked |
| `Model` (container) | Transform passthrough — components attach to children | (same) | (same) | (children baked) |
| `Folder` (container) | Transform passthrough | (same) | (same) | (children baked) |
| `Workspace` | Implicit root — no LOD | (same) | (same) | (same) |
| `Terrain` | Voxel mesh LOD ([terrain/lod.rs](../../eustress/crates/common/src/terrain/lod.rs)) | (same, LOD level driven by `LodUpdateState`) | LOD level 4-5 (coarse heightmap) | baked into panorama |
| `ChunkedWorld` | Children rendered per their own class rules | (same) | (same) | (same) |
| `Humanoid` (character) | LOD0 skinned mesh, anim 60 Hz, physics | LOD2 skinned, anim 10 Hz, kinematic | LOD3 unskinned or IMP, anim 2 Hz | NOT included (chars are always live) |
| `PointLight` | LIVE, SHADOW (up to cap) | LIVE, NOSHADOW | PROBE only | baked |
| `SpotLight` | LIVE, SHADOW | LIVE, NOSHADOW | PROBE only | baked |
| `SurfaceLight` | LIVE, area-light, SHADOW | LIVE, NOSHADOW | PROBE only | baked |
| `DirectionalLight` (sun/moon) | LIVE, cascaded shadow maps | LIVE, cascaded shadows | LIVE — affects everything | baked |
| `Camera` (non-active) | n/a | n/a | n/a | n/a |
| `BillboardGui` | LIVE per-frame text | CACHED (re-render on Changed) | HIDDEN beyond 1 km | n/a |
| `SurfaceGui` (decal on a Part face) | LIVE | CACHED | merged into mesh atlas or HIDDEN | baked |
| `ScreenGui` | screen-space — not in cascade | (same) | (same) | (same) |
| `TextLabel` (inside ScreenGui) | not in cascade | (same) | (same) | (same) |
| `Frame` / `ScrollingFrame` / `ImageLabel` / `TextButton` / `ImageButton` | screen-space only | (same) | (same) | (same) |
| `TextBox` | screen-space only | (same) | (same) | (same) |
| `ViewportFrame` | 3D mini-scene, render budget ≤ 0.5 ms | (same) | downgrade to screenshot | n/a |
| `Decal` | LIVE, full texture | CACHED | merged into ground atlas or HIDDEN | baked |
| `ParticleEmitter` | LIVE, full spawn rate | 25 % spawn rate, half-res | smoke-puff decal only | baked |
| `Beam` (laser/lightning) | LIVE, animated | LIVE, animated | LOD: static line | HIDDEN |
| `Sound` | LIVE 3D spatial | LIVE 3D, mono | LIVE quieter, no spatialisation | HIDDEN |
| `Animator` / `KeyframeSequence` | drives Hero skin anims | drives Active anims @ 10 Hz | drives Streamed anims @ 2 Hz | n/a |
| `Sky` | renders skybox at all distances (one entity, always) | (same) | (same) | composited with horizon panorama |
| `Atmosphere` | LIVE fog | LIVE fog | LIVE fog | composited |
| `Clouds` | LIVE volumetric | LIVE volumetric (lower res) | flat layer | baked |
| `Star` | LIVE | LIVE | LIVE | baked |
| `Moon` | LIVE with shadows | LIVE no shadows | LIVE | baked |
| `WeldConstraint` / `Motor6D` / `HingeConstraint` / `DistanceConstraint` / `PrismaticConstraint` / `BallSocketConstraint` / `SpringConstraint` / `RopeConstraint` | solver runs | solver disabled (parts pinned by static collider) | no solver | n/a |
| `UnionOperation` (CSG result) | treated as `Part` — LOD baked at union time | same | same | baked |
| `Attachment` (transform anchor) | passthrough | passthrough | passthrough | n/a |
| `LuauScript` / `LuauLocalScript` / `LuauModuleScript` / `SoulScript` | run per-script update policy ([scripting](../../eustress/crates/common/src/scripting/)) — unaffected by render-cascade | (same) | (same) | (same) |
| `RemoteEvent` / `RemoteFunction` / `BindableEvent` / `BindableFunction` | not rendered | (same) | (same) | (same) |
| `Team` | not rendered | (same) | (same) | (same) |
| `Animator` | drives anims at SimLodTier rate | (same) | (same) | n/a |
| `Lighting` (service) | not rendered | (same) | (same) | (same) |
| `Document` / `ImageAsset` / `VideoAsset` | asset references, not rendered | (same) | (same) | (same) |
| `DocumentFrame` / `WebFrame` / `VideoFrame` | screen-space 3D widget | LOD: static thumbnail | HIDDEN | n/a |
| `SolarSystem` / `CelestialBody` / `RegionChunk` | orbital/grid spatial only | (same) | (same) | (same) |
| `BoxHandleAdornment` / `SphereHandleAdornment` / `ConeHandleAdornment` / `CylinderHandleAdornment` / `LineHandleAdornment` | LIVE editor gizmo | HIDDEN beyond Hero (editor-only) | HIDDEN | HIDDEN |

### Notes per category

- **Adornments** are editor-only — never rendered in published Spaces.
  They're at most Hero-tier in Studio, off elsewhere.
- **Container classes** (`Model`, `Folder`, `Workspace`, `ChunkedWorld`)
  don't render — their children render per their own rules.
- **2D UI** (`Frame`, `ScreenGui`, etc.) is screen-space, outside the
  world-render-cascade entirely.
- **Constraints** affect physics, not rendering — they're listed for
  completeness but the row mostly mirrors the physics-tier behaviour.
- **Scripts** run on their own update cadence (SoulScript can opt
  into a frame rate, Luau is event-driven) — unaffected by tier.

---

## 15. Open questions

Questions needing human decision before Wave 3 implementation:

- **Q15.1 Tier-transition cross-fade duration.** Snap-swap LOD looks
  bad on hero geometry. Cross-fade at 100→500 m boundary costs
  double-render for the duration. **Recommend:** 150 ms cross-fade,
  cap-budget allowing — defer the polish to Wave 4.

- **Q15.2 Shadow-caster cap default.** Bevy default 4, can be tuned
  in `ClusteredForwardPlugin`. **Recommend:** 4 desktop / 2 mobile.

- **Q15.3 Origin-rebase priority.** Required for v1 or deferred?
  **Recommend:** defer to Wave 5 with a "no Spaces > 4km from spawn"
  constraint for v1.

- **Q15.4 `lod_components` on `ClassName` vs `ExtraSectionClaim`.**
  Centralised match-by-ClassName OR per-plugin override hook?
  **Recommend:** centralised on `ClassName` for v1, with an opt-in
  `ExtraSectionClaim::lod_override(tier)` hook for plugins.

- **Q15.5 Tier-exclusive component archetypes.** Use a marker
  `HeroTier` / `ActiveTier` / `StreamedTier` per tier (cleaner
  queries, more archetypes) OR a single `RenderTier` enum (one
  archetype, must check the value in every system)? **Recommend:**
  enum first, switch to markers if archetype churn shows in profiles.

- **Q15.6 `simplify` vs `simplify_sloppy` per mesh.** Ambient
  foliage / decoration tolerates topology break (sloppy faster + smaller);
  hero / silhouette geometry needs locked borders. **Recommend:**
  a per-mesh TOML opt: `lod_strategy = "sloppy" | "locked_border"`,
  default `sloppy` (most assets are ambient).

- **Q15.7 Impostor re-bake on lighting change.** Skybox/sun rotation
  shifts shadows in the impostor texture. **Recommend:** bake one
  set per major "lighting preset" — noon / dusk / night — and select
  the closest at render. Three atlases per asset = 3× storage.

- **Q15.8 Horizon panorama TOD coverage.** Same problem at planet scale.
  **Recommend:** v1 ships one panorama per chunk at noon; sun / moon
  overlay is dynamically composited (one quad). Real TOD coverage
  is Wave 5+.

- **Q15.9 Two-tile blend for impostors.** Doubles texture fetches.
  Worth it for grass tufts (no), trees (yes), buildings (maybe)?
  **Recommend:** per-asset TOML opt `impostor_blend = true`, off by
  default.

- **Q15.10 `bevy_floating_origin` crate vs in-house origin-rebase.**
  The crate exists but isn't maintained by Bevy core. **Recommend:**
  vendor and patch for Avian compat if/when origin-rebase ships.

- **Q15.11 Caps as instance count vs render budget.** Currently
  caps are entity counts (2k Hero, 20k Active). What if Hero items
  are 100k-triangle hero meshes vs 100-tri primitives? **Recommend:**
  later — add a `budget_units` weight per class for Wave 4
  refinement.

- **Q15.12 Multi-camera (split-screen) tier assignment.** Two
  cameras at different positions: union or intersection of tiers?
  **Recommend:** union — an entity in either camera's Hero band is
  Hero-tier. Caps stay global. See Risk R3.

- **Q15.13 Streamed-tier per-entity vs per-chunk.** For impostors,
  one chunk = one draw call. For LOD3 geometry, per-entity is fine.
  Which path per asset? **Recommend:** per-class default
  (`Part` instances → per-chunk batch; `MeshPart` named items →
  per-entity LOD3).

- **Q15.14 Class-list freshness.** The class list in §14 was
  manually verified against
  [`eustress/crates/common/assets/class_schema/`](../../eustress/crates/common/assets/class_schema/)
  on 2026-05-26. **Recommend:** add a CI check that fails if a new
  template lands without a row in this doc — mirrors the
  existing `class_schema drift` log check.

---

## 16. Risks and mitigations

### R1 — LOD pop at tier boundary

**Risk.** A Hero→Active transition swaps a LOD0 mesh for LOD1 in one
frame. Visible silhouette change.

**Mitigations.**
- 16-frame cadence (the boundary check itself runs every 267 ms)
- Hysteresis (re-cross threshold is 20 m wider)
- Per-class `lod_strategy = "locked_border"` keeps silhouette stable
- (Q15.1) optional 150 ms cross-fade — Wave 4 polish

### R2 — Impostor rotation flip

**Risk.** Camera arcs around an impostor: at each 45° yaw boundary,
the tile swaps and you see a flip.

**Mitigations.**
- 8 yaw bins (45° each) — smaller than human "noticing" arc at 1 km
- (Q15.9) two-tile blend kills the snap for hero-class impostors
- Octahedral encoding (Phase 2) eliminates the issue entirely
- Apply impostor only to Streamed tier (≥ 500 m) — far enough that
  the angular subtense is small

### R3 — Multi-camera (split-screen, NPC POV) tier assignment

**Risk.** Two cameras at different positions might both want Hero
treatment of the same entity, doubling Hero cap. Or, naïvely picking
one camera makes the other's view degraded.

**Mitigations.**
- Tier = MIN(tier-per-camera) — "if any camera sees you as Hero, you're Hero"
- Caps stay global (2k Hero total across all cameras)
- Telemetry: split-screen mode raises caps proportionally
  (configurable; default 2× for 2 cameras)
- See Q15.12

### R4 — Hover at tier boundary

**Risk.** A stationary player exactly at 100.000 m oscillates between
Hero and Active every frame.

**Mitigations.**
- Hysteresis (Hero in at ≤ 100 m, out at > 120 m — 20 m dead zone)
- 16-frame cadence (even if oscillation happens, it's 4 Hz max)

### R5 — Origin-rebase shift visible to player

**Risk.** A 1024 m subtraction applied to every entity mid-game
*could* cause a one-frame visible "snap" if any system isn't notified.

**Mitigations.**
- Single source of truth: a `RenderOrigin` resource
- All systems (physics / audio / particles / picking) subscribe to
  the `OriginShifted` event
- Throttle: max 1 shift per second
- Shift between tier-switcher cadence points (16-frame boundary
  guaranteed quiet)
- (Q15.11) defer to Wave 5 entirely

### R6 — VRAM thrashing under cache pressure

**Risk.** A player teleports between two distant areas: each one
unique mesh-set; cache evicts everything from area A on entering B.

**Mitigations.**
- LRU keeps recently-accessed in cache regardless of distance
- Teleport pre-warm: streamer pre-loads destination chunks (existing
  pattern, see [05_SPACE_STREAMING.md M1](../AUDIT/05_SPACE_STREAMING.md#feature-3))
- Cache miss falls back to LOD3 + impostor while LOD0/1/2 load — no
  blank frame

### R7 — Shadow caster cap demotion creates flicker

**Risk.** A scene with 5 hero-tier lights, cap=4. The 5th's shadow
state flicks on/off as the player rotates.

**Mitigations.**
- Hysteresis on distance-sort: only re-rank shadow casters every 16
  frames (same cadence as tier switcher)
- Telemetry: log `render.cascade.shadow_demoted` so artists see the
  pattern and can raise cap or split lights

### R8 — Horizon panorama mismatch with live content at boundary

**Risk.** A baked panorama shows a building. The streamer spawns the
live building at 4500 m. Both render at once → "double building".

**Mitigations.**
- Panorama bake-time **excludes** anything within
  `radius_in_panorama_m` (= 4500 m, hysteresis with Streamed tier
  outer boundary = 5500 m)
- Boundary band (4500–5500 m) renders live entity AND has it baked
  OUT of the panorama → seamless
- Live entity LOD3 → impostor → off as it crosses outward

### R9 — Bake-pipeline runtime cost

**Risk.** Importing 1000 GLBs into Studio means 4000 LOD bakes
+ 1000 impostor bakes + N horizon bakes — could take hours.

**Mitigations.**
- Bake in background (rayon thread pool, single thread for visibility)
- Cache by content-hash: skip if hashes match
- Build-farm path: publish triggers cloud bake of unchanged horizon
  panoramas
- Show progress + ETA in Studio status bar

### R10 — Streamer + render-cascade race at 500m boundary

**Risk.** The streamer demotes an entity at 600 m (evict_radius),
*before* the render-cascade has moved it to Streamed tier. One frame
with no representation.

**Mitigations.**
- Schedule ordering: `sys_render_cascade` runs AFTER
  `streaming::sys_radius_gate` in same `Update`
- 100 m hysteresis between render-cascade's `streamed_in_m` (480 m)
  and the streamer's `evict_radius` (600 m) — entity stays alive in
  the overlap

---

## 17. Wave 3 implementation order checklist

Phase 1 (week 1-2): tier infrastructure + Hero/Active tiers.

- [ ] **W3.1** Add `RenderTier` enum + `RenderCascadeConfig` resource to
  `eustress_common::streaming::render_cascade` (new submodule).
- [ ] **W3.2** Implement `sys_render_cascade` with the 16-frame cadence,
  hysteresis logic, and LRU cap enforcement. Chain it after
  `sys_radius_gate` in the streaming plugin.
- [ ] **W3.3** Implement `sys_apply_tier_change` reactor — uses
  `Changed<RenderTier>` to insert/remove tier-exclusive components.
- [ ] **W3.4** Wire up `ClassName::lod_components(tier, instance, mesh_cache)`
  for `Part`, `MeshPart`, and `Folder` / `Model` containers.
- [ ] **W3.5** Add `MeshLodCache` resource with blake3-keyed content-hashing
  + LRU eviction. Migrate `engine::mesh_optimizer::LodCache` to use
  it (consolidates the runtime-LOD path with offline-LOD).
- [ ] **W3.6** Wire `NotShadowCaster` insertion for Active tier;
  implement `sys_shadow_caster_cap` for Hero tier.
- [ ] **W3.7** Add per-tier telemetry events
  (`render.cascade.tier_changed`, `frame_ms.hero`, `cap_evict`).
- [ ] **W3.8** Acceptance test: spawn 100k `Part`s in a Space, verify
  Hero ≤ 2k entities, Active ≤ 20k, no shadow flicker, 60+ FPS.

Phase 2 (week 3-4): Streamed tier with impostors.

- [ ] **W3.9** Build offline LOD baker (`eustress mesh bake <glb>`).
  Outputs LOD1/2/3 GLB variants + content-hashed manifest entries.
- [ ] **W3.10** Build offline impostor baker (`eustress mesh bake-impostor`).
  One-shot scene + RenderTarget; outputs `assets/impostors/<asset>.png`.
- [ ] **W3.11** Implement Streamed tier component bundle: impostor quad +
  per-chunk draw batching + `RenderLayers::layer(1)`.
- [ ] **W3.12** Chunk-level frustum cull pass for Streamed tier.
- [ ] **W3.13** Acceptance test: 1M instances in a Space, walk the player
  from origin to 5km, verify smooth 60 fps the whole way.

Phase 3 (week 5-6): Horizon tier + polish.

- [ ] **W3.14** Build offline horizon panorama baker — equirect-render
  per 1km×1km coarse chunk, write KTX2.
- [ ] **W3.15** Implement runtime horizon pass — sample nearest 4
  panoramas + bilinear blend by chunk-distance.
- [ ] **W3.16** Acceptance test: 10M-instance Space, fly the camera to
  10 km altitude, verify all four tiers visible + 60 fps.

Phase 4 (optional, Wave 4): cross-fade + advanced features.

- [ ] **W4.1** Cross-fade at tier transition (Q15.1).
- [ ] **W4.2** Two-tile impostor blend (Q15.9).
- [ ] **W4.3** Origin rebase (Q15.3 / §9) — only if Wave 3 acceptance
  tests show jitter at the 4km mark.
- [ ] **W4.4** Phase-2 occlusion culling (§11).

---

## 18. Citations and references

### Existing Eustress documents

- [docs/AUDIT/05_SPACE_STREAMING.md](../AUDIT/05_SPACE_STREAMING.md) —
  chunk delivery, streaming radius gate, server dispatcher
- [docs/development/CHUNKED_STORAGE.md](../development/CHUNKED_STORAGE.md) —
  `.echk` format, manifest, hybrid TOML+binary
- [docs/development/HYBRID_COORDINATES.md](../development/HYBRID_COORDINATES.md) —
  DVec3 + Vec3 auto-switching (origin rebase is a special case)
- [docs/architecture/ORBITAL_GRID.md](../architecture/ORBITAL_GRID.md) —
  planetary-scale spatial partitioning (Tier 3 conceptual sibling)
- [docs/architecture/APEX_ENGINE.md](../architecture/APEX_ENGINE.md) —
  general engine targets including 10km cell sizes
- [docs/classes/CLASS_EXTENSIBILITY.md](../classes/CLASS_EXTENSIBILITY.md) —
  canonical class registry mechanism (template + `ClassName` enum +
  `ExtraSectionClaim`)
- [docs/AUDIT/04_ASSET_PIPELINE.md](../AUDIT/04_ASSET_PIPELINE.md) —
  asset bake / publish flow that hosts the LOD + impostor + horizon
  bake stages
- [docs/AUDIT/10_TELEMETRY.md](../AUDIT/10_TELEMETRY.md) — stream
  backbone for the `render.cascade.*` events
- [docs/AUDIT/11_SIMULATION_DEBUGGER.md](../AUDIT/11_SIMULATION_DEBUGGER.md) —
  deterministic replay implications (tier assignment must be
  reproducible from a frame seed)
- [docs/AUDIT/15_MOBILE_PLATFORM.md](../AUDIT/15_MOBILE_PLATFORM.md) —
  mobile derate (caps × 0.5, budget × 2)

### Existing source code (references)

- [`eustress/crates/common/src/streaming/types.rs`](../../eustress/crates/common/src/streaming/types.rs) —
  `StreamingConfig`, `Tier` enum, `ChunkCoord`
- [`eustress/crates/common/src/streaming/radius_gate.rs`](../../eustress/crates/common/src/streaming/radius_gate.rs) —
  `HysteresisRadiusGate` — render-cascade reuses
- [`eustress/crates/common/src/streaming/plugin.rs`](../../eustress/crates/common/src/streaming/plugin.rs) —
  `StreamingPlugin`, `sys_radius_gate` — render-cascade chains after
- [`eustress/crates/engine/src/mesh_optimizer.rs`](../../eustress/crates/engine/src/mesh_optimizer.rs) —
  existing runtime LOD via `meshopt::simplify`; refactor target for offline pipeline
- [`eustress/crates/engine/src/billboard_pipeline.rs`](../../eustress/crates/engine/src/billboard_pipeline.rs) —
  existing billboard rendering — impostor pipeline borrows the
  `NoFrustumCulling` pattern
- [`eustress/crates/common/src/realism/lod.rs`](../../eustress/crates/common/src/realism/lod.rs) —
  per-entity *simulation* LOD (SimLodTier::High/Mid/Low/Culled);
  render-cascade complements it by setting the matching tier
- [`eustress/crates/common/src/classes.rs`](../../eustress/crates/common/src/classes.rs) —
  `ClassName` enum (49 variants); `lod_components()` extension point
- [`eustress/crates/common/assets/class_schema/`](../../eustress/crates/common/assets/class_schema/) —
  the live class registry; see §14 freshness checks

### External references

- meshopt-rs — https://github.com/gwihlidal/meshopt-rs — used for
  offline LOD simplification and runtime cache/overdraw optimisation
- Star Citizen / Outerra origin-rebase technique — see
  https://www.gdcvault.com/play/1023002/Star-Citizen-A-Technical-Postmortem
  and https://outerra.blogspot.com/2008/12/precision-in-z-buffer.html
- Bevy frustum culling — `bevy_render::camera::Frustum` +
  `check_visibility` system
- `bevy_floating_origin` (third-party crate, vendoring candidate)
- meshoptimizer reference paper — https://github.com/zeux/meshoptimizer

### Changelog

- **P1 (2026-05-26):** Initial spec written. Documents Wave 1 design
  for the four-tier render cascade (Hero / Active / Streamed /
  Horizon), integrating with the existing chunked-streaming
  scaffold. No code changes — Wave 3 implementation order in §17.

---

### Critical files for implementation

The Wave-3 implementation will touch these files most directly:

- `eustress/crates/common/src/streaming/plugin.rs` — add the render-cascade systems to `StreamingPlugin::build`
- `eustress/crates/common/src/streaming/render_cascade.rs` — **new file** for `RenderTier`, `sys_render_cascade`, `sys_apply_tier_change`, `sys_shadow_caster_cap`
- `eustress/crates/common/src/classes.rs` — add `ClassName::lod_components(tier, ...)`
- `eustress/crates/engine/src/mesh_optimizer.rs` — refactor to share simplify pipeline with offline baker, plug into `MeshLodCache`
- `eustress/crates/common/src/streaming/types.rs` — add `RenderCascadeConfig` + `MeshLodCache` resource types
