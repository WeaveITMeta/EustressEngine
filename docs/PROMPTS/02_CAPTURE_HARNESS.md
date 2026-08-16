# 02 — BLIND A/B CAPTURE HARNESS

**Status:** Normative. **This is item zero of the program** (`00_MASTER_PROTOCOL.md` §3.2, G1).
**Runs under:** `00_MASTER_PROTOCOL.md`. **Feeds:** `01_CRITIC_RUBRIC.md`.

Nothing downstream is measurable until this exists. A blind A/B against a prior build or an external
reference is only meaningful if both sides were produced by the *same* deterministic recipe. Today
the repo has two capture paths and neither is deterministic, neither is content-addressed, and
neither can strip a side label. This document specifies the harness and, in §9, states exactly what
must be built.

---

## 1. What exists today

Grounded in the repo as it stands. These are the pieces the harness is assembled from.

| Capability | Where | State |
|---|---|---|
| Off-screen AI camera, GPU readback → PNG | `eustress/crates/engine/src/ai_camera.rs` | Works. Fixed **1280×720** (`AI_CAM_WIDTH`/`AI_CAM_HEIGHT`, lines 43–44). Capture is queued to a path by `request_capture` (line 158) and written by the bridge handler. |
| MCP camera control | `ai_camera_set_pose` (position + `look_at` or `rotation`), `ai_camera_orbit` (center/distance/yaw_deg/pitch_deg), `ai_camera_frame` (entity name) — `eustress/crates/mcp-server/src/bridge_tools.rs:1292,1322,1352` | Works. Pose is exact and scriptable. |
| MCP capture | `ai_camera_capture` (`bridge_tools.rs:1381`), `capture_viewport` (`bridge_tools.rs:1266`) | Works, returns the PNG inline. **Both take an empty input schema** — no resolution, no output path, no frame tag. |
| Client PNG burst | `eustress/crates/client/src/systems/frame_capture.rs` | Works. `EUSTRESS_CAPTURE=<count>[@<every_n>]`, `EUSTRESS_CAPTURE_DIR`, 90-frame arm delay, F9 hotkey. Client-only, wall-clock/frame-indexed, **not tick-indexed**. |
| Frame-time series | `eustress/crates/engine/src/profiler.rs`, `frame_diagnostics.rs` | Works. `EUSTRESS_PROFILE=1`, window via `EUSTRESS_PROFILE_FRAMES` (default 120). Emits `eustress_profile.txt` + `eustress_profile.svg` to cwd. |
| Headless batch runner | `eustress/crates/engine/src/bin/headless.rs` | Works for logic. `--space <dir> [--ticks N] [--tick-rate HZ] [--no-autoplay] [--autoplay-delay-frames N]`. **`MinimalPlugins` + `ScheduleRunnerPlugin` only — no renderer.** |
| Sim recording export | `eustress/crates/common/src/simulation/recorder.rs` (`export_json`, `export_csv`), fired on enter-Edit by `eustress/crates/engine/src/simulation/plugin.rs:154` | Works. Lands in `<universe>/.eustress/knowledge/recordings/<space>/sim_<timestamp>.json`. |
| Experiment runner | `run_experiment` / `compare_runs` / `list_experiments`, `eustress/crates/tools/src/simulation_tools.rs:1567,1760,1889` | Works. Results in `<universe>/.eustress/experiments/`. |
| Determinism seed | `eustress/crates/common/src/physics/determinism.rs` | 57 lines. A `GlobalRngSeed` resource and nothing else. |
| Fixed-step pins | `eustress/crates/engine/src/main.rs` — `Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`, `SolverConfig` | Present. |

**Honest summary of the gap:** we can aim a camera precisely and take a picture. We cannot yet
guarantee that taking the same picture twice produces the same bytes, cannot tag a frame with the
tick it came from, cannot choose a resolution, cannot capture at all without a desktop session, and
have no manifest binding a capture to the commit and settings that produced it.

---

## 2. Harness invariants

Every capture the Critic ever sees must satisfy all seven. A bundle violating any of them is
inadmissible and the Critic rejects rather than scores it.

| # | Invariant | Why |
|---|---|---|
| **I1** | **Fixed scene.** The scene is loaded from a pinned Space directory at a pinned commit. No procedural variation, no time-of-day drift, no random placement. | Two sides must differ only in the thing under test. |
| **I2** | **Fixed camera path.** Pose per frame is a declared list of exact positions and orientations, not an interactive fly-through. | Human camera motion is unrepeatable. |
| **I3** | **Fixed seed.** `GlobalRngSeed` set explicitly; every other RNG consumer seeded from it. | Physics and any stochastic system must replay identically. |
| **I4** | **Tick-indexed frames.** Captures are taken at declared **simulation tick indices**, never at wall-clock intervals or frame counters. | Wall-clock indexing means a slower machine captures a different moment. |
| **I5** | **Fixed resolution and colour pipeline.** Declared width, height, colour space, and tonemapping operator, identical on both sides. | Resolution and tonemap differences dominate any real quality difference. |
| **I6** | **Content-addressed output.** Bundle directory name is the SHA-256 of the manifest. Frames carry their own hashes. | Makes a capture citable, tamper-evident, and de-duplicable. |
| **I7** | **Labels stripped and randomised.** No filename, EXIF field, watermark, overlay, HUD, window title, or directory name reveals which side is which. Side assignment is randomised per trial. | The Critic must not be able to infer the side. |

---

## 3. Fixed scene set

Six scenes, each a pinned Space directory under `eustress/spaces/harness/`. Each is authored once,
committed, and thereafter **frozen** — changing a harness scene invalidates every prior bundle that
used it and requires a new scene id.

| ID | Name | Purpose (rubric dimension) | Content requirements |
|----|------|---------------------------|----------------------|
| **S1** | `material_array` | D2 | 8×5 grid of identical spheres: roughness 0.0→1.0 across one axis, metallic 0→1 across the other. Three fixed light sources at declared positions. Neutral 18% grey ground. One calibration chart object with known albedo patches. |
| **S2** | `interior_mixed` | D2, D6 | Enclosed room, 6 m × 8 m × 3 m. One window (directional through an aperture), two artificial sources of differing colour temperature. Objects spanning metal / dielectric / thin-shell / emissive. |
| **S3** | `exterior_single` | D2, D3, D6 | Open terrain, single directional source at declared elevation and azimuth, no sky-dome cheat. Objects casting shadows across 0.2 m to 20 m occluder distances (this is what exercises rubric check M6). |
| **S4** | `studio_ui` | D4 | The studio at a pinned Space with a pinned selection, pinned panel layout, and pinned window sizes. Interaction sequences listed in §4.3. |
| **S5** | `physics_stress` | D3, D5 | Mass ratios spanning 1000:1, a stack settling under gravity, a joint chain, a high-restitution impact, and a resting body held for 600 ticks to expose jitter. Avian, fixed-step 60 Hz, `SubstepCount(6)`. |
| **S6** | `domain_sim` | D5, W4 | The domain simulation under test for the item, with its recorder configured and its independent step counter armed. Bound per item; the item's prompt names which domain. |

**Scene freeze rule.** A harness scene's Space directory hash is recorded in the manifest. If the
hash changes, the scene id must change (`S1` → `S1b`). Never silently edit a frozen scene — that
retroactively falsifies every archived comparison that used it.

---

## 4. Fixed camera paths and frame sets

### 4.1 Path definitions

Paths are declared as JSON, checked in at `eustress/spaces/harness/paths/<id>.json`. Each entry
gives an exact tick index and an exact pose. No interpolation at capture time — poses are
enumerated, and the harness sets each one with `ai_camera_set_pose` before capturing.

| ID | Name | Ticks | Shape |
|----|------|-------|-------|
| **CP-A** | `first_impression` | 0–179 (3.0 s at 60 Hz) | Slow dolly toward the composed hero framing. Frames tagged `role: first_impression` — this is the **only** set the Critic sees for rubric D1. |
| **CP-B** | `orbit_survey` | 0–599 | Full 360° orbit at fixed radius and pitch. Exercises D3 temporal stability and D2 across all incident angles. |
| **CP-S** | `static_studies` | single tick each | Enumerated fixed poses, one per material or lighting study. Used for D2 checks M1–M8. |
| **CP-U** | `ui_sequences` | event-indexed | Not a camera path — full-window captures at declared interaction steps. See §4.3. |

### 4.2 Frame sets

| Set | Scenes | Path | Tick indices captured |
|-----|--------|------|-----------------------|
| `FS-IMPRESSION` | S2, S3 | CP-A | every tick, 0–179 (180 frames) |
| `FS-MOTION` | S3, S5 | CP-B | every tick, 0–599 (600 frames) |
| `FS-MATERIAL` | S1, S2 | CP-S | 24 enumerated poses at tick 120 (post-settle) |
| `FS-SIM` | S5, S6 | CP-S | ticks 0, 60, 300, 600, 1800, 3600 |
| `FS-UI` | S4 | CP-U | see §4.3 |

Total frames per full bundle side: 180 + 180 + 600 + 600 + 24 + 24 + 6 + 6 + 21 (UI) = **1,641**.
At 3840×2160 PNG this is on the order of 8–14 GB per side. **Bundles are therefore not committed to
git.** They live under `docs/PROMPTS/artifacts/bundles/<hash>/` which must be gitignored, with only
the manifest committed.

### 4.3 UI interaction sequences

Four sequences, captured at three window sizes each (1920×1080, 2560×1440, 3840×2160). Each step is
one full-window PNG.

| Sequence | Steps |
|---|---|
| `panel_open` | idle → hover panel tab → click → panel settled (4) |
| `entity_select` | idle → hover entity → click → selection settled with gizmo (4) |
| `property_edit` | select → focus field → type value → commit → **re-read after a reload** (5) |
| `error_surface` | trigger a known-invalid action → error appears → error dismissed (3) |

`property_edit` step 5 is deliberate. `docs/AUDIT/02_STUDIO_ENGINE.md` records that the Properties
panel does not persist edits in the default build; the re-read step is what makes that visible to
the Critic instead of invisible. Rubric D4 caps at 4.0 if the edit visually succeeds and does not
persist.

---

## 5. Seeds, settings, and the determinism contract

Every capture run declares, and the manifest records:

```
seed.global_rng            = 0x5EED_E057_1234_ABCD   (crates/common/src/physics/determinism.rs)
seed.harness               = <per-bundle u64>
physics.fixed_hz           = 60.0                    (engine/src/main.rs Time::<Fixed>::from_hz)
physics.substeps           = 6                       (SubstepCount)
physics.solver             = <SolverConfig dump>
render.width               = 3840
render.height              = 2160
render.colorspace          = sRGB
render.tonemapping         = <operator name>         (filmic today; photoreal.rs post-stack is on hold)
render.msaa                = <sample count>
render.shadow_distance     = <EUSTRESS_SHADOW_DISTANCE>
render.hlod_radius         = <EUSTRESS_HLOD_RADIUS>
render.splat_budget        = <EUSTRESS_SPLAT_BUDGET>
sim.time_scale             = 1.0                     (see §5.1)
sim.max_ticks_per_frame    = <value>                 (see §5.1)
```

### 5.1 Time compression is forbidden inside a capture run

`eustress/crates/common/src/simulation/clock.rs:83 advance()` advances `simulation_time_s` by the
full compressed delta but caps physics ticks at `max_ticks_per_frame` and **zeroes the accumulator
on saturation** (`clock.rs:100-102`). Under compression the clock reports time that was never
stepped, and `effective_compression()` (`clock.rs:131`) reads correct while it happens.

Therefore: **`sim.time_scale` must be `1.0` for every capture run.** Any item that needs compressed
time must run it in a separate, explicitly-labelled experiment, and that experiment's output is not
admissible as a D5 numeric artifact until the independent step counter (§9, B7) exists.

### 5.2 Determinism gate

Before any bundle is admissible, the harness runs the same recipe **twice** and asserts:

1. Every frame PNG is byte-identical across the two runs.
2. The exported recording JSON is byte-identical (modulo a whitelisted timestamp field).
3. The independent physics-step count is identical.

This is the gate written at `docs/architecture/HEADLESS_RUNTIME.md:294`, which has never been
executed. **Executing it is G2's exit condition.** Until it passes, every bundle carries
`determinism_verified: false` in its manifest and the Critic must treat all numeric claims as
provisional and cap rubric D5 accordingly.

---

## 6. Side randomisation and blinding

### 6.1 Procedure

```
  1. Two capture runs produce two frame trees, keyed internally as SUBJECT and CONTROL.
     SUBJECT = the build under test.  CONTROL = the comparison (prior commit, or a licensed
     external reference).
  2. The harness draws a per-trial permutation from a CSPRNG seeded independently of the
     bundle hash.  Assignment: {SUBJECT, CONTROL} -> {ALPHA, BETA}.
  3. Frames are copied into the blinded tree renamed by POSITION ONLY:
        alpha/<set>/<tick>.png      beta/<set>/<tick>.png
     Original filenames, directory names, and side keys are discarded from the blinded tree.
  4. Metadata scrub (mandatory, all of it):
        - strip ALL PNG ancillary chunks except IHDR/IDAT/IEND
        - strip EXIF and XMP
        - normalise file mtime to a constant
        - normalise file ordering (sorted by tick index only)
        - verify no pixel content contains a build string, version overlay, watermark, FPS
          counter, window title bar, or debug HUD
  5. The key — which of ALPHA/BETA is SUBJECT — is written to a SEPARATE file that L1 holds and
     that is NEVER placed in the Critic's input directory.
  6. Repeat 2-5 for each of the 5 preference trials with a fresh permutation.
```

### 6.2 Blinding leak checklist

L1 runs this before every Critic invocation. Each item is a real leak channel:

- [ ] No filename, path component, or archive name contains `eustress`, a commit hash, a version, a product name, `subject`, `control`, `a`, `b`, `old`, `new`, `before`, `after`.
- [ ] No PNG text chunk, EXIF, or XMP survives.
- [ ] No frame contains a UI element carrying a product name, version, or build date.
- [ ] Frame counts are equal on both sides. **An unequal count is itself a label.**
- [ ] Resolutions are identical on both sides.
- [ ] File sizes are not systematically ordered in a way that identifies a side (check: is one side uniformly larger? if so, note it; the Critic must record `blinding_compromised`).
- [ ] No accompanying prose. The Critic receives the rubric, the frames, L1's verified measurement, and the drawn held-out criteria — nothing else.

---

## 7. Output layout and provenance manifest

### 7.1 Directory layout

```
docs/PROMPTS/artifacts/bundles/<manifest-sha256>/
├── manifest.json               # committed to git; everything else gitignored
├── manifest.sig                # optional detached signature
├── blinded/
│   ├── trial_1/
│   │   ├── alpha/
│   │   │   ├── FS-IMPRESSION/0000.png … 0179.png
│   │   │   ├── FS-MOTION/0000.png … 0599.png
│   │   │   ├── FS-MATERIAL/00.png … 23.png
│   │   │   ├── FS-SIM/000000.png … 003600.png
│   │   │   └── FS-UI/<seq>_<size>_<step>.png
│   │   └── beta/   (same shape, equal counts)
│   ├── trial_2/ … trial_5/
├── measurements/
│   ├── profile_alpha.txt       # from EUSTRESS_PROFILE=1
│   ├── profile_beta.txt
│   ├── frametimes_alpha.csv    # per-frame ms, tick-indexed
│   ├── frametimes_beta.csv
│   ├── recording_alpha.json    # simulation recorder export
│   ├── recording_beta.json
│   └── stepcount_{alpha,beta}.json
├── hashes.txt                  # sha256 of every file in the bundle
└── KEY.json                    # side assignment — NEVER shipped to the Critic
```

`KEY.json` lives in the bundle for archival but is removed from the Critic's working copy. Better
still: L1 stores it outside the bundle entirely.

### 7.2 Manifest schema

```json
{
  "schema": "eustress.capture.manifest/1",
  "bundle_id": "sha256:…",
  "created_utc": "2026-08-06T14:02:11Z",
  "operator": "…",
  "determinism_verified": false,
  "sides": {
    "subject": {
      "kind": "eustress",
      "commit": "71ccf6fe…",
      "dirty": false,
      "binary": "eustress-engine",
      "build_profile": "release",
      "cargo_features": ["…"],
      "space_dir": "eustress/spaces/harness/S3_exterior_single",
      "space_hash": "sha256:…"
    },
    "control": {
      "kind": "eustress | external_reference",
      "commit": "30412479…",
      "product": "<name, only when kind=external_reference>",
      "product_version": "<exact build>",
      "scene_source": "<url or path>",
      "scene_license": "<licence name and whether it permits publication>",
      "publication_rights": "internal_calibration_only | publishable",
      "capture_operator": "…",
      "capture_date_utc": "…"
    }
  },
  "hardware": {
    "cpu": "…", "gpu": "…", "gpu_driver": "…", "ram_gb": 64, "os": "Windows 11 Pro 10.0.26200"
  },
  "settings": {
    "seed_global_rng": "0x5EEDE0571234ABCD",
    "seed_harness": "…",
    "physics_fixed_hz": 60.0,
    "physics_substeps": 6,
    "sim_time_scale": 1.0,
    "sim_max_ticks_per_frame": 10,
    "render_width": 3840, "render_height": 2160,
    "render_colorspace": "sRGB", "render_tonemapping": "filmic",
    "render_msaa": 4,
    "env": { "EUSTRESS_PROFILE": "1", "EUSTRESS_PROFILE_FRAMES": "600",
             "EUSTRESS_SHADOW_DISTANCE": "…", "EUSTRESS_HLOD_RADIUS": "…",
             "EUSTRESS_SPLAT_BUDGET": "…" }
  },
  "scenes": ["S1","S2","S3","S4","S5","S6"],
  "paths":  ["CP-A","CP-B","CP-S","CP-U"],
  "frame_sets": { "FS-IMPRESSION": 180, "FS-MOTION": 600, "FS-MATERIAL": 24, "FS-SIM": 6, "FS-UI": 21 },
  "trials": 5,
  "blinding": { "labels_stripped": true, "exif_stripped": true, "counts_equal": true,
                "leak_checklist_passed": true, "checklist_operator": "…" },
  "frame_hashes": "hashes.txt"
}
```

### 7.3 Reference-capture provenance rule

A `control` of `kind: external_reference` is **inadmissible** unless every one of `product`,
`product_version`, `scene_source`, `scene_license`, `publication_rights`, `capture_operator`,
`capture_date_utc`, and the full `hardware` block is populated. The Critic rejects the bundle rather
than scoring it. A bar nobody can reproduce is not a bar
(`00_MASTER_PROTOCOL.md` §4.1 R3).

`publication_rights` defaults to `internal_calibration_only`. Only the human may set it to
`publishable`, and only after confirming the source artifact's licence permits it. Several
commercial engine and platform EULAs carry benchmarking and publication clauses; assume one applies
until counsel says otherwise (`00_MASTER_PROTOCOL.md` §4.1 R2). **No agent may draft, generate, or
stage an external-facing side-by-side.**

The always-legal default comparison is `eustress@<commit-a>` vs `eustress@<commit-b>`. Prefer it.

---

## 8. Invocation

The target — one command, from a clean checkout, on a machine that is not the founder's:

```
cargo run --release --bin eustress-capture -- \
    --recipe docs/PROMPTS/harness/recipes/G3_material.json \
    --subject HEAD \
    --control 30412479 \
    --out docs/PROMPTS/artifacts/bundles \
    --trials 5 \
    --verify-determinism
```

The recipe file names the scenes, paths, frame sets, resolution, and settings. `--verify-determinism`
runs each side twice and fails the bundle if any frame differs.

**`eustress-capture` does not exist.** Building it is the substance of §9.

Until it exists, an interim recipe using only what ships today — sufficient to unblock rubric D2 and
D4 scoring on static frames, and nothing else:

```
1. Launch the engine on the pinned harness Space (windowed; there is no GPU headless tier).
2. For each enumerated pose in CP-S:
     ai_camera_set_pose { position: [...], look_at: [...] }
     ai_camera_capture                      # 1280x720, inline PNG
3. Collect the PNGs, hash them, hand-author manifest.json.
4. Scrub metadata and randomise sides manually per §6.
```

Interim limitations, which every interim bundle must declare in its manifest as
`"interim": true`: 1280×720 only, no tick indexing, no determinism verification, no motion set (so
rubric D3 is unscorable), requires a desktop session, and the manifest is hand-authored and
therefore untrustworthy as provenance. **Interim bundles may calibrate; they may not close an item.**

---

## 9. BUILD LIST — what does not exist and must be implemented

Ordered by dependency. This is G1's work item set.

### Blocking (harness cannot function without these)

| ID | Item | Why | Touches |
|----|------|-----|---------|
| **B1** | **Parameterise the AI camera.** `ai_camera_capture` and `capture_viewport` both take an empty input schema (`bridge_tools.rs:1266,1381`) and `AI_CAM_WIDTH`/`AI_CAM_HEIGHT` are `const` at `ai_camera.rs:43-44`. Add `width`, `height`, and `out_path` parameters, plumbed through the bridge to `request_capture` (`ai_camera.rs:158`). | I5, I6 — 720p is below the resolution at which D2 material checks are decidable | `eustress/crates/engine/src/ai_camera.rs`, `eustress/crates/mcp-server/src/bridge_tools.rs`, the bridge protocol |
| **B2** | **Tick-indexed capture trigger.** A capture must fire *at* simulation tick N, deterministically, not on a wall-clock timer or a frame counter. Add a tick-scheduled capture queue driven by the fixed-step schedule. | I4 — the existing client burst (`frame_capture.rs`) is frame-indexed with a 90-frame arm delay, so two machines capture different moments | `eustress/crates/engine/src/ai_camera.rs`, `eustress/crates/engine/src/simulation/plugin.rs` |
| **B3** | **Camera-path player.** Load a `paths/<id>.json` pose list and set the AI camera pose per tick, driven by the same tick schedule as B2. | I2 | new module under `eustress/crates/engine/src/` |
| **B4** | **`eustress-capture` binary.** The single-command driver of §8: parse recipe, launch or attach, load scene, arm path + tick triggers, run, collect frames + profile + recording, compute hashes, write manifest, optionally run twice and diff. | §8 | new bin in `eustress/crates/engine/src/bin/` |
| **B5** | **Blinding tool.** Copy to the blinded tree, rename by position, strip PNG ancillary chunks / EXIF / XMP, normalise mtimes, run the §6.2 leak checklist, emit `KEY.json` separately, repeat per trial. | I7 | subcommand of B4 |
| **B6** | **Manifest writer + verifier.** Emit §7.2, and a `verify` mode that re-hashes a bundle and rejects it on mismatch or on an incomplete external-reference provenance block. | I6, §7.3 | subcommand of B4 |

### Blocking for numeric claims

| ID | Item | Why | Touches |
|----|------|-----|---------|
| **B7** | **Independent physics-step counter.** A monotonically incremented count of physics steps *actually executed*, exported into the recording and the manifest — independent of `simulation_time_s` and of `effective_compression()`. | Without it, rubric check N4 is unverifiable and D5 caps at 5.0. This is also the instrument that makes the `clock.rs:100-102` accumulator-zeroing visible rather than silent. | `eustress/crates/common/src/simulation/clock.rs`, `recorder.rs` |
| **B8** | **Per-frame frame-time CSV export.** The profiler emits a ranked table and a flamegraph (`profiler.rs`); the harness needs the raw per-frame series, tick-indexed, to compute `ft_p50/p99/max/cv`. | Rubric D3 Part B | `eustress/crates/engine/src/profiler.rs`, `frame_diagnostics.rs` |
| **B9** | **Determinism verification mode.** `--verify-determinism`: run the recipe twice, diff frames byte-for-byte, diff the recording, diff the step count, set `determinism_verified` accordingly. | §5.2, the `HEADLESS_RUNTIME.md:294` gate | B4 |

### Blocking for CI-hosted capture (G7)

| ID | Item | Why | Touches |
|----|------|-----|---------|
| **B10** | **Headless GPU tier (`--render gpu`).** `eustress/crates/engine/src/bin/headless.rs` is `MinimalPlugins` + `ScheduleRunnerPlugin`; its own module docs (line 25) say capture needs the future `--render gpu` tier, and P6 in `docs/architecture/HEADLESS_RUNTIME.md:269` is unstarted. Until this exists, every capture requires a desktop session and the observation half of the agent loop cannot run in CI. | I1–I7 in an automated context; `00_MASTER_PROTOCOL.md` G7 | `eustress/crates/engine/src/bin/headless.rs` |
| **B11** | **CI capture job.** A workflow that runs `eustress-capture` on a GPU runner and uploads the manifest + hashes as build artifacts. Note the current CI baseline: `.github/workflows/ci.yml` has three jobs (cargo-deny, a naga WGSL check that skips any file with naga_oil directives, and a `cargo tree` grep), and `linux-engine.yml` runs one `cargo check`. **No `cargo test` runs anywhere**, against 2,061 `#[test]` functions in the workspace. | `00_MASTER_PROTOCOL.md` D3 | `.github/workflows/` |

### Non-blocking but high value

| ID | Item | Why |
|----|------|-----|
| **B12** | Perceptual diff tool (per-frame ΔE and SSIM between sides) so L1 can pre-screen bundles and detect a null result before spending Critic budget. |
| **B13** | Bundle browser — a static local page that plays a frame set at rate, for human review of what the Critic saw. |
| **B14** | Harness scene authoring script, so S1–S6 are regenerable from source rather than hand-built and frozen by accident. |

**G1 exit condition** (`00_MASTER_PROTOCOL.md` §3.2): B1–B9 complete, and one command produces a
content-addressed bundle with a valid manifest, on a machine that is not the founder's. B10–B11 are
G7's, not G1's — but note that until B10 lands, the founder remains the entire regression surface,
and each verification cycle costs a serialized 10–15 minute build.
