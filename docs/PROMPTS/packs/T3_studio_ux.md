# PACK T3 — Studio UI / UX in Slint

**Phase:** `G6` (Studio UI Craft). Every item ID in this pack is `G6.<nn>`.
**Status:** Authored. Items are `DRAFT` until an L1 promotes them to `READY`.
**Runs under:** `docs/PROMPTS/00_MASTER_PROTOCOL.md`. **Schema:** `docs/PROMPTS/03_PROMPT_SCHEMA.md`.

---

## What this pack owns

The editor experience of the Eustress Studio — the desktop surface built from 60 `.slint` files in
`eustress/crates/engine/ui/slint/` and 27 Rust files in `eustress/crates/engine/src/ui/`. Slint
compiles to Rust; there is no "Slint side" versus "Rust side" in the sense of two languages, only
two files in one build.

Concretely: layout and docking; information density and typographic hierarchy; the Properties panel
as the single polymorphic inspector; tool discoverability and the command surface; selection and
gizmo feel; responsiveness under load and the rule that the UI must never block on the engine;
notification/toast and undo affordances; keyboard-first workflows; theming and visual coherence;
empty and error states; first-run onboarding for a professional user; and accessibility basics.

**Phase exit condition** (`00_MASTER_PROTOCOL.md` §3.2): *Critic ≥ 8.0 on UI craftsmanship; the
drain-skip failure class has a regression test that runs in CI.* This pack delivers the test
(`G6.03`). Wiring it into a workflow is out of scope for every item here. **CI is owned by pack
T5** (`docs/PROMPTS/packs/T5_robustness_and_cohesion.md`), whose `G7.34` adds the `cargo test` gate
to `.github/workflows/ci.yml` and whose `G7.43` runs the standing regression fleet. Cite `G6.03`'s
test to T5; do not wire it yourself.

## Workloads this pack's evidence feeds

| Workload | What this pack produces for it |
|---|---|
| **W1 Provable Quality** | Blind Critic scorecards on D4 (UI craftsmanship) and D6 (coherence) over the FS-UI frame set |
| **W3 Trust & Verifiability** | The drain-contract regression test; a reproducible latency instrument whose CSV a stranger can re-derive |
| **W4 Vertical Proof** | The unwired-button sweep — a buyer clicking a ribbon button never gets a silent no-op |
| **W6 Operator Leverage** | Click-depth reduction on the top 20 commands; the `slint_ui.rs` split that makes UI work cheap again |

W2 (Revenue Rail) and W5 (Extension Surface) are **not** served by this pack. Do not author items
against them here.

## ITEM ZERO

**`G6.01` — Studio UX baseline census.** It is tier `S`, consumes zero builds, and produces the
single JSON every other item in this pack cites for its "before" number. Nothing else in this pack
may start until `G6.01` is `PASSED`. You cannot improve what you have not measured, and eleven of
the fourteen downstream items state their exit criterion as a delta against a field in that file.

## Dependency graph

| ID | Title | Tier | `depends_on` |
|---|---|---|---|
| **G6.01** | Studio UX baseline census **(ITEM ZERO)** | S | — |
| G6.02 | UI interaction-latency instrument (`EUSTRESS_UI_TRACE`) | M | G6.01 |
| G6.03 | Drain-contract regression test; delete the dead parallel drain | M | G6.01, G1.01 |
| G6.04 | Drain path never blocks on the engine | L | G6.02, G6.03, G1.12 |
| G6.05 | Properties panel persists every edit it accepts | L | G6.01, G6.03, G6.04, G1.12 |
| G6.06 | Pre-click honesty for the 1,969 unwired ribbon tools | M | G6.01, G1.12 |
| G6.07 | Command palette — click depth ≤ 2 for the top 20 commands | L | G6.01, G6.02, G6.04, G1.12 |
| G6.08 | Keyboard-first coverage and conflict surfacing | M | G6.04, G6.07, G1.12 |
| G6.09 | Selection identity and theme-token coherence | M | G6.01, G1.12 |
| G6.10 | Empty states and error states in the eight primary panels | M | G6.01, G6.04, G6.09, G1.12 |
| G6.11 | Accessibility annotations and focus order | L | G6.01, G6.04, G6.09, G1.12 |
| G6.12 | Information density and typographic hierarchy | L | G6.09, G6.10, G1.12 |
| G6.13 | Split `slint_ui.rs` — no UI file over 3,000 lines | XL | G6.03, G6.04 |
| G6.14 | Time-to-first-successful-task for a first-time professional | L | G6.05, G6.06, G6.07, G6.08, G6.10, G1.12 |
| G6.15 | Blind D4/D6 scorecard — the pack's proof artifact | L | G6.04, G6.06, G6.09, G6.10, G6.11, G6.12, G6.13, G6.14, G1.12 |

Acyclic. The two items that target measurable interaction latency rather than appearance are
**G6.02** (build the instrument, prove it is accurate and cheap) and **G6.04** (move the number).
**G6.13** additionally guards the number against regression during the refactor.

**Why five items depend on `G6.04` for a reason unrelated to latency.** `G6.05`, `G6.07`, `G6.08`,
`G6.10`, and `G6.11` all declare `capture_recipe:
docs/PROMPTS/harness/recipes/G6_ui_sequences.json`, and `G6.04` is the only item in this pack whose
scope permits authoring that file. An item cannot be captured against a recipe that does not yet
exist, so the recipe's author is in every consumer's `depends_on`. `G6.04`'s own closure is
`{G6.01, G6.02, G6.03}` and contains none of the five, so the edges introduce no cycle.

---

## Cross-pack file ownership

Items in this pack share source files with items in other packs. `docs/PROMPTS/04_FILE_OWNERSHIP.md` names one owner per contested path and is normative; where it conflicts with an item's scope list, it wins. This pack's own dependency-graph table records its internal edges together with the program-gate edges of `02_QUEUE.md` §8.2 and §8.4; the cross-pack edges arising from contested paths are these:

| Item | Now depends on | Contested path | Effect on this item's scope |
|---|---|---|---|
| `G6.02` | `G1.04` (G1) | `eustress/crates/engine/src/lib.rs` | may append to but not alter it |
| `G6.02` | `G7.31` (T5) | `eustress/crates/engine/src/main.rs` | may no longer edit it |

An item blocked by one of these entries emits a `FILE-OWNERSHIP` decision packet to L0 (`docs/PROMPTS/04_FILE_OWNERSHIP.md` §6). It does not edit the file and does not work around it.

---


## How to use this file

Each section below is a complete prompt file. An L1 splits each one out verbatim to
`docs/PROMPTS/items/<ID>_<slug>.md`, sets `status: READY`, and dispatches it to an L2. The L2
receives **only** that one file. Nothing in this header is inherited by the executing agent — every
prompt restates what it needs.

---
---

# `docs/PROMPTS/items/G6.01_ux-baseline-census.md`

```markdown
---
id: G6.01
title: Studio UX baseline census — the measured before-state of the editor surface
workload: W6
workload_secondary: [W1, W3]
phase: G6
depends_on: []
blocks: [G6.02, G6.03, G6.05, G6.06, G6.07, G6.09, G6.10, G6.11]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G6.01/ux_baseline_census.json
escalation: >
  If any of the eleven required census fields cannot be derived from a single literal command that
  a third party can rerun — i.e. the count depends on a judgement call — STALL rather than
  publishing an estimate. A census with one soft number poisons every downstream exit criterion
  that subtracts from it.
status: DRAFT
notes: >
  Tier S with zero builds: this item compiles nothing. Every number is derived by reading files
  that are already in the repository. It is deliberately the cheapest item in the pack and
  deliberately the one everything else depends on.
---

## 1. Objective

A single machine-readable file records the measured before-state of the Eustress Studio editor
surface: how large it is, how many of its buttons do anything, how deep its commands are buried,
how much of it is annotated for assistive technology, and where its drain contract is fragile.
Every subsequent item in phase G6 states its exit criterion as a delta against a named field in
this file, so the file is the pack's shared coordinate system.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. It is never described as a game engine. The 3D viewport and the ECS are
implementation details in service of that. The licence is PolyForm Shield 1.0.0; the project is
**source-available**, never "open source". Physics is **Avian**, never Rapier. Units are
meter-native; studs are a display unit only.

**Slint is Rust.** The `.slint` files under `eustress/crates/engine/ui/slint/` compile to Rust at
build time. There is no "Rust-first versus Slint" framing. When this item counts lines in both
`.slint` and `.rs` files it is counting one program.

**The surface being counted.** Verified at authoring time:

- `eustress/crates/engine/ui/slint/` contains **60** `.slint` files. Largest:
  `main.slint` (4,021 lines), `ribbon.slint` (3,389), `properties.slint` (2,859),
  `workshop_panel.slint` (2,109), `simulation_settings.slint` (1,131), `theme.slint` (1,010),
  `settings.slint` (990), `script_editor.slint` (932), `dock_layout.slint` (864).
- `eustress/crates/engine/src/ui/` contains 27 `.rs` files totalling 37,533 lines. The largest is
  `eustress/crates/engine/src/ui/slint_ui.rs` at **23,103 lines** — the single largest file in the
  workspace and the biggest structural liability in the editor.
- `eustress/crates/engine/src/ui/slint_main.rs` is **598 lines** and is **dead code**: no
  `mod slint_main;` declaration exists anywhere under `eustress/crates/engine/src/`. It still
  contains 136 references to `SlintAction`, i.e. a second, parallel, never-executed copy of the
  action-drain logic.

**The drain pattern.** Slint callbacks push variants of `SlintAction`
(`eustress/crates/engine/src/ui/slint_ui.rs:250`) onto a queue. One Bevy system,
`drain_slint_actions` (`slint_ui.rs:4965`), registered into the `SlintSystems::Drain` set at
`slint_ui.rs:1428`, converts them into Bevy state and events. It takes three parameter bundles —
`DrainEventWriters` (`slint_ui.rs:3449`, 45 fields), `DrainResources` (`slint_ui.rs:3549`, 48
fields), `DrainActionQueries` (`slint_ui.rs:3941`, 24 fields) — plus five direct parameters.

**Why the drain matters to a census.** Bevy skips a system whose parameters fail validation. A
required (non-`Option`) resource parameter whose resource was never registered makes Bevy skip
`drain_slint_actions` **every frame**, which silently kills **every** UI click in the studio while
logging one `failed validation` WARN. Two comments in the source record this having happened
already — `slint_ui.rs:1373-1378` (the `LabelEditState` case) and `slint_ui.rs:1379-1383` (the
`TerrainVisibility` case, described there as "the theme/mode clicks do nothing bug"). Counting the
required parameters counts the live landmines.

**Ribbon tools.** `eustress/crates/engine/src/tool_metadata.rs` is generated by
`scripts/gen_tool_metadata.py` and is 2,151 lines. Its `tool_meta` function (`:147`) is a match with
**1,999** arms, one per ribbon tool id declared across the ten mode manifests in
`eustress/crates/engine/modes/`. Each arm ends in a `wired: bool`. Measured at authoring time:
**30** arms are `wired: true`, **1,969** are `wired: false`.

**Accessibility.** Exactly **1** of the 60 `.slint` files contains the string `accessible-role`
(`eustress/crates/engine/ui/slint/script_editor.slint`, one occurrence). This is a 1/60 baseline,
not a 0% one, and the census must say so precisely.

**Theme.** `eustress/crates/engine/ui/slint/theme.slint` defines a `ThemeData` struct (`:23`) and
two palette constants: `preset-classic` (`:135`) and `preset-modern` (`:217`), selected into
`Theme.data` (`:304`). Selection-related tokens differ between the two presets.

**Tests and CI.** `eustress/crates/engine/src/ui/slint_ui.rs` contains **zero** `#[test]`
functions. The `eustress-engine` crate contains 797. `.github/workflows/` holds `ci.yml`,
`linux-engine.yml`, and `release.yml`, and a search of that directory for `cargo test` or
`cargo clippy` returns **no matches** — no test runs in CI anywhere in this repository.

**Build reality (does not apply to this item, stated so you do not violate it by reflex).** A full
engine build takes 10–15 minutes; only one cargo build may run at a time because the workspace
shares one `target/`; never kill a build mid-compile. Validate with `cargo run`, never
`cargo check`. **This item compiles nothing.** `max_builds` is 0. If you find yourself starting a
build, you have misread the item.

**The environment.** Windows 11, PowerShell 7 (`pwsh`) available. All commands below are given in
`pwsh` form and were executed successfully at authoring time from the repository root
`E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may create
- `docs/PROMPTS/artifacts/G6.01/ux_baseline_census.json` (create; the directory may not exist yet)
- `docs/PROMPTS/artifacts/G6.01/census_commands.txt` (create; the literal command list, one per line, in field order)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- **Every file under `eustress/`** — this item is read-only against the codebase. It counts; it does not change.
- Any other file under `docs/PROMPTS/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Here that means: do not narrow a glob, do not exclude a file because it is "not really a panel",
  do not round, and do not substitute a number quoted from a document for a number you derived from
  a command. If a documented number and your command disagree, the command wins and you record both
  with a `discrepancy` note.
- Every field must be reproducible. For each numeric field, the exact command that produced it goes
  in `census_commands.txt` at the same ordinal position. A third party running that file top to
  bottom must reproduce every number.
- Do not estimate click depth by intuition. Click depth is defined in §5 and is counted by reading
  the `.slint` source for the containing surface, recording the file and line that establishes each
  step. A depth without a `path:line` chain is not a measurement.
- Do not compile anything. Do not run the engine.

## 5. Exit criterion

### Criterion
`docs/PROMPTS/artifacts/G6.01/ux_baseline_census.json` exists, parses as JSON, contains **all
eleven** required top-level fields listed below, and its four independently checkable count fields
match the four verification commands exactly — `slint_file_count == 60`,
`ribbon_tools_total == 1999`, `ribbon_tools_wired == 30`, `slint_files_with_accessible_role == 1`.

### Measurement

Command:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.01_check.ps1

The checker re-derives the four cheapest counts from the source tree and compares them against the
census file: `slint_file_count` against a glob of `eustress/crates/engine/ui/slint/*.slint`,
`ribbon_tools_total` and `ribbon_tools_wired` against the `wired` literals in
`eustress/crates/engine/src/tool_metadata.rs`, and `slint_files_with_accessible_role` against a
distinct-file scan for `accessible-role`. It also asserts all eleven required fields are present.

Expected output shape:

    missing=
    slint_file_count 60 vs 60
    ribbon_tools_total 1999 vs 1999
    ribbon_tools_wired 30 vs 30
    accessible_role_files 1 vs 1
    CENSUS_OK

Pass condition:

    The final line is exactly CENSUS_OK and the process exit code is 0.

Read the exit code, not the presence of the file. A census file that exists but disagrees with the
repository is worse than no census.

### Required field definitions

| Field | Type | Definition |
|---|---|---|
| `slint_file_count` | int | Count of `*.slint` under `eustress/crates/engine/ui/slint/` |
| `slint_total_lines` | int | Sum of physical lines across those files |
| `ui_rs_total_lines` | int | Sum of physical lines across `*.rs` in `eustress/crates/engine/src/ui/` |
| `largest_ui_file` | object | `{path, lines}` for the largest file in `eustress/crates/engine/src/ui/` |
| `drain_required_params` | array | One entry per **non-`Option`** parameter reaching `drain_slint_actions`, across `DrainEventWriters`, `DrainResources`, `DrainActionQueries`, and the function's five direct parameters. Each entry `{name, type, source_path, source_line, registered_at}` where `registered_at` is the `path:line` of the `init_resource` / `insert_resource` / `add_message` call that registers it, or `null` if you cannot find one. A `null` here is the single most valuable output of this item. |
| `ribbon_tools_total` | int | Match arms in `tool_meta` |
| `ribbon_tools_wired` | int | Arms with `wired: true` |
| `slint_files_with_accessible_role` | int | Distinct `.slint` files containing `accessible-role` |
| `top20_command_click_depth` | array | Exactly 20 entries `{command, keybinding_action_or_null, clicks_from_cold_start, path_chain}` — see below |
| `theme_selection_tokens` | object | For each of `selection-background`, `selection-border`, `accent-cyan`, `accent-eustress`, `border-focus`: the literal value in `preset-classic` and in `preset-modern`, with `theme.slint` line numbers |
| `panel_inventory` | array | One entry per `.slint` file `{file, lines, is_panel}` where `is_panel` is true if the file exports a component docked or floated by `main.slint` |

**The top-20 command list is fixed. Use exactly these, in this order**, so that every downstream
item measures the same set: Undo, Redo, Save, Copy, Paste, Duplicate, Delete, Group, Ungroup,
Select All, Select Tool, Move Tool, Rotate Tool, Scale Tool, Insert Part, Insert Script, Play Solo,
Stop, Toggle Explorer, Toggle Properties.

**Click depth definition.** From a cold start — studio open, default layout, nothing selected, no
panel expanded, mouse only, keyboard forbidden — the number of discrete mouse-down events required
to invoke the command. A command on the visible ribbon Home tab is depth 1. A command requiring one
tab switch then one click is depth 2. A command reachable only through a menu that must first be
opened is depth 2 or more. A command with **no** mouse route at all is recorded as `-1`, not as a
large number. `path_chain` lists the `path:line` for each step.

## 6. Critic gate

`critic_gate` is `[]`. This item produces no visual artifact, so there is nothing for a blind
Critic to score. The mechanical criterion in §5 replaces it and is deliberately strict: four
independently derived counts must match exactly, and eleven fields must be present. The item's real
risk is a plausible-looking number nobody checked, which is why the verification command re-derives
the four cheapest counts from the source rather than trusting the file.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — derive every field directly from source files with pwsh
   -> if still failing, MANDATORY approach change. Re-running the same query with a
      different glob is NOT an approach change; switching from text matching to parsing
      the Rust token stream is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations in which the number of missing or mismatched
                  census fields drops by less than 1
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: any required census field cannot be derived from a single rerunnable
                   command (see front matter)
```

A STALL packet is one screen and must request exactly one of: LOWER the field set to a stated
subset with a statement of which downstream items lose their baseline; FUND approach D with an
estimate; DEFER behind a named item; or KILL with a statement of what the phase loses. A packet
that asks the human to "review the situation" is malformed.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.01/ux_baseline_census.json`

A reader finds the eleven fields defined in §5, each with its source `path:line` where the
definition calls for one. Alongside it,
`docs/PROMPTS/artifacts/G6.01/census_commands.txt` lists the literal command that produced each
numeric field, in field order, so the whole census is rerunnable by a stranger with no context.

This file is the W6 evidence for the item — it is what makes the rest of phase G6 measurable rather
than assertive — and eleven downstream items cite it by field name.

## 9. Definition of NOT done

- The JSON exists and parses but `drain_required_params` is a count instead of an array with a
  `registered_at` per entry. The array is the point; the count is the trivia.
- Click depths are filled in from the ribbon's *intent* ("Undo is obviously depth 1") rather than
  from reading `ribbon.slint` and `main.slint` and recording the line that puts the control on
  screen. An unsourced depth is a guess wearing a number's clothes.
- A command with no mouse route is recorded as `99` or `null` instead of `-1`, hiding a genuine
  discoverability hole behind a sentinel that sorts like a large depth.
- The census quotes `1,400 tools` from a design document instead of the 1,999 arms the generated
  table actually contains. Documented numbers are not measurements.
- `panel_inventory` marks every `.slint` file as a panel, including `theme.slint`,
  `numeric_field.slint`, and the dialogs. `is_panel` must mean docked or floated by `main.slint`,
  and must cite the line in `main.slint` that does it.
- The agent edits a source file "to make counting easier". This item is read-only against
  `eustress/`; any diff there fails it outright.
- The four verification counts match because the agent read them out of this prompt rather than
  deriving them. If the repository has moved since authoring, the derived number is correct and the
  number in this prompt is stale — record the discrepancy, do not reproduce the prompt.
```

---
---

# `docs/PROMPTS/items/G6.02_ui-latency-instrument.md`

```markdown
---
id: G6.02
title: UI interaction-latency instrument with per-frame input-to-present trace
workload: W3
workload_secondary: [W1, W6]
phase: G6
depends_on: [G6.01, G1.04, G7.31]
blocks: [G6.04, G6.07, G6.13, G6.14]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G6.02/ui_trace_baseline.json
escalation: >
  If the instrument's own measured overhead exceeds 0.15 ms per frame at the p99, STALL rather than
  shipping it. An instrument that costs more than the effects it measures makes every downstream
  latency number in phase G6 unfalsifiable.
status: DRAFT
notes: >
  Tier M with 6 builds. The instrument is one new module plus registration; the cost is that
  validating it requires running the real windowed studio, and each cycle is a 10-15 minute build.
  Budget for two measurement runs, not six.
---

## 1. Objective

The Eustress Studio emits a per-frame, tick-indexed CSV that decomposes the path from a user input
event to the frame that reflects it: when the input arrived, when the resulting `SlintAction` was
enqueued, how long `drain_slint_actions` ran, how long the Slint sync systems ran, how much of that
was synchronous filesystem I/O, and when the frame presented. From that CSV, p50/p99/max
input-to-present latency is computable by a third party with no access to the running engine.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust, so Slint *is* Rust here. Units are meter-native; studs are a display unit only.

**What exists today for timing, and what it cannot do.**

- `eustress/crates/engine/src/profiler.rs` plus `eustress/crates/engine/src/frame_diagnostics.rs`
  are always compiled. Arm with `EUSTRESS_PROFILE=1`; window size via `EUSTRESS_PROFILE_FRAMES`
  (default 120 — CONFIG DEFAULT). Output is `eustress_profile.txt` (a ranked table) and
  `eustress_profile.svg` (an inferno flamegraph) written to the current working directory.
  `frame_diagnostics.rs` defines `FrameTimeTracker` (`:7`), `track_frame_time` (`:43`), and
  `FrameDiagnosticsPlugin` (`:142`). **This is an aggregate ranked table, not a per-frame series**,
  and it is frame-indexed, not tick-indexed. It cannot answer "what was the p99 latency from click
  to pixel".
- `eustress/crates/engine/src/ui/slint_ui.rs:1177` defines a `UIPerformance` resource with
  `frame_times`, `fps`, `avg_frame_time_ms`, `ui_budget_ms`, `last_ui_time_ms`,
  `skip_heavy_updates`, `frame_counter`. It is a rolling average used for adaptive throttling. It
  is not exported and not tick-indexed.
- There is **no** input-to-present measurement in this repository today. This capability is 0%.

**The path you are instrumenting.** Registered in `eustress/crates/engine/src/ui/slint_ui.rs`:

- `forward_input_to_slint` (`:3020`), added `.before(SlintSystems::Drain)` at `:1425`, reads
  `MouseButtonInput` and `MouseWheel` messages and forwards pointer events into Slint.
- `forward_keyboard_to_slint`, added `.before(SlintSystems::Drain)` at `:1426`.
- Slint callbacks push `SlintAction` variants (`enum SlintAction` at `:250`) onto
  `SlintActionQueue`.
- `drain_slint_actions` (`:4965`), registered `.in_set(SlintSystems::Drain)` at `:1428`, consumes
  the queue.
- Roughly twenty `sync_*_to_slint` systems run `.after(SlintSystems::Drain)` (`:1462` through
  `:1487`), pushing Bevy state back into Slint properties.

**Why the last stage matters.** Several drain arms perform synchronous filesystem work on this
path. A search of `eustress/crates/engine/src/ui/slint_ui.rs` for
`std::fs::(read_to_string|write|read_dir|create_dir_all|copy|remove)` returns **64** matches
(MEASURED, `Select-String`, at authoring time), among them the `SlintAction::ImportAsset` arm which
opens a **synchronous modal OS file picker** (`rfd::FileDialog::new()` … `.pick_file()`) inside the
drain. Your instrument must attribute that time separately or the drain-cost column will be
uninterpretable. Fixing it is **not** this item — it is `G6.04`. This item only has to make it
visible.

**Baseline you are extending.** `docs/PROMPTS/artifacts/G6.01/ux_baseline_census.json` (produced by
item G6.01, already `PASSED`) records the surface inventory. This item adds the first *temporal*
baseline. Read the census; do not re-derive it.

**Build reality.** A full engine build takes 10-15 minutes. Only one cargo build at a time — the
workspace shares a single `target/` and concurrent builds produce link failures (LNK2001, or SAC
os error 4551). Never kill a build mid-compile; if you must recover from a poisoned build, use
`cargo clean -p eustress-engine`. Validate with `cargo run`, **never** `cargo check` —
`cargo check` will not catch the plugin-registration failure this item is most likely to produce.

**The drain-skip trap, which this item can trigger.** Bevy skips a system whose parameters fail
validation. If you add a required (non-`Option`) resource parameter to a system on the drain path
and forget to register the resource, Bevy silently skips that system every frame — and if the
system you touched is `drain_slint_actions`, **every** UI click in the studio dies while Bevy logs
a single `failed validation` WARN. Two source comments record this happening:
`slint_ui.rs:1373-1378` and `slint_ui.rs:1379-1383`. Prefer `Option<Res<...>>` for anything you add,
or register it in the same edit alongside the other `init_resource` calls that begin at
`slint_ui.rs:1360`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/ui_trace.rs` (create)
- `eustress/crates/engine/src/lib.rs` (add the `pub mod ui_trace;` declaration only)
- `eustress/crates/engine/src/ui/slint_ui.rs` (timestamp hooks only — no behaviour change to any action arm)
- `docs/PROMPTS/artifacts/G6.02/` (create; artifacts)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` — the census is frozen; if you believe it is wrong, report it, do not edit it
- Any `.slint` file — this item measures the surface, it does not change it
- `eustress/crates/engine/src/profiler.rs` and `eustress/crates/engine/src/frame_diagnostics.rs` — the existing profiler stays as it is; do not repurpose it
- Anything under `eustress/crates/common/src/physics/`
- `eustress/crates/engine/src/main.rs` — owned by `G7.31` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- Any existing entry in `eustress/crates/engine/src/lib.rs` — the file is owned by `G1.04`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  For this item specifically: do not measure on an empty scene to make the numbers look calm, do not
  drop frames beyond the declared warm-up, do not compute p99 over a window shorter than the
  declared one, and do not silently exclude frames where a modal dialog was open. If the
  measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The trace must be **off by default**. It arms only when `EUSTRESS_UI_TRACE=1` is set. An
  always-on instrument that costs frame time is a regression, not a tool.
- Timestamps must come from one monotonic clock source read on the render thread. Do not mix
  `Instant::now()` on one stage with a Bevy `Time` delta on another; the resulting column is not a
  latency.
- One row per frame. Do not aggregate in the engine — emit raw and let the analysis step compute
  percentiles. Aggregation in-engine is how a p99 becomes unauditable.
- Batch your verification. Six builds is the entire budget; design so that one build validates the
  module, the registration, and the CSV schema together.
- Do not add a required (non-`Option`) system parameter anywhere on the drain path without
  registering its resource in the same edit. See §2.

## 5. Exit criterion

### Criterion
With `EUSTRESS_UI_TRACE=1` armed on a 10,000-part scene, a single studio session produces a CSV of
**at least 600 rows** with a strictly increasing `frame` column and all eleven required columns
present; the derived analysis JSON reports p50, p99 and max `input_to_present_ms`; and the
instrument's own measured cost, `trace_overhead_ms` at p99, is **≤ 0.15 ms**.

### Measurement

Step 1 — generate the loaded scene (run once; it writes part files and does not need the studio):

    cargo run --release -p eustress-engine --bin generate-benchmark-map -- --grid-size 100 --spacing 4.0 --seed 42

Step 2 — run the studio with the trace armed, exercise the four declared interaction sequences
(`panel_open`, `entity_select`, `property_edit`, `error_surface` — the same four named in
`docs/PROMPTS/02_CAPTURE_HARNESS.md` §4.3), then close the studio:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.02_check.ps1

The checker arms `EUSTRESS_UI_TRACE=1`, points `EUSTRESS_UI_TRACE_OUT` at
`docs/PROMPTS/artifacts/G6.02/ui_trace.csv`, launches the studio, and exits with the studio's own
exit code. It does not gate the trace — step 3 does.

Step 3 — derive and check:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.02/analyze_trace.ps1 -Csv docs/PROMPTS/artifacts/G6.02/ui_trace.csv -Out docs/PROMPTS/artifacts/G6.02/ui_trace_baseline.json

`analyze_trace.ps1` is written by this item and must: assert the eleven required columns are
present; assert `frame` is strictly increasing; **exclude** rows whose `input_event_ms` cell is
empty from the latency percentiles (an empty string casting to `0.0` would silently deflate p50);
compute p50/p99/max of `input_to_present_ms` and p99 of `trace_overhead_ms`; write the JSON; print
`TRACE_OK` and `exit 0` when `rows >= 600 AND monotonic AND no missing columns AND
trace_overhead_p99_ms <= 0.15`, otherwise print `TRACE_FAIL` and `exit 1`.

Expected output shape:

    {"rows":842,"monotonic":true,"missing_columns":[],"input_to_present_p50_ms":18.4,"input_to_present_p99_ms":74.9,"input_to_present_max_ms":1310.2,"trace_overhead_p99_ms":0.06}
    TRACE_OK

Pass condition:

    rows >= 600  AND  monotonic == true  AND  missing_columns is empty  AND  trace_overhead_p99_ms <= 0.15
    AND the step-3 process exits 0.

The p50/p99/max latency values themselves are **not** gated by this item — this item builds and
validates the instrument. `G6.04` is the item that must move them. Record whatever they are; a bad
baseline is a correct output here. Verify by reading the emitted `TRACE_OK` marker and the exit
code, not by observing that the CSV file appeared.

### Required CSV columns

`frame`, `tick`, `input_event_ms`, `action_enqueued_ms`, `drain_start_ms`, `drain_end_ms`,
`drain_io_blocking_ms`, `sync_end_ms`, `present_ms`, `input_to_present_ms`, `trace_overhead_ms`.

All `*_ms` values are milliseconds from one monotonic origin captured at trace-arm time.
`input_to_present_ms` is `present_ms - input_event_ms` for the frame that first reflects that
input; rows with no input that frame carry empty `input_event_ms` and `input_to_present_ms`.

## 6. Critic gate

`critic_gate` is `[]`. This item produces a CSV and a JSON, not an image; there is nothing for a
blind Critic to score. The mechanical criterion in §5 replaces it, and it is deliberately harsh in
one direction: the instrument's own overhead is gated at 0.15 ms p99, because the whole downstream
value of phase G6's latency work depends on the instrument not being part of the problem.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — timestamps captured inside the existing Bevy systems, written
                 to a buffered writer flushed on app exit
   -> if still failing, MANDATORY approach change. Moving a timestamp call ten lines is NOT
      an approach change; moving from in-system instrumentation to a schedule-level
      system-execution hook is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with trace_overhead_p99_ms moving < 5% and the
                  missing-column count unchanged
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: trace_overhead_p99_ms cannot be brought to <= 0.15 ms (see front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the overhead ceiling to a
stated value with a statement of which downstream measurements become unreliable; FUND approach D
with an estimate and why it is materially different; DEFER behind a named item; or KILL with a
statement of what phase G6 loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.02/ui_trace_baseline.json`

A reader finds: the row count, whether the frame column was monotonic, the missing-column list
(empty on pass), p50/p99/max input-to-present latency in milliseconds, and the instrument's own p99
overhead. Archived alongside it: `docs/PROMPTS/artifacts/G6.02/ui_trace.csv` (the raw per-frame
series), `analyze_trace.ps1` (the derivation), and
`docs/PROMPTS/artifacts/G6.02/run_conditions.json` recording the scene (grid size, spacing, seed),
the commit, the GPU and driver version, the display resolution, and the OS build.

This is the W3 evidence for the item: a stranger with the CSV can re-derive every number in the
JSON without access to the machine that produced it.

## 9. Definition of NOT done

- The CSV exists with all columns but `input_to_present_ms` is computed as
  `present_ms - drain_start_ms`. That measures the back half of the pipeline and calls it latency —
  the user's click happened earlier, and the gap is exactly where the interesting stalls live.
- Overhead is reported as an average rather than a p99. Instrument cost is bursty by nature; the
  average hides the frame where the writer flushed.
- The trace is armed by a cargo feature instead of an environment variable, so measuring requires a
  10-15 minute rebuild and nobody ever reruns it. It must arm at run time.
- Empty `input_event_ms` cells cast to `0.0` and enter the percentile computation, deflating p50
  toward zero and making every later improvement look small.
- The instrument writes one file per frame, or flushes per row, and its own I/O becomes the
  dominant cost — passing the row-count check while failing the purpose.
- A required (non-`Option`) resource is added to a drain-path system without registration, Bevy
  starts skipping `drain_slint_actions`, and the studio's buttons all die. The trace will still
  produce rows. The build is broken anyway.
- The measurement run is done on an empty scene rather than the 10,000-part benchmark grid, so the
  baseline is calm and `G6.04` later "improves" a number that was never under load.
- `drain_io_blocking_ms` is always 0 because the instrument wraps only `std::fs` calls it could
  find by name and misses the synchronous `rfd::FileDialog` modal, which is the single largest
  stall on the path.
```

---
---

# `docs/PROMPTS/items/G6.03_drain-contract-regression-test.md`

```markdown
---
id: G6.03
title: Drain-contract regression test and removal of the dead parallel drain
workload: W3
workload_secondary: [W6]
phase: G6
depends_on: [G6.01, G1.01]
blocks: [G6.04, G6.05, G6.13]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G6.03/drain_contract_report.json
escalation: >
  If asserting that drain_slint_actions actually ran requires constructing a full windowed Slint
  context inside a unit test — i.e. the test cannot run without a desktop session — STALL rather
  than writing a test that only passes on the founder's machine. A regression test that cannot run
  headless does not close this failure class.
status: DRAFT
notes: >
  Tier M. The test itself is small; the cost is in proving it actually fails when the defect is
  reintroduced, which requires additional builds with deliberately broken registrations.
---

## 1. Objective

The failure class in which a single missing resource registration makes Bevy skip
`drain_slint_actions` — silently killing every button in the Eustress Studio while logging one
warning — is closed by an automated test. The test passes on the current tree and fails when any
one of the drain's required parameters is deregistered. The dead second copy of the drain logic
that makes this class recur is deleted.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` compiles to
Rust. Units are meter-native; studs are a display unit only.

**The defect class, precisely.** Slint callbacks push `SlintAction` variants
(`eustress/crates/engine/src/ui/slint_ui.rs:250`) onto `SlintActionQueue`. Exactly one Bevy system,
`drain_slint_actions` (`eustress/crates/engine/src/ui/slint_ui.rs:4965`), registered
`.in_set(SlintSystems::Drain)` at `slint_ui.rs:1428`, converts those into Bevy state and events.

Bevy **skips** a system whose parameters fail validation. `drain_slint_actions` takes:

- `DrainEventWriters` (`slint_ui.rs:3449`) — 45 fields, none `Option`. Each is a message writer
  whose message type must have been registered with `add_message`.
- `DrainResources` (`slint_ui.rs:3549`) — 48 fields, of which **two are required (non-`Option`)**:
  `terrain_visibility: ResMut<TerrainVisibility>` and `materials: ResMut<Assets<StandardMaterial>>`.
- `DrainActionQueries` (`slint_ui.rs:3941`) — 24 fields.
- Five direct parameters, three of which are required resources: `asset_server: Res<AssetServer>`,
  `label_edit_state: ResMut<LabelEditState>`, `pending_insert: ResMut<PendingInsertSelection>`.

If any one of those required registrations is missing, Bevy skips the whole system every frame.
The user sees a studio where **nothing clickable works** — Explorer, ribbon, tags, theme switch,
tool selection, play — with no error dialog. Bevy logs one `failed validation` WARN.

This is not hypothetical. Two comments in the source record it having shipped twice:

- `slint_ui.rs:1373-1378` — `drain_slint_actions` takes `ResMut<LabelEditState>` for the
  EditLabelDialog handler; missing means Bevy skips the entire drain every frame and all Slint UI
  callbacks (Explorer, tag removal, add-tag, ribbon) silently die.
- `slint_ui.rs:1379-1383` — `DrainResources.terrain_visibility` is a required (non-`Option`)
  `ResMut` that was registered only in a legacy plugin, which skipped the whole drain; the source
  calls this "the theme/mode clicks do nothing bug", and notes Bevy logs one `failed validation`
  WARN.

Both comments name the same root cause: **a second, parallel registration site**. One plugin
registers the resources; a different plugin runs the system.

**The residue of that root cause is still in the tree.**
`eustress/crates/engine/src/ui/slint_main.rs` is 598 lines containing 136 references to
`SlintAction` — a second copy of the drain-and-registration logic. It is **dead**: a search of
`eustress/crates/engine/src/` for `mod slint_main` returns no matches, and
`eustress/crates/engine/src/ui/mod.rs` declares its submodules at lines 22-45, none of which is
`slint_main`. It compiles nothing, ships nothing, and exists only to be mistaken for the live path
by the next person who greps for a `SlintAction` arm. Deleting it is part of this item.

**Also relevant:** `eustress/crates/engine/src/ui/mod.rs:253` defines `StudioState`, and there is
now exactly **one** definition of it in the workspace. That single-definition invariant is recorded
in `docs/AUDIT/02_STUDIO_ENGINE.md` under the Implementation snapshot's memory invariants: "single
`StudioState` (never duplicate types)". Your test must keep it that way; the duplicate-type check
below is part of the item.

**Testing reality.** `eustress/crates/engine/src/ui/slint_ui.rs` contains **zero** `#[test]`
functions across 23,103 lines (MEASURED at authoring time). The `eustress-engine` crate contains
797. A search of `.github/workflows/` for `cargo test` or `cargo clippy` returns **no matches** —
no test in this repository runs in CI. Your test's value is therefore entirely in a human or an L1
running it, so it must be runnable by one obvious command and must be fast.

**CI is out of scope for you.** The phase exit condition mentions CI, but `.github/workflows/` is
out of scope for every item in this pack. CI is owned by pack T5
(`docs/PROMPTS/packs/T5_robustness_and_cohesion.md`): `G7.34` is the item that adds the `cargo test`
gate to `.github/workflows/ci.yml`, and `G7.43` is the item that keeps it re-running. Deliver the
test so `G7.34` can gate on it; do not touch the workflow yourself.

**Build reality.** A full engine build takes 10-15 minutes; one cargo build at a time (shared
`target/`; concurrent builds give LNK2001 or SAC os error 4551); never kill a build mid-compile.
Validate with `cargo run`, never `cargo check`. `cargo test` is required by this item and is not
the same thing as `cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`;
the cargo workspace root is `E:/Workspace/EustressEngine/eustress`.

## 3. Scope

### In scope — files this item may edit, create, or delete
- `eustress/crates/engine/src/ui/drain_contract.rs` (create — the test module and the startup guard)
- `eustress/crates/engine/src/ui/mod.rs` (add the module declaration only)
- `eustress/crates/engine/src/ui/slint_ui.rs` (add the startup guard call and, if needed, make the drain's required-parameter set introspectable — no behaviour change to any action arm)
- `eustress/crates/engine/src/ui/slint_main.rs` (**delete**)
- `docs/PROMPTS/artifacts/G6.03/` (create; artifacts)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` and `docs/PROMPTS/artifacts/G6.02/` — frozen prior evidence
- Any `.slint` file
- The bodies of the `SlintAction` match arms in `drain_slint_actions` — this item proves the drain runs; it does not change what the drain does
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Here that means: do not make the test pass by converting a required parameter to `Option<...>`.
  That silences the symptom and *deepens* the defect — an `Option` parameter that is `None` makes
  that action arm a silent no-op instead of making the whole drain a loud one. If a parameter
  genuinely should be optional, that is a separate change with its own justification, and the test
  must still cover it.
- The test must run **headless**, under `cargo test`, on a machine with no display. If proving the
  drain ran requires a windowed Slint context, you have chosen the wrong assertion — assert on the
  Bevy side (a queued action is consumed and the expected event or state change is observable),
  not on the Slint side.
- The test must be a **negative test as well as a positive one**. A test that only asserts "the app
  builds" does not close this class. You must demonstrate, with evidence in the artifact, that the
  test fails when a required registration is removed.
- Do not delete `slint_main.rs` without first confirming, in the artifact, that nothing references
  it: a search for `slint_main` across `eustress/crates/` must return zero matches afterwards.
- Batch verification. Six builds is the whole budget and two of them are deliberate-break runs.

## 5. Exit criterion

### Criterion
`cargo test -p eustress-engine --lib ui::drain_contract` passes with **at least 3** tests, and the
same suite **fails** with a non-zero exit code when any one required drain registration is removed
— demonstrated for **at least 2** distinct required parameters. Additionally, `slint_main.rs` no
longer exists and no file under `eustress/crates/` references the identifier `slint_main`.

### Measurement

Step 1 — the positive run:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.03_check_1.ps1

The checker runs the suite with `--test-threads=1 --nocapture`, tees the full output to
`docs/PROMPTS/artifacts/G6.03/positive_run.txt`, prints `EXITCODE=<n>`, and exits with the cargo
test exit code.

Step 2 — the dead-module check:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.03_check_2.ps1

The checker counts references to the identifier `slint_main` across `eustress/crates` and tests
whether `eustress/crates/engine/src/ui/slint_main.rs` still exists. It prints `DEADCODE_OK` and
exits 0 only when the reference count is 0 and the file is gone.

Step 3 — the negative runs. For each of `LabelEditState` and `TerrainVisibility`, comment out its
registration line in `eustress/crates/engine/src/ui/slint_ui.rs` (the `init_resource` calls are
grouped beginning at `:1360`), rerun the step-1 checker with
`-Out ../docs/PROMPTS/artifacts/G6.03/negative_run_<param>.txt`, record the exit code, then restore
the line. Both runs must report a non-zero exit code.

Expected output shape (step 1):

    running 3 tests
    test ui::drain_contract::every_required_drain_param_is_registered ... ok
    test ui::drain_contract::drain_consumes_a_queued_action ... ok
    test ui::drain_contract::studio_state_is_defined_exactly_once ... ok
    test result: ok. 3 passed; 0 failed
    EXITCODE=0

Pass condition:

    step 1 EXITCODE == 0 AND the reported test count >= 3
    AND step 2 prints DEADCODE_OK and exits 0
    AND both step-3 negative runs report a non-zero exit code

Grep the `EXITCODE=` marker and the `DEADCODE_OK` marker. Do not infer success from the tests
appearing in the output — a test binary that fails to link also prints no failures.

### The three required tests

1. `every_required_drain_param_is_registered` — build the real `App` through the same plugin
   registration path the studio uses, run one `Update`, and assert that no system in the
   `SlintSystems::Drain` set was skipped. If the pinned Bevy does not expose skip status directly,
   assert equivalently: for every required parameter enumerated in
   `docs/PROMPTS/artifacts/G6.01/ux_baseline_census.json` field `drain_required_params`, assert the
   corresponding resource or message is present in the `World` after startup. Enumerating from the
   census is acceptable and preferred — it keeps the test honest against the census.
2. `drain_consumes_a_queued_action` — push one benign `SlintAction` onto the queue, run one
   `Update`, assert the queue is empty **and** the expected observable side effect occurred. Queue
   emptiness alone is insufficient: a drain that runs and drops everything also empties the queue.
3. `studio_state_is_defined_exactly_once` — assert the workspace contains exactly one definition of
   `StudioState`. A source-text scan is acceptable and is exactly the check that would have caught
   the historical duplicate-state defect.

## 6. Critic gate

`critic_gate` is `[]`. There is no visual artifact and nothing for a blind Critic to score. The
mechanical criterion in §5 replaces it and is unusually tight for that reason: the item is not
satisfied by a passing test, only by a passing test **demonstrated to fail** on two independently
reintroduced defects. A green test nobody proved could go red is decoration.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — build the real App in-test and assert on World contents after startup
   -> if still failing, MANDATORY approach change. Adding a fourth assertion is NOT an approach
      change; moving from World introspection to a compile-time enumeration of the drain's
      parameter set (one const list that both the system and the test consume) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where neither negative run flips to a non-zero
                  exit code
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the assertion requires a windowed Slint context (see front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the requirement to a
startup-time runtime guard with no test, stating what regresses; FUND approach D; DEFER behind a
named item; or KILL with a statement of what the phase exit condition loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.03/drain_contract_report.json`

A reader finds: the list of required drain parameters and where each is registered (`path:line`);
the three test names and the positive run's exit code; for each of the two deliberate breaks, the
parameter removed, the line commented, and the resulting non-zero exit code and failure message;
the confirmation that `slint_main.rs` was deleted and that zero references remain; and the commit
hash. Archived alongside: `positive_run.txt` and one `negative_run_<param>.txt` per break.

This is the W3 evidence for the item and the artifact the phase exit condition cites.

## 9. Definition of NOT done

- The test passes and was never shown to fail. Without the negative runs there is no evidence the
  test constrains anything.
- A required parameter was changed to `Option<...>` to make the test green. That converts a loud,
  total failure into a quiet, partial one — strictly worse, because the next occurrence looks like
  "one button is broken" instead of "all of them are".
- The test asserts only that the queue emptied. A drain that runs and discards every action passes
  that assertion while the studio is just as dead.
- The test requires a display and is skipped in a headless run, so it will never guard anything in
  an automated context.
- `slint_main.rs` is left in place "for reference". Its 136 `SlintAction` references are precisely
  what causes an agent to edit the wrong drain and conclude the change had no effect.
- The 45 message writers in `DrainEventWriters` are ignored because "messages are always
  registered". They are registered by explicit `add_message` calls in the same plugin; a removed
  one fails validation exactly like a resource does.
- The item ends by adding a `cargo test` step to `.github/workflows/ci.yml`. That file is out of
  scope for every item in this pack; the CI wiring belongs to `G7.34` in pack T5.
```

---
---

# `docs/PROMPTS/items/G6.04_drain-never-blocks.md`

```markdown
---
id: G6.04
title: The studio UI never blocks on the engine — p99 drain cost under 2 ms
workload: W1
workload_secondary: [W3, W6]
phase: G6
depends_on: [G6.02, G6.03, G1.12]
blocks: [G6.05, G6.07, G6.08, G6.10, G6.11, G6.13, G6.15, G6.31]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D4]
capture_recipe: docs/PROMPTS/harness/recipes/G6_ui_sequences.json
artifact: docs/PROMPTS/artifacts/G6.04/drain_latency_after.json
escalation: >
  If moving synchronous filesystem work off the drain path requires an async runtime that is not
  already a dependency of eustress-engine, STALL rather than adding one. A new runtime in the UI
  crate is an architecture decision, not an item-level one.
status: DRAFT
notes: >
  Tier L: cross-cutting change across many drain arms, and each iteration costs a 10-15 minute
  build plus a manual measurement session. Design so one build validates several arms.
---

## 1. Objective

Interacting with the Eustress Studio never stalls the window. Measured on a 10,000-part scene
across the four declared interaction sequences, the per-frame cost of `drain_slint_actions` has a
p99 at or below 2.0 ms and a maximum at or below 8.0 ms, and no single frame attributes more than
1.0 ms to synchronous filesystem work on the drain path. The user-visible consequence is that a
click is acknowledged on the next frame, every time, including while the engine is busy.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust; Slint *is* Rust here. Units are meter-native; studs are a display unit only.

**The rule this item enforces.** The UI thread must never block on the engine. In this codebase the
UI and the Bevy `Update` schedule run on the same thread, so "blocking on the engine" concretely
means: a `SlintAction` arm that performs synchronous work whose duration is not bounded by a few
hundred microseconds. While that arm runs, no frame presents, and the window is unresponsive.

**Where the blocking is.** One system, `drain_slint_actions`
(`eustress/crates/engine/src/ui/slint_ui.rs:4965`), registered `.in_set(SlintSystems::Drain)` at
`slint_ui.rs:1428`, contains the match over every `SlintAction` variant
(`enum SlintAction` at `slint_ui.rs:250`). MEASURED at authoring time by `Select-String` over
`eustress/crates/engine/src/ui/slint_ui.rs` for
`std::fs::(read_to_string|write|read_dir|create_dir_all|copy|remove)`: **64 matches**. Verified
call sites on drain-reachable paths include lines 780, 793, 3373, 3800, 3862, 3892, 5642, 5719,
5930, 6215, 6223, 7038, 7206, 7224, 7333, 7355, 10827, 10951, and 11998.

The worst single offender is the `SlintAction::ImportAsset` arm, which opens a **synchronous modal
OS file picker** — `rfd::FileDialog::new()` … `.pick_file()` — inside the drain. The source comment
there argues the synchronous call is "fine here because this arm fires from the user's deliberate
ribbon click, not a hot path". That reasoning is about *frequency*, not about *duration*: while the
picker is open the studio presents no frames at all, which is exactly the stall a professional user
reads as "this tool is not finished".

**Your instrument.** Item `G6.02` (already `PASSED`) delivered `EUSTRESS_UI_TRACE=1`, which writes
a per-frame CSV with columns `frame`, `tick`, `input_event_ms`, `action_enqueued_ms`,
`drain_start_ms`, `drain_end_ms`, `drain_io_blocking_ms`, `sync_end_ms`, `present_ms`,
`input_to_present_ms`, `trace_overhead_ms`, plus the analysis script
`docs/PROMPTS/artifacts/G6.02/analyze_trace.ps1`. The baseline it produced is at
`docs/PROMPTS/artifacts/G6.02/ui_trace_baseline.json`. **Read that file for the before-numbers; do
not re-derive them and do not quote numbers from this prompt as the baseline.**

**Your safety net.** Item `G6.03` (already `PASSED`) delivered
`cargo test -p eustress-engine --lib ui::drain_contract`, which fails if a required drain parameter
loses its registration. Every build in this item must keep that suite green — moving work off the
drain is exactly the kind of change that drops a registration.

**The trap.** Bevy skips a system whose parameters fail validation. Adding a required (non-`Option`)
resource parameter to a drain-path system without registering the resource makes Bevy silently skip
`drain_slint_actions` every frame, killing **every** UI click while logging one `failed validation`
WARN. Source comments at `slint_ui.rs:1373-1378` and `slint_ui.rs:1379-1383` record this having
shipped twice. Prefer `Option<Res<...>>`, or register in the same edit alongside the `init_resource`
calls that begin at `slint_ui.rs:1360`.

**Build reality.** A full engine build takes 10-15 minutes. One cargo build at a time — the
workspace shares a single `target/` and concurrent builds produce LNK2001 or SAC os error 4551.
Never kill a build mid-compile; recover with `cargo clean -p eustress-engine`. Validate with
`cargo run`, never `cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/ui/slint_ui.rs` — the `SlintAction` match arms only
- `eustress/crates/engine/src/ui/file_dialogs.rs` — to make the picker non-blocking
- `eustress/crates/engine/src/ui/file_event_handler.rs` — to receive deferred work
- `eustress/crates/engine/src/ui/mod.rs` — new module declarations only
- New modules under `eustress/crates/engine/src/ui/` for deferred-work queues
- `docs/PROMPTS/artifacts/G6.04/` (create; artifacts)
- `docs/PROMPTS/harness/recipes/G6_ui_sequences.json` (create if absent, following the recipe schema in `docs/PROMPTS/02_CAPTURE_HARNESS.md` §8)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/`, `G6.02/`, `G6.03/` — frozen prior evidence
- `eustress/crates/engine/src/ui_trace.rs` — **the instrument is frozen.** Editing what it measures while measuring is the definition of moving the goalposts
- Any `.slint` file — this item changes when work happens, not what the surface looks like
- `eustress/crates/engine/src/ui/drain_contract.rs` — the regression test is frozen; it must pass unchanged
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: editing `ui_trace.rs`; excluding the `ImportAsset` frames from the
  percentile; measuring on a smaller scene than the 100x100 benchmark grid; shortening the
  interaction sequences; raising the `trace_overhead_ms` allowance; and computing p99 over a
  filtered subset. If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with
  evidence and stop.
- Do not add an async runtime to `eustress-engine`. See the escalation trigger. Deferred work
  belongs on a `std::thread` plus a channel drained by an existing Bevy system, or in an already
  present Bevy task pool.
- Do not make an arm "fast" by dropping its work. A `SlintAction` that used to write a file and now
  writes nothing is a data-loss bug that measures beautifully. Every deferred operation must still
  complete, and the artifact must show where it completes.
- Preserve ordering guarantees. If two actions previously took effect in queue order because both
  were synchronous, deferring one must not let the other overtake it in a user-visible way.
- The user must be told when work is deferred. An operation that now takes 400 ms in the background
  needs a visible pending affordance; a silently deferred save is worse than a blocking one.
- Batch verification. Twelve builds covers three approaches; one build should validate a group of
  arms, not one arm.

## 5. Exit criterion

### Criterion
On the 10,000-part benchmark scene, across the four interaction sequences, the measured
`drain_ms` distribution satisfies **p99 ≤ 2.0 ms** and **max ≤ 8.0 ms**, and
**max `drain_io_blocking_ms` ≤ 1.0 ms**, while `cargo test -p eustress-engine --lib
ui::drain_contract` still exits 0.

### Measurement

Step 1 — regenerate the identical scene (same seed as the baseline):

    cargo run --release -p eustress-engine --bin generate-benchmark-map -- --grid-size 100 --spacing 4.0 --seed 42

Step 2 — run the four sequences with the frozen instrument armed:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.04_check_1.ps1

The checker arms `EUSTRESS_UI_TRACE=1`, points `EUSTRESS_UI_TRACE_OUT` at
`docs/PROMPTS/artifacts/G6.04/ui_trace_after.csv`, launches the studio, and exits with the studio's
own exit code. The drain gate is step 3.

The four sequences are the ones named in `docs/PROMPTS/02_CAPTURE_HARNESS.md` §4.3:
`panel_open` (idle, hover panel tab, click, settled), `entity_select` (idle, hover entity, click,
settled with gizmo), `property_edit` (select, focus field, type value, commit, re-read after
reload), `error_surface` (trigger a known-invalid action, error appears, error dismissed). Run each
five times.

Step 3 — derive and gate:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.04/gate_drain.ps1 -Csv docs/PROMPTS/artifacts/G6.04/ui_trace_after.csv -Baseline docs/PROMPTS/artifacts/G6.02/ui_trace_baseline.json -Out docs/PROMPTS/artifacts/G6.04/drain_latency_after.json

`gate_drain.ps1` is written by this item. It computes `drain_ms = drain_end_ms - drain_start_ms`
per row, reports p50/p99/max of `drain_ms`, max of `drain_io_blocking_ms`, and p50/p99/max of
`input_to_present_ms`; copies the baseline values in for side-by-side reading; prints
`DRAIN_OK` and exits 0 when `drain_p99_ms <= 2.0 AND drain_max_ms <= 8.0 AND
drain_io_blocking_max_ms <= 1.0`; otherwise prints `DRAIN_FAIL` and exits 1.

Step 4 — the regression test must still pass:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.04_check_2.ps1

The checker runs the frozen suite, prints `EXITCODE=<n>`, and exits with the cargo test exit code.

Expected output shape (step 3):

    {"drain_p50_ms":0.11,"drain_p99_ms":1.62,"drain_max_ms":6.9,"drain_io_blocking_max_ms":0.4,
     "input_to_present_p99_ms":31.2,"baseline_drain_p99_ms":58.7,"baseline_drain_max_ms":1180.3}
    DRAIN_OK

Pass condition:

    drain_p99_ms <= 2.0  AND  drain_max_ms <= 8.0  AND  drain_io_blocking_max_ms <= 1.0
    AND step 3 exits 0  AND  step 4 EXITCODE == 0

Read the emitted values and the exit codes. Do not infer success from the CSV existing.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)**, floor **8.0**. The mean across dimensions is irrelevant; D4
below 8.0 fails the item.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_ui_sequences.json`, producing the `FS-UI` frame set
over harness scene `S4_studio_ui` at the three declared window sizes (1920x1080, 2560x1440,
3840x2160), per `docs/PROMPTS/02_CAPTURE_HARNESS.md` §4.3. Note that the harness scene set and the
`eustress-capture` binary are delivered by phase G1; if the recipe file does not exist, author it
against the schema in §8 of that document rather than inventing a new format.

Three properties of the Critic shape how you should work. It never sees anything you write about
your own work — not your result block, not a commit message, not a caption. Every score it gives
must cite a specific frame or a measured value, and an uncited score auto-fails the scorecard. And
it can refuse to pass something that clears every number. So the deferral affordance matters as
much as the milliseconds: a frame in which an operation is pending and nothing on screen says so
will be cited against you.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — defer the identified filesystem arms to a worker thread with a
                 result channel drained by an existing post-drain system
   -> if still failing, MANDATORY approach change. Deferring three more arms is NOT an approach
      change; restructuring the drain so that every arm is a bounded state transition and all
      I/O lives behind one command queue is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with drain_p99_ms moving < 5% AND the worst gated
                  dimension moving < 0.5
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the fix requires adding an async runtime to eustress-engine (front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the p99 ceiling to a stated
value with the consequence stated; FUND approach D with an estimate and why it differs materially;
DEFER behind a named item; or KILL with what the phase loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.04/drain_latency_after.json`

A reader finds: p50/p99/max of `drain_ms`, max `drain_io_blocking_ms`, p50/p99/max
`input_to_present_ms`, the corresponding baseline values from `G6.02` for side-by-side reading, the
scene parameters, the commit, the GPU and driver version, and the number of sequence repetitions.
Archived alongside: `ui_trace_after.csv`, `gate_drain.ps1`, and
`docs/PROMPTS/artifacts/G6.04/deferred_operations.md` — a table of every operation moved off the
drain, where it now completes, and what the user sees while it is pending.

This is the W1 evidence for the item; `G6.15` cites it.

## 9. Definition of NOT done

- p99 clears 2.0 ms because the `ImportAsset` arm now does nothing until a second click. The number
  moved; the stall moved with the user.
- A deferred write is fire-and-forget with no completion path, so a save that fails fails silently.
  That is a data-loss defect disguised as a latency win.
- `drain_io_blocking_ms` reads 0.0 everywhere because the deferred work no longer passes through the
  instrumented wrappers. Attribution must follow the work, not the call site.
- The numbers pass on an empty scene. The exit criterion names the 100x100 grid at seed 42 for a
  reason: the baseline was taken there.
- The drain is fast and `input_to_present_ms` is unchanged, because the real stall was in a
  post-drain `sync_*_to_slint` system. Report that honestly — it is a legitimate finding and a
  candidate follow-on item, not a reason to declare victory.
- `cargo test -p eustress-engine --lib ui::drain_contract` was not rerun after the last build, and a
  registration was dropped somewhere in the refactor. Every build in this item must end with step 4.
- The Critic passes the numbers but refuses the item, citing a frame where an operation is pending
  and the interface gives no sign of it. That is a legitimate refusal; the item is not done.
```

---
---

# `docs/PROMPTS/items/G6.05_properties-panel-persists.md`

```markdown
---
id: G6.05
title: The Properties panel persists every edit it visually accepts
workload: W1
workload_secondary: [W4, W3]
phase: G6
depends_on: [G6.01, G6.03, G6.04, G1.12]
blocks: [G6.14, G6.15]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D4]
capture_recipe: docs/PROMPTS/harness/recipes/G6_ui_sequences.json
artifact: docs/PROMPTS/artifacts/G6.05/property_roundtrip.json
escalation: >
  If persisting a property kind requires a schema change to the Fjall entity record format — i.e.
  the fix is a storage-format change rather than a write-back wiring change — STALL. A storage
  format decision is not an item-level call and would invalidate archived Spaces.
status: DRAFT
notes: >
  Tier L: the round-trip harness plus write-back wiring across many property kinds, each iteration
  gated on a 10-15 minute build. The 12 property kinds are the budget driver, not the mechanism.
---

## 1. Objective

Every property the Eustress Studio's Properties panel lets a user change is still changed after the
Space is closed and reopened. For a declared set of at least twelve property kinds spanning
transform, appearance, physics, naming, and attributes, the value written in the panel round-trips
through a save and a reload with no loss and no silent coercion.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` compiles to
Rust. Units are meter-native; studs are a display unit only — a property edited in studs is stored
in meters, and the round-trip must survive that conversion without drift.

**The defect, quoted from the audit.** `docs/AUDIT/02_STUDIO_ENGINE.md`, in its 2026-05-16 storage
note and again in its concept summary, records: *"the Properties panel does **not** persist edits in
the default build — legacy TOML write-back is gated behind an opt-in `toml` cargo feature and the
Fjall mirror only writes Transform so far."* Feature 5 (File I/O round-trip: load, edit, persist) is
marked 🟡. Feature 8 (class serialisation coverage) is marked 🟡. The wiring-gap list in that
document opens with "1. Properties write-back system (atomic + rename-suppressed)".

That is the state you start from. Do not restate it as though it were surprising, and do not begin
by re-confirming it — begin by measuring exactly which property kinds round-trip and which do not.

**Where the edit enters the system.** The Slint side raises `on_property_changed(key, value)`,
wired at `eustress/crates/engine/src/ui/slint_ui.rs:1870`, which pushes
`SlintAction::PropertyChanged(String, String)` (`slint_ui.rs:377`). The drain arm that handles it is
at `slint_ui.rs:7762`. Vector properties are decomposed into a synthesised
`PropertyChanged(name, "x, y, z")` event (see the comment at `slint_ui.rs:401` and the re-entry at
`slint_ui.rs:7745-7749`); colour edits re-enter as a second `PropertyChanged("Color", rgb)` at
`slint_ui.rs:7772`. The panel itself is `eustress/crates/engine/ui/slint/properties.slint`
(2,859 lines).

**The `toml` cargo feature.** `eustress/crates/engine/Cargo.toml` declares `[features]` at `:327`
with `default = ["core", "data"]`. Verified: exactly two files in
`eustress/crates/engine/src/` are gated on `feature = "toml"` — `src/billboard_gui.rs` and
`src/space/file_loader.rs`. Do **not** solve this item by making `toml` a default feature. The
storage direction of the project is Fjall-primary with TOML as legacy seed and human-editable
schema; re-enabling the legacy write path is a regression dressed as a fix.

**Your safety net.** Item `G6.03` (already `PASSED`) delivered
`cargo test -p eustress-engine --lib ui::drain_contract`. It must stay green through every build
here — the `PropertyChanged` arm is inside the drain, and a dropped registration kills every button
in the studio at once. Source comments at `slint_ui.rs:1373-1378` and `:1379-1383` record that
failure shipping twice.

**Instruments available to drive the studio from outside.** The MCP bridge exposes, from
`eustress/crates/mcp-server/src/bridge_tools.rs`: `select_entity` (`:1091`), `get_editor_state`
(`:1163`), `invoke_action` (`:1220`, which runs the same handler a key press does), `equip_tool`
(`:1041`). Also available from the tools crate: `create_entity`, `update_entity`, `query_entities`,
`find_entity`. `invoke_action` accepts the action names from `eustress/crates/engine/src/keybindings.rs`
(`pub enum Action` at `:7`), including `SaveScene`. Use these to script the round-trip rather than
driving the GUI by hand — a hand-driven round-trip is not rerunnable by a stranger.

**Build reality.** 10-15 minutes per build; one build at a time (shared `target/`; concurrent builds
give LNK2001 or SAC os error 4551); never kill a build mid-compile; recover with
`cargo clean -p eustress-engine`. Validate with `cargo run`, never `cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/ui/slint_ui.rs` — the `SlintAction::PropertyChanged` arm and its helpers
- `eustress/crates/engine/src/space/` — the write-back path into the Fjall store
- `eustress/crates/engine/ui/slint/properties.slint` — only to add commit/pending/failed affordances, not to restyle
- New modules under `eustress/crates/engine/src/space/` for write-back
- `docs/PROMPTS/artifacts/G6.05/` (create; artifacts, including the round-trip driver script)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.04/` — frozen prior evidence
- `eustress/crates/engine/Cargo.toml` `[features]` — do not change which features are default
- `eustress/crates/engine/src/ui/drain_contract.rs` — the regression test is frozen
- `eustress/crates/engine/src/ui_trace.rs` — the latency instrument is frozen
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: reducing the property set below twelve; choosing twelve properties that
  are all `Transform` fields; comparing with a tolerance wide enough to hide a coercion; verifying
  by reading the panel again in the same session instead of after a genuine close-and-reopen; and
  enabling the `toml` feature so the legacy path does the work. If the measurement is genuinely
  wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The twelve kinds must span at least five distinct categories. A suggested spread: `Name`
  (string), `Position` (Vec3, meters), `Size` (Vec3, meters), `Rotation` (Euler or quaternion),
  `Color` / `BrickColor` (colour with a named-palette round-trip), `Material` (enum), `Anchored`
  (bool), `CanCollide` (bool), `Transparency` (float 0..1), `CastShadow` (bool), a custom attribute
  (string key, typed value), and `Parent` (a reparent). Substitutions are allowed; the spread is
  not.
- Write-back must be atomic and must not fight the file watcher. The audit's wiring-gap entry says
  "atomic + rename-suppressed" for a reason: a naive write triggers a reload that clobbers the next
  edit.
- If a property cannot be persisted, the panel must **say so at edit time**, not accept the edit
  and lose it. A field that visibly refuses is honest; a field that visibly accepts and forgets is
  the defect this item exists to remove.
- Batch verification. Twelve builds across three approaches; one build should cover several
  property kinds.

## 5. Exit criterion

### Criterion
For each of at least **12** declared property kinds, a scripted edit-save-reload-read cycle returns
the written value: **12/12 round-trip**, with numeric comparison at a relative tolerance of `1e-6`
and exact comparison for strings, booleans, and enums. `cargo test -p eustress-engine --lib
ui::drain_contract` still exits 0.

### Measurement

Step 1 — build and launch the studio with the engine bridge available:

    cargo run --release -p eustress-engine --bin eustress-engine

Step 2 — run the round-trip driver, which for each property kind: `create_entity`, `select_entity`,
apply the edit through the same `PropertyChanged` path the panel uses, `invoke_action` with
`SaveScene`, close and reopen the Space, `find_entity`, and read the value back:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.05/roundtrip.ps1 -Out docs/PROMPTS/artifacts/G6.05/property_roundtrip.json

`roundtrip.ps1` is written by this item. It must print one line per property kind in the form
`<kind> written=<value> readback=<value> match=<true|false>`, then a summary line
`ROUNDTRIP <passed>/<total>`, then `ROUNDTRIP_OK` and exit 0 when `passed == total AND total >= 12`;
otherwise `ROUNDTRIP_FAIL` and exit 1.

Step 3 — the regression test must still pass:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.05_check.ps1

The checker runs the frozen suite, prints `EXITCODE=<n>`, and exits with the cargo test exit code.

Expected output shape (step 2):

    Name written=Bracket_A readback=Bracket_A match=true
    Position written=1.5,2.25,-3.0 readback=1.5,2.25,-3.0 match=true
    ...
    ROUNDTRIP 12/12
    ROUNDTRIP_OK

Pass condition:

    the summary line reads ROUNDTRIP <n>/<n> with n >= 12, step 2 exits 0,
    AND step 3 EXITCODE == 0

Read the summary line and the exit codes. A JSON file appearing is not a pass; the audit's whole
point is that this panel already looks like it worked.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)**, floor **8.0**. D4 below 8.0 fails the item regardless of the
round-trip count.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_ui_sequences.json`. The relevant sequence is
`property_edit`, which `docs/PROMPTS/02_CAPTURE_HARNESS.md` §4.3 defines as five steps: select,
focus field, type value, commit, **and re-read after a reload**. That fifth step exists precisely
because of this defect, and the same document records that D4 caps at 4.0 if the edit visually
succeeds and does not persist. Your captured frames must show the reloaded value.

The Critic never sees your self-report, cites or fails, and may refuse to pass a numerically clean
result. Design the commit affordance so that a single still frame shows whether a value is
committed, pending, or rejected.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — extend the Fjall write-back from Transform to the full declared
                 property set, atomic write plus watcher suppression
   -> if still failing, MANDATORY approach change. Adding a thirteenth property to the same
      write path is NOT an approach change; moving from per-property write-back to a
      component-level serialisation round-trip driven by the class schema is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the passing round-trip count moving by < 1
                  AND the worst gated dimension moving < 0.5
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the fix requires a Fjall record-format change (see front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the required property count to
a stated number, naming which kinds are dropped and what that costs; FUND approach D; DEFER behind
a named item; or KILL with what the phase loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.05/property_roundtrip.json`

A reader finds: one record per property kind with its category, the written value, the read-back
value, the comparison mode and tolerance, and the pass flag; the total and passing counts; the
commit; and the Space path used. Archived alongside: `roundtrip.ps1` (the driver), the console
transcript, and `docs/PROMPTS/artifacts/G6.05/unpersistable.md` — a list of any property the panel
now refuses to accept, with the reason shown to the user.

This is the W1 and W4 evidence for the item.

## 9. Definition of NOT done

- Twelve properties round-trip and eleven of them are components of `Transform`. The audit already
  records that Transform persists; a spread that avoids the gap does not close it.
- The round-trip is verified by reading the panel again without closing the Space. Values live in
  memory; the defect is on the persistence boundary.
- The `toml` cargo feature was enabled to make the legacy write path do the work. That reverses the
  project's storage direction to pass an item.
- Write-back works but fights the file watcher, so a rapid second edit is clobbered by the reload
  the first edit triggered. The scripted driver may not catch this; add a back-to-back edit case.
- A colour set from a named palette reads back as a slightly different sRGB triple. Silent coercion
  is a round-trip failure even when the pixels look identical.
- Meter-native storage and stud-based display disagree by a rounding step, and the tolerance was
  widened to absorb it. Fix the conversion; do not widen the tolerance.
- A property that cannot be persisted still renders as an editable field that visually accepts
  input. The panel must refuse visibly.
- The Critic passes the round-trip count but refuses the item, citing a frame in which a committed
  value is indistinguishable from an uncommitted one. That is a legitimate refusal.
```

---
---

# `docs/PROMPTS/items/G6.06_unwired-tool-honesty.md`

```markdown
---
id: G6.06
title: Pre-click honesty for the 1,969 unwired ribbon tools
workload: W4
workload_secondary: [W1]
phase: G6
depends_on: [G6.01, G1.12]
blocks: [G6.14, G6.15]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D4, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G6_ribbon_honesty.json
artifact: docs/PROMPTS/artifacts/G6.06/ribbon_honesty.json
escalation: >
  If distinguishing unwired tools requires changing which tools appear in a mode manifest — i.e.
  the fix is to hide tools rather than to label them — STALL. Removing declared intentions from the
  ribbon is a product decision about what the modes claim, not an item-level UI change.
status: DRAFT
notes: >
  Tier M. One data field threaded from Rust to Slint plus one visual treatment, but it touches the
  ribbon's hottest rendering path and must be validated on the real studio.
---

## 1. Objective

A professional user can tell, **before clicking**, which ribbon tools do something today. Every
tool id whose generated metadata says `wired: false` renders with a distinct, deliberate treatment
that reads as "declared, not yet built", and every tool id whose metadata says `wired: true`
renders as a normal, fully live control. No tool changes category as a side effect.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust; Slint *is* Rust here. Units are meter-native; studs are a display unit only.

**The exact state today.** `eustress/crates/engine/src/tool_metadata.rs` is 2,151 lines, generated
by `scripts/gen_tool_metadata.py` from the ten mode manifests in `eustress/crates/engine/modes/`
(`business`, `civil`, `engineering`, `gaming`, `government`, `health`, `justice`, `legal`,
`military`, `student`). Its `tool_meta(id) -> Option<ToolMeta>` function at `:147` is a match with
**1,999** arms. `ToolMeta` (`:12`) carries `label`, `tooltip`, `icon`, and `wired: bool`, whose doc
comment reads: *"True when clicking this actually does something today. False = a deliberate 'dream'
button: it renders fully but has no dispatch arm, so the UI must say so honestly and the click is
counted as demand."*

MEASURED at authoring time by `Select-String` over `tool_metadata.rs`: **30** arms end `, true),`
and **1,969** end `, false),`.

**What already works.** `eustress/crates/engine/src/ui/slint_ui.rs:11682` intercepts every
manifest-tool click, records it to usage telemetry with the `wired` flag (`:11689`), and for an
unwired tool emits a notification — *"<Label> is on the roadmap / Not built yet — your click was
counted as a vote for it"* — plus an Output-panel line, then `continue`s so the click never reaches
a silent no-op (`:11691-11705`). That post-click behaviour is correct and is **not** what this item
changes.

**What is missing.** The row builder at `eustress/crates/engine/src/ui/slint_ui.rs:15523` reads the
same `ToolMeta` and pushes a `CustomTabToolData` to Slint carrying `tab_id`, `section_id`, `label`,
`tooltip`, `action_id`, `icon_id`, and `tint` — **and not `wired`**. So all 1,999 buttons render
identically. A buyer evaluating the studio sees a dense, professional ribbon, clicks something
plausible, and learns only afterwards that it was an intention. That post-hoc discovery is the
single fastest way to lose a vertical deal, and it is why this item is tagged W4.

**Design constraint you inherit.** `eustress/crates/engine/modes/government.toml` states the
project's own posture in its header: *"every id has a `tool_metadata` entry (generated), but almost
none has live dispatch yet. That is the same honest-placeholder posture … a declared button is a
declared intention."* The intent is to **keep** declaring them, honestly. Do not solve this item by
hiding them.

**Selection colour, for the treatment you choose.** `eustress/crates/engine/ui/slint/theme.slint`
defines `ThemeData` (`:23`) and two palettes, `preset-classic` (`:135`) and `preset-modern`
(`:217`). The studio's selection identity is **cyan `#00bcd4`** — never call it teal. Verified in
the 3D surface at `eustress/crates/engine/src/adornment_renderer.rs:272`
(`Color::srgba(0.0, 0.737, 0.831, 0.85)`) and `eustress/crates/engine/src/lock_tool.rs:43`. Do not
use the selection colour for the unwired treatment; a "not built" state must not read as "selected".

**Build reality.** 10-15 minutes per build; one build at a time (shared `target/`; concurrent builds
give LNK2001 or SAC os error 4551); never kill a build mid-compile. Validate with `cargo run`, never
`cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/ui/slint/ribbon.slint` — the `CustomTabToolData` struct and the tool-button component
- `eustress/crates/engine/src/ui/slint_ui.rs` — the row builder at `:15523` only, to populate the new field
- `eustress/crates/engine/ui/slint/theme.slint` — only to add a token for the unwired treatment, in **both** presets
- `docs/PROMPTS/artifacts/G6.06/` (create; artifacts)
- `docs/PROMPTS/harness/recipes/G6_ribbon_honesty.json` (create if absent, following `docs/PROMPTS/02_CAPTURE_HARNESS.md` §8)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.05/` — frozen prior evidence
- `eustress/crates/engine/src/tool_metadata.rs` — **generated**; regenerate with `python scripts/gen_tool_metadata.py` if a manifest changes, never hand-edit
- `eustress/crates/engine/modes/*.toml` — do not add, remove, or reclassify a tool id
- `scripts/gen_tool_metadata.py`
- `eustress/crates/engine/src/usage_telemetry.rs` — the demand-counting path is frozen
- `eustress/crates/engine/src/ui/drain_contract.rs` and `eustress/crates/engine/src/ui_trace.rs`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: flipping any `wired` flag to `true` without a dispatch arm; hiding
  unwired tools; removing tool ids from a manifest; and filtering the ribbon to a subset before
  capture. If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with
  evidence and stop.
- The treatment must be **legible at a glance and not merely lower opacity**. A 60%-alpha button
  reads as "disabled because of context" — the wrong message. This state means "declared, not built
  yet", which is a different thing from disabled, and it must read that way in a still frame.
- The treatment must survive both palettes. `preset-classic` and `preset-modern` have different
  backgrounds; a token added to one and not the other is a defect, and `theme.slint`'s own comment
  at `:12-14` notes that every `ThemeData` field must be filled in both because Rust swaps the
  whole struct.
- Do not regress the post-click behaviour at `slint_ui.rs:11682-11705`. The notification and the
  telemetry vote must still fire; this item adds a *pre*-click signal, it does not replace the
  post-click one.
- Keep the ribbon's per-frame cost flat. The ribbon is rebuilt on mode and tab changes; do not add
  a per-button `tool_meta` lookup inside a render loop when the value can be resolved once in the
  row builder.

## 5. Exit criterion

### Criterion
`CustomTabToolData` carries a `wired` boolean; the row builder populates it from
`tool_metadata::tool_meta`; and for a full sweep of all ten mode manifests, the number of tool rows
emitted with `wired == false` equals the number of `wired: false` arms in the generated table
(**1,969** at authoring time — re-derive, do not assume), with **zero** rows where the emitted flag
disagrees with `tool_meta`.

### Measurement

Step 1 — derive the ground truth from the generated table:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.06_check_1.ps1

The checker counts the `, true),` and `, false),` literals in
`eustress/crates/engine/src/tool_metadata.rs` and prints `truth_wired=<n> truth_unwired=<n>`. Those
two numbers are the ground truth step 3 compares against; do not substitute the numbers quoted in
§5's criterion.

Step 2 — run the studio with the ribbon-sweep dump armed. This item adds an
`EUSTRESS_RIBBON_DUMP=<path>` environment knob that, on startup, walks every mode and submode,
builds the tool rows exactly as the ribbon does, and writes one JSON line per row
(`{mode, submode, action_id, label, wired}`), then exits:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.06_check_2.ps1

The checker points `EUSTRESS_RIBBON_DUMP` at
`docs/PROMPTS/artifacts/G6.06/ribbon_rows.jsonl`, launches the studio, prints `EXITCODE=<n>`, and
exits with the studio's own exit code.

Step 3 — compare:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.06/check_honesty.ps1 -Rows docs/PROMPTS/artifacts/G6.06/ribbon_rows.jsonl -Table eustress/crates/engine/src/tool_metadata.rs -Out docs/PROMPTS/artifacts/G6.06/ribbon_honesty.json

`check_honesty.ps1` is written by this item. It parses the generated table for each id's `wired`
value, joins against the dumped rows on `action_id`, and reports `rows_total`, `rows_wired`,
`rows_unwired`, `mismatches` (rows whose emitted flag differs from the table), and
`ids_missing_from_dump`. It prints `HONESTY_OK` and exits 0 when `mismatches == 0 AND
ids_missing_from_dump == 0 AND rows_unwired == truth_unwired`; otherwise `HONESTY_FAIL`, exit 1.

Expected output shape:

    truth_wired=30 truth_unwired=1969
    {"rows_total":1999,"rows_wired":30,"rows_unwired":1969,"mismatches":0,"ids_missing_from_dump":0}
    HONESTY_OK

Pass condition:

    mismatches == 0  AND  ids_missing_from_dump == 0  AND  rows_unwired == truth_unwired
    AND step 3 exits 0

Read the emitted counts and the exit code. The visual treatment is judged by the Critic in §6; this
command proves the data is correct, which is the precondition for the treatment meaning anything.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)** and **D6 (overall coherence)**, floor **8.0 each**. Either below
8.0 fails the item; the mean is irrelevant.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_ribbon_honesty.json`, producing full-window frames
of the ribbon in at least three modes (one with mostly wired tools, one with mostly unwired, one
mixed), in **both** palettes, at the three declared window sizes.

D6 is gated because a treatment that reads correctly in isolation but clashes with the rest of the
ribbon — a novel colour, a second icon language, a badge that competes with the tool glyph — fixes
one problem and creates a worse one. The treatment must look like it was always part of the design
system.

The Critic never sees anything you write about your own work; every score it gives cites a frame or
a measured value; and it can refuse a numerically clean result. Design so the distinction is
obvious in a single still frame with no motion, no tooltip, and no hover.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — thread `wired` into CustomTabToolData; apply a distinct
                 non-opacity treatment in ribbon.slint driven by a new theme token
   -> if still failing, MANDATORY approach change. Adjusting the treatment's colour is NOT an
      approach change; moving from a per-button treatment to a grouped layout that separates
      live tools from declared ones within each section is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  the mismatch count unchanged
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the fix requires editing a mode manifest (see front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the gate to the data-correctness
half only, stating what the buyer still experiences; FUND approach D; DEFER behind a named item; or
KILL with what W4 loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.06/ribbon_honesty.json`

A reader finds: the ground-truth wired and unwired counts derived from the generated table; the
emitted row counts; the mismatch list (empty on pass); the list of ids present in the table but
absent from the dump (empty on pass); the theme token added and its value in both palettes; and the
commit. Archived alongside: `ribbon_rows.jsonl`, `check_honesty.ps1`, and the capture bundle
manifest.

This is the W4 evidence for the item: it is what lets a sales conversation say "1,969 of these are
declared intentions and the interface says so" instead of discovering it live.

## 9. Definition of NOT done

- The count matches because a `wired` flag was flipped to `true` somewhere. Flags follow dispatch
  arms; a flag without an arm is the original defect with extra steps.
- Unwired tools are hidden, or the ribbon is filtered to wired tools only. The modes' declared
  intentions are the product's roadmap surface; erasing them fails W4 in the other direction.
- The treatment is `opacity: 0.5`. That reads as "disabled right now, try again later", which is a
  different and misleading claim.
- The treatment appears in `preset-classic` and not in `preset-modern`, because a `ThemeData` field
  was added to one literal and not the other.
- The unwired treatment reuses cyan `#00bcd4`, the selection identity, so an unwired button reads
  as selected.
- The pre-click signal is added and the post-click notification at `slint_ui.rs:11691` is removed as
  redundant. The click is also a demand vote; deleting it destroys the usage-telemetry signal.
- The dump knob only walks the currently active mode, so `ids_missing_from_dump` is large and the
  comparison is meaningless. The sweep must cover all ten manifests.
- The Critic passes D4 and refuses D6, citing that the ribbon now reads as two visual languages in
  one strip. That is a legitimate refusal; the item is not done.
```

---
---

# `docs/PROMPTS/items/G6.07_command-palette-click-depth.md`

```markdown
---
id: G6.07
title: Command palette — every top-20 command reachable in at most two clicks
workload: W6
workload_secondary: [W1]
phase: G6
depends_on: [G6.01, G6.02, G6.04, G1.12]
blocks: [G6.08, G6.14, G6.15]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D4]
capture_recipe: docs/PROMPTS/harness/recipes/G6_ui_sequences.json
artifact: docs/PROMPTS/artifacts/G6.07/click_depth_after.json
escalation: >
  If the palette's median keystroke-to-result latency exceeds 120 ms on the 10,000-part scene,
  STALL rather than shipping it. A command surface that lags behind typing is worse than a deeper
  menu, and trading G6.04's latency win for a discoverability win fails the pack.
status: DRAFT
notes: >
  Tier L. The palette itself is a new Slint component plus a Rust index; the budget goes to
  wiring it to the real action surface without duplicating dispatch.
---

## 1. Objective

A professional user reaches any of the twenty most-used studio commands in at most two mouse
actions from a cold start, through one searchable command surface, and the surface responds to
typing without perceptible lag. Measured mean click depth across the fixed top-20 list drops to
**≤ 2.0** with **no command at `-1`** (unreachable by mouse).

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust; Slint *is* Rust here. Units are meter-native; studs are a display unit only.

**The fixed top-20 command list.** Established by item `G6.01` and used unchanged by every
downstream item so the numbers compare: Undo, Redo, Save, Copy, Paste, Duplicate, Delete, Group,
Ungroup, Select All, Select Tool, Move Tool, Rotate Tool, Scale Tool, Insert Part, Insert Script,
Play Solo, Stop, Toggle Explorer, Toggle Properties.

**Click depth, defined.** From a cold start — studio open, default layout, nothing selected, no
panel expanded, mouse only, keyboard forbidden — the number of discrete mouse-down events required
to invoke the command. A command on the visible ribbon Home tab is depth 1. One tab switch then one
click is depth 2. A command with no mouse route at all is `-1`, not a large number.

**The before-number.** `docs/PROMPTS/artifacts/G6.01/ux_baseline_census.json`, field
`top20_command_click_depth`, holds one entry per command with `clicks_from_cold_start` and a
`path_chain` of `path:line` steps. **Read it. Do not re-derive it and do not quote a number from
this prompt as the baseline.**

**What exists today, and what it is not.**
`eustress/crates/engine/ui/slint/command_bar.slint` is a **script REPL**, not a command palette. Its
own header states it is "Docked at the bottom of the engine window for executing Rune/Luau scripts.
Output goes to the existing Output panel". It does declare a `CommandSuggestion` struct
(`id`, `label`, `description`, `shortcut`, `category`) and an `on-select-suggestion(int)` callback,
so there is a suggestion mechanism to build on, but it is bound to script text, not to studio
actions. The Rust side is `CommandBarState` at `eustress/crates/engine/src/ui/slint_ui.rs:462`, with
a thin façade module at `eustress/crates/engine/src/ui/mod.rs:763`.

There is no fuzzy command palette over the studio's action surface today. That capability is 0%.

**The action surface to index.** `eustress/crates/engine/src/keybindings.rs` is 2,268 lines and
defines `pub enum Action` at `:7` — 38 unit variants covering tools, transforms, file operations,
panels, and playback — referenced 239 times in that file. Default bindings are installed around
`:345-430`, and a test `no_duplicate_default_bindings` at `:2131` already asserts no two actions
share a default chord. The MCP bridge exposes `invoke_action`
(`eustress/crates/mcp-server/src/bridge_tools.rs:1220`) which, per its own description, "Runs the
SAME handler a real key press does" — so `Action` is the canonical dispatch identity and your
palette must dispatch through it rather than re-implementing any command.

The ribbon's manifest tools are a second, larger surface: 1,999 ids in
`eustress/crates/engine/src/tool_metadata.rs` (`tool_meta` at `:147`), of which 30 carry
`wired: true`. The palette may index them, but if it does, it must respect the `wired` flag exactly
as item `G6.06` establishes — an unwired result must be visibly distinguished in the palette too.

**Latency budget you must not spend.** Item `G6.04` brought `drain_slint_actions` to a p99 at or
below 2.0 ms with `drain_io_blocking_ms` max at or below 1.0 ms, measured by the frozen instrument
from `G6.02` (`EUSTRESS_UI_TRACE=1`, analysis in
`docs/PROMPTS/artifacts/G6.02/analyze_trace.ps1`). Your palette runs on the same thread. A fuzzy
search over two thousand entries per keystroke, executed inside a drain arm, would undo that work
in one commit.

**Build reality.** 10-15 minutes per build; one build at a time (shared `target/`; concurrent builds
give LNK2001 or SAC os error 4551); never kill a build mid-compile; recover with
`cargo clean -p eustress-engine`. Validate with `cargo run`, never `cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/ui/slint/command_palette.slint` (create)
- `eustress/crates/engine/ui/slint/main.slint` — mount the palette and add its single entry affordance
- `eustress/crates/engine/src/ui/command_palette.rs` (create — the index and the query)
- `eustress/crates/engine/src/ui/mod.rs` — module declaration only
- `eustress/crates/engine/src/ui/slint_ui.rs` — one new `SlintAction` variant and its arm, dispatching through `keybindings::Action`
- `docs/PROMPTS/artifacts/G6.07/` (create; artifacts)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.06/` — frozen prior evidence
- `eustress/crates/engine/src/keybindings.rs` — the action surface is the input to this item, not its subject; `G6.08` owns bindings
- `eustress/crates/engine/ui/slint/command_bar.slint` — the script REPL stays as it is; do not repurpose it
- `eustress/crates/engine/src/tool_metadata.rs` — generated
- `eustress/crates/engine/src/ui_trace.rs`, `eustress/crates/engine/src/ui/drain_contract.rs` — frozen instruments

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: altering the fixed top-20 list; redefining click depth (for instance
  counting a keyboard shortcut as a click); measuring depth from a non-default layout; and counting
  the palette's own opening click as free. Opening the palette **is** a click and counts toward the
  depth of every command reached through it. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Dispatch through `keybindings::Action`. Do not add a second code path that performs a command;
  duplicate dispatch is how the studio acquired its historical duplicate-state defects.
- The index is built once, not per keystroke. Query cost must be independent of the action count in
  the observable range.
- The palette must not steal focus destructively. Opening it from a text field and dismissing it
  must return focus and the caret exactly where they were.
- Respect `wired`. If manifest tools are indexed, an unwired result must carry the same honest
  treatment `G6.06` established, and selecting one must still fire the roadmap notification and the
  demand vote at `eustress/crates/engine/src/ui/slint_ui.rs:11682-11705`.
- Batch verification. Twelve builds across three approaches.

## 5. Exit criterion

### Criterion
Mean `clicks_from_cold_start` across the fixed top-20 command list is **≤ 2.0**, **no command is
`-1`**, and **no command exceeds 3**; and the palette's keystroke-to-rendered-results latency has a
median **≤ 120 ms** and a p99 **≤ 250 ms**, measured on the 10,000-part scene with the frozen
instrument.

### Measurement

Step 1 — regenerate the scene:

    cargo run --release -p eustress-engine --bin generate-benchmark-map -- --grid-size 100 --spacing 4.0 --seed 42

Step 2 — re-run the click-depth census with the same method `G6.01` used, over the same fixed list,
recording a `path_chain` per command:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.07/measure_click_depth.ps1 -Baseline docs/PROMPTS/artifacts/G6.01/ux_baseline_census.json -Out docs/PROMPTS/artifacts/G6.07/click_depth_after.json

`measure_click_depth.ps1` is written by this item. For each of the twenty commands it records the
new depth and the `path:line` chain that establishes each step, carries the baseline depth across
for side-by-side reading, computes `mean_depth_after`, `max_depth_after`, and
`unreachable_count_after`, and prints `DEPTH_OK` with exit 0 when `mean_depth_after <= 2.0 AND
max_depth_after <= 3 AND unreachable_count_after == 0`; otherwise `DEPTH_FAIL`, exit 1.

Step 3 — palette latency, using the frozen `G6.02` instrument:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.07_check.ps1

The checker arms `EUSTRESS_UI_TRACE=1`, points `EUSTRESS_UI_TRACE_OUT` at
`docs/PROMPTS/artifacts/G6.07/palette_trace.csv`, launches the studio, and exits with the studio's
own exit code.

Open the palette and type each of twenty distinct queries at a natural typing rate, five
repetitions. Then:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.07/palette_latency.ps1 -Csv docs/PROMPTS/artifacts/G6.07/palette_trace.csv -Out docs/PROMPTS/artifacts/G6.07/palette_latency.json

`palette_latency.ps1` restricts to frames where a palette keystroke was the input event, computes
median and p99 of `input_to_present_ms`, and prints `PALETTE_OK` with exit 0 when
`median <= 120 AND p99 <= 250`; otherwise `PALETTE_FAIL`, exit 1.

Expected output shape:

    {"mean_depth_before":2.85,"mean_depth_after":1.60,"max_depth_after":2,"unreachable_count_before":3,"unreachable_count_after":0}
    DEPTH_OK
    {"palette_keystroke_median_ms":41.2,"palette_keystroke_p99_ms":103.8,"samples":1000}
    PALETTE_OK

Pass condition:

    mean_depth_after <= 2.0  AND  max_depth_after <= 3  AND  unreachable_count_after == 0
    AND palette_keystroke_median_ms <= 120  AND  palette_keystroke_p99_ms <= 250
    AND both scripts exit 0

Read the emitted values and both exit codes.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)**, floor **8.0**. Below 8.0 fails the item regardless of the depth
number.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_ui_sequences.json`, `FS-UI` frame set over harness
scene `S4_studio_ui` at the three declared window sizes. Include palette-open, mid-query, and
result-selected frames.

The Critic never sees your self-report, must cite a frame or a measured value for every score, and
can refuse a numerically clean result. Priorities that follow from that: the palette must look like
part of this studio and not like a borrowed component; result rows must show the keyboard shortcut
where one exists, because that is how a palette teaches; and an empty query must show something
useful rather than an empty box (recent and suggested commands), since the empty state is the
frame most likely to be captured.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — a modal palette over the whole window, indexing keybindings::Action
                 plus wired manifest tools, prefix-and-subsequence matching
   -> if still failing, MANDATORY approach change. Changing the match ranking is NOT an approach
      change; moving from a modal palette to an always-present omnibox in the title area, or to
      a context-sensitive surface anchored on the current selection, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with mean_depth_after moving < 5% AND the worst
                  gated dimension moving < 0.5
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: palette keystroke median latency exceeds 120 ms (see front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the mean-depth ceiling to a
stated value with the consequence; FUND approach D; DEFER behind a named item; or KILL with what W6
loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.07/click_depth_after.json`

A reader finds: one record per command in the fixed top-20 list with its baseline depth, its new
depth, and the `path:line` chain for each step; the mean, max, and unreachable counts before and
after; and the commit. Archived alongside: `measure_click_depth.ps1`, `palette_latency.json`,
`palette_latency.ps1`, `palette_trace.csv`, and the capture bundle manifest.

This is the W6 evidence for the item — an operator-throughput number a stranger can re-derive.

## 9. Definition of NOT done

- Mean depth clears 2.0 because the palette's own opening click was not counted. Opening the
  palette is a mouse action; a command reached through it is depth 2 at best.
- The palette dispatches by re-implementing commands instead of invoking `keybindings::Action`, so
  Undo in the palette and Ctrl+Z take different paths and drift.
- The palette indexes all 1,999 manifest ids and returns unwired results indistinguishably, undoing
  `G6.06` inside the new surface.
- Keystroke latency is measured on an empty scene. The exit criterion names the 100x100 grid at seed
  42 because that is where a per-keystroke scan actually hurts.
- The index is rebuilt on every keystroke, so latency is fine at twenty entries and terrible at two
  thousand; the measurement passes because only short queries were typed.
- Opening the palette from a focused text field and dismissing it loses the caret position, so the
  fastest path to a command is now the fastest way to lose your place.
- The palette's empty state is a blank box. That frame will be captured, and it says the surface was
  assembled rather than designed.
- The Critic passes D4 for the palette in isolation but cites the ribbon and the palette offering
  the same command with two different labels. Coherence is judged across the surface, not within
  the new component.
```

---
---

# `docs/PROMPTS/items/G6.08_keyboard-first-coverage.md`

```markdown
---
id: G6.08
title: Keyboard-first coverage of the top 20 commands with visible conflict resolution
workload: W1
workload_secondary: [W6]
phase: G6
depends_on: [G6.04, G6.07, G1.12]
blocks: [G6.14, G6.15]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D4]
capture_recipe: docs/PROMPTS/harness/recipes/G6_ui_sequences.json
artifact: docs/PROMPTS/artifacts/G6.08/keyboard_coverage.json
escalation: >
  If reaching 20/20 keyboard coverage requires binding a chord that collides with a Windows or
  Slint text-editing default (Ctrl+A, Ctrl+C, Ctrl+V, Ctrl+X, Ctrl+Z inside a focused text field),
  STALL rather than shipping a binding that breaks typing. Focus-gated bindings are the correct
  answer and are an approach, not a workaround; if focus gating cannot be made reliable, escalate.
status: DRAFT
notes: >
  Tier M. The binding table already exists; the work is coverage, conflict surfacing, and an
  in-studio editor for the two gaps the audit records.
---

## 1. Objective

Every command in the fixed top-20 list is invocable from the keyboard alone, with no mouse action,
from a cold start. When a user's binding collides with an existing one, the studio says so at the
moment of binding, names the action already holding the chord, and offers a resolution — instead of
silently accepting a shadowed binding.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust; Slint *is* Rust here. Units are meter-native; studs are a display unit only.

**The fixed top-20 command list**, unchanged from `G6.01` and `G6.07`: Undo, Redo, Save, Copy,
Paste, Duplicate, Delete, Group, Ungroup, Select All, Select Tool, Move Tool, Rotate Tool, Scale
Tool, Insert Part, Insert Script, Play Solo, Stop, Toggle Explorer, Toggle Properties.

**What exists today.** `eustress/crates/engine/src/keybindings.rs` is 2,268 lines. `pub enum Action`
at `:7` declares 38 unit variants; the identifier `Action::` appears 239 times in the file. Default
bindings are installed around `:345-430` — note the two comments at `:428` and `:429` recording
bindings that were moved to avoid collisions (`ToggleAssets` moved off `A`, `ToggleCollaboration`
moved off `C`), which tells you collisions are a live problem and were resolved by hand. A test
`no_duplicate_default_bindings` at `:2131` asserts no two **defaults** collide, and a second
assertion at `:2254` requires that the error "must name the conflicting action so the dialog can
show it" — so the error path already carries the information a dialog would need. There is a
`eustress/crates/engine/ui/slint/keybindings.slint` panel (366 lines).

**The recorded gaps, quoted.** `docs/AUDIT/02_STUDIO_ENGINE.md`, Feature 21 (Keybindings system,
50+ actions, state ✅) lists exactly two: *"R21.1 No in-Studio keybinding editor UI. R21.2 Conflict
detection if user binds two actions to same key."* Its Implications line adds: *"the focus-gating
fix in P0 batch is load-bearing."* Those two gaps are this item's subject.

**Focus gating is the hard part.** Slint text inputs consume keystrokes. The tool shortcuts in
`keybindings.rs` are Alt-based specifically to avoid text-input conflicts (see the comment at
`:345`). Any binding you add must not fire while a text field, the script editor, or the command
bar has focus. `SlintUIFocus` is registered as a resource in the UI plugin
(`eustress/crates/engine/src/ui/slint_ui.rs`, in the `init_resource` block beginning at `:1360`) and
`eustress/crates/engine/ui/slint/command_bar.slint` exposes `input-has-focus` as an out property —
those are the mechanisms to build on.

**Your dependency.** Item `G6.07` (already `PASSED`) delivered a command palette that dispatches
through `keybindings::Action`. If a command has no chord, the palette is its keyboard route, and
that counts as keyboard-reachable **only if** the palette itself opens from a chord and the command
can be selected without the mouse. Record which of the twenty are reached by direct chord and which
by palette; both count, but the split is part of the artifact.

**Build reality.** 10-15 minutes per build; one build at a time (shared `target/`; concurrent builds
give LNK2001 or SAC os error 4551); never kill a build mid-compile. Validate with `cargo run`, never
`cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/keybindings.rs` — bindings, conflict detection, and the rebind API
- `eustress/crates/engine/ui/slint/keybindings.slint` — the in-studio editor and the conflict dialog
- `eustress/crates/engine/src/ui/slint_ui.rs` — wiring the editor's callbacks into the drain
- `docs/PROMPTS/artifacts/G6.08/` (create; artifacts)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.07/` — frozen prior evidence
- `eustress/crates/engine/ui/slint/command_palette.slint` and `eustress/crates/engine/src/ui/command_palette.rs` — the palette is frozen; this item uses it, it does not change it
- `eustress/crates/engine/src/ui_trace.rs`, `eustress/crates/engine/src/ui/drain_contract.rs` — frozen instruments
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: shrinking the top-20 list; counting a command as keyboard-reachable
  because a chord exists in the table when pressing it does nothing; testing coverage with no panel
  focused so focus gating is never exercised; and weakening `no_duplicate_default_bindings` to
  admit a new collision. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Coverage is proven by **invoking**, not by inspecting the table. A binding that exists but is
  swallowed by focus gating is not coverage.
- Conflict detection must fire at bind time, in the editor, naming the incumbent action. The
  existing test at `keybindings.rs:2254` already requires the error to name it; surface that string,
  do not invent a second one.
- Do not break typing. A binding that fires inside a focused text field is a regression worse than
  the missing binding it replaced.
- The existing `no_duplicate_default_bindings` test must still pass unmodified.

## 5. Exit criterion

### Criterion
**20 of 20** commands in the fixed list are invocable with the keyboard alone from a cold start,
verified by observing each command's effect; **0** of them fire while a text input has focus; and
attempting to bind an already-taken chord in the in-studio editor produces a message naming the
incumbent action within **1.0 s**.

### Measurement

Step 1 — the existing default-binding test must still pass:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.08_check.ps1

The checker runs the existing `keybindings` suite, prints `EXITCODE=<n>`, and exits with the cargo
test exit code. `no_duplicate_default_bindings` is inside that suite and must pass unmodified.

Step 2 — coverage, driven through the bridge so it is rerunnable. For each of the twenty commands,
the driver synthesises the keyboard route (direct chord, or palette-open chord plus query plus
Enter), then verifies the effect with `get_editor_state`
(`eustress/crates/mcp-server/src/bridge_tools.rs:1163`), `query_entities`, or `git_status`-free
state reads as appropriate to the command:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.08/keyboard_sweep.ps1 -Out docs/PROMPTS/artifacts/G6.08/keyboard_coverage.json

`keyboard_sweep.ps1` is written by this item. It prints one line per command
`<command> route=<chord|palette> fired=<true|false> verified=<true|false>`, then a focus-gating
block that repeats the sweep with a text field focused and asserts `fired=false` for every command,
then `KEYBOARD <verified>/<total> focus_leaks=<n>`, then `KEYBOARD_OK` and exit 0 when
`verified == total AND total == 20 AND focus_leaks == 0`; otherwise `KEYBOARD_FAIL`, exit 1.

Step 3 — conflict surfacing, measured with the frozen `G6.02` instrument armed
(`EUSTRESS_UI_TRACE=1`): open the keybindings editor, attempt to bind an already-taken chord ten
times, and confirm the conflict message appears within 1.0 s each time. Report
`conflict_message_p99_ms` from the trace's `input_to_present_ms` restricted to those events.

Expected output shape (step 2):

    Undo route=chord fired=true verified=true
    Group route=palette fired=true verified=true
    ...
    KEYBOARD 20/20 focus_leaks=0
    KEYBOARD_OK

Pass condition:

    step 1 EXITCODE == 0
    AND verified == total == 20  AND  focus_leaks == 0  AND step 2 exits 0
    AND conflict_message_p99_ms <= 1000

Read the summary line and the exit codes. A populated binding table is not a pass.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)**, floor **8.0**.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_ui_sequences.json`. The relevant frames are the
keybindings editor at rest, mid-rebind, and showing a conflict. The `error_surface` sequence from
`docs/PROMPTS/02_CAPTURE_HARNESS.md` §4.3 (trigger a known-invalid action, error appears, error
dismissed) maps directly onto the conflict flow — use it.

The Critic never sees your self-report and cites or fails. What that implies here: the conflict
message must be legible and actionable in a single still frame — it must name the incumbent action,
show the chord, and offer the resolution — because a message that only makes sense in motion will
not be credited.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — fill binding gaps with focus-gated chords; add conflict detection
                 to the rebind API and surface it in keybindings.slint
   -> if still failing, MANDATORY approach change. Choosing different chords is NOT an approach
      change; moving from per-chord gating to a modal keymap layer (a leader key that suspends
      text input for one chord) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verified count moving by < 1 AND the worst
                  gated dimension moving < 0.5
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: coverage requires a chord that breaks text editing (see front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER coverage to a stated count,
naming which commands remain mouse-only; FUND approach D; DEFER behind a named item; or KILL with
what the phase loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.08/keyboard_coverage.json`

A reader finds: one record per command with its route (direct chord or palette), the chord itself,
whether it fired, how the effect was verified, and the focus-gated result; the verified and leak
counts; the conflict-message latency distribution; and the commit. Archived alongside:
`keyboard_sweep.ps1`, the trace CSV restricted to conflict events, and the capture bundle manifest.

This is the W1 evidence for the item.

## 9. Definition of NOT done

- Coverage is 20/20 because the table has twenty entries. The table is not the behaviour; the sweep
  must invoke and verify.
- A new chord fires while the script editor has focus, so typing a `d` deletes the selection. That
  is a data-loss regression traded for a coverage number.
- Conflict detection exists in the API and is never surfaced in the editor, so the user still binds
  a shadowed chord and discovers it later.
- The conflict message says "that key is taken" without naming the incumbent action, even though
  `keybindings.rs:2254` already requires the error string to carry it.
- `no_duplicate_default_bindings` was edited to accommodate a new default. That test is the only
  existing guard on this surface.
- Commands reached only through the palette are counted without recording the split, so the artifact
  cannot distinguish "has a shortcut" from "is findable".
- The Critic passes the numbers and refuses the item, citing a conflict dialog that offers no
  resolution — only an acknowledgement. That is a legitimate refusal.
```

---
---

# `docs/PROMPTS/items/G6.09_selection-identity-theme-coherence.md`

```markdown
---
id: G6.09
title: One selection identity across both palettes and the 3D viewport
workload: W1
workload_secondary: [W3]
phase: G6
depends_on: [G6.01, G1.12]
blocks: [G6.10, G6.11, G6.12, G6.15]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D2, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G6_theme_coherence.json
artifact: docs/PROMPTS/artifacts/G6.09/theme_coherence.json
escalation: >
  If unifying the selection identity forces any body-text contrast ratio in either palette below
  4.5:1, STALL rather than shipping it. Trading legibility for coherence fails both D2 and the
  accessibility item that depends on this one.
status: DRAFT
notes: >
  Tier M. Token work is cheap to write and expensive to verify — every panel must be looked at in
  both palettes, which is why the capture recipe covers both.
---

## 1. Objective

Selection means one thing everywhere in the Eustress Studio. A selected row in the Explorer, a
selected field in the Properties panel, a selected tool in the ribbon, and a selected part in the
3D viewport all carry the same cyan identity, in both shipped palettes, and no other state in the
interface borrows that colour.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust; Slint *is* Rust here. Units are meter-native; studs are a display unit only.

**The selection identity is cyan `#00bcd4`. Never call it teal.**
`docs/AUDIT/02_STUDIO_ENGINE.md` Feature 22 (Theme system + accent tokens) names its sub-features as
"`theme.slint` design tokens · accent `#00bcd4` cyan · dark mode CSS variables · light mode
incomplete · toggle persists per-user", state 🟡, with forecast items *"R22.1 Light-mode panel
colours unverified"* and an operational implication that *"shipping light mode requires a pass on
every Slint file."*

**Where the identity already holds.** The 3D surface is correct. Verified:
`eustress/crates/engine/src/adornment_renderer.rs:268` comments "accent-cyan #00bcd4" and `:272-273`
use `Color::srgba(0.0, 0.737, 0.831, 0.85)` with a matching emissive;
`eustress/crates/engine/src/lock_tool.rs:43` declares
`const HOVER_UNLOCKED: Color = Color::srgb(0.0, 0.737, 0.831); // ~#00bcd4`.

**Where it does not hold.** `eustress/crates/engine/ui/slint/theme.slint` (1,010 lines) defines a
`ThemeData` struct at `:23` and two palette constants — `preset-classic` at `:135` and
`preset-modern` at `:217` — selected into `Theme.data` at `:304`, with a `modern` boolean at `:310`.
MEASURED values at authoring time:

| Token | `preset-classic` | `preset-modern` |
|---|---|---|
| `selection-background` (`:170` / `:252`) | `#14301c` (green-black) | `#0a84ff26` (blue, 15% alpha) |
| `selection-border` (`:171` / `:253`) | `#3cba54` (green) | `#0a84ff` (blue) |
| `accent-cyan` (`:177` / `:259`) | `#00bcd4` | `#22d3ee` |
| `accent-eustress` (`:179` / `:261`) | `#3cba54` (green) | `#0a84ff` (blue) |
| `border-focus` | `#0a84ff26`-family | `#0a84ff` |

So the panel surface says selection is green in one palette and blue in the other, while the
viewport says cyan in both, and the token literally named `accent-eustress` is neither. A user
selecting a part sees one colour in the tree and a different colour on the object.

**A structural rule from the file itself.** `theme.slint:12-14` records that `ThemeData` exists as
one struct "so Rust can swap the WHOLE palette atomically with a single … struct literal [that]
must fill every field" — meaning a token added to one preset and not the other is a compile error at
best and a silent divergence at worst. Every change must be made in both literals.

**Slint files that consume these tokens.** 60 `.slint` files in
`eustress/crates/engine/ui/slint/`. The panels most likely to hard-code a selection colour rather
than reading a token are `explorer.slint` (430 lines), `properties.slint` (2,859),
`ribbon.slint` (3,389), `main.slint` (4,021), `data_grid.slint`, `services_browser.slint`, and
`timeline_panel.slint`. `theme.slint` also exports shared components — `IconButton` (`:435`),
`SidePanelHeader` (`:613`), `PanelHeader` (`:690`), `PropertyRow` (`:734`), `TreeItem` (`:773`),
`LogEntry` (`:945`) — which are the right place to fix a shared state rather than in each panel.

**Build reality.** 10-15 minutes per build; one build at a time (shared `target/`; concurrent builds
give LNK2001 or SAC os error 4551); never kill a build mid-compile. Validate with `cargo run`, never
`cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/ui/slint/theme.slint` — both preset literals and the shared components
- Any `.slint` file under `eustress/crates/engine/ui/slint/` — **only** to replace a hard-coded colour with a token reference
- `docs/PROMPTS/artifacts/G6.09/` (create; artifacts)
- `docs/PROMPTS/harness/recipes/G6_theme_coherence.json` (create if absent, per `docs/PROMPTS/02_CAPTURE_HARNESS.md` §8)

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.08/` — frozen prior evidence
- `eustress/crates/engine/src/adornment_renderer.rs` and `eustress/crates/engine/src/lock_tool.rs` — **the viewport is already correct**; the panels move to meet it, not the other way round
- Any `.rs` file except where a hard-coded UI colour must be replaced by a token, which must be listed in the artifact
- `eustress/crates/engine/src/ui_trace.rs`, `eustress/crates/engine/src/ui/drain_contract.rs`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: changing the viewport colour to match a panel token; capturing only one
  palette; excluding a panel from the sweep because it is "not really selectable"; and relaxing the
  contrast floor. If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with
  evidence and stop.
- Every token change goes into **both** preset literals. See `theme.slint:12-14`.
- Fix shared components first. Changing `TreeItem` and `PropertyRow` in `theme.slint` should resolve
  most panels at once; a per-panel patch that leaves the shared component wrong will drift back.
- Selection is cyan. Focus, hover, active-tool, error, warning, and success remain distinct states
  with distinct tokens. Collapsing two states into one colour to reduce the palette is a different
  and worse defect.
- Do not restyle. This item unifies an identity; density and typography are `G6.12`.

## 5. Exit criterion

### Criterion
In both palettes: **zero** hard-coded selection colours remain in any `.slint` file (every selection
surface reads a token); the resolved selection token equals `#00bcd4` and its measured CIE ΔE(2000)
against the viewport adornment colour `srgb(0.0, 0.737, 0.831)` is **≤ 2.0**; and every body-text
foreground/background pair in both palettes has a contrast ratio **≥ 4.5:1**.

### Measurement

Step 1 — token audit and contrast computation, static:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.09/audit_theme.ps1 -SlintDir eustress/crates/engine/ui/slint -Theme eustress/crates/engine/ui/slint/theme.slint -Out docs/PROMPTS/artifacts/G6.09/theme_coherence.json

`audit_theme.ps1` is written by this item. It must: scan all 60 `.slint` files for literal colour
values (`#rrggbb`, `#rrggbbaa`) appearing in any property whose name contains `selection`,
`selected`, `highlight`, or which is applied to a selected-state conditional, and report them as
`hardcoded_selection_colors`; parse both preset literals for the five selection-related tokens;
compute ΔE(2000) between the resolved selection token and `#00bcd4`; compute the contrast ratio for
each declared text-on-background pair in both presets; and print `THEME_OK` with exit 0 when
`hardcoded_selection_colors == 0 AND delta_e_classic <= 2.0 AND delta_e_modern <= 2.0 AND
min_contrast_ratio >= 4.5`; otherwise `THEME_FAIL`, exit 1.

Step 2 — visual confirmation, both palettes, one selection made in each of Explorer, Properties,
ribbon, and viewport:

    pwsh -NoProfile -Command "cd 'E:/Workspace/EustressEngine'; cargo run --release -p eustress-engine --bin eustress-engine"

then capture with the recipe named in §6.

Expected output shape (step 1):

    {"hardcoded_selection_colors":0,"selection_token_classic":"#00bcd4","selection_token_modern":"#00bcd4",
     "delta_e_classic":0.0,"delta_e_modern":0.0,"viewport_reference":"#00bcd4",
     "min_contrast_ratio":4.83,"min_contrast_pair":"text-secondary on panel-background (modern)"}
    THEME_OK

Pass condition:

    hardcoded_selection_colors == 0  AND  delta_e_classic <= 2.0  AND  delta_e_modern <= 2.0
    AND  min_contrast_ratio >= 4.5  AND step 1 exits 0

Read the emitted values and the exit code.

## 6. Critic gate

Gated on **D2 (material and lighting realism)** and **D6 (overall coherence)**, floor **8.0 each**.
Either below 8.0 fails the item.

D2 is gated because the viewport adornment is an emissive surface in a lit 3D scene
(`adornment_renderer.rs:272-273` sets both `base_color` and a 3x emissive), and a selection colour
that reads correctly in a flat panel can read blown-out or muddy against the rendered scene. The
same nominal hex in two contexts is not the same perceived colour, and the item is only done when it
reads as one identity, not when two hex strings match.

D6 is gated because unifying one state across a 60-file surface is exactly where a system either
holds together or reveals that its panels never met.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_theme_coherence.json` — both palettes, four
surfaces, three window sizes.

The Critic never sees your self-report, cites or fails, and can refuse a numerically clean result.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — unify the selection tokens in both preset literals and route the
                 shared theme.slint components through them
   -> if still failing, MANDATORY approach change. Nudging a hex value is NOT an approach change;
      introducing a semantic token layer (state names that resolve to palette values, so panels
      reference `state-selected` rather than any colour) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with hardcoded_selection_colors dropping by < 1 AND
                  the worst gated dimension moving < 0.5
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: any body-text contrast ratio falls below 4.5:1 (see front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the ΔE ceiling to a stated
value with the consequence; FUND approach D; DEFER behind a named item; or KILL with what the phase
loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.09/theme_coherence.json`

A reader finds: the before and after values of the five selection-related tokens in both presets
with their `theme.slint` line numbers; the list of hard-coded selection colours found and the file
and line each was replaced at; the ΔE(2000) figures against the viewport reference; the full
contrast table for both presets with the worst pair named; and the commit. Archived alongside:
`audit_theme.ps1` and the capture bundle manifest.

This is the W1 evidence for the item, and `G6.11` (accessibility) and `G6.12` (density) both build
on its contrast table.

## 9. Definition of NOT done

- The tokens match and the panels still look green, because a panel hard-codes its own selection
  colour and the audit's property-name filter missed it.
- The viewport was changed to match the panels. The viewport was already right; the audit's own
  reference is `adornment_renderer.rs:272`.
- A token was added to `preset-classic` and not to `preset-modern`, so one palette silently falls
  back to a stale value.
- Selection and focus now share a colour, so a focused-but-unselected field is indistinguishable
  from a selected one. Reducing the palette is not the same as unifying an identity.
- The hex values match but the emissive multiplier makes the viewport adornment read as a different
  colour under scene lighting. The Critic scores the perceived identity, not the literal.
- Contrast passes on the average pair and fails on the worst one. The floor is on the minimum.
- The item quietly restyles panel spacing or type sizes while in the files. That is `G6.12`, and
  mixing it here makes the blind comparison in `G6.15` unattributable.
- The word "teal" appears anywhere in the artifact or the code comments. The selection identity is
  cyan.
```

---
---

# `docs/PROMPTS/items/G6.10_empty-and-error-states.md`

```markdown
---
id: G6.10
title: Designed empty states and error states in the eight primary panels
workload: W1
workload_secondary: [W4]
phase: G6
depends_on: [G6.01, G6.04, G6.09, G1.12]
blocks: [G6.14, G6.15]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D4, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G6_ui_sequences.json
artifact: docs/PROMPTS/artifacts/G6.10/state_coverage.json
escalation: >
  If driving a panel into its error state requires fabricating a failure the engine cannot actually
  produce — i.e. the error path does not exist and would have to be invented — STALL. Inventing an
  error to have something to render is a fake artifact and fails the pack's honesty rule.
status: DRAFT
notes: >
  Tier M. Sixteen states across eight panels, but they share one component vocabulary, so the
  design cost is paid once and applied eight times.
---

## 1. Objective

Every primary panel in the Eustress Studio tells the user something useful when it has nothing to
show and when something has gone wrong. For eight named panels, both the empty state and the error
state are designed surfaces carrying a cause, a consequence, and a next action — never a blank
rectangle, never a raw error string, never a spinner that never resolves.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` compiles to
Rust. Units are meter-native; studs are a display unit only.

**The eight panels, with their verified files.** All under
`eustress/crates/engine/ui/slint/`, line counts MEASURED at authoring time:

| Panel | File | Lines |
|---|---|---|
| Explorer | `explorer.slint` | 430 |
| Properties | `properties.slint` | 2,859 |
| Output | `output.slint` | 680 |
| Problems | `problems_panel.slint` | (present) |
| Timeline | `timeline_panel.slint` | 455 |
| History | `history_panel.slint` | (present) |
| Data Grid | `data_grid.slint` | (present) |
| Services Browser | `services_browser.slint` | 633 |

**Why these eight.** They are the panels a professional user opens in the first session, and they
are the panels most likely to be legitimately empty at that moment — nothing selected, no output
yet, no problems, no recorded timeline, no history, no dataset connected, no service inspected. An
empty panel at first run is the single most common frame a new user sees, and a blank rectangle in
that frame is the fastest way to communicate "experimental".

**What a designed empty state contains, for the purposes of this item.** Three things: a statement
of what the panel shows, a statement of why it is currently empty, and a single actionable next
step (a control the user can press, or the exact gesture that populates it). A panel that shows only
"No items" satisfies none of the three.

**What a designed error state contains.** Four things: what failed, in the user's vocabulary; what
the consequence is; what to do about it; and a way to see the underlying detail without leaving the
panel. A raw `Err` string rendered into a label satisfies none of the four.

**What already exists to build on.** `eustress/crates/engine/ui/slint/theme.slint` (1,010 lines)
exports shared components including `PanelHeader` (`:690`), `SidePanelHeader` (`:613`),
`TreeItem` (`:773`), `PropertyRow` (`:734`), and `LogEntry` (`:945`) — the right home for a shared
empty/error component so that eight panels do not grow eight vocabularies. The notification system
is `eustress/crates/engine/ui/slint/notifications.slint` (with a `NotificationData` struct carrying
`level`, `title`, `message`, `duration-ms`, `dismissible`) plus
`eustress/crates/engine/src/ui/notifications.rs` (228 lines) — that is for transient events and is
**not** a substitute for a panel's own resting state.

**Your dependency.** Item `G6.09` (already `PASSED`) unified the selection identity and produced a
contrast table for both palettes at `docs/PROMPTS/artifacts/G6.09/theme_coherence.json`. Your empty
and error states must hold the contrast floor of 4.5:1 established there, in both palettes.

**Build reality.** 10-15 minutes per build; one build at a time (shared `target/`; concurrent builds
give LNK2001 or SAC os error 4551); never kill a build mid-compile. Validate with `cargo run`, never
`cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- The eight `.slint` panel files named above
- `eustress/crates/engine/ui/slint/theme.slint` — to add the shared empty-state and error-state components
- `eustress/crates/engine/src/ui/slint_ui.rs` — only to push the state discriminant and its message fields into Slint
- `docs/PROMPTS/artifacts/G6.10/` (create; artifacts)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.09/` — frozen prior evidence
- `eustress/crates/engine/ui/slint/notifications.slint` and `eustress/crates/engine/src/ui/notifications.rs` — toasts are transient events, a different concern
- `eustress/crates/engine/src/ui_trace.rs`, `eustress/crates/engine/src/ui/drain_contract.rs`
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: reducing the panel set below eight; counting a panel as covered because
  its empty state exists in one palette; substituting a toast for a resting state; and — most
  importantly — fabricating an error condition the engine cannot produce, so that a designed error
  frame can be captured. Every error state must be reachable by a real failure, and the artifact
  must record the exact trigger. If a panel has no real error path, say so and record it as
  `no_real_error_path` rather than inventing one.
- One vocabulary, eight applications. Add the shared component to `theme.slint`; do not write eight
  bespoke empty states.
- The empty state must not be a disabled-looking panel. Empty is a normal, expected condition;
  it should read as ready, not as broken.
- Hold the contrast floor from `G6.09` (4.5:1) in both palettes, including for muted explanatory
  text, which is where empty states habitually fail.
- Do not restyle the populated state. Density and typography are `G6.12`.

## 5. Exit criterion

### Criterion
**8 of 8** panels render a designed empty state and **8 of 8** render a designed error state, each
verified by driving the panel into that state and confirming three required elements are present in
the empty state and four in the error state; **zero** panels render a blank region larger than
20% of their client area in either state; and every text pair in the new states holds contrast
**≥ 4.5:1** in both palettes.

### Measurement

Step 1 — drive each panel into each state and dump its rendered state descriptor. This item adds an
`EUSTRESS_PANEL_STATE_DUMP=<path>` knob that, for the focused panel, writes a JSON line
`{panel, state, has_title, has_reason, has_action, has_detail_affordance, blank_fraction}`:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.10_check.ps1

The checker points `EUSTRESS_PANEL_STATE_DUMP` at
`docs/PROMPTS/artifacts/G6.10/panel_states.jsonl`, launches the studio, and exits with the studio's
own exit code. The coverage gate is step 2.

Drive each of the eight panels into empty and into error using the triggers recorded in
`docs/PROMPTS/artifacts/G6.10/triggers.md` (written by this item; one real, reproducible trigger per
panel per state).

Step 2 — gate:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.10/check_states.ps1 -States docs/PROMPTS/artifacts/G6.10/panel_states.jsonl -Contrast docs/PROMPTS/artifacts/G6.09/theme_coherence.json -Out docs/PROMPTS/artifacts/G6.10/state_coverage.json

`check_states.ps1` is written by this item. It requires, for every panel: an `empty` record with
`has_title AND has_reason AND has_action` true, an `error` record with all four true,
`blank_fraction <= 0.20` in both, and both palettes present. It prints
`STATES <covered>/<required>` then `STATES_OK` and exit 0 when `covered == required == 16 AND
min_contrast_ratio >= 4.5`; otherwise `STATES_FAIL`, exit 1.

Expected output shape:

    {"panels":8,"empty_covered":8,"error_covered":8,"max_blank_fraction":0.14,"min_contrast_ratio":4.71,
     "no_real_error_path":[]}
    STATES 16/16
    STATES_OK

Pass condition:

    empty_covered == 8  AND  error_covered == 8  AND  max_blank_fraction <= 0.20
    AND  min_contrast_ratio >= 4.5  AND step 2 exits 0

Read the summary line and the exit code.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)** and **D6 (overall coherence)**, floor **8.0 each**.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_ui_sequences.json`. The `error_surface` sequence
from `docs/PROMPTS/02_CAPTURE_HARNESS.md` §4.3 — trigger a known-invalid action, error appears,
error dismissed — is exactly the flow to capture for the error half. Capture both palettes.

D6 is gated because sixteen states are where a design system either proves it exists or reveals that
each panel was decorated separately. If the Explorer's empty state and the Data Grid's empty state
do not obviously come from the same hand, the item has not landed.

The Critic never sees your self-report, cites or fails, and can refuse a numerically clean result.
Write the copy carefully: a first-time professional user reads these sentences before they read
anything else in the product.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — one shared EmptyState/ErrorState component in theme.slint, applied
                 to eight panels, driven by a state discriminant pushed from Rust
   -> if still failing, MANDATORY approach change. Rewriting the copy is NOT an approach change;
      moving from a panel-local state discriminant to a single studio-wide panel-state model that
      every panel subscribes to is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the covered count moving by < 2 AND the worst
                  gated dimension moving < 0.5
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a panel has no reachable real error path (see front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the panel set to a stated
subset, naming which panels stay blank; FUND approach D; DEFER behind a named item; or KILL with
what the phase loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.10/state_coverage.json`

A reader finds: one record per panel per state with the four presence flags, the blank fraction, the
palette, and the exact trigger used; the covered counts; the contrast minimum; and the commit.
Archived alongside: `triggers.md` (the reproducible trigger per panel per state),
`panel_states.jsonl`, `check_states.ps1`, and the capture bundle manifest.

This is the W1 evidence for the item.

## 9. Definition of NOT done

- A panel's empty state says "No items" and nothing else. That is a label, not a state.
- An error state renders the raw `Err` text. The user's vocabulary and the type's `Display` impl are
  not the same language.
- An error frame was captured by fabricating a failure the engine cannot actually produce. That is a
  fake artifact, and it fails the item outright even if the frame is beautiful.
- Coverage is claimed in one palette. `theme.slint` swaps the whole `ThemeData` struct; a state
  verified only in `preset-classic` is unverified.
- The empty state is styled like a disabled panel — greyed, dimmed, inert — teaching the user that an
  empty Problems panel means something is wrong.
- The states are eight bespoke layouts. Sixteen frames from eight different design vocabularies is
  a D6 failure even when each frame is individually fine.
- The explanatory text is muted grey at 3.9:1 contrast, below the floor `G6.09` established, because
  muted text is where empty states habitually fail accessibility.
- The Critic passes the counts and refuses the item, citing an error state whose "next action" does
  not actually resolve the error. That is a legitimate refusal.
```

---
---

# `docs/PROMPTS/items/G6.11_accessibility-annotations-and-focus-order.md`

```markdown
---
id: G6.11
title: Accessibility annotations and deterministic focus order across ten panels
workload: W1
workload_secondary: [W5]
phase: G6
depends_on: [G6.01, G6.04, G6.09, G1.12]
blocks: [G6.15]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D4]
capture_recipe: docs/PROMPTS/harness/recipes/G6_ui_sequences.json
artifact: docs/PROMPTS/artifacts/G6.11/a11y_coverage.json
escalation: >
  If the pinned Slint version does not expose an accessible property required for a control class
  (for example a tree item's expanded state), STALL and name the missing property rather than
  faking it with a label string. A label that encodes state is not an accessible state and will
  mislead assistive technology.
status: DRAFT
notes: >
  Tier L because the annotation pass touches ten files with thousands of lines between them and
  focus order must be verified interactively, not just declared.
---

## 1. Objective

The interactive surface of the Eustress Studio's ten primary panels is annotated for assistive
technology, and keyboard focus moves through each panel in a deterministic, visible, and sensible
order. A user navigating by keyboard alone can always tell where focus is, and a screen reader
receives a role and a name for every control they can reach.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust; Slint *is* Rust here. Units are meter-native; studs are a display unit only.

**The measured starting point.** MEASURED at authoring time: of the **60** `.slint` files in
`eustress/crates/engine/ui/slint/`, exactly **1** contains the string `accessible-role` —
`script_editor.slint`, with a single occurrence. Everything else in the studio is unannotated.

**What the audit records.** `docs/AUDIT/02_STUDIO_ENGINE.md` Feature 23 (Accessibility manifest),
state 🟡, effort L, risk Med: *"Design-time manifest exists; runtime bindings not shipped. WCAG AA
compliance is a launch gate for government/enterprise."* Its forecast items are *"R23.1 Manifest
bloat at 100+ components. R23.2 No CI accessibility lint. R23.3 Color-blind modes need theme
variants."* Its mitigation is *"M23.1 Ship ARIA bindings on top panels first; defer rest."* This
item **is** M23.1: top panels first, explicitly not the whole surface.

**Scope discipline, stated up front.** Full WCAG AA is listed as a non-goal of the whole program in
`docs/PROMPTS/00_MASTER_PROTOCOL.md` §1.2. This item does not claim WCAG AA compliance, must not
claim it in the artifact, and must not be described as achieving it. It delivers annotation coverage
and focus determinism on ten panels — a measurable, honest subset.

**The ten panels.** Files under `eustress/crates/engine/ui/slint/`, line counts MEASURED:
`explorer.slint` (430), `properties.slint` (2,859), `ribbon.slint` (3,389), `output.slint` (680),
`problems_panel.slint`, `timeline_panel.slint` (455), `history_panel.slint`,
`services_browser.slint` (633), `toolbox.slint`, `settings.slint` (990).

**Shared components are the lever.** `eustress/crates/engine/ui/slint/theme.slint` (1,010 lines)
exports `IconButton` (`:435`), `ToolbarSeparator` (`:603`), `SidePanelHeader` (`:613`),
`PanelHeader` (`:690`), `PropertyRow` (`:734`), `TreeItem` (`:773`), and `LogEntry` (`:945`).
Annotating those seven correctly covers a large fraction of the ten panels' controls in one place —
which is also the answer to the audit's R23.1 bloat forecast.

**Your dependency.** Item `G6.09` (already `PASSED`) unified the selection identity and produced the
contrast table at `docs/PROMPTS/artifacts/G6.09/theme_coherence.json` with a 4.5:1 floor in both
palettes. Focus indication in this item must be visible independently of colour — a focus ring that
is only a hue change fails for a user who cannot distinguish that hue.

**Keyboard context.** Item `G6.08` may or may not have run before this one (it is not a declared
dependency). Do not assume any particular chord exists. Focus movement in this item means the
platform Tab / Shift+Tab / arrow conventions, not studio command shortcuts.

**Build reality.** 10-15 minutes per build; one build at a time (shared `target/`; concurrent builds
give LNK2001 or SAC os error 4551); never kill a build mid-compile. Validate with `cargo run`, never
`cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- The ten `.slint` panel files named above
- `eustress/crates/engine/ui/slint/theme.slint` — annotations on the seven shared components, plus a focus-indicator token in **both** presets
- `eustress/crates/engine/src/ui/slint_ui.rs` — only to supply accessible names that must come from data (entity names, property names, log levels)
- `docs/PROMPTS/artifacts/G6.11/` (create; artifacts)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.10/` — frozen prior evidence
- The other 50 `.slint` files — this item is deliberately the top-ten subset
- `eustress/crates/engine/src/ui_trace.rs`, `eustress/crates/engine/src/ui/drain_contract.rs`
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: reducing the panel set below ten; redefining "interactive element" to
  exclude a control class that is hard to annotate; annotating a container instead of its children
  and counting the children as covered; and encoding state into a label string because the pinned
  Slint version lacks the state property. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- An interactive element, for counting purposes, is any element that declares a `clicked`,
  `pressed`, `toggled`, `edited`, `changed`, or `activated` callback, or is a `TouchArea`,
  `LineEdit`, `TextInput`, `Button`, `CheckBox`, `ComboBox`, `Slider`, or `ScrollView` handle.
  Define the set once, in the audit script, and apply it uniformly.
- Annotate the shared components first. See R23.1.
- The focus indicator must be visible without relying on hue alone — outline weight, offset, or a
  second visual channel in addition to colour.
- Do not claim WCAG AA. Say what was measured.

## 5. Exit criterion

### Criterion
Across the ten named panels, **≥ 95%** of interactive elements carry both `accessible-role` and a
non-empty `accessible-label`; **10 of 10** panels have a focus order that visits every focusable
element exactly once with no dead end and no trap; and the focus indicator is measurably visible in
both palettes at a luminance contrast **≥ 3.0:1** against the adjacent surface.

### Measurement

Step 1 — static annotation coverage:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.11/audit_a11y.ps1 -Panels docs/PROMPTS/artifacts/G6.11/panels.txt -SlintDir eustress/crates/engine/ui/slint -Out docs/PROMPTS/artifacts/G6.11/a11y_coverage.json

`audit_a11y.ps1` is written by this item, along with `panels.txt` listing the ten file names, one
per line. The script enumerates interactive elements by the definition in §4, counts how many carry
both `accessible-role` and a non-empty `accessible-label`, resolves annotations inherited from the
seven shared `theme.slint` components, and reports `elements_total`, `elements_annotated`,
`coverage_pct`, and an `unannotated` list of `file:line`.

Step 2 — focus-order traversal. This item adds an `EUSTRESS_FOCUS_TRACE=<path>` knob that logs one
line per focus change (`{panel, element_id, ordinal, timestamp_ms}`):

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.11_check.ps1

The checker points `EUSTRESS_FOCUS_TRACE` at
`docs/PROMPTS/artifacts/G6.11/focus_trace.jsonl`, launches the studio, and exits with the studio's
own exit code. The a11y gate is step 3.

For each of the ten panels, press Tab until focus returns to the first element, then Shift+Tab back.

Step 3 — gate:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.11/check_a11y.ps1 -Coverage docs/PROMPTS/artifacts/G6.11/a11y_coverage.json -Focus docs/PROMPTS/artifacts/G6.11/focus_trace.jsonl -Out docs/PROMPTS/artifacts/G6.11/a11y_coverage.json

`check_a11y.ps1` asserts `coverage_pct >= 95.0`; for each panel that the Tab cycle visited every
focusable element exactly once and returned to the start (no repeats, no omissions, no trap where
Tab does not advance); and that the recorded focus-indicator contrast is `>= 3.0`. It prints
`A11Y <panels_ok>/10 coverage=<pct>%` then `A11Y_OK` and exit 0, or `A11Y_FAIL` and exit 1.

Expected output shape:

    {"elements_total":1284,"elements_annotated":1236,"coverage_pct":96.3,"unannotated":["settings.slint:412", ...],
     "focus_indicator_contrast_classic":3.4,"focus_indicator_contrast_modern":3.9}
    A11Y 10/10 coverage=96.3%
    A11Y_OK

Pass condition:

    coverage_pct >= 95.0  AND  panels_ok == 10  AND  both focus_indicator_contrast values >= 3.0
    AND step 3 exits 0

Read the emitted values and the exit code.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)**, floor **8.0**.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_ui_sequences.json`. Capture the focus indicator at
several positions in each of at least four panels, in both palettes, at the three declared window
sizes — the indicator's visibility at 3840x2160 is where a hairline outline disappears.

The Critic never sees your self-report and cites or fails. What follows: the focus indicator is the
only part of this item that is visible at all, so it carries the entire D4 score. It must read as a
designed part of the system, not as a browser default rectangle.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — annotate the seven shared theme.slint components, then sweep the
                 ten panels for elements not covered by inheritance
   -> if still failing, MANDATORY approach change. Annotating twenty more elements by hand is NOT
      an approach change; introducing an annotated wrapper component that every interactive
      element in the ten panels is migrated onto is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with coverage_pct moving < 2 points AND panels_ok
                  unchanged
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the pinned Slint version lacks a required accessible property (front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER coverage to a stated percentage
with the consequence named; FUND approach D; DEFER behind a named item; or KILL with what the phase
loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.11/a11y_coverage.json`

A reader finds: total and annotated interactive-element counts, the coverage percentage, the
`file:line` list of every element still unannotated, per-panel focus-traversal results (visited,
repeats, omissions, traps), the focus-indicator contrast in both palettes, an explicit statement
that this item measured annotation coverage and focus determinism and **did not** assess WCAG AA
conformance, and the commit. Archived alongside: `panels.txt`, `audit_a11y.ps1`, `check_a11y.ps1`,
`focus_trace.jsonl`, and the capture bundle manifest.

This is the W1 evidence for the item, and the W5 evidence that the extension surface is
navigable by someone who did not build it.

## 9. Definition of NOT done

- Coverage passes because containers were annotated and their children counted as inheriting when
  they do not. Verify inheritance in the rendered tree, not by assumption.
- A control's state is encoded in its label — "Explorer (expanded)" — because the state property
  was unavailable. Assistive technology reads that as a name, not a state, and it goes stale.
- Focus order is declared in the source but was never traversed. The Tab sweep is the measurement;
  the declaration is the intention.
- One panel contains a focus trap where Tab does not advance, and it was not found because the
  sweep stopped after twenty presses instead of after returning to the start.
- The focus indicator is a 1 px hue change that is invisible at 3840x2160 and indistinguishable for
  a user who cannot separate that hue from the background.
- The artifact claims WCAG AA conformance. It was not measured, it is a stated program non-goal,
  and claiming it in a document a buyer may read is a compliance liability.
- Coverage is 95% in `preset-classic` and untested in `preset-modern`, so the focus indicator
  disappears on the palette half the users prefer.
```

---
---

# `docs/PROMPTS/items/G6.12_density-and-typographic-hierarchy.md`

```markdown
---
id: G6.12
title: Information density and typographic hierarchy in Explorer, Properties, and Output
workload: W1
workload_secondary: []
phase: G6
depends_on: [G6.09, G6.10, G1.12]
blocks: [G6.15]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D4, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G6_density.json
artifact: docs/PROMPTS/artifacts/G6.12/density_measurement.json
escalation: >
  If reaching the row-count target drives any body text below 12 physical pixels at 1920x1080 or
  below the 4.5:1 contrast floor established by G6.09, STALL. Density bought with legibility is not
  density; it is a smaller version of a worse product.
status: DRAFT
notes: >
  Tier L. The change is small in lines and large in consequence — every panel using the shared
  row components moves at once, so verification spans the whole studio.
---

## 1. Objective

The three panels a professional user lives in — Explorer, Properties, and Output — show
substantially more information per screen than they do today, at a legible type size, with a type
scale small enough to read as a system rather than an accumulation. Measured at 1920x1080, visible
rows per panel increase by at least 25% over the baseline while no body text falls below 12 physical
pixels and no contrast pair falls below 4.5:1.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust; Slint *is* Rust here. Units are meter-native; studs are a display unit only — a
density pass must not change which unit a field displays.

**The three panels.** `eustress/crates/engine/ui/slint/explorer.slint` (430 lines),
`eustress/crates/engine/ui/slint/properties.slint` (2,859 lines), and
`eustress/crates/engine/ui/slint/output.slint` (680 lines). Line counts MEASURED at authoring time.

**The shared row vocabulary.** `eustress/crates/engine/ui/slint/theme.slint` (1,010 lines) exports
the components these panels are built from: `TreeItem` (`:773`) for Explorer rows, `PropertyRow`
(`:734`) for Properties rows, `LogEntry` (`:945`) for Output rows, plus `PanelHeader` (`:690`),
`SidePanelHeader` (`:613`), and `IconButton` (`:435`). Changing row height or type size in those
components changes every panel that uses them, which is both the leverage and the risk in this item.

**The theme structure you must respect.** `theme.slint` defines a `ThemeData` struct at `:23` and
two full palette literals — `preset-classic` at `:135` and `preset-modern` at `:217` — selected into
`Theme.data` at `:304`. The comment at `:12-14` records that Rust swaps the whole struct atomically
and every field must be filled in both literals. Spacing and radius tokens (`radius-xl` and
siblings, around `:215`) live in the same struct, so a density token added to one preset must be
added to the other.

**Your dependencies.** Item `G6.09` (already `PASSED`) unified selection identity and produced the
contrast table at `docs/PROMPTS/artifacts/G6.09/theme_coherence.json` with a 4.5:1 floor in both
palettes — that floor is inherited here and is not negotiable. Item `G6.10` (already `PASSED`) added
empty and error states to eight panels including these three; a density change must not push those
states below their own contrast floor or clip their copy.

**The baseline.** `docs/PROMPTS/artifacts/G6.01/ux_baseline_census.json` records the panel
inventory. It does **not** record rows-per-viewport; this item establishes that number for the three
panels as part of its own measurement, capturing the before-value from the current build before
making any change. Capture the before-value first, in the same session, with the same script — a
before-value measured differently from the after-value is not a comparison.

**Build reality.** 10-15 minutes per build; one build at a time (shared `target/`; concurrent builds
give LNK2001 or SAC os error 4551); never kill a build mid-compile. Validate with `cargo run`, never
`cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/ui/slint/theme.slint` — the shared row components and the spacing/type tokens in **both** presets
- `eustress/crates/engine/ui/slint/explorer.slint`
- `eustress/crates/engine/ui/slint/properties.slint`
- `eustress/crates/engine/ui/slint/output.slint`
- `docs/PROMPTS/artifacts/G6.12/` (create; artifacts)
- `docs/PROMPTS/harness/recipes/G6_density.json` (create if absent, per `docs/PROMPTS/02_CAPTURE_HARNESS.md` §8)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.11/` — frozen prior evidence
- Any `.rs` file — this item changes presentation, not data
- The selection tokens unified by `G6.09` — density does not get to renegotiate the identity
- The empty and error state components added by `G6.10` — you may verify them, not restyle them
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: measuring rows at a larger window than 1920x1080; counting a partially
  visible clipped row as visible; reaching the target by removing a column or an icon rather than by
  tightening the layout; measuring before-and-after with different scripts; and relaxing the type
  floor or the contrast floor. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Density comes from vertical rhythm, not from shrinking type. The type floor is 12 physical pixels
  at 1920x1080 and it is a floor, not a target.
- Reduce the type scale to at most **5** distinct sizes across the three panels. A studio with nine
  type sizes is not a hierarchy; it is a history.
- Everything changed goes in both palette literals. See `theme.slint:12-14`.
- Do not remove information to make room. A denser panel showing less is a regression with a good
  number.
- Verify the whole studio, not the three panels. `TreeItem`, `PropertyRow`, and `LogEntry` are used
  elsewhere; a row-height change ripples.

## 5. Exit criterion

### Criterion
At 1920x1080, in both palettes, visible full rows increase by **≥ 25%** over the same-session
baseline in each of the three panels; the number of distinct type sizes across the three panels is
**≤ 5**; the smallest body text is **≥ 12** physical pixels; and the minimum contrast ratio across
the three panels remains **≥ 4.5:1**.

### Measurement

Step 1 — capture the before-state, on the current build, before any edit. This item adds an
`EUSTRESS_LAYOUT_DUMP=<path>` knob that, for a named panel, writes
`{panel, palette, window_w, window_h, visible_full_rows, row_height_px, type_sizes_px[], min_contrast}`:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.12_check.ps1

The checker points `EUSTRESS_LAYOUT_DUMP` at its `-Out` path, defaulting to
`docs/PROMPTS/artifacts/G6.12/layout_before.jsonl`, launches the studio, and exits with the studio's
own exit code.

Open Explorer, Properties, and Output in turn, in both palettes, at exactly 1920x1080, with a Space
containing at least 200 entities so the panels are genuinely full:

    cargo run --release -p eustress-engine --bin generate-benchmark-map -- --grid-size 15 --spacing 4.0 --seed 42

Step 2 — after the change, rerun the **same** checker with the after path. Using one script for both
halves is what makes the comparison legitimate:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.12_check.ps1 -Out docs/PROMPTS/artifacts/G6.12/layout_after.jsonl

Step 3 — gate:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.12/check_density.ps1 -Before docs/PROMPTS/artifacts/G6.12/layout_before.jsonl -After docs/PROMPTS/artifacts/G6.12/layout_after.jsonl -Out docs/PROMPTS/artifacts/G6.12/density_measurement.json

`check_density.ps1` is written by this item. It computes per panel per palette
`row_gain_pct = (after.visible_full_rows / before.visible_full_rows - 1) * 100`, the union of
`type_sizes_px` across the three panels, the minimum body type size, and the minimum contrast. It
prints `DENSITY_OK` and exit 0 when every `row_gain_pct >= 25.0 AND distinct_type_sizes <= 5 AND
min_type_px >= 12 AND min_contrast >= 4.5`; otherwise `DENSITY_FAIL`, exit 1.

Expected output shape:

    {"explorer":{"classic":{"before":24,"after":32,"gain_pct":33.3},"modern":{"before":24,"after":32,"gain_pct":33.3}},
     "properties":{"classic":{"before":11,"after":15,"gain_pct":36.4},"modern":{"before":11,"after":15,"gain_pct":36.4}},
     "output":{"classic":{"before":18,"after":23,"gain_pct":27.8},"modern":{"before":18,"after":23,"gain_pct":27.8}},
     "distinct_type_sizes":4,"min_type_px":12,"min_contrast":4.62}
    DENSITY_OK

Pass condition:

    every row_gain_pct >= 25.0  AND  distinct_type_sizes <= 5  AND  min_type_px >= 12
    AND  min_contrast >= 4.5  AND step 3 exits 0

Read the emitted values and the exit code.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)** and **D6 (overall coherence)**, floor **8.0 each**.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_density.json` — the three panels, both palettes,
three window sizes, with the panels genuinely full.

D6 is gated because this item changes shared components. A density pass that makes Explorer,
Properties, and Output beautiful while leaving the Services Browser and the Data Grid on the old
rhythm produces a studio with two metrics, which reads worse than one consistent loose one.

The Critic never sees your self-report and cites or fails. Density is judged as legibility per
screen, not as rows per screen: a panel that fits more and is harder to scan will be cited against
you with a frame reference.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — tighten vertical rhythm in the shared row components; collapse the
                 type scale; reclaim padding from panel chrome
   -> if still failing, MANDATORY approach change. Shaving two more pixels off row height is NOT
      an approach change; moving Properties from one-row-per-field to a two-column grid with
      grouped sections is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst panel's row_gain_pct moving < 5% AND
                  the worst gated dimension moving < 0.5
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: body text would fall below 12 px or contrast below 4.5:1 (front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the row-gain floor to a stated
percentage; FUND approach D; DEFER behind a named item; or KILL with what the phase loses.
Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.12/density_measurement.json`

A reader finds: per panel per palette, the before and after visible-row counts and the percentage
gain; the row height before and after; the union of type sizes before and after with the count; the
minimum body type size in physical pixels; the minimum contrast ratio and the pair that produced it;
the window size and the scene used; and the commit. Archived alongside: `layout_before.jsonl`,
`layout_after.jsonl`, `check_density.ps1`, and the capture bundle manifest.

This is the W1 evidence for the item.

## 9. Definition of NOT done

- The row gain is reached by shrinking body text to 11 px. The floor is 12 px at 1920x1080 and
  exists because this is a tool people read for eight hours.
- Rows increased because a column was removed. The panel now fits more of less.
- The before-value was measured on a different scene or a different window size from the
  after-value, so the percentage is not a comparison.
- Partially visible clipped rows were counted as visible, inflating the gain by one row per panel.
- The type scale is 5 sizes in the three panels and 9 across the studio, because the shared
  components were bypassed with local overrides.
- A spacing token was added to `preset-classic` and not to `preset-modern`, so one palette is dense
  and the other is not.
- `TreeItem` got shorter and the Services Browser, which also uses it, now clips its own rows —
  a defect introduced outside the three panels being measured.
- The empty and error states from `G6.10` now clip their explanatory copy at the tighter row
  rhythm. That is a regression against a passed item.
- The Critic passes the numbers and refuses the item, citing that the denser Properties panel is
  harder to scan because the hierarchy flattened along with the type scale. That is a legitimate
  refusal.
```

---
---

# `docs/PROMPTS/items/G6.13_split-slint-ui.md`

```markdown
---
id: G6.13
title: Split slint_ui.rs — no studio UI file over 3,000 lines, with behaviour held constant
workload: W6
workload_secondary: [W3]
phase: G6
depends_on: [G6.03, G6.04]
blocks: [G6.15, G7.42]
tier: XL
token_envelope: 1200000
wallclock_envelope: 5d
max_builds: 20
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G6.13/split_report.json
escalation: >
  If splitting the drain requires changing which Bevy system consumes the SlintAction queue — i.e.
  more than one system would drain it — STALL. Multiple drains is precisely the duplicate-state
  architecture that produced this pack's worst historical defect, and re-creating it to reduce a
  line count is a net loss.
status: DRAFT
notes: >
  Tier XL: 23,103 lines to redistribute, and the only acceptable outcome is zero behaviour change,
  which means every build must re-run two frozen instruments. 20 builds is roughly 4 hours of pure
  compile; design so one build validates a whole module group.
---

## 1. Objective

No file under `eustress/crates/engine/src/ui/` exceeds 3,000 lines. The `SlintAction` drain is
organised into per-domain modules, still consumed by exactly one Bevy system, with the drain-contract
regression test passing unchanged and the measured UI latency unchanged within ±5% of the value
`G6.04` established.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust; Slint *is* Rust here. Units are meter-native; studs are a display unit only.

**The problem, measured.** `eustress/crates/engine/src/ui/slint_ui.rs` is **23,103 lines** — the
largest file in the workspace. The whole of `eustress/crates/engine/src/ui/` is 37,533 lines across
27 files, so one file is 62% of the studio's Rust UI layer. The next largest are
`file_event_handler.rs` (1,964), `world_view.rs` (1,928), and `mod.rs` (1,032). It contains
**zero** `#[test]` functions.

Everything funnels through it: the `SlintAction` enum (`:250`), the `SlintSystems` set (`:238`), the
plugin registration block (from `:1360`), the input forwarders (`:3020`), the three drain parameter
bundles (`:3449`, `:3549`, `:3941`), the drain itself (`:4965`), and roughly twenty
`sync_*_to_slint` systems (`:1462`-`:1487`). Any UI change requires reading a file that does not fit
in a working context, which is the direct cause of the historical defects this pack has been
closing.

**What must not change.** Exactly one system drains the queue. `drain_slint_actions`
(`:4965`), registered `.in_set(SlintSystems::Drain)` at `:1428`, is the sole consumer, and roughly
thirty other systems across the crate order themselves against that set — see
`eustress/crates/engine/src/main.rs:589`, `play_mode.rs:1743`, `road_tool.rs:89`, and
`script_plugin_host.rs:148-149`. Splitting the *file* is the goal; splitting the *drain* is not.

**The failure mode this refactor invites.** Bevy skips a system whose parameters fail validation.
Moving code between modules is exactly how a resource registration gets orphaned, and if
`drain_slint_actions` is skipped, **every** UI click in the studio dies while Bevy logs one
`failed validation` WARN. Two source comments record this having shipped twice —
`slint_ui.rs:1373-1378` (the `LabelEditState` case) and `slint_ui.rs:1379-1383` (the
`TerrainVisibility` case, "the theme/mode clicks do nothing bug"). Both were caused by a second,
parallel registration site. Do not create one.

**Your two frozen instruments.**

- Item `G6.03` (already `PASSED`) delivered
  `cargo test -p eustress-engine --lib ui::drain_contract`, which asserts every required drain
  parameter is registered, that the drain consumes a queued action with an observable effect, and
  that `StudioState` is defined exactly once. It must pass **unchanged** after the split. It also
  deleted the dead parallel drain that used to live in `slint_main.rs`; do not resurrect that
  pattern.
- Item `G6.04` (already `PASSED`) established the latency floor: `drain_ms` p99 ≤ 2.0 ms, max
  ≤ 8.0 ms, `drain_io_blocking_ms` max ≤ 1.0 ms, measured with the frozen `EUSTRESS_UI_TRACE=1`
  instrument on the 100x100 benchmark grid at seed 42. Its results are at
  `docs/PROMPTS/artifacts/G6.04/drain_latency_after.json`. **Read that file for the reference
  values; do not quote numbers from this prompt.**

**Build reality — this is the binding constraint on this item.** A full engine build takes 10-15
minutes. Only one cargo build at a time; the workspace shares a single `target/` and concurrent
builds produce LNK2001 or SAC os error 4551. Never kill a build mid-compile; recover with
`cargo clean -p eustress-engine`. Validate with `cargo run`, never `cargo check` — a module split
is exactly the change where `cargo check` passes and plugin registration silently breaks. Twenty
builds is about four hours of pure compile; move a whole domain group per build, not a function.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`;
cargo workspace root `E:/Workspace/EustressEngine/eustress`.

## 3. Scope

### In scope — files this item may edit or create
- `eustress/crates/engine/src/ui/slint_ui.rs` — redistribute its contents
- New modules under `eustress/crates/engine/src/ui/` (for example `drain/file.rs`, `drain/edit.rs`, `drain/insert.rs`, `drain/view.rs`, `drain/simulation.rs`, `sync/`)
- `eustress/crates/engine/src/ui/mod.rs` — module declarations
- Import paths in any file that referenced a moved item — mechanical `use` updates only
- `docs/PROMPTS/artifacts/G6.13/` (create; artifacts)
- `docs/PROMPTS/harness/checkers/` — the committed exit-criterion checker scripts this item is measured by

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.12/` — frozen prior evidence
- `eustress/crates/engine/src/ui/drain_contract.rs` — the regression test is frozen and must pass unchanged
- `eustress/crates/engine/src/ui_trace.rs` — the latency instrument is frozen
- Any `.slint` file — this is a Rust-side reorganisation; the surface does not move
- The **body** of any `SlintAction` match arm — moving an arm is in scope, changing what it does is not
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: reaching the line target by moving code into a `.slint` file, a macro
  expansion, a generated file, or a sibling crate outside `src/ui/`; excluding a file from the count
  because it is "generated"; relaxing the 3,000-line ceiling; and re-running the latency measurement
  on a different scene. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- **Exactly one system drains the queue, before and after.** See the escalation trigger.
- **One registration site.** All `init_resource`, `insert_resource`, and `add_message` calls for the
  drain's parameters stay together in one function. If a module needs a resource, it declares the
  type and the single plugin registers it.
- Behaviour must be identical. This is a move, not a rewrite. If you find a bug while moving, record
  it in the artifact as a follow-on candidate and **do not fix it here** — a behaviour change inside
  a refactor makes the latency comparison unattributable.
- Batch aggressively. Twenty builds across three approaches means roughly six builds per approach,
  so one build must validate a whole domain group.
- Run both frozen instruments after every build that compiles. A split that passes at the end and
  broke in the middle costs more than it saves.

## 5. Exit criterion

### Criterion
The largest file under `eustress/crates/engine/src/ui/` is **≤ 3,000** physical lines; exactly
**one** system is registered in the `SlintSystems::Drain` set; the `SlintAction` variant count is
**unchanged** from before the split; `cargo test -p eustress-engine --lib ui::drain_contract` exits
0; and measured `drain_ms` p99 is within **±5%** of the value recorded in
`docs/PROMPTS/artifacts/G6.04/drain_latency_after.json`.

### Measurement

Step 1 — line ceiling and drain uniqueness:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.13_check_1.ps1

The checker finds the largest `.rs` file under `eustress/crates/engine/src/ui`, counts
`.in_set(SlintSystems::Drain)` registrations across `eustress/crates/engine/src`, and prints
`SPLIT_OK` with exit 0 only when the largest file is ≤ 3,000 lines and there is exactly one drain
registration.

Step 2 — the frozen regression test:

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.13_check_2.ps1

The checker runs the frozen suite, prints `EXITCODE=<n>`, and exits with the cargo test exit code.

Step 3 — the frozen latency instrument, same scene and seed as `G6.04`:

    cargo run --release -p eustress-engine --bin generate-benchmark-map -- --grid-size 100 --spacing 4.0 --seed 42

    pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.13_check_3.ps1

The checker arms `EUSTRESS_UI_TRACE=1`, points `EUSTRESS_UI_TRACE_OUT` at
`docs/PROMPTS/artifacts/G6.13/ui_trace_postsplit.csv`, launches the studio, and exits with the
studio's own exit code.

Run the same four interaction sequences `G6.04` used (`panel_open`, `entity_select`,
`property_edit`, `error_surface`, five repetitions each), then:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.04/gate_drain.ps1 -Csv docs/PROMPTS/artifacts/G6.13/ui_trace_postsplit.csv -Baseline docs/PROMPTS/artifacts/G6.04/drain_latency_after.json -Out docs/PROMPTS/artifacts/G6.13/latency_postsplit.json

Expected output shape (step 1):

    max_file=E:\Workspace\EustressEngine\eustress\crates\engine\src\ui\slint_ui.rs lines=2841
    drain_registrations=1
    SPLIT_OK

Pass condition:

    max_file lines <= 3000  AND  drain_registrations == 1  AND step 1 exits 0
    AND step 2 EXITCODE == 0
    AND |drain_p99_postsplit - drain_p99_G6.04| / drain_p99_G6.04 <= 0.05

Read the emitted values and the exit codes. A tidy module tree that broke the drain is a
catastrophic pass.

## 6. Critic gate

`critic_gate` is `[]`. This item must produce **no** visible change, so scoring it visually would be
scoring noise. The mechanical criterion in §5 replaces it and is unusually tight for exactly that
reason: it gates on a line ceiling, on drain uniqueness, on a frozen test passing unchanged, and on
a latency band of ±5%. The last of those is the real gate — it is what distinguishes a refactor from
a rewrite wearing a refactor's name.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — extract by SlintAction domain (file / edit / insert / view /
                 simulation / workshop), one module per domain, arms moved verbatim, single
                 registration function retained in the plugin
   -> if still failing, MANDATORY approach change. Extracting two more domains is NOT an approach
      change; moving from domain modules to a trait-object handler registry, where each domain
      registers handlers for its variants and the single drain dispatches through the registry, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the largest-file line count dropping < 5% AND
                  the drain-contract test not returning to green
  - Budget      : 1.8M tokens or 30 builds consumed (150% of the XL envelope)
  - Item-specific: the split would require more than one system draining the queue (front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the ceiling to a stated line
count with the consequence named; FUND approach D with an estimate and why it differs materially;
DEFER behind a named item; or KILL with a statement of what W6 loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.13/split_report.json`

A reader finds: the before and after line count of every file under
`eustress/crates/engine/src/ui/`; the module map showing which `SlintAction` variants moved where;
the count of systems registered in the `SlintSystems::Drain` set before and after (1 and 1); the
`SlintAction` variant count before and after (equal); the drain-contract test exit code; the
`drain_ms` p99 before and after with the percentage delta; a list of bugs found while moving and
deliberately **not** fixed; and the commit. Archived alongside: `ui_trace_postsplit.csv`,
`latency_postsplit.json`, and the step-1 console output.

This is the W6 evidence for the item: the number that says future UI work is cheaper.

## 9. Definition of NOT done

- The largest file is 2,900 lines because 20,000 lines moved into a file outside
  `eustress/crates/engine/src/ui/`. The measurement is on that directory because that is where UI
  work happens.
- A second system now drains the queue "to keep the modules independent". That is the duplicate
  architecture that produced this pack's worst historical defect.
- Resource registration was distributed to the modules that need it, so there are now six
  registration sites. Both recorded occurrences of the total-UI-death bug were caused by exactly
  that.
- The drain-contract test was edited to accommodate the new module paths. Mechanical `use` updates
  in the test's own imports are acceptable and must be listed in the artifact; changing an assertion
  is not.
- A bug was fixed during the move, so the ±5% latency comparison is measuring two changes at once
  and can no longer attribute either.
- `cargo check` passed and `cargo run` was never executed, so the plugin registration is broken and
  the studio opens with a dead interface.
- The `SlintAction` variant count changed because two variants were "obviously redundant". That is a
  behaviour change inside a refactor.
- The latency measurement was run on a different scene than `G6.04` used, so the ±5% band is
  meaningless.
```

---
---

# `docs/PROMPTS/items/G6.14_time-to-first-successful-task.md`

```markdown
---
id: G6.14
title: Time-to-first-successful-task for a first-time professional user
workload: W1
workload_secondary: [W4, W6]
phase: G6
depends_on: [G6.05, G6.06, G6.07, G6.08, G6.10, G1.12]
blocks: [G6.15]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D4, D6]
capture_recipe: docs/PROMPTS/harness/recipes/G6_first_task.json
artifact: docs/PROMPTS/artifacts/G6.14/first_task_journey.json
escalation: >
  If completing the scripted journey requires any step whose success cannot be verified
  programmatically — i.e. the only evidence a step worked is that a human looked at it — STALL. An
  onboarding claim that cannot be re-verified by a stranger is a demo, not a measurement.
status: DRAFT
notes: >
  Tier L. The journey script is the deliverable's spine; the studio changes are whatever the
  journey's measured failures demand, which is why this item runs late in the pack.
---

## 1. Objective

A professional user opening the Eustress Studio for the first time reaches a verified successful
outcome — a part created, given a property, persisted, and confirmed after a reload — in a bounded
number of actions and a bounded time, with no dead end, no silent no-op, and no step requiring
knowledge that is not on screen. The journey is scripted, so the claim is re-runnable by someone who
has never seen the product.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust; Slint *is* Rust here. Units are meter-native; studs are a display unit only.

**The journey, fixed. Do not change these eight steps.**

1. Open the studio to a new, empty Space.
2. Create a Part.
3. Select it.
4. Rename it to `Bracket_A`.
5. Set its Position to a declared value.
6. Set its Material to a declared non-default value.
7. Save.
8. Close the Space, reopen it, and confirm all three property values survived.

Step 8 exists because `docs/AUDIT/02_STUDIO_ENGINE.md` records that the Properties panel
historically did not persist edits in the default build — the note reads *"legacy TOML write-back is
gated behind an opt-in `toml` cargo feature and the Fjall mirror only writes Transform so far"*.
Item `G6.05` closed that for a declared property set; step 8 is where this journey proves it end to
end rather than per property.

**What you inherit, and must not re-litigate.**

- `G6.05` (`PASSED`): at least twelve property kinds round-trip through save and reload. Evidence:
  `docs/PROMPTS/artifacts/G6.05/property_roundtrip.json`.
- `G6.06` (`PASSED`): unwired ribbon tools are visually distinguished before the click. Evidence:
  `docs/PROMPTS/artifacts/G6.06/ribbon_honesty.json`. Of the 1,999 tool ids in
  `eustress/crates/engine/src/tool_metadata.rs`, 30 carry `wired: true`. **The journey must use only
  wired tools**; a journey that routes a first-time user through a roadmap button is not an
  onboarding path.
- `G6.07` (`PASSED`): every command in the fixed top-20 list is within two clicks. Evidence:
  `docs/PROMPTS/artifacts/G6.07/click_depth_after.json`.
- `G6.08` (`PASSED`): every command in that list is keyboard-reachable. Evidence:
  `docs/PROMPTS/artifacts/G6.08/keyboard_coverage.json`.
- `G6.10` (`PASSED`): eight primary panels have designed empty and error states. Evidence:
  `docs/PROMPTS/artifacts/G6.10/state_coverage.json`. A first-time user sees the empty states before
  anything else, so this journey is where their copy is judged in context.

**How to drive the journey.** Use the engine bridge so the run is scriptable and rerunnable. From
`eustress/crates/mcp-server/src/bridge_tools.rs`: `equip_tool` (`:1041`), `select_entity` (`:1091`),
`get_editor_state` (`:1163`), `invoke_action` (`:1220`, which "Runs the SAME handler a real key
press does"), `capture_viewport` (`:1266`), `ai_camera_capture` (`:1381`). From the tools crate:
`create_entity`, `update_entity`, `query_entities`, `find_entity`. `invoke_action` accepts the
action names declared in `eustress/crates/engine/src/keybindings.rs` (`pub enum Action` at `:7`,
38 variants), including `SaveScene`.

**Action counting.** An action is one discrete user input: one mouse-down, or one chord press. The
journey's action count is the sum across all eight steps on the **shortest path a first-time user
could reasonably find**, not the shortest path an expert knows. If two routes exist, count the one a
first-time user would discover from what is on screen, and record why.

**Build reality.** 10-15 minutes per build; one build at a time (shared `target/`; concurrent builds
give LNK2001 or SAC os error 4551); never kill a build mid-compile. Validate with `cargo run`, never
`cargo check`.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may edit or create
- `docs/PROMPTS/artifacts/G6.14/` (create; the journey script and its artifacts)
- `docs/PROMPTS/harness/recipes/G6_first_task.json` (create if absent, per `docs/PROMPTS/02_CAPTURE_HARNESS.md` §8)
- `eustress/crates/engine/ui/slint/main.slint` — first-run affordances only, if the measured journey shows a discoverability failure
- The eight panel `.slint` files touched by `G6.10` — first-run copy only, not layout
- `eustress/crates/engine/src/ui/slint_ui.rs` — only to wire a first-run state, if one is needed

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.13/` — frozen prior evidence
- `eustress/crates/engine/src/tool_metadata.rs` — generated
- `eustress/crates/engine/src/ui_trace.rs`, `eustress/crates/engine/src/ui/drain_contract.rs`
- The `wired` treatment from `G6.06`, the palette from `G6.07`, the bindings from `G6.08` — you may use them, not change them
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: removing a step from the journey; pre-configuring the Space so a step is
  already done; counting an expert-only route as the discoverable one; starting the timer after the
  studio has loaded; and adding a scripted tutorial overlay that performs steps on the user's
  behalf. If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence
  and stop.
- Every step's success must be **programmatically verified**. Step 4 is verified by reading the name
  back, not by a screenshot of a text field.
- Do not add a modal tour. A first-run experience that blocks the interface to explain it has
  conceded that the interface does not explain itself, and the Critic will read the tour frames as
  what they are.
- Use only wired tools. See `G6.06`.
- Measure the journey **before** making any studio change, in the same session with the same script.
  A before-value measured differently is not a comparison.

## 5. Exit criterion

### Criterion
The eight-step journey completes with **8 of 8** steps programmatically verified, in **≤ 14 total
user actions** and **≤ 180 s** of wall-clock from process launch to the step-8 confirmation,
with **zero** steps that produce a silent no-op and **zero** steps requiring a tool whose
`tool_meta` says `wired: false`.

### Measurement

Step 1 — cold start with a fresh, empty Space:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.14/run_journey.ps1 -Fresh -Out docs/PROMPTS/artifacts/G6.14/first_task_journey.json

`run_journey.ps1` is written by this item. It launches the studio, starts a wall-clock timer at
process launch, drives the eight steps over the engine bridge, and for each step records
`{ordinal, description, actions, route, verified, verification_method, elapsed_ms, wired_tools_used}`.
Verification per step: step 2 by `query_entities` returning the new Part; step 3 by
`get_editor_state` reporting the selection; steps 4-6 by reading the property back; step 7 by the
save completing without error; step 8 by closing, reopening, and re-reading all three values.

It prints one line per step, then
`JOURNEY steps=<v>/8 actions=<n> elapsed_s=<t> silent_noops=<k> unwired_tools=<u>`, then
`JOURNEY_OK` and exit 0 when `v == 8 AND n <= 14 AND t <= 180 AND k == 0 AND u == 0`; otherwise
`JOURNEY_FAIL`, exit 1.

Step 2 — repeat five times and report the median and the worst run:

    pwsh -NoProfile -File docs/PROMPTS/artifacts/G6.14/run_journey.ps1 -Fresh -Repeat 5 -Out docs/PROMPTS/artifacts/G6.14/first_task_journey.json

Expected output shape:

    1 open_empty_space actions=0 verified=true 4210ms
    2 create_part actions=2 verified=true 5980ms
    ...
    8 reload_and_confirm actions=3 verified=true 41220ms
    JOURNEY steps=8/8 actions=13 elapsed_s=41.2 silent_noops=0 unwired_tools=0
    JOURNEY_OK

Pass condition:

    every one of 5 runs: steps == 8/8  AND  actions <= 14  AND  elapsed_s <= 180
    AND  silent_noops == 0  AND  unwired_tools == 0
    AND the script exits 0 on all five

Read the summary line and the exit code. A journey that "works when you know where things are" is
the failure this item exists to detect.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)** and **D6 (overall coherence)**, floor **8.0 each**.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_first_task.json` — one full-window frame per
journey step, in both palettes, at the three declared window sizes. The first frame, the empty
studio before any action, is the one that carries D4's first-impression weight, and it is the frame
`G6.10`'s empty states were built for.

D6 is gated because a journey crossing eight steps touches the ribbon, the viewport, the Explorer,
the Properties panel, the save path, and the reload path. If those read as six products, the
journey completing in thirteen actions will not save the score.

The Critic never sees your self-report, cites or fails, and can refuse a numerically clean result.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — script the journey, measure, then remove the specific measured
                 obstacles (missing affordance, ambiguous label, unverifiable step)
   -> if still failing, MANDATORY approach change. Renaming a button is NOT an approach change;
      restructuring the first-run layout so the journey's next step is always the most prominent
      thing on screen is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the median action count moving < 1 AND the worst
                  gated dimension moving < 0.5
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a step's success cannot be programmatically verified (see front matter)
```

The STALL packet fits one screen and requests exactly one of: LOWER the action or time ceiling to a
stated value with the consequence; FUND approach D; DEFER behind a named item; or KILL with what W4
loses. Recommend one.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.14/first_task_journey.json`

A reader finds: for each of five runs, the eight step records with actions, route, verification
method and result, and elapsed time; the median and worst-case action count and elapsed time; the
silent-no-op count; the list of tools used with each one's `wired` flag; the before-values from the
pre-change measurement; and the commit. Archived alongside: `run_journey.ps1` and the capture bundle
manifest.

This is the W1 and W4 evidence for the item, and it is the artifact a pilot conversation can point
at when it claims a professional can be productive in the first session.

## 9. Definition of NOT done

- The journey passes because the script knows the exact coordinates of every control. The action
  count must reflect a route a first-time user could find from what is on screen, and the artifact
  must say how each route was discoverable.
- The timer starts after the studio window appears, hiding a 40-second load. A first-time user's
  clock starts when they double-click the executable.
- A step is "verified" by a screenshot. Step verification is programmatic; screenshots are for the
  Critic, not for the gate.
- The journey routes through a tool whose `tool_meta` says `wired: false`, so the first thing a new
  user learns is that the ribbon is aspirational.
- Step 8 confirms one property instead of all three, so a partial-persistence regression against
  `G6.05` passes unnoticed.
- A modal first-run tour was added and the action count fell because the tour performs steps. The
  interface still does not explain itself; it just has a chaperone.
- Only one run was performed. Five runs exist because a journey that works once and fails twice is
  the normal failure mode of onboarding.
- The Critic passes the numbers and refuses the item, citing the first frame — an empty studio that
  gives a professional no idea what to do next. That is a legitimate refusal, and it is exactly the
  frame `G6.10` was meant to fix.
```

---
---

# `docs/PROMPTS/items/G6.15_blind-ui-craft-scorecard.md`

```markdown
---
id: G6.15
title: Blind UI-craft scorecard — the pack's proof artifact
workload: W1
workload_secondary: [W3]
phase: G6
depends_on: [G6.04, G6.06, G6.09, G6.10, G6.11, G6.12, G6.13, G6.14, G1.12]
blocks: []
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: [D1, D4, D6, D7]
capture_recipe: docs/PROMPTS/harness/recipes/G6_final_blind.json
artifact: docs/PROMPTS/artifacts/G6.15/blind_bundle_manifest.json
escalation: >
  If the blinding leak checklist cannot be satisfied — for example if the studio renders a version
  string, a build date, or a product name anywhere in a captured frame and it cannot be suppressed
  for the capture — STALL. A bundle the Critic can de-blind produces a scorecard that proves
  nothing, and running the Critic on it wastes the pack's most expensive resource.
status: DRAFT
notes: >
  Tier L. This item produces no product change. Its entire cost is capture, blinding, verification,
  and the Critic invocation. The 12 builds are for capturing the control side at the pre-pack commit.
---

## 1. Objective

An independent, blinded Critic, shown randomised unlabelled captures of the Eustress Studio before
and after phase G6, scores the post-pack build at or above the floor on every gated dimension and
selects it on the forced preference question in at least four of five independent trials. The
scorecard is the pack's proof that the UI craft work moved something a stranger can perceive.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. Never "a game engine". Licence PolyForm Shield 1.0.0, so
**source-available**, never "open source". Physics is **Avian**, never Rapier. `.slint` files
compile to Rust; Slint *is* Rust here. Units are meter-native; studs are a display unit only.

**What this item is not.** It is not a chance to improve the studio. Every product change in phase
G6 has already landed and been measured. This item captures, blinds, verifies, and submits. If the
Critic fails the bundle, the correct response is a stall packet naming which prior item's floor was
too low — not a quiet fix followed by a re-capture.

**The seven prior items whose work is on trial, and their evidence.**

| Item | What it changed | Evidence |
|---|---|---|
| `G6.04` | Drain path never blocks; p99 ≤ 2.0 ms | `docs/PROMPTS/artifacts/G6.04/drain_latency_after.json` |
| `G6.06` | 1,969 unwired ribbon tools visually distinguished pre-click | `docs/PROMPTS/artifacts/G6.06/ribbon_honesty.json` |
| `G6.09` | One cyan `#00bcd4` selection identity in both palettes and the viewport | `docs/PROMPTS/artifacts/G6.09/theme_coherence.json` |
| `G6.10` | Designed empty and error states in eight panels | `docs/PROMPTS/artifacts/G6.10/state_coverage.json` |
| `G6.11` | Accessibility annotations and focus order in ten panels | `docs/PROMPTS/artifacts/G6.11/a11y_coverage.json` |
| `G6.12` | Density and typographic hierarchy in three panels | `docs/PROMPTS/artifacts/G6.12/density_measurement.json` |
| `G6.14` | Verified eight-step first-task journey | `docs/PROMPTS/artifacts/G6.14/first_task_journey.json` |

**The control side.** The comparison is against the commit at which phase G6 opened — the parent of
`G6.01`'s first commit. Identify it from git history and record it in the manifest. Both sides must
be captured with the **same** recipe, the same scene, the same window sizes, the same palette, and
the same interaction sequences. Anything that differs between the sides other than the code under
test invalidates the comparison.

**The blinding contract, restated so you do not have to fetch it.**
`docs/PROMPTS/02_CAPTURE_HARNESS.md` §6.1 specifies the procedure: two capture runs produce SUBJECT
and CONTROL trees; a per-trial permutation drawn from a CSPRNG seeded independently of the bundle
hash assigns them to ALPHA and BETA; frames are copied into the blinded tree renamed by **position
only**; then a mandatory metadata scrub — strip all PNG ancillary chunks except IHDR/IDAT/IEND,
strip EXIF and XMP, normalise file mtimes to a constant, normalise file ordering to tick index only,
and verify no pixel content contains a build string, version overlay, watermark, FPS counter, window
title bar, or debug HUD. The key mapping ALPHA/BETA to SUBJECT/CONTROL goes in a separate file the
L1 holds and that is **never** placed in the Critic's input directory. The permutation is redrawn
for each of the five preference trials.

§6.2 gives the leak checklist, every item of which is a real leak channel: no filename, path
component, or archive name containing `eustress`, a commit hash, a version, a product name,
`subject`, `control`, `a`, `b`, `old`, `new`, `before`, or `after`; no surviving PNG text chunk,
EXIF, or XMP; no frame containing a UI element carrying a product name, version, or build date;
**equal frame counts on both sides**, because an unequal count is itself a label; identical
resolutions; and no accompanying prose.

That last one matters for how you work: the Critic receives the rubric, the frames, the L1's
verified measurement, and the drawn held-out criteria — **nothing else**. It never sees your result
block, your commit messages, or any caption. Do not write anything intended for it.

**The floors.** Pass floor is **8.0** on every gated dimension; the mean across dimensions is
irrelevant. The preference question requires the post-pack side to be chosen in **≥ 4 of 5**
independent trials. Every Critic score must carry a citation (a frame index, a `path:line`, or a
measured value); an uncited score auto-fails the whole scorecard. The Critic may refuse to pass
something that clears every number, and may never pass something that misses one.

**The window-title problem, specifically.** The studio is a desktop application. Full-window
captures at `docs/PROMPTS/02_CAPTURE_HARNESS.md` §4.3's three sizes will include chrome unless
suppressed, and `eustress/crates/engine/Cargo.toml` sets `ProductName = "Eustress Engine"` in its
version-info block, which surfaces in the window title. Solve this in the capture, not by cropping
one side.

**Build reality.** 10-15 minutes per build; one build at a time (shared `target/`; concurrent builds
give LNK2001 or SAC os error 4551); never kill a build mid-compile. Validate with `cargo run`, never
`cargo check`. Capturing the control side requires building at the pre-pack commit — use a separate
`--target-dir` rather than switching the working tree back and forth, and never run the two builds
concurrently.

**Environment.** Windows 11, PowerShell 7 (`pwsh`). Repository root `E:/Workspace/EustressEngine`.

## 3. Scope

### In scope — files this item may create
- `docs/PROMPTS/artifacts/G6.15/` (create; the manifest, the leak-check report, the scorecard record)
- `docs/PROMPTS/harness/recipes/G6_final_blind.json` (create if absent, per `docs/PROMPTS/02_CAPTURE_HARNESS.md` §8)

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.01/` through `G6.14/` — frozen prior evidence
- **Every file under `eustress/`** — this item captures and judges; it does not change the product
- The blinding key file, once written — it belongs to the L1

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Specifically forbidden: capturing the two sides with different recipes, scenes, window sizes,
  palettes, or sequences; cropping one side; re-capturing after seeing a scorecard; reducing the
  frame set; reducing the trial count below five; and reusing one permutation across trials. If the
  measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- **Do not change the product.** If the capture reveals a defect, record it as a finding and, if it
  is severe enough, stall. Fixing it here destroys the attribution the whole pack depends on.
- Equal frame counts on both sides. An unequal count is a label, per §6.2.
- Run the leak checklist and record every item's result before the Critic is invoked. A bundle
  submitted without a completed checklist is inadmissible.
- If the leak checklist finds `blinding_compromised` for any reason — including the file-size
  ordering check in §6.2 — record it in the manifest and stall rather than proceeding. A compromised
  blind produces a scorecard nobody can cite.

## 5. Exit criterion

### Criterion
A content-addressed blinded bundle exists whose manifest validates; the leak checklist passes on
**all** items with `blinding_compromised: false`; both sides have **equal** frame counts and
identical resolutions; five independent preference trials were drawn with five distinct
permutations; and the returned Critic scorecard reports **≥ 8.0 on every gated dimension** and the
post-pack side selected in **≥ 4 of 5** trials.

### Measurement

Step 1 — capture both sides with one recipe:

    cargo run --release --bin eustress-capture -- --recipe docs/PROMPTS/harness/recipes/G6_final_blind.json --subject HEAD --control <pre-pack-commit> --out docs/PROMPTS/artifacts/bundles --trials 5 --verify-determinism

`eustress-capture` and its `blind`, `manifest`, and `verify` subcommands are delivered by phase G1
(build-list items B4, B5, B6, B9 in `docs/PROMPTS/02_CAPTURE_HARNESS.md` §9). Substitute the actual
pre-pack commit hash for `<pre-pack-commit>`.

Step 2 — verify the bundle and the blind:

    cargo run --release --bin eustress-capture -- verify --bundle docs/PROMPTS/artifacts/bundles/<hash>/ --leak-check --out docs/PROMPTS/artifacts/G6.15/blind_bundle_manifest.json

`verify` must re-hash the bundle, reject on mismatch, run the §6.2 leak checklist, and report
`frames_alpha`, `frames_beta`, `resolutions_match`, `permutations_distinct`, and
`blinding_compromised`. It prints `BLIND_OK` and exits 0 when the manifest re-hashes, every leak
item passes, `frames_alpha == frames_beta`, `resolutions_match == true`,
`permutations_distinct == 5`, and `blinding_compromised == false`; otherwise `BLIND_FAIL`, exit 1.

Step 3 — the L1 invokes the Critic on the blinded tree and archives the returned scorecard at
`docs/PROMPTS/artifacts/critic/G6.15/scorecard.json`. The executing agent does **not** invoke the
Critic and does not read the rubric.

Expected output shape (step 2):

    {"bundle":"<sha256>","frames_alpha":1263,"frames_beta":1263,"resolutions_match":true,
     "permutations_distinct":5,"leak_items_failed":[],"blinding_compromised":false,
     "determinism_verified":true}
    BLIND_OK

Pass condition:

    step 2 prints BLIND_OK and exits 0
    AND the archived scorecard reports every gated dimension >= 8.0
    AND the post-pack side is selected in >= 4 of 5 trials

Read the emitted markers and the scorecard's values. A bundle existing is not a pass; the whole
point of this item is that someone else does the judging.

## 6. Critic gate

Gated on **D1 (first-three-seconds impact)**, **D4 (UI craftsmanship)**, **D6 (overall coherence)**,
and **D7 (the preference question)**. Floor **8.0** on D1, D4, and D6; D7 requires the post-pack
side in **4 of 5** blind forced-choice trials. One dimension below floor fails the item; the mean is
irrelevant.

Capture recipe: `docs/PROMPTS/harness/recipes/G6_final_blind.json`. It must cover: the `FS-UI`
sequences from `docs/PROMPTS/02_CAPTURE_HARNESS.md` §4.3 (`panel_open`, `entity_select`,
`property_edit` including its post-reload step, `error_surface`) at all three window sizes; the
first-run empty studio; the ribbon in three modes; and both palettes.

D1 is gated here and nowhere else in the pack because a first-run frame is the only place a stranger
forms a three-second judgement of a tool, and phase G6 is the phase that owns that frame.

The Critic never sees anything the executing agent writes, cites or fails on every score, and can
refuse to pass a bundle that clears every number. Nothing in this item's scope can change that
outcome — which is the point.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — capture both sides with the shared recipe, blind, verify, submit
   -> if the BUNDLE fails verification, iterate on the capture. If the SCORECARD fails a floor,
      that is NOT an iteration of this item — it is a stall, because fixing the product here
      destroys attribution.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff) — only
                 for bundle/blinding failures
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with leak_items_failed unchanged
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the leak checklist cannot be satisfied (see front matter)
  - Scorecard   : ANY gated dimension below 8.0, or the preference question below 4 of 5 —
                  stall immediately with the failing dimension and its cited frames
```

The STALL packet fits one screen and requests exactly one of: LOWER a named floor to a stated value
with the consequence for the program's definition of done; FUND a re-open of a specific prior item
with its identifier and the citation that motivates it; DEFER behind a named item; or KILL with what
the phase loses. Recommend one. A scorecard-driven stall must name **which prior item's floor was
too low**, with the Critic's cited frames as evidence — that is the packet's whole value.

## 8. Artifact

`docs/PROMPTS/artifacts/G6.15/blind_bundle_manifest.json`

A reader finds: the bundle SHA-256; the subject and control commits; the recipe path and its hash;
the scene, window sizes, palettes, and sequences captured; frame counts per side; the resolution
check; the five permutations' distinctness; the completed §6.2 leak checklist with each item's
result; the `blinding_compromised` flag; the `determinism_verified` flag; and a pointer to the
archived scorecard at `docs/PROMPTS/artifacts/critic/G6.15/scorecard.json`.

This is the W1 evidence for the pack as a whole, the W3 evidence that the comparison was
verifiable, and the artifact the phase report and any pilot conversation cite.

## 9. Definition of NOT done

- The two sides were captured with different recipes, so the comparison measures the recipe.
- One side was cropped to remove a title bar and the other was not, so file sizes are
  systematically ordered and the blind is compromised in a way the §6.2 checklist explicitly names.
- Frame counts differ between the sides. An unequal count is itself a label.
- One permutation was reused across trials, so the five preference trials are one trial reported
  five times.
- The window title bar, containing `ProductName = "Eustress Engine"` from
  `eustress/crates/engine/Cargo.toml`, survives in the frames on both sides — technically symmetric,
  but it names the product to a Critic that must not know it.
- The scorecard came back with one dimension at 7.6 and the agent quietly fixed a panel and
  re-captured. That destroys attribution across seven prior items and makes the whole pack's
  evidence chain unciteable.
- The executing agent read `docs/PROMPTS/01_CRITIC_RUBRIC.md` to anticipate the held-out criteria.
  The rubric is not readable by an executing agent, and a bundle produced with knowledge of the
  held-out criteria is inadmissible.
- The stall packet says the Critic "was not impressed" instead of naming which prior item's floor
  was too low and citing the frames. A packet without a named item and a citation is malformed.
```
