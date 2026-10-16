# World Model Review: Before and After, and How Eustress Beats Marble

**Date:** 2026-09-30
**Baseline ("before"):** `WORLD_MODEL_STATUS_2026_08_15.md` and
`WORLD_GENERATION_OVER_MCP_PLAN.md` (2026-08-16).
**Method:** every Eustress claim was re-checked against the working tree today
with the reach test: does it reach a running binary. "Written, never run" is
recorded as such. Marble facts come from public sources listed at the end.

---

## 1. Verdict

- **Eustress already beats Marble at the thing World Labs itself calls the
  linchpin.** World Labs' June 2026 essay splits world models into renderers
  (pixels), simulators (state programs compute on) and planners (actions), puts
  Marble in the simulator category, and names "geometry that can look correct
  while containing self-intersections or wrong scale that produce nonsensical
  physics" as "the defining open problem in world model research today."
  Eustress worlds are state first: classed objects, metric units, Avian
  physics, scripts, determinism. Marble's own coverage says "the world itself
  simulates nothing."
- **Marble beats Eustress on everything a user sees in the first five
  minutes:** photorealism from one sentence, a 20-second draft, six input
  types, exports into every major tool, and a web and VR viewer.
- **The plan below closes the first-five-minutes gap without giving up the
  state lead.** The decisive move is generated appearance on authored geometry
  (section 10): Claude blocks out the world as real, metric, physical state, and
  NVIDIA's commercially licensed open models paint only its appearance. Ingesting
  Marble worlds (I1) is a second route: it makes Eustress at least equal on
  visuals for any world Marble can make, and strictly ahead on every state
  measure.
- **"Beat Marble" is defined in section 7 as a benchmark that can fail.**
  Until it runs, the claim is a plan, not a result.

---

## 2. Before and after: the world model since August 15

### Improved

| Item | Before (Aug 15) | After (Sep 30) | Evidence |
|---|---|---|---|
| Generative architecture loop (`genesis`) | 752 lines reaching no binary; panicked on bad input; optimizer collapsed members to zero area | Wired in the engine (`app_core.rs:310`), FEA yield gate, input validation, 6/6 tests | commit `2b53fcf6` |
| Gravity path | Canonical unit-converting system scheduled nowhere | Scheduled in Studio (`WorkspacePlugin`) and the Player (`client/src/main.rs:271`) | commit `9729e74d`; Roblox-faithful values landed 09-25, not yet built |
| `.echk` chunk format | Exporter with no caller | Engine `export_world` calls `bake_to_echk_with` (`echk_export.rs:859`); four Player modules read it | written and type-checked, **never run** |
| Terrain worldgen over MCP | Deterministic generator unreachable | `terrain.generate` plus 11 more terrain bridge verbs, all exposed as MCP tools | `engine_bridge/terrain.rs:1022` |
| Checkpoints | No verb | `snapshot.save/list/diff/revert/cancel_revert`, five MCP tools | built since build 8; 10 of 11 live checks passed on build 10 |
| Procedural textures | Standalone binary, 8 maps, tiled 512 px with visible seams | Library (`lib.rs`, `mips.rs`, `procgen/`) linked by engine and common; 18 seamless 2048 px PBR sets with mips; tangents fixed so normal maps render | seam check: old set failed 50/54, new passes 54/54 |
| Sky and lighting | Bevy 0.19 atmosphere had never rendered a pixel | Sky dome, atmosphere, volumetric clouds, reflections, contact shadows | sky sections built in build 8 |
| Multiplayer | Two half-built stacks | Server authority protocol v3, 56/56 tests | written and tested, **never run live** |

### Unchanged

| Item | Status on Sep 30 |
|---|---|
| WorldState DTO (one serializer for bridge, MCP, Properties) | does not exist |
| Op-log `Update` records | never produced; `parent_tx` is set only in tests |
| `scene.measure`, `scene.observe`, `scene.affordances` | absent |
| Episode runner (sandbox, budget, manifest replay) | `eustress-headless` has none of the three |
| `Substrate` facade and three projections | absent |
| Mass units, `Quantity` type | absent |
| `spatial-llm` (prompt-to-entities, 1,515 lines) | still linked by nothing |
| `generative_pipeline.rs` | still an empty stub with an unused `Abstract` mode |
| Machine quality tier | absent (`RenderTier` exists, but it is per-entity distance LOD, a different thing) |
| Dual channel (`LinkedState` / `LinkedAppearance`) | absent |
| Mesh-to-splat bake, `.spz` support, splat export | absent; importer is `.ply` only |
| Batch create, class-validated `create_entity` | absent |
| SPDX headers | 2 files |

### Worse

| Item | Before | After |
|---|---|---|
| Duplicated Luau and Rune bindings (C4) | 10,270 lines | 10,466 lines |
| Engine crate size (C13) | 556 files | 637 files |

The August engine line count (190,126) undercounted: its shell glob descended
only two directories. Today's full count is 310,404 lines.

---

## 3. What Marble is on 2026-09-30

| Area | Marble |
|---|---|
| Inputs | text, one image, several images, 360-degree panorama, video up to 100 MB, Chisel 3D block-outs (imported meshes since January 2026) |
| Models | 1.1 Plus (largest worlds, auto-expands), 1.1, 1.0 (legacy), 1.0 Draft |
| Speed | Draft about 20 s; navigable preview about 30 s; full world about 5 min; HQ mesh up to an hour, 4 per hour |
| Output | Gaussian splats as `.spz` (default since December 2025) or `.ply`, about 2 M or 500 k splats; collider GLB of 100 k to 200 k triangles; visual GLB of about 600 k textured triangles; 2560x1280 panorama; camera-path video |
| Editing | remove, swap or restyle elements; Expand a region (seam artifacts noted); Composer to stitch worlds |
| Reach | Spark open-source web renderer (2.0 added LOD streaming); VR link (Quest 3, Vision Pro); guides for Blender, Unreal, Unity, Houdini; Isaac Sim, MuJoCo, RoboSuite in robotics workflows |
| Price | API about $0.12 (Draft) to $2.48 (1.1 Plus) per world; HQ mesh about $2.80; commercial use needs the $35/month Pro plan |
| Missing | no physics, no dynamics, no characters, no scripting, no semantic objects, determinism undocumented |

Atlas (announced 2026-09-01) is World Labs' omni model for images and video
with camera control, up to 1440p and one minute. It produces pixels, not a
persistent world, so Marble remains the competitor for this comparison.

---

## 4. Scorecard before: Eustress against Marble today

| Axis | Marble | Eustress today | Leader |
|---|---|---|---|
| Photorealism from one sentence | trained generative model, millions of splats | PBR primitives, seamless materials, sky, clouds, reflections; no global illumination; no splat synthesis | **Marble, decisively** |
| Time to first look | 20 s draft, 30 s preview | unmeasured; one MCP call per entity | **Marble** |
| Input types | six | text through Claude, image through VIGA, CAD, mesh and `.ply` import | **Marble** |
| Export into other tools | splats, two GLBs, panorama, video, engine guides | per-part CAD GLB; `.echk` (own format); no whole-world export | **Marble** |
| Web and VR | Spark viewer, VR link | WASM Player decided, not started; no VR | **Marble** |
| Generative editing | restyle or swap a region by prompt | none | **Marble** |
| Physics and simulation | none | Avian bodies and joints; deformation and fracture verified live; particle fluids; FEA gate | **Eustress, decisively** |
| Physical validity of geometry | collider mesh, no validity checks | authored metric state; checks planned, not built | **Eustress** (structurally; unmeasured) |
| Semantic objects | one splat cloud plus one mesh | every object a classed instance (78 class schemas) with properties and tags | **Eustress, decisively** |
| Scripting and interactivity | none | Luau and Rune, Play mode, server authority | **Eustress, decisively** |
| Determinism | undocumented | pinned physics with a test; bit-exact worldgen; replayable optimizer | **Eustress** |
| Precise editing and versioning | prompt edits | full editor, undo, snapshots with diff and revert, git | **Eustress** |
| Agent control | generation API | about 250 MCP tool definitions, live bridge, AI camera, stepping, raycasts | **Eustress** |
| Large worlds | auto-expand with seam artifacts; web LOD streaming | HLOD, Morton streaming, bit-exact seamless terrain regions | roughly even; unmeasured |
| Cost and ownership | per-world fees; commercial needs Pro | local compute; cost is agent tokens; source-available | **Eustress** |

**Before: Marble leads 6 axes, Eustress leads 8, 1 is even.** Marble's six are
the ones a buyer sees first.

---

## 5. The improvements, in order

Each has an exit that can fail. Code lands only on the orchestrator's wave;
nothing below lands while `BUILD_STATUS.md` says HOLD.

**I1. Ingest Marble worlds (closes the photorealism gap for Marble-seeded worlds).**
- `.spz` decoder alongside `.ply` (Marble's default export).
- A Marble world-package importer: splats become the appearance channel; the
  collider GLB becomes Avian colliders on the state channel; the visual GLB is
  kept as a mesh LOD; real-world scale honoured; provenance
  `IngestSource::Vendor("worldlabs-marble")` on every created record.
- Re-derive ladder: segment the collider mesh into classed instances (floor,
  walls, doors, furniture) so the world becomes editable objects.
- Exit: a Marble world opens in Eustress and is walkable in Play; raycasts hit
  colliders, never splats; at least one segmented object can be moved, and the
  splats covering it move with it.

**I2. Time to first look under 30 seconds.**
- `entities.create_batch`; `world.plan` (brief to class-valid spec, with
  `spatial-llm`'s generator as the first proposer, finally linked); the existing
  `terrain.generate`.
- Exit: from a typed brief, a blocked-out world with terrain, sky, light and
  spawn exists in the live engine in 30 s or less, measured from first call to
  last commit.

**I3. Abstract and Realistic views on one state.**
- `LinkedState` / `LinkedAppearance`; appearance excluded from physics, raycast
  and measurement; a machine `QualityTier` chosen before the first camera
  spawns; `view.set`. Replace the stub `generative_pipeline.rs` rather than
  leave its unused `Abstract` mode beside the real one.
- Exit: toggling views changes zero state bytes; raycasts agree in both views.

**I4. Validity gates: measure World Labs' defining open problem.**
- `scene.measure`, `scene.observe`, `scene.affordances`, then `world.verify`:
  self-intersection volume, scale against the brief, fall-through under
  `sim_step`, walkable reach from spawn.
- Exit: a deliberately broken world fails with the gate named; a good one
  passes. Run the same gates on an ingested Marble world and publish the
  numbers.

**I5. Export parity.**
- Whole-world export package: glTF scene with class and property metadata in
  node extras, splats as `.ply` and `.spz`, a collider GLB. Then a 360-degree
  panorama and a camera-path video from the AI camera.
- Exit: an Eustress world opens in Blender and Unreal with its object
  hierarchy and names intact.

**I6. More inputs.**
- Panorama and multi-image references through VIGA (a panorama becomes six
  cube-face reference views). Update VIGA's default model, which is still a
  Claude 4 generation ID (`claude-sonnet-4-20250514`), to a current Claude 5
  model.
- Exit: one panorama produces a scene VIGA's own verifier scores above its
  threshold.

**I7. Deterministic appearance enrichment.**
- `splat.bake`: sample state surfaces into Gaussians with material colour and
  normal-aligned orientation, bound as appearance. `texture.generate` over the
  new procedural library.
- Exit: a baked-splat world raycasts identically to its mesh source.

**I8. Reach.**
- The WASM Player (decided 2026-09-23; waits on multiplayer's Phase 4 exit
  test). VR after that.
- Exit: an Eustress world plays in a browser tab with physics and scripts
  running, which Spark cannot do.

**I9. The top visual rung.**
- Raycast global illumination (Bevy's experimental Solari path is not adopted
  today) as the highest tier; relightable splats later.
- Exit: the same world renders with measured global illumination at a recorded
  frame cost, or the tier reports why it cannot yet.

**I10. Generated appearance on authored geometry (section 10).**
- The photorealism lead item once I3 lands: render the block-out's pose, depth
  and class segmentation along a camera path, restyle it into photoreal video
  with Cosmos-Transfer, lift it to splats with Brush using the known poses, and
  bind the splats to their state objects.
- Exit: on a held-out camera pose, the splats' rendered depth agrees with the
  state's depth within a set tolerance, and raycasts still hit only state.

---

## 6. Scorecard after

Photorealism is split into two rows, because ingest changes one case and not
the other. That makes 16 rows.

| Axis | After the improvements | Leader |
|---|---|---|
| Photorealism, Marble-seeded worlds | same splats as Marble, plus state | **parity** (I1) |
| Photorealism, in-house only | generated appearance on authored geometry (I10), PBR, baked splats, sky, GI tier | **contested** until Marble Bench measures it (section 10) |
| Time to first look | 30 s target for a blockout | **parity** (I2) |
| Input types | text, image, panorama, multi-image, CAD, mesh, Marble worlds | **parity**; only Eustress ingests the competitor (I1, I6) |
| Export into other tools | glTF with semantics, splats, collider, panorama, video | **Eustress**: exports objects, not one blob (I5) |
| Web and VR | WASM Player with physics and scripts | **Eustress** on web once shipped (I8); Marble keeps VR until ours exists |
| Generative editing | prompt edits through the agent on classed objects | **contested** |
| Large worlds | seamless regions, HLOD, streaming; still unmeasured head to head | **contested** |
| Physics and simulation | unchanged | **Eustress** |
| Physical validity of geometry | now measured by I4 | **Eustress** |
| Semantic objects | unchanged; ingested worlds become objects too | **Eustress** |
| Scripting and interactivity | unchanged | **Eustress** |
| Determinism | unchanged | **Eustress** |
| Precise editing and versioning | unchanged | **Eustress** |
| Agent control | plus measure, observe, affordances, verify | **Eustress** |
| Cost and ownership | unchanged | **Eustress** |

**After: Eustress leads 10 rows, 3 are at parity, 3 are contested, and Marble
leads none outright.** Marble keeps VR until Eustress builds it. The contested
photorealism row is decided by Marble Bench, not by this table, and Marble's
mature product should be expected to win its early rounds on polish.

---

## 7. Marble Bench: the head-to-head that decides it

Twelve briefs spanning interiors, architecture, streets, industrial, forest,
coast and mountain. For each brief, three worlds:

- **M:** Marble 1.1 output.
- **E:** Eustress in-house (no Marble input).
- **E+M:** Eustress after ingesting M.
- **E+G:** Eustress with generated appearance on its own block-out (section 10).

Measured on every world, with the numbers published:

| Measure | How |
|---|---|
| Visual preference | blind pairwise judgment from 8 fixed camera poses per world |
| Fall-through | 100 dropped probes, `sim_step(600)`; count that end below the walkable surface |
| Self-intersection | interpenetrating collider volume as a share of total collider volume |
| Scale error | brief-stated dimensions (door 2.0 to 2.2 m, ceiling, road width) against measured |
| Semantic coverage | share of the brief's must-haves present as separate classed objects |
| Behaviour | brief-stated interactions that work in Play (a door opens, a light switches) |
| Determinism | same brief and seed give an identical WorldDb digest (E only) |
| Time and cost | wall time to first look and to finish; tokens or dollars |

**Eustress beats Marble when both hold:**
1. E+M is at least equal to M on visual preference and strictly better on every
   other measure.
2. E is strictly better than M on every state measure, and within an agreed
   visual margin on the interior and architecture briefs.

A loss on either is reported as a loss.

---

## 8. Decisions

- **2026-10-01, photorealism comes from generated appearance** (section 10),
  painted over authored geometry. Ingesting Marble worlds (I1) stays an optional
  input and a benchmark arm.
- **2026-10-01, Brush builds the splats, diffusion makes the images, and the
  code is our own** with no dependency beyond Brush. Design:
  `SPLAT_OBJECTS_AND_INHOUSE_DIFFUSION.md`. Lyra and Cosmos are no longer the
  pipeline; at most, Cosmos-Transfer serves once as a teacher that produces
  training pairs.
- **2026-10-01, the priority is splat objects:** segmentation, classification,
  separate objects with accurate colliders, and per-instance properties. That
  design delivers them before any diffusion model exists.

Still open:
- **Where training and generation run.** Option one: a local workstation GPU
  (cheaper once bought). Option two: rented cloud GPUs (nothing to buy, a cost
  per run).
- **Whether to bootstrap with a teacher.** Option one: distil once from
  Cosmos-Transfer1 (fastest; commercially licensed). Option two: train only on
  licensed photographs and our own renders (fully independent; slower to reach
  realism).

---

## 9. Constraints

- Build 15 is building and landings are on HOLD. Every improvement above needs
  the orchestrator's wave and an orchestrator build.
- Several "after" items are written but never run (`.echk`, server authority,
  the latest gravity values). A benchmark run is also their first live test.
- No Lyra, Cosmos or Brush model has been downloaded or run for Eustress yet.
  Every figure in section 10 is from the models' own documentation.
- C4 is still growing. It does not block any item here, but every scripting
  feature the benchmark's behaviour measure needs is written twice until it is
  unified.

---

## 10. Photorealism through generated appearance

Direction set by McKale on 2026-10-01: get photorealism from NVIDIA's Lyra, fed
by images or video, with a generative model bridging sentences to those images.
Checked against each model's own documentation:

| Model | What it does | Weights licence | Hardware and time | Caveats |
|---|---|---|---|---|
| Lyra 1.0 (September 2025, ICLR 2026) | one image or a sentence to a 3D Gaussian scene in one forward pass; a monocular video to a moving scene; writes `.ply` | NVIDIA Open Model License: commercial use allowed; NVIDIA claims no ownership of outputs; rights end if a safety guardrail is bypassed | tested on A100 and H100; about 43 GB peak with full offloading; time not published | built on Cosmos-Predict1 and GEN3C; its splat decoder was trained only on Cosmos-generated synthetic data; scale is not metric |
| Lyra 2.0 (April 2026; weights July 20, 2026) | image, camera path and per-segment captions to a long, explorable walkthrough, lifted to `reconstructed_scene.ply` or meshes | NVIDIA Internal Scientific Research and Development Model License: research use; a product needs a custom licence | one H100 80 GB: about 9 min per 80 frames (about 35 s in its distilled mode, which weakens prompt following), then about 1 min to reconstruct | built on Wan 2.1 14B; may hallucinate objects along the path; the camera path is relative to estimated depth, not metric |
| Cosmos-Predict2 2B Text2Image | a sentence to a 1280x704 image | NVIDIA Open Model License | 26 GB; 3 to 12 s per image | the licence-aligned sentence bridge for Lyra 1 |
| Cosmos-Transfer1 | restyles video while holding depth, segmentation or edges | NVIDIA Open Model License | 7B class | lets generated appearance follow Eustress geometry |
| Brush | trains Gaussian splats from images with known camera poses; writes `.ply` | Apache-2.0 | Rust, Burn and wgpu; Windows, Linux, macOS, web, Android | keeps the reconstruction step in Rust |

**On the GAN bridge.** A GAN can turn sentences into images, but it is the
weaker tool here. Diffusion and flow models lead on following a sentence and on
variety; NVIDIA made Lyra 1's own input images with FLUX; and distilled
diffusion already reaches GAN-like speed (Lyra 2's distilled mode is one
example). The recommendation is Cosmos-Predict2's text-to-image model. Claude
stays the sentence interface: it turns the brief into the World Spec, region
captions and camera paths, and the image model only draws them.

### The flagship pipeline: generated appearance on authored geometry

Marble's Chisel takes a block-out as a hint and folds it into a splat. Eustress
keeps the block-out as the world and generates only its skin:

1. **State first.** Claude blocks out the world (rung A1) in metric units:
   classes, colliders, terrain.
2. **Render the truth.** The AI camera flies a planned path over the Abstract
   view and records, per frame, the exact camera pose, metric depth and a class
   segmentation (the labelled G-buffer the roadmap lists as new).
3. **Paint.** Cosmos-Transfer turns those depth and segmentation frames into
   photoreal video along the same path, guided by the region captions.
4. **Lift.** Brush trains Gaussian splats from that video using the known poses
   and depth, so reconstruction never guesses camera motion or scale.
5. **Bind.** The splats become the appearance channel of their state objects.
   Physics, raycasts and measurement keep using the authored geometry.
6. **Check.** A new gate compares the splats' rendered depth with the state's
   depth from held-out poses. Where they disagree, the agent either adds state
   geometry under splats that show something solid, or culls the splats.

This answers World Labs' "defining open problem" directly: scale and solidity
come from authored, validated geometry, and the generative model is trusted only
for what it does well, which is appearance.

Two faster paths for when there is no block-out:
- **Sentence to splats.** Cosmos text-to-image, then Lyra 1, then Eustress
  imports the `.ply` (the importer already reads `.ply`), extracts colliders
  (Tier A already exists), and fixes scale against the brief's stated
  dimensions. Good for vistas and backdrops.
- **Lyra 2 explorable worlds.** Internal research and Marble Bench only, until
  NVIDIA grants a product licence.

### Rules

- Learned models author appearance offline. Nothing learned runs inside the
  render loop or physics. The world looks the way it does because of geometry,
  materials, splats and light, rendered deterministically.
- Every generated asset is stored in the Space with provenance: model and
  version, seed, prompt, input hashes. A world replays bit-exact from its stored
  assets. Regenerating from the same seed is best effort, because diffusion
  sampling is not guaranteed identical across GPUs and drivers.
- Generation runs as a separate service (PyTorch, Linux, 48 GB or more, or a
  cloud A100 or H100) behind the existing `GenerationBackend` contract in
  `genesis::ingest`. No Python enters the engine. The splats it produces play
  on ordinary GPUs through the renderer Eustress already has.
- Brush's wgpu and Burn versions must be checked against the engine's before it
  is linked in; until then it runs as its own process.

### Superseded by the in-house design

On 2026-10-01 McKale chose Brush, diffusion and our own code. The flagship
pipeline above keeps its shape, with in-house parts in place of NVIDIA's models:
our diffusion model paints the views, Brush lifts them to splats, and objects
separate by construction. Full design: `SPLAT_OBJECTS_AND_INHOUSE_DIFFUSION.md`.

### Effect on the scorecard

"Photorealism, in-house only" moves from Marble to **contested**: Eustress now
has a trained generative prior for appearance, but no Lyra or Cosmos world has
been measured against Marble yet. Marble Bench gains a fourth arm, **E+G**.

---

## Sources

- [World Labs Marble explained (updated August 2026), invideo](https://invideo.io/blog/world-labs-marble-3d-worlds/)
- [World Labs splits world models into three types, Tech Times, 2026-06-06](https://www.techtimes.com/articles/317927/20260606/feifei-lis-world-labs-splits-world-model-three-types-marble-targets-simulation-linchpin.htm)
- [World Labs unveils Atlas, Crypto Briefing, 2026-09-01](https://cryptobriefing.com/world-labs-atlas-multimodal-world-model/)
- [Marble: a multimodal world model, World Labs](https://www.worldlabs.ai/blog/marble-world-model)
- [Marble with NVIDIA Isaac Sim, NVIDIA developer blog](https://developer.nvidia.com/blog/simulate-robotic-environments-faster-with-nvidia-isaac-sim-and-world-labs-marble/)
- [Marble review: the state of AI 3D world generation in 2026, AI Indigo](https://aiindigo.com/blog/marble-review-the-state-of-ai-3d-world-generation-in-2026)
- [Lyra repository (Lyra 1.0 and 2.0 READMEs, licences, runtimes), NVIDIA](https://github.com/nv-tlabs/lyra)
- [Lyra model card, Hugging Face](https://huggingface.co/nvidia/Lyra)
- [Project Lyra, NVIDIA Toronto AI Lab](https://nv-tlabs.github.io/Project-Lyra/)
- [Cosmos-Predict2 2B Text2Image model card](https://huggingface.co/nvidia/Cosmos-Predict2-2B-Text2Image)
- [Cosmos-Transfer1, NVIDIA Research](https://research.nvidia.com/labs/cosmos-lab/cosmos-transfer1)
- [NVIDIA Open Model License](https://www.nvidia.com/en-us/agreements/enterprise-software/nvidia-open-model-license)
- [Brush, Gaussian splat training in Rust](https://github.com/ArthurBrussee/brush)
