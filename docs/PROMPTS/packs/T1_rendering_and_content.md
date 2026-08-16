# T1 — Rendering & Visual Fidelity + Content Pipeline

**Pack owner scope:** everything that decides what a stranger sees in the first three seconds and
whether they still believe it after ten. PBR correctness, lighting and shadow, reflections, the
post-processing chain, temporal stability under motion, anti-aliasing, volumetrics, the material
service, Gaussian-splatting compositing, HLOD proxy fidelity, asset import/export round-trip, and
the one flagship scene that has to survive a blind comparison against an engine the buyer already
trusts.

**Workloads this pack feeds:** primarily **W1 (Provable Quality)** — Critic scorecards over capture
bundles. Secondary **W3 (Trust & Verifiability)** for the capture-determinism and round-trip items,
and **W6 (Operator Leverage)** for the lighting-preset and material-authoring items, which cut
wall-clock on scene setup.

**ITEM ZERO: `G3.01`.** Nothing else in this pack may start until `G3.01` is `PASSED`. Every other
item measures a delta between two captures; if two captures of the same static scene are not
repeatable, every delta in the pack is noise. `G3.01` makes the capture path deterministic and
parameterised, and it is the only item here whose exit criterion is purely mechanical.

`G3.01` is this pack's entry point, not the program's. It consumes the general capture harness
rather than rebuilding it: the parameterised AI camera is `G1.05`, the seeded scene generator is
`G1.03`, and the frames-and-recording determinism proof is `G1.11`. `G3.01` therefore carries
`depends_on: [G1.03, G1.05, G1.11]`, and what remains uniquely its own is the render-side capture
path this pack measures against. See `docs/PROMPTS/04_FILE_OWNERSHIP.md`.

**Honest state of the world this pack starts from** (all verified in-tree, 2026-08-06):

- `eustress/crates/engine/src/photoreal.rs` is 67 lines: a `PhotorealSettings` resource and an empty
  `Plugin::build`. No post-process pass is registered by it.
- `eustress/crates/engine/Cargo.toml` lines 133–134 **do** enable the bevy features
  `bevy_anti_alias` and `bevy_post_process` on Bevy 0.19, with an inline comment stating the
  photoreal camera components "were reverted out of `studio_camera_bundle` — they washed the editor
  viewport to flat grey — so DefaultPlugins registers these plugins but NO camera carries the
  components, i.e. they are inert."
- `studio_camera_bundle` (`eustress/crates/engine/src/default_scene.rs:34`) ships `Camera3d`,
  `Tonemapping::TonyMcMapface`, a 70° perspective projection (near 0.1, far 10000), and
  `DepthPrepass`. No `Hdr`, no `Msaa` override, no AA, no bloom, no auto-exposure.
- `eustress/crates/engine/src/pbr_materials.rs` (14 lines) and
  `eustress/crates/engine/src/light_cookies.rs` (14 lines) are `TODO` stubs.
- `eustress/crates/engine/src/ai_camera.rs` renders an off-screen camera at a hardcoded
  1280×720 (`AI_CAM_WIDTH`/`AI_CAM_HEIGHT`) and `request_capture` takes a path and nothing else.
- `eustress/crates/common/src/streaming/render_cascade.rs` (736 lines) is real and registered at
  `eustress/crates/common/src/streaming/plugin.rs:224`. It toggles `Visibility` on tier change with
  no cross-fade. `docs/architecture/RENDER_CASCADE.md` still heads the section "Wave 1 SPEC ONLY" —
  the doc understates the code.
- `eustress/crates/engine/src/space/hlod.rs` (1391 lines) builds merged per-Morton-cell proxies and
  toggles their `Visibility` against residency. Also a hard cut.
- `eustress/crates/radiance/src/lib.rs` (768 lines) wraps `bevy_gaussian_splatting`;
  `eustress/crates/radiance/src/collider.rs` states the extraction "is not implemented yet".
- No frame-time percentile is computed anywhere: `grep -n "p99\|percentile" ` over
  `eustress/crates/engine/src/profiler.rs` and `.../frame_diagnostics.rs` returns nothing.
- The engine bridge writes its TCP port to `<universe>/.eustress/engine.port`
  (`eustress/crates/engine/src/engine_bridge/mod.rs:29`). That file is how any external measurement
  process finds the running engine.

**Project invariants every item in this pack inherits.** Eustress is an AI-native simulation
substrate / world engine, never a game engine — rendering is an implementation detail in service of
that. The licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian, never Rapier.
Slint is Rust. Units are meter-native; studs are a display unit only. A full engine build takes
10–15 minutes, one cargo build at a time against the shared `target/`, and is never killed
mid-compile. Validate with `cargo run`, not `cargo check`.

## Dependency ladder

| ID | Title | Tier | Critic gate | `depends_on` |
|---|---|---|---|---|
| **G3.01** | **Deterministic, parameterised reference capture path** (**ITEM ZERO**) | M | — | — |
| G3.02 | `render-probe` image-analysis binary + PBR reference baseline | L | — | G3.01 |
| G4.01 | Frame-time distribution instrumentation (p50/p95/p99/max) | M | — | G3.01 |
| G3.03 | PBR energy conservation and specular response | L | D2 | G3.02 |
| G3.04 | Exposure and tone response that survives the post-stack | L | D2, D1 | G3.02, G3.03 |
| G3.05 | Shadow cascade quality and contact grounding | L | D2 | G3.02 |
| G4.02 | Anti-aliasing path that does not require MSAA | L | D3, D2 | G4.01, G3.04 |
| G3.06 | Screen-space ambient occlusion under `Msaa::Off` | L | D2, D6 | G4.02, G3.05 |
| G4.03 | Temporal stability under camera motion | L | D3 | G4.02, G4.01 |
| G3.07 | Reflections: environment probes and specular occlusion | L | D2 | G3.03, G3.04 |
| G3.08 | Volumetric atmosphere and fog coherence | L | D2, D6 | G3.04, G3.07 |
| G4.04 | Render-cascade tier-transition pop suppression | L | D3, D6 | G4.03 |
| G5.01 | HLOD proxy fidelity at the swap boundary | L | D3, D2 | G4.04 |
| G3.09 | Material service: `.mat.toml` authoring round-trip | L | D2, D4 | G3.03 |
| G5.02 | Asset import/export round-trip fidelity budget | L | D6 | G3.09 |
| G3.10 | Gaussian-splatting / mesh composite correctness | L | D2, D6 | G3.07, G3.04 |
| G3.11 | Lighting presets: one action, measurably distinct | M | D1, D4 | G3.04, G3.05, G3.08, G3.09 |
| G3.12 | The first-three-seconds opening frame | L | D1, D6 | G3.11, G4.03, G3.06 |
| G3.13 | Flagship blind-comparison scene | XL | D1, D2, D3, D5, D6, D7 | G3.12, G5.01, G5.02, G3.10 |

Phase note: `G4.01` is instrumentation and is deliberately scheduled before most of `G3`. It builds
no render capability; it only makes frame-time measurable, which several `G3` items need in order to
prove they did not buy fidelity with frame budget.

---

## Cross-pack file ownership

Items in this pack share source files with items in other packs. `docs/PROMPTS/04_FILE_OWNERSHIP.md` names one owner per contested path and is normative; where it conflicts with an item's scope list, it wins. This pack's own dependency-graph table records its internal edges; the cross-pack edges are these:

| Item | Now depends on | Contested path | Effect on this item's scope |
|---|---|---|---|
| `G3.01` | `G1.03` (G1) | `eustress/crates/engine/Cargo.toml` | may append to but not alter it |
| `G3.01` | `G1.03` (G1) | `eustress/spaces/harness/` | may no longer edit it |
| `G3.01` | `G1.05` (G1) | `eustress/crates/engine/src/ai_camera.rs` | may no longer edit it |
| `G3.01` | `G1.05` (G1) | `eustress/crates/engine/src/engine_bridge/protocol.rs` | may append to but not alter it |
| `G3.01` | `G1.05` (G1) | `eustress/crates/mcp-server/src/bridge_tools.rs` | may append to but not alter it |
| `G3.01` | `G1.11` (G1) | — | G3.01 determinism criterion covers ai_camera.capture alone; frames-AND-recording-AND-step-count is G1.11 |
| `G3.02` | `G1.01` (G1) | `eustress/Cargo.toml` | may append to but not alter it |
| `G3.03` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G3.04` | `G1.05` (G1) | `eustress/crates/engine/src/ai_camera.rs` | may no longer edit it |
| `G3.04` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G3.05` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G3.06` | `G1.05` (G1) | `eustress/crates/engine/src/ai_camera.rs` | may no longer edit it |
| `G3.06` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G3.07` | `G1.05` (G1) | `eustress/crates/engine/src/ai_camera.rs` | may no longer edit it |
| `G3.07` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G3.08` | `G1.05` (G1) | `eustress/crates/engine/src/ai_camera.rs` | may no longer edit it |
| `G3.08` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G3.09` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G3.10` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G3.11` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G3.12` | `G7.39` (T5) | `eustress/crates/engine/src/space/load_phase.rs` | may no longer edit it |
| `G3.12` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G3.13` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G4.01` | `G1.05` (G1) | `eustress/crates/engine/src/engine_bridge/protocol.rs` | may append to but not alter it |
| `G4.01` | `G1.08` (G1) | `eustress/crates/engine/src/frame_diagnostics.rs` | may no longer edit it |
| `G4.02` | `G1.03` (G1) | `eustress/crates/engine/Cargo.toml` | may append to but not alter it |
| `G4.02` | `G1.05` (G1) | `eustress/crates/engine/src/ai_camera.rs` | may no longer edit it |
| `G4.02` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G4.03` | `G1.05` (G1) | `eustress/crates/engine/src/ai_camera.rs` | may no longer edit it |
| `G4.03` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G4.04` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G5.01` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |
| `G5.02` | `G2.34` (B1) | `eustress/crates/engine/src/mesh_import.rs` | may no longer edit it |
| `G5.02` | `G1.12` (G1) | `docs/PROMPTS/harness/recipes/` | consumes its capture recipe; may not author one |

Every item in this pack that names a `capture_recipe` other than `none` carries the `G1.12` edge.
`G1.12` authors all 33 cited recipes once in a single library-wide format and itself depends on
`G1.09`, the `eustress-capture` binary that consumes them, so one edge carries the whole harness
prerequisite. An agent that finds its recipe absent does not author one.

An item blocked by one of these entries emits a `FILE-OWNERSHIP` decision packet to L0 (`docs/PROMPTS/04_FILE_OWNERSHIP.md` §6). It does not edit the file and does not work around it.

---


---

## G3.01 — Deterministic, parameterised reference capture path (**ITEM ZERO**)

Item path: `docs/PROMPTS/items/G3.01_deterministic-reference-capture.md`

````markdown
---
id: G3.01
title: Deterministic, parameterised reference capture path
workload: W3
workload_secondary: [W1, W6]
phase: G3
depends_on: [G1.03, G1.05, G1.11]
blocks: [G3.02, G4.01]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G3.01/capture_determinism.json
escalation: >
  If two captures of the same static scene from the same pose cannot be made byte-identical
  because a render feature in the default studio camera path is inherently nondeterministic
  (for example a frame-indexed jitter or a time-seeded noise term), STALL immediately with the
  offending feature named and its path:line — do not disable the feature to force a pass.
status: DRAFT
notes: >
  Item zero for the T1 pack. Tier M rather than L because the change is confined to two files
  plus a manifest entry, but it still costs engine builds, hence 6.
---

## 1. Objective

The engine can be asked for a frame at a specified resolution, from a specified camera pose, in a
specified scene, and give back the same PNG bytes every time. Two captures taken from the same pose
in the same static harness scene hash identically, and a capture requested at 3840×2160 comes back
at 3840×2160. Until this is true, no render-fidelity delta in this pack is distinguishable from
capture noise.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. It is never described as a game engine. Rendering is an implementation
detail in service of that, which is why this item is gated on a mechanical, falsifiable measurement
rather than on taste.

**The capture path that exists today.** `eustress/crates/engine/src/ai_camera.rs` (199 lines)
spawns a second, off-screen `Camera3d` built from the same `studio_camera_bundle` as the editor
camera, targeting an `Image` render target. Two facts limit it:

- Resolution is hardcoded: `pub const AI_CAM_WIDTH: u32 = 1280;` and
  `pub const AI_CAM_HEIGHT: u32 = 720;` at `ai_camera.rs:43-44`. The image is created once in
  `spawn_ai_camera` and never resized.
- `pub fn request_capture(state: &mut AiCameraState, path: PathBuf)` at `ai_camera.rs:158` takes a
  path and nothing else. The bridge handler `ai_camera_capture`
  (`eustress/crates/engine/src/engine_bridge/protocol.rs:2547`) is dispatched from
  `eustress/crates/engine/src/engine_bridge/mod.rs:407`, and the MCP wrapper at
  `eustress/crates/mcp-server/src/bridge_tools.rs:1381` calls `ai_camera.capture` with an empty
  JSON object.

Pose control already exists and works: `ai_camera.set_pose` (params `position` `[x,y,z]` plus
either `look_at` `[x,y,z]` or `rotation` `[x,y,z,w]`), `ai_camera.orbit`, `ai_camera.frame`. Do not
rebuild those.

**A hazard you must respect.** `ai_camera.rs:133-149` documents a hard wgpu abort: two `Camera3d`s
both carrying Bevy `Atmosphere` hit a multi-camera atmosphere prepare-race and wgpu aborts with
"bind group descriptor (21) != layout (24)". The AI camera therefore carries
`eustress_common::plugins::lighting_plugin::NoAtmosphere`. Keep it. Also note `photoreal.rs:21`:
toggling `Msaa`/`Hdr` at runtime changes the view-bind-group shape and panics against
`SharedLightingPlugin`'s shared mesh-view bind-group layout. If you change either, change it once at
spawn, identically on both cameras, and never at runtime.

**How an external process finds the engine.** On startup the bridge binds a TCP listener and writes
the port to `<universe>/.eustress/engine.port`
(`eustress/crates/engine/src/engine_bridge/mod.rs:29`). The protocol is JSON-RPC over that socket.

**How the engine opens a specific scene.** `eustress/crates/engine/src/startup.rs:50` — the
`--space <dir>` flag opens a Space directory directly, overriding auto-discovery. `--universe <dir>`
opens the first Space inside a Universe.

**There is no reference scene in the repo yet.** `eustress/spaces/` does not exist. You will create
it. The precedent for a deterministic, code-generated scene is
`eustress/crates/engine/src/bin/generate_benchmark_map.rs` (494 lines), registered as the bin
`generate-benchmark-map` at `eustress/crates/engine/Cargo.toml:23-25`. Follow that pattern exactly:
a seeded generator binary is the source of truth, the emitted Space is a derived artifact.

**Build reality.** A full engine build takes 10–15 minutes. Only one cargo build may run at a time
against the shared `target/`. Never kill a build mid-compile. Validate with `cargo run`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — only the `ai_camera_capture` handler
- `eustress/crates/engine/src/bin/render_harness.rs` — new file
- `eustress/crates/engine/Cargo.toml` — only to register the new `[[bin]]`
- `eustress/crates/mcp-server/src/bridge_tools.rs` — only the `ai_camera_capture` input schema

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/default_scene.rs` — the shared camera bundle belongs to later items
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/ai_camera.rs` — owned by `G1.05` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- Any existing entry in `eustress/crates/engine/src/engine_bridge/protocol.rs` — the file is owned by `G1.05`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.
- Any existing entry in `eustress/crates/mcp-server/src/bridge_tools.rs` — the file is owned by `G1.05`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.
- Any existing entry in `eustress/crates/engine/Cargo.toml` — the file is owned by `G1.03`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.
- `eustress/spaces/harness/` — owned by `G1.03` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Comparing downscaled images, hashing only a crop, comparing with a tolerance, or making the
  harness scene emptier are all measurement changes. If byte-identity is genuinely unachievable for
  a named, cited reason, report `EXIT_CRITERION_UNMEASURABLE` with the `path:line` of the
  nondeterministic source and stop.
- Do not resize the off-screen image every frame. Resize on request only, and only when the
  requested size differs from the current size — a per-frame reallocation of a 4K render target will
  wreck the frame budget that later items in this pack must measure.
- Keep `NoAtmosphere` on the AI camera. Removing it reintroduces a documented hard wgpu abort.
- The three harness scenes must be emitted by a seeded generator, not hand-placed. A hand-built
  scene cannot be regenerated by a Critic or by a later item.
- Batch verification. Six builds is the whole budget; one build should validate the resolution
  change, the settle logic, and the generator together.

## 5. Exit criterion

### Criterion
Two consecutive `ai_camera.capture` calls at 3840×2160 from an identical pose in harness scene
`RH1_sphere_grid` produce PNGs whose SHA-256 digests are equal, and the emitted PNG dimensions are
exactly 3840×2160.

### Measurement

Command (three steps, run in order; the engine build and the probe run are separate cargo
invocations — do not run them concurrently):

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH1_sphere_grid \
        --seed 42 \
        --out eustress/spaces/harness/RH1_sphere_grid

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH1_sphere_grid

    cargo run -p eustress-engine --release --bin render-harness -- \
        selftest-determinism \
        --universe eustress/spaces/harness \
        --width 3840 --height 2160 \
        --position 6.0 3.5 9.0 --look-at 0.0 1.0 0.0 \
        --settle-frames 30 \
        --out docs/PROMPTS/artifacts/G3.01/capture_determinism.json

Expected output shape:

    {
      "requested": { "width": 3840, "height": 2160 },
      "emitted":   { "width": 3840, "height": 2160 },
      "capture_a_sha256": "9f1c...",
      "capture_b_sha256": "9f1c...",
      "identical": true,
      "settle_frames": 30,
      "commit": "71ccf6fe",
      "gpu": "NVIDIA GeForce RTX ...",
      "driver": "..."
    }

Pass condition:

    identical == true
      AND emitted.width == 3840 AND emitted.height == 2160
      AND the selftest-determinism process exits 0

Verify by reading `identical` and `emitted` out of the JSON and by checking the process exit code.
The existence of the JSON file is not a pass.

## 6. Critic gate

`critic_gate` is `[]`. This item ships no visual change, so there is nothing for a blinded evaluator
to score. The mechanical criterion in §5 replaces the gate and is therefore unusually tight: byte
equality, not similarity, and an exact resolution match rather than an aspect-ratio match.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — resize-on-request against the existing Image render target
   -> if still failing, MANDATORY approach change. Tuning the settle-frame count is NOT an
      approach change; despawning and respawning the AI camera with a fresh target is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the two digests still differ AND the count of
                  differing pixels moves < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a render feature in the default studio camera path is proven inherently
                   nondeterministic (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER byte-identity to a stated
per-pixel tolerance with the stated consequence for every downstream item; FUND a specific approach
D; DEFER behind a named blocking item; or KILL.

## 8. Artifact

`docs/PROMPTS/artifacts/G3.01/capture_determinism.json`

A reader finds: requested and emitted dimensions, both capture digests, the identity verdict, the
settle-frame count that achieved it, the generator seed, the SHA-256 of
`eustress/crates/engine/src/bin/render_harness.rs` (so the harness scene is reproducible even if the
emitted Space is not committed), the commit, and the GPU and driver version. Every later item in
this pack cites this file as the reason its own deltas are trustworthy.

## 9. Definition of NOT done

- Captures match at 1280×720 but not at 3840×2160. Resolution-dependent determinism is the defect,
  not evidence against it.
- Captures match because the settle count was raised until the scene stopped changing, but the
  harness scene contains an animated element that the count merely outran. The harness scenes must
  be genuinely static.
- The resolution parameter is accepted and echoed back but the emitted PNG is still 1280×720
  upscaled. Read the actual PNG header, not the request.
- The generator produces a different scene on a second run with the same seed. Then the harness is
  not a harness.
- Byte-identity is achieved by writing the PNG from a cached buffer rather than re-rendering. The
  second capture must be a genuine second render.
- The change works for the AI camera but silently breaks `viewport.capture` for the editor window.
  Both paths must still function; say in the result block that you checked.
````

---

## G3.02 — `render-probe` image-analysis binary and PBR reference baseline

Item path: `docs/PROMPTS/items/G3.02_render-probe-and-pbr-baseline.md`

````markdown
---
id: G3.02
title: render-probe image-analysis binary and PBR reference baseline
workload: W1
workload_secondary: [W3]
phase: G3
depends_on: [G3.01, G1.01]
blocks: [G3.03, G3.04, G3.05, G3.09]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G3.02/pbr_reference_baseline.json
escalation: >
  If the reference sphere grid cannot be rendered without the studio UI overlay contaminating the
  analysis region, STALL rather than cropping the analysis region to dodge the overlay — a moving
  crop invalidates every comparison the pack will later make against this baseline.
status: DRAFT
notes: >
  Tier L: a new workspace crate plus a full capture-and-analyse loop, each iteration costing an
  engine build.
---

## 1. Objective

A single binary, `render-probe`, reads captured PNGs and emits numeric JSON verdicts that a Critic
can inspect without trusting anything the executing agent wrote. Running it against the reference
sphere grid produces the pack's baseline scorecard: measured albedo, measured specular peak
position, measured Fresnel rim ratio, and measured energy sum for a 7×7 metallic-versus-roughness
grid, at the current commit, before any fidelity work begins.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Rendering serves the world model; this item
exists so that later render claims are numbers rather than adjectives.

**What already exists.** `G3.01` delivered: a parameterised `ai_camera.capture` accepting `width`,
`height`, and an output path; a `render-harness` generator bin registered in
`eustress/crates/engine/Cargo.toml`; and the harness Universe at `eustress/spaces/harness/` with the
scene `RH1_sphere_grid`. `G3.01` also proved two captures of the same static pose are byte-identical
at 3840×2160. Use all of it; do not rebuild it.

**What `RH1_sphere_grid` must contain** (extend the `G3.01` generator if it does not yet):
a 7×7 grid of unit spheres on a neutral 18%-albedo ground plane, metallic varying 0.0→1.0 across
columns and perceptual roughness varying 0.05→1.0 across rows, all with base colour sRGB
`#BFBFBF`, lit by exactly one directional light and the scene's environment lighting. Meter-native:
sphere radius 0.5 m, spacing 1.5 m.

**What the renderer actually is.** Bevy 0.19 PBR — `StandardMaterial`, the `bevy_pbr` feature set in
`eustress/crates/engine/Cargo.toml:99-135`. The studio camera
(`eustress/crates/engine/src/default_scene.rs:34`, `studio_camera_bundle`) ships
`Tonemapping::TonyMcMapface` and `DepthPrepass` and nothing else — no `Hdr`, no AA, no bloom, no
auto-exposure. `eustress/crates/engine/src/pbr_materials.rs` is a 14-line `TODO` stub and registers
no systems; material properties reach `StandardMaterial` through
`eustress/crates/engine/src/material_sync.rs` (333 lines) and
`eustress/crates/engine/src/space/material_loader.rs` (766 lines).

**Tonemapping matters for your maths.** Because `TonyMcMapface` is applied, raw PNG pixel values are
not linear radiance. Your probe must either invert the tonemap for energy analysis or measure
quantities that are monotonic under it and say which, explicitly, in the artifact. Do not silently
treat sRGB PNG values as linear.

**Where the probe lives.** Create a new workspace crate `eustress/crates/render-probe`, package name
`eustress-render-probe`, with one bin `render-probe`. Add it to the workspace members list in
`eustress/Cargo.toml`. It must not depend on `eustress-engine` — it reads PNGs and speaks the bridge
protocol over TCP, nothing more. The bridge port is at `<universe>/.eustress/engine.port`
(`eustress/crates/engine/src/engine_bridge/mod.rs:29`).

**Subcommands this pack will need from `render-probe`.** Implement all of them now; later items
depend on them and must not have to extend the tool:
`pbr-grid`, `penumbra`, `histogram`, `temporal`, `tier-pop`, `silhouette`, `diff`, `capture`.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
`cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/render-probe/` — new crate, all files
- `eustress/Cargo.toml` — only to add the workspace member
- `eustress/crates/engine/src/bin/render_harness.rs` — only to extend `RH1_sphere_grid`
- `docs/PROMPTS/harness/recipes/` — new recipe JSONs for this pack

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- Any renderer or material source file. This item measures; it does not improve. Changing the
  renderer here contaminates the baseline it exists to establish.
- Anything under `eustress/crates/common/src/physics/`
- Any existing entry in `eustress/Cargo.toml` — the file is owned by `G1.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Here that inverts: you must
  not change the artifact at all. If you find a render defect while building the probe, record it in
  the baseline JSON's `observations` array and leave the renderer alone.
- Every probe subcommand exits non-zero on a failed pass condition and prints the failing field.
  A probe that always exits 0 is useless as a gate.
- Probe output must be reproducible: same PNG in, same JSON out, no timestamps inside the measured
  fields.
- No image-comparison library that resamples. Analyse at native resolution.
- Batch verification: twelve builds across three approaches; the probe crate itself builds fast, so
  spend builds on the engine only when the harness scene changes.

## 5. Exit criterion

### Criterion
`render-probe pbr-grid` emits, for all 49 spheres in `RH1_sphere_grid`, a measured
`specular_peak_offset_px`, `fresnel_rim_ratio`, `mean_linear_albedo`, and `energy_sum`, and
re-running it on the same PNG reproduces every value to within `1e-9`.

### Measurement

Command:

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH1_sphere_grid

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH1_pbr_grid.json \
        --out docs/PROMPTS/artifacts/G3.02/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        pbr-grid \
        --frame docs/PROMPTS/artifacts/G3.02/frames/RH1_000.png \
        --grid 7x7 \
        --out docs/PROMPTS/artifacts/G3.02/pbr_reference_baseline.json

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        pbr-grid \
        --frame docs/PROMPTS/artifacts/G3.02/frames/RH1_000.png \
        --grid 7x7 \
        --out docs/PROMPTS/artifacts/G3.02/pbr_reference_baseline_repeat.json

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        diff \
        --a docs/PROMPTS/artifacts/G3.02/pbr_reference_baseline.json \
        --b docs/PROMPTS/artifacts/G3.02/pbr_reference_baseline_repeat.json \
        --max-abs 1e-9

Expected output shape (from the `diff` step):

    { "fields_compared": 196, "max_abs_delta": 0.0, "within_tolerance": true }

Pass condition:

    fields_compared == 196
      AND within_tolerance == true
      AND the pbr-grid runs each report exactly 49 cells with all four fields non-null
      AND all four commands exit 0

Verify by reading `fields_compared`, `within_tolerance`, and the per-cell null count. Do not infer
success from the JSON files appearing.

## 6. Critic gate

`critic_gate` is `[]`. This item ships a measuring instrument and a number, not a visual change.
The mechanical criterion in §5 replaces the gate: 49 cells × 4 fields = 196 values, all present,
all reproducible to `1e-9`. It also produces the recipe JSONs under
`docs/PROMPTS/harness/recipes/` that the rest of this pack's Critic gates consume, so a sloppy
recipe here weakens every later scorecard.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — analytic sphere segmentation from known grid geometry
   -> if still failing, MANDATORY approach change. Tuning a threshold is NOT an approach change;
      switching to marker-driven segmentation (a known-colour registration fiducial emitted by the
      harness generator) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the count of successfully measured cells moving < 2
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the studio UI overlay cannot be kept out of the analysis region (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.02/pbr_reference_baseline.json`

A reader finds: one object per grid cell keyed by `(metallic, roughness)`, each carrying
`mean_linear_albedo`, `specular_peak_offset_px`, `fresnel_rim_ratio`, `energy_sum`; the tonemap
handling declaration (inverted, or measured-monotonic-under-tonemap, stated explicitly); the capture
recipe path and its SHA-256; the frame digest; the commit; and the GPU and driver version. An
`observations` array records any defect noticed but deliberately not fixed. Every fidelity item in
this pack states its improvement as a delta against this file.

## 9. Definition of NOT done

- The probe measures 49 cells but three of them are the ground plane because segmentation drifted.
  A wrong cell is worse than a missing one.
- Values reproduce to `1e-9` because the probe caches its previous result keyed by input path.
  Re-run must recompute.
- The probe treats sRGB PNG values as linear radiance and reports an `energy_sum` that means
  nothing. The tonemap declaration in the artifact must be true.
- The baseline is captured after "just a small fix" to a material or light. Then it is not a
  baseline and every later delta is measured from a moving origin.
- `pbr-grid` works but `penumbra`, `temporal`, `tier-pop`, and `silhouette` are stubs that print
  `todo!()`. Later items are blocked and the ladder stalls three rungs down.
- The recipes under `docs/PROMPTS/harness/recipes/` name poses that are not reproducible from the
  generator seed, so a Critic cannot regenerate the frame set.
````

---

## G4.01 — Frame-time distribution instrumentation

Item path: `docs/PROMPTS/items/G4.01_frame-time-distribution.md`

````markdown
---
id: G4.01
title: Frame-time distribution instrumentation with p50/p95/p99/max
workload: W1
workload_secondary: [W3, W6]
phase: G4
depends_on: [G3.01, G1.05, G1.08]
blocks: [G4.02, G4.03, G4.04]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G4.01/frametime_baseline.json
escalation: >
  If collecting per-frame timings at the required window length measurably perturbs the frame time
  it is measuring — more than 0.2 ms mean delta between an instrumented and an uninstrumented run
  of the same scene — STALL rather than shortening the window to hide the perturbation.
status: DRAFT
notes: >
  Tier M: one new module plus a plugin registration. Every render item after this one has to prove
  it did not buy fidelity with frame budget, and today there is no percentile anywhere in the tree.
---

## 1. Objective

The engine can report the distribution of its frame times, not just their mean. A run against a
named harness scene emits p50, p95, p99, max, and the 1% low, over a bounded window, to a JSON file
that any later item can cite as its frame-cost evidence.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Frame time matters here because a fidelity
improvement paid for with a stutter is not an improvement, and the Critic's motion dimension has
hard numeric sub-floors.

**What exists today.** Two profiling paths, neither of which computes a percentile:

- `eustress/crates/engine/src/profiler.rs` (702 lines) — the always-compiled phase profiler. Armed
  by the env var `EUSTRESS_PROFILE`, window length via `EUSTRESS_PROFILE_FRAMES` (default 120,
  `profiler.rs:112`). Writes a ranked table to `eustress_profile.txt` (`profiler.rs:329`) and an
  inferno flamegraph to `eustress_profile.svg`, both into the current working directory. It
  attributes time to systems and phases — it does not characterise the frame-time distribution.
- `eustress/crates/engine/src/frame_diagnostics.rs` (151 lines) — `FrameTimeTracker`, whose default
  is `Self::new(1000)`, i.e. it only logs frames over one second. It stores a `HashMap` of system
  times and has no percentile logic.

`grep -n "p99\|percentile\|p95"` over both files returns nothing. This capability is at 0%.

**A measured number you may cite, and its provenance.**
`docs/development/BENCHMARK_VS_ENGINE_AUDIT.md` records a benchmark run at 8K entities @ 5,406 FPS
against the engine at 10K entities @ ~45 FPS, with ~10,000 draw calls and one-second stutters traced
to `write_instance_changes_system` performing 20K synchronous disk operations. Treat those as
MEASURED-elsewhere context, not as your baseline. Your baseline is what you measure.

**A number you must not cite as measured.** The 2.10M-entity figure in
`docs/AUDIT/05_SPACE_STREAMING.md:23` is an `active_cap` CONFIG DEFAULT, not a measurement.

**What `G3.01` gave you.** The harness Universe `eustress/spaces/harness/`, the `render-harness`
generator bin, deterministic parameterised capture, and the `--space <dir>` launch flag
(`eustress/crates/engine/src/startup.rs:50`).

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
`cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/bin/render_harness.rs` — only to add the `frametime` subcommand
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — only to add a `frametime.report` handler
- `eustress/crates/engine/src/plugins/` — only the registration of the above

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/profiler.rs` — the phase profiler is a different instrument with a
  different arming mechanism; do not entangle them
- Anything under `eustress/crates/common/src/physics/`
- Any existing entry in `eustress/crates/engine/src/engine_bridge/protocol.rs` — the file is owned by `G1.05`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.
- `eustress/crates/engine/src/frame_diagnostics.rs` — owned by `G1.08` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Discarding the first N
  frames beyond a declared, fixed warm-up; excluding outliers; smoothing the series before taking
  percentiles; or shortening the window until the numbers look calm are all measurement changes.
- Percentiles are computed over the raw per-frame series with no smoothing. Store the raw series
  length and the warm-up count in the artifact so a Critic can check the arithmetic.
- The collector must be allocation-free in steady state: a fixed-capacity ring sized from the
  requested window at startup. A `Vec` that grows per frame perturbs what it measures.
- Arm it explicitly (an env var or a bridge call), off by default. It must cost nothing in a normal
  session.

## 5. Exit criterion

### Criterion
A 3,000-frame run against harness scene `RH2_interior` emits p50, p95, p99, max, and 1%-low frame
times in milliseconds, and the instrumented run's mean frame time differs from an uninstrumented run
of the same scene by no more than 0.2 ms.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH2_interior --seed 42 \
        --out eustress/spaces/harness/RH2_interior

    cargo run -p eustress-engine --release --bin render-harness -- \
        frametime \
        --space eustress/spaces/harness/RH2_interior \
        --frames 3000 --warmup 300 \
        --out docs/PROMPTS/artifacts/G4.01/frametime_baseline.json

    cargo run -p eustress-engine --release --bin render-harness -- \
        frametime \
        --space eustress/spaces/harness/RH2_interior \
        --frames 3000 --warmup 300 --collector off \
        --out docs/PROMPTS/artifacts/G4.01/frametime_uninstrumented.json

Expected output shape:

    {
      "frames_measured": 3000,
      "warmup_frames": 300,
      "ft_p50_ms": 11.9,
      "ft_p95_ms": 14.2,
      "ft_p99_ms": 18.6,
      "ft_max_ms": 41.0,
      "ft_1pct_low_ms": 53.8,
      "ft_mean_ms": 12.3,
      "collector": "on",
      "commit": "71ccf6fe",
      "gpu": "...",
      "driver": "..."
    }

Pass condition:

    frames_measured == 3000
      AND every one of ft_p50_ms, ft_p95_ms, ft_p99_ms, ft_max_ms, ft_1pct_low_ms is present and > 0
      AND abs(instrumented.ft_mean_ms - uninstrumented.ft_mean_ms) <= 0.2
      AND both runs exit 0

Verify by reading the emitted values and computing the mean delta. File existence is not a pass.

## 6. Critic gate

`critic_gate` is `[]`. This item ships an instrument, not a visual change. The mechanical criterion
replaces it and is tight in two directions at once: the instrument must produce all five statistics
over a stated window, and it must be provably non-perturbing to within 0.2 ms. Later items in this
pack — `G4.02`, `G4.03`, `G4.04`, `G5.01`, and the flagship `G3.13` — all cite this file's fields as
their frame-cost evidence, so an instrument that lies here poisons five downstream scorecards.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — fixed-capacity ring in a Bevy resource, sampled once per frame in Last
   -> if still failing, MANDATORY approach change. Changing the ring size is NOT an approach change;
      moving the sample point to the render sub-app's frame boundary is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the instrumented-vs-uninstrumented mean delta
                  moving < 5% and still above 0.2 ms
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: collection at the required window length perturbs frame time (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G4.01/frametime_baseline.json`

A reader finds: the five statistics, the mean, the raw series length, the warm-up count, the scene
name and generator seed, the collector state, the commit, and the GPU and driver version. Alongside
it, `frametime_uninstrumented.json` with the same shape and `"collector": "off"`, which is the
evidence that the instrument does not perturb its subject.

## 9. Definition of NOT done

- p99 is computed over a smoothed or decimated series. The number is then not p99 of anything real.
- The window is 3,000 frames but the scene finished loading at frame 2,400, so 80% of the series is
  load-time. Warm-up must genuinely reach steady state; state the evidence that it did.
- The collector is always on and adds 0.15 ms. Under the threshold, but every future session now
  pays for it. It must be off by default.
- The five statistics are emitted but the raw series length is not, so a Critic cannot check whether
  p99 was taken over 3,000 samples or 30.
- The 1% low is reported in milliseconds in one place and FPS in another. Pick one unit, declare it
  in the field name, and keep it.
- The instrument works only in the harness scene because it depends on a marker the generator emits.
  It must work in any Space.
````

---

## G3.03 — PBR energy conservation and specular response

Item path: `docs/PROMPTS/items/G3.03_pbr-energy-and-specular.md`

````markdown
---
id: G3.03
title: PBR energy conservation and specular response on the reference grid
workload: W1
workload_secondary: []
phase: G3
depends_on: [G3.02, G1.12]
blocks: [G3.04, G3.07, G3.09]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D2]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH1_pbr_grid.json
artifact: docs/PROMPTS/artifacts/G3.03/pbr_energy.json
escalation: >
  If closing the energy error requires replacing Bevy's StandardMaterial with a custom material
  across the whole scene graph, STALL immediately — that is a subsystem-scale change and must be
  funded as its own XL item, not smuggled into a fidelity fix.
status: DRAFT
notes: >
  Tier L: cross-file change through material_sync plus a capture-and-Critic loop, one engine build
  per iteration.
---

## 1. Objective

Rough metals stop being darker than they should be and smooth dielectrics stop losing their grazing
highlight. On the 7×7 reference sphere grid, total reflected energy varies monotonically with
roughness within a bounded error, and the Fresnel rim ratio rises with grazing angle on every
dielectric row — both measured against the `G3.02` baseline, from the same frozen capture recipe.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never a game engine. This item exists because a domain expert who does not
believe the surfaces will not believe the simulation either.

**The scene, frozen.** `eustress/spaces/harness/RH1_sphere_grid`, emitted by
`cargo run -p eustress-engine --release --bin render-harness -- --scene RH1_sphere_grid --seed 42`.
7×7 unit spheres, radius 0.5 m, spacing 1.5 m, base colour sRGB `#BFBFBF`, metallic 0.0→1.0 across
columns, perceptual roughness 0.05→1.0 across rows, on an 18%-albedo ground plane, one directional
light plus environment lighting. Meter-native throughout.

**The baseline you are beating.** `docs/PROMPTS/artifacts/G3.02/pbr_reference_baseline.json`, which
records for each of the 49 cells: `mean_linear_albedo`, `specular_peak_offset_px`,
`fresnel_rim_ratio`, `energy_sum`, plus an explicit declaration of how tonemapping was handled. Read
that declaration before doing any arithmetic — the studio camera applies
`Tonemapping::TonyMcMapface` (`eustress/crates/engine/src/default_scene.rs:39`), so PNG values are
not linear radiance.

**Where material properties actually come from.** `eustress/crates/engine/src/material_sync.rs`
(333 lines) runs three chained systems each `Update`: `reapply_materials_on_registry_change`,
`set_material_textures_to_repeat`, `sync_basepart_to_material`. It maps `BasePart` and the
`Material` enum onto `StandardMaterial`. `eustress/crates/engine/src/space/material_loader.rs`
(766 lines) owns the `MaterialRegistry`. `eustress/crates/engine/src/pbr_materials.rs` is a 14-line
`TODO` stub that registers no systems — do not be misled by its name.

**Environment lighting.** `eustress/crates/common/src/plugins/lighting_plugin.rs:574` inserts an
`EnvironmentMapLight`; `eustress/crates/common/src/services/lighting.rs:264` and `:273` describe
`GeneratedEnvironmentMapLight` and `AtmosphereEnvironmentMapLight` selection. Specular response on
the metallic columns is dominated by this, not by the directional light, so an energy error here may
be an IBL intensity error rather than a BRDF error. Diagnose before you change.

**A hazard.** `eustress/crates/engine/src/ai_camera.rs:133-149`: the AI camera carries
`NoAtmosphere` because two atmosphere cameras abort wgpu. That means the capture camera and the
editor camera do not see identical ambient. Compare capture-to-capture, never capture-to-viewport.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/material_sync.rs`
- `eustress/crates/engine/src/pbr_materials.rs`
- `eustress/crates/engine/src/space/material_loader.rs`
- `eustress/crates/common/src/plugins/lighting_plugin.rs` — environment-light intensity only
- `eustress/crates/engine/assets/shaders/` — a new WGSL file is permitted if justified in the result

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/render-probe/` — changing the instrument to change the number fails the item
- `eustress/crates/engine/src/bin/render_harness.rs` — the harness scene is frozen for this item
- `eustress/crates/engine/src/photoreal.rs` and `eustress/crates/engine/src/default_scene.rs` — the
  post-stack and exposure are `G3.04`'s subject, not yours
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Editing the probe, moving the grid cells, widening the tolerance, dropping the roughest row,
  re-seeding the harness, or lowering capture resolution are all measurement changes. If the
  measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Diagnose before editing. State in the result block whether the error is BRDF-side, IBL-intensity
  side, or tonemap-side, with the measured evidence that distinguishes them.
- Do not compensate an energy error by scaling `base_color`. That trades a physical error for a
  cosmetic one and will fail the Critic's coherence reading elsewhere in the scene.
- Do not touch exposure. If the whole grid is uniformly too dark or too bright, that is `G3.04`.
  Report it and leave it.
- Batch your verification. Twelve builds across three approaches; one build should validate several
  changes.

## 5. Exit criterion

### Criterion
On `RH1_sphere_grid`, the maximum absolute deviation of `energy_sum` from the monotone roughness
trend across each metallic column falls to **≤ 4%** of that column's mean `energy_sum`, and
`fresnel_rim_ratio` is strictly increasing with grazing angle on all seven dielectric
(metallic = 0.0) cells.

### Measurement

Command:

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH1_sphere_grid

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH1_pbr_grid.json \
        --out docs/PROMPTS/artifacts/G3.03/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        pbr-grid \
        --frame docs/PROMPTS/artifacts/G3.03/frames/RH1_000.png \
        --grid 7x7 \
        --baseline docs/PROMPTS/artifacts/G3.02/pbr_reference_baseline.json \
        --out docs/PROMPTS/artifacts/G3.03/pbr_energy.json

Expected output shape:

    {
      "max_column_energy_deviation_pct": 3.1,
      "baseline_max_column_energy_deviation_pct": 11.7,
      "fresnel_monotonic_dielectric_cells": 7,
      "dielectric_cells_total": 7,
      "cells_measured": 49,
      "frame_sha256": "...",
      "recipe_sha256": "...",
      "commit": "...",
      "gpu": "...",
      "driver": "..."
    }

Pass condition:

    max_column_energy_deviation_pct <= 4.0
      AND fresnel_monotonic_dielectric_cells == dielectric_cells_total
      AND cells_measured == 49
      AND every command exits 0

Verify by reading the emitted values and the exit codes, never by observing that files appeared.

## 6. Critic gate

Gated on **D2 (material and lighting realism)**, floor **8.0**. One dimension below floor fails the
item; there is no mean to hide behind. Capture recipe:
`docs/PROMPTS/harness/recipes/T1_RH1_pbr_grid.json`.

Prioritise accordingly: the improvement must be legible in a single still frame, because the Critic
never sees your result block, your commit message, or any caption, and every score it awards must
cite a specific frame index or a measured value. An improvement visible only in a spreadsheet will
not be credited. Do not turn the darkening of rough metals into a uniform brightening of the whole
grid — a grid that is energy-correct but flat reads worse to a blinded evaluator than one that is
slightly wrong but structured.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — correct the IBL contribution path in material_sync / lighting_plugin
   -> if still failing, MANDATORY approach change. Rescaling an intensity constant is NOT an
      approach change; adding a multi-scatter energy-compensation term to the BRDF is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D2 moving < 0.5 AND
                  max_column_energy_deviation_pct moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the fix requires replacing StandardMaterial scene-wide (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.03/pbr_energy.json`

A reader finds: the measured and baseline column-energy deviations, the dielectric Fresnel
monotonicity count, all 49 per-cell values, the frame and recipe digests, the diagnosis (BRDF /
IBL / tonemap) with its supporting numbers, the commit, and the GPU and driver version. Archived
alongside: the capture bundle manifest and the D2 scorecard.

## 9. Definition of NOT done

- Deviation drops below 4% because the roughest row was excluded from the trend fit. Measurement
  change; the item fails outright.
- Energy is conserved but every sphere is now the same brightness, so metallic and dielectric
  columns are indistinguishable. That is worse than the defect.
- The number passes at 3840×2160 and fails at 1920×1080. Resolution-dependent energy is a sampling
  artifact, not a BRDF fix.
- The fix lands in `material_sync.rs` but `material_loader.rs` re-applies the old values on the next
  registry change, so the grid is correct for one frame after load and wrong thereafter. Capture
  after the registry settles and say so.
- The Critic passes D2 on the sphere grid, but the same change makes `RH2_interior` visibly washed
  out. Coherence across scenes is checked; state what you verified.
- A tonemap or exposure change was used to move the number. Both belong to `G3.04`; moving them here
  makes that item unmeasurable.
````

---

## G3.04 — Exposure and tone response that survives the post-stack

Item path: `docs/PROMPTS/items/G3.04_exposure-and-tone-response.md`

````markdown
---
id: G3.04
title: Exposure and tone response that survives the post-stack
workload: W1
workload_secondary: [W6]
phase: G3
depends_on: [G3.02, G3.03, G1.05, G1.12]
blocks: [G4.02, G3.07, G3.08, G3.11]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D2, D1]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH1_exposure.json
artifact: docs/PROMPTS/artifacts/G3.04/exposure_response.json
escalation: >
  If adding Hdr to the shared studio camera bundle reintroduces the mesh-view bind-group panic
  described in photoreal.rs (bind group descriptor size mismatch against SharedLightingPlugin),
  STALL immediately with the panic text and both camera spawn sites — do not work around it by
  giving the editor and AI cameras different view configurations.
status: DRAFT
notes: >
  Tier L. Direct successor to a previously reverted attempt; the manifest comment at
  eustress/crates/engine/Cargo.toml:128-135 is the failure signature it must not reproduce.
---

## 1. Objective

An 18% mid-grey card in the harness scene lands at a predictable, stated output value, and a
four-stop exposure sweep moves it monotonically by the expected magnitude. The post-process chain is
attached to a camera and demonstrably active, and the frame does not go flat grey — the failure that
caused the previous attempt to be reverted.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Exposure matters here for a specific
reason: the first three seconds a stranger spends with a frame are dominated by tone response, and a
substrate whose output looks washed will not be trusted with a model.

**The exact prior failure, quoted from the manifest.**
`eustress/crates/engine/Cargo.toml:128-135` enables the bevy features `bevy_anti_alias` and
`bevy_post_process` on Bevy 0.19, with this inline comment:

> Kept enabled only so the previous build's bevy artifacts are reused (a ~100-min bevy recompile
> otherwise). The photoreal camera components (Bloom/TAA/SSAO) were reverted out of
> `studio_camera_bundle` — they washed the editor viewport to flat grey — so DefaultPlugins
> registers these plugins but NO camera carries the components, i.e. they are inert. Drop both
> features (and accept the bevy rebuild) when doing a clean pass.

So the crates are available and their plugins are registered; the components are simply not attached
to any camera. "Washed to flat grey" is the signature of an exposure/HDR mismatch — bloom and
tonemapping operating on a non-HDR target — not evidence that the post-stack is unavailable.

**What the camera bundle ships today.** `eustress/crates/engine/src/default_scene.rs:34`,
`studio_camera_bundle`: `Camera3d::default()`, `Tonemapping::TonyMcMapface`, a
`PerspectiveProjection` at 70° with near 0.1 and far 10000, `Instance`, `Name`, and `DepthPrepass`.
There is no `Hdr` component and no `Msaa` override. `bevy::camera::Hdr` is inserted in exactly one
place in the tree — `eustress/crates/engine/src/ui/slint_ui.rs:2437`, for the gizmo pipeline — not
on the studio camera.

**Auto-exposure is not registered.** `eustress/crates/engine/src/photoreal.rs:16-17` states that
Bevy's `DefaultPlugins`/`PostProcessPlugin` does NOT include auto-exposure and that
`AutoExposurePlugin` would have to be registered explicitly. `PhotorealPlugin::build`
(`photoreal.rs:60-67`) currently does nothing but `init_resource::<PhotorealSettings>()`.

**The bind-group hazard, from `photoreal.rs:19-23`.** A settings-to-components sync system must keep
`Msaa::Off` and `Hdr` permanent, because toggling them at runtime changes the view bind-group shape
and panics against `SharedLightingPlugin`'s shared mesh-view bind-group layout, and must apply
changes identically to BOTH cameras — the editor camera (`default_scene.rs:136`) and the off-screen
AI camera (`ai_camera.rs:110-150`). Both are built from `studio_camera_bundle`, which is why they
must move together.

**Exposure already has a system.** `eustress/crates/common/src/plugins/lighting_plugin.rs` registers
`update_exposure_compensation` in `Update` inside `SharedLightingPlugin::build`. Read it before
adding a second exposure authority.

**What `G3.02` and `G3.03` gave you.** The `render-probe` binary with a `histogram` subcommand, the
frozen harness Universe, and a physically-corrected sphere grid.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check` — `cargo check` will not catch the pipeline
registration and bind-group failures this item is most likely to produce.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/photoreal.rs`
- `eustress/crates/engine/src/default_scene.rs` — `studio_camera_bundle` only
- `eustress/crates/common/src/plugins/lighting_plugin.rs` — `update_exposure_compensation` only
- `eustress/crates/engine/src/bin/render_harness.rs` — only to add an 18% mid-grey card to
  `RH1_sphere_grid`. That changes the harness, so you must re-run `G3.02`'s baseline capture and
  record the new baseline digest in your artifact.

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/render-probe/`
- `eustress/crates/engine/src/material_sync.rs` — `G3.03` owns the BRDF; do not re-tune it here
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/ai_camera.rs` — owned by `G1.05` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Moving the grey card, changing its albedo, sampling a brighter region, or relaxing the stop
  spacing are measurement changes.
- Editor camera and AI camera must receive identical view configuration. If you add `Hdr` or
  `Msaa::Off`, add it in `studio_camera_bundle` so both inherit it, and never toggle either at
  runtime.
- Do not enable bloom in this item. Bloom without correct exposure is exactly what produced the flat
  grey. Land exposure first; bloom is available to a later item once mid-grey is anchored.
- If you register `AutoExposurePlugin`, the exit criterion is still measured with adaptation locked,
  because an adapting exposure makes every later capture in this pack non-reproducible. Provide the
  lock and document how to set it.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
With adaptation locked, an 18% mid-grey card in `RH1_sphere_grid` renders at a mean output luminance
within **±3/255** of a declared target value, and an exposure sweep of −2, −1, 0, +1 stops moves that
mean monotonically, with each single-stop step changing scene-linear luminance by a factor in
**[1.7, 2.3]**.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH1_sphere_grid --seed 42 \
        --out eustress/spaces/harness/RH1_sphere_grid

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH1_sphere_grid

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        histogram \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH1_exposure.json \
        --exposure-sweep -2,-1,0,1 \
        --patch grey_card_18 \
        --out docs/PROMPTS/artifacts/G3.04/exposure_response.json

Expected output shape:

    {
      "target_mid_grey_out": 118,
      "measured_mid_grey_out": 117.4,
      "abs_error": 0.6,
      "sweep": [
        { "stops": -2, "linear_luma": 0.0455 },
        { "stops": -1, "linear_luma": 0.0910 },
        { "stops":  0, "linear_luma": 0.1810 },
        { "stops":  1, "linear_luma": 0.3640 }
      ],
      "step_ratios": [2.00, 1.99, 2.01],
      "monotonic": true,
      "adaptation_locked": true,
      "post_stack_active": ["tonemapping"],
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    abs_error <= 3.0
      AND monotonic == true
      AND every value in step_ratios is within [1.7, 2.3]
      AND adaptation_locked == true
      AND the command exits 0

Verify by reading `abs_error`, `monotonic`, and each `step_ratios` entry.

## 6. Critic gate

Gated on **D2 (material and lighting realism)** and **D1 (first-three-seconds impact)**, floor
**8.0 each**. Either below floor fails the item. Capture recipe:
`docs/PROMPTS/harness/recipes/T1_RH1_exposure.json`.

D1 is included deliberately: exposure is the single largest lever on what a stranger concludes in
3.0 seconds, before any explanation. The Critic sees frames only — no result block, no commit
message, no caption — and must cite a frame index or a measured value for every score. Design the
change so a single still frame reads as correctly exposed, with highlights that roll off rather than
clip and shadows that retain separation.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — anchor mid-grey through the existing update_exposure_compensation
                 path, with Hdr added to the shared camera bundle
   -> if still failing, MANDATORY approach change. Nudging an exposure constant is NOT an approach
      change; registering AutoExposurePlugin with a locked metering mode is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  abs_error moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: adding Hdr reproduces the shared bind-group panic (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.04/exposure_response.json`

A reader finds: the declared mid-grey target and why it was chosen, the measured value and error,
the four-point sweep with per-step ratios, the adaptation-lock state and how to set it, the list of
post-stack components actually attached to `studio_camera_bundle` after the change, the re-captured
`G3.02` baseline digest, the commit, and the GPU and driver version.

## 9. Definition of NOT done

- Mid-grey lands correctly because a lift was added to the tonemap curve, so blacks are now milky.
  Check the shadow end of the histogram and report it.
- The sweep is monotonic but each stop moves luminance by 1.3×, so the control is not calibrated in
  stops and no lighting preset built on it will be predictable.
- `Hdr` is added to the editor camera but not the AI camera, so captures and the viewport diverge.
  Every measurement in this pack is taken through the AI camera; a divergence here silently
  invalidates the rest of the pack.
- Bloom is enabled "since it was already available" and the frame goes flat grey again. That is the
  exact reverted failure; reproducing it fails the item.
- Auto-exposure is registered without a lock, so two captures of the same static scene now differ
  and `G3.01`'s determinism guarantee is broken. Re-run `G3.01`'s determinism selftest and report
  the digest equality.
- The number passes on `RH1_sphere_grid` and `RH2_interior` is two stops dark. State what you
  measured in both.
````

---

## G3.05 — Shadow cascade quality and contact grounding

Item path: `docs/PROMPTS/items/G3.05_shadow-cascade-quality.md`

````markdown
---
id: G3.05
title: Shadow cascade quality and contact grounding
workload: W1
workload_secondary: []
phase: G3
depends_on: [G3.02, G1.12]
blocks: [G3.06, G3.11]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D2]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH3_shadow.json
artifact: docs/PROMPTS/artifacts/G3.05/shadow_quality.json
escalation: >
  If reaching the penumbra ratio requires a shadow technique whose measured added cost exceeds
  2.0 ms/frame at 3840x2160 on the harness reference GPU, STALL immediately rather than trading
  frame-time headroom (which G4.03 and G4.04 both depend on) for a D2 score.
status: DRAFT
notes: >
  Tier L: touches the shadow path and requires a full capture-and-Critic loop, so each iteration
  costs an engine build.
---

## 1. Objective

Shadows read as contact rather than as decals. In the exterior harness scene, penumbra width scales
with occluder-to-receiver distance, objects darken where they touch the ground, and the cascade
boundary is not visible as a hard line across the frame.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Shadow contact cues are checked by a
professional evaluator before almost anything else, which is why this item is gated on a measurement
rather than on taste.

**The shadow configuration that exists today.**
`eustress/crates/engine/src/plugins/lighting_plugin.rs:205-212` builds the sun's cascade config with
`num_cascades: 4`, `minimum_distance: 0.1`, `maximum_distance: sun_shadow_distance()`,
`first_cascade_far_bound: 90.0`, `overlap_proportion: 0.25`.

`sun_shadow_distance()` (`lighting_plugin.rs:118-131`) reads `EUSTRESS_SHADOW_DISTANCE` once through
a `OnceLock`, defaulting to **1000.0** meters (CONFIG DEFAULT). The `DirectionalLight` inserted at
`lighting_plugin.rs:222-229` uses `illuminance: lux::RAW_SUNLIGHT`, `shadow_maps_enabled: true`,
`shadow_depth_bias: 0.02`, `shadow_normal_bias: 1.8`. The same two bias defaults appear in
`eustress/crates/common/src/classes.rs:2224-2225` and are mirrored per-light in
`eustress/crates/engine/src/light_sync.rs:139-147`.

**There is no soft-shadow path.** Nothing in the tree implements percentage-closer soft shadows or a
contact-hardening term. Penumbra width is governed by the shadow map's filter kernel, which is
constant in texel space, so it does not vary with occluder distance. This capability is at 0%.

**Shadow-caster budgeting exists and is load-bearing.**
`eustress/crates/engine/src/light_cull.rs` (318 lines) provides `cull_lights_to_nearest` (registered
in `Update`) and `enforce_shadow_budget` (registered in `PostUpdate`, see
`lighting_plugin.rs:100-106`) — a hard shadow-caster cap that exists so a large import cannot
exhaust the GPU shadow atlas during load. Do not remove or bypass either. HLOD proxies beyond the
near ring are `NotShadowCaster` by design (`lighting_plugin.rs:112-117`).

**The scene.** `eustress/spaces/harness/RH3_exterior`, emitted by the `render-harness` generator. It
must contain, visible in one frame, an occluder 0.2 m above the receiving plane and an occluder
3.0 m above it, plus a vertical post crossing the first cascade boundary at 90 m. If `RH3_exterior`
does not yet contain these, extending the generator to add them is explicitly in scope; record the
regenerated seed and digest in your artifact.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run` — `cargo check` will not catch the pipeline registration failures this
item is most likely to produce.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/plugins/lighting_plugin.rs`
- `eustress/crates/engine/src/light_sync.rs`
- `eustress/crates/engine/assets/shaders/` — a new WGSL file is permitted if justified
- `eustress/crates/engine/src/bin/render_harness.rs` — only to add the two occluders and the
  cascade-crossing post to `RH3_exterior`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/light_cull.rs` — the shadow-caster cap is a load-time safety
  mechanism; weakening it to buy shadow quality is out of bounds
- `eustress/crates/render-probe/`
- `eustress/crates/engine/src/photoreal.rs` — the post-stack belongs to `G3.04` and `G4.02`
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Moving the occluders closer together, cropping the analysis region, lowering capture resolution,
  or re-seeding the harness are measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Measure the added frame cost; do not estimate it. The escalation trigger is a hard limit of
  2.0 ms/frame at 3840×2160 on the harness reference GPU, and `G4.01`'s `frametime` subcommand is
  how you measure it.
- Do not solve peter-panning by raising `shadow_depth_bias` alone. That detaches contact shadows,
  which is the worse defect — a blinded evaluator reads detached contact as objects floating.
- Do not introduce a technique that shimmers under motion. Temporal stability is a separately gated
  dimension owned by `G4.03`; a noisy penumbra will re-open this item.
- `EUSTRESS_SHADOW_DISTANCE` may be set in the measurement command, but the pass must also hold at
  the default 1000.0. Report both.

## 5. Exit criterion

### Criterion
In `RH3_exterior`, measured penumbra width at a 3.0 m occluder-to-receiver distance is at least
**2.5×** the width at 0.2 m, both measured from the same captured frame, and the cascade-boundary
luminance discontinuity across the 90 m post is **≤ 2/255**.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH3_exterior --seed 42 \
        --out eustress/spaces/harness/RH3_exterior

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH3_exterior

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        penumbra \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH3_shadow.json \
        --near-occluder-m 0.2 --far-occluder-m 3.0 \
        --cascade-seam-probe 90.0 \
        --out docs/PROMPTS/artifacts/G3.05/shadow_quality.json

    cargo run -p eustress-engine --release --bin render-harness -- \
        frametime \
        --space eustress/spaces/harness/RH3_exterior \
        --frames 3000 --warmup 300 \
        --out docs/PROMPTS/artifacts/G3.05/frametime_after.json

Expected output shape:

    {
      "near_occluder_distance_m": 0.2,
      "far_occluder_distance_m": 3.0,
      "near_penumbra_px": 4.1,
      "far_penumbra_px": 11.8,
      "ratio": 2.88,
      "cascade_seam_delta_255": 1.3,
      "contact_darkening_ratio": 0.62,
      "shadow_distance_m": 1000.0,
      "frame_sha256": "...", "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    ratio >= 2.5
      AND cascade_seam_delta_255 <= 2.0
      AND (frametime_after.ft_mean_ms - G4.01 baseline ft_mean_ms) <= 2.0
      AND both probe commands exit 0

Verify by reading `ratio`, `cascade_seam_delta_255`, and the two mean frame times. Do not infer
success from files appearing.

## 6. Critic gate

Gated on **D2 (material and lighting realism)**, floor **8.0**. Capture recipe:
`docs/PROMPTS/harness/recipes/T1_RH3_shadow.json`.

This item targets the distance-varying-penumbra cue but must not regress contact darkening — soft
shadows that no longer darken at the point of contact read as objects floating, a worse defect than
the one being fixed. The Critic sees frames only, never your self-report, and must cite a frame
index or a measured value for every score. Design the change so the improvement is legible in a
single still frame.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — blocker-search soft shadows in the existing directional shadow path
   -> if still failing, MANDATORY approach change. Tuning the kernel radius is NOT an approach
      change; moving from a fixed kernel to a variable-penumbra blocker search is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D2 moving < 0.5 AND ratio moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: added frame cost exceeds 2.0 ms/frame at 3840x2160 (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.05/shadow_quality.json`

A reader finds: both probe distances in meters, both measured penumbra widths in pixels, their
ratio, the cascade-seam delta, the contact-darkening ratio, the shadow distance the measurement was
taken at, the measured frame-time delta against the `G4.01` baseline, the frame digest, the commit,
and the GPU and driver version. Archived alongside: the capture bundle manifest and the D2
scorecard.

## 9. Definition of NOT done

- The ratio clears 2.5 but added frame cost exceeds 2.0 ms. That is a trade, not a fix, and the
  escalation trigger fires.
- The ratio clears 2.5 in a hand-built scene rather than in the generated `RH3_exterior`.
  Non-harness results are not admissible.
- Penumbra varies with distance but shimmers under camera motion. That fails a different gated
  dimension and the item will be re-opened by `G4.03`.
- Peter-panning is fixed by raising `shadow_depth_bias`, so contact shadows detach and objects read
  as floating.
- The cascade seam is hidden by raising `overlap_proportion` until the cascades cost twice as much.
  Check the frame-time delta before claiming the seam fixed.
- The pass holds at `EUSTRESS_SHADOW_DISTANCE=200` but not at the default 1000.0. The default is
  what ships; report both.
- `enforce_shadow_budget` was relaxed so more casters reach the atlas. That is out of scope and
  reintroduces a load-time GPU-memory risk.
````

---

## G4.02 — Anti-aliasing path that does not require MSAA

Item path: `docs/PROMPTS/items/G4.02_anti-aliasing-without-msaa.md`

````markdown
---
id: G4.02
title: Anti-aliasing path that does not require MSAA
workload: W1
workload_secondary: [W3]
phase: G4
depends_on: [G4.01, G3.04, G1.03, G1.05, G1.12]
blocks: [G3.06, G4.03]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D3, D2]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH3_aliasing.json
artifact: docs/PROMPTS/artifacts/G4.02/aliasing.json
escalation: >
  If the only AA configuration that reaches the edge-gradient floor also breaks G3.01's capture
  determinism (two captures of the same static pose no longer hash identically) and no lock or
  jitter-reset makes it deterministic again, STALL — determinism is a pack-wide prerequisite and
  may not be traded for an edge-quality score.
status: DRAFT
notes: >
  Tier L. The AA crates are already enabled in the manifest but inert; this item attaches a
  component to a camera and proves the result, so most of the cost is the capture-and-Critic loop.
---

## 1. Objective

Geometric edges in the exterior harness scene stop stair-stepping. A high-contrast diagonal roof
edge resolves across a measurable multi-pixel gradient rather than a hard one-pixel jump, at native
capture resolution, without MSAA — which the ambient-occlusion path in `G3.06` cannot coexist with.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Aliased edges are the single most common
reason a blinded evaluator dates a renderer, which is why this is gated on the motion dimension as
well as the material dimension.

**What is available and what is inert.** `eustress/crates/engine/Cargo.toml:133-134` enables the
bevy features `bevy_anti_alias` and `bevy_post_process` on Bevy 0.19. The manifest comment at
`Cargo.toml:128-133` states plainly: "DefaultPlugins registers these plugins but NO camera carries
the components, i.e. they are inert." So the plugins exist in the app; no camera opts in.

**No camera opts in today.** `studio_camera_bundle`
(`eustress/crates/engine/src/default_scene.rs:34`) ships `Camera3d::default()`,
`Tonemapping::TonyMcMapface`, a 70° perspective projection, `Instance`, `Name`, and `DepthPrepass`.
There is no AA component and no `Msaa` override anywhere in the tree — `grep -rn "Msaa::"` over
`eustress/crates` returns only comments in `photoreal.rs`, a comment in
`eustress/crates/common/src/plugins/lighting_plugin.rs:581`, and a WebGPU note in
`eustress/crates/engine/src/spawn.rs:1434`.

**Why MSAA is not the answer here.** `eustress/crates/engine/src/photoreal.rs:9-11` records that
GTAO is available in `bevy_pbr` but requires `Msaa::Off`, and that without TAA/FXAA that means
aliased edges. `G3.06` depends on this item precisely because it needs `Msaa::Off` to be survivable.

**The runtime hazard.** `photoreal.rs:19-23`: `Msaa` and `Hdr` must be set once and kept permanent —
toggling them at runtime changes the view bind-group shape and panics against
`SharedLightingPlugin`'s shared mesh-view bind-group layout — and any change must apply identically
to BOTH cameras, the editor camera (`default_scene.rs:136`) and the off-screen AI camera
(`ai_camera.rs:110-150`).

**The determinism constraint.** `G3.01` established that two captures of the same static pose in
`RH1_sphere_grid` are byte-identical, and every measurement in this pack rests on that. Temporal AA
accumulates across frames and is usually seeded by a frame-indexed jitter sequence. If you choose a
temporal technique, you must supply a deterministic reset or a fixed jitter phase for capture, and
re-prove `G3.01`'s selftest.

**What `G4.01` gave you.** The `frametime` subcommand emitting p50/p95/p99/max/1%-low over a stated
window, plus `docs/PROMPTS/artifacts/G4.01/frametime_baseline.json`, the number your added cost is
measured against.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/default_scene.rs` — `studio_camera_bundle` only
- `eustress/crates/engine/src/photoreal.rs`
- `eustress/crates/engine/Cargo.toml` — only if a bevy feature must be added, and say why

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/render-probe/`
- `eustress/crates/engine/src/bin/render_harness.rs` — the harness scenes are frozen for this item
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/ai_camera.rs` — owned by `G1.05` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- Any existing entry in `eustress/crates/engine/Cargo.toml` — the file is owned by `G1.03`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Rendering at 2× and downsampling into the capture, moving the edge probe to a lower-contrast
  region, softening the whole image, or lowering capture resolution are all measurement changes.
- Supersampling the capture path only is explicitly forbidden. The editor viewport must get the same
  AA the capture does; the capture exists to represent the product, not to flatter it.
- Set `Msaa` and `Hdr` once at spawn, in the shared bundle, never at runtime.
- Do not buy edge quality with blur. The exit criterion includes a texture-detail retention check
  precisely to catch a whole-image softening pass.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
On the high-contrast diagonal roof edge in `RH3_exterior`, the mean edge-transition width rises from
its baseline to **≥ 2.0 px** at 3840×2160, while a high-frequency texture patch elsewhere in the same
frame retains **≥ 92%** of its baseline local contrast, added frame cost is **≤ 1.5 ms** against
`G4.01`'s baseline mean, and `G3.01`'s capture determinism selftest still reports `identical: true`.

### Measurement

Command:

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH3_exterior

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH3_aliasing.json \
        --out docs/PROMPTS/artifacts/G4.02/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        histogram \
        --frame docs/PROMPTS/artifacts/G4.02/frames/RH3_000.png \
        --edge-probe roof_diagonal \
        --detail-patch brick_wall \
        --baseline docs/PROMPTS/artifacts/G3.02/pbr_reference_baseline.json \
        --out docs/PROMPTS/artifacts/G4.02/aliasing.json

    cargo run -p eustress-engine --release --bin render-harness -- \
        frametime --space eustress/spaces/harness/RH3_exterior \
        --frames 3000 --warmup 300 \
        --out docs/PROMPTS/artifacts/G4.02/frametime_after.json

    cargo run -p eustress-engine --release --bin render-harness -- \
        selftest-determinism --universe eustress/spaces/harness \
        --width 3840 --height 2160 \
        --position 6.0 3.5 9.0 --look-at 0.0 1.0 0.0 --settle-frames 30 \
        --out docs/PROMPTS/artifacts/G4.02/capture_determinism_recheck.json

Expected output shape:

    {
      "aa_technique": "TAA",
      "msaa": "Off",
      "hdr": true,
      "edge_transition_px": 2.6,
      "baseline_edge_transition_px": 1.1,
      "detail_contrast_retained_pct": 95.4,
      "frame_sha256": "...", "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    edge_transition_px >= 2.0
      AND detail_contrast_retained_pct >= 92.0
      AND (frametime_after.ft_mean_ms - G4.01 baseline ft_mean_ms) <= 1.5
      AND capture_determinism_recheck.identical == true
      AND every command exits 0

Verify by reading each emitted value and the exit codes.

## 6. Critic gate

Gated on **D3 (motion and temporal stability)** and **D2 (material and lighting realism)**, floor
**8.0 each**; either below floor fails. Capture recipe:
`docs/PROMPTS/harness/recipes/T1_RH3_aliasing.json`.

D3 is the primary gate because an AA technique that resolves a still edge while introducing ghosting
or crawl under motion is a net loss on the dimension that dominates a blind comparison. D2 is
included so that softening the whole frame — which would improve the edge number — is caught as a
material-detail regression. The Critic never sees your self-report and must cite a frame index or a
measured value for every score.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — spatial AA (FXAA or SMAA) on the shared camera bundle with Msaa::Off
   -> if still failing, MANDATORY approach change. Changing an AA quality preset is NOT an approach
      change; moving from a spatial filter to a temporal accumulation with a deterministic jitter
      phase is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  edge_transition_px moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the only qualifying configuration breaks capture determinism (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G4.02/aliasing.json`

A reader finds: the AA technique chosen and why, the `Msaa` and `Hdr` state now permanent on the
shared camera bundle, the measured and baseline edge-transition widths, the detail-contrast
retention, the measured frame-cost delta, the determinism re-check verdict, the frame digest, the
commit, and the GPU and driver version.

## 9. Definition of NOT done

- The edge number passes because the capture path renders at 2× and downsamples while the editor
  viewport still stair-steps. The product is the viewport.
- Edges resolve but text and thin gizmo lines in the studio overlay now shimmer. Say what you
  checked in the UI layer.
- A temporal technique passes the still-frame edge measurement and leaves trails behind moving
  geometry. `G4.03` will re-open the item; catch it here.
- Determinism is preserved by disabling the technique during capture. Then the measurement does not
  describe the product.
- `Msaa::Off` is set on the editor camera only, and the AI camera keeps a different view
  configuration, so every capture in this pack now measures a different renderer than the one
  shipping.
- The frame-cost delta is under 1.5 ms at 1920×1080 but 4 ms at 3840×2160. The criterion is stated
  at 3840×2160; measure there.
````

---

## G3.06 — Screen-space ambient occlusion under `Msaa::Off`

Item path: `docs/PROMPTS/items/G3.06_ssao-contact-grounding.md`

````markdown
---
id: G3.06
title: Screen-space ambient occlusion that grounds objects without haloing
workload: W1
workload_secondary: []
phase: G3
depends_on: [G4.02, G3.05, G1.05, G1.12]
blocks: [G3.12]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D2, D6]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH2_ao.json
artifact: docs/PROMPTS/artifacts/G3.06/ambient_occlusion.json
escalation: >
  If enabling the ambient-occlusion pass reintroduces the flat-grey wash recorded in
  eustress/crates/engine/Cargo.toml:128-133 even with exposure anchored by G3.04, STALL with the
  measured mid-grey drift rather than compensating with a second exposure offset.
status: DRAFT
notes: >
  Tier L. Depends on G4.02 because the AO path requires Msaa::Off, and on G3.05 so that AO is not
  used to paper over a missing contact shadow.
---

## 1. Objective

Objects sit in the interior harness scene instead of hovering over it. Concave corners, the
underside of furniture, and the meeting line between wall and floor darken measurably, while convex
silhouettes against the background gain no bright halo.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Ambient occlusion is in scope here because
contact grounding is the cue a blinded evaluator uses to decide whether geometry is in the scene or
pasted on it.

**Current state: 0% enabled.** `eustress/crates/common/src/plugins/lighting_plugin.rs:583` contains
the line `// bevy::pbr::ScreenSpaceAmbientOcclusion::default(),` — commented out, with the note at
`:581` that "SSAO requires Msaa::Off which conflicts with MSAA anti-aliasing."
`eustress/crates/engine/src/photoreal.rs:9-11` states GTAO "IS available but requires `Msaa::Off`,
which without TAA/FXAA means aliased edges — held pending a decision." That decision is now made:
`G4.02` landed an AA path that does not require MSAA and set `Msaa::Off` permanently on
`studio_camera_bundle`.

**The depth prepass you need already exists.** `studio_camera_bundle`
(`eustress/crates/engine/src/default_scene.rs:55-59`) carries `DepthPrepass`, added to the shared
bundle so the editor and AI cameras stay in lockstep against `SharedLightingPlugin`'s view
bind-group layout. Any additional prepass you require must be added in the same shared bundle for
the same reason.

**The runtime hazard.** `photoreal.rs:19-23`: `Msaa` and `Hdr` are permanent and must never be
toggled at runtime; changes apply identically to both cameras.

**Do not use AO as a substitute for shadow.** `G3.05` landed distance-varying penumbra and contact
darkening in the directional shadow path, measured in
`docs/PROMPTS/artifacts/G3.05/shadow_quality.json`. If your AO strength is tuned high enough to
carry contact on its own, the exterior scene will read as dirt rather than shadow and D6 will fail.

**The scene.** `eustress/spaces/harness/RH2_interior`, emitted by the `render-harness` generator.
It must contain at least: one interior corner where two walls and a floor meet, one object resting
directly on the floor with a visible underside, and one convex object silhouetted against a bright
background. Extending the generator to guarantee these is in scope.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/src/plugins/lighting_plugin.rs`
- `eustress/crates/engine/src/photoreal.rs`
- `eustress/crates/engine/src/default_scene.rs` — `studio_camera_bundle` only
- `eustress/crates/engine/src/bin/render_harness.rs` — only to guarantee the three `RH2_interior`
  features listed above

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/plugins/lighting_plugin.rs` — the shadow path is `G3.05`'s, and
  re-tuning it here makes both items unmeasurable
- `eustress/crates/render-probe/`
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/ai_camera.rs` — owned by `G1.05` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Moving the corner probe deeper into the corner, sampling the halo further from the silhouette, or
  reducing capture resolution are measurement changes.
- Do not compensate an overall darkening with an exposure offset. `G3.04` anchored mid-grey; the AO
  pass must leave that anchor within its stated tolerance, which is part of the pass condition.
- AO strength must be a named, discoverable setting, not a magic constant buried in a shader.
  `PhotorealSettings` (`photoreal.rs:30-42`) already carries a `gtao` flag; wire it truthfully so
  the flag reflects reality.
- Do not add a second full-resolution depth prepass. Use the one already in the shared bundle.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
In `RH2_interior`, mean luminance in the wall-floor corner probe drops by **≥ 25%** relative to the
adjacent open-wall probe, the bright halo measured just outside a convex silhouette rises by
**≤ 2/255** over its no-AO value, and the 18% mid-grey card stays within **±3/255** of the target
established in `G3.04`.

### Measurement

Command:

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH2_interior

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH2_ao.json \
        --out docs/PROMPTS/artifacts/G3.06/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        histogram \
        --frame docs/PROMPTS/artifacts/G3.06/frames/RH2_000.png \
        --probe corner_wall_floor --probe open_wall \
        --halo-probe convex_silhouette \
        --patch grey_card_18 \
        --exposure-baseline docs/PROMPTS/artifacts/G3.04/exposure_response.json \
        --out docs/PROMPTS/artifacts/G3.06/ambient_occlusion.json

Expected output shape:

    {
      "corner_luma": 74.2,
      "open_wall_luma": 108.9,
      "corner_darkening_pct": 31.9,
      "halo_delta_255": 0.8,
      "mid_grey_out": 117.9,
      "mid_grey_target": 118,
      "mid_grey_abs_error": 0.1,
      "ao_enabled": true,
      "msaa": "Off",
      "frame_sha256": "...", "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    corner_darkening_pct >= 25.0
      AND halo_delta_255 <= 2.0
      AND mid_grey_abs_error <= 3.0
      AND ao_enabled == true
      AND every command exits 0

Verify by reading each emitted value, not by observing that the pass ran.

## 6. Critic gate

Gated on **D2 (material and lighting realism)** and **D6 (overall coherence)**, floor **8.0 each**.
Capture recipe: `docs/PROMPTS/harness/recipes/T1_RH2_ao.json`.

D6 is included because ambient occlusion that visually diverges from the rest of the lighting model
— for instance heavy contact darkening indoors against unchanged exteriors, or AO that reads as
painted-on grime rather than as absent light — breaks coherence even where it improves realism
locally. The Critic sees frames only and must cite a frame index or a measured value for every
score.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — enable Bevy's screen-space ambient occlusion on the shared camera
                 bundle now that Msaa::Off is permanent
   -> if still failing, MANDATORY approach change. Turning the strength up is NOT an approach
      change; changing the occlusion estimator or its sampling radius policy is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  corner_darkening_pct moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the flat-grey wash returns with exposure anchored (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.06/ambient_occlusion.json`

A reader finds: the corner and open-wall luminances and their ratio, the halo delta at the convex
silhouette, the mid-grey value and its error against `G3.04`'s anchor, the AO technique and its
exposed setting name, the `Msaa` state, the frame digest, the commit, and the GPU and driver
version.

## 9. Definition of NOT done

- The corner darkens by 25% and so does everything else, because the AO term is really a global
  ambient reduction. Check the open-wall probe did not move.
- A bright halo appears around every object against the sky. That is the classic screen-space
  failure and it is more visible than the benefit.
- AO carries the contact cue and `G3.05`'s shadow work is now invisible under it. Both must read;
  say what you verified in the exterior scene.
- Mid-grey drifts two units and is "corrected" with an exposure offset, silently undoing `G3.04`.
- `PhotorealSettings.gtao` reports `true` while no pass is attached. A settings field that lies is
  worse than one that is absent.
- AO is enabled on the AI camera only, so captures show grounding the shipping viewport does not.
````

---

## G4.03 — Temporal stability under camera motion

Item path: `docs/PROMPTS/items/G4.03_temporal-stability-under-motion.md`

````markdown
---
id: G4.03
title: Temporal stability under camera motion — no ghosting, no crawl
workload: W1
workload_secondary: [W3]
phase: G4
depends_on: [G4.02, G4.01, G1.05, G1.12]
blocks: [G4.04, G3.12]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D3]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH3_dolly.json
artifact: docs/PROMPTS/artifacts/G4.03/temporal_stability.json
escalation: >
  If reaching the ghosting floor requires disabling the anti-aliasing landed by G4.02, STALL rather
  than reverting it — that would trade a gated D3 sub-property for another and leaves the program
  no better off. Report both measurements and request a decision.
status: DRAFT
notes: >
  Tier L. The first of two temporal-stability items in this pack; G4.04 covers pop at LOD
  transitions, this one covers per-pixel stability during continuous motion.
---

## 1. Objective

A camera dolly through the exterior harness scene produces a stable image. Static surfaces do not
crawl or shimmer between consecutive frames, moving geometry leaves no visible trail behind it, and
the frame-time distribution stays tight enough that motion reads as smooth rather than as a
sequence of frames.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. This item exists because temporal
instability is where most renderers visibly lose a blind comparison: a still frame can be tuned, but
twelve seconds of motion cannot be faked.

**What `G4.02` landed.** An anti-aliasing path on `studio_camera_bundle`
(`eustress/crates/engine/src/default_scene.rs:34`) that does not require MSAA, with `Msaa` and `Hdr`
now permanent on both the editor camera (`default_scene.rs:136`) and the off-screen AI camera
(`eustress/crates/engine/src/ai_camera.rs:110-150`). The technique chosen, and whether it is
temporal, is recorded in `docs/PROMPTS/artifacts/G4.02/aliasing.json` under `aa_technique`. Read that
file first — if the technique is temporal, ghosting is the expected failure mode and reprojection is
where you will spend your iterations.

**What `G4.01` landed.** A `frametime` subcommand emitting p50/p95/p99/max/1%-low over a stated
window, plus `docs/PROMPTS/artifacts/G4.01/frametime_baseline.json`. Before `G4.01` there was no
percentile computed anywhere in the tree — `eustress/crates/engine/src/profiler.rs` (702 lines)
attributes time to systems and phases, and
`eustress/crates/engine/src/frame_diagnostics.rs` (151 lines) only logs frames over one second by
default (`FrameTimeTracker::default()` calls `Self::new(1000)`).

**The motion path.** The dolly must be driven by the capture recipe, not by hand. The AI camera's
pose is set through the bridge method `ai_camera.set_pose`
(`eustress/crates/engine/src/engine_bridge/protocol.rs:2433`; params `position` `[x,y,z]` plus
`look_at` or `rotation`), dispatched at
`eustress/crates/engine/src/engine_bridge/mod.rs:404`. The bridge's TCP port is written to
`<universe>/.eustress/engine.port` (`engine_bridge/mod.rs:29`). A scripted, evenly-sampled dolly of
120 poses is reproducible; a hand-flown camera is not.

**A confound to control.** The render cascade
(`eustress/crates/common/src/streaming/render_cascade.rs`, registered at
`eustress/crates/common/src/streaming/plugin.rs:224`) switches entity `Visibility` on tier change
with a 16-frame cadence and hysteresis. That produces discrete pop, which is `G4.04`'s subject, not
yours. Keep the dolly inside a single tier band, or exclude tier-transition frames and say exactly
which frames were excluded and why — that exclusion is permitted only because a different gated item
owns it.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/photoreal.rs`
- `eustress/crates/engine/src/default_scene.rs` — `studio_camera_bundle` only
- `eustress/crates/engine/assets/shaders/` — a new WGSL file is permitted if justified
- `eustress/crates/render-probe/` — only to add the `--dolly` mode to the `temporal` subcommand, if
  `G3.02` did not already provide it; the metric definitions themselves are frozen

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/streaming/render_cascade.rs` — tier-transition pop is `G4.04`'s item
- `eustress/crates/engine/src/space/hlod.rs` — proxy swap is `G5.01`'s item
- `eustress/crates/engine/src/bin/render_harness.rs` — the harness scenes are frozen for this item
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/ai_camera.rs` — owned by `G1.05` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Slowing the dolly, shortening the sequence, excluding frames beyond the tier-transition exclusion
  stated above, lowering capture resolution, or blurring the comparison are all measurement changes.
  If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The dolly speed is fixed by the recipe and stated in meters per second in the artifact. A stability
  result at an unstated speed is meaningless.
- Do not fix ghosting by shortening the temporal history to the point that aliasing returns.
  `G4.02`'s edge-transition measurement is re-run as part of this item's pass condition.
- Do not fix crawl by adding a global blur under motion. The detail-retention check from `G4.02` is
  re-run at the dolly's midpoint frame.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
Over a 120-frame scripted dolly through `RH3_exterior` at a fixed 2.0 m/s, the 95th-percentile
per-pixel absolute difference between consecutive frames, restricted to statically-shaded surfaces,
is **≤ 6/255**; the trailing-edge ghost residual behind the moving foreground object decays to
**≤ 3/255** within **2 frames** of the object passing; `ft_p99_ms` is **≤ 1.5×** `ft_p50_ms`; and
`G4.02`'s edge-transition width at the midpoint frame is still **≥ 2.0 px**.

### Measurement

Command:

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH3_exterior

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH3_dolly.json \
        --dolly-frames 120 --dolly-speed-mps 2.0 \
        --out docs/PROMPTS/artifacts/G4.03/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        temporal \
        --frames docs/PROMPTS/artifacts/G4.03/frames/ \
        --static-mask-from-recipe docs/PROMPTS/harness/recipes/T1_RH3_dolly.json \
        --ghost-probe moving_pillar \
        --edge-probe roof_diagonal --edge-frame 60 \
        --out docs/PROMPTS/artifacts/G4.03/temporal_stability.json

    cargo run -p eustress-engine --release --bin render-harness -- \
        frametime --space eustress/spaces/harness/RH3_exterior \
        --frames 3000 --warmup 300 --dolly \
        --out docs/PROMPTS/artifacts/G4.03/frametime_dolly.json

Expected output shape:

    {
      "dolly_frames": 120,
      "dolly_speed_mps": 2.0,
      "static_interframe_p95_255": 4.7,
      "ghost_residual_255_at_frame_plus_1": 4.1,
      "ghost_residual_255_at_frame_plus_2": 2.2,
      "ghost_decay_frames": 2,
      "edge_transition_px_frame_60": 2.5,
      "ft_p50_ms": 12.0,
      "ft_p99_ms": 16.8,
      "ft_p99_over_p50": 1.40,
      "tier_transition_frames_excluded": [],
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    static_interframe_p95_255 <= 6.0
      AND ghost_decay_frames <= 2 AND ghost_residual_255_at_frame_plus_2 <= 3.0
      AND ft_p99_over_p50 <= 1.5
      AND edge_transition_px_frame_60 >= 2.0
      AND every command exits 0

Verify by reading each emitted value and the exit codes.

## 6. Critic gate

Gated on **D3 (motion and temporal stability)**, floor **8.0**. D3 carries hard numeric sub-floors;
the numbers in §5 are the entry price, not the argument. Capture recipe:
`docs/PROMPTS/harness/recipes/T1_RH3_dolly.json`.

The Critic evaluates a frame sequence, not a still, and never sees your self-report — every score it
gives cites a frame index or a measured value. Two properties therefore matter more than they might
seem: the sequence must be continuous (no dropped or duplicated frames in the bundle), and the
motion must be long enough that a trail has somewhere to persist. A technique that scores well on
120 frames but was tuned against the first 20 will be caught.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — improve reprojection and history rejection in the AA path landed by
                 G4.02
   -> if still failing, MANDATORY approach change. Changing a history blend weight is NOT an
      approach change; adding velocity-buffer-driven reprojection with a variance clamp is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D3 moving < 0.5 AND
                  static_interframe_p95_255 moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the only qualifying fix is to disable G4.02's AA (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G4.03/temporal_stability.json`

A reader finds: the dolly length and speed, the static-surface interframe p95, the ghost residual at
one and two frames after passage and the derived decay length, the re-measured edge-transition width
at the midpoint frame, the p50 and p99 frame times and their ratio, the list of any excluded
tier-transition frames with the reason, the recipe digest, the commit, and the GPU and driver
version.

## 9. Definition of NOT done

- The interframe number passes because the dolly was slowed to 0.5 m/s. Speed is fixed at 2.0 m/s
  and stated in the artifact.
- Ghosting decays in two frames on the pillar probe but a bright specular highlight smears for ten.
  Probe the worst case, not the convenient one.
- `ft_p99_over_p50` passes because the window included 2,900 idle frames. `G4.01`'s warm-up and
  window rules apply unchanged.
- Crawl is removed by softening the image, and `edge_transition_px_frame_60` is still ≥ 2.0 only
  because everything is blurred. Report the detail-retention check as well.
- The sequence has 120 files but three are duplicates of frame 59 because a capture failed silently.
  Check frame digests for accidental duplication.
- Tier-transition frames were excluded without stating which, so the exclusion cannot be audited.
  An unaudited exclusion is a measurement change.
````

---

## G3.07 — Reflections: environment probes and specular occlusion

Item path: `docs/PROMPTS/items/G3.07_reflections-and-specular-occlusion.md`

````markdown
---
id: G3.07
title: Reflections that respond to the room, not to the sky
workload: W1
workload_secondary: []
phase: G3
depends_on: [G3.03, G3.04, G1.05, G1.12]
blocks: [G3.08, G3.10]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D2]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH2_reflection.json
artifact: docs/PROMPTS/artifacts/G3.07/reflections.json
escalation: >
  If the only technique reaching the indoor/outdoor reflection separation is full screen-space
  reflections and its measured cost exceeds 2.5 ms/frame at 3840x2160 on the harness reference GPU,
  STALL and request a decision between the cost and a probe-only result — do not ship SSR over
  budget and do not quietly drop the criterion to a probe-only floor.
status: DRAFT
notes: >
  Tier L. The honest starting point is that Eustress has environment-map lighting and no
  screen-space reflection path at all; this item decides and lands one, measured.
---

## 1. Objective

A polished floor indoors reflects the room it is in rather than the sky outside it. In the interior
harness scene, the measured colour of the reflection on a smooth metallic panel matches the room's
dominant interior colour far more closely than it matches the exterior sky colour, and the reflection
darkens where the surface is occluded.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Reflection correctness is in scope because
a mirror that shows the wrong world is the fastest way for an evaluator to conclude the scene is not
being simulated.

**What exists today.** Environment-map lighting only.
`eustress/crates/common/src/plugins/lighting_plugin.rs:574` inserts an `EnvironmentMapLight`;
`eustress/crates/common/src/services/lighting.rs:264` documents a `GeneratedEnvironmentMapLight`
path for the procedural skybox and `:273` an `AtmosphereEnvironmentMapLight` path for dynamic
environment lighting. The module header at `common/src/plugins/lighting_plugin.rs:1-11` lists
"Realtime-filtered environment maps with AtmosphereEnvironmentMapLight" among the plugin's
responsibilities.

**What does not exist.** `grep -rn "ScreenSpaceReflections"` over `eustress/crates` returns nothing.
There is no screen-space reflection path, no local reflection probe with a bounded influence volume,
and no specular occlusion term. Reflections on every surface therefore come from one global
environment source, which is why an indoor surface reflects the sky. This capability is at 0%.

**Why this compounds the metallic columns.** `G3.03` established that specular response on the
metallic cells of `RH1_sphere_grid` is dominated by the environment map rather than by the
directional light, and recorded the per-cell `specular_peak_offset_px` and `fresnel_rim_ratio` in
`docs/PROMPTS/artifacts/G3.03/pbr_energy.json`. Changing the environment path can move those
numbers; re-run that probe and report any movement.

**Exposure is anchored and must stay anchored.** `G3.04` established an 18% mid-grey target and
recorded it in `docs/PROMPTS/artifacts/G3.04/exposure_response.json`. A brighter environment map is
not a reflection fix.

**A hazard.** `eustress/crates/engine/src/ai_camera.rs:133-149` — the AI camera carries
`NoAtmosphere` because two atmosphere cameras abort wgpu with a bind-group size mismatch. If your
approach adds a per-camera environment component, it must respect that opt-out and must be applied
identically to the editor camera (`eustress/crates/engine/src/default_scene.rs:136`) and the AI
camera, both of which are built from `studio_camera_bundle` (`default_scene.rs:34`).

**The scene.** `eustress/spaces/harness/RH2_interior` must contain a smooth metallic floor panel
(roughness ≤ 0.1, metallic 1.0) inside the room, a window opening onto the exterior sky, and a
strongly-coloured interior wall. Extending the `render-harness` generator to guarantee these is in
scope.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/src/plugins/lighting_plugin.rs`
- `eustress/crates/common/src/services/lighting.rs`
- `eustress/crates/engine/src/default_scene.rs` — `studio_camera_bundle` only
- `eustress/crates/engine/assets/shaders/` — a new WGSL file is permitted if justified
- `eustress/crates/engine/src/bin/render_harness.rs` — only to guarantee the three `RH2_interior`
  features listed above

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/material_sync.rs` — `G3.03` owns the BRDF
- `eustress/crates/render-probe/`
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/ai_camera.rs` — owned by `G1.05` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Moving the reflection probe, darkening the sky so the mismatch shrinks, roughening the panel, or
  reducing capture resolution are all measurement changes.
- Measure cost, do not estimate it. Use `G4.01`'s `frametime` subcommand against
  `docs/PROMPTS/artifacts/G4.01/frametime_baseline.json`.
- If you choose a screen-space technique, it must not shimmer under motion. `G4.03`'s
  `static_interframe_p95_255` is re-run as part of this item's pass condition, because a noisy
  reflection is a temporal defect wearing a spatial disguise.
- Do not compensate a reflection colour error by tinting `base_color` on the panel. That is a
  cosmetic fix to a physical error and will fail coherence elsewhere.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
On the smooth metallic floor panel in `RH2_interior`, the CIE ΔE between the measured reflection
colour and the room's dominant interior colour is **≤ 0.45×** the ΔE between that same measured
reflection and the exterior sky colour; the added frame cost is **≤ 2.5 ms** against `G4.01`'s
baseline mean; and `G4.03`'s `static_interframe_p95_255` on the dolly sequence is still **≤ 6/255**.

### Measurement

Command:

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH2_interior

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH2_reflection.json \
        --out docs/PROMPTS/artifacts/G3.07/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        histogram \
        --frame docs/PROMPTS/artifacts/G3.07/frames/RH2_000.png \
        --probe mirror_panel --probe interior_wall --probe sky_through_window \
        --delta-e mirror_panel:interior_wall,mirror_panel:sky_through_window \
        --out docs/PROMPTS/artifacts/G3.07/reflections.json

    cargo run -p eustress-engine --release --bin render-harness -- \
        frametime --space eustress/spaces/harness/RH2_interior \
        --frames 3000 --warmup 300 \
        --out docs/PROMPTS/artifacts/G3.07/frametime_after.json

Expected output shape:

    {
      "technique": "local reflection probe with bounded influence volume",
      "delta_e_to_interior": 6.2,
      "delta_e_to_sky": 21.9,
      "interior_over_sky_ratio": 0.283,
      "specular_occlusion_applied": true,
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    interior_over_sky_ratio <= 0.45
      AND (frametime_after.ft_mean_ms - G4.01 baseline ft_mean_ms) <= 2.5
      AND the re-run G4.03 static_interframe_p95_255 <= 6.0
      AND every command exits 0

Verify by reading `interior_over_sky_ratio`, the two frame-time means, and the re-run temporal value.

## 6. Critic gate

Gated on **D2 (material and lighting realism)**, floor **8.0**. Capture recipe:
`docs/PROMPTS/harness/recipes/T1_RH2_reflection.json`.

The Critic sees frames only and cites a frame index or a measured value for every score. Design the
change so that a single still frame of the interior shows a floor that belongs to its room. Note the
regression risk the Critic will look for: a probe that fixes the interior while making exteriors
reflect a stale indoor capture is a worse defect than the one being fixed, because it is visible in
the scene a stranger sees first.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — a local reflection probe with a bounded influence volume, authored
                 into the harness interior and blended against the global environment map
   -> if still failing, MANDATORY approach change. Re-baking the probe at a different resolution is
      NOT an approach change; adding a screen-space reflection pass with a probe fallback is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D2 moving < 0.5 AND
                  interior_over_sky_ratio moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: SSR is the only qualifying technique and it exceeds 2.5 ms (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.07/reflections.json`

A reader finds: the technique chosen and why, the three probe colours, both ΔE values and their
ratio, whether a specular occlusion term was applied, the measured frame-cost delta, the re-run
`G4.03` temporal value, any movement in `G3.03`'s per-cell specular numbers, the frame digest, the
commit, and the GPU and driver version.

## 9. Definition of NOT done

- The ratio passes because the sky was darkened until the two ΔE values converged. Measurement
  change; the item fails.
- The interior floor is correct and every exterior surface now reflects the indoor probe. Report the
  exterior scene measurement too.
- A screen-space pass lands and reflections boil under camera motion. `G4.03`'s number is in the
  pass condition for exactly this reason.
- Reflections improve but `G3.03`'s metallic-column specular peaks moved outside their recorded
  values, so the BRDF work is silently undone. Re-run that probe and report.
- The probe is baked once at startup and never updates, so opening a door changes nothing. State the
  update policy explicitly, even if the answer is "static, by design".
- Specular occlusion is claimed but the field is hardcoded `true` with no term in the shader.
````

---

## G3.08 — Volumetric atmosphere and fog coherence

Item path: `docs/PROMPTS/items/G3.08_volumetric-atmosphere-and-fog.md`

````markdown
---
id: G3.08
title: Volumetric atmosphere and fog that agree with the light
workload: W1
workload_secondary: []
phase: G3
depends_on: [G3.04, G3.07, G1.05, G1.12]
blocks: [G3.11]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D2, D6]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH3_atmosphere.json
artifact: docs/PROMPTS/artifacts/G3.08/atmosphere.json
escalation: >
  If enabling a volumetric pass on the editor camera reproduces the multi-camera atmosphere prepare
  race documented in ai_camera.rs:133-149 (wgpu abort, bind group descriptor size mismatch), STALL
  immediately with the abort text — do not resolve it by removing NoAtmosphere from the AI camera,
  which is the documented cause.
status: DRAFT
notes: >
  Tier L. VolumetricLight is already inserted on the sun; no volumetric fog volume exists anywhere
  in the tree, so the light marker currently has nothing to scatter through.
---

## 1. Objective

Distance reads as distance. In the exterior harness scene, aerial perspective desaturates and lifts
far geometry by a measured amount that increases monotonically with depth, and a shaft of light
through the scene is visibly scattered by the medium rather than being a painted overlay.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Atmosphere is in scope because depth cues
are how a viewer judges scale, and scale is what makes a simulated world legible.

**What exists today.** `eustress/crates/common/src/plugins/lighting_plugin.rs` imports
`bevy::pbr::{DistanceFog, FogFalloff}` and, on Bevy 0.19, `bevy::light::{Atmosphere as
BevyAtmosphere, Skybox, GlobalAmbientLight, SunDisk}` plus
`bevy::light::atmosphere::ScatteringMedium`. `SharedLightingPlugin::build` registers
`update_fog_settings` in `Update`. So distance fog and Bevy's atmosphere model are wired.

**What does not exist.** `grep -rn "VolumetricFog\|FogVolume"` over `eustress/crates` returns
nothing. The only volumetric symbol in the tree is `VolumetricLight`, imported at
`eustress/crates/engine/src/plugins/lighting_plugin.rs:17` and inserted on the sun entity at
`lighting_plugin.rs:235`. A `VolumetricLight` marker with no participating medium scatters through
nothing — the marker is currently inert. This capability is at 0%.

**The hard hazard, quoted from `eustress/crates/engine/src/ai_camera.rs:136-142`.** Two `Camera3d`s
both carrying Bevy `Atmosphere` hit a multi-camera atmosphere prepare-race: the atmosphere LUT bind
group is not ready for one view when `prepare_mesh_view_bind_groups` builds it, the transient bind
group is short by exactly the atmosphere bindings, and wgpu aborts with "bind group descriptor (21)
!= layout (24)". The AI camera therefore carries
`eustress_common::plugins::lighting_plugin::NoAtmosphere`, which also makes `SharedLightingPlugin`
skip the skybox attach for that camera. A prior attempt to drop the marker for an "identical look"
reproduced the abort. **Keep it.**

**The consequence for measurement you must handle honestly.** Because the AI camera opts out of
dynamic atmosphere, a capture taken through it does not automatically show what the editor viewport
shows. `ai_camera.rs:143-149` names the clean follow-up: attach an explicit `Skybox`, a static
`EnvironmentMapLight`, and a flat `AmbientLight` directly to the AI camera — none of those three
causes the race; only Bevy's dynamic `Atmosphere` does. Doing that is in scope for this item, and if
you do it you must state in the artifact exactly how capture parity was achieved and what still
differs.

**Exposure is anchored.** `docs/PROMPTS/artifacts/G3.04/exposure_response.json` holds the 18%
mid-grey target. Fog that lifts the whole frame is not atmosphere; it is an exposure regression.

**The scene.** `eustress/spaces/harness/RH3_exterior` must contain identical reference posts at
50 m, 200 m, and 800 m from the camera anchor, and a light shaft geometry through which the sun is
visible. Extending the `render-harness` generator to guarantee these is in scope.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/src/plugins/lighting_plugin.rs`
- `eustress/crates/engine/src/plugins/lighting_plugin.rs`
- `eustress/crates/common/src/services/lighting.rs`
- `eustress/crates/engine/src/bin/render_harness.rs` — only to add the three reference posts and the
  light-shaft geometry to `RH3_exterior`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- The `NoAtmosphere` opt-out on the AI camera — removing it is the documented cause of a hard abort
- `eustress/crates/render-probe/`
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/ai_camera.rs` — owned by `G1.05` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Moving the reference posts, changing their albedo, sampling only the nearest two, or reducing
  capture resolution are all measurement changes.
- Fog density is a scene property, not a global constant baked into the plugin. Whatever you land
  must be settable per Space through the existing lighting service surface, because `G3.11` builds
  presets on top of it.
- Do not lift blacks globally to fake aerial perspective. The mid-grey anchor and the shadow end of
  the histogram are both checked.
- Measure cost with `G4.01`'s `frametime`; volumetrics are the most common way to lose the frame
  budget this pack has been protecting.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
In `RH3_exterior`, the measured saturation of identical reference posts decreases monotonically with
distance across 50 m, 200 m, and 800 m, with the 800 m post at **≤ 0.55×** the saturation of the
50 m post; the light shaft's measured intensity falls off with distance from the source rather than
being constant; the 18% mid-grey card stays within **±3/255** of `G3.04`'s target; and added frame
cost is **≤ 2.0 ms** against `G4.01`'s baseline mean.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH3_exterior --seed 42 \
        --out eustress/spaces/harness/RH3_exterior

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH3_exterior

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH3_atmosphere.json \
        --out docs/PROMPTS/artifacts/G3.08/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        histogram \
        --frame docs/PROMPTS/artifacts/G3.08/frames/RH3_000.png \
        --probe post_50m --probe post_200m --probe post_800m \
        --shaft-probe god_ray --patch grey_card_18 \
        --exposure-baseline docs/PROMPTS/artifacts/G3.04/exposure_response.json \
        --out docs/PROMPTS/artifacts/G3.08/atmosphere.json

    cargo run -p eustress-engine --release --bin render-harness -- \
        frametime --space eustress/spaces/harness/RH3_exterior \
        --frames 3000 --warmup 300 \
        --out docs/PROMPTS/artifacts/G3.08/frametime_after.json

Expected output shape:

    {
      "saturation": { "post_50m": 0.71, "post_200m": 0.55, "post_800m": 0.33 },
      "monotonic_desaturation": true,
      "far_over_near_saturation": 0.465,
      "shaft_intensity_falloff_monotonic": true,
      "mid_grey_abs_error": 0.9,
      "capture_parity_note": "AI camera carries explicit Skybox + static EnvironmentMapLight + flat AmbientLight; dynamic Atmosphere remains editor-only",
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    monotonic_desaturation == true
      AND far_over_near_saturation <= 0.55
      AND shaft_intensity_falloff_monotonic == true
      AND mid_grey_abs_error <= 3.0
      AND (frametime_after.ft_mean_ms - G4.01 baseline ft_mean_ms) <= 2.0
      AND every command exits 0

Verify by reading each emitted value and the exit codes.

## 6. Critic gate

Gated on **D2 (material and lighting realism)** and **D6 (overall coherence)**, floor **8.0 each**.
Capture recipe: `docs/PROMPTS/harness/recipes/T1_RH3_atmosphere.json`.

D6 is the sharp one here. Atmosphere that does not agree with the sun's direction, or a fog colour
that does not match the sky it is supposed to be scattering, reads as two systems that never met —
exactly what D6 penalises. The Critic sees frames only, never your self-report, and cites a frame
index or a measured value for every score.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — drive DistanceFog / FogFalloff from the atmosphere's own scattering
                 parameters so fog colour and sky colour cannot diverge
   -> if still failing, MANDATORY approach change. Tuning a falloff constant is NOT an approach
      change; adding a participating medium the existing VolumetricLight marker can scatter through
      is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  far_over_near_saturation moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the multi-camera atmosphere prepare race returns (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.08/atmosphere.json`

A reader finds: the three post saturations and their ratio, the monotonicity verdicts for
desaturation and shaft falloff, the mid-grey error against `G3.04`, the measured frame-cost delta,
an explicit capture-parity note stating what the AI camera now carries and what still differs from
the editor viewport, the frame digest, the commit, and the GPU and driver version.

## 9. Definition of NOT done

- Far geometry desaturates because a grey overlay was composited at a fixed depth. Check that the
  800 m post and the 200 m post differ, not just near versus far.
- The light shaft is a textured quad. State how the shaft is produced; a painted shaft fails D6 on
  inspection because it does not move correctly with the camera.
- Atmosphere looks right in the editor viewport and absent in every capture, because the AI camera's
  `NoAtmosphere` opt-out was never addressed. Then no evidence in this pack shows the feature.
- `NoAtmosphere` was removed to get parity and the build now aborts in wgpu on some machines. That is
  the documented failure; reproducing it fails the item.
- Fog density is hardcoded in the plugin, so `G3.11`'s presets cannot vary it and that item stalls.
- The frame-cost delta is measured on the interior scene where there is no visible atmosphere.
  Measure on `RH3_exterior`, where the pass is claimed.
````

---

## G4.04 — Render-cascade tier-transition pop suppression

Item path: `docs/PROMPTS/items/G4.04_render-cascade-pop-suppression.md`

````markdown
---
id: G4.04
title: Render-cascade tier transitions that are not visible as pop
workload: W1
workload_secondary: [W3]
phase: G4
depends_on: [G4.03, G1.12]
blocks: [G5.01]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D3, D6]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH4_cascade_dolly.json
artifact: docs/PROMPTS/artifacts/G4.04/tier_pop.json
escalation: >
  If suppressing pop requires holding both tiers resident simultaneously in a way that pushes the
  live entity count past the Active tier LRU cap of 20000, STALL — the caps exist to bound
  per-frame cost and raising one to win a temporal score moves the failure into G5's phase rather
  than fixing it.
status: DRAFT
notes: >
  Tier L. The cascade code is real and registered; the doc that describes it is still headed
  "Wave 1 SPEC ONLY" and understates the implementation. Read the code, not the doc.
---

## 1. Objective

Driving toward and away from distant geometry produces no visible snap. When an entity crosses a
render-cascade tier boundary, the per-frame luminance change in its screen region stays within the
same bound as an ordinary motion frame, so a viewer cannot tell from the image where the boundary
is.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Streaming pop is in scope because a world
that visibly assembles itself as you approach reads as a level rather than as a place.

**The cascade is real code, not a spec.**
`eustress/crates/common/src/streaming/render_cascade.rs` is 736 lines and is registered at
`eustress/crates/common/src/streaming/plugin.rs:224`. Its module header states the design directly:

- Each streamed entity gets exactly one `RenderTier` — Hero / Active / Streamed / Horizon — assigned
  on a **16-frame cadence** from camera distance with **hysteresis**.
- Distance bands: Hero out to 100 m, Active to 600 m, Streamed to 5500 m, despawned beyond.
- LRU caps per tier: **2000 / 20000 / 200000** (`TierCaps`).
- `sys_render_cascade` is the tier switcher; `sys_apply_tier_change` is the `Changed<RenderTier>`
  reactor and it changes **visual components only** — `Visibility` plus a `MeshLodTier` marker.
- The header is explicit that it must **never** touch `RigidBody` or `Collider`, because an LOD
  demotion that removes a collider while leaving a `RigidBody::Dynamic` makes the Avian solver run a
  body with no shape. That constraint is absolute and is repeated in §4 below.

Deferred to later waves, per the same header: impostor and panorama swap, the
`ClassName::lod_components(tier)` bundle wiring, `MeshLodCache`, the shadow-caster cap, telemetry,
and the Horizon skybox layer.

**Why pop happens.** `sys_apply_tier_change` toggles `Visibility`. A boolean visibility change is a
step function: the entity is fully drawn on one frame and fully absent on the next. Hysteresis
prevents oscillation at the boundary; it does not soften the transition itself. There is no
cross-fade, no dither, and no alpha ramp anywhere in the module.

**The documentation is behind the code.** `docs/architecture/RENDER_CASCADE.md` still heads its
section "Wave 1 SPEC ONLY". Do not treat that heading as a statement about the implementation. If
you change behaviour, correct the document so it reads as though it was always accurate — no
changelog residue, no "previously" commentary in the body.

**What `G4.03` gave you.** A scripted-dolly capture mode and the `temporal` probe subcommand, plus
`docs/PROMPTS/artifacts/G4.03/temporal_stability.json`, whose `static_interframe_p95_255` is the
bound this item's transition frames must live inside. `G4.03` explicitly excluded tier-transition
frames from its own measurement and named them; those are exactly the frames you now own.

**The scene.** `eustress/spaces/harness/RH4_cascade` — a corridor of identical structures placed so
that a straight-line dolly crosses the Hero→Active boundary at 100 m and the Active→Streamed
boundary at 600 m with geometry on screen at both moments. Extending the `render-harness` generator
to produce it is in scope.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/src/streaming/render_cascade.rs`
- `eustress/crates/common/src/streaming/plugin.rs` — registration only
- `eustress/crates/engine/assets/shaders/` — a new WGSL file is permitted if justified
- `eustress/crates/engine/src/bin/render_harness.rs` — only to add the `RH4_cascade` scene
- `docs/architecture/RENDER_CASCADE.md` — only to make the document match the shipped behaviour

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/space/hlod.rs` — proxy swap is `G5.01`'s item
- Anything under `eustress/crates/common/src/physics/` and any `RigidBody` or `Collider` component
  anywhere — see §4
- `eustress/crates/render-probe/`

## 4. Approach constraints

- **`sys_apply_tier_change` must continue to touch visual components only.** Adding or removing a
  `Collider` or changing a `RigidBody` on tier change makes the Avian solver run a body with no
  shape. This is stated in the module's own header as the primary risk for the file. Violating it
  fails the item regardless of the visual result.
- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Widening the distance bands so the dolly never crosses one, raising the cadence so transitions
  land between captured frames, excluding transition frames, or lowering capture resolution are all
  measurement changes.
- Do not raise the LRU caps. `TierCaps` at 2000 / 20000 / 200000 bounds per-frame cost; the
  escalation trigger fires if your approach needs more residency than that.
- Whatever you land must be frame-rate independent. A fade defined in frames rather than seconds
  will look different on every machine and will fail on the harness reference GPU at a different
  frame rate than the one you tuned on.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
Over a scripted dolly through `RH4_cascade` that crosses both the 100 m and 600 m boundaries, the
maximum per-frame luminance change within the transitioning entity's screen region is
**≤ 8/255** — at most 1.33× `G4.03`'s measured `static_interframe_p95_255` bound of 6/255 — with no
single frame in which the region's mean luminance changes by more than **12/255**, and the live
entity count never exceeds the Active tier cap of 20000.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH4_cascade --seed 42 \
        --out eustress/spaces/harness/RH4_cascade

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH4_cascade

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH4_cascade_dolly.json \
        --dolly-frames 400 --dolly-speed-mps 8.0 \
        --out docs/PROMPTS/artifacts/G4.04/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        tier-pop \
        --frames docs/PROMPTS/artifacts/G4.04/frames/ \
        --boundary-m 100 --boundary-m 600 \
        --region-from-recipe docs/PROMPTS/harness/recipes/T1_RH4_cascade_dolly.json \
        --out docs/PROMPTS/artifacts/G4.04/tier_pop.json

Expected output shape:

    {
      "boundaries_crossed": [100, 600],
      "max_region_interframe_luma_delta_255": 6.9,
      "worst_frame_index": 187,
      "max_region_mean_luma_step_255": 9.4,
      "peak_live_entities": 14820,
      "active_tier_cap": 20000,
      "transition_technique": "screen-door dither ramp over 0.25 s",
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    max_region_interframe_luma_delta_255 <= 8.0
      AND max_region_mean_luma_step_255 <= 12.0
      AND peak_live_entities <= active_tier_cap
      AND both commands exit 0

Verify by reading each emitted value and the exit codes.

## 6. Critic gate

Gated on **D3 (motion and temporal stability)** and **D6 (overall coherence)**, floor **8.0 each**.
Capture recipe: `docs/PROMPTS/harness/recipes/T1_RH4_cascade_dolly.json`.

D6 is included because a transition technique that reads differently from the rest of the scene — a
dither pattern that is visible as a pattern, or a fade that only some object classes receive —
breaks coherence even where it removes pop. The Critic evaluates a sequence, never sees your
self-report, and must cite a frame index or a measured value for every score. The frame index in
`worst_frame_index` is where it will look first.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — a time-based alpha or dither ramp applied by sys_apply_tier_change
                 instead of a boolean Visibility toggle
   -> if still failing, MANDATORY approach change. Lengthening the ramp is NOT an approach change;
      switching to a dual-resident cross-dissolve between two tier representations is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D3 moving < 0.5 AND
                  max_region_interframe_luma_delta_255 moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: suppression requires exceeding the Active tier LRU cap (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G4.04/tier_pop.json`

A reader finds: which boundaries the dolly crossed, the maximum per-frame luminance delta in the
transitioning region and the frame index where it occurred, the maximum mean-luminance step, the
peak live entity count against the Active tier cap, the transition technique and its duration in
seconds, the dolly speed, the recipe digest, the commit, and the GPU and driver version.

## 9. Definition of NOT done

- Pop disappears because the dolly speed was lowered until the ramp had time to finish. Speed is
  fixed by the recipe and stated in the artifact.
- The ramp is defined in frames, so it takes four times as long on a slow machine and is invisible
  on a fast one. State the duration in seconds and how frame-rate independence is achieved.
- The transition is smooth but a dither pattern is visible as a pattern on the transitioning
  surface. That is a new artifact, not a fix.
- `sys_apply_tier_change` now inserts or removes a `Collider`. The item fails on the module's own
  primary risk regardless of the visual score.
- The live entity count passes because the harness scene is too small to stress the caps. State the
  scene's total entity count so the headroom claim can be judged.
- `docs/architecture/RENDER_CASCADE.md` is left describing behaviour that no longer matches the
  code, or is edited with changelog residue in the body instead of reading as though always correct.
````

---

## G5.01 — HLOD proxy fidelity at the swap boundary

Item path: `docs/PROMPTS/items/G5.01_hlod-proxy-fidelity.md`

````markdown
---
id: G5.01
title: HLOD proxy fidelity at the swap boundary
workload: W1
workload_secondary: [W3]
phase: G5
depends_on: [G4.04, G1.12]
blocks: [G3.13]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D3, D2]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH5_hlod_swap.json
artifact: docs/PROMPTS/artifacts/G5.01/hlod_fidelity.json
escalation: >
  If reaching the silhouette-error floor requires proxy meshes whose combined memory exceeds
  1.5x the memory of the individual parts they replace, STALL — an HLOD proxy that costs more than
  the geometry it stands in for defeats the reason the system exists.
status: DRAFT
notes: >
  Tier L. hlod.rs is 1391 lines of working code with a build-once/persist/visibility-toggle
  lifecycle; this item changes what the proxy looks like at the moment of the swap, not the
  lifecycle.
---

## 1. Objective

Crossing the HLOD boundary does not change what the world looks like. At the residency boundary, a
merged far-cell proxy and the individual near parts it replaces produce the same silhouette and the
same mean colour to within a measured bound, so the swap is invisible in the image even though it is
a large change in the entity count.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Whole-map visibility matters because a
substrate that can only render what is within 350 m of the camera cannot represent a city, a site,
or a plant.

**How HLOD actually works today.** `eustress/crates/engine/src/space/hlod.rs` is 1391 lines and its
module header is precise:

- Residency spawns every visible binary part (`BinaryEcsInstance`) as a full Bevy entity inside
  `load_radius` (about 350 m) and despawns everything beyond it.
- HLOD renders the ring from `load_radius` out to `hlod_radius` as **one merged mesh per Morton
  cell** — a proxy — instead of thousands of individual entities.
- The boundary uses the same shared cell math residency loads in, `chunk_size = 256`, so a cell's
  geometry is EITHER individuals OR one proxy, never both.
- Lifecycle is **build-once / persist / visibility-toggle**: cells are enumerated once on Space load
  from the DB's actual non-empty cells, each cell is merged once by a background task, and a proxy
  is never despawned for camera position. Crossing far→near **hides** the proxy (residency then
  spawns individuals); near→far re-shows it.
- HLOD proxies are `NotShadowCaster`
  (`eustress/crates/engine/src/plugins/lighting_plugin.rs:112-117`), which is why the sun's cascade
  distance can be bounded.

The lifecycle is sound and is **not** what this item changes. What this item changes is the visual
agreement between a proxy and the individuals it stands in for at the instant of the hide/show
toggle.

**Environment tunables that exist.** `EUSTRESS_HLOD_RADIUS` controls the proxy ring's outer bound;
`EUSTRESS_RESIDENCY_*` variables control the near ring. Both may be set in the measurement command;
the pass must also hold at defaults, and both settings must be reported.

**What `G4.04` gave you.** A tier-transition measurement discipline and the `tier-pop` probe
subcommand, plus `docs/PROMPTS/artifacts/G4.04/tier_pop.json`. `G4.04` covered the render cascade's
`Visibility` toggle. This item covers the HLOD proxy toggle, which is a different code path in a
different crate and has a different failure mode: the cascade's failure is a step in opacity, HLOD's
failure is a step in *geometry and colour*.

**The scene.** `eustress/spaces/harness/RH5_hlod` — a dense district of several thousand parts
occupying at least four adjacent Morton cells, arranged so a straight-line dolly crosses the
residency boundary with the district filling a large part of the frame. Extending the
`render-harness` generator to produce it is in scope. State the part count in the artifact.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/space/hlod.rs`
- `eustress/crates/engine/src/mesh_optimizer.rs`
- `eustress/crates/engine/src/bin/render_harness.rs` — only to add the `RH5_hlod` scene

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/streaming/render_cascade.rs` — `G4.04` owns it
- The build-once / persist lifecycle in `hlod.rs`. Changing a proxy to despawn on camera position
  reintroduces per-frame merge cost and is out of bounds for this item.
- `eustress/crates/render-probe/`
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Moving the boundary so the district is off-screen when it crosses, shrinking the district,
  lowering capture resolution, or comparing at a distance where the district is a few pixels wide
  are all measurement changes.
- Do not fix the swap by making the near ring larger. Raising `load_radius` hides the defect and
  costs live-entity budget, which is exactly what HLOD exists to save.
- The proxy must remain `NotShadowCaster`. Making proxies cast shadows to improve their appearance
  re-inflates the sun cascade's caster set, which `eustress/crates/engine/src/light_cull.rs` and the
  bounded `sun_shadow_distance()` were built to avoid.
- Proxy build remains a background task with a cap. An approach that merges on the main thread will
  produce a load-time stall that `G4.01`'s p99 will catch.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
Across the residency boundary crossing in `RH5_hlod`, the silhouette intersection-over-union between
the proxy-rendered frame and the individuals-rendered frame of the same district from the same pose
is **≥ 0.93**, the mean colour difference over the district's screen region is **≤ 6/255**, and the
maximum per-frame luminance change in that region during the swap is **≤ 8/255**.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH5_hlod --seed 42 \
        --out eustress/spaces/harness/RH5_hlod

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH5_hlod

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH5_hlod_swap.json \
        --dolly-frames 300 --dolly-speed-mps 12.0 \
        --out docs/PROMPTS/artifacts/G5.01/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        silhouette \
        --frames docs/PROMPTS/artifacts/G5.01/frames/ \
        --region-from-recipe docs/PROMPTS/harness/recipes/T1_RH5_hlod_swap.json \
        --pair proxy_side:individuals_side \
        --out docs/PROMPTS/artifacts/G5.01/hlod_fidelity.json

Expected output shape:

    {
      "district_part_count": 5184,
      "cells_covered": 4,
      "silhouette_iou": 0.951,
      "mean_colour_delta_255": 4.2,
      "max_region_interframe_luma_delta_255": 6.1,
      "worst_frame_index": 143,
      "proxy_memory_bytes": 18342912,
      "individuals_memory_bytes": 41220096,
      "proxy_over_individuals_memory": 0.445,
      "hlod_radius_m": 2000.0,
      "load_radius_m": 350.0,
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    silhouette_iou >= 0.93
      AND mean_colour_delta_255 <= 6.0
      AND max_region_interframe_luma_delta_255 <= 8.0
      AND proxy_over_individuals_memory <= 1.5
      AND both commands exit 0

Verify by reading each emitted value and the exit codes.

## 6. Critic gate

Gated on **D3 (motion and temporal stability)** and **D2 (material and lighting realism)**, floor
**8.0 each**. Capture recipe: `docs/PROMPTS/harness/recipes/T1_RH5_hlod_swap.json`.

D2 is included because the most common way a merged proxy fails is colour: merging thousands of
individually-materialled parts into one mesh tends to average them into a flat grey mass, which
reads as a texture-less blob at distance even when its silhouette is perfect. The Critic sees frames
only, never your self-report, and cites a frame index or a measured value for every score;
`worst_frame_index` is where it will look first.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — preserve per-part material identity in the merged proxy (vertex colour
                 or a small merged material set) rather than collapsing to one material
   -> if still failing, MANDATORY approach change. Changing the decimation ratio is NOT an approach
      change; adding a brief dual-resident cross-dissolve at the hide/show moment is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  silhouette_iou moving < 0.01
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: proxy memory exceeds 1.5x the individuals it replaces (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G5.01/hlod_fidelity.json`

A reader finds: the district's part count and cell coverage, the silhouette IoU, the mean colour
delta, the worst per-frame luminance change during the swap and its frame index, proxy and
individuals memory with their ratio, the `hlod_radius` and `load_radius` the measurement was taken
at, the recipe digest, the commit, and the GPU and driver version.

## 9. Definition of NOT done

- IoU passes because the district was measured at 900 m where it is 40 pixels wide. Measure at the
  boundary crossing, where the swap actually happens.
- Silhouette matches and the proxy is a uniform grey slab. The colour delta exists to catch this;
  report both.
- The swap is invisible because `load_radius` was raised until the proxy never shows on screen. That
  is not an HLOD fix, it is HLOD avoidance.
- Proxies now cast shadows, so the district looks better and the sun cascade's caster set is back to
  where the shadow-distance bound was introduced to fix it.
- Proxy build moved to the main thread and the p99 frame time on Space load doubled. Re-run
  `G4.01`'s `frametime` on load and report it.
- The proxy is rebuilt every time the camera re-enters the cell, defeating the build-once lifecycle.
  State the build count over the dolly.
````

---

## G3.09 — Material service: `.mat.toml` authoring round-trip

Item path: `docs/PROMPTS/items/G3.09_material-service-round-trip.md`

````markdown
---
id: G3.09
title: Material service authoring round-trip from file to pixels and back
workload: W6
workload_secondary: [W1]
phase: G3
depends_on: [G3.03, G1.12]
blocks: [G5.02, G3.11]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D2, D4]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH1_material_roundtrip.json
artifact: docs/PROMPTS/artifacts/G3.09/material_roundtrip.json
escalation: >
  If a property cannot be written back to its .mat.toml because the Properties panel does not
  persist edits in the default build, STALL and name the property — do not route the write-back
  through a cargo feature that is off by default and then claim the round-trip works.
status: DRAFT
notes: >
  Tier L. The .mat.toml parser and registry are real and substantial; the gap is the closed loop
  from a panel edit back to the file, and the fidelity of the loop's pixels.
---

## 1. Objective

A material authored as a file becomes exactly those pixels, and an edit made in the studio becomes
exactly that file. Editing a `.mat.toml` value on disk changes the rendered surface by the expected
amount within one hot-reload, and editing the same value in the Properties panel writes it back to
the same file in the default build, with no cargo feature required.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Materials are the clearest case of that duality: the same `.mat.toml` is
both the document and the model input.

**What already exists, and it is substantial.**
`eustress/crates/engine/src/space/material_loader.rs` (766 lines) parses `.mat.toml` files from a
Space's `MaterialService/` folder into `StandardMaterial` handles. Concretely:

- `MaterialDefinition` is the parsed `.mat.toml` structure (`material_loader.rs:24`).
- `MaterialRegistry` (`:119`) is populated on Space load from `MaterialService/*.mat.toml` and keeps
  a name → source path map explicitly "for writeback and hot-reload" (`:124`).
- Public API includes `load_material_definition` (`:421`),
  `load_material_definition_from_str` (`:429`), `material_name_from_path` (`:435`),
  `build_standard_material` (`:449`), and registry accessors `get`, `insert`, `remove`,
  `get_definition`, `names`, `len`.
- `build_standard_material` loads six texture slots: `base_color`, `normal`,
  `metallic_roughness`, `emissive`, `occlusion`, `depth` (`:515-540`).
- `eustress/crates/engine/src/material_sync.rs` (333 lines) runs
  `reapply_materials_on_registry_change`, `set_material_textures_to_repeat`, and
  `sync_basepart_to_material` each `Update`, chained.

`eustress/crates/engine/src/pbr_materials.rs` is a 14-line `TODO` stub and registers nothing; ignore
its name. The design document is `docs/development/MATERIAL_SERVICE_ARCHITECTURE.md`.

**The honest gap.** `docs/AUDIT/02_STUDIO_ENGINE.md` records that the Properties panel **does not
persist edits in the default build** — legacy TOML write-back sits behind an opt-in `toml` cargo
feature, and the Fjall mirror writes only `Transform`. So the file→pixels direction is real and the
pixels→file direction is not. That asymmetry is what this item closes.

**A rendering constraint you inherit.** `G3.03` established energy-correct PBR on
`RH1_sphere_grid` and recorded per-cell values in
`docs/PROMPTS/artifacts/G3.03/pbr_energy.json`. A round-trip that changes those values has changed
the material pipeline, not just its plumbing; re-run that probe and report any movement.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/space/material_loader.rs`
- `eustress/crates/engine/src/material_sync.rs`
- `eustress/crates/engine/src/properties.rs`
- `eustress/crates/engine/src/bin/render_harness.rs` — only to add a `MaterialService/` folder with
  the round-trip test material to `RH1_sphere_grid`
- `docs/development/MATERIAL_SERVICE_ARCHITECTURE.md` — only to make the document match shipped
  behaviour, written so it reads as though always correct

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/ui/slint_ui.rs` — at 23,103 lines it is the single largest file in the
  tree and the pack's biggest structural liability; the drain path is another pack's item. If the
  round-trip genuinely requires a change there, that is an escalation, not a licence.
- `eustress/crates/render-probe/`
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Testing the round-trip on a property that happens to already work, reducing the property set, or
  measuring the file rather than the pixels are all measurement changes.
- The round-trip must work in the **default build**. `cargo run -p eustress-engine --release` with
  no extra features is the only configuration that counts. A result behind `--features toml` fails.
- Write-back must preserve the file. Comments, key ordering, and untouched sections in the
  `.mat.toml` survive a write of one value. A rewrite that reformats the file is data loss to the
  person who authored it.
- Hot-reload must not require a Space reload. The registry already keeps the source path for this
  purpose.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
For each of six `.mat.toml` properties — `base_color`, `metallic`, `perceptual_roughness`,
`emissive`, `reflectance`, and one texture slot path — a disk edit changes the rendered surface
within **2.0 s** with the measured pixel change matching the expected direction and magnitude, and a
Properties-panel edit of the same property in the **default build** writes the value back to the
same file with byte-for-byte preservation of every other line.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH1_sphere_grid --seed 42 --with-material-service \
        --out eustress/spaces/harness/RH1_sphere_grid

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH1_sphere_grid

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        roundtrip \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH1_material_roundtrip.json \
        --material MaterialService/RoundTrip.mat.toml \
        --properties base_color,metallic,perceptual_roughness,emissive,reflectance,textures.normal \
        --reload-timeout-s 2.0 \
        --out docs/PROMPTS/artifacts/G3.09/material_roundtrip.json

Expected output shape:

    {
      "build_features": [],
      "properties": [
        { "name": "base_color", "disk_to_pixel_s": 0.41, "pixel_delta_matches_expected": true,
          "panel_to_disk_written": true, "other_lines_preserved": true },
        { "name": "metallic", "disk_to_pixel_s": 0.38, "pixel_delta_matches_expected": true,
          "panel_to_disk_written": true, "other_lines_preserved": true }
      ],
      "properties_passing": 6,
      "properties_total": 6,
      "g3_03_specular_drift_max_pct": 0.4,
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    properties_passing == properties_total == 6
      AND every disk_to_pixel_s <= 2.0
      AND every pixel_delta_matches_expected == true
      AND every panel_to_disk_written == true
      AND every other_lines_preserved == true
      AND build_features == []
      AND g3_03_specular_drift_max_pct <= 2.0
      AND the command exits 0

Verify by reading each per-property flag and `build_features`, not by observing that the file
changed.

## 6. Critic gate

Gated on **D2 (material and lighting realism)** and **D4 (UI craftsmanship)**, floor **8.0 each**.
Capture recipe: `docs/PROMPTS/harness/recipes/T1_RH1_material_roundtrip.json`.

D4 is included because the round-trip is a studio surface, not only a file format: an edit that
lands but gives no feedback, or a panel that silently discards a value it cannot represent, is a
craftsmanship failure even when the file is correct. The Critic sees frames and the studio surface,
never your self-report, and cites a frame index, a `path:line`, or a measured value for every score.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — close the write-back through the existing registry name → source path
                 map, in the default build, with a format-preserving TOML edit
   -> if still failing, MANDATORY approach change. Adding another property to the same path is NOT
      an approach change; moving write-back to a change-journal applied on a debounce is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with properties_passing moving < 1 AND the worst gated
                  dimension moving < 0.5
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a property cannot persist without the opt-in toml feature (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.09/material_roundtrip.json`

A reader finds: the exact build features used (which must be empty), one record per property with
its disk-to-pixel latency, whether the pixel change matched the expected direction and magnitude,
whether the panel write reached disk, and whether the rest of the file survived; the count passing;
the measured drift in `G3.03`'s specular numbers; the commit; and the GPU and driver version.

## 9. Definition of NOT done

- Five of six properties round-trip. The sixth is the one a user will reach for.
- The write-back works but rewrites the `.mat.toml` from the parsed struct, so comments and ordering
  are gone. That is silent data loss on an authored file.
- The round-trip works with `--features toml`. The default build is the product.
- Hot-reload requires closing and reopening the Space. State the latency measured without a reload.
- A texture-slot path is written back as an absolute path from the developer's machine, so the Space
  is no longer portable.
- `G3.03`'s per-cell specular numbers moved by more than 2%, meaning the material pipeline changed
  under the plumbing change. Re-run and report.
````

---

## G5.02 — Asset import/export round-trip fidelity budget

Item path: `docs/PROMPTS/items/G5.02_asset-roundtrip-fidelity.md`

````markdown
---
id: G5.02
title: Asset import and export round-trip inside a stated fidelity budget
workload: W3
workload_secondary: [W1, W6]
phase: G5
depends_on: [G3.09, G2.34, G1.12]
blocks: [G3.13]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D6]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH6_roundtrip.json
artifact: docs/PROMPTS/artifacts/G5.02/asset_roundtrip.json
escalation: >
  If a material property cannot survive the export because the glTF encoder in
  eustress/crates/cad/src/export_glb.rs has no slot for it, STALL and name the property and the
  standard extension that would carry it — do not drop the property from the comparison set.
status: DRAFT
notes: >
  Tier L. Import and export both exist as real code; what does not exist is a measured statement of
  what survives the trip.
---

## 1. Objective

An asset brought into Eustress and taken out again is still the same asset, within a budget that is
written down. A GLB imported, rendered, exported, and re-imported matches the original on vertex
count, bounding box, triangle count, and rendered appearance to within stated tolerances, and every
property that does not survive is enumerated rather than discovered later by a user.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Round-trip fidelity is a trust property: a
buyer who cannot get their geometry back out will not put it in.

**What exists.** `eustress/crates/engine/src/mesh_import.rs` (342 lines) is the GLB import path.
`eustress/crates/engine/src/mesh_optimizer.rs` (203 lines) runs `optimize_loaded_meshes` once per
mesh asset after GLB load — vertex cache optimisation, overdraw optimisation, and vertex fetch
reorder via `meshopt`, gated by `on_message::<AssetEvent<Mesh>>`. Its header notes the runtime LOD
system was dead code and has been deleted. On the export side,
`eustress/crates/cad/src/export_glb.rs` provides `encode_glb` and `write_glb`, re-exported at
`eustress/crates/cad/src/lib.rs:81`, driven from `handle_cad_export_glb`
(`eustress/crates/engine/src/cad_plugin.rs:1406`) and reachable from the studio as the tool id
`cad:export_glb` (`eustress/crates/engine/src/tool_metadata.rs:314`).

**Two facts that will bite you.** First, `optimize_loaded_meshes` reorders vertices and indices on
import, so a naive byte comparison of the re-exported buffer against the original will always fail —
your comparison must be over geometric invariants, not buffer layout. Second, USD is not a route
out: `eustress/crates/engine/src/usd_loader.rs` is 14 lines, a `UsdLoaderPlugin` whose `build` is
empty with a `TODO`. Do not propose USD as the round-trip format.

**Precedent for an honest fidelity report.** `eustress/crates/roblox-import/src/import_report.rs`
already models exactly the artifact this item must produce: `ImportReport` carries
`total_nodes_seen`, `total_nodes_imported`, `class_counts`, `unmapped_classes`,
`unmapped_properties`, and `asset_warnings`, archived as JSON under
`<space_root>/.eustress/import_reports/<ts>.json`. Follow that shape — an enumerated list of what did
not survive is the point, not a footnote.

**What `G3.09` gave you.** A closed material round-trip through `.mat.toml` in the default build.
This item extends the same discipline to geometry and to the glTF material representation.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/cad/src/export_glb.rs`
- `eustress/crates/engine/src/cad_plugin.rs` — only `handle_cad_export_glb`
- `eustress/crates/engine/src/bin/render_harness.rs` — only to add the `RH6_roundtrip` scene and the
  reference GLB it imports

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/mesh_optimizer.rs` — disabling meshopt to make buffers compare
  byte-for-byte is a measurement change and forfeits the item
- `eustress/crates/engine/src/usd_loader.rs` — a 14-line stub; implementing USD is a separate
  subsystem-scale item
- `eustress/crates/render-probe/`
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/mesh_import.rs` — owned by `G2.34` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Choosing a simpler test asset, dropping a property from the comparison set, disabling meshopt, or
  widening a tolerance are all measurement changes.
- The test asset must exercise the hard cases: at least one mesh with a normal map, one with
  emissive, one non-uniformly scaled node, one node with a negative scale component, and a nested
  transform hierarchy at least three levels deep.
- Every property that does not survive must appear in an `unsupported` array with the reason. An
  empty `unsupported` array on a non-trivial asset is a claim that will be checked.
- Compare geometry by invariants — vertex count, triangle count, axis-aligned bounding box in
  meters, total surface area — not by buffer bytes.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
For the reference asset, after import → export → re-import: vertex count and triangle count match
exactly, the axis-aligned bounding box matches within **1e-4 m** per axis, total surface area matches
within **0.1%**, the rendered frame from a fixed pose differs from the original import's frame by a
mean of **≤ 4/255**, and every non-surviving property is enumerated in the artifact's `unsupported`
array.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH6_roundtrip --seed 42 \
        --out eustress/spaces/harness/RH6_roundtrip

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH6_roundtrip

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        roundtrip \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH6_roundtrip.json \
        --asset eustress/spaces/harness/RH6_roundtrip/Assets/reference.glb \
        --cycles 1 \
        --out docs/PROMPTS/artifacts/G5.02/asset_roundtrip.json

Expected output shape:

    {
      "asset": "reference.glb",
      "vertex_count": { "original": 48213, "roundtrip": 48213, "match": true },
      "triangle_count": { "original": 91004, "roundtrip": 91004, "match": true },
      "aabb_max_axis_delta_m": 0.000021,
      "surface_area_delta_pct": 0.03,
      "render_mean_delta_255": 2.7,
      "unsupported": [
        { "property": "KHR_materials_clearcoat", "reason": "no slot in export_glb encoder" }
      ],
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    vertex_count.match == true AND triangle_count.match == true
      AND aabb_max_axis_delta_m <= 1e-4
      AND surface_area_delta_pct <= 0.1
      AND render_mean_delta_255 <= 4.0
      AND the unsupported array is present (it may be empty only if genuinely nothing was dropped)
      AND the command exits 0

Verify by reading each emitted value and the exit code.

## 6. Critic gate

Gated on **D6 (overall coherence)**, floor **8.0**. Capture recipe:
`docs/PROMPTS/harness/recipes/T1_RH6_roundtrip.json`.

D6 is the right single gate here because the failure this item guards against is systemic rather
than local: an asset that arrives with correct geometry and wrong orientation, or with materials
that survive on some nodes and not others, reads as a pipeline that was assembled rather than
designed. The Critic sees the before and after frames side by side, never your self-report, and
cites a frame index or a measured value for every score. A negative-scale node that silently flips
its winding is the specific defect to expect and to check for.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — align the export encoder's transform and material handling with what
                 the import path produces, invariant by invariant
   -> if still failing, MANDATORY approach change. Fixing one more invariant is NOT an approach
      change; adding a canonical intermediate scene description that both paths target is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the count of matching invariants unchanged AND
                  render_mean_delta_255 moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a material property has no representable slot in the export encoder (front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G5.02/asset_roundtrip.json`

A reader finds: the asset name and its digest, each geometric invariant with original and round-trip
values and a match verdict, the bounding-box and surface-area deltas in meters and percent, the
rendered-frame mean delta, an enumerated `unsupported` array with a reason per entry, the number of
round-trip cycles run, the commit, and the GPU and driver version. This file is the W3 evidence a
prospective user is shown when they ask what happens to their geometry.

## 9. Definition of NOT done

- The invariants match on a cube. The reference asset must contain the five hard cases listed in §4.
- Geometry survives and the model comes back mirrored, because a negative-scale node flipped its
  winding order. Check face orientation explicitly.
- `unsupported` is empty because unsupported properties were removed from the source asset rather
  than reported.
- The comparison passes after one cycle and diverges after three. Report the cycle count; if
  divergence accumulates, say so.
- Meshopt was disabled to make the comparison easy. That is a measurement change and forfeits the
  item.
- Units drift: the bounding box matches numerically but the re-import is in centimeters. Eustress is
  meter-native; state the unit at every boundary.
````

---

## G3.10 — Gaussian-splatting / mesh composite correctness

Item path: `docs/PROMPTS/items/G3.10_gaussian-splat-mesh-composite.md`

````markdown
---
id: G3.10
title: Gaussian splats and meshes composite as one scene
workload: W1
workload_secondary: [W3]
phase: G3
depends_on: [G3.07, G3.04, G1.12]
blocks: [G3.13]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D2, D6]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH7_splat_composite.json
artifact: docs/PROMPTS/artifacts/G3.10/splat_composite.json
escalation: >
  If enabling the gaussian-splatting feature makes ordinary part meshes fail to load — the
  glTF-loader shadowing failure documented in radiance/src/lib.rs — STALL immediately with the
  loader registration that caused it, rather than working around it by moving part meshes to a
  different extension.
status: DRAFT
notes: >
  Tier L. The splat pipeline is opt-in behind the `gaussian-splatting` cargo feature, so every
  build in this item carries an extra dependency tree; budget accordingly.
---

## 1. Objective

A captured radiance field and authored geometry occupy the same world. In a scene containing both, a
mesh in front of a splat cloud occludes it correctly, a mesh behind it is occluded, both are tone
mapped once by the same curve, and the splat cloud's exposure matches the mesh scene's within a
measured bound.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. Splats matter here because captured reality
is the cheapest route to a scene a stranger recognises, and a captured scene that cannot host
simulated geometry is a photograph, not a world.

**What exists.** `eustress/crates/radiance/src/lib.rs` (768 lines) wraps
`bevy_gaussian_splatting` behind `RadiancePlugin`, giving the render pipeline, GPU depth sort, and
`.ply` / `.gcloud` / glTF `KHR_gaussian_splatting` loaders. It is **opt-in**:
`eustress/crates/engine/Cargo.toml` defines `gaussian-splatting = ["dep:eustress-radiance"]` with the
comment "OPT-IN, NOT in `core`/`default` … Activate with
`cargo run -p eustress-engine --features gaussian-splatting`". `eustress/crates/radiance/src/collider.rs`
states the collider-extraction pipeline "is not implemented yet"; this item does not need it.

**The single most important trap, quoted from `radiance/src/lib.rs:45-56`.** `RadiancePlugin`
replicates `bevy_gaussian_splatting::GaussianSplattingPlugin` **except** its glTF scene loader
(`io::scene::GaussianScenePlugin`), because that loader registers an `AssetLoader` for
`.glb`/`.gltf` and **shadows Bevy's `GltfLoader`**, so the engine's normal part meshes
(`parts/block.glb`) fail with "no KHR_gaussian_splatting primitives found" and vanish from the
scene. Preserve that exclusion. If you need glTF-embedded splat scenes, register a loader scoped to
a distinct extension — never plain `.glb`.

**An existing control you should use.** `EUSTRESS_SPLAT_BUDGET`
(`radiance/src/lib.rs:567-599`) applies an optional hard splat budget by decimation, default off,
logging the before and after counts. Report the value used.

**Exposure and tonemapping are anchored.** `G3.04` anchored an 18% mid-grey and recorded it in
`docs/PROMPTS/artifacts/G3.04/exposure_response.json`; `studio_camera_bundle`
(`eustress/crates/engine/src/default_scene.rs:34`) applies `Tonemapping::TonyMcMapface`. The
classic splat-compositing defect is double tone mapping — the splat pass emitting already-tonemapped
colour into a pipeline that tone maps again — which shows up as a washed cloud against correctly
exposed geometry.

**Reflections are probe-aware.** `G3.07` landed a reflection path recorded in
`docs/PROMPTS/artifacts/G3.07/reflections.json`. A splat cloud is not a reflection occluder; say
explicitly in the artifact how the two interact, even if the answer is "not at all, by design".

**The scene.** `eustress/spaces/harness/RH7_splat_composite` must contain: one `.ply` splat cloud,
one authored mesh box positioned unambiguously in front of it, one positioned unambiguously behind
it, and one intersecting it. Extending the `render-harness` generator to place these, and to ship a
small reference `.ply`, is in scope.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile. The
extra feature adds to that. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/radiance/src/lib.rs`
- `eustress/crates/engine/src/rendering.rs`
- `eustress/crates/engine/src/bin/render_harness.rs` — only to add `RH7_splat_composite`
- `docs/architecture/GAUSSIAN_SPLATTING.md` — only to make the document match shipped behaviour,
  written so it reads as though always correct

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- The `GaussianScenePlugin` exclusion in `RadiancePlugin` — re-adding it breaks every part mesh
- `eustress/crates/radiance/src/collider.rs` — collider extraction is a separate subsystem item
- `eustress/crates/render-probe/`
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Moving the meshes so the depth relationship is unambiguous only from one angle, decimating the
  cloud until occlusion is trivial, or lowering capture resolution are measurement changes.
- Tone map once. If the splat pass needs to opt out of the camera's tonemapping, say so explicitly
  and prove the final image is tonemapped exactly once by matching the mid-grey anchor.
- The default build must not change. `cargo run -p eustress-engine --release` without
  `--features gaussian-splatting` must still load ordinary part meshes; verify and state it.
- Do not disable the GPU depth sort to make ordering deterministic. If sort order is
  frame-dependent, `G3.01`'s determinism selftest will catch it and that is a real finding.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
In `RH7_splat_composite`, the front mesh occludes the cloud over **≥ 99%** of its silhouette, the
back mesh is occluded by the cloud over **≥ 99%** of its silhouette, the intersecting mesh shows an
interpenetration boundary rather than a hard rectangle, the 18% mid-grey card stays within
**±3/255** of `G3.04`'s target with the cloud in frame, and the default build (no
`gaussian-splatting` feature) still renders every part mesh in `RH1_sphere_grid`.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH7_splat_composite --seed 42 \
        --out eustress/spaces/harness/RH7_splat_composite

    cargo run -p eustress-engine --release --features gaussian-splatting -- \
        --space eustress/spaces/harness/RH7_splat_composite

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH7_splat_composite.json \
        --out docs/PROMPTS/artifacts/G3.10/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        silhouette \
        --frame docs/PROMPTS/artifacts/G3.10/frames/RH7_000.png \
        --occlusion-pair front_box:cloud --occlusion-pair cloud:back_box \
        --patch grey_card_18 \
        --exposure-baseline docs/PROMPTS/artifacts/G3.04/exposure_response.json \
        --out docs/PROMPTS/artifacts/G3.10/splat_composite.json

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH1_sphere_grid

Expected output shape:

    {
      "front_box_occludes_cloud_pct": 99.7,
      "cloud_occludes_back_box_pct": 99.4,
      "intersection_boundary_is_soft": true,
      "mid_grey_abs_error": 1.1,
      "tonemap_applied_times": 1,
      "splat_budget": "off",
      "default_build_part_meshes_render": true,
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    front_box_occludes_cloud_pct >= 99.0
      AND cloud_occludes_back_box_pct >= 99.0
      AND intersection_boundary_is_soft == true
      AND mid_grey_abs_error <= 3.0
      AND default_build_part_meshes_render == true
      AND every command exits 0

Verify by reading each emitted value and the exit codes.

## 6. Critic gate

Gated on **D2 (material and lighting realism)** and **D6 (overall coherence)**, floor **8.0 each**.
Capture recipe: `docs/PROMPTS/harness/recipes/T1_RH7_splat_composite.json`.

D6 carries the weight here: the failure mode is not that either half looks bad, it is that the two
halves look like two renderers sharing a window. Different exposure, different tone response, a
cloud that receives no shadow from geometry standing on it, or geometry that casts no contact into
the cloud all read as "capable parts that never met". The Critic sees frames only, never your
self-report, and cites a frame index or a measured value for every score.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — correct the splat pass's colour space and depth participation so it
                 shares the camera's single tonemap and the depth buffer the meshes write
   -> if still failing, MANDATORY approach change. Adjusting a colour-space constant is NOT an
      approach change; moving the splat composite to a different render phase is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  front_box_occludes_cloud_pct moving < 1.0
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: enabling the feature breaks ordinary part-mesh loading (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.10/splat_composite.json`

A reader finds: both occlusion percentages, the interpenetration verdict, the mid-grey error against
`G3.04`, how many times the frame is tone mapped and how that was established, the splat budget
used, the confirmation that the default build still renders part meshes, a statement of how splats
and `G3.07`'s reflection path interact, the frame digest, the commit, and the GPU and driver
version.

## 9. Definition of NOT done

- Occlusion is correct from the recipe's pose and inverted from the opposite side, because the depth
  sort is view-dependent in a way that was never checked. Capture from two poses.
- The composite looks right and ordinary part meshes stopped loading under the feature. That is the
  documented loader-shadowing regression.
- The cloud is correctly composited and two stops brighter than the geometry. The mid-grey check
  exists to catch double tone mapping; report `tonemap_applied_times` honestly.
- Determinism broke: two captures of the same static pose no longer hash identically because the GPU
  splat sort is order-unstable. Report it as a finding rather than hiding it.
- `EUSTRESS_SPLAT_BUDGET` was set low enough that the cloud is sparse and occlusion is easy. State
  the budget and the splat count.
- The documentation is left claiming a phase that has not shipped, or is edited with changelog
  residue in the body instead of reading as though always correct.
````

---

## G3.11 — Lighting presets: one action, measurably distinct

Item path: `docs/PROMPTS/items/G3.11_lighting-presets.md`

````markdown
---
id: G3.11
title: Lighting presets that are one action and measurably distinct
workload: W6
workload_secondary: [W1]
phase: G3
depends_on: [G3.04, G3.05, G3.08, G3.09, G1.12]
blocks: [G3.12]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D1, D4]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH3_presets.json
artifact: docs/PROMPTS/artifacts/G3.11/lighting_presets.json
escalation: >
  If applying a preset cannot be made a single user action because the property it must set has no
  write path in the default build, STALL and name the property and its owning file — do not ship a
  preset that requires the user to hand-edit a TOML afterwards.
status: DRAFT
notes: >
  Tier M: the lighting classes and their instance templates already exist; this item composes them
  and proves the compositions differ, which needs few builds but a real capture set.
---

## 1. Objective

Setting up believable lighting takes one action instead of twenty. Four named presets — `Overcast`,
`GoldenHour`, `Noon`, `Interior` — each apply in a single action to any Space, and the four resulting
frames of the same scene are measurably distinct from one another rather than four small variations
on one look.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate. This item is operator leverage: the wall
clock a solo operator spends dialling lights is time not spent on the model, and a preset that lands
a credible look in one action is the difference between a demo happening and not happening.

**The pieces already exist as first-class objects.**
`eustress/crates/engine/assets/lighting_templates/` contains nine instance templates:
`Atmosphere.instance.toml`, `DirectionalLight.instance.toml`, `Moon.instance.toml`,
`PointLight.instance.toml`, `Sky.instance.toml`, `Skybox.instance.toml`,
`SpotLight.instance.toml`, `Sun.instance.toml`, `SurfaceLight.instance.toml`.

`eustress/crates/engine/src/plugins/lighting_plugin.rs` hydrates file-loaded `Lighting/` entities
into real ECS components: `Star → DirectionalLight + SunMarker + SunClass + cascade shadows +
SunDisk`, `Moon → DirectionalLight + MoonMarker + MoonClass`, `Sky → Sky`,
`Atmosphere → Atmosphere + EustressAtmosphere`. `eustress/crates/engine/src/light_sync.rs` mirrors
per-light `shadow_depth_bias` and `shadow_normal_bias` (`light_sync.rs:139-147`).
`eustress/crates/common/src/plugins/lighting_plugin.rs` owns `LightingService` and runs
`update_sun_position`, `update_moon_position`, `update_ambient_light`,
`update_exposure_compensation`, and `update_fog_settings`.

**What the three items you depend on established, and which you must not undo.**
`G3.04` anchored an 18% mid-grey target (`docs/PROMPTS/artifacts/G3.04/exposure_response.json`) and
a stops-calibrated exposure control. `G3.05` landed distance-varying penumbra and a bounded cascade
seam (`docs/PROMPTS/artifacts/G3.05/shadow_quality.json`). `G3.08` landed aerial perspective and a
per-Space fog density (`docs/PROMPTS/artifacts/G3.08/atmosphere.json`) — that per-Space settability
is what makes `Overcast` differ from `Noon`, so if it is missing this item is blocked, not
improvised around.

**What a preset is allowed to be.** A composition of existing lighting-class instances and service
settings — sun elevation and colour, ambient, fog density and colour, exposure compensation, shadow
softness. It is not a new rendering feature and it is not a post-process look-up table.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/assets/lighting_templates/` — new preset files permitted
- `eustress/crates/engine/src/plugins/lighting_plugin.rs`
- `eustress/crates/common/src/services/lighting.rs`
- `eustress/crates/engine/src/tool_metadata.rs` — only to register the preset actions
- `docs/development/LIGHTING_SYSTEM.md` — only to document the presets, written so it reads as
  though always correct

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/ui/slint_ui.rs` — 23,103 lines, another pack's item. Register the
  actions through the existing tool-metadata surface; if that genuinely cannot reach the UI, that is
  an escalation, not a licence.
- `eustress/crates/render-probe/`
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Choosing four presets that differ mainly in colour temperature so the distance metric is easy,
  measuring on a scene with one flat surface, or lowering capture resolution are measurement
  changes.
- One action means one action. A preset that requires the user to then set fog by hand is not a
  preset; that is the escalation trigger.
- Presets must be data, not code paths. A preset defined by a `match` arm in Rust cannot be authored
  by a user and fails the leverage this item exists to create.
- Each preset must keep the 18% mid-grey anchor from `G3.04` within tolerance. A preset is a look,
  not an exposure error.
- Batch verification: six builds total.

## 5. Exit criterion

### Criterion
Each of the four presets applies to `RH3_exterior` in exactly **one action**, every pairwise
combination of the four resulting frames differs by a mean CIE ΔE of **≥ 8.0**, and every preset
holds the 18% mid-grey card within **±4/255** of `G3.04`'s target.

### Measurement

Command:

    cargo run -p eustress-engine --release -- \
        --space eustress/spaces/harness/RH3_exterior

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH3_presets.json \
        --presets Overcast,GoldenHour,Noon,Interior \
        --count-actions \
        --out docs/PROMPTS/artifacts/G3.11/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        diff \
        --frames docs/PROMPTS/artifacts/G3.11/frames/ \
        --pairwise --metric delta-e \
        --patch grey_card_18 \
        --exposure-baseline docs/PROMPTS/artifacts/G3.04/exposure_response.json \
        --out docs/PROMPTS/artifacts/G3.11/lighting_presets.json

Expected output shape:

    {
      "presets": ["Overcast", "GoldenHour", "Noon", "Interior"],
      "actions_per_preset": { "Overcast": 1, "GoldenHour": 1, "Noon": 1, "Interior": 1 },
      "pairwise_delta_e": {
        "Overcast:GoldenHour": 24.1, "Overcast:Noon": 12.6, "Overcast:Interior": 19.8,
        "GoldenHour:Noon": 17.3, "GoldenHour:Interior": 21.0, "Noon:Interior": 15.5
      },
      "min_pairwise_delta_e": 12.6,
      "mid_grey_abs_error": { "Overcast": 1.2, "GoldenHour": 2.9, "Noon": 0.7, "Interior": 3.1 },
      "preset_definition_format": "lighting_templates/*.preset.toml",
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    every value in actions_per_preset == 1
      AND min_pairwise_delta_e >= 8.0
      AND every value in mid_grey_abs_error <= 4.0
      AND both commands exit 0

Verify by reading `actions_per_preset`, `min_pairwise_delta_e`, and each mid-grey error.

## 6. Critic gate

Gated on **D1 (first-three-seconds impact)** and **D4 (UI craftsmanship)**, floor **8.0 each**.
Capture recipe: `docs/PROMPTS/harness/recipes/T1_RH3_presets.json`.

D1 because a preset's only job is the impression it creates immediately; a preset that is
technically distinct but reads as "the same scene with a filter" fails the purpose. D4 because the
preset is a studio affordance: how it is named, discovered, previewed, and undone is the product.
The Critic sees frames and the studio surface, never your self-report, and cites a frame index, a
`path:line`, or a measured value for every score.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — presets as data files composed from the existing lighting instance
                 templates and service settings
   -> if still failing, MANDATORY approach change. Retuning one preset's sun angle is NOT an
      approach change; changing what a preset is permitted to set is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  min_pairwise_delta_e moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a preset property has no write path in the default build (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.11/lighting_presets.json`

A reader finds: the four preset names, the action count measured for each, the full pairwise ΔE
matrix and its minimum, each preset's mid-grey error against `G3.04`'s anchor, the file format a
user would author a fifth preset in, the recipe digest, the commit, and the GPU and driver version.

## 9. Definition of NOT done

- The four presets differ by ΔE 8 because one of them is simply two stops darker. Distinctness by
  exposure is why the mid-grey tolerance is in the pass condition.
- Applying a preset takes one click and then a five-second popping cascade of shadow rebuilds. State
  the settle time.
- Presets are Rust `match` arms, so a user cannot author a fifth. The leverage does not transfer.
- `Interior` looks correct in `RH2_interior` and absurd in `RH3_exterior`, and the measurement was
  taken only where it flatters. Measure all four on the same scene, as the criterion requires.
- Applying a preset cannot be undone in one action. A one-way door is a craftsmanship failure
  regardless of the frame.
- `G3.08`'s per-Space fog setting was bypassed with a hardcoded value inside a preset, so fog no
  longer responds to the Space at all.
````

---

## G3.12 — The first-three-seconds opening frame

Item path: `docs/PROMPTS/items/G3.12_first-three-seconds.md`

````markdown
---
id: G3.12
title: The first-three-seconds opening frame
workload: W1
workload_secondary: []
phase: G3
depends_on: [G3.11, G4.03, G3.06, G7.39, G1.12]
blocks: [G3.13]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D1, D6]
capture_recipe: docs/PROMPTS/harness/recipes/T1_RH8_opening.json
artifact: docs/PROMPTS/artifacts/G3.12/opening_frame.json
escalation: >
  If reaching a 3.0 s time-to-first-credible-frame requires deferring geometry or lighting that is
  visible in that frame — that is, the frame is on time because it is not yet the frame — STALL and
  report both the time and what was missing, rather than shipping a fast frame that lies.
status: DRAFT
notes: >
  Tier L. This is the item that consolidates G3 and G4 into a single opening shot; it adds little
  new capability and spends its budget on composition and on the load path that gets there.
---

## 1. Objective

The first three seconds are decided on purpose. Opening the flagship Space produces a fully-resolved
establishing frame within 3.0 seconds of the window appearing, and that frame — shown to a stranger
with no explanation — carries lighting, contact, atmosphere, and material variety simultaneously.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never a game engine. The opening frame is where a stranger decides whether
the substrate is real, and it is the only moment in the evaluation where nothing has been explained
yet.

**What is already landed and must all be visible in one frame.** This item composes, it does not
invent:

- `G3.04` — exposure anchored to an 18% mid-grey with stops-calibrated control
  (`docs/PROMPTS/artifacts/G3.04/exposure_response.json`).
- `G3.05` — distance-varying penumbra, contact darkening, bounded cascade seam
  (`docs/PROMPTS/artifacts/G3.05/shadow_quality.json`).
- `G3.06` — ambient occlusion grounding objects without haloing
  (`docs/PROMPTS/artifacts/G3.06/ambient_occlusion.json`).
- `G3.08` — aerial perspective with monotone desaturation over distance
  (`docs/PROMPTS/artifacts/G3.08/atmosphere.json`).
- `G3.11` — four one-action lighting presets, measurably distinct
  (`docs/PROMPTS/artifacts/G3.11/lighting_presets.json`).
- `G4.03` — temporal stability under a 2.0 m/s dolly
  (`docs/PROMPTS/artifacts/G4.03/temporal_stability.json`).

If any of those artifacts is missing, the corresponding item is not `PASSED` and this one may not
start.

**What the load path costs today.** `docs/development/BENCHMARK_VS_ENGINE_AUDIT.md` records
one-second stutters traced to `write_instance_changes_system` performing 20K synchronous disk
operations, several of which that document marks fixed. `eustress/crates/engine/src/space/hlod.rs`
enumerates non-empty cells once on Space load and builds each proxy once on a background task with a
cap. `EUSTRESS_LOAD_SPAWN_BUDGET` and `EUSTRESS_PHASE_WATCHDOG_SECS` exist as load-path controls.
Measure what the load actually costs before assuming where the three seconds go.

**The measurement window is honest, not generous.** The clock starts when the window appears, not
when loading finishes. A frame that is on time because half the scene has not streamed in is the
failure this item's escalation trigger names.

**The scene.** `eustress/spaces/harness/RH8_opening` — the establishing shot. It must contain, in a
single frame: a foreground object with visible contact shadow and ambient occlusion, mid-ground
geometry with at least four distinguishable materials, background geometry far enough away to show
aerial perspective, and a light direction that produces a readable shadow across the frame.
Authoring it through the `render-harness` generator is in scope.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile.
Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/bin/render_harness.rs` — the `RH8_opening` scene
- `eustress/crates/engine/src/space/hlod.rs` — load-order and first-frame readiness only
- `eustress/crates/engine/assets/lighting_templates/` — a preset for the opening shot

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- Any renderer file owned by `G3.04`, `G3.05`, `G3.06`, `G3.08`, or `G4.03`. If the opening frame
  reveals a defect in one of those, that is a re-open of the owning item, not a patch here.
- `eustress/crates/render-probe/`
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/space/load_phase.rs` — owned by `G7.39` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Starting the clock after load, shrinking the scene, moving the camera closer so less is visible,
  or lowering capture resolution are all measurement changes.
- Do not reach the time budget by deferring what the frame shows. The completeness check — four
  distinguishable materials, visible contact, visible aerial perspective — is measured on the frame
  captured at the 3.0 s mark, not on a later one.
- No pre-rendered splash, no static image, no fade-in that hides an unfinished frame. The measured
  frame must be a live render of the loaded Space.
- The opening frame must survive motion: the same pose held for 60 frames must satisfy `G4.03`'s
  static-surface interframe bound.
- Batch verification: twelve builds across three approaches.

## 5. Exit criterion

### Criterion
Opening `RH8_opening` produces a frame at **≤ 3.0 s** after window appearance in which: at least
**four** distinguishable material clusters are measurable, the foreground object's contact region is
**≥ 25%** darker than the adjacent open ground, the background reference marker's saturation is
**≤ 0.55×** the foreground marker's, and holding the pose for 60 frames gives a static-surface
interframe p95 of **≤ 6/255**.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene RH8_opening --seed 42 \
        --out eustress/spaces/harness/RH8_opening

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_RH8_opening.json \
        --cold-open --deadline-s 3.0 --hold-frames 60 \
        --out docs/PROMPTS/artifacts/G3.12/frames/

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        histogram \
        --frame docs/PROMPTS/artifacts/G3.12/frames/RH8_deadline.png \
        --material-clusters \
        --probe foreground_contact --probe open_ground \
        --probe marker_near --probe marker_far \
        --out docs/PROMPTS/artifacts/G3.12/opening_frame.json

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        temporal \
        --frames docs/PROMPTS/artifacts/G3.12/frames/hold/ \
        --static-mask-from-recipe docs/PROMPTS/harness/recipes/T1_RH8_opening.json \
        --append-to docs/PROMPTS/artifacts/G3.12/opening_frame.json

Expected output shape:

    {
      "time_to_frame_s": 2.4,
      "deadline_s": 3.0,
      "material_clusters": 6,
      "contact_darkening_pct": 31.0,
      "far_over_near_saturation": 0.48,
      "static_interframe_p95_255": 3.9,
      "frame_is_live_render": true,
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    time_to_frame_s <= 3.0
      AND material_clusters >= 4
      AND contact_darkening_pct >= 25.0
      AND far_over_near_saturation <= 0.55
      AND static_interframe_p95_255 <= 6.0
      AND frame_is_live_render == true
      AND every command exits 0

Verify by reading each emitted value and the exit codes.

## 6. Critic gate

Gated on **D1 (first-three-seconds impact)** and **D6 (overall coherence)**, floor **8.0 each**.
Capture recipe: `docs/PROMPTS/harness/recipes/T1_RH8_opening.json`.

D1 is measured exactly as its name says: what a stranger concludes in 3.0 seconds, before any
explanation. The Critic never sees your result block, your commit message, or a caption baked into
the capture — the frame argues alone. D6 is the partner check: this frame is where the whole pack's
work is either one designed system or a collection of individually-passing features. Expect the
Critic to invoke its ability to refuse a frame that clears every number and still reads as placed
objects under placed lights. That refusal is legitimate and the item is not done.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — compose the shot from the landed capabilities and shorten the load
                 path to the deadline
   -> if still failing, MANDATORY approach change. Moving the camera is NOT an approach change;
      changing what the establishing shot is of — subject, scale, time of day — is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with D1 moving < 0.5 AND time_to_frame_s moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the deadline is met only by deferring visible content (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G3.12/opening_frame.json`

A reader finds: the measured time to the first credible frame against the deadline, the material
cluster count, the contact-darkening and aerial-perspective measurements, the 60-frame hold
stability, confirmation the frame is a live render, the recipe digest, the commit, and the GPU and
driver version. Archived alongside: the deadline frame itself and the D1/D6 scorecard.

## 9. Definition of NOT done

- The frame arrives in 2.4 s and the background is still streaming in at 4 s. The deadline frame is
  what is judged; if it is incomplete, the time is not a pass.
- Four material clusters are measurable because four differently-tinted copies of the same material
  were placed. Distinguishable means distinguishable to a viewer, and the Critic will say so.
- The shot is beautiful and shows nothing that indicates a simulation substrate rather than a static
  scene. D6 covers coherence, and the flagship item that depends on this one will inherit the
  problem.
- Load time was won by lowering `EUSTRESS_LOAD_SPAWN_BUDGET` so fewer entities spawn per frame,
  which moves the missing content later rather than removing the cost.
- The frame is stable held still and shimmers the moment the camera moves. Hold-frame stability is
  necessary, not sufficient; check a short move too.
- The Critic passes every number and refuses the impression. That is a legitimate refusal and the
  item is not done.
````

---

## G3.13 — Flagship blind-comparison scene

Item path: `docs/PROMPTS/items/G3.13_flagship-blind-comparison-scene.md`

````markdown
---
id: G3.13
title: Flagship scene that survives a blind comparison
workload: W1
workload_secondary: [W3, W4]
phase: G3
depends_on: [G3.12, G5.01, G5.02, G3.10, G1.12]
blocks: []
tier: XL
token_envelope: 1200000
wallclock_envelope: 5d
max_builds: 20
critic_gate: [D1, D2, D3, D5, D6, D7]
capture_recipe: docs/PROMPTS/harness/recipes/T1_FLAGSHIP.json
artifact: docs/PROMPTS/artifacts/G3.13/flagship_scorecard.json
escalation: >
  If the blind preference trials return 2 of 5 or worse on two consecutive iterations with
  different scene compositions, STALL rather than continuing to recompose — two failures at that
  margin mean the deficit is a capability, not a composition, and the packet must name which
  gated dimension carries it.
status: DRAFT
notes: >
  Tier XL: this is the pack's proof artifact and the only item whose gate includes the forced-choice
  preference dimension. It composes everything the pack landed and adds no new renderer capability.
---

## 1. Objective

One scene, rendered by Eustress, is preferred over a scene rendered by an engine the buyer already
trusts, in a blind forced choice, at least 4 times out of 5 randomised trials. The scene is
reproducible from a seed, the capture is reproducible from a recipe, and every number that supports
it is measured rather than asserted.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. It is never described as a game engine, and the flagship scene must not be
composed as a game level. The scene's subject should be something a substrate is *for*: a site, a
facility, an instrumented environment — something where the geometry exists because it is being
modelled, not because it is being played.

**The licence, because this artifact will be shown externally.** PolyForm Shield 1.0.0. Any text
shipped alongside this scene says **source-available**. It never says open source.

**Everything this item composes is already landed and measured.** Do not rebuild any of it:

| Capability | Owning item | Evidence file |
|---|---|---|
| Deterministic parameterised capture | `G3.01` | `docs/PROMPTS/artifacts/G3.01/capture_determinism.json` |
| Image-analysis probe + PBR baseline | `G3.02` | `docs/PROMPTS/artifacts/G3.02/pbr_reference_baseline.json` |
| Frame-time distribution | `G4.01` | `docs/PROMPTS/artifacts/G4.01/frametime_baseline.json` |
| Energy-correct PBR | `G3.03` | `docs/PROMPTS/artifacts/G3.03/pbr_energy.json` |
| Anchored exposure | `G3.04` | `docs/PROMPTS/artifacts/G3.04/exposure_response.json` |
| Contact-correct shadows | `G3.05` | `docs/PROMPTS/artifacts/G3.05/shadow_quality.json` |
| MSAA-free anti-aliasing | `G4.02` | `docs/PROMPTS/artifacts/G4.02/aliasing.json` |
| Ambient occlusion | `G3.06` | `docs/PROMPTS/artifacts/G3.06/ambient_occlusion.json` |
| Temporal stability | `G4.03` | `docs/PROMPTS/artifacts/G4.03/temporal_stability.json` |
| Room-correct reflections | `G3.07` | `docs/PROMPTS/artifacts/G3.07/reflections.json` |
| Atmosphere and aerial perspective | `G3.08` | `docs/PROMPTS/artifacts/G3.08/atmosphere.json` |
| Pop-free tier transitions | `G4.04` | `docs/PROMPTS/artifacts/G4.04/tier_pop.json` |
| HLOD proxy fidelity | `G5.01` | `docs/PROMPTS/artifacts/G5.01/hlod_fidelity.json` |
| Material round-trip | `G3.09` | `docs/PROMPTS/artifacts/G3.09/material_roundtrip.json` |
| Asset round-trip budget | `G5.02` | `docs/PROMPTS/artifacts/G5.02/asset_roundtrip.json` |
| Splat/mesh compositing | `G3.10` | `docs/PROMPTS/artifacts/G3.10/splat_composite.json` |
| Lighting presets | `G3.11` | `docs/PROMPTS/artifacts/G3.11/lighting_presets.json` |
| Opening frame | `G3.12` | `docs/PROMPTS/artifacts/G3.12/opening_frame.json` |

If any of those files is absent, its item is not `PASSED` and this item may not start.

**What the scene must contain.** A single Space, generated from a seed by the `render-harness`
binary, containing: an exterior establishing view with visible aerial perspective; an interior with
a reflective floor and a window to the exterior; at least twelve distinguishable authored materials;
a district dense enough to cross the HLOD boundary during the camera move; at least one imported
GLB asset that passed `G5.02`'s round-trip; and at least one Gaussian-splat cloud composited with
authored geometry. Meter-native throughout; studs may appear only as a display unit in the studio.

**What the comparison is.** A camera move of at least 12 seconds, captured as a frame sequence
through the deterministic capture path, rendered by Eustress at HEAD. The control side is the same
subject rendered by an engine a buyer already trusts. Producing the control is a human decision —
the executing agent produces the Eustress side, the reproducible recipe, and the measured scorecard,
and states plainly in the artifact that the control was supplied externally and by whom.

**Frame budget is a hard part of the claim.** A flagship that renders at 12 FPS is not a flagship.
`G4.01`'s `frametime` subcommand is how you show it.

**Build reality.** 10–15 minutes per engine build, one at a time against the shared `target/`, never
killed mid-compile. Twenty builds is the entire budget for three approaches, so one build must
validate several changes. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/bin/render_harness.rs` — the `FLAGSHIP` scene generator
- `eustress/crates/engine/assets/lighting_templates/` — a preset for the flagship
- `docs/PROMPTS/harness/recipes/T1_FLAGSHIP.json` — the flagship capture recipe
- `eustress/spaces/harness/FLAGSHIP/` — generator output

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- **Every renderer file owned by an upstream item in this pack.** If the flagship reveals a defect in
  shadows, exposure, reflections, atmosphere, AA, temporal stability, HLOD, splats, or materials,
  that is a re-open of the owning item with its own measurement — not a patch here. This item ships
  no new renderer capability by design.
- `eustress/crates/render-probe/`
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Choosing a subject that flatters the renderer's strengths and hides its weaknesses, shortening the
  camera move, capturing only stills, lowering resolution, or excluding the HLOD crossing are all
  measurement changes.
- The scene must be generated from a seed. A hand-placed flagship cannot be regenerated by a Critic,
  cannot be re-captured after a later fix, and is worth nothing as evidence.
- No per-scene renderer special-casing. If a value has to be different for the flagship than for the
  harness scenes, that is a defect in the default, not a flagship feature. Say so and re-open the
  owning item.
- The camera move must include a moment of each landed capability. A flagship that never crosses the
  HLOD boundary is not exercising `G5.01`, and the Critic will notice the pack's own evidence is
  unrepresented.
- Do not fabricate a measured number anywhere in the artifact or in any text shipped with the scene.
  Every number is `MEASURED` with its command and hardware, `TARGET` labelled inline, or
  `CONFIG DEFAULT` labelled.

## 5. Exit criterion

### Criterion
In **5 randomised blind trials** of the flagship sequence against the externally-supplied control,
Eustress is preferred in **≥ 4**; every gated Critic dimension scores **≥ 8.0**; and the flagship
sequence sustains **`ft_p99_ms` ≤ 25.0** at 3840×2160 on the harness reference GPU.

### Measurement

Command:

    cargo run -p eustress-engine --release --bin render-harness -- \
        --scene FLAGSHIP --seed 42 \
        --out eustress/spaces/harness/FLAGSHIP

    cargo run -p eustress-engine --release --features gaussian-splatting -- \
        --space eustress/spaces/harness/FLAGSHIP

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        capture \
        --universe eustress/spaces/harness \
        --recipe docs/PROMPTS/harness/recipes/T1_FLAGSHIP.json \
        --width 3840 --height 2160 \
        --dolly-frames 720 --dolly-speed-mps 2.0 \
        --out docs/PROMPTS/artifacts/G3.13/frames/

    cargo run -p eustress-engine --release --features gaussian-splatting --bin render-harness -- \
        frametime --space eustress/spaces/harness/FLAGSHIP \
        --frames 3000 --warmup 300 --dolly \
        --out docs/PROMPTS/artifacts/G3.13/frametime_flagship.json

    cargo run -p eustress-render-probe --release --bin render-probe -- \
        diff \
        --subject docs/PROMPTS/artifacts/G3.13/frames/ \
        --control docs/PROMPTS/artifacts/G3.13/control/ \
        --trials 5 --randomise --blind \
        --out docs/PROMPTS/artifacts/G3.13/flagship_scorecard.json

The `diff` subcommand assembles the blinded comparison bundle: five randomised subject/control pairs
with side labels stripped, plus the provenance needed to audit the randomisation. It records what
was put in front of the judge. It does not record what the judge chose, and it emits no dimension
scores — those come from the Critic and only from the Critic.

Expected output shape:

    {
      "trials": 5,
      "control_source": "externally supplied; see control_provenance",
      "control_provenance": "...",
      "trial_order_sha256": "...",
      "ft_p50_ms": 13.1,
      "ft_p99_ms": 22.4,
      "resolution": "3840x2160",
      "scene_seed": 42,
      "harness_generator_sha256": "...",
      "recipe_sha256": "...",
      "commit": "...", "gpu": "...", "driver": "..."
    }

Pass condition:

    ft_p99_ms <= 25.0
      AND resolution == "3840x2160"
      AND every command exits 0
      AND the frame set and the provenance hashes are complete

Verify by reading `ft_p99_ms`, `resolution`, and the exit codes. Do not infer success from the frame
set existing.

**§5 is half the criterion.** Clearing every number above makes the bundle eligible for judgement; it
does not make the item `PASSED`. This item is not `PASSED` until an independent blinded Critic has
filed a scorecard against the archived bundle and that scorecard clears the floors in §6. Nothing the
executing agent writes — including `flagship_scorecard.json` — contributes a preference count or a
dimension score.

## 6. Critic gate

Gated on **D1, D2, D3, D5, D6, and D7**, floor **8.0 on every one**. The mean is irrelevant — a
single dimension below floor fails the item.

**This section carries the independently-scored half of the exit criterion.** Both thresholds below
are evaluated only against the scorecard an independent, blinded Critic files against the archived
bundle from §5, per `docs/PROMPTS/03_PROMPT_SCHEMA.md` §3.4:

    subject_preferred >= 4 of trials == 5
      AND min gated dimension >= 8.0

**D7 is the forced-choice preference dimension** and its floor is 4 of 5 blind randomised trials. It
is the only place in this pack where the artifact is judged against something other than itself. The
executing agent supplies the frames, the control provenance, and the randomisation record; the
Critic supplies the choice. An agent that reports its own preference count has produced no evidence.

**D5 (simulation believability and numerical trust)** is included and is the one an agent will
underweight. The scene must not only look right; a domain expert looking at it must have no reason
to doubt the numbers behind it. That means meter-native scale that reads correctly, lighting that is
consistent with a stated time and place, and no visual claim the simulation cannot back.

Three properties of the gate shape how you compose the scene. The Critic never sees your self-report
— not the result block, not commit messages, not a README, not a caption baked into a capture; strip
all of it. Every score it gives cites a frame index, a `path:line`, or a measured value, and an
uncited score auto-fails the whole scorecard. And the Critic may refuse to pass something that clears
every number — the wow gate — but can never pass something that misses one. Treat the numeric floor
as the beginning of the argument.

Capture recipe: `docs/PROMPTS/harness/recipes/T1_FLAGSHIP.json`.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — compose the flagship from the landed capabilities, exercising each
                 one at a distinct moment of the camera move
   -> if still failing, MANDATORY approach change. Re-framing a shot is NOT an approach change;
      changing the subject of the scene — what is being modelled and why — is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  subject_preferred unchanged
  - Budget      : 1.8M tokens or 30 builds consumed (150% of the XL envelope)
  - Item-specific: 2 of 5 or worse on two consecutive iterations with different compositions
                   (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the preference floor to a
stated value with the stated consequence for the pack's W1 claim; FUND a specific approach D; DEFER
behind a named blocking item — most likely a re-open of whichever gated dimension carries the
deficit; or KILL, with a statement of what the program loses.

## 8. Artifact

`docs/PROMPTS/artifacts/G3.13/flagship_scorecard.json`

A reader finds: the trial count, the provenance of the externally supplied control, the SHA-256 of
the trial order that proves the randomisation, p50 and p99 frame time at the stated resolution, the
scene seed, the SHA-256 of the harness generator and of the capture recipe, the commit, and the GPU
and driver version. Archived alongside: the full frame sequence, the capture bundle manifest, and
the independent blinded Critic scorecard — which is where the preference count and every gated
dimension's score live. Read together, the two files are the pack's W1 evidence and are what a
prospective buyer is shown.

## 9. Definition of NOT done

- 4 of 5 is reached on a scene composed to hide a weakness — no fast camera motion because temporal
  stability is marginal, or no wide exterior because atmosphere is thin. A flagship that avoids the
  pack's own gaps is not evidence.
- The scene is hand-placed and cannot be regenerated from the seed, so no later fix can be
  re-measured against it.
- A renderer value was special-cased for the flagship. Then the product is not what was compared.
- `ft_p99_ms` passes because the measurement was taken at 1920×1080 while the frames were captured
  at 3840×2160. The criterion states the resolution; measure there.
- The artifact contains a number labelled as measured that was estimated. One fabricated number
  invalidates the whole scorecard, and this is the artifact that leaves the building.
- Text shipped with the scene calls Eustress a game engine, or calls the licence open source. Both
  are invariant violations regardless of the frames.
- The Critic clears every number and refuses the wow gate, citing that the scene reads as a
  well-lit model rather than a place. That refusal is legitimate and the item is not done.
````

---

## Pack notes

**Cross-pack overlap, resolved.** The general capture harness belongs to pack `G1`: `G1.05` owns the
parameterised AI camera, `G1.03` owns the seeded scene generator and `eustress/spaces/harness/`,
`G1.09` owns the capture binary and the provenance-manifest format, and `G1.11` owns the
frames-and-recording determinism proof. `G3.01` consumes all four rather than duplicating them, and
`G3.02`'s `render-probe` is scoped to image analysis only. The image-analysis probe and the reference
sphere grid are uniquely this pack's. The full ruling, including the eighteen cross-pack edges this
pack's items now carry, is in `docs/PROMPTS/04_FILE_OWNERSHIP.md`.

**Two items target temporal stability, as required.** `G4.03` covers per-pixel stability during
continuous motion — crawl, shimmer, and ghosting. `G4.04` covers discrete stability at LOD tier
transitions — pop. `G5.01` extends the same discipline to the HLOD proxy swap, which is a third,
distinct code path. They are deliberately separate items because they fail differently and are fixed
in different crates.

**One item targets the first-three-seconds impression.** `G3.12`, gated on D1 and D6, measured
against a 3.0 s deadline with a completeness check on the deadline frame so the clock cannot be won
by deferring content.

**One item produces the flagship blind-test scene.** `G3.13`, the only item in the pack gated on D7,
the forced-choice preference dimension.

**Capabilities this pack starts at 0% and says so in the prompt body.** Soft shadows with
distance-varying penumbra (`G3.05`); ambient occlusion, currently a commented-out line at
`eustress/crates/common/src/plugins/lighting_plugin.rs:583` (`G3.06`); anti-aliasing, with the
crates enabled in the manifest but no camera carrying a component (`G4.02`); local reflection probes
and screen-space reflections, absent from the tree entirely (`G3.07`); volumetric fog, where
`VolumetricLight` is inserted on the sun but no participating medium exists (`G3.08`); frame-time
percentiles, computed nowhere (`G4.01`); and Properties-panel write-back in the default build
(`G3.09`).

**Standing out-of-scope entries appear in every item**: CI workflows, the Critic rubric, and any
capture already hashed into a provenance manifest.
