# 03 — PROMPT FILE SCHEMA

**Status:** Normative. Every prompt file in `docs/PROMPTS/` must conform.
**Audience:** anyone authoring a prompt for this library — human or agent.

This spec is self-contained. You do not need to have read `00_MASTER_PROTOCOL.md`,
`01_CRITIC_RUBRIC.md`, or `02_CAPTURE_HARNESS.md` to author a conforming prompt; everything a
prompt author must know is restated here.

---

## 0. The one rule everything else serves

**A prompt must be runnable COLD.**

The agent that executes it (an "L2 specialist") receives exactly one thing: the prompt file. No
conversation history. No prior turn. No "as we discussed." No access to the author. If the executing
agent has to guess, infer, or ask, the prompt is defective.

Concretely, a conforming prompt never contains: "the file we changed earlier", "continue from where
you left off", "the usual approach", "see the previous item", "you know the drill", an unresolved
pronoun referring to something outside the file, or a path that is not spelled out in full.

Test before you ship a prompt: **hand it to someone who has never seen this repository. Can they
begin work without asking a single question?** If not, fix the prompt.

---

## 1. Project invariants every prompt inherits

State-of-the-world facts an executing agent must respect. Restate any that are load-bearing for your
item directly in the prompt body — do not rely on the agent having read this file.

- **Eustress is an AI-native simulation substrate / world engine. It is NEVER called a game engine.** 3D rendering and the ECS are implementation details in service of that goal.
- **The licence is PolyForm Shield 1.0.0.** Say **source-available**. Never say open source.
- **The physics engine is Avian.** Never Rapier.
- **Slint is Rust.** `.slint` files compile to Rust. Never frame "Rust-first" as opposed to Slint.
- **Units are meter-native.** Studs are a display unit only.
- **Builds take 10–15 minutes. One cargo build at a time.** The workspace shares a single `target/`; concurrent builds produce link failures. Never kill a build mid-compile.
- **Validate with `cargo run`, not `cargo check`.**
- **Documents must read as though always correct.** No "previously we thought", no changelog residue in the body, no self-justifying commentary.
- **Never state a measured number that was not measured.** Every number is `MEASURED` (cite the command, hardware, and artifact path), `TARGET` (labelled inline), or `CONFIG DEFAULT` (labelled).
- **Ground every path claim.** If you have not verified a path exists, do not cite it.

---

## 2. File conventions

| Property | Rule |
|---|---|
| Location | `docs/PROMPTS/items/<ID>_<slug>.md` |
| Filename | The stable ID, then an underscore, then a lowercase-hyphen slug. E.g. `G3.04_shadow-penumbra-falloff.md` |
| One item per file | Never two items in one file. Split instead. |
| Format | Markdown with a YAML front-matter block, then the fixed section sequence in §4 |
| Immutability of ID | Once an ID is used, it is never reused or renumbered, even if the item is killed |
| Encoding | UTF-8, LF line endings, no BOM |

---

## 3. Required front-matter fields

YAML block, first thing in the file, delimited by `---`. Every field below is **required** unless
marked optional. Unknown fields are permitted but ignored.

| Field | Type | Rule |
|---|---|---|
| `id` | string | Stable identifier, `<PHASE>.<NN>`, e.g. `G3.04`. Never reused. Phase prefix must be one of §3.1. |
| `title` | string | ≤ 80 chars. Names the artifact-level outcome, not the activity. "Variable-width shadow penumbra", not "work on shadows". |
| `workload` | string | Exactly one primary tag from §3.2. |
| `workload_secondary` | list | Zero or more additional tags from §3.2. May be empty. |
| `phase` | string | Exactly one tag from §3.1. Must match the ID prefix. |
| `depends_on` | list of IDs | Other prompt IDs that must be `PASSED` first. Empty list if none. No cycles. |
| `blocks` | list of IDs | Optional. Informational; the dependency graph is authoritative through `depends_on`. |
| `tier` | enum | `S` \| `M` \| `L` \| `XL`. Budget envelope, §3.3. An omitted tier defaults to `M`. |
| `token_envelope` | integer | Must equal the tier's value in §3.3 unless an override is justified in `notes`. |
| `wallclock_envelope` | string | Must equal the tier's value in §3.3 unless overridden. |
| `max_builds` | integer | Must equal the tier's value in §3.3. |
| `critic_gate` | list | Which rubric dimensions gate this item, from §3.4. May be `[]` for items whose exit criterion is purely mechanical (e.g. a CI change) — but then `exit_criterion` must be unusually tight. |
| `capture_recipe` | string | Path to the capture recipe under `docs/PROMPTS/harness/recipes/`, or `none` if `critic_gate` is `[]`. |
| `artifact` | string | Path (or glob) the item must leave behind as evidence. Required. Not "a report" — a specific path. |
| `escalation` | string | The trigger condition that jumps this item straight to STALL, beyond the standard ladder. |
| `status` | enum | `DRAFT` \| `READY` \| `IN_PROGRESS` \| `PASSED` \| `STALLED` \| `KILLED`. Authored as `DRAFT`; an L1 promotes to `READY`. |
| `notes` | string | Optional. Author notes, envelope overrides with justification. Never instructions — instructions live in the body. |

### 3.1 Phase tags (`phase`)

Ordered. A phase may not open until its predecessors' hard dependencies are `PASSED`.

| Tag | Phase | Concern |
|---|---|---|
| `G0` | Security, Rights & External Reality | The trust boundary around the agent tool surface and the plugin loader; the licence and data-rights terms a third party is handed; and the countable facts that someone outside the company arrived, ran, or reported |
| `G1` | Capture & Measurement Harness | Item zero. Deterministic, reproducible, content-addressed capture. Nothing downstream is measurable without it. |
| `G2` | Determinism & Numerical Trust | Byte-identical reruns; instrumented time-compression; sim-time (not wall-time) alerting |
| `G3` | Render Fidelity | Materials, lighting, shadow, tone response |
| `G4` | Motion & Temporal Stability | Frame-time distribution, aliasing, LOD/streaming pop, camera weight |
| `G5` | Scale & Streaming | Measured entity counts at measured frame rates, replacing config-default claims |
| `G6` | Studio UI Craft | Alignment, state completeness, honest feedback, panel architecture |
| `G7` | Agent Loop Closure | The observe→act→judge loop running headless, in CI, with no desktop session |

### 3.2 Workload tags (`workload`, `workload_secondary`)

| Tag | Workload | The question it answers | Evidence type |
|---|---|---|---|
| `W1` | Provable Quality | Does the artifact beat what the buyer already trusts, judged blind? | Critic scorecards over capture bundles |
| `W2` | Revenue Rail | Can money actually move, end to end, once? | An executed transaction or signed agreement plus its receipt |
| `W3` | Trust & Verifiability | Can a stranger reproduce our claims without us? | Byte-identical reruns; green CI; signed binaries |
| `W4` | Vertical Proof | Does one vertical work end to end, with no unwired buttons? | A recorded end-to-end session in that vertical |
| `W5` | Extension Surface & Merit Ladder | Can someone outside the company build on this, and on what terms? | A third-party extension running unmodified; a written contributor terms document |
| `W6` | Operator Leverage | Does the solo operator's throughput per wall-clock hour go up? | Before/after wall-clock on a fixed task list |

**Evidence declaration rule.** The `artifact:` field must name the specific file that constitutes the
primary workload's evidence. An item that cannot name such a file is not an item; it is a wish.
Rewrite it or drop it.

**W5 scope note.** W5 is *not* "grow an open-source community" — the licence forbids it. Under
PolyForm Shield, W5 covers four things that are actually available: the extension surface (plugins,
Rune and Luau scripts, MCP tools, and end-products built *with* the substrate, all permitted at no
cost and royalty-free); a published merit ladder from user to trusted contributor; contributor
licensing (the repo has no `CONTRIBUTING.md` and no CLA or DCO, so inbound IP is currently
undefined); and a defined path from free adopter to a negotiated commercial licence. Anything
requiring a licence change — distro/registry packaging under open-source norms, third-party hosting
as a service, or promising a contributor they may build an adjacent commercial tool — is a **human
decision** and must never be authored as an agent item.

### 3.3 Tier envelopes (`tier`)

| Tier | Typical item | `token_envelope` | `wallclock_envelope` | `max_builds` | Max iterations |
|---|---|---|---|---|---|
| `S` | Doc, spec, analysis, no compile | 60000 | `1h` | 0 | 3 |
| `M` | Single-crate change, one measurement | 200000 | `4h` | 6 | 6 |
| `L` | Cross-crate change, capture + Critic loop | 500000 | `2d` | 12 | 9 |
| `XL` | New subsystem | 1200000 | `5d` | 20 | 9 |

Build count, not token count, is usually the real ceiling: an XL item at 20 builds is already ~4
hours of pure compile. Design items so one build validates several changes. **Consuming 150% of the
envelope triggers a STALL, not a quiet extension.**

### 3.4 Critic gate dimensions (`critic_gate`)

Each is scored 0–10 by an independent, blinded Critic. **Pass floor is 8.0 on every listed
dimension** — the mean is irrelevant; one dimension below floor fails the item.

| Tag | Dimension | What it measures |
|---|---|---|
| `D1` | First-three-seconds impact | What a stranger concludes in 3.0 s, before any explanation |
| `D2` | Material and lighting realism | Whether surfaces behave like materials and light behaves like light |
| `D3` | Motion and temporal stability | Convincing motion + a stable image while moving (has hard numeric sub-floors) |
| `D4` | UI craftsmanship | Whether the studio surface was designed or merely assembled |
| `D5` | Simulation believability and numerical trust | Does it look right AND would a domain expert trust the numbers |
| `D6` | Overall coherence | One designed system, or capable parts that never met |
| `D7` | The preference question | Blind, forced choice across 5 randomised trials; floor is 4 of 5 |

Three properties of the gate that shape how you write an item:

1. **The Critic never sees the executing agent's self-report** — not the result block, not commit messages, not a README, not a caption baked into a capture. Evidence is frames, measured numbers, and `path:line`. Write the item so its evidence survives that stripping.
2. **The Critic cites or fails.** Every score carries a citation (`frame:0072`, `path/file.rs:118`, `ft_p99=22.4ms`). An uncited score auto-fails the whole scorecard.
3. **The Critic can refuse to pass something that clears every number** (the "wow gate"), but can *never* pass something that misses a number. Design exit criteria assuming the numeric floor is the beginning of the argument, not the end.

---

## 4. Required body sections

Exactly these, in this order, with these headings. No extra top-level sections. Add depth with
subsections.

```
## 1. Objective
## 2. Context you need (self-contained)
## 3. Scope
## 4. Approach constraints
## 5. Exit criterion
## 6. Critic gate
## 7. Loop cadence and escalation
## 8. Artifact
## 9. Definition of NOT done
```

### 4.1 `## 1. Objective`

Two to four sentences. What will be true when this item passes, stated as a property of the
artifact, not as an activity. Wrong: "Improve shadow quality." Right: "Shadow penumbra width scales
with occluder distance, verifiable at 0.2 m and 3.0 m in the same frame."

### 4.2 `## 2. Context you need (self-contained)`

Everything the executing agent must know, restated here. Every relevant path spelled out in full and
verified to exist. Every relevant constraint. Every known-broken thing in the blast radius.

**Cite nothing the agent cannot open.** If you reference `docs/AUDIT/11_SIMULATION_DEBUGGER.md`,
either the agent can read it, or you must quote the relevant finding inline. Prefer quoting — it
survives the file moving.

### 4.3 `## 3. Scope`

Two explicit lists. Both are contractual: the executing agent may edit files in the first list and
**may not** edit anything else. If the fix requires a file outside scope, that is an escalation, not
a licence.

```
### In scope — files this item may edit
- eustress/crates/engine/src/foo.rs
- eustress/crates/engine/assets/shaders/bar.wgsl

### Out of scope — do not edit
- Anything under .github/workflows/   (never modify CI to make a gate pass)
- docs/PROMPTS/01_CRITIC_RUBRIC.md     (never readable/editable by an executing agent)
- Any capture already hashed into a provenance manifest
```

Three out-of-scope entries are standing and belong in every prompt: CI workflows, the Critic rubric,
and any already-hashed capture.

**Contested paths.** A source file claimed in scope by items in more than one pack has exactly one
owner, named in `04_FILE_OWNERSHIP.md`. That file is normative and wins over an item's scope list.
Before adding a path to an in-scope list, check it there: if another pack owns it, the item carries
`depends_on: [<owner>]` and the path belongs in its out-of-scope list instead.

### 4.4 `## 4. Approach constraints`

What the agent must not do, especially the tempting shortcuts. Always state the anti-reward-hacking
rule in the item's own terms:

> Changing the measurement instead of the artifact fails this item, whatever number results.
> Loosening a tolerance, shrinking the frame set, excluding a scene, lowering a resolution, or
> disabling an assertion are all measurement changes. If the measurement is genuinely wrong, report
> `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.

### 4.5 `## 5. Exit criterion`

**The single most important section.** It must be falsifiable by someone who was not present, and it
must come with the literal command or MCP call that measures it.

Required shape:

```
### Criterion
<one sentence, with a number and a comparator>

### Measurement
Command:
    <the exact, literal, copy-pasteable command — or the exact MCP tool call with its JSON input>

Expected output shape:
    <what the pass looks like in the output>

Pass condition:
    <the comparison, with the threshold>
```

Rules:
- The command must be literal. Not "run the benchmark" — the actual command line.
- MCP calls are given as tool name plus the exact JSON input object.
- Never verify by side effect. "The file exists" is not a pass; grep the exit-code marker or the measured value.
- If the criterion is inherently perceptual, the numeric part still exists (the Critic gate), and the Measurement block names the capture recipe that produces the bundle.

### 4.6 `## 6. Critic gate`

Restate which dimensions gate the item and what the executing agent should therefore prioritise.
Name the capture recipe. State the floor explicitly (8.0 per gated dimension). If `critic_gate` is
`[]`, say why and point at the mechanical criterion that replaces it.

### 4.7 `## 7. Loop cadence and escalation`

State the ladder in the item's own terms:

```
Iterations 1-3 : approach A
   -> if still failing, MANDATORY approach change. A parameter change is NOT an approach change.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with worst-dimension Critic score moving < 0.5
                  AND the exit measurement moving < 5%
  - Budget      : 150% of the tier envelope consumed
  - Item-specific: <whatever the front-matter `escalation` field says>
```

A STALL is a respectable outcome and produces a one-screen packet for the human containing: the
floor missed and the best value achieved; the path to the best artifact; the three approaches with
their failure signatures; the best current root-cause theory with evidence; consumption vs envelope;
and a **decision request that is exactly one of** `LOWER the floor to <x'>` / `FUND approach D` /
`DEFER behind <id>` / `KILL`, plus a recommendation. A packet asking the human to "review the
situation" instead of choosing among four named options is malformed.

### 4.8 `## 8. Artifact`

The exact path the item must leave behind, and what a reader will find there. This is the workload
evidence. Not "a summary" — a file at a path.

### 4.9 `## 9. Definition of NOT done`

Enumerate the near-misses that will feel like success. This section is where prompts earn their
keep. Every entry is a specific failure mode you predict for this item.

---

## 5. Authoring checklist

Before setting `status: READY`:

- [ ] A stranger could execute this with no questions.
- [ ] Every path cited has been verified to exist.
- [ ] Every number is labelled `MEASURED` / `TARGET` / `CONFIG DEFAULT`.
- [ ] The exit criterion contains a number, a comparator, and a literal command.
- [ ] The measurement checks the exit code or the value, never a side effect.
- [ ] Scope lists are explicit and include the three standing out-of-scope entries.
- [ ] `depends_on` introduces no cycle.
- [ ] `artifact` names a specific path, not a category.
- [ ] `tier` and the three envelope fields agree with §3.3.
- [ ] `## 9. Definition of NOT done` has at least three entries.
- [ ] Nothing in the file leaks the Critic's held-out criteria.
- [ ] No invariant from §1 is violated (no "game engine", no "open source", no Rapier, no studs-as-native).

---

## 6. Worked example — complete and conforming

Copy the structure exactly. This is `docs/PROMPTS/items/G3.04_shadow-penumbra-falloff.md`.

````markdown
---
id: G3.04
title: Variable-width shadow penumbra with distance-correct falloff
workload: W1
workload_secondary: [W3]
phase: G3
depends_on: [G1.01, G1.04]
blocks: [G3.07]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D2, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G3_shadow_falloff.json
artifact: docs/PROMPTS/artifacts/G3.04/penumbra_measurement.json
escalation: >
  If achieving the floor requires a shadow technique whose cost exceeds 2.0 ms/frame at
  3840x2160 on the harness reference GPU, STALL immediately rather than trading D3 frame-time
  headroom for a D2 score.
status: DRAFT
notes: >
  Tier L rather than M because the capture + Critic loop is required and the change touches the
  render pipeline, so each iteration costs a full engine build.
---

## 1. Objective

Shadow penumbra width in the Eustress renderer scales with occluder-to-receiver distance rather
than being constant. When the harness exterior scene is captured, a 0.2 m-distant occluder and a
3.0 m-distant occluder visible in the same frame produce measurably different penumbra widths, in
the correct direction and the correct approximate ratio.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. It is never described as a game engine. Rendering is an implementation
detail in service of that goal, which is why this item is gated on a *measurement* and not on
taste.

**Where shadows come from today.** The renderer is Bevy 0.19 PBR with a directional light and
cascaded shadow maps. Cascade distance is influenced by the env var `EUSTRESS_SHADOW_DISTANCE`.
There is no percentage-closer-soft-shadows path and no contact-hardening term; penumbra width is
therefore governed by the shadow map's filter kernel, which is constant in texel space, so it does
not vary with occluder distance.

**Why this specific defect matters.** The Critic rubric's material-and-lighting dimension (D2)
contains a check, `M6`, defined as: "penumbra widens with distance from the occluder." A blind
professional evaluator checks contact and falloff cues before almost anything else. `M6` is one of
eight checks, and D2 requires all eight to pass to reach its floor of 8.0. So this single defect
caps the entire render-fidelity phase.

**What is on hold and must not be treated as available.** The file
`eustress/crates/engine/src/photoreal.rs` records that GTAO, TAA, bloom, and auto-exposure are on
hold, because the relevant Bevy post-process crates have not published stable releases against the
pinned engine version. Only filmic tonemapping ships. Do not attempt to solve this item by adding
ambient occlusion or a post-process pass — those are separate items and the dependencies are not
available.

**The custom shader surface is small.** Only five WGSL files exist outside vendored wgpu:
`eustress/crates/engine/assets/shaders/billboard.wgsl`,
`eustress/crates/engine/assets/shaders/moon_phase.wgsl`,
`eustress/crates/engine/assets/shaders/sun_disc.wgsl`,
`eustress/crates/engine/src/moon_phase.wgsl`, and
`eustress/crates/engine/src/parts/instanced_material.wgsl`. None of them is the shadow path. A
shadow change means either engaging Bevy's shadow pipeline through its public extension points or
adding a sixth shader — decide which, and say which in your result block.

**Build reality.** A full engine build takes 10-15 minutes. Only one cargo build may run at a time;
the workspace shares a single `target/` directory and concurrent builds produce link failures.
Never kill a build mid-compile. Validate with `cargo run`, not `cargo check` — `cargo check` will
not catch the pipeline-registration failures this item is most likely to produce.

**Physics is Avian.** Nothing in this item touches physics, but do not "helpfully" adjust anything
under `eustress/crates/common/src/physics/`.

**Prerequisites already satisfied.** `G1.01` delivered the parameterised AI camera, so
`ai_camera_capture` accepts `width`, `height`, and `out_path`. `G1.04` delivered the
`eustress-capture` binary. Both are done; use them, do not rebuild them.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/` — the render/lighting path only
- `eustress/crates/engine/assets/shaders/` — a new shader file is permitted if justified
- `eustress/crates/engine/Cargo.toml` — only if a dependency is genuinely required, and say why

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/spaces/harness/` — the harness scenes are frozen; editing one invalidates every
  archived comparison that used it
- `eustress/crates/engine/src/photoreal.rs` — the post-stack is a separate item
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Widening the measurement tolerance, moving the occluders closer together, cropping the analysis
  region, lowering the capture resolution, or changing the harness scene are all measurement
  changes. If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with
  evidence and stop.
- Do not trade frame time for shadow quality without measuring the trade. The escalation trigger in
  the front matter is a hard limit: 2.0 ms/frame added cost at 3840x2160 on the harness reference
  GPU. Measure it; do not estimate it.
- Do not introduce a technique that produces temporal instability. Motion stability is a separate
  gated dimension and a shimmering penumbra fails it. If your approach adds noise, it needs a
  temporal story before it counts.
- Batch your verification. Twelve builds is the whole budget for three approaches; one build should
  validate several changes.

## 5. Exit criterion

### Criterion
In harness scene `S3_exterior_single`, measured penumbra width at a 3.0 m occluder-to-receiver
distance is at least **2.5x** the width at 0.2 m, and both are measured from the same captured
frame.

### Measurement

Command:

    cargo run --release --bin eustress-capture -- \
        --recipe docs/PROMPTS/harness/recipes/G3_shadow_falloff.json \
        --subject HEAD \
        --control 71ccf6fe \
        --out docs/PROMPTS/artifacts/bundles \
        --trials 5 \
        --verify-determinism

    cargo run --release --bin penumbra-measure -- \
        --bundle docs/PROMPTS/artifacts/bundles/<hash>/ \
        --side subject \
        --frame FS-MATERIAL/07 \
        --probe-near "x=812,y=1440,axis=x" \
        --probe-far  "x=2140,y=1440,axis=x" \
        --out docs/PROMPTS/artifacts/G3.04/penumbra_measurement.json

Expected output shape:

    {
      "near_occluder_distance_m": 0.2,
      "far_occluder_distance_m": 3.0,
      "near_penumbra_px": 4.1,
      "far_penumbra_px": 11.8,
      "ratio": 2.88,
      "frame_time_delta_ms": 0.7
    }

Pass condition:

    ratio >= 2.5  AND  frame_time_delta_ms <= 2.0

Verify by reading the emitted value, not by observing that the JSON file exists. Both binaries
must exit 0; grep the exit code, do not infer success from files appearing.

## 6. Critic gate

Gated on **D2 (material and lighting realism)** and **D6 (overall coherence)**, floor **8.0 each**.
The mean is irrelevant — either dimension below 8.0 fails the item.

D2 contains eight checks and all eight must pass; this item targets `M6` (penumbra widens with
occluder distance) but must not regress `M5` (contact shadows darken at the contact point). Capture
recipe: `docs/PROMPTS/harness/recipes/G3_shadow_falloff.json`.

D6 is included because a shadow technique that visually diverges from the rest of the lighting
model — for instance, soft ground shadows against hard shadows on every other surface — breaks
coherence even when it improves realism locally.

Note that the Critic never sees anything you write about your own work, and every score it gives
must cite a specific frame or measured value. Design the change so the improvement is visible in a
single still frame; an improvement only perceptible in motion will not be credited here.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A
   -> if still failing, MANDATORY approach change. Tuning a kernel radius is NOT an
      approach change; moving from a fixed kernel to a blocker-search technique is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  the measured ratio moving < 5%
  - Budget      : 750k tokens or 12 builds consumed (150% of the L envelope)
  - Item-specific: added frame cost exceeds 2.0 ms/frame at 3840x2160 on the harness
                   reference GPU (see front matter)
```

The stall packet must fit one screen and must request exactly one of: LOWER the ratio floor to a
stated value with the stated consequence; FUND a specific approach D with an estimate and a reason
it is materially different; DEFER behind a named blocking item; or KILL with a statement of what the
program loses.

## 8. Artifact

`docs/PROMPTS/artifacts/G3.04/penumbra_measurement.json`

A reader finds: the two probe distances in meters, both measured penumbra widths in pixels, their
ratio, the measured frame-time delta, the bundle hash the measurement was taken from, the frame
index, the commit, and the GPU and driver version. This file is the W1 evidence for the item and is
what the phase report cites.

Also archived alongside it, and referenced by the phase report: the capture bundle manifest and the
Critic scorecard.

## 9. Definition of NOT done

- The ratio clears 2.5 but the added frame cost exceeds 2.0 ms. That is a trade, not a fix, and the
  escalation trigger fires.
- The ratio clears 2.5 in a hand-built test scene rather than in the frozen harness scene `S3`.
  Non-harness results are not admissible.
- Penumbra varies with distance but shimmers under camera motion. Temporal instability fails a
  different gated dimension and the item will be re-opened.
- `M6` passes and `M5` regresses — soft shadows that no longer darken at the contact point read as
  objects floating, which is a worse defect than the one being fixed.
- The change is visible only at 3840x2160 and disappears at 1920x1080. Resolution-dependent quality
  is a sampling artifact, not a lighting improvement.
- The Critic passes D2 but refuses the wow gate, citing that penumbra is now correct while the
  overall lighting still reads as placed sources rather than a described environment. That is a
  legitimate refusal; the item is not done.
- The measurement passes because the probe coordinates were moved to a more favourable region of
  the frame. That is a measurement change and fails the item outright.
````

---

## 7. Common defects in authored prompts

| Defect | Symptom | Fix |
|---|---|---|
| Activity objective | "Improve X" | State the property that will be true of the artifact |
| Unfalsifiable exit | "looks better" | A number, a comparator, and a literal command |
| Side-effect verification | "the file is created" | Read the emitted value; grep the exit code |
| Context by reference | "see the audit doc" | Quote the finding inline |
| Open scope | no out-of-scope list | Always list, always include the three standing entries |
| Missing NOT-done | section absent or one bullet | At least three predicted near-misses |
| Envelope mismatch | tier `M`, envelope 800k | Match §3.3 or justify in `notes` |
| Rubric leakage | prompt quotes held-out criteria | Never; the executing agent must not see them |
| Cyclic dependency | A depends on B depends on A | Restructure; the graph must be acyclic |
| Invariant violation | "game engine", "open source", "Rapier", studs-as-native | See §1 |
