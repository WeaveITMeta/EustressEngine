# G1 — Capture & Measurement Harness

**Pack owner scope:** the instrument every other pack measures with. The workspace compiling at all;
the pinned reference machine that makes a millisecond threshold mean something; the frozen harness
scenes and camera paths; the parameterised, tick-indexed capture path; the `eustress-capture`
binary with its manifest, blinding, and determinism-verification subcommands; the single
library-wide capture-recipe format; and the externally licensed control artifact that the program's
own definition of done depends on.

**Workloads this pack feeds:** primarily **W3 (Trust & Verifiability)** — a stranger reproducing our
claims without us. Secondary **W6 (Operator Leverage)**, because the whole point of `eustress-capture`
is that one command replaces a manual, desktop-bound, hand-authored evidence ritual, and **W1
(Provable Quality)** for the items that make a Critic bundle admissible at all.

**ITEM ZERO: `G1.01`.** Nothing else in this pack, and nothing in any other pack, may start until
`G1.01` is `PASSED`. `G1.01` is not a harness item. It is the fact that
`cargo build --workspace` does not currently succeed, which means every "we built it and measured
it" claim anywhere in this library rests on a workspace nobody has compiled. `00_MASTER_PROTOCOL.md`
§3.2 calls G1 "item zero and not negotiable"; this is item zero *of* item zero.

**Why this pack exists.** `docs/PROMPTS/02_CAPTURE_HARNESS.md` §9 lists build items **B1–B14** and
declares B1–B9 to be G1's exit condition. `00_MASTER_PROTOCOL.md` §3.2 makes every other phase's
exit condition a statement about a measurement G1 produces. Across the seven authored packs, **53
items cite a capture recipe**, spread over **33 distinct recipe paths**, every one of them under
`docs/PROMPTS/harness/recipes/` — a directory that does not exist. Five packs independently instruct
their executing agent to author the missing recipe "against the schema in §8 of that document",
which would produce five mutually incompatible formats authored by five agents who never meet.
`G1.12` in this pack authors all 33 once, in one format, so that never happens.

---

## Honest state of the world this pack starts from

All verified in-tree on 2026-08-07 at commit `71ccf6fe`.

**The workspace does not compile.**
`eustress/crates/backend` is workspace member 11 (`eustress/Cargo.toml:12`).
`eustress/crates/backend/src/marketplace.rs` calls four methods on `state.db`
(`eustress_backend::db::Database`) that `eustress/crates/backend/src/db.rs` (286 lines) does not
define:

| Call site | Method called | Nearest thing that exists in `db.rs` |
|---|---|---|
| `marketplace.rs:166`, `:194`, `:231` | `find_marketplace_item_by_id` | `get_marketplace_item_by_id` at `db.rs:282` |
| `marketplace.rs:199` | `has_purchased` | nothing |
| `marketplace.rs:207`, `:215` | `get_user_balance` | nothing |
| `marketplace.rs:213` | `purchase_item` | nothing (the free function `marketplace::purchase_item` at `marketplace.rs:185` is the HTTP handler, routed at `main.rs:124`) |

**CI has never compiled the workspace.** `.github/workflows/ci.yml` has three jobs — `security`
(`cargo deny --config ../deny.toml check advisories bans sources`, line 64), `shader-validate`
(naga, line 66), and `data-graph-default` (a `cargo tree` grep, line 121). `.github/workflows/
linux-engine.yml:48` runs `cargo check --package eustress-engine` and nothing else. No job has ever
run `cargo build --workspace`, and no job runs `cargo test`.

**Nothing under `eustress/spaces/` exists.** The directory is absent. Eleven items across the packs
cite scene paths beneath it.

**Nothing under `docs/PROMPTS/harness/`, `docs/PROMPTS/artifacts/`, or `docs/PROMPTS/items/`
exists.** `docs/PROMPTS/` contains exactly `00_MASTER_PROTOCOL.md`, `01_CRITIC_RUBRIC.md`,
`02_CAPTURE_HARNESS.md`, `03_PROMPT_SCHEMA.md`, and `packs/`.

**`eustress-capture` does not exist.** `eustress/crates/engine/Cargo.toml` declares six `[[bin]]`
targets: `eustress-engine`, `generate-benchmark-map`, `convert-to-eustress`, `reseed-space-subtree`,
`eustress-lsp`, `eustress-headless`. Three authored items already invoke
`cargo run --release --bin eustress-capture` in their exit-criterion measurement blocks —
`T3` `G6.15` (`T3_studio_ux.md:3767` and `:3775`), `T5` `G7.42`
(`T5_robustness_and_cohesion.md:2594`), and `T5` `G7.45`
(`T5_robustness_and_cohesion.md:3280`) — so those three exit criteria are currently unmeasurable.

**The AI camera cannot be asked for a resolution.** `eustress/crates/engine/src/ai_camera.rs:43-44`
declares `pub const AI_CAM_WIDTH: u32 = 1280;` and `pub const AI_CAM_HEIGHT: u32 = 720;`.
`pub fn request_capture(state: &mut AiCameraState, path: PathBuf)` at `ai_camera.rs:158` takes a
path and nothing else. The MCP wrapper `ai_camera_capture`
(`eustress/crates/mcp-server/src/bridge_tools.rs:1381`) sends an empty JSON object. 4K capture is
therefore impossible today, and `02_CAPTURE_HARNESS.md` invariant **I5** requires a declared
resolution identical on both sides.

**Pose control already works and must not be rebuilt.** `ai_camera_set_pose`, `ai_camera_orbit`, and
`ai_camera_frame` at `bridge_tools.rs:1292,1322,1352` set exact poses.

**There is no headless GPU tier.** `eustress/crates/engine/src/bin/headless.rs` is 291 lines of
`MinimalPlugins` + `ScheduleRunnerPlugin`, registered as `eustress-headless` behind
`required-features = ["world-db"]` (`engine/Cargo.toml:59-62`). `docs/architecture/
HEADLESS_RUNTIME.md:269` lists `P6 --render gpu` as `new` — unstarted. Every capture therefore needs
a desktop session today. That is `02_CAPTURE_HARNESS.md` **B10** and it belongs to **G7**, not to
this pack.

**The determinism gate has never been executed.** `docs/architecture/HEADLESS_RUNTIME.md:294` states
it: "the same space + same `--ticks` produces byte-identical recordings across two runs". It has
never been run. `eustress/crates/common/src/physics/determinism.rs` is 56 lines — a `GlobalRngSeed`
resource and nothing else.

**Time compression silently drops physics steps.**
`eustress/crates/common/src/simulation/clock.rs:83` `advance()` adds the full compressed delta to
`simulation_time_s` (`clock.rs:88`), drains the accumulator into at most `max_ticks_per_frame`
discrete ticks, and then **zeroes the accumulator on saturation** (`clock.rs:100-102`). Under
compression the clock reports time that was never stepped. This is why `02_CAPTURE_HARNESS.md` §5.1
forbids `sim.time_scale != 1.0` inside any capture run, and why **B7**, the independent step
counter, is in this pack.

**Project invariants every item in this pack inherits.** Eustress is an AI-native simulation
substrate / world engine, never a game engine — rendering and the ECS are implementation details in
service of that. The licence is PolyForm Shield 1.0.0; say source-available, never open source.
Physics is Avian, never Rapier. Slint is Rust; `.slint` compiles to Rust. Units are meter-native and
studs are a display unit only. A full engine build takes 10–15 minutes, one cargo build at a time
against the shared `target/`, never killed mid-compile. Validate with `cargo run`, not `cargo check`.

---

## Pack notes — four decisions this pack makes on the record

### N1. The G1 exit condition is amended from "not the founder's machine" to "clean checkout, no operator-local state"

`00_MASTER_PROTOCOL.md` §3.2 sets G1's exit condition as: the harness "runs from a single command,
on a machine that is not the founder's, and produces a content-addressed bundle with a valid
provenance manifest." The operator is a solo founder with one Windows box. As written the condition
is unmeetable, and an unmeetable exit condition silently converts G1 into a phase that can never
close, which blocks all seven phases behind it.

The condition is therefore amended to: **"from a clean checkout of the repository at a named commit,
in a working directory containing no operator-local state, with every path in the recipe resolved
relative to that checkout."** `G1.13` is the item that measures it, and it measures it by cloning
into a fresh directory and running the command with `HOME`/`USERPROFILE` and every `EUSTRESS_*`
environment variable cleared.

The second-machine requirement is not deleted — it is converted into an explicit human decision.
`G1.13` emits a decision request naming exactly two options (procure or borrow a second machine;
or accept the clean-checkout substitute and record the residual risk in the phase report). Only the
human chooses. What the substitute does not prove is machine-independence: a GPU-driver-dependent
render difference would survive a clean checkout on the same box undetected. That residual risk is
stated in `G1.13`'s artifact rather than left implicit.

### N2. `RM-1` is pinned, and the frame-cost triggers already in the library are now falsifiable

`T1_rendering_and_content.md` sets hard escalation triggers against "the harness reference GPU" at
`:1221` (2.0 ms), `:1300` (2.0 ms), `:2119` (2.5 ms), `:2216` (2.5 ms), `:2441` (2.0 ms), `:1535`
(1.5 ms), and an `ft_p99_ms ≤ 25.0` ceiling at `:4275`. No machine is named anywhere in the
repository, so none of those triggers can currently be met or missed.

`G1.02` pins reference machine **`RM-1`** and every recipe carries `"reference_machine": "RM-1"`.
The current operator machine, `MEASURED` on 2026-08-07 via
`Get-CimInstance Win32_VideoController` and `Get-CimInstance Win32_OperatingSystem`:

| Field | Value |
|---|---|
| GPU | NVIDIA GeForce GTX 1080 Ti |
| GPU driver | 32.0.15.8253 |
| OS | Windows 11 Pro |
| OS build | 10.0.26200 |

**Flagged for the human, not for an agent to fix silently:** those seven frame-cost thresholds were
authored before any machine was named. A GTX 1080 Ti is a 2017 part. A 1.5 ms budget at 3840×2160 on
it is a materially harder target than the same number on a current GPU, and several T1 items may
STALL on the trigger rather than on the fidelity work. Pinning `RM-1` does not change any threshold;
it makes each one checkable. Re-basing a threshold is a floor change, which
`00_MASTER_PROTOCOL.md` §2.1 forbids to an L1 and §5.3 routes to the human as a `LOWER` decision.

### N3. `G3.01` and this pack overlap; G1 should own the capture path and `G3.01` should be re-scoped

`T1_rendering_and_content.md:93` declares `G3.01` "Deterministic, parameterised reference capture
path" as T1's own item zero. Its scope overlaps three items here:

| `G3.01` sub-goal | This pack's owner |
|---|---|
| Resolution and pose parameterisation of `ai_camera.rs` / `request_capture` | `G1.05` |
| Two captures of the same static scene hashing identically | `G1.11` |
| Emitting harness scenes under `eustress/spaces/harness/` from a seeded generator | `G1.03` |

**Recommendation to L0: G1 owns all three, and `G3.01` is re-scoped to depend on
`[G1.03, G1.05, G1.11]` rather than to rebuild them.** Two reasons, neither of them tidiness.

First, `G3.01` scopes its generator to three T1 scenes (`RH1_sphere_grid`, `RH2_interior`,
`RH3_exterior`) and a new bin `render-harness`. `02_CAPTURE_HARNESS.md` §3 normatively requires six
scenes `S1`–`S6`, and `T3_studio_ux.md:1069` already cites `S4_studio_ui` by name. If `G3.01` lands
first, `S4`–`S6` are owned by nobody and `T3`'s eleven UI items have no scene.

Second, `G3.01`'s determinism criterion is scoped to `ai_camera.capture` alone. `G1.11` verifies
determinism over a whole recipe — frames, recording JSON, and step count — which is the property
`00_MASTER_PROTOCOL.md` §1.1 D2 actually requires.

`G1.03` resolves the naming collision without breaking anything already authored: the on-disk
directory names for `S1`–`S3` are `RH1_sphere_grid`, `RH2_interior`, and `RH3_exterior`, because
eleven authored items already cite those exact paths, while the manifest records the normative
`scene_id` (`S1`/`S2`/`S3`). `RH4_cascade`, `RH5_hlod`, `RH6_roundtrip`, `RH7_splat_composite`,
`RH8_opening`, and `FLAGSHIP` stay with T1 — they are render-specific and each has a T1 item that
owns it. This is an L0 ruling, not an L1 one, because it crosses packs.

### N4. `G7_time_compression.json` is a recipe for evidence *of* compression, never a compressed capture

`T4` `G7.15` cites `docs/PROMPTS/harness/recipes/G7_time_compression.json`.
`02_CAPTURE_HARNESS.md` §5.1 forbids `sim.time_scale != 1.0` in any capture run. Both hold: the
recipe schema in `G1.12` hard-constrains `settings.sim_time_scale` to `1.0` and rejects any other
value at parse time, and the compression evidence is carried in a separate, explicitly labelled
`compression_experiment` block whose output is a measurement series, not a frame set. The frames in
that bundle are captured at `1.0` and show the instrumentation surfacing the step drop. A recipe
that tries to capture frames under compression is malformed and `eustress-capture` must refuse it.

---

## Dependency ladder

| ID | Title | Tier | Critic gate | `depends_on` | `02` build items |
|---|---|---|---|---|---|
| **G1.01** | **`cargo build --workspace` exits 0** (**ITEM ZERO**) | M | — | — | — |
| G1.02 | Pinned reference machine `RM-1` and the hardware block | S | — | — | — |
| G1.03 | Harness scene set `S1`–`S6` and the seeded scene generator | L | — | G1.01 | B14 |
| G1.04 | Camera path files `CP-A/B/S/U` and the path player | M | — | G1.03 | B3 |
| G1.05 | AI camera resolution and pose parameterisation | M | — | G1.01 | B1 |
| G1.06 | Tick-indexed capture trigger | M | — | G1.05 | B2 |
| G1.07 | Independent physics-step counter | M | — | G1.01 | B7 |
| G1.08 | Per-frame, tick-indexed frame-time CSV export | M | — | G1.01 | B8 |
| G1.09 | `eustress-capture` binary: recipe → bundle → manifest | XL | — | G1.02, G1.04, G1.06, G1.07, G1.08 | B4, B6 |
| G1.10 | `blind` subcommand, leak checklist, and `KEY.json` | L | — | G1.09 | B5 |
| G1.11 | `--verify-determinism` and the byte-identity selftest | L | — | G1.09 | B9 |
| G1.12 | The 33 capture recipes in one library-wide format | L | — | G1.02, G1.09 | §8 |
| G1.13 | Clean-checkout one-command G1 gate | L | — | G1.10, G1.11, G1.12 | §8 |
| G1.14 | External reference control and publication-rights memo (**HUMAN-EXECUTED**) | S | — | G1.02, G1.12 | §7.3 |

`critic_gate` is `[]` on every item in this pack, and `capture_recipe` is `none` on every item. That
is not an oversight and each item says why in its own §6. This pack builds the instrument the Critic
scores *with*; an item here cannot cite a bundle produced by a binary the same item is building, and
a blinded evaluator has nothing to look at in a manifest writer. Every item compensates with an
unusually tight mechanical criterion, as `03_PROMPT_SCHEMA.md` §3 requires when `critic_gate` is
empty.

`B10` (headless GPU tier) and `B11` (CI capture job) are **G7's**, per `02_CAPTURE_HARNESS.md` §9,
and are deliberately absent from this ladder. `B12` (perceptual diff) and `B13` (bundle browser) are
marked non-blocking in `02` §9 and are not authored here; if L0 wants them they belong at `G1.15+`.

---

## Cross-pack file ownership

Items in this pack share source files with items in other packs. `docs/PROMPTS/04_FILE_OWNERSHIP.md` names one owner per contested path and is normative; where it conflicts with an item's scope list, it wins. This pack's own dependency-graph table records its internal edges; the cross-pack edges are these:

| Item | Now depends on | Contested path | Effect on this item's scope |
|---|---|---|---|
| `G1.11` | `G2.02` (T2) | `eustress/crates/common/src/physics/determinism.rs` | may no longer edit it |

An item blocked by one of these entries emits a `FILE-OWNERSHIP` decision packet to L0 (`docs/PROMPTS/04_FILE_OWNERSHIP.md` §6). It does not edit the file and does not work around it.

---


---

## G1.01 — `cargo build --workspace` exits 0 (**ITEM ZERO**)

Item path: `docs/PROMPTS/items/G1.01_workspace-compiles.md`

````markdown
---
id: G1.01
title: cargo build --workspace exits 0
workload: W3
workload_secondary: [W6]
phase: G1
depends_on: []
blocks: [G1.03, G1.05, G1.07, G1.08, G0.03, G3.02, G7.01]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.01/workspace_build.json
escalation: >
  If making the workspace build requires editing a crate other than eustress/crates/backend or the
  workspace members list in eustress/Cargo.toml, STALL immediately with the crate named and the
  first compiler error quoted verbatim. A second broken member is a different item and must not be
  absorbed into this one silently.
status: DRAFT
notes: >
  Tier M rather than S because the item is defined by a build and the verification is a build. Six
  build slots against a full workspace build is roughly ninety minutes of pure compile, which is the
  real ceiling here; the token envelope will not bind.
---

## 1. Objective

`cargo build --workspace` run from `eustress/` completes with exit code 0. Every crate listed in
`eustress/Cargo.toml` `[workspace] members` compiles. The item states, in its artifact, which of two
permitted remedies it chose for `eustress/crates/backend` and the evidence that justified the
choice.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — a world model an AI reasons
over and a document a human edits. It is never described as a game engine. The licence is PolyForm
Shield 1.0.0; the project is source-available, never open source. Physics is Avian, never Rapier.

**Why this item is item zero of the whole program.** `docs/PROMPTS/00_MASTER_PROTOCOL.md` §3.2 makes
phase G1 "item zero and not negotiable", and every later phase's exit condition is a statement about
a measurement G1 produces. A measurement produced by a workspace that has never compiled is not a
measurement. Three things in the prompt library depend on this item and cannot be satisfied without
it:

- `T5` item `G7.34` (`docs/PROMPTS/packs/T5_robustness_and_cohesion.md:890`) requires `cargo test` to
  run in CI and be green. `cargo test --workspace` compiles every member; it cannot pass while a
  member does not build.
- `B2` item `G1.40` (`docs/PROMPTS/packs/B2_revenue_ecosystem_org.md:102`) builds
  `eustress-backend` specifically, to replace source-reading assertions about the money rail with
  build evidence. Its own escalation clause fires if that build cannot complete.
- CI has only ever compiled one package, so nothing has ever caught this.

**The exact defect.** `eustress/crates/backend` is workspace member 11 at `eustress/Cargo.toml:12`.
`eustress/crates/backend/src/marketplace.rs` calls four methods on `state.db` (type
`eustress_backend::db::Database`) that `eustress/crates/backend/src/db.rs` (286 lines) does not
define:

| Call site | Method called | Nearest existing method |
|---|---|---|
| `marketplace.rs:166`, `:194`, `:231` | `find_marketplace_item_by_id` | `get_marketplace_item_by_id` at `db.rs:282` |
| `marketplace.rs:199` | `has_purchased(user_id, item_id) -> Result<bool, _>` | none |
| `marketplace.rs:207`, `:215` | `get_user_balance(user_id)` | none |
| `marketplace.rs:213` | `purchase_item(user_id, item_id, price)` | none |

Note the name collision that makes this easy to misread: `marketplace.rs:185` defines a free
function `pub async fn purchase_item(State(state), headers, Json(req))`, which is the axum HTTP
handler routed at `eustress/crates/backend/src/main.rs:124`. The missing item is a *method on
`Database`* with a different signature. Do not confuse them.

`db.rs` uses `sqlx` against `Pool<Sqlite>`; existing methods return `Result<_, sqlx::Error>`, and
migrations run through `pub async fn run_migrations` at `db.rs:126`.

**The two remedies you are explicitly permitted to choose between.** Exactly one, and you must state
which and why:

1. **Implement the four methods** on `Database` in `eustress/crates/backend/src/db.rs`, including
   whatever schema the balance and purchase tables require, wired through the existing migration
   path. Choose this if the marketplace purchase route is a capability the project intends to keep.
2. **Remove `"crates/backend"` from the `members` list** in `eustress/Cargo.toml`. Choose this if
   the crate is unfinished scaffolding that no shipped surface depends on. If you choose this you
   must first establish, and record, that no retained workspace member depends on
   `eustress-backend` — a member that does would then fail to resolve, turning one broken build into
   two.

Both are legitimate. Neither is the default. The artifact must name the choice, the evidence, and
what the project loses by it.

**What you may NOT do.** Do not use `#[allow(dead_code)]`, `todo!()`, `unimplemented!()`, or a
`#[cfg(feature = "...")]` gate to make the call sites vanish so the compiler goes quiet while
`marketplace.rs` still calls methods that do not exist. Do not delete `marketplace.rs`. Do not
comment out the route at `main.rs:124` while leaving the handler in place. Each of those produces a
green build and a crate that is more dishonest than the broken one.

**Money-rail constraint, and it is a hard one.** `00_MASTER_PROTOCOL.md` §6 makes "any spend, any
pricing decision, any signed agreement" a human-only decision, and §3.1 records that the Bliss
balance ledger is at 0%. If you choose remedy 1 you are making a *compilation* fix, not a
*financial* one. The four methods must be implemented such that no real balance can move: the
purchase path operates against the local SQLite schema only, calls no external payment provider, and
is presented nowhere as a working money rail. Say so in the artifact.

**Build reality.** A full workspace build takes considerably longer than the 10–15 minutes an engine
build takes, because it compiles every member. Only one cargo build may run at a time against the
shared `target/`; concurrent builds produce link failures (LNK2001, or SAC os error 4551 on
Windows). **Never kill a build mid-compile** — a cancelled build costs more than the build, and
recovery is `cargo clean -p <crate>`. Validate with `cargo build` / `cargo run`, not `cargo check`;
`cargo check` does not link and will not surface the failures this item is most likely to produce.

**Where cargo must be run.** All cargo commands run from the `eustress/` directory — that is where
the workspace `Cargo.toml` lives. `.github/workflows/ci.yml:26` records this constraint for CI.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/backend/src/db.rs`
- `eustress/crates/backend/src/marketplace.rs` — only to align call sites with the method names you
  implement, for example `find_marketplace_item_by_id` becoming `get_marketplace_item_by_id`
- `eustress/crates/backend/migrations/` — only if remedy 1 needs a schema addition
- `eustress/crates/backend/Cargo.toml` — only if remedy 1 genuinely requires a dependency, and say why
- `eustress/Cargo.toml` — **only** the `[workspace] members` list, and only under remedy 2

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- Any crate other than `eustress/crates/backend` — a second broken member is an escalation, not a
  licence; see the front-matter `escalation`
- `eustress/crates/bliss/` — the Bliss ledger is a human-decision surface
  (`00_MASTER_PROTOCOL.md` §6.5) and this item does not touch it

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Building a subset of members, adding `--exclude`, gating the crate behind a non-default feature so
  the default build skips it, or narrowing to `cargo build -p eustress-backend` are all measurement
  changes. The criterion is `cargo build --workspace` and it stays that. If the measurement is
  genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Do not silence the errors. `todo!()`, `unimplemented!()`, an `#[allow(...)]`, a stub returning
  `Ok(Default::default())`, or a `#[cfg]` that excludes the call sites all produce a passing number
  and a worse crate. If remedy 1's methods cannot be honestly implemented, that is evidence *for*
  remedy 2 — take remedy 2 and say so.
- Batch your builds. Six slots against a full workspace build is the binding constraint. Iterate
  with `cargo build -p eustress-backend` and spend the expensive `--workspace` builds only on
  confirmation.
- Fix the first error class, rebuild, and read the next. Rust stops reporting downstream errors once
  a type fails to resolve, so the four missing methods may not be the only thing wrong with the
  crate. Expect at least one more round.

## 5. Exit criterion

### Criterion
`cargo build --workspace`, run from `eustress/`, exits with code **0**, and the emitted artifact
records which of the two permitted remedies was chosen along with the count of workspace members
compiled.

### Measurement

Command (run from the repository root; the directory change is part of the command because cargo
must run inside `eustress/`):

    cd eustress && cargo build --workspace > build.log 2>&1 ; echo "BUILD_EXIT=$?" ; tail -n 40 build.log

Then record the member count:

    cd eustress && cargo metadata --no-deps --format-version 1 > meta.json ; echo "META_EXIT=$?"
    python -c "import json;print('MEMBERS=%d'%len(json.load(open('eustress/meta.json'))['packages']))"

Expected output shape:

    BUILD_EXIT=0
        Finished `dev` profile [unoptimized + debuginfo] target(s) in 812.44s
    META_EXIT=0
    MEMBERS=44

Pass condition:

    BUILD_EXIT == 0

Read the exit code. The presence of the word `Finished` in the log is **not** the pass condition — a
warning stream can contain it and a partial build can print it for a subset. Grep for the literal
`BUILD_EXIT=0`. `MEMBERS` is recorded in the artifact for provenance; under remedy 2 it will be one
lower than under remedy 1, and that difference must be visible.

## 6. Critic gate

`critic_gate` is `[]`. This item ships no visual change and produces no frames; a blinded evaluator
has nothing to score. The mechanical criterion in §5 replaces the gate and is correspondingly tight:
a process exit code of exactly 0 on the full workspace, with no `--exclude` and no feature gating.
There is no partial credit and no "compiles except for" outcome.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — implement the four Database methods in backend/src/db.rs
   -> if still failing, MANDATORY approach change. Renaming a method or adjusting a signature is
      NOT an approach change; removing crates/backend from the workspace members list is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with BUILD_EXIT still non-zero AND the count of distinct
                  compiler error codes in the output moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a crate other than eustress/crates/backend fails to compile (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the criterion to a named
member subset, stating exactly which downstream items lose their evidence base as a result; FUND a
specific approach D; DEFER behind a named blocking item; or KILL, stating that the program then has
no compiling workspace and every build-evidence claim in `B2` and `T5` is void.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.01/workspace_build.json`

A reader finds: the literal command run and its exit code; the workspace member count before and
after; which remedy was chosen (`implement_methods` or `remove_member`) and a one-paragraph
justification; for remedy 1, the four method signatures added and the migration file if any; for
remedy 2, the dependency scan that established no retained member depends on `eustress-backend`,
plus a statement of what the project loses; the commit; and the toolchain version from
`rustc --version`. This file is the W3 evidence for the item, and it is what `T5` `G7.34` and `B2`
`G1.40` cite as the reason their own build evidence is trustworthy.

## 9. Definition of NOT done

- `cargo build -p eustress-backend` passes and `cargo build --workspace` does not. The criterion is
  the workspace, because a second broken member is exactly the failure this item exists to expose.
- The build passes because the four call sites were replaced with `todo!()`, or because the methods
  return `Ok(Default::default())`. That is a quieter compiler and a more dishonest crate.
- The build passes because `crates/backend` was moved behind a non-default feature or an
  `--exclude`. That is a measurement change and fails the item outright.
- Remedy 2 was chosen and a retained member depends on `eustress-backend`, so the next
  `cargo build --workspace` fails on an unresolved path dependency. The dependency scan is not
  optional.
- Remedy 1 was chosen and `purchase_item` now moves a real balance, or calls an external payment
  provider. That is a money-rail change and a human-only decision; this item is a compilation fix.
- The build exits 0 but only after `cargo clean`, and a subsequent incremental build fails. Run the
  measurement twice and say so in the result block.
- The artifact says "fixed the backend crate" without naming the remedy. The remedy and its
  justification are the substance of the artifact, not decoration on it.
````

---

## G1.02 — Pinned reference machine `RM-1` and the manifest hardware block

Item path: `docs/PROMPTS/items/G1.02_reference-machine-pin.md`

````markdown
---
id: G1.02
title: Pinned reference machine RM-1 and the manifest hardware block
workload: W3
workload_secondary: []
phase: G1
depends_on: []
blocks: [G1.09, G1.12, G1.14]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/harness/reference_machines.json
escalation: >
  If the live GPU driver version cannot be read programmatically on this platform, STALL rather than
  hand-transcribing it from a settings dialog. A hardware pin whose provenance is a human reading a
  screen is not a pin, because nothing detects it drifting.
status: DRAFT
notes: >
  Tier S: no compile, no build slots. The item writes three files and runs a verification command
  that queries the live machine. It is deliberately independent of G1.01 so it can run while a
  workspace build occupies the only build slot.
---

## 1. Objective

Exactly one machine identifier, `RM-1`, exists in the repository with a complete, machine-readable
hardware descriptor, and a verification command proves that descriptor still matches the live
machine. Every frame-cost threshold in the prompt library that says "on the harness reference GPU"
resolves to `RM-1` and is therefore falsifiable.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian.

**The defect this item closes.** `docs/PROMPTS/packs/T1_rendering_and_content.md` sets hard
escalation triggers and exit thresholds against "the harness reference GPU":

| Line | Threshold |
|---|---|
| `T1_rendering_and_content.md:1221` | added cost at most 2.0 ms/frame at 3840x2160 |
| `:1300` | added cost at most 2.0 ms/frame at 3840x2160 |
| `:1535` | added cost at most 1.5 ms |
| `:2119` | measured cost at most 2.5 ms/frame at 3840x2160 |
| `:2216` | added cost at most 2.5 ms |
| `:2441` | added cost at most 2.0 ms |
| `:4275` | `ft_p99_ms` at most 25.0 at 3840x2160 |

No machine is named anywhere in the repository. A millisecond budget without a named machine is not
a threshold. Until `RM-1` exists, none of those seven can be met or missed, which means none of them
gates anything.

**What the manifest already demands.** `docs/PROMPTS/02_CAPTURE_HARNESS.md` §7.2 defines a
`hardware` block on every bundle manifest with the keys `cpu`, `gpu`, `gpu_driver`, `ram_gb`, `os`.
§7.3 makes an external-reference control **inadmissible** unless the full `hardware` block is
populated — the Critic rejects the bundle rather than scoring it. This item defines the canonical
source of those values, so a manifest writer fills them from a registry rather than from a guess.

**What is known about the current operator machine.** `MEASURED` on 2026-08-07 on the operator's
Windows box via `Get-CimInstance Win32_VideoController` and `Get-CimInstance Win32_OperatingSystem`:
GPU `NVIDIA GeForce GTX 1080 Ti`, GPU driver `32.0.15.8253`, OS `Windows 11 Pro`, OS build
`10.0.26200`. `cpu` and `ram_gb` were not captured in that reading; this item must measure them
rather than infer them.

**What `RM-1` is not.** `RM-1` is not a claim that this is a good machine, a representative machine,
or the machine those thresholds were calibrated against. It is a claim that this is *the* machine
the numbers refer to. Whether a 2017-generation GPU is the right basis for a 1.5 ms budget is a
floor question, and `00_MASTER_PROTOCOL.md` §5.3 routes floor changes to the human as a `LOWER`
decision. Do not adjust any threshold in any pack. Record the pin and stop.

**Platform.** Windows 11 Pro, build 10.0.26200. PowerShell 7+ is available, and so is Git Bash. The
verification command below is PowerShell because `Win32_VideoController` is the only reliable
programmatic source of the driver version on this platform.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/harness/reference_machines.json` — new file, the registry
- `docs/PROMPTS/harness/RM-1.md` — new file, the human-readable descriptor and its provenance
- `docs/PROMPTS/harness/verify_reference_machine.ps1` — new file, the verification script

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- **Any file under `docs/PROMPTS/packs/`** — the seven frame-cost thresholds stay exactly as
  authored; re-basing one is a floor change and a human decision
- `docs/PROMPTS/02_CAPTURE_HARNESS.md` — the manifest schema is normative and this item consumes it

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Marking a field optional because it was inconvenient to read, comparing only the GPU name and not
  the driver version, or relaxing the match to a substring are all measurement changes. If a field
  genuinely cannot be read programmatically, report `EXIT_CRITERION_UNMEASURABLE` naming the field
  and stop.
- Every value in the registry is read from the machine by a command recorded alongside it. No field
  is transcribed from a settings dialog, a product page, or memory.
- The registry is a list, not a single object, and the schema must already accommodate `RM-2`. A
  second machine is a live possibility — see `G1.13` — and a schema that has to change to admit one
  is a schema that will not be changed in time.
- Do not adjust any millisecond threshold anywhere. Not one.

## 5. Exit criterion

### Criterion
`docs/PROMPTS/harness/reference_machines.json` contains an entry with `"id": "RM-1"` in which all
**eleven** required fields are non-empty, and the live machine's GPU name and GPU driver version
match that entry exactly, string for string.

The eleven required fields: `id`, `cpu`, `gpu`, `gpu_driver`, `ram_gb`, `os`, `os_build`,
`wgpu_backend`, `rustc_version`, `recorded_by`, `recorded_utc`.

### Measurement

Command:

    pwsh -NoProfile -File docs/PROMPTS/harness/verify_reference_machine.ps1 -Id RM-1 ; echo "RM_EXIT=$?"

The script is authored by this item and must perform exactly this:

    $reg  = Get-Content docs/PROMPTS/harness/reference_machines.json -Raw | ConvertFrom-Json
    $m    = $reg.machines | Where-Object { $_.id -eq $Id }
    $req  = 'id','cpu','gpu','gpu_driver','ram_gb','os','os_build','wgpu_backend','rustc_version','recorded_by','recorded_utc'
    $missing = $req | Where-Object { -not $m.$_ }
    $live = Get-CimInstance Win32_VideoController | Select-Object -First 1
    Write-Output ("missing="      + ($missing -join ','))
    Write-Output ("gpu_match="    + ($m.gpu        -eq $live.Name))
    Write-Output ("driver_match=" + ($m.gpu_driver -eq $live.DriverVersion))
    if ($missing.Count -eq 0 -and $m.gpu -eq $live.Name -and $m.gpu_driver -eq $live.DriverVersion) { exit 0 } else { exit 1 }

Expected output shape:

    missing=
    gpu_match=True
    driver_match=True
    RM_EXIT=0

Pass condition:

    RM_EXIT == 0

Read the exit code and the three printed values. The existence of `reference_machines.json` is not a
pass — a registry with an empty `gpu_driver` field exits 1, and must.

## 6. Critic gate

`critic_gate` is `[]`. This item produces no frames and changes no rendered output; there is nothing
for a blinded evaluator to score. The mechanical criterion in §5 replaces the gate and is
correspondingly tight: eleven fields non-empty and two exact string matches against a live hardware
query — not a substring match, and not a human attestation.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — CIM/WMI queries (Win32_VideoController, Win32_Processor,
                 Win32_ComputerSystem, Win32_OperatingSystem) as the field source
   -> if still failing, MANDATORY approach change. Rewording a query is NOT an approach change;
      sourcing the fields from a wgpu adapter enumeration instead is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same field still unreadable
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: the live GPU driver version cannot be read programmatically (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the required field set to a
named subset, stating which of the seven frame-cost thresholds becomes unfalsifiable as a result;
FUND a specific approach D; DEFER behind a named blocking item; or KILL, stating that every "harness
reference GPU" threshold in the library then remains unfalsifiable.

## 8. Artifact

`docs/PROMPTS/harness/reference_machines.json`

A reader finds a `machines` array whose `RM-1` entry carries all eleven fields, and for each field
the literal command that produced its value. Alongside it, `docs/PROMPTS/harness/RM-1.md` states in
prose what `RM-1` is, that it is the operator's own box, that every "harness reference GPU"
threshold in the prompt library resolves to it, and — explicitly — that pinning it is not a
judgement that those thresholds are achievable on it. This file is the W3 evidence for the item;
`G1.09` reads it to populate the manifest `hardware` block, and `G1.12` requires every recipe to
name it.

## 9. Definition of NOT done

- The GPU name matches and the driver version is absent or stale. The driver version is the field
  that actually moves, which is why it is the one checked against the live machine.
- The registry is a single object rather than a list, so `RM-2` cannot be added without a schema
  change. `G1.13` may well need `RM-2`.
- `cpu` or `ram_gb` was inferred from the GPU model, from a previous document, or from the operator
  rather than measured. Every field carries the command that produced it.
- A threshold in `T1_rendering_and_content.md` was adjusted "to match the pinned hardware". That is a
  floor change, it is out of scope, and it fails the item.
- The script exits 0 because it compares the registry against itself rather than querying the live
  machine. The whole value of the check is that it detects drift.
- `RM-1.md` reads as a specification of a machine the project intends to acquire. It documents the
  machine that exists.
````

---

## G1.03 — Harness scene set `S1`–`S6` and the seeded scene generator

Item path: `docs/PROMPTS/items/G1.03_harness-scene-set.md`

````markdown
---
id: G1.03
title: Harness scene set S1-S6 emitted by a seeded generator
workload: W3
workload_secondary: [W1]
phase: G1
depends_on: [G1.01]
blocks: [G1.04, G1.09, G1.12, G0.03, G2.01, G3.01, G4.02, G5.23, G7.03, G7.37]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.03/scene_freeze.json
escalation: >
  If a scene cannot be emitted deterministically because a Space-writing code path embeds a
  timestamp, a UUID, a hash-map iteration order, or an absolute machine path into the on-disk
  representation, STALL immediately with the field named and its path:line. Do not strip the field
  from the comparison; a Space directory that does not hash stably cannot be frozen, and a scene
  that cannot be frozen cannot be a harness scene.
status: DRAFT
notes: >
  Tier L: six scenes across the render, studio, and physics surfaces, plus a new generator binary,
  plus a freeze record. The build cost is the binding constraint, not the token count.
---

## 1. Objective

Six harness Spaces exist under `eustress/spaces/harness/`, each emitted by a seeded generator binary
rather than hand-placed, each hashing to a recorded, stable SHA-256, and each carrying its normative
scene id from `02_CAPTURE_HARNESS.md` §3. Running the generator twice with the same seed produces
byte-identical Space directories, which is what makes "frozen scene" a checkable property rather
than a promise.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine.
Rendering and the ECS are implementation details in service of a world model an AI reasons over and
a human edits. The licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian, never
Rapier. Units are meter-native; studs are a display unit only.

**Nothing under `eustress/spaces/` exists today.** You are creating that directory tree.

**The six scenes are normatively specified** at `docs/PROMPTS/02_CAPTURE_HARNESS.md` §3. Restated
here in full so you need not open it:

| Scene id | Purpose | Content requirements |
|---|---|---|
| `S1` | material and lighting | 8x5 grid of identical spheres, roughness 0.0 to 1.0 across one axis and metallic 0 to 1 across the other; three fixed light sources at declared positions; neutral 18% grey ground; one calibration chart object with known albedo patches |
| `S2` | material, coherence | enclosed room 6 m x 8 m x 3 m; one window admitting a directional source through an aperture; two artificial sources of differing colour temperature; objects spanning metal, dielectric, thin-shell, and emissive |
| `S3` | material, motion, coherence | open terrain, a single directional source at a declared elevation and azimuth, no sky-dome cheat; objects casting shadows across occluder distances from 0.2 m to 20 m |
| `S4` | studio UI | the studio at a pinned Space with a pinned selection, a pinned panel layout, and pinned window sizes |
| `S5` | motion, simulation | mass ratios spanning 1000:1, a stack settling under gravity, a joint chain, a high-restitution impact, and a resting body held for 600 ticks to expose jitter; Avian, fixed-step 60 Hz, `SubstepCount(6)` |
| `S6` | simulation, vertical proof | the domain simulation under test, with its recorder configured and its independent step counter armed; bound per item by that item's prompt |

**On-disk directory names, and why they are not simply `S1`..`S6`.** Eleven already-authored items
cite exact paths beneath `eustress/spaces/harness/`. Renaming them would break those items. The
resolution, which is a decision this item implements and does not re-open:

| Scene id | On-disk directory | Why this name |
|---|---|---|
| `S1` | `eustress/spaces/harness/RH1_sphere_grid` | cited by `T1_rendering_and_content.md` |
| `S2` | `eustress/spaces/harness/RH2_interior` | cited by `T1_rendering_and_content.md` |
| `S3` | `eustress/spaces/harness/RH3_exterior` | cited by `T1_rendering_and_content.md` |
| `S4` | `eustress/spaces/harness/S4_studio_ui` | cited by name at `T3_studio_ux.md:1069` |
| `S5` | `eustress/spaces/harness/S5_physics_stress` | no prior citation; use the normative id |
| `S6` | `eustress/spaces/harness/S6_domain_sim` | no prior citation; use the normative id |

The manifest records the normative `scene_id` (`S1`..`S6`); the recipe records the `space_dir`. Both
appear, so neither naming convention is lost.

**What is NOT yours.** `RH4_cascade`, `RH5_hlod`, `RH6_roundtrip`, `RH7_splat_composite`,
`RH8_opening`, and `FLAGSHIP` are also cited under `eustress/spaces/harness/`, by
`T1_rendering_and_content.md`. They are render-specific extension scenes and each has a T1 item that
owns it. Do not author them. Do not delete or reserve their directory names either — just leave
room.

**The precedent to follow exactly.** `eustress/crates/engine/src/bin/generate_benchmark_map.rs` (494
lines) is registered as the bin `generate-benchmark-map` at `eustress/crates/engine/Cargo.toml:23-25`.
It is the in-tree pattern for a seeded generator binary whose emitted Space is a derived artifact.
Follow it: the generator source is the source of truth, the Space directory is output.

**The scene freeze rule**, from `02_CAPTURE_HARNESS.md` §3, restated: a harness scene's Space
directory hash is recorded in the manifest. If the hash changes, the scene id must change (`S1`
becomes `S1b`). Never silently edit a frozen scene — that retroactively falsifies every archived
comparison that used it.

**Determinism pins that already exist.** `eustress/crates/engine/src/main.rs` sets
`Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`, and a `SolverConfig`.
`eustress/crates/common/src/physics/determinism.rs` (56 lines) holds a `GlobalRngSeed` resource and
nothing else. `S5` must be authored against those pins, not against defaults it sets itself.

**Build reality.** A full engine build takes 10–15 minutes, one cargo build at a time against the
shared `target/`, never killed mid-compile. Validate with `cargo run`, not `cargo check`. Twelve
build slots is roughly three hours of pure compile and is the real ceiling on this item.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/bin/harness_scenes.rs` — new file, the seeded generator
- `eustress/crates/engine/Cargo.toml` — only to register the new `[[bin]]`
- `eustress/spaces/harness/` — new directory; generator output only, never hand-edited
- `.gitignore` — only to exclude generated Space payloads if their size warrants it, and say why

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/bin/generate_benchmark_map.rs` — it is the pattern to copy, not to change
- Anything under `eustress/crates/common/src/physics/` — `S5` consumes the existing determinism and
  solver pins; it does not adjust them
- `eustress/spaces/harness/RH4_cascade`, `RH5_hlod`, `RH6_roundtrip`, `RH7_splat_composite`,
  `RH8_opening`, `FLAGSHIP` — owned by T1 items

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Hashing only a subset of the Space directory, excluding a file type from the hash, normalising a
  field out of the comparison, or emptying a scene until it hashes stably are all measurement
  changes. If a Space-writing path embeds genuinely unstable data, report
  `EXIT_CRITERION_UNMEASURABLE` with the `path:line` and stop.
- Hand-placing any object fails the item. Every object's position, rotation, scale, and material is
  computed by the generator from the seed. A scene a Critic cannot regenerate is not reproducible,
  and a later item that needs to add a probe object to `S1` must be able to do it by changing the
  generator, not by opening the studio.
- `S5` must run against the existing fixed-step and substep pins. Do not set your own physics
  configuration inside the scene; a scene that configures physics differently from the engine under
  test measures the scene, not the engine.
- `S6` ships as a named, documented domain simulation with its recorder configured. It is bound per
  item by later prompts, but it must be a real, running simulation now, not an empty placeholder.
- Batch verification. One build should validate all six generators; do not spend a build per scene.

## 5. Exit criterion

### Criterion
The generator emits all **six** scenes; a second run with the same seed produces byte-identical
Space directories for all six; and each scene's recorded SHA-256 in the freeze record equals the
hash recomputed from disk.

### Measurement

Command:

    cd eustress && cargo run --release --bin harness-scenes -- \
        --seed 42 --out spaces/harness --all ; echo "GEN_A_EXIT=$?"

    cd eustress && cargo run --release --bin harness-scenes -- \
        --seed 42 --out spaces/harness --all --verify-freeze \
        --record ../docs/PROMPTS/artifacts/G1.03/scene_freeze.json ; echo "GEN_B_EXIT=$?"

`--verify-freeze` re-emits every scene into a temporary directory, hashes both trees, compares them
scene by scene, writes the freeze record, and exits non-zero if any scene differs.

Expected output shape:

    GEN_A_EXIT=0
    S1 RH1_sphere_grid       sha256:4c1e...  identical
    S2 RH2_interior          sha256:9ab0...  identical
    S3 RH3_exterior          sha256:71d3...  identical
    S4 S4_studio_ui          sha256:2f88...  identical
    S5 S5_physics_stress     sha256:e05c...  identical
    S6 S6_domain_sim         sha256:be47...  identical
    scenes=6 identical=6 differing=0
    GEN_B_EXIT=0

Pass condition:

    GEN_A_EXIT == 0  AND  GEN_B_EXIT == 0  AND  scenes == 6  AND  differing == 0

Read `scenes`, `differing`, and both exit codes. Six directories existing on disk is not a pass —
the second, verifying run is the measurement.

## 6. Critic gate

`critic_gate` is `[]`. The scenes are the *input* to every later Critic bundle, not something a
blinded evaluator scores. Scoring a harness scene at this stage would also be circular: an item that
authors the fixture cannot be judged against captures of that fixture. The mechanical criterion in
§5 replaces the gate and is correspondingly tight: byte-identical regeneration across all six
scenes, not five, and not "identical apart from".

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — one generator binary emitting all six Spaces through the existing
                 Space-writing path, with an explicit deterministic ordering pass
   -> if still failing, MANDATORY approach change. Changing the seed or reordering one loop is NOT
      an approach change; emitting a canonical serialised form and writing that, rather than
      round-tripping through the live Space writer, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same scene still differing AND the count of
                  differing files within it moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a Space-writing path embeds a timestamp, UUID, iteration-order-dependent field,
                   or absolute machine path (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the scene set to a named
subset, listing which packs lose their fixture as a result; FUND a specific approach D; DEFER behind
a named blocking item; or KILL, stating that the program then has no frozen scenes and every
comparison in the library is between two things that were never the same.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.03/scene_freeze.json`

A reader finds, for each of the six scenes: the normative scene id, the on-disk directory, the
SHA-256 of the Space directory, the generator seed, the SHA-256 of
`eustress/crates/engine/src/bin/harness_scenes.rs` at emission time, the file count and byte size,
the commit, and the verdict from the regeneration comparison. This file is the W3 evidence for the
item and is what every recipe in `G1.12` and every manifest written by `G1.09` cites when it claims
its scene was frozen.

## 9. Definition of NOT done

- Five scenes regenerate identically and one does not. Six of six, or the item is not done.
- The scenes were authored in the studio and exported. A hand-built scene cannot be regenerated by a
  Critic or amended by a later item, which defeats the entire point of the generator.
- The hashes are stable because the generator writes a canonical file that the engine then ignores,
  so the Space the engine actually loads differs from the Space that was hashed. The hashed tree must
  be the loaded tree.
- `S5` settles because its solver configuration differs from the engine's. It must run against the
  existing 60 Hz fixed step and `SubstepCount(6)`.
- `S6` is an empty Space with a recorder attached. It must be a real domain simulation that produces
  a non-trivial recording.
- The freeze record exists but records hashes that were computed once and never re-verified. The
  second run is the measurement; the first run is just setup.
- `S4` pins a panel layout that the studio does not restore on load, so the "pinned" layout silently
  varies. Load it twice and confirm before recording the hash.
````

---

## G1.04 — Camera path files `CP-A/B/S/U` and the path player

Item path: `docs/PROMPTS/items/G1.04_camera-paths.md`

````markdown
---
id: G1.04
title: Camera path files CP-A/B/S/U and the tick-driven path player
workload: W3
workload_secondary: [W1]
phase: G1
depends_on: [G1.03]
blocks: [G1.09, G6.02]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.04/path_replay.json
escalation: >
  If a pose set through the existing ai_camera.set_pose bridge call does not read back bit-identical
  on the following tick, STALL immediately with the divergent component named. Interpolating,
  smoothing, or snapping the pose to hide the divergence is forbidden — an unrepeatable camera makes
  every frame-to-frame comparison in the library meaningless.
status: DRAFT
notes: >
  Tier M. The path player is one module and four JSON files; the cost is engine builds, not tokens.
  Deliberately separated from G1.06 (tick trigger) so a pose-replay failure and a capture-timing
  failure produce distinguishable signatures.
---

## 1. Objective

Four camera path files exist at `eustress/spaces/harness/paths/`, each enumerating exact poses
against exact tick indices, and a path player drives the AI camera through any of them from the
fixed-step schedule. Replaying a path twice produces a pose log whose every component matches bit for
bit, so "fixed camera path" is a measured property rather than an intention.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian. Units are meter-native.

**The invariant this item satisfies.** `docs/PROMPTS/02_CAPTURE_HARNESS.md` invariant **I2**:
"Fixed camera path. Pose per frame is a declared list of exact positions and orientations, not an
interactive fly-through," because "human camera motion is unrepeatable."

**The four paths are normatively specified** at `02_CAPTURE_HARNESS.md` §4.1, restated in full:

| ID | Name | Ticks | Shape |
|---|---|---|---|
| `CP-A` | `first_impression` | 0–179 (3.0 s at 60 Hz) | slow dolly toward the composed hero framing; frames tagged `role: first_impression`, and this is the **only** set the Critic sees for the first-three-seconds dimension |
| `CP-B` | `orbit_survey` | 0–599 | full 360-degree orbit at fixed radius and pitch |
| `CP-S` | `static_studies` | single tick each | enumerated fixed poses, one per material or lighting study |
| `CP-U` | `ui_sequences` | event-indexed | not a camera path — full-window captures at declared interaction steps, per `02` §4.3 |

`CP-U` is included in this item because the recipe format must be able to name it uniformly, but it
carries interaction steps rather than poses. The four sequences, from `02` §4.3: `panel_open`
(idle, hover panel tab, click, panel settled — 4 steps); `entity_select` (idle, hover entity, click,
selection settled with gizmo — 4 steps); `property_edit` (select, focus field, type value, commit,
**re-read after a reload** — 5 steps); `error_surface` (trigger a known-invalid action, error
appears, error dismissed — 3 steps). Each is captured at 1920x1080, 2560x1440, and 3840x2160.

The fifth step of `property_edit` is deliberate and must not be dropped as redundant: it exists to
make a non-persisting property edit visible to a blinded evaluator instead of invisible.

**No interpolation at capture time.** `02` §4.1: poses are enumerated, and the harness sets each one
before capturing. If a path file declares 180 poses, the player sets 180 poses; it does not declare
two and interpolate 178.

**What already exists and must not be rebuilt.** Pose control works today through the engine bridge:
`ai_camera_set_pose` (`position` as `[x,y,z]`, plus either `look_at` as `[x,y,z]` or `rotation` as
`[x,y,z,w]`), `ai_camera_orbit`, and `ai_camera_frame`, at
`eustress/crates/mcp-server/src/bridge_tools.rs:1292,1322,1352`. The bridge writes its TCP port to
`<universe>/.eustress/engine.port` (`eustress/crates/engine/src/engine_bridge/mod.rs:29`); that file
is how an external process finds a running engine. The protocol is JSON-RPC over that socket.

**A hazard you must respect.** `eustress/crates/engine/src/ai_camera.rs:133-149` documents a hard
wgpu abort: two `Camera3d`s both carrying Bevy `Atmosphere` hit a multi-camera atmosphere
prepare-race and wgpu aborts. The AI camera therefore carries
`eustress_common::plugins::lighting_plugin::NoAtmosphere`. Keep it.

**The scenes exist.** `G1.03` delivered six frozen harness Spaces under `eustress/spaces/harness/`
(`RH1_sphere_grid`, `RH2_interior`, `RH3_exterior`, `S4_studio_ui`, `S5_physics_stress`,
`S6_domain_sim`) from a seeded generator. Author the poses against those, and do not modify them —
editing a frozen scene invalidates its hash.

**Build reality.** 10–15 minutes per engine build, one at a time against the shared `target/`, never
killed mid-compile. Validate with `cargo run`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/camera_path.rs` — new module, the path player
- `eustress/crates/engine/src/lib.rs` or the module tree that declares it — only to add `mod camera_path;`
- `eustress/spaces/harness/paths/CP-A.json`, `CP-B.json`, `CP-S.json`, `CP-U.json` — new files
- `eustress/crates/engine/src/bin/harness_scenes.rs` — only to add a `paths` subcommand that emits
  and verifies the path files

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/spaces/harness/RH1_sphere_grid`, `RH2_interior`, `RH3_exterior`, `S4_studio_ui`,
  `S5_physics_stress`, `S6_domain_sim` — frozen by `G1.03`
- `eustress/crates/engine/src/ai_camera.rs` — resolution and capture parameterisation belong to
  `G1.05`; this item only *sets poses* through the existing interface
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Comparing poses with a tolerance, comparing only position and not orientation, sampling every tenth
  tick, or shortening a path are all measurement changes. If bit-identical pose replay is genuinely
  unachievable for a named, cited reason, report `EXIT_CRITERION_UNMEASURABLE` and stop.
- Do not interpolate. A path file is an enumeration. If `CP-A` needs 180 distinct poses, it contains
  180 distinct poses, and the generator that computes them is deterministic.
- Do not drive the camera from wall-clock time or from a frame counter. The player advances on the
  fixed-step schedule; that is the same reason `02` invariant **I4** exists.
- Keep `NoAtmosphere` on the AI camera. Removing it reintroduces a documented hard wgpu abort.
- Batch verification. One build should validate all four path files and the player.

## 5. Exit criterion

### Criterion
Replaying `CP-A`, `CP-B`, and `CP-S` twice each against harness scene `RH3_exterior` produces pose
logs that are **bit-identical** across the two runs for every tick, and the tick count in each log
equals the tick count declared in that path's JSON file (180, 600, and the enumerated pose count
respectively).

### Measurement

Command:

    cd eustress && cargo run --release --bin harness-scenes -- \
        paths --verify --out ../docs/PROMPTS/artifacts/G1.04/path_replay.json \
        --space spaces/harness/RH3_exterior \
        --paths CP-A,CP-B,CP-S ; echo "PATH_EXIT=$?"

`--verify` launches the engine on the named Space, replays each path twice, logs the pose actually
read back from the AI camera transform on each tick, compares the two logs component by component as
raw `f32` bit patterns, and exits non-zero on any mismatch or on any tick-count mismatch.

Expected output shape:

    CP-A ticks_declared=180 ticks_replayed=180 run_a_sha256=1b7c... run_b_sha256=1b7c... identical=true
    CP-B ticks_declared=600 ticks_replayed=600 run_a_sha256=af02... run_b_sha256=af02... identical=true
    CP-S ticks_declared=24  ticks_replayed=24  run_a_sha256=c930... run_b_sha256=c930... identical=true
    paths=3 identical=3 tick_mismatches=0
    PATH_EXIT=0

Pass condition:

    PATH_EXIT == 0  AND  identical == 3  AND  tick_mismatches == 0

Read the three fields and the exit code. The path JSON files existing is not a pass; the replay
comparison is.

`CP-U` is excluded from this criterion because it is event-indexed rather than tick-indexed and its
replay is exercised by the UI capture path in `G1.09`. Its file must still be authored and must
still parse — `paths --verify` validates its schema and step counts without replaying it, and a
schema failure exits non-zero.

## 6. Critic gate

`critic_gate` is `[]`. A camera path is a fixture, not a rendered result; there is nothing here for a
blinded evaluator to score, and scoring the fixture that produces the evidence would be circular.
The mechanical criterion in §5 replaces the gate and is correspondingly tight: bit-identical `f32`
comparison of every pose component across two runs, not an epsilon comparison.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — enumerate poses in JSON and set each one through the existing
                 ai_camera.set_pose path from the fixed-step schedule
   -> if still failing, MANDATORY approach change. Changing a pose value or the settle count is NOT
      an approach change; writing the camera transform directly from the path player, bypassing the
      bridge call, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same path still non-identical AND the count of
                  differing ticks moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a pose set through ai_camera.set_pose does not read back bit-identical on the
                   following tick (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER bit-identity to a stated
per-component epsilon, naming the downstream comparisons that become approximate as a result; FUND a
specific approach D; DEFER behind a named blocking item; or KILL, stating that the library then has
no repeatable camera and every motion and first-impression comparison is between different shots.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.04/path_replay.json`

A reader finds, per path: the path id and name, the declared tick range, the replayed tick count,
both run hashes, the identity verdict, and for `CP-S` the enumerated pose count. Plus the SHA-256 of
each of the four path JSON files, the scene the replay ran against and its frozen hash from `G1.03`,
the commit, and the reference machine id. `G1.09` cites this file as the reason a recipe naming a
path can claim its frames were taken from the same shot on both sides.

## 9. Definition of NOT done

- The poses replay identically but the tick indices drift, so the same pose lands on a different
  tick on the second run. Both the pose and the tick must match.
- `CP-A` is authored as two keyframes with interpolation between them. It is an enumeration of 180
  poses.
- The player advances on `Update` rather than on the fixed-step schedule, so a slower machine replays
  a different path. Wall-clock indexing is the exact defect invariant I4 exists to prevent.
- `CP-U.json` was skipped because it carries no poses. It must exist, parse, and declare all four
  sequences with their step counts, or `T3`'s eleven UI items have nothing to name.
- The `property_edit` sequence was authored with four steps because the fifth looked redundant. The
  re-read-after-reload step is the point of the sequence.
- Replay is identical because the second run reused the first run's pose log rather than re-driving
  the camera. The second run must genuinely re-drive it.
- `NoAtmosphere` was removed from the AI camera to make a path look better, reintroducing the
  documented wgpu abort.
````

---

## G1.05 — AI camera resolution and pose parameterisation

Item path: `docs/PROMPTS/items/G1.05_ai-camera-parameterisation.md`

````markdown
---
id: G1.05
title: AI camera capture accepts width, height, and output path
workload: W3
workload_secondary: [W1, W6]
phase: G1
depends_on: [G1.01]
blocks: [G1.06, G1.09, G3.01, G3.04, G3.06, G3.07, G3.08, G4.01, G4.02, G4.03, G7.02, G7.03, G7.05, G7.07, G7.09, G7.13, G7.16, G7.17]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.05/capture_parameterisation.json
escalation: >
  If a 3840x2160 off-screen render target cannot be allocated on the pinned reference machine RM-1,
  STALL immediately with the wgpu error quoted verbatim and the reported adapter limits. Do not
  fall back to a lower resolution and declare the item done; every material check in the library is
  specified at 3840x2160 and a silently downscaled capture would corrupt all of them.
status: DRAFT
notes: >
  Tier M: two files plus a bridge schema, but every verification is an engine build. This is
  02_CAPTURE_HARNESS.md build item B1. See the pack note N3 — this item, not T1's G3.01, owns the
  parameterisation.
---

## 1. Objective

A capture can be requested at an arbitrary declared resolution, to an arbitrary declared path, from
the AI camera, over the engine bridge and over MCP. A capture requested at 3840x2160 comes back as a
PNG whose IHDR header reads 3840x2160, and one requested at 1920x1080 comes back at 1920x1080, in
the same session without restarting the engine.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine.
Rendering is an implementation detail in service of a world model an AI reasons over. The licence is
PolyForm Shield 1.0.0; say source-available. Physics is Avian.

**The defect.** `eustress/crates/engine/src/ai_camera.rs:43-44`:

    /// Off-screen render resolution for the AI camera.
    pub const AI_CAM_WIDTH: u32 = 1280;
    pub const AI_CAM_HEIGHT: u32 = 720;

The `Image` render target is created once in `spawn_ai_camera` and never resized; the spawn log line
at `ai_camera.rs:151-154` interpolates those two constants. `pub fn request_capture(state: &mut
AiCameraState, path: PathBuf)` at `ai_camera.rs:158` takes a path and nothing else, storing a
`PendingCapture { path }`. `process_ai_capture` then screenshots the existing image. The MCP wrapper
`ai_camera_capture` at `eustress/crates/mcp-server/src/bridge_tools.rs:1381` sends an empty JSON
object; `capture_viewport` at `bridge_tools.rs:1266` likewise takes an empty input schema.

**Why 720p is not good enough.** `02_CAPTURE_HARNESS.md` §9 build item **B1** states it directly:
720p "is below the resolution at which D2 material checks are decidable". Invariant **I5** requires a
declared, identical resolution on both sides of every comparison, and §5 pins `render.width = 3840`,
`render.height = 2160` in the settings block every manifest records. Every material and lighting
threshold in `T1_rendering_and_content.md` is specified at 3840x2160.

**What already works and must not be rebuilt.** Pose control: `ai_camera_set_pose` (`position`, plus
`look_at` or `rotation`), `ai_camera_orbit`, `ai_camera_frame`, at
`eustress/crates/mcp-server/src/bridge_tools.rs:1292,1322,1352`. The dispatch chain is MCP wrapper →
engine bridge handler in `eustress/crates/engine/src/engine_bridge/protocol.rs` → `request_capture`.
The bridge writes its TCP port to `<universe>/.eustress/engine.port`
(`eustress/crates/engine/src/engine_bridge/mod.rs:29`).

**Two documented hazards you must respect.**

1. `ai_camera.rs:133-149` records a hard wgpu abort: two `Camera3d`s both carrying Bevy `Atmosphere`
   hit a multi-camera atmosphere prepare-race and wgpu aborts with a bind-group descriptor/layout
   mismatch. The AI camera carries `eustress_common::plugins::lighting_plugin::NoAtmosphere` for
   this reason. Keep it.
2. Toggling `Msaa` or `Hdr` at runtime changes the view-bind-group shape and panics against the
   shared mesh-view bind-group layout. If you change either, change it once at spawn, identically on
   both cameras, and never at runtime. A resize is not an `Msaa` change — do not let one become the
   other.

**Do not reallocate every frame.** A per-frame reallocation of a 4K render target will destroy the
frame budget that `G1.08` and every T1 item must measure. Resize on request only, and only when the
requested size differs from the current size.

**The pinned reference machine.** `G1.02` delivered `RM-1` at
`docs/PROMPTS/harness/reference_machines.json`. Its GPU is an NVIDIA GeForce GTX 1080 Ti. Record the
machine id in the artifact; a resolution capability claim is machine-specific.

**Build reality.** 10–15 minutes per engine build, one at a time against the shared `target/`, never
killed mid-compile. Validate with `cargo run`, not `cargo check` — `cargo check` will not catch the
render-target and pipeline failures this item is most likely to produce.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/ai_camera.rs`
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — only the `ai_camera_capture` and
  `capture_viewport` handlers
- `eustress/crates/mcp-server/src/bridge_tools.rs` — only the input schemas for those two tools
- `eustress/crates/bridge-client/src/lib.rs` — only if the client needs the new parameters plumbed

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/default_scene.rs` — the shared `studio_camera_bundle` belongs to later
  render items
- `eustress/crates/engine/src/photoreal.rs` — the post-stack is a separate concern
- Anything under `eustress/crates/common/src/physics/`
- `eustress/spaces/harness/` — the scenes are frozen by `G1.03`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Reading back the *requested* dimensions rather than the emitted PNG header, accepting an
  aspect-ratio match instead of an exact match, upscaling a 720p render to 4K, or capping the
  accepted resolution are all measurement changes. Report `EXIT_CRITERION_UNMEASURABLE` with
  evidence and stop if the measurement is genuinely wrong.
- The emitted PNG must be a genuine render at the requested size, not an upscale. The measurement
  reads the IHDR header, so an upscale will pass the header check — which is precisely why the
  criterion also requires that a 4K capture and a 1080p capture of the same static pose differ in
  fine detail, not merely in pixel count. State how you established that.
- Resize on request only, never per frame, and only when the requested size differs from the current
  size.
- Both resolutions must be achievable in one session, in either order, without restarting the
  engine. A parameterisation that only works on the first request is a constant with extra steps.
- Keep `NoAtmosphere`. Do not toggle `Msaa` or `Hdr` at runtime.
- `capture_viewport` must continue to work. Both capture paths ship today; breaking one to
  parameterise the other fails the item.

## 5. Exit criterion

### Criterion
In a single engine session, four captures requested in the order 3840x2160, 1920x1080, 3840x2160,
2560x1440 each emit a PNG whose IHDR width and height equal the requested values exactly, each to
the distinct requested output path, and the two 3840x2160 captures from the identical pose are
byte-identical to each other.

### Measurement

Command (the engine must already be running on the frozen harness Space; start it first, then run
the probe against the bridge port):

    cd eustress && cargo run --release -p eustress-engine -- \
        --space spaces/harness/RH1_sphere_grid &

    cd eustress && cargo run --release --bin harness-scenes -- \
        capture-selftest \
        --universe spaces/harness \
        --pose-position 6.0 3.5 9.0 --pose-look-at 0.0 1.0 0.0 \
        --sizes 3840x2160,1920x1080,3840x2160,2560x1440 \
        --out ../docs/PROMPTS/artifacts/G1.05/capture_parameterisation.json ; echo "CAP_EXIT=$?"

`capture-selftest` connects to the bridge via `<universe>/.eustress/engine.port`, sets the pose once,
issues the four capture requests in order, reads the IHDR header of each emitted PNG directly from
its first 24 bytes, hashes each file, and exits non-zero on any dimension mismatch or if the two
3840x2160 digests differ.

Expected output shape:

    1 requested=3840x2160 emitted=3840x2160 sha256=6d21... path=.../cap_0.png
    2 requested=1920x1080 emitted=1920x1080 sha256=0af7... path=.../cap_1.png
    3 requested=3840x2160 emitted=3840x2160 sha256=6d21... path=.../cap_2.png
    4 requested=2560x1440 emitted=2560x1440 sha256=b114... path=.../cap_3.png
    captures=4 dimension_mismatches=0 repeat_4k_identical=true
    CAP_EXIT=0

Pass condition:

    CAP_EXIT == 0
      AND dimension_mismatches == 0
      AND repeat_4k_identical == true
      AND captures == 4

Read the emitted dimensions out of the PNG headers and read the exit code. Four files appearing on
disk is not a pass.

## 6. Critic gate

`critic_gate` is `[]`. This item changes no rendered content — only the size of the buffer that
content is rendered into and where it is written. There is nothing for a blinded evaluator to score,
and the item deliberately produces no visual delta. The mechanical criterion in §5 replaces the gate
and is correspondingly tight: exact header equality at three distinct resolutions in one session,
plus byte-identity on the repeated request.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — resize the existing Image render target on request, reusing the
                 spawned AI camera
   -> if still failing, MANDATORY approach change. Adjusting the settle-frame count or the resize
      trigger condition is NOT an approach change; despawning the AI camera and respawning it with
      a freshly created target of the requested size is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with dimension_mismatches unchanged AND no new wgpu or
                  Bevy error class in the log
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a 3840x2160 off-screen target cannot be allocated on RM-1 (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the maximum capture resolution
to a stated value, listing every T1 threshold that becomes unmeasurable as a result; FUND a specific
approach D; DEFER behind a named blocking item; or KILL, stating that the library's material checks
then remain undecidable at the only resolution they were specified at.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.05/capture_parameterisation.json`

A reader finds: the four requested sizes in request order, the four emitted sizes read from the PNG
IHDR headers, the four file digests, the repeat-identity verdict, the pose used, the harness scene
and its frozen hash from `G1.03`, the reference machine id `RM-1` with its GPU and driver from
`docs/PROMPTS/harness/reference_machines.json`, the commit, and a statement of how the agent
established that the 4K capture is a genuine 4K render rather than an upscale. This file is the W3
evidence for the item; `G1.09` cites it as the reason a recipe may declare a resolution at all.

## 9. Definition of NOT done

- The resolution parameter is accepted, echoed back, and the emitted PNG is a 1280x720 render
  upscaled to 3840x2160. Read the header *and* establish genuine detail.
- 3840x2160 works on the first request and every later request returns the first size. One-shot
  parameterisation is a constant with extra steps.
- The requested path is accepted but every capture lands in the working directory. The output path
  is half the item.
- The resize happens every frame, so the frame budget that `G1.08` and every T1 item must measure is
  now dominated by render-target reallocation.
- `capture_viewport` silently broke while `ai_camera_capture` was parameterised. Both paths ship;
  confirm both and say so in the result block.
- `NoAtmosphere` was removed, or `Msaa`/`Hdr` was toggled at runtime to make a resize work,
  reintroducing a documented hard abort.
- The two repeated 4K captures differ. That is a determinism failure and it belongs to `G1.11`, but
  it fails this item's criterion too, and reporting it here rather than deferring it is the correct
  behaviour.
````

---

## G1.06 — Tick-indexed capture trigger

Item path: `docs/PROMPTS/items/G1.06_tick-indexed-capture.md`

````markdown
---
id: G1.06
title: Captures fire at declared simulation tick indices
workload: W3
workload_secondary: []
phase: G1
depends_on: [G1.05]
blocks: [G1.09]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.06/tick_trigger.json
escalation: >
  If a capture cannot be made to fire on the declared tick because the GPU readback completes on a
  later frame than the tick that armed it, STALL with the observed tick offset and its distribution
  rather than declaring a fixed offset acceptable. A constant offset is only harmless if it is
  provably constant, and proving that is a different item.
status: DRAFT
notes: >
  Tier M. The change is a scheduling change in two files; the cost is engine builds. This is
  02_CAPTURE_HARNESS.md build item B2.
---

## 1. Objective

A capture request can name a simulation tick index, and the emitted frame is the frame corresponding
to that tick — not to a wall-clock instant and not to a frame counter. Each emitted PNG carries its
tick index in its filename and in the capture log, and running the same tick list on a deliberately
slowed machine yields the same tick set.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian. Units are meter-native.

**The invariant this item satisfies.** `docs/PROMPTS/02_CAPTURE_HARNESS.md` invariant **I4**:
"Tick-indexed frames. Captures are taken at declared **simulation tick indices**, never at wall-clock
intervals or frame counters," because "wall-clock indexing means a slower machine captures a
different moment."

**The defect, stated precisely.** `02_CAPTURE_HARNESS.md` §9 build item **B2**: "the existing client
burst (`eustress/crates/client/src/systems/frame_capture.rs`, 132 lines) is frame-indexed with a
90-frame arm delay, so two machines capture different moments." That path is driven by
`EUSTRESS_CAPTURE=<count>[@<every_n>]` with `EUSTRESS_CAPTURE_DIR`, plus an F9 hotkey. It is
client-only. The AI camera path (`eustress/crates/engine/src/ai_camera.rs`) has no tick concept at
all — `request_capture` stores a `PendingCapture` and `process_ai_capture` consumes it on whatever
frame runs next.

**Where ticks come from.** `eustress/crates/engine/src/main.rs` pins
`Time::<Fixed>::from_hz(60.0)` with `SubstepCount(6)`.
`eustress/crates/common/src/simulation/clock.rs:83` `advance()` maintains `tick_count`, incrementing
once per drained fixed timestep (`clock.rs:96`). That counter is the tick index this item schedules
against.

**A trap in that same function you must not build on top of.** `clock.rs:100-102` **zeroes the
accumulator** when `ticks >= max_ticks_per_frame`, so under time compression the clock advances
`simulation_time_s` by the full compressed delta while executing fewer steps. This is exactly why
`02` §5.1 requires `sim.time_scale = 1.0` for every capture run. Your trigger must schedule against
the tick counter, and the capture path must refuse to run when `time_scale != 1.0` rather than
producing frames that are silently mis-indexed. The independent step counter that makes the drop
visible is `G1.07`; this item does not build it, but it must not paper over it.

**What already exists.** `G1.05` delivered resolution and output-path parameterisation on
`ai_camera_capture`, so `request_capture` now carries width, height, and path. `G1.04` delivered the
camera path player, which advances on the fixed-step schedule and is where the poses for a tick list
come from. Use both; do not rebuild either.

**Build reality.** 10–15 minutes per engine build, one at a time against the shared `target/`, never
killed mid-compile. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/ai_camera.rs` — the pending-capture queue becomes tick-scheduled
- `eustress/crates/engine/src/simulation/plugin.rs` — only to drive the queue from the fixed-step
  schedule
- `eustress/crates/engine/src/camera_path.rs` — only to arm captures alongside pose changes
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — only the `ai_camera_capture` handler, to
  accept a tick list

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/simulation/clock.rs` — the accumulator-zeroing defect is `G1.07`'s
  instrument and a G2 item's fix; this item reads the tick counter and does not change the clock
- `eustress/crates/client/src/systems/frame_capture.rs` — the client burst is a separate, retained
  path
- Anything under `eustress/crates/common/src/physics/`
- `eustress/spaces/harness/` — frozen by `G1.03`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Accepting a tick tolerance, capturing on the nearest available frame and relabelling it, dropping a
  tick from the requested list, or reducing the tick list to ticks that happen to align are all
  measurement changes. If the readback genuinely cannot be aligned to a tick, report
  `EXIT_CRITERION_UNMEASURABLE` with the observed offset distribution and stop.
- Do not schedule on `Update`, on a frame counter, on a timer, or on a delay. The trigger reads the
  tick counter.
- The capture path must refuse to run, loudly and with a named error, when `sim.time_scale != 1.0`.
  A frame captured under compression carries a tick index that does not correspond to the physics
  that was actually executed.
- The tick index must be recorded in the filename and in the log, not inferred later from ordering.
  Ordering is not provenance.
- Batch verification. One build should validate the trigger, the filename tagging, and the
  compression refusal.

## 5. Exit criterion

### Criterion
Requesting captures at the tick list `0, 60, 300, 600` in harness scene `S5_physics_stress` emits
exactly four PNGs whose recorded tick indices are exactly `0, 60, 300, 600`; repeating the run with
the frame rate artificially halved emits the same four tick indices; and a run with
`sim.time_scale = 2.0` exits non-zero without emitting any frame.

### Measurement

Command:

    cd eustress && cargo run --release --bin harness-scenes -- \
        tick-selftest \
        --space spaces/harness/S5_physics_stress \
        --ticks 0,60,300,600 \
        --width 1920 --height 1080 \
        --slow-run-frame-cap 30 \
        --compression-refusal-check 2.0 \
        --out ../docs/PROMPTS/artifacts/G1.06/tick_trigger.json ; echo "TICK_EXIT=$?"

`tick-selftest` runs the tick list three times: once normally, once with the render loop capped at
30 frames per second so the wall-clock-to-tick relationship differs, and once with
`sim.time_scale = 2.0` to confirm the refusal. It exits non-zero if any requested tick is missing,
if any emitted tick index differs between the normal and slowed runs, or if the compression run
emits a frame.

Expected output shape:

    normal   ticks_requested=[0,60,300,600] ticks_emitted=[0,60,300,600] frames=4
    slowed   ticks_requested=[0,60,300,600] ticks_emitted=[0,60,300,600] frames=4
    tick_sets_equal=true
    compression_run exit=1 frames_emitted=0 error="capture refused: sim.time_scale=2.0, required 1.0"
    TICK_EXIT=0

Pass condition:

    TICK_EXIT == 0
      AND tick_sets_equal == true
      AND both runs emitted frames == 4
      AND compression_run.frames_emitted == 0

Read the emitted tick lists and the exit code. Four files existing under each run is not a pass; the
tick indices must match across the normal and slowed runs.

## 6. Critic gate

`critic_gate` is `[]`. This item changes when a frame is taken, not what is in it; a blinded
evaluator shown two correctly-indexed frames and two incorrectly-indexed ones could not tell them
apart, which is precisely the failure mode the mechanical criterion exists to catch. The criterion in
§5 replaces the gate and is correspondingly tight: exact tick-set equality across a normal and a
deliberately slowed run, plus a hard refusal under compression.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — a tick-keyed capture queue drained inside the fixed-step schedule,
                 with the GPU readback awaited before the tick is marked satisfied
   -> if still failing, MANDATORY approach change. Adjusting where in the schedule the queue drains
      is NOT an approach change; pausing the simulation at the target tick, capturing, and resuming
      is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same tick still mis-emitted AND the observed
                  tick offset moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the GPU readback completes on a later frame than the arming tick (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the criterion to a stated,
proven-constant tick offset, naming what that costs every motion comparison; FUND a specific approach
D; DEFER behind a named blocking item; or KILL, stating that captures then remain wall-clock indexed
and no two machines capture the same moment.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.06/tick_trigger.json`

A reader finds: the requested tick list; the emitted tick list from the normal run and from the
slowed run; the frame-cap used to slow the second run; the equality verdict; the compression-refusal
result including the literal error message and the emitted frame count of zero; the harness scene and
its frozen hash from `G1.03`; the fixed-step rate and substep count read from the running engine; the
reference machine id; and the commit. `G1.09` cites this file as the reason a recipe may declare a
frame set by tick index.

## 9. Definition of NOT done

- The captures fire near the requested ticks with a small offset, and the offset is declared
  acceptable. A tick index is exact or it is not a tick index.
- The tick indices match on both runs because the second run was not actually slowed. Confirm the
  frame cap took effect and record the two wall-clock durations.
- The tick index is written into the filename by a counter that assumes the requests were satisfied
  in order. Record the tick that actually fired, read from the clock at fire time.
- A capture under `sim.time_scale = 2.0` succeeds and is silently mis-indexed. The refusal is part of
  the criterion precisely because a silently wrong index is worse than a missing frame.
- `clock.rs` was edited to make the trigger easier. It is out of scope; the accumulator-zeroing
  behaviour is an instrument that `G1.07` exposes and a G2 item fixes.
- The client burst path in `frame_capture.rs` was retargeted instead of the AI camera path. The
  harness drives the AI camera; the client burst stays as it is.
````

---

## G1.07 — Independent physics-step counter

Item path: `docs/PROMPTS/items/G1.07_independent-step-counter.md`

````markdown
---
id: G1.07
title: Independent physics-step counter exported into the recording and the manifest
workload: W3
workload_secondary: [W1]
phase: G1
depends_on: [G1.01]
blocks: [G1.09, G2.03, G2.04, G2.05, G2.14, G7.15]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.07/step_counter.json
escalation: >
  If the count of physics steps actually executed cannot be observed independently of the simulation
  clock because the physics schedule and the clock share the single counter that clock.rs already
  increments, STALL with the path:line rather than exporting that counter under a new name. Renaming
  the instrument you were sent to build an alternative to is the exact failure this item exists to
  prevent.
status: DRAFT
notes: >
  Tier M. This is 02_CAPTURE_HARNESS.md build item B7. Without it, rubric check N4 is unverifiable
  and the simulation-trust dimension caps at 5.0, so it gates every numeric claim in T2's thirteen
  recipe-citing items.
---

## 1. Objective

A monotonically incremented count of physics steps **actually executed** is maintained independently
of `simulation_time_s` and of `effective_compression()`, exported into every simulation recording and
into every capture manifest. Under a run that saturates the per-frame tick cap, the counter and the
clock disagree, and the disagreement is visible in the exported artifact rather than silent.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
simulation half of that sentence is what this item defends. The licence is PolyForm Shield 1.0.0;
say source-available. Physics is Avian, never Rapier. Units are meter-native.

**The defect, quoted from the source.**
`eustress/crates/common/src/simulation/clock.rs:83` `pub fn advance(&mut self, wall_delta_s: f64) -> u32`:

    self.wall_time_s += wall_delta_s;
    let sim_delta = wall_delta_s * self.time_scale;
    // Clock tracks the full compressed simulation time.
    self.simulation_time_s += sim_delta;
    self.accumulator_s += sim_delta;
    let mut ticks = 0u32;
    while self.accumulator_s >= self.fixed_timestep_s && ticks < self.max_ticks_per_frame {
        self.accumulator_s -= self.fixed_timestep_s;
        self.tick_count += 1;
        ticks += 1;
    }
    if ticks >= self.max_ticks_per_frame {
        self.accumulator_s = 0.0;
    }

The final branch (`clock.rs:100-102`) discards the un-drained accumulator on saturation.
`simulation_time_s` has already advanced by the full compressed delta. The clock therefore reports
simulated time that was never stepped, and `effective_compression()` at `clock.rs:131` reads correct
while it happens. The function's own doc comment at `clock.rs:80-82` states the decoupling plainly:
"Clock time is therefore decoupled from the number of physics steps executed."

**Why this matters beyond tidiness.** `02_CAPTURE_HARNESS.md` §9 build item **B7** records the
consequence: without an independent counter, "rubric check N4 is unverifiable and D5 caps at 5.0",
and the counter "is also the instrument that makes the `clock.rs:100-102` accumulator-zeroing visible
rather than silent". `02` §5.1 forbids `sim.time_scale != 1.0` inside any capture run for exactly
this reason, and §5.2's determinism gate requires "the independent physics-step count is identical"
across two runs as one of its three conditions.

**The distinction you must hold.** `tick_count` on the clock is a count of timesteps the *clock*
drained. It is incremented by the same loop that the cap terminates, in the same struct, from the
same accumulator. It is not independent. The counter this item builds must be incremented by the
physics schedule at the point where a step is actually executed, so that if the physics schedule ever
runs a different number of steps than the clock drained — for any reason, including ones nobody has
predicted — the two numbers differ and the difference is exported.

**Where the recording goes.** `eustress/crates/common/src/simulation/recorder.rs` provides
`export_json` and `export_csv`. The export fires on entering Edit mode from
`eustress/crates/engine/src/simulation/plugin.rs:154`. Output lands at
`<universe>/.eustress/knowledge/recordings/<space>/sim_<timestamp>.json`.

**Where the manifest wants it.** `02` §7.1 places `stepcount_{alpha,beta}.json` in every bundle's
`measurements/` directory. `G1.09` will write those; this item makes the number exist and exports it.

**Determinism context.** `eustress/crates/common/src/physics/determinism.rs` is 56 lines — a
`GlobalRngSeed` resource and nothing else. `eustress/crates/common/tests/determinism.rs` exists but
is gated behind a non-default `physics` feature, so it has never run in any default test invocation.
Do not assume any determinism property is currently enforced.

**Build reality.** 10–15 minutes per engine build, one at a time against the shared `target/`, never
killed mid-compile. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/src/simulation/clock.rs` — **only** to add the independent counter and its
  accessor; the existing `advance()` arithmetic, including the accumulator-zeroing branch, must not
  change
- `eustress/crates/common/src/simulation/recorder.rs` — to export the counter
- `eustress/crates/engine/src/simulation/plugin.rs` — to increment the counter from the physics
  schedule and to include it in the export

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- Anything under `eustress/crates/common/src/physics/` — this item instruments the step count; it
  does not change the solver, the timestep, or the seed
- `eustress/spaces/harness/` — frozen by `G1.03`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Exporting `clock.tick_count` under a new field name, deriving the step count from
  `simulation_time_s / fixed_timestep_s`, or computing it from the recording's frame count are all
  measurement changes — each reproduces the very number the counter exists to disagree with.
- **Do not fix the accumulator-zeroing behaviour.** That is a G2 item. This item's job is to make the
  drop *visible*. A fix that lands here removes the discrepancy this item's exit criterion measures,
  which means the instrument would be verified against a condition it can no longer produce.
- The counter increments where a step is executed, not where a step is scheduled and not where the
  accumulator is drained. If those turn out to be the same line, say so explicitly with the
  `path:line` and explain why the counter is still independent — that is a legitimate finding and it
  belongs in the artifact.
- The counter is monotonic and never reset by a time-scale change, a pause, or a mode transition.
  Say in the result block which of those you tested.
- Batch verification. One build should validate the counter, the recorder export, and the divergence
  test.

## 5. Exit criterion

### Criterion
In a run that deliberately saturates `max_ticks_per_frame`, the exported
`physics_steps_executed` is **strictly less than** the exported `clock_tick_count_implied_by_time`
(that is, `round(simulation_time_s / fixed_timestep_s)`), and the difference is reported as a
non-zero `dropped_steps` field. In an unsaturated run at `sim.time_scale = 1.0`, the two are equal
and `dropped_steps` is exactly 0.

### Measurement

Command:

    cd eustress && cargo run --release --bin eustress-headless -- \
        --space spaces/harness/S5_physics_stress --ticks 600 \
        --export-stepcount ../docs/PROMPTS/artifacts/G1.07/step_counter_baseline.json \
        ; echo "BASE_EXIT=$?"

    cd eustress && cargo run --release --bin eustress-headless -- \
        --space spaces/harness/S5_physics_stress --ticks 600 \
        --time-scale 8.0 --max-ticks-per-frame 2 \
        --export-stepcount ../docs/PROMPTS/artifacts/G1.07/step_counter_saturated.json \
        ; echo "SAT_EXIT=$?"

    python -c "import json; b=json.load(open('docs/PROMPTS/artifacts/G1.07/step_counter_baseline.json')); s=json.load(open('docs/PROMPTS/artifacts/G1.07/step_counter_saturated.json')); print('base_dropped=%d sat_dropped=%d sat_steps=%d sat_implied=%d' % (b['dropped_steps'], s['dropped_steps'], s['physics_steps_executed'], s['clock_tick_count_implied_by_time'])); import sys; sys.exit(0 if b['dropped_steps']==0 and s['dropped_steps']>0 and s['physics_steps_executed'] < s['clock_tick_count_implied_by_time'] else 1)" ; echo "CMP_EXIT=$?"

Expected output shape:

    BASE_EXIT=0
    SAT_EXIT=0
    base_dropped=0 sat_dropped=1847 sat_steps=1200 sat_implied=3047
    CMP_EXIT=0

Pass condition:

    BASE_EXIT == 0  AND  SAT_EXIT == 0  AND  CMP_EXIT == 0

which asserts: `base.dropped_steps == 0` and `sat.dropped_steps > 0` and
`sat.physics_steps_executed < sat.clock_tick_count_implied_by_time`.

Read the emitted values and all three exit codes. The presence of the two JSON files is not a pass —
a counter that mirrors the clock produces `sat_dropped=0` and exits 1, and must.

The saturated run is a deliberately compressed run and is therefore **not** a capture run. It emits
no frames and produces no admissible bundle; it is an explicitly labelled experiment, which is
exactly the arrangement `02_CAPTURE_HARNESS.md` §5.1 requires.

## 6. Critic gate

`critic_gate` is `[]`. This item produces a number, not a picture. It is nevertheless one of the two
items in this pack with the largest downstream effect on a Critic score: `02` §9 records that without
it the simulation-trust dimension caps at 5.0 regardless of how good the simulation is. The
mechanical criterion in §5 replaces the gate and is correspondingly tight — it requires the counter
to *disagree* with the clock under a condition where disagreement is the correct answer, which a
mirrored counter cannot fake.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — an atomic counter incremented in the physics schedule at the point of
                 step execution, read out by the recorder
   -> if still failing, MANDATORY approach change. Moving the increment one system earlier or later
      is NOT an approach change; instrumenting the Avian schedule's own step run condition and
      counting from there is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with sat_dropped still 0 AND no new evidence about where
                  steps are actually executed
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the executed-step count is not observable independently of the clock's own
                   counter (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the criterion to exporting
the clock's own counter, stating explicitly that the simulation-trust dimension then remains capped
and naming the thirteen T2 items affected; FUND a specific approach D; DEFER behind a named blocking
item; or KILL, stating that the time-compression step drop then remains silent.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.07/step_counter.json`

A reader finds: both run configurations (ticks, time scale, max ticks per frame); for each run the
`physics_steps_executed`, the `clock_tick_count_implied_by_time`, the raw `simulation_time_s`, the
`clock.tick_count`, and the derived `dropped_steps`; the `path:line` at which the counter is
incremented, with a one-paragraph argument for why that point is independent of the clock's drain
loop; the list of transitions tested for counter monotonicity (time-scale change, pause, mode
transition); the harness scene and its frozen hash; the commit; and the reference machine id. This
file is the W3 evidence for the item, and it is what every recipe that declares a simulation frame
set cites as the reason its tick indices correspond to executed physics.

## 9. Definition of NOT done

- `physics_steps_executed` equals `clock.tick_count` in every run including the saturated one. That
  is the clock's counter under a new name, and it is the specific failure this item exists to avoid.
- The saturated run shows a drop because `max_ticks_per_frame` was lowered *and* the counter was
  derived from `max_ticks_per_frame`. The counter must be observed, not computed.
- The accumulator-zeroing branch was "fixed" so no drop occurs, which makes the exit criterion
  unsatisfiable and removes the instrument's only test condition. That fix is a G2 item.
- The counter resets on entering play mode, on a pause, or on a time-scale change, so a long
  recording's total is wrong. Monotonic means monotonic.
- The counter is exported into the recording but not into a standalone file the manifest can
  reference, so `G1.09` has to re-derive it. Both exports are required.
- The saturated run emitted frames. A compressed run is an experiment, never a capture.
````

---

## G1.08 — Per-frame, tick-indexed frame-time CSV export

Item path: `docs/PROMPTS/items/G1.08_frametime-csv.md`

````markdown
---
id: G1.08
title: Per-frame tick-indexed frame-time CSV with p50/p99/max/cv
workload: W3
workload_secondary: [W1]
phase: G1
depends_on: [G1.01]
blocks: [G1.09, G4.01, G5.21, G7.40]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.08/frametime_series.json
escalation: >
  If per-frame timings cannot be associated with a simulation tick index because the render frame
  and the fixed-step tick have no stable correspondence in the running engine, STALL with the
  observed frames-per-tick distribution rather than emitting a wall-clock-indexed series relabelled
  as tick-indexed. A mislabelled index is worse than a missing one.
status: DRAFT
notes: >
  Tier M. This is 02_CAPTURE_HARNESS.md build item B8. The profiler already collects the data; the
  gap is a raw per-frame series and the percentile computation, not new instrumentation.
---

## 1. Objective

A capture run emits a raw per-frame frame-time series as CSV, tick-indexed, alongside a summary
carrying `ft_p50`, `ft_p95`, `ft_p99`, `ft_max`, and the coefficient of variation. Every frame-cost
threshold in the prompt library becomes computable from an archived file rather than from a
screen-read of a flamegraph.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian. Units are meter-native.

**What exists today.** `eustress/crates/engine/src/profiler.rs` is 702 lines and works: with
`EUSTRESS_PROFILE=1` and a window set by `EUSTRESS_PROFILE_FRAMES` (default 120) it emits
`eustress_profile.txt` and `eustress_profile.svg` into the current working directory.
`eustress/crates/engine/src/frame_diagnostics.rs` is the companion. What they produce is a ranked
table and a flamegraph.

**What is missing, quoted from `02_CAPTURE_HARNESS.md` §9 build item B8:** "the harness needs the raw
per-frame series, tick-indexed, to compute `ft_p50/p99/max/cv`." `T1_rendering_and_content.md` records
that no percentile is computed anywhere in the engine today.

**What depends on this.** Seven thresholds in `T1_rendering_and_content.md` are stated in
milliseconds against the harness reference GPU (lines `1221`, `1300`, `1535`, `2119`, `2216`, `2441`)
and one is stated as `ft_p99_ms` at most 25.0 (line `4275`). `RM-1` names the machine (`G1.02`); this
item makes the number computable. `02` §7.1 places `frametimes_alpha.csv` and `frametimes_beta.csv`
in every bundle's `measurements/` directory with the note "per-frame ms, tick-indexed".

**Tick indexing.** `G1.06` delivered a tick-scheduled capture trigger reading the tick counter
maintained by `eustress/crates/common/src/simulation/clock.rs`. The frame-time series must carry the
same index so a frame-cost spike can be attributed to a specific captured frame. Where several render
frames fall within one tick, or a tick spans several frames, record the actual relationship rather
than assuming one-to-one — the ratio itself is diagnostic.

**Output location.** The profiler writes to the current working directory today. The harness needs a
declared output path, because `G1.09` will collect these files into a bundle. Add the path parameter;
do not change the profiler's existing default behaviour, which other work relies on.

**Statistical definitions, stated so two implementations agree.** `ft_p99` is the value at the 99th
percentile using the nearest-rank method on the sorted series (index `ceil(0.99 * n) - 1`,
zero-based). `cv` is the sample standard deviation divided by the mean. The series excludes no
frames — no warm-up trim, no outlier rejection. If a warm-up region must be excluded for a threshold
to be meaningful, that is a recipe-level declaration, recorded in the recipe, not a silent trim
inside the exporter.

**Build reality.** 10–15 minutes per engine build, one at a time against the shared `target/`, never
killed mid-compile. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/profiler.rs` — add the raw series export and the output-path parameter
- `eustress/crates/engine/src/frame_diagnostics.rs` — only to supply the per-frame values and the
  tick association

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/simulation/clock.rs` — read the tick counter; do not change it
- Anything under `eustress/crates/common/src/physics/`
- `eustress/spaces/harness/` — frozen by `G1.03`
- **Any file under `docs/PROMPTS/packs/`** — the seven millisecond thresholds stay exactly as
  authored; this item makes them computable, it does not adjust them

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Trimming warm-up frames, rejecting outliers, smoothing the series, computing the percentile from a
  histogram bucket rather than the raw series, or shortening the capture window are all measurement
  changes. If the series is genuinely unusable, report `EXIT_CRITERION_UNMEASURABLE` and stop.
- Export the raw series, not a summary. The summary is derived from the CSV, and the CSV is what is
  archived, so a later reader can recompute any statistic including ones nobody has thought of yet.
- Do not change the profiler's existing default output behaviour. Add a parameter; other work relies
  on `eustress_profile.txt` landing where it lands.
- Record the actual frame-to-tick relationship. If it is not one-to-one, that is data, not a problem
  to hide.
- Batch verification. One build should validate the CSV export, the percentile computation, and the
  tick association.

## 5. Exit criterion

### Criterion
A 600-frame profiled run over harness scene `RH3_exterior` emits a CSV with exactly 600 data rows,
each carrying a frame index, a tick index, and a frame time in milliseconds; and the summary's
`ft_p99_ms` recomputed independently from that CSV by the nearest-rank method agrees with the
summary's own value to within `0.001` ms.

### Measurement

Command:

    cd eustress && EUSTRESS_PROFILE=1 EUSTRESS_PROFILE_FRAMES=600 \
      cargo run --release --bin harness-scenes -- \
        frametime \
        --space spaces/harness/RH3_exterior \
        --path CP-B \
        --width 3840 --height 2160 \
        --csv ../docs/PROMPTS/artifacts/G1.08/frametimes.csv \
        --summary ../docs/PROMPTS/artifacts/G1.08/frametime_series.json \
        ; echo "FT_EXIT=$?"

    python -c "
    import csv, json, math, sys
    rows = list(csv.DictReader(open('docs/PROMPTS/artifacts/G1.08/frametimes.csv')))
    s = json.load(open('docs/PROMPTS/artifacts/G1.08/frametime_series.json'))
    ms = sorted(float(r['frame_time_ms']) for r in rows)
    p99 = ms[math.ceil(0.99*len(ms))-1]
    ok = len(rows)==600 and abs(p99 - s['ft_p99_ms']) <= 0.001 and all(r['tick_index']!='' for r in rows)
    print('rows=%d recomputed_p99=%.4f reported_p99=%.4f' % (len(rows), p99, s['ft_p99_ms']))
    sys.exit(0 if ok else 1)
    " ; echo "CHK_EXIT=$?"

Expected output shape:

    FT_EXIT=0
    rows=600 recomputed_p99=21.7431 reported_p99=21.7431
    CHK_EXIT=0

Pass condition:

    FT_EXIT == 0  AND  CHK_EXIT == 0

which asserts 600 rows, every row carrying a non-empty tick index, and the reported `ft_p99_ms`
matching an independent recomputation to within 0.001 ms.

Read the recomputed and reported values and both exit codes. A CSV existing is not a pass.

## 6. Critic gate

`critic_gate` is `[]`. This item emits a measurement file and changes nothing a blinded evaluator can
see. The mechanical criterion in §5 replaces the gate and is correspondingly tight: an exact row
count, a non-empty tick index on every row, and agreement between the reported percentile and an
independent recomputation from the raw series — which is specifically a check that the summary was
not computed by a different method than the one it claims.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — accumulate per-frame timings in the existing profiler ring and flush
                 them to CSV with the tick index read at frame end
   -> if still failing, MANDATORY approach change. Changing where the timestamp is taken within the
      frame is NOT an approach change; recording from Bevy's own diagnostics store with a separate
      tick-tagging system is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the row count or the tick association still wrong
                  AND the count of malformed rows moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: render frames and fixed-step ticks have no stable correspondence (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the criterion to a
wall-clock-indexed series, naming the eight T1 thresholds that then cannot be attributed to a
captured frame; FUND a specific approach D; DEFER behind a named blocking item; or KILL, stating that
every millisecond threshold in the library then remains uncomputable from an archived artifact.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.08/frametime_series.json`

A reader finds: the frame count; `ft_p50_ms`, `ft_p95_ms`, `ft_p99_ms`, `ft_max_ms`, `ft_mean_ms`,
and `cv`; the percentile method named explicitly as nearest-rank; a statement that no frames were
trimmed; the observed frames-per-tick distribution; the path to the raw CSV and its SHA-256; the
resolution, the camera path, the harness scene and its frozen hash; the reference machine id `RM-1`
with its GPU and driver; and the commit. Alongside it, `docs/PROMPTS/artifacts/G1.08/frametimes.csv`
holds the raw series. Every T1 item that must prove it did not buy fidelity with frame budget cites
this pair.

## 9. Definition of NOT done

- The summary reports `ft_p99_ms` computed by linear interpolation while the artifact declares
  nearest-rank. The method is stated so two implementations agree; a mismatch fails the check.
- The CSV has 600 rows but the tick index column is empty or constant. Tick indexing is the property
  `02` build item B8 asks for.
- Warm-up frames were trimmed inside the exporter to make the percentile look better. Any exclusion
  is a recipe-level declaration, recorded in the recipe.
- The percentile is computed from a histogram bucket rather than the raw sorted series, so it agrees
  to within a bucket width instead of to within 0.001 ms.
- The profiler's existing `eustress_profile.txt` output stopped appearing where it used to. Add a
  parameter; do not change the default.
- The series is emitted but not at 3840x2160, so it cannot be compared against thresholds specified
  at that resolution.
- A threshold in `T1_rendering_and_content.md` was adjusted because the measured numbers came out
  above it. That is a floor change, it is out of scope, and it fails the item.
````

---

## G1.09 — `eustress-capture` binary: recipe to bundle to manifest

Item path: `docs/PROMPTS/items/G1.09_eustress-capture-binary.md`

````markdown
---
id: G1.09
title: eustress-capture binary producing a content-addressed bundle with a valid manifest
workload: W3
workload_secondary: [W6, W1]
phase: G1
depends_on: [G1.02, G1.04, G1.06, G1.07, G1.08]
blocks: [G1.10, G1.11, G1.12, G1.13]
tier: XL
token_envelope: 1200000
wallclock_envelope: 5d
max_builds: 20
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.09/bundle_selftest.json
escalation: >
  If a full frame set cannot be captured in one process lifetime because the engine must be
  restarted between scenes, STALL with the restart cause named rather than shipping a driver that
  silently spans several engine sessions. A bundle whose two sides were produced across different
  process lifetimes has an uncontrolled variable in it and invariant I1 is violated.
status: DRAFT
notes: >
  Tier XL: this is a new subsystem, 02_CAPTURE_HARNESS.md build items B4 and B6 together. Twenty
  build slots is roughly four hours of pure compile and is the binding constraint. Design so one
  build validates recipe parsing, the run driver, hashing, and the manifest writer together.
---

## 1. Objective

One command reads a recipe, drives the engine through every scene, camera path, and tick-indexed
frame set the recipe declares, collects the frames plus the profile, the frame-time CSV, the
recording, and the step count, hashes everything, and writes a bundle whose directory name is the
SHA-256 of its own manifest. A `verify` subcommand re-hashes an existing bundle and rejects it on any
mismatch or on an incomplete external-reference provenance block.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine.
Rendering and the ECS are implementation details in service of a world model an AI reasons over and a
human edits. The licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian, never
Rapier. Slint is Rust. Units are meter-native.

**The command this item must make work**, quoted from `docs/PROMPTS/02_CAPTURE_HARNESS.md` §8:

    cargo run --release --bin eustress-capture -- \
        --recipe docs/PROMPTS/harness/recipes/G3_material.json \
        --subject HEAD \
        --control 30412479 \
        --out docs/PROMPTS/artifacts/bundles \
        --trials 5 \
        --verify-determinism

Three already-authored items invoke this exact form in their exit-criterion measurement blocks and
are currently unmeasurable without it: `T3` `G6.15` at `docs/PROMPTS/packs/T3_studio_ux.md:3767`,
`T5` `G7.42` at `docs/PROMPTS/packs/T5_robustness_and_cohesion.md:2594`, and `T5` `G7.45` at
`T5_robustness_and_cohesion.md:3280`. `T3` `G6.15` additionally invokes a `verify` subcommand at
`T3_studio_ux.md:3775`:

    cargo run --release --bin eustress-capture -- verify \
        --bundle docs/PROMPTS/artifacts/bundles/<hash>/ --leak-check \
        --out docs/PROMPTS/artifacts/G6.15/blind_bundle_manifest.json

Both call forms are part of this item's contract. `--verify-determinism` is implemented by `G1.11`
and `--leak-check` by `G1.10`; this item must accept both flags and dispatch to them, and until those
items land the flags must fail loudly with `not yet implemented`, never silently succeed.

**The seven invariants every bundle must satisfy**, from `02` §2, restated so you need not open it:
**I1** fixed scene from a pinned Space at a pinned commit; **I2** fixed camera path, poses enumerated
not interactive; **I3** fixed seed, `GlobalRngSeed` set explicitly; **I4** tick-indexed frames, never
wall-clock or frame counters; **I5** fixed resolution and colour pipeline, identical on both sides;
**I6** content-addressed output, bundle directory name is the SHA-256 of the manifest and frames
carry their own hashes; **I7** labels stripped and randomised. This item owns I1 through I6. I7 is
`G1.10`.

**The output layout**, from `02` §7.1, is normative and this item must produce exactly it:

    docs/PROMPTS/artifacts/bundles/<manifest-sha256>/
    +-- manifest.json               # committed to git; everything else gitignored
    +-- manifest.sig                # optional detached signature
    +-- blinded/trial_1..trial_5/{alpha,beta}/<frame-set>/<index>.png
    +-- measurements/
    |     profile_{alpha,beta}.txt
    |     frametimes_{alpha,beta}.csv
    |     recording_{alpha,beta}.json
    |     stepcount_{alpha,beta}.json
    +-- hashes.txt                  # sha256 of every file in the bundle
    +-- KEY.json                    # side assignment - NEVER shipped to the Critic

**The manifest schema** is `02` §7.2 and is normative. Its top-level keys: `schema`
(`"eustress.capture.manifest/1"`), `bundle_id`, `created_utc`, `operator`, `determinism_verified`,
`sides` (with `subject` and `control` sub-objects), `hardware`, `settings`, `scenes`, `paths`,
`frame_sets`, `trials`, `blinding`, `frame_hashes`. The `settings` block records the seed, the fixed
step rate, the substep count, `sim_time_scale`, `sim_max_ticks_per_frame`, render width/height/
colourspace/tonemapping/msaa, and an `env` map. Read `02` §7.2 for the exact shape and reproduce it
key for key.

**Bundle size, and why almost nothing is committed.** `02` §4.2 computes 1,641 frames per full bundle
side, which at 3840x2160 PNG is on the order of 8 to 14 GB per side. **Bundles are not committed to
git.** Only `manifest.json` is. The bundle root must be added to `.gitignore` with the manifest
explicitly re-included, and this item does that.

**The provenance rule that makes `verify` load-bearing.** `02` §7.3: a `control` of
`kind: external_reference` is inadmissible unless every one of `product`, `product_version`,
`scene_source`, `scene_license`, `publication_rights`, `capture_operator`, `capture_date_utc`, and
the full `hardware` block is populated. The Critic rejects such a bundle rather than scoring it.
`verify` must implement that rejection. `publication_rights` defaults to `internal_calibration_only`
and **only the human may set it to `publishable`** — the verifier must reject a bundle in which an
agent set it.

**Time compression is forbidden inside a capture run.** `02` §5.1: `sim.time_scale` must be `1.0` for
every capture run, because
`eustress/crates/common/src/simulation/clock.rs:100-102` zeroes the accumulator on saturation and the
clock then reports time that was never stepped. A recipe declaring any other value is malformed and
this binary must refuse it at parse time with a named error.

**What already exists, delivered by this item's dependencies, and must be used rather than
rebuilt.** `G1.02`: the reference-machine registry at
`docs/PROMPTS/harness/reference_machines.json`, which is where the manifest `hardware` block is
filled from. `G1.04`: the four camera path files under `eustress/spaces/harness/paths/` and the
tick-driven path player. `G1.06`: the tick-indexed capture trigger and its refusal under compression.
`G1.07`: the independent step counter and its export. `G1.08`: the tick-indexed frame-time CSV and
its summary. `G1.03`, transitively: the six frozen harness Spaces and their recorded hashes at
`docs/PROMPTS/artifacts/G1.03/scene_freeze.json`.

**How the binary reaches a running engine.** The engine bridge binds a TCP listener on startup and
writes its port to `<universe>/.eustress/engine.port`
(`eustress/crates/engine/src/engine_bridge/mod.rs:29`); the protocol is JSON-RPC over that socket.
`eustress/crates/bridge-client/src/lib.rs` is the client crate. The engine opens a specific Space
with `--space <dir>`.

**There is no headless GPU tier, and that is not this item's problem to solve.**
`eustress/crates/engine/src/bin/headless.rs` is 291 lines of `MinimalPlugins` +
`ScheduleRunnerPlugin`; `docs/architecture/HEADLESS_RUNTIME.md:269` lists `P6 --render gpu` as
unstarted. Every capture therefore requires a desktop session today. That is `02` build item **B10**,
it belongs to G7, and this binary must work with a windowed engine now and be structured so that a
headless GPU tier later requires no change to the recipe format.

**Build reality.** 10–15 minutes per engine build, one at a time against the shared `target/`, never
killed mid-compile; recovery from a killed build is `cargo clean -p <crate>`. Validate with
`cargo run`, not `cargo check`. Twenty build slots is roughly four hours of pure compile.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/bin/capture.rs` — new file, the binary
- `eustress/crates/engine/src/capture/` — new module tree: recipe parsing, run driver, collection,
  hashing, manifest writing, verification
- `eustress/crates/engine/Cargo.toml` — to register the `eustress-capture` `[[bin]]` and any
  genuinely required dependency, with a stated reason
- `eustress/crates/bridge-client/src/lib.rs` — only if a bridge call the driver needs is missing
- `.gitignore` — to exclude `docs/PROMPTS/artifacts/bundles/` while re-including `manifest.json`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass; the CI capture job is
  `02` build item B11 and belongs to G7
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/02_CAPTURE_HARNESS.md` — the manifest schema and layout are normative and this item
  implements them; it does not amend them
- `eustress/spaces/harness/` — frozen by `G1.03`
- `eustress/crates/engine/src/bin/headless.rs` — the headless GPU tier is G7's
- Anything under `eustress/crates/common/src/physics/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Emitting a manifest with fewer keys than `02` §7.2 declares, hashing a subset of the bundle,
  reducing the frame count, lowering the resolution, or making `verify` warn where the spec says
  reject are all measurement changes. If the specification is genuinely unimplementable, report
  `EXIT_CRITERION_UNMEASURABLE` with the specific clause and stop.
- The bundle directory name **is** the SHA-256 of the manifest. Not a prefix of it, not a timestamp,
  not a random id with the hash inside. That is invariant I6 and it is what makes a capture citable.
- `hashes.txt` covers every file in the bundle, including the manifest itself is excluded only if
  including it would be self-referential — state which convention you chose and why, and make
  `verify` use the same one.
- `verify` **rejects**. It does not warn, it does not annotate, it does not pass with a note. An
  incomplete external-reference provenance block, a `publication_rights` of `publishable` not
  attributable to the human, a hash mismatch, or unequal frame counts between the two sides each
  produce a non-zero exit.
- Refuse a recipe declaring `sim_time_scale != 1.0` at parse time, with a named error, before
  anything launches.
- One process lifetime per side. If the engine must restart between scenes, that is the escalation in
  the front matter, not a design choice you may make quietly.
- Batch verification hard. Twenty builds across three approaches is four hours of compile; design so
  one build exercises recipe parsing, the driver, the collector, the hasher, and the manifest writer
  in a single run against a small recipe.

## 5. Exit criterion

### Criterion
Running `eustress-capture` against a two-scene, two-frame-set selftest recipe with
`--subject HEAD --control HEAD` produces a bundle directory whose name equals the SHA-256 of its own
`manifest.json`; the manifest contains all fifteen top-level keys required by
`02_CAPTURE_HARNESS.md` §7.2; `hashes.txt` covers every file present; the two sides have equal frame
counts; and `verify` on that bundle exits 0, while `verify` on the same bundle with one frame byte
altered exits non-zero.

### Measurement

Command:

    cd eustress && cargo run --release --bin eustress-capture -- \
        --recipe ../docs/PROMPTS/harness/recipes/G1_selftest.json \
        --subject HEAD --control HEAD \
        --out ../docs/PROMPTS/artifacts/bundles \
        --trials 1 ; echo "CAP_EXIT=$?"

    python -c "
    import hashlib, json, glob, os, sys
    roots = sorted(glob.glob('docs/PROMPTS/artifacts/bundles/*/'), key=os.path.getmtime)
    root = roots[-1]
    raw = open(os.path.join(root,'manifest.json'),'rb').read()
    digest = hashlib.sha256(raw).hexdigest()
    m = json.loads(raw)
    req = ['schema','bundle_id','created_utc','operator','determinism_verified','sides','hardware',
           'settings','scenes','paths','frame_sets','trials','blinding','frame_hashes']
    missing = [k for k in req if k not in m]
    named = os.path.basename(root.rstrip('/\\\\'))
    files = sum(len(f) for _,_,f in os.walk(root))
    hashed = len([l for l in open(os.path.join(root,'hashes.txt')) if l.strip()])
    print('dir=%s digest=%s name_matches=%s missing=%s files=%d hashed=%d'
          % (named, digest[:16], named==digest, missing, files, hashed))
    sys.exit(0 if named==digest and not missing else 1)
    " ; echo "MAN_EXIT=$?"

    cd eustress && cargo run --release --bin eustress-capture -- verify \
        --bundle ../docs/PROMPTS/artifacts/bundles/<hash>/ \
        --out ../docs/PROMPTS/artifacts/G1.09/bundle_selftest.json ; echo "VERIFY_CLEAN_EXIT=$?"

    printf '\\x00' | dd of=docs/PROMPTS/artifacts/bundles/<hash>/blinded/trial_1/alpha/FS-MATERIAL/00.png bs=1 seek=200 count=1 conv=notrunc
    cd eustress && cargo run --release --bin eustress-capture -- verify \
        --bundle ../docs/PROMPTS/artifacts/bundles/<hash>/ ; echo "VERIFY_TAMPERED_EXIT=$?"

Expected output shape:

    CAP_EXIT=0
    dir=8f21ac... digest=8f21ac... name_matches=True missing=[] files=57 hashed=56
    MAN_EXIT=0
    VERIFY_CLEAN_EXIT=0
    manifest hash mismatch: blinded/trial_1/alpha/FS-MATERIAL/00.png
    VERIFY_TAMPERED_EXIT=1

Pass condition:

    CAP_EXIT == 0  AND  MAN_EXIT == 0
      AND VERIFY_CLEAN_EXIT == 0
      AND VERIFY_TAMPERED_EXIT != 0

The tamper leg is not optional. A verifier that only ever sees valid bundles has never been shown to
reject anything, and rejection is its entire purpose.

`docs/PROMPTS/harness/recipes/G1_selftest.json` is authored by this item as a minimal recipe — two
scenes, two frame sets, a handful of ticks — expressly so the exit criterion is cheap to run. It is
not one of the 33 library recipes, which `G1.12` authors against the same format.

## 6. Critic gate

`critic_gate` is `[]`. This item builds the machine that produces every bundle a Critic will ever
score; it cannot itself be scored against a bundle it produced without circularity. The mechanical
criterion in §5 replaces the gate and is correspondingly tight: exact content-addressing (the
directory name equals the manifest digest), complete manifest key coverage against the normative
schema, and a demonstrated rejection of a deliberately corrupted bundle.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — a driver process that launches the engine, attaches over the bridge
                 TCP port, and orchestrates scene load, path replay, and tick-armed capture per side
   -> if still failing, MANDATORY approach change. Reordering the orchestration steps or changing a
      timeout is NOT an approach change; embedding the driver inside the engine process as a
      capture-mode plugin, so no cross-process handshake exists, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same manifest key or the same layout element
                  still missing AND the count of produced frames moving < 5%
  - Budget      : 1.8M tokens or 30 builds consumed (150% of the XL envelope)
  - Item-specific: a full frame set cannot be captured in one engine process lifetime (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the criterion to a named
reduced bundle shape, listing every item in the library whose exit criterion becomes unmeasurable;
FUND a specific approach D; DEFER behind a named blocking item; or KILL, stating that the program
then has no capture harness and `00_MASTER_PROTOCOL.md` §3.2's G1 exit condition cannot be met at
all.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.09/bundle_selftest.json`

A reader finds: the selftest recipe path and its SHA-256; the produced bundle directory name and the
independently computed manifest digest, with the equality verdict; the list of manifest keys present
and the list required by `02` §7.2 with the difference stated; the file count and the `hashes.txt`
line count; the per-side frame counts and their equality verdict; the clean-verify exit code; the
tampered-verify exit code and the literal rejection message; the harness scenes used with their
frozen hashes from `G1.03`; the reference machine id `RM-1`; and the commit. This file is the W3
evidence for the item and is the file `G1.13` cites when it claims one command produces a
content-addressed bundle.

## 9. Definition of NOT done

- The bundle directory is named with a timestamp, a UUID, or a truncated digest. Invariant I6 says the
  name is the SHA-256 of the manifest; a prefix is not the hash.
- The manifest is missing `hardware`, or fills it with placeholder strings rather than reading
  `docs/PROMPTS/harness/reference_machines.json`. An incomplete hardware block makes an
  external-reference bundle inadmissible by `02` §7.3.
- `verify` warns on a hash mismatch and exits 0. The verifier's entire purpose is rejection.
- `verify` accepts a bundle whose `publication_rights` is `publishable` with no human attribution.
  Only the human may set that value; a verifier that lets an agent set it has removed the one control
  standing between an internal calibration capture and a licence violation.
- `--verify-determinism` or `--leak-check` is accepted and silently ignored because the implementing
  item has not landed. Unimplemented flags fail loudly.
- The two sides have different frame counts. `02` §6.2 records that an unequal count is itself a
  label, so this is a blinding failure disguised as a collection bug.
- A recipe declaring `sim_time_scale = 2.0` is accepted and run. It must be refused at parse time.
- The bundle was committed to git. Only `manifest.json` is; the rest is gitignored, and a 14 GB
  commit is not recoverable from politely.
- The driver restarts the engine between scenes, so the two sides of a comparison were produced in
  different process lifetimes. That is an uncontrolled variable and it violates invariant I1.
````

---

## G1.10 — `blind` subcommand, leak checklist, and `KEY.json`

Item path: `docs/PROMPTS/items/G1.10_blinding-tool.md`

````markdown
---
id: G1.10
title: Blinding subcommand with metadata scrub, leak checklist, and separated key
workload: W3
workload_secondary: [W1]
phase: G1
depends_on: [G1.09]
blocks: [G1.13]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.10/blinding_audit.json
escalation: >
  If a rendered frame contains a product name, version string, build date, or watermark that cannot
  be removed without changing what the Critic is being asked to judge, STALL with the frame index
  and the pixel region named. Cropping the region out is forbidden; a cropped frame set is a
  different frame set on one side only, which is itself a label.
status: DRAFT
notes: >
  Tier L. This is 02_CAPTURE_HARNESS.md build item B5, a subcommand of B4. The work is mostly
  file-tree manipulation and PNG chunk surgery, but it needs engine builds to produce test bundles
  and the leak checklist has to be exercised against real frames.
---

## 1. Objective

A capture bundle can be blinded: frames copied into a position-only tree, all PNG ancillary chunks
and EXIF and XMP stripped, mtimes normalised, side assignment drawn fresh per trial from a CSPRNG
seeded independently of the bundle hash, the side key written to a file that never enters the
Critic's working copy, and the seven-point leak checklist run and recorded. A blinded tree in which a
side is inferable fails the audit and the audit says which channel leaked.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian.

**The invariant this item satisfies.** `docs/PROMPTS/02_CAPTURE_HARNESS.md` invariant **I7**: "Labels
stripped and randomised. No filename, EXIF field, watermark, overlay, HUD, window title, or directory
name reveals which side is which. Side assignment is randomised per trial," because "the Critic must
not be able to infer the side."

**The procedure is normative**, `02` §6.1, restated so you need not open it:

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
    5. The key - which of ALPHA/BETA is SUBJECT - is written to a SEPARATE file that L1 holds and
       that is NEVER placed in the Critic's input directory.
    6. Repeat 2-5 for each of the 5 preference trials with a fresh permutation.

**The leak checklist is normative**, `02` §6.2, and each entry is a real leak channel:

- No filename, path component, or archive name contains `eustress`, a commit hash, a version, a
  product name, `subject`, `control`, `a`, `b`, `old`, `new`, `before`, `after`.
- No PNG text chunk, EXIF, or XMP survives.
- No frame contains a UI element carrying a product name, version, or build date.
- Frame counts are equal on both sides. **An unequal count is itself a label.**
- Resolutions are identical on both sides.
- File sizes are not systematically ordered in a way that identifies a side — if one side is
  uniformly larger, that must be noted and the bundle marked `blinding_compromised`.
- No accompanying prose. The Critic receives the rubric, the frames, L1's verified measurement, and
  the drawn held-out criteria, and nothing else.

**Where the key goes.** `02` §7.1 places `KEY.json` inside the bundle for archival but removes it
from the Critic's working copy, and notes that better still, L1 stores it outside the bundle
entirely. Implement both: write `KEY.json` into the bundle, and make the blinding step emit a
Critic-ready directory that provably does not contain it.

**The seed independence requirement is not decorative.** If the trial permutation were derived from
the bundle hash, anyone holding the manifest could recompute the side assignment, and the manifest is
the one file that *is* committed to git. Seed the CSPRNG from an independent source and record only
that a draw occurred, never the seed itself, in anything the Critic can reach.

**What exists.** `G1.09` delivered `eustress-capture` with its recipe parser, run driver, hashing,
manifest writer, and `verify` subcommand, and accepts a `--leak-check` flag on `verify` that
currently fails loudly as unimplemented. This item implements it and adds the `blind` subcommand.

**Where a product name could realistically appear in a frame.** The studio window title, the ribbon,
any notification toast, and any billboard label authored into a scene. `S4_studio_ui` is the scene
where this risk is concentrated, and `T3`'s eleven UI items all capture it. Test against that scene
specifically.

**Build reality.** 10–15 minutes per engine build, one at a time against the shared `target/`, never
killed mid-compile. Validate with `cargo run`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/capture/` — the `blind` and leak-check modules
- `eustress/crates/engine/src/bin/capture.rs` — only to register the `blind` subcommand and wire
  `--leak-check`
- `eustress/crates/engine/Cargo.toml` — only for a genuinely required dependency, with a stated reason

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest — a blinded tree is a *copy*; the source
  frames are never modified in place
- `docs/PROMPTS/02_CAPTURE_HARNESS.md` — §6 is normative and this item implements it
- `eustress/spaces/harness/` — frozen by `G1.03`
- The studio UI source — if a frame carries a product name, that is the front-matter escalation, not
  a licence to edit the UI from inside this item

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Dropping a checklist entry, downgrading a rejection to a warning, cropping a frame region to hide
  an overlay, or excluding a frame set from the audit are all measurement changes. Report
  `EXIT_CRITERION_UNMEASURABLE` with the specific clause and stop if the check is genuinely wrong.
- Never modify a source frame in place. Blinding produces a copy; the hashed originals stay hashed.
- The per-trial permutation is drawn fresh for every trial from a CSPRNG seeded independently of the
  bundle hash. Reusing one permutation across five trials makes the five trials one trial.
- `KEY.json` must not be reachable from the Critic-ready directory by any path, including a symlink,
  a relative parent traversal, or an archive that happens to include it. Test that.
- A `blinding_compromised` finding is recorded, not suppressed. The file-size ordering check exists
  precisely because a compromised bundle that is *known* to be compromised is still usable with a
  caveat, while one that is silently compromised is not.
- Batch verification. Twelve builds across three approaches; produce one test bundle and run the
  blinding and audit repeatedly against it.

## 5. Exit criterion

### Criterion
Blinding a two-side bundle for 5 trials produces 5 trial directories, each containing `alpha` and
`beta` subtrees with **equal frame counts and identical resolutions**; across the 5 trials the side
assignment is **not constant**; zero PNG ancillary chunks other than `IHDR`, `IDAT`, and `IEND`
survive across every frame; every filename and path component passes the forbidden-token scan; and
`KEY.json` is absent from the Critic-ready directory while present in the archival bundle.

### Measurement

Command:

    cd eustress && cargo run --release --bin eustress-capture -- blind \
        --bundle ../docs/PROMPTS/artifacts/bundles/<hash>/ \
        --trials 5 \
        --critic-dir ../docs/PROMPTS/artifacts/critic_input/<hash>/ \
        --audit ../docs/PROMPTS/artifacts/G1.10/blinding_audit.json ; echo "BLIND_EXIT=$?"

    cd eustress && cargo run --release --bin eustress-capture -- verify \
        --bundle ../docs/PROMPTS/artifacts/bundles/<hash>/ --leak-check ; echo "LEAK_EXIT=$?"

    python -c "
    import json, os, sys, glob, struct
    a = json.load(open('docs/PROMPTS/artifacts/G1.10/blinding_audit.json'))
    crit = 'docs/PROMPTS/artifacts/critic_input/'
    key_leaks = [p for p in glob.glob(crit+'**/*', recursive=True) if 'KEY' in os.path.basename(p)]
    # count non-critical PNG chunks across every blinded frame
    bad = 0
    for p in glob.glob(crit+'**/*.png', recursive=True):
        d = open(p,'rb').read(); i = 8
        while i < len(d):
            ln = struct.unpack('>I', d[i:i+4])[0]; typ = d[i+4:i+8].decode('ascii','replace')
            if typ not in ('IHDR','IDAT','IEND','PLTE'): bad += 1
            i += 12 + ln
            if typ == 'IEND': break
    print('trials=%d counts_equal=%s res_equal=%s assignment_varies=%s forbidden_tokens=%d ancillary_chunks=%d key_in_critic_dir=%d'
          % (a['trials'], a['counts_equal'], a['resolutions_equal'], a['assignment_varies'],
             a['forbidden_token_hits'], bad, len(key_leaks)))
    sys.exit(0 if a['trials']==5 and a['counts_equal'] and a['resolutions_equal']
             and a['assignment_varies'] and a['forbidden_token_hits']==0
             and bad==0 and len(key_leaks)==0 else 1)
    " ; echo "AUDIT_EXIT=$?"

Expected output shape:

    BLIND_EXIT=0
    LEAK_EXIT=0
    trials=5 counts_equal=True res_equal=True assignment_varies=True forbidden_tokens=0 ancillary_chunks=0 key_in_critic_dir=0
    AUDIT_EXIT=0

Pass condition:

    BLIND_EXIT == 0  AND  LEAK_EXIT == 0  AND  AUDIT_EXIT == 0

The chunk scan is performed by the checking script independently of the tool's own audit, so a tool
that reports zero surviving chunks while leaving them in place fails.

## 6. Critic gate

`critic_gate` is `[]`, and here the reason is structural rather than practical: this item builds the
mechanism that keeps the Critic honest. Submitting it to the Critic would require showing the Critic
the very side labels the item exists to hide. The mechanical criterion in §5 replaces the gate and is
correspondingly tight — an independent chunk scan, an independent key-leak scan, and a requirement
that the side assignment demonstrably varies across trials.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — copy-and-rewrite: decode each PNG, re-encode with only the critical
                 chunks, write into the position-only tree
   -> if still failing, MANDATORY approach change. Adding one more chunk type to the strip list is
      NOT an approach change; a byte-level chunk walker that rewrites the stream without decoding
      the image is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same leak channel still open AND the count of
                  leaking files moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a rendered frame carries a product name, version, or build date that cannot be
                   removed without changing what is being judged (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the blinding requirement to a
named reduced set, stating that the preference dimension becomes uninterpretable and naming every
item that depends on it; FUND a specific approach D; DEFER behind a named blocking item; or KILL,
stating that every blind comparison in the program then rests on the Critic not noticing.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.10/blinding_audit.json`

A reader finds: the trial count; per trial, the assignment recorded **only as a boolean of whether it
differed from the previous trial**, never the assignment itself; the per-side frame counts and
resolutions with their equality verdicts; the forbidden-token scan result with the token list used;
the surviving-ancillary-chunk count; the mtime normalisation constant; the file-size distribution per
side and the `blinding_compromised` verdict; the confirmation that `KEY.json` is present in the
archival bundle and absent from the Critic-ready directory, with both paths; the seven checklist
entries with pass or fail against each; the bundle id; and the commit. This file is the W3 evidence
for the item. It is written for L1, and it deliberately contains nothing from which a side assignment
could be reconstructed.

## 9. Definition of NOT done

- All five trials share one permutation, so five trials are one trial repeated. `02` §6.1 step 6
  requires a fresh permutation per trial.
- The permutation is derived from the bundle hash, which is in the committed manifest, so anyone with
  the manifest can recover the key.
- `KEY.json` is absent from the Critic directory but reachable through a relative path, a symlink, or
  an archive. Test the reachability, not just the listing.
- Ancillary chunks are stripped from the frames but the audit is what reports it, and an independent
  scan finds them still present. The independent scan is in the criterion for this reason.
- Frame counts differ between sides and the tool proceeds. `02` §6.2 states plainly that an unequal
  count is itself a label.
- One side is uniformly larger in file size and the tool neither notes it nor sets
  `blinding_compromised`. A known-compromised bundle is usable with a caveat; a silently compromised
  one is not.
- The blinded frames are the originals, moved rather than copied, so the hashed source tree no longer
  matches its manifest.
- A studio frame in the `S4_studio_ui` set carries the window title, and the fix was to crop it. A
  crop applied to one side is a label.
````

---

## G1.11 — `--verify-determinism` and the byte-identity selftest

Item path: `docs/PROMPTS/items/G1.11_determinism-verification.md`

````markdown
---
id: G1.11
title: Determinism verification mode and the two-capture byte-identity selftest
workload: W3
workload_secondary: [W1]
phase: G1
depends_on: [G1.09, G2.02]
blocks: [G1.13, G3.01]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.11/determinism_verification.json
escalation: >
  If two captures of the same static scene from the same pose cannot be made byte-identical because
  a render feature in the default studio camera path is inherently nondeterministic - a
  frame-indexed jitter, a time-seeded noise term, a temporal accumulation - STALL immediately with
  the feature named and its path:line. Disabling the feature to force a pass is forbidden: it
  changes what every downstream item is measuring.
status: DRAFT
notes: >
  Tier L. This is 02_CAPTURE_HARNESS.md build item B9 plus the deterministic-capture selftest. It is
  the item that executes the gate written at docs/architecture/HEADLESS_RUNTIME.md:294, which has
  never been run. See pack note N3: this item, not T1's G3.01, owns capture determinism.
---

## 1. Objective

`eustress-capture --verify-determinism` runs a recipe twice per side and asserts three things:
every frame PNG is byte-identical across the two runs, the exported recording JSON is byte-identical
modulo a whitelisted timestamp field, and the independent physics-step count is identical. A separate
`selftest-determinism` subcommand proves the simplest case — two consecutive captures of the same
static scene from the same pose hash to the same SHA-256 — so that a determinism failure elsewhere
can be localised rather than guessed at.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian, never Rapier.

**The gate this item executes.** `docs/architecture/HEADLESS_RUNTIME.md:294`: "the same space + same
`--ticks` produces byte-identical recordings across two runs (the determinism pins make this a real,
testable claim)." `02_CAPTURE_HARNESS.md` §5.2 states it has never been executed, and
`00_MASTER_PROTOCOL.md` §1.1 condition **D2** makes it one of four conditions on the program being
done at all: "Two independent runs of the same headless recipe produce byte-identical recordings and
byte-identical capture frames."

**The three assertions are normative**, `02` §5.2, restated: before any bundle is admissible the
harness runs the same recipe twice and asserts (1) every frame PNG is byte-identical across the two
runs; (2) the exported recording JSON is byte-identical, modulo a whitelisted timestamp field; (3)
the independent physics-step count is identical. Until it passes, every bundle carries
`determinism_verified: false` in its manifest and the Critic must treat all numeric claims as
provisional.

**Which timestamp is whitelisted, and nothing else is.** The recording export lands at
`<universe>/.eustress/knowledge/recordings/<space>/sim_<timestamp>.json`
(`eustress/crates/common/src/simulation/recorder.rs`, fired from
`eustress/crates/engine/src/simulation/plugin.rs:154`). The whitelist is exactly the wall-clock
creation timestamp field. Any other differing field is a determinism failure, not a second whitelist
candidate. State the exact field name you whitelisted in the artifact.

**What determinism machinery exists.** `eustress/crates/common/src/physics/determinism.rs` is 56
lines: a `GlobalRngSeed` resource and nothing else. `eustress/crates/engine/src/main.rs` pins
`Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`, and a `SolverConfig`.
`eustress/crates/common/tests/determinism.rs` exists but is gated behind a non-default `physics`
feature, so it has never run in a default test invocation — do not assume any determinism property is
currently enforced anywhere.

**Invariant I3**, `02` §2: "Fixed seed. `GlobalRngSeed` set explicitly; every other RNG consumer
seeded from it." Verifying that every RNG consumer is in fact seeded from it is part of this item's
diagnostic burden when a run diverges — a consumer seeded from system entropy is the most likely
single cause of a frame divergence and the artifact must name it if found.

**What exists, delivered by dependencies.** `G1.09`: `eustress-capture` with recipe parsing, the run
driver, hashing, the manifest writer, and `verify`; it accepts `--verify-determinism` today and fails
loudly as unimplemented. `G1.07`: the independent physics-step counter and its export, which is
assertion (3)'s subject. `G1.05`: resolution and output-path parameterisation. `G1.06`: the
tick-indexed trigger, which is what makes "the same frame" a well-defined phrase at all.

**A hazard.** `eustress/crates/engine/src/ai_camera.rs:133-149` documents a hard wgpu abort when two
`Camera3d`s both carry Bevy `Atmosphere`; the AI camera carries `NoAtmosphere` for that reason. Keep
it. Do not toggle `Msaa` or `Hdr` at runtime.

**Build reality.** 10–15 minutes per engine build, one at a time against the shared `target/`, never
killed mid-compile. Validate with `cargo run`, not `cargo check` — `cargo check` will not catch the
pipeline-level nondeterminism this item exists to find.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/capture/` — the determinism verification module and the diff reporting
- `eustress/crates/engine/src/bin/capture.rs` — only to wire `--verify-determinism` and register the
  `selftest-determinism` subcommand

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/simulation/clock.rs` — the accumulator-zeroing behaviour is a G2 item
- `eustress/spaces/harness/` — frozen by `G1.03`
- `eustress/crates/engine/src/default_scene.rs` and `photoreal.rs` — a nondeterministic render feature
  is an escalation, not a licence to disable it
- `eustress/crates/common/src/physics/determinism.rs` — owned by `G2.02` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Comparing downscaled images, hashing a crop, comparing with a per-pixel tolerance, widening the
  timestamp whitelist, excluding a frame set, lowering the capture resolution, or making the harness
  scene emptier are all measurement changes. If byte-identity is genuinely unachievable for a named,
  cited reason, report `EXIT_CRITERION_UNMEASURABLE` with the `path:line` of the nondeterministic
  source and stop.
- **Do not disable a render feature to force a pass.** A capture path that is deterministic because
  it renders less is not the capture path every downstream item is measuring. That is the front-matter
  escalation.
- The second capture must be a genuine second render. Returning a cached buffer, reusing a decoded
  image, or short-circuiting on an unchanged scene all produce byte-identity and prove nothing.
- Exactly one whitelisted field in the recording comparison. Name it. If a second field differs, that
  is a finding, not a whitelist entry.
- When a divergence is found, report *where*: the frame index, the byte offset of the first
  difference, and the count of differing bytes. A boolean `false` is not a diagnosis and the next
  agent in the ladder will need the signature.
- Batch verification. Twelve builds across three approaches; the selftest is cheap and should be run
  many times per build.

## 5. Exit criterion

### Criterion
Two consecutive captures of the frozen static harness scene `RH1_sphere_grid` from an identical pose
at 3840x2160 produce PNGs whose SHA-256 digests are **equal**; and running the two-scene selftest
recipe with `--verify-determinism` reports every frame byte-identical across the two runs, the
recording JSON byte-identical modulo exactly one whitelisted timestamp field, the independent
physics-step counts equal, and writes `determinism_verified: true` into the resulting manifest.

### Measurement

Command, part one — the simplest case, isolated:

    cd eustress && cargo run --release --bin eustress-capture -- selftest-determinism \
        --space spaces/harness/RH1_sphere_grid \
        --width 3840 --height 2160 \
        --position 6.0 3.5 9.0 --look-at 0.0 1.0 0.0 \
        --settle-ticks 30 \
        --out ../docs/PROMPTS/artifacts/G1.11/determinism_verification.json ; echo "SELF_EXIT=$?"

Command, part two — the full three-assertion gate:

    cd eustress && cargo run --release --bin eustress-capture -- \
        --recipe ../docs/PROMPTS/harness/recipes/G1_selftest.json \
        --subject HEAD --control HEAD \
        --out ../docs/PROMPTS/artifacts/bundles \
        --trials 1 --verify-determinism ; echo "DET_EXIT=$?"

    python -c "
    import json, glob, os, sys
    root = sorted(glob.glob('docs/PROMPTS/artifacts/bundles/*/'), key=os.path.getmtime)[-1]
    m = json.load(open(os.path.join(root,'manifest.json')))
    s = json.load(open('docs/PROMPTS/artifacts/G1.11/determinism_verification.json'))
    print('capture_a=%s capture_b=%s identical=%s frames_differing=%d recording_identical=%s stepcounts_equal=%s manifest_flag=%s'
          % (s['capture_a_sha256'][:16], s['capture_b_sha256'][:16], s['identical'],
             s['frames_differing'], s['recording_identical'], s['stepcounts_equal'],
             m['determinism_verified']))
    sys.exit(0 if s['identical'] and s['frames_differing']==0 and s['recording_identical']
             and s['stepcounts_equal'] and m['determinism_verified'] is True else 1)
    " ; echo "CHK_EXIT=$?"

Expected output shape:

    SELF_EXIT=0
    DET_EXIT=0
    capture_a=9f1c4d2e0a7b3856 capture_b=9f1c4d2e0a7b3856 identical=True frames_differing=0 recording_identical=True stepcounts_equal=True manifest_flag=True
    CHK_EXIT=0

Pass condition:

    SELF_EXIT == 0  AND  DET_EXIT == 0  AND  CHK_EXIT == 0

which asserts: the two selftest digests are equal; zero frames differ across the two full runs; the
recording comparison is identical modulo the single whitelisted field; the two step counts are equal;
and the manifest records `determinism_verified: true`.

Read the emitted digests and the four boolean fields. The JSON files existing is not a pass, and a
`determinism_verified: true` written without the three assertions having run is exactly the failure
`02` §5.2 exists to prevent.

## 6. Critic gate

`critic_gate` is `[]`. This item produces no visual change; its whole subject is whether two
identical things are identical. Note that it has an unusually large effect on Critic scoring even so:
`02` §5.2 records that until it passes, every bundle carries `determinism_verified: false` and the
Critic must treat all numeric claims as provisional and cap the simulation-trust dimension
accordingly. The mechanical criterion in §5 replaces the gate and is correspondingly tight: byte
equality, not similarity, across three independent assertions.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — run the recipe twice in separate engine sessions and diff the outputs
                 byte-for-byte, isolating divergence by frame index and byte offset
   -> if still failing, MANDATORY approach change. Increasing the settle-tick count is NOT an
      approach change; capturing twice within one engine session, so process-startup state is
      eliminated as a variable, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the two digests still differing AND the count of
                  differing bytes moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a render feature in the default studio camera path is proven inherently
                   nondeterministic (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER byte-identity to a stated
per-pixel tolerance, naming the consequence for `00_MASTER_PROTOCOL.md` §1.1 condition D2 and for
every downstream item that compares frames; FUND a specific approach D; DEFER behind a named blocking
item; or KILL, stating that the program's own definition of done then contains a condition it has
chosen not to meet.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.11/determinism_verification.json`

A reader finds: both selftest capture digests and the identity verdict; the settle-tick count that
achieved it; the frame-by-frame comparison summary from the full run, including for any divergence
the frame index, the first differing byte offset, and the differing byte count; the recording
comparison result with the single whitelisted field named explicitly; both independent step counts;
the `determinism_verified` value written into the manifest; the list of RNG consumers checked for
seeding from `GlobalRngSeed` and any found seeded from system entropy with its `path:line`; the
harness scene and its frozen hash from `G1.03`; the reference machine id `RM-1` with GPU and driver;
and the commit. This file is the W3 evidence for the item and is what `00_MASTER_PROTOCOL.md` §1.1
condition D2 is satisfied by.

## 9. Definition of NOT done

- The digests match at 1280x720 and differ at 3840x2160. Resolution-dependent determinism is the
  defect, not evidence against it.
- The digests match because the settle count was raised until the scene stopped changing, while the
  harness scene contains an animated element the count merely outran. The harness scenes are static
  by construction; if one is not, that is a `G1.03` finding to report.
- Byte-identity is achieved by writing the second PNG from a cached buffer rather than re-rendering.
  The second capture must be a genuine second render.
- The recording comparison passes because the whitelist grew to cover every field that differed.
  Exactly one field is whitelisted and it is named in the artifact.
- The step counts are equal because both were read from the clock rather than from the independent
  counter `G1.07` built. Assertion (3) is about the independent counter specifically.
- `determinism_verified: true` is written by the manifest writer without the assertions having
  executed. That inverts the entire purpose of the flag.
- A nondeterministic render feature was disabled to force a pass, so the capture path is now
  deterministic and different from the one every downstream item measures.
- The report says `identical: false` with no frame index and no byte offset. The next agent in the
  ladder receives the failure signature, and a boolean is not a signature.
````

---

## G1.12 — The 33 capture recipes in one library-wide format

Item path: `docs/PROMPTS/items/G1.12_capture-recipes.md`

````markdown
---
id: G1.12
title: All 33 cited capture recipes authored once in a single library-wide format
workload: W3
workload_secondary: [W1, W6]
phase: G1
depends_on: [G1.02, G1.09]
blocks: [G1.13, G1.14]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.12/recipe_inventory.json
escalation: >
  If a cited recipe cannot be authored because the item citing it declares a frame set, a scene, or
  a setting that contradicts 02_CAPTURE_HARNESS.md - a compressed capture, a scene id that does not
  exist and no item owns, a resolution below the one its own threshold is stated at - STALL with the
  citing item id and the contradiction named. Do not resolve the contradiction by silently choosing
  one side; that decision belongs to L0 because it crosses packs.
status: DRAFT
notes: >
  Tier L rather than S despite compiling nothing: 33 recipe files plus a normative schema document
  plus a validator pass over all of them exceeds the S token envelope. The 12 build slots are a
  ceiling, not a requirement - the only builds needed are to run the already-built eustress-capture
  in dry-run mode.
---

## 1. Objective

Every capture recipe path cited anywhere in the prompt library exists, at exactly the cited path, in
exactly one format defined by one normative schema document. The set of recipe files on disk equals
the set of recipe paths cited by the packs — no missing recipe, and no invented recipe that no item
uses.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian. Units are meter-native.

**The defect this item closes.** Across the seven authored packs, **53 items** declare a
`capture_recipe:` pointing under `docs/PROMPTS/harness/recipes/`, spanning **33 distinct paths**. The
directory does not exist and not one of the 33 files has been written. Worse, five packs instruct
their own executing agent to author the missing file if absent — for example `T3_studio_ux.md:1069`
says the recipe should be authored "against the schema in §8 of that document rather than inventing a
new format", and `T5_robustness_and_cohesion.md:2596` says "If [the recipe] does not yet exist, this
item authors it, following the recipe format the capture harness defines". Five cold agents following
that instruction independently would produce five incompatible formats. This item authors all 33
once so that never happens.

**The 33 recipes and the items that cite them.** This table is the contract. Author exactly these
paths, no more and no fewer.

| # | Recipe path (under `docs/PROMPTS/harness/recipes/`) | Cited by | Primary scenes |
|---|---|---|---|
| 1 | `T1_RH1_pbr_grid.json` | `G3.03` | S1 |
| 2 | `T1_RH1_exposure.json` | `G3.04` | S1, S2 |
| 3 | `T1_RH1_material_roundtrip.json` | `G3.09` | S1 |
| 4 | `T1_RH2_ao.json` | `G3.06` | S2 |
| 5 | `T1_RH2_reflection.json` | `G3.07` | S2 |
| 6 | `T1_RH3_shadow.json` | `G3.05` | S3 |
| 7 | `T1_RH3_aliasing.json` | `G4.02` | S3 |
| 8 | `T1_RH3_dolly.json` | `G4.03` | S3 |
| 9 | `T1_RH3_atmosphere.json` | `G3.08` | S3 |
| 10 | `T1_RH3_presets.json` | `G3.11` | S3 |
| 11 | `T1_RH4_cascade_dolly.json` | `G4.04` | `RH4_cascade` (T1-owned) |
| 12 | `T1_RH5_hlod_swap.json` | `G5.01` | `RH5_hlod` (T1-owned) |
| 13 | `T1_RH6_roundtrip.json` | `G5.02` | `RH6_roundtrip` (T1-owned) |
| 14 | `T1_RH7_splat_composite.json` | `G3.10` | `RH7_splat_composite` (T1-owned) |
| 15 | `T1_RH8_opening.json` | `G3.12` | `RH8_opening` (T1-owned) |
| 16 | `T1_FLAGSHIP.json` | `G3.13` | `FLAGSHIP` (T1-owned) |
| 17 | `G2_sim_evidence.json` | `G2.04`, `G2.05`, `G2.06`, `G2.07`, `G2.08`, `G2.09`, `G2.10`, `G2.11`, `G2.13`, `G2.14`, `G2.15`, `G2.16`, `G5.22` | S5, S6 |
| 18 | `G6_ui_sequences.json` | `G6.04`, `G6.05`, `G6.07`, `G6.08`, `G6.10`, `G6.11` | S4 |
| 19 | `G6_ribbon_honesty.json` | `G6.06` | S4 |
| 20 | `G6_theme_coherence.json` | `G6.09` | S4 |
| 21 | `G6_density.json` | `G6.12` | S4 |
| 22 | `G6_first_task.json` | `G6.14` | S4 |
| 23 | `G6_final_blind.json` | `G6.15` | S4, S2 |
| 24 | `G7_agent_camera.json` | `G7.09` | S3, S6 |
| 25 | `G7_detect_propose.json` | `G7.10` | S6 |
| 26 | `G7_etask_spec.json` | `G7.12` | S6 |
| 27 | `G7_time_compression.json` | `G7.15` | S5 |
| 28 | `G7_scoring.json` | `G7.18`, `G7.19` | S6 |
| 29 | `G7_benchmark.json` | `G7.22`, `G7.23`, `G7.24` | S5, S6 |
| 30 | `G7_error_surface.json` | `G7.42` | S4 |
| 31 | `G7_cohesion_journey.json` | `G7.45` | S4, S2 |
| 32 | `B1_validation_report.json` | `G2.33` | S6 |
| 33 | `B1_vertical_demo_path.json` | `G6.31` | S4, S6 |

Two paths appear in the normative documents as illustrative examples and are cited by **no** item —
`docs/PROMPTS/harness/recipes/G3_material.json` in `02_CAPTURE_HARNESS.md` §8, and
`docs/PROMPTS/harness/recipes/G3_shadow_falloff.json` in `03_PROMPT_SCHEMA.md` §6. They are examples
inside specification documents, not deliverables. **Do not author them**; authoring a recipe no item
uses is exactly the invention this item exists to prevent.

`docs/PROMPTS/harness/recipes/G1_selftest.json` already exists, authored by `G1.09` as its own cheap
exit-criterion fixture. It is not one of the 33; leave it alone but validate it against the same
schema.

**The recipe format is derived, not invented.** `02_CAPTURE_HARNESS.md` §8 states what a recipe is:
"The recipe file names the scenes, paths, frame sets, resolution, and settings." Every field of the
schema you write must be traceable to a normative clause:

| Recipe block | Source |
|---|---|
| `scenes` | `02` §3, the `S1`–`S6` table and the freeze rule |
| `paths` | `02` §4.1, the `CP-A/B/S/U` table |
| `frame_sets` | `02` §4.2, the frame-set table with its tick indices |
| `render` | `02` §5, the `render.*` settings block; invariant I5 |
| `settings` | `02` §5, the seed and physics block; §5.1 for `sim_time_scale` |
| `trials` | `02` §6.1 step 6, five preference trials |
| `reference_machine` | `G1.02`, `docs/PROMPTS/harness/reference_machines.json` |
| `determinism` | `02` §5.2 |

**Hard constraints the schema must enforce at parse time**, not at run time:

1. `settings.sim_time_scale` must be exactly `1.0`. `02` §5.1 forbids time compression inside a
   capture run, because `eustress/crates/common/src/simulation/clock.rs:100-102` zeroes the
   accumulator on saturation and the clock then reports time that was never stepped. A recipe
   declaring any other value is malformed.
2. `reference_machine` must name a machine present in `docs/PROMPTS/harness/reference_machines.json`.
3. Every `frame_sets[].scene_id` must appear in `scenes[]`, and every `frame_sets[].path` must be one
   of `CP-A`, `CP-B`, `CP-S`, `CP-U`.
4. `render.width` and `render.height` must be declared; there is no default. Invariant I5 requires a
   declared resolution.

**`G7_time_compression.json` needs care and is the reason constraint 1 is stated so bluntly.** `T4`
item `G7.15` is about time compression, and constraint 1 forbids capturing under compression. Both
hold: that recipe captures frames at `sim_time_scale = 1.0` showing the *instrumentation* that
surfaces the step drop, and carries the compression evidence in a separate, explicitly labelled
`compression_experiment` block whose output is a measurement series, not a frame set. That block is
schema-valid and clearly marked as a non-capture leg, exactly as `02` §5.1 requires: "Any item that
needs compressed time must run it in a separate, explicitly-labelled experiment."

**Scenes some recipes name are not yet authored, and that is expected.** `G1.03` delivered `S1`–`S6`
under `eustress/spaces/harness/` as `RH1_sphere_grid`, `RH2_interior`, `RH3_exterior`,
`S4_studio_ui`, `S5_physics_stress`, `S6_domain_sim`. Six of the 33 recipes name T1-owned extension
scenes (`RH4_cascade`, `RH5_hlod`, `RH6_roundtrip`, `RH7_splat_composite`, `RH8_opening`,
`FLAGSHIP`) that the T1 items authoring them will create. A recipe naming an unauthored scene is
**schema-valid and not yet runnable**, and the validator must distinguish those two states with
different exit paths: a malformed recipe is a failure of this item, while a not-yet-runnable recipe
is a correctly-authored recipe waiting on another item. This item's exit criterion counts
schema-valid recipes, never runnable ones.

**Two recipes need the domain simulation named.** `02` §3 says `S6` "is bound per item; the item's
prompt names which domain." Where a recipe's citing item does not name a domain, the recipe declares
`scene_id: "S6"` with `domain: null` and a comment stating the citing item must bind it. Do not
invent a domain.

**What exists.** `G1.09` delivered `eustress-capture` with recipe parsing. `G1.02` delivered the
reference machine registry. Use both.

**Build reality.** This item compiles nothing new. The only cargo invocations are runs of the
already-built `eustress-capture` in dry-run mode. Still: one cargo build at a time against the shared
`target/`, never killed mid-compile.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/harness/RECIPE_SCHEMA.md` — new file, the single normative recipe format
- `docs/PROMPTS/harness/recipes/*.json` — the 33 files in the table above, and only those
- `eustress/crates/engine/src/capture/` — **only** the recipe parser's validation rules, and only to
  enforce the four hard constraints above if `G1.09`'s parser does not already

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- **Any file under `docs/PROMPTS/packs/`** — if a citing item's declared threshold or frame set
  contradicts the harness spec, that is the front-matter escalation and an L0 decision, because it
  crosses packs
- `docs/PROMPTS/02_CAPTURE_HARNESS.md` and `docs/PROMPTS/03_PROMPT_SCHEMA.md` — normative; this item
  derives from them
- `docs/PROMPTS/harness/recipes/G1_selftest.json` — owned by `G1.09`; validate it, do not rewrite it
- `docs/PROMPTS/harness/reference_machines.json` — owned by `G1.02`
- `eustress/spaces/harness/` — frozen by `G1.03`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Authoring 30 recipes and declaring the remaining three out of scope, relaxing the set-equality
  check to a subset check, marking a required schema field optional because one recipe was awkward,
  or lowering a recipe's declared resolution below the resolution its citing item's threshold is
  stated at are all measurement changes.
- **Do not author a recipe no item cites.** The set on disk must equal the set cited. An extra file
  is as much a failure as a missing one, because an unused recipe is a format that will drift.
- One schema, one document. Every field in every recipe is defined in
  `docs/PROMPTS/harness/RECIPE_SCHEMA.md`, and every field in that document is traceable to a clause
  in `02_CAPTURE_HARNESS.md` or to `G1.02`. A field justified only by convenience does not belong.
- Where a citing item declares a resolution or a frame set explicitly in its own measurement block,
  the recipe must match it. `T5` `G7.42` at `T5_robustness_and_cohesion.md:2596-2599` declares the
  `error_surface` sequence at 1920x1080, 2560x1440, and 3840x2160; the recipe records exactly those
  three sizes. Read each citing item's measurement block before authoring its recipe.
- Never set `publication_rights` to anything but `internal_calibration_only` in any recipe or in any
  control block. `02` §7.3 reserves `publishable` to the human, and `00_MASTER_PROTOCOL.md` §6.1
  makes an external comparison a human-only decision.

## 5. Exit criterion

### Criterion
The set of `*.json` files under `docs/PROMPTS/harness/recipes/`, excluding `G1_selftest.json`, is
**exactly equal** to the set of 33 recipe paths cited by `capture_recipe:` declarations across
`docs/PROMPTS/packs/*.md`; and all 34 files, including `G1_selftest.json`, validate against
`docs/PROMPTS/harness/RECIPE_SCHEMA.md` with zero errors.

### Measurement

Command, part one — set equality between cited and on-disk:

    python -c "
    import glob, os, re, sys
    cited = set()
    for f in glob.glob('docs/PROMPTS/packs/*.md'):
        for line in open(f, encoding='utf-8'):
            m = re.match(r'^capture_recipe:\s+(docs/PROMPTS/harness/recipes/\S+\.json)', line.rstrip())
            if m: cited.add(m.group(1))
    disk = {p.replace(os.sep,'/') for p in glob.glob('docs/PROMPTS/harness/recipes/*.json')}
    disk.discard('docs/PROMPTS/harness/recipes/G1_selftest.json')
    print('cited=%d on_disk=%d' % (len(cited), len(disk)))
    print('missing=%s' % sorted(cited - disk))
    print('uncited=%s' % sorted(disk - cited))
    sys.exit(0 if cited == disk and len(cited) == 33 else 1)
    " ; echo "SET_EXIT=$?"

Command, part two — schema validation of every recipe by the real parser:

    cd eustress && cargo run --release --bin eustress-capture -- validate-recipes \
        --dir ../docs/PROMPTS/harness/recipes \
        --schema ../docs/PROMPTS/harness/RECIPE_SCHEMA.md \
        --out ../docs/PROMPTS/artifacts/G1.12/recipe_inventory.json ; echo "VAL_EXIT=$?"

`validate-recipes` parses every recipe with the same parser `eustress-capture` uses for a real run,
enforces the four hard constraints, and classifies each recipe as `valid_runnable` (every named scene
exists on disk) or `valid_pending_scene` (schema-valid, one or more scenes not yet authored, with the
owning item named). It exits non-zero on any `invalid`.

Expected output shape:

    cited=33 on_disk=33
    missing=[]
    uncited=[]
    SET_EXIT=0
    recipes=34 valid_runnable=27 valid_pending_scene=7 invalid=0
    time_scale_violations=0 unknown_reference_machine=0 undeclared_resolution=0 dangling_scene_ref=0
    VAL_EXIT=0

Pass condition:

    SET_EXIT == 0  AND  VAL_EXIT == 0  AND  invalid == 0  AND  cited == 33

Read `missing`, `uncited`, and `invalid`. Files existing in the directory is not a pass; both set
equality and parser validation are required, and `uncited` being non-empty fails just as hard as
`missing`.

## 6. Critic gate

`critic_gate` is `[]`. A recipe is the instruction that produces a bundle, not a bundle; there is
nothing for a blinded evaluator to look at, and this item's own `capture_recipe` field is `none`
precisely because an item that authors the recipes cannot cite one. The mechanical criterion in §5
replaces the gate and is correspondingly tight: exact set equality in both directions, plus
validation by the same parser that will run the recipes for real.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — derive the schema from 02_CAPTURE_HARNESS.md clause by clause, then
                 author the 33 recipes against it, reading each citing item's measurement block first
   -> if still failing, MANDATORY approach change. Adding a field to accommodate one awkward recipe
      is NOT an approach change; generating the recipes from the citing items' declared measurement
      blocks mechanically, with the schema as the output rather than the input, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the same recipe still invalid AND the total invalid
                  count moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: a citing item declares a frame set, scene, or setting contradicting
                   02_CAPTURE_HARNESS.md (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the recipe set to a named
subset, listing exactly which of the 53 citing items lose their capture recipe as a result; FUND a
specific approach D; DEFER behind a named blocking item; or KILL, stating that 53 items across six
packs then have an unresolvable `capture_recipe` field and five packs will each invent their own
format.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.12/recipe_inventory.json`

A reader finds: the 33 cited paths and the 33 on-disk paths with the set-difference in both
directions; per recipe, its id, the items citing it, its scenes with their `scene_id` and
`space_dir`, its frame sets, its declared resolution, its `reference_machine`, its validation status
(`valid_runnable` / `valid_pending_scene` with the owning item named / `invalid`), and its SHA-256;
the counts of each hard-constraint violation, all of which must be zero; the SHA-256 of
`docs/PROMPTS/harness/RECIPE_SCHEMA.md`; and the commit. This file is the W3 evidence for the item.
`G1.13` cites it as the reason a single command can be run against any recipe in the library.

Alongside it, `docs/PROMPTS/harness/RECIPE_SCHEMA.md` is the normative format document. Every field
it defines carries the `02_CAPTURE_HARNESS.md` clause it derives from, so a later reader can tell
what is specification and what is implementation convenience — and there should be none of the
latter.

## 9. Definition of NOT done

- Thirty-two recipes are authored and one is deferred to the item that cites it. That is the exact
  outcome — one agent authoring one recipe in its own format — this item exists to prevent.
- A thirty-fourth recipe was authored because it seemed useful. An uncited recipe is a format that
  will drift, and the set-equality check fails in that direction too.
- `G3_material.json` or `G3_shadow_falloff.json` was authored because it appears in a normative
  document. Those are illustrations inside specifications, cited by no item.
- A recipe declares `sim_time_scale` other than `1.0`. `02` §5.1 forbids it, the parser must refuse
  it, and `G7_time_compression.json` is the one that will be tempted.
- `G7_time_compression.json` was authored as a compressed capture. Its frames are captured at 1.0 and
  its compression evidence lives in a separate, explicitly labelled experiment block.
- A recipe declares a resolution below the resolution its citing item's own threshold is stated at,
  so the threshold becomes unmeasurable from that recipe's bundle.
- A recipe naming a T1-owned scene was marked invalid rather than `valid_pending_scene`. A
  correctly-authored recipe waiting on another item is not a defect, and conflating the two states
  hides real defects.
- The schema document defines a field that no clause in `02_CAPTURE_HARNESS.md` and no delivered
  dependency justifies. Every field is traceable or it is removed.
- A recipe or control block sets `publication_rights` to `publishable`. Only the human may set that,
  and an agent setting it is a licence exposure, not a formatting choice.
````

---

## G1.13 — Clean-checkout one-command G1 gate

Item path: `docs/PROMPTS/items/G1.13_clean-checkout-gate.md`

````markdown
---
id: G1.13
title: One command produces a valid bundle from a clean checkout with no operator-local state
workload: W3
workload_secondary: [W6]
phase: G1
depends_on: [G1.10, G1.11, G1.12]
blocks: []
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.13/clean_checkout_gate.json
escalation: >
  If the capture succeeds in the operator's working tree and fails in the clean checkout, STALL with
  the specific piece of operator-local state named and its absolute path. Copying that state into
  the clean checkout to make the run pass is forbidden - it is the entire defect the item measures.
status: DRAFT
notes: >
  Tier L. This is the G1 phase exit condition, amended per pack note N1 from "on a machine that is
  not the founder's" to "from a clean checkout with no operator-local state", with the
  second-machine requirement converted into an explicit human decision this item emits rather than
  silently drops.
---

## 1. Objective

A single `eustress-capture` invocation, run in a freshly cloned working tree containing no
operator-local state, produces a content-addressed bundle with a valid provenance manifest, a blinded
tree that passes the leak checklist, and `determinism_verified: true`. The item also emits a
one-screen decision request putting the second-machine question to the human explicitly rather than
leaving the phase exit condition quietly unmet.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
licence is PolyForm Shield 1.0.0; say source-available. Physics is Avian. Slint is Rust.

**The phase exit condition, and the amendment.** `docs/PROMPTS/00_MASTER_PROTOCOL.md` §3.2 sets G1's
exit condition as: the harness "runs from a single command, on a machine that is not the founder's,
and produces a content-addressed bundle with a valid provenance manifest."
`02_CAPTURE_HARNESS.md` §8 repeats the phrase. The operator is a solo founder with one Windows box,
so "a machine that is not the founder's" is unmeetable as written, and an unmeetable exit condition
converts G1 into a phase that can never close — which blocks all seven phases behind it, since every
one of their exit conditions is stated in terms of a measurement G1 produces.

The condition this item measures is therefore the amended one: **from a clean checkout of the
repository at a named commit, in a working directory containing no operator-local state, with every
path in the recipe resolved relative to that checkout.**

**What "no operator-local state" means concretely**, and this is the substance of the item:

- A fresh `git clone` into a directory that has never been used for this project, not a `git clean`
  of the existing tree. A `git clean` leaves the shared `target/`, the user profile, and every
  environment variable in place.
- Every `EUSTRESS_*` environment variable cleared. The known set includes `EUSTRESS_PROFILE`,
  `EUSTRESS_PROFILE_FRAMES`, `EUSTRESS_CAPTURE`, `EUSTRESS_CAPTURE_DIR`, `EUSTRESS_SHADOW_DISTANCE`,
  `EUSTRESS_HLOD_RADIUS`, and `EUSTRESS_SPLAT_BUDGET`; enumerate the live environment and clear every
  match rather than relying on that list being complete.
- `HOME` and `USERPROFILE` redirected to an empty temporary directory, so any config, cache, or
  credential the engine would read from a user profile is absent.
- A separate `CARGO_TARGET_DIR` inside the clean checkout, so the shared `target/` cannot supply a
  stale artifact. Note the consequence honestly: this forces a full cold build, which is the single
  largest wall-clock cost in this item.
- No pre-existing Universe or Space outside the checkout. The recipe's scene paths resolve inside it.

**What the run must produce**, all of which the dependencies already deliver and this item only
composes: `G1.09` gives `eustress-capture`, content-addressed bundles, and the manifest writer and
`verify`; `G1.10` gives `blind` and the leak checklist; `G1.11` gives `--verify-determinism`;
`G1.12` gives the 33 recipes and the schema; `G1.03` gives the frozen scenes; `G1.02` gives the
reference machine registry.

**What this run cannot prove, and must say so.** A clean checkout on the same physical box does not
establish machine independence. A GPU-driver-dependent render difference, a driver-version-dependent
shader compilation result, or an OS-build-dependent timing characteristic would survive this test
undetected. That residual risk is recorded in the artifact in those words, not softened.

**The second machine is a human decision, not a silent omission.** `00_MASTER_PROTOCOL.md` §6 lists
the human-only decisions, and §5.3 gives the stall-packet form for putting one to the human. This
item emits a decision request in that form even on a pass, offering exactly two named options: procure
or borrow a second machine and re-run this item as `G1.13b` against `RM-2`; or accept the
clean-checkout substitute and record the residual risk in the phase report. The registry `G1.02`
built is already a list precisely so `RM-2` can be added without a schema change.

**Bundle size.** `02` §4.2 computes 1,641 frames per full bundle side, on the order of 8–14 GB per
side at 3840x2160. Run this gate against a recipe sized for the criterion, not against
`T1_FLAGSHIP.json`. `G6_ui_sequences.json` is a reasonable choice — it exercises the UI capture path,
the three window sizes, and the blinding tree, without 14 GB of frames.

**Build reality.** A cold full build in a fresh `CARGO_TARGET_DIR` is longer than the usual 10–15
minutes because nothing is cached. One cargo build at a time against any given target directory,
never killed mid-compile. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/harness/CLEAN_CHECKOUT_GATE.md` — new file, the reproducible procedure
- `docs/PROMPTS/harness/clean_checkout_gate.ps1` — new file, the script that performs the run
- `eustress/crates/engine/src/capture/` — **only** if the run surfaces a path that is resolved
  against the operator's home directory or an absolute machine path rather than the checkout root

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass; the CI capture job is
  `02` build item B11 and belongs to G7
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/00_MASTER_PROTOCOL.md` — the amendment is recorded in this pack's notes and in this
  item's artifact; amending the protocol document itself is an L0 action
- `eustress/spaces/harness/` — frozen by `G1.03`
- `docs/PROMPTS/harness/recipes/` — owned by `G1.12`
- `docs/PROMPTS/harness/reference_machines.json` — owned by `G1.02`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Reusing the shared `target/`, leaving one `EUSTRESS_*` variable set, running in the existing
  working tree after a `git clean`, copying a config file into the clean checkout, or shrinking the
  recipe after a failure are all measurement changes. If the gate is genuinely unmeasurable, report
  `EXIT_CRITERION_UNMEASURABLE` and stop.
- **Never copy operator-local state into the clean checkout.** If the run needs something the
  checkout does not contain, that thing is a missing repository asset or a hardcoded absolute path,
  and naming it is the most valuable output this item can produce.
- One command. The criterion is that a single `eustress-capture` invocation produces the bundle. A
  setup script that clones and clears the environment is permitted and expected; a setup script that
  performs part of the capture is not.
- Emit the human decision request whether the item passes or fails. A pass that quietly drops the
  second-machine requirement is exactly what the amendment exists to avoid.
- State the residual risk in the words given in §2. Do not soften it.

## 5. Exit criterion

### Criterion
In a freshly cloned working tree with `HOME`/`USERPROFILE` redirected to an empty directory, every
`EUSTRESS_*` variable cleared, and a checkout-local `CARGO_TARGET_DIR`, a single `eustress-capture`
invocation exits 0 and produces a bundle for which: the directory name equals the SHA-256 of its
`manifest.json`; `verify --leak-check` exits 0; `determinism_verified` is `true`; and the per-side
frame counts are equal and non-zero.

### Measurement

Command:

    pwsh -NoProfile -File docs/PROMPTS/harness/clean_checkout_gate.ps1 `
        -Commit HEAD `
        -Recipe docs/PROMPTS/harness/recipes/G6_ui_sequences.json `
        -Out docs/PROMPTS/artifacts/G1.13/clean_checkout_gate.json ; echo "GATE_EXIT=$?"

The script must perform, in order and with no step skipped: create an empty temporary root; clone the
repository at `-Commit` into it; set `HOME` and `USERPROFILE` to an empty directory inside that root;
enumerate and clear every environment variable matching `EUSTRESS_*`; set `CARGO_TARGET_DIR` inside
the clone; then run exactly one capture command inside the clone —

    cargo run --release --bin eustress-capture -- \
        --recipe <recipe> --subject HEAD --control HEAD \
        --out docs/PROMPTS/artifacts/bundles --trials 5 --verify-determinism

— and then, still inside the clone, `verify --leak-check` on the produced bundle. It records every
exit code and every asserted value, and exits non-zero if any assertion fails.

Expected output shape:

    clone_root=C:\...\g113\repo commit=71ccf6fe
    env_cleared=EUSTRESS_PROFILE,EUSTRESS_PROFILE_FRAMES,EUSTRESS_CAPTURE_DIR
    home_redirected=True cargo_target_dir_local=True
    capture_exit=0
    bundle=3ad9f10c... name_matches_manifest_digest=True
    determinism_verified=True
    frames_alpha=63 frames_beta=63 counts_equal=True
    verify_leakcheck_exit=0
    GATE_EXIT=0

Pass condition:

    GATE_EXIT == 0
      AND capture_exit == 0
      AND name_matches_manifest_digest == True
      AND determinism_verified == True
      AND counts_equal == True AND frames_alpha > 0
      AND verify_leakcheck_exit == 0

Read every asserted value and every exit code. A bundle appearing in the clean checkout is not a
pass; the four assertions are.

## 6. Critic gate

`critic_gate` is `[]`. This is the phase's own gate, not an artifact a Critic scores — it verifies
that the machine which *produces* Critic input works from nothing. The mechanical criterion in §5
replaces the gate and is correspondingly tight: four independent assertions inside an environment
constructed specifically to remove every advantage the operator's machine confers.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — clone, sanitise the environment, run the single command, assert
   -> if still failing, MANDATORY approach change. Clearing one more environment variable is NOT an
      approach change; running the whole gate inside a container or a fresh Windows user profile,
      so the sanitisation is enforced by the boundary rather than by a script, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations failing on the same missing piece of local state
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the capture succeeds in the operator's tree and fails in the clean checkout
                   (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the gate to a run in the
operator's own working tree, stating that phase G1 then has no reproducibility evidence at all; FUND
a specific approach D; DEFER behind a named blocking item; or KILL, stating that
`00_MASTER_PROTOCOL.md` §3.2's G1 exit condition is then not met in any form.

**Separately, and on a pass as well as on a failure**, this item emits the second-machine decision
request to the human, in the §5.3 packet form, offering exactly two options: (a) PROCURE or borrow a
second machine, add it to the registry as `RM-2`, and re-run this gate as `G1.13b`; or (b) ACCEPT the
clean-checkout substitute, with the residual risk — that a GPU-driver-dependent, driver-version-
dependent, or OS-build-dependent difference would survive this test undetected — recorded in the
phase report. The recommendation is (b) for now with (a) revisited before any external claim of
reproducibility is made, because `00_MASTER_PROTOCOL.md` §1.1 condition D3 puts the harness in CI on
a GPU runner, and that runner is the second machine arriving by another route.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.13/clean_checkout_gate.json`

A reader finds: the clone root and the commit; the full list of environment variables enumerated and
cleared; confirmation that `HOME`/`USERPROFILE` were redirected and that `CARGO_TARGET_DIR` was
checkout-local, with the paths; the single capture command exactly as run and its exit code; the
bundle directory name and the independently computed manifest digest with the equality verdict;
`determinism_verified`; both per-side frame counts; the `verify --leak-check` exit code; the recipe
path and its SHA-256 from `G1.12`; the reference machine id `RM-1`; the wall-clock cost of the cold
build; the residual-risk statement in the words given above; and the second-machine decision request
in the `00_MASTER_PROTOCOL.md` §5.3 packet form. This file is the W3 evidence for the item and is
what an L1 cites when declaring phase G1 complete.

Alongside it, `docs/PROMPTS/harness/CLEAN_CHECKOUT_GATE.md` documents the procedure so a stranger can
re-run it without this item's agent.

## 9. Definition of NOT done

- The gate ran in the operator's existing working tree after a `git clean`. That leaves the shared
  `target/`, the user profile, and the environment intact, which is most of what the gate exists to
  remove.
- One `EUSTRESS_*` variable was left set because it "only affects profiling". Enumerate and clear
  every match; a variable that only affects profiling still affects the frame-time series the
  manifest records.
- `CARGO_TARGET_DIR` pointed at the shared `target/`, so a stale artifact from the operator's tree
  supplied something the clean checkout does not contain.
- A config file, a Universe, or a credential was copied into the clean checkout to make the run
  succeed. That is the defect, not the fix.
- The bundle was produced but `determinism_verified` is `false`, and the item passed anyway. `02`
  §5.2 makes that flag the difference between an admissible bundle and a provisional one.
- The gate passed and the second-machine decision request was not emitted, so the amended exit
  condition silently replaced the original. The amendment is on the record or it is not an amendment.
- The residual-risk statement was softened to "minor residual risk". A GPU-driver-dependent
  difference surviving undetected is not minor; it is the exact class of difference this program
  measures.
- The gate ran against `T1_FLAGSHIP.json` and consumed 14 GB per side to prove a property a
  63-frame recipe proves equally well.
````

---

## G1.14 — External reference control and publication-rights memo (**HUMAN-EXECUTED**)

Item path: `docs/PROMPTS/items/G1.14_external-reference-control.md`

````markdown
---
id: G1.14
title: HUMAN-EXECUTED external reference control artifact and publication-rights determination
workload: W1
workload_secondary: [W3]
phase: G1
depends_on: [G1.02, G1.12]
blocks: []
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.14/reference_control_manifest.json
escalation: >
  If the procurement brief cannot be written without the agent selecting a specific commercial
  product to capture, STALL and hand the selection to the human. Naming the product is a licensing
  exposure decision, not a technical one, and 00_MASTER_PROTOCOL.md §6.1 reserves it to the human.
status: DRAFT
notes: >
  HUMAN-EXECUTED. The agent produces two documents: a procurement brief and a rights-memo template.
  A human supplies the reference artifact itself and the completed rights determination. The exit
  criterion is a countable external fact - a populated manifest control block that the verifier
  accepts - not the existence of a document. Tier S because the agent's half compiles nothing and
  writes two files.
---

## 1. Objective

The program's own definition of done requires a comparison against something the project does not
own. This item makes that comparison procurable: an agent-written procurement brief states exactly
what artifact is needed and in what form, an agent-written rights-memo template states exactly what
determination must accompany it, and a human supplies both. The item passes when
`eustress-capture verify` accepts a bundle whose control side is a complete
`kind: external_reference` block — a countable external fact, not a document that says one exists.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
licence is PolyForm Shield 1.0.0; the project is **source-available**, never open source. Physics is
Avian.

**Why this item exists.** `docs/PROMPTS/00_MASTER_PROTOCOL.md` §1.1 condition **D1** — one of four
conditions on the program being done at all — requires: "A blind Critic, shown randomised unlabelled
captures of Eustress and of a licensed reference artifact, scores Eustress at or above 8.0 on every
rubric dimension and selects the Eustress side on the preference question in at least 4 of 5
independent trials." No item anywhere in the 133-item library procures that reference artifact. D1 is
therefore unowned, and once achieved would be unpublishable, because the rights question has never
been answered.

**The two rules that shape this item**, `00_MASTER_PROTOCOL.md` §4.1:

- **R1 — Internal calibration is unrestricted.** "Capturing output from any commercial engine or
  platform to calibrate our own quality bar, internally, is permitted and encouraged. A quality bar
  you cannot see is not a bar."
- **R2 — Publication is restricted to artifacts we are licensed to publish.** No side-by-side,
  screenshot, frame, video, or derived score naming a third-party product may appear in any external
  surface unless the project holds publication rights. "Several commercial engine and platform EULAs
  contain benchmarking and publication clauses; assume one applies until counsel says otherwise.
  **Only the human may authorise a published comparison.** An agent that finds itself drafting a
  public comparison must stop and escalate."

`02_CAPTURE_HARNESS.md` §7.3 adds that `publication_rights` defaults to `internal_calibration_only`,
that only the human may set it to `publishable`, and — in terms this item must respect exactly —
"**No agent may draft, generate, or stage an external-facing side-by-side.**"

**What that means for you, the agent executing this item.** You write two documents and nothing else.
You do not select the product. You do not acquire the artifact. You do not perform the capture. You
do not fill in the rights determination. You do not draft any comparison. The reason is not caution
for its own sake: naming a specific commercial product as the comparison target is a licensing
exposure decision under R2, and `00_MASTER_PROTOCOL.md` §6.1 reserves it to the human.

**The provenance block the human's artifact must satisfy**, `02` §7.3, is complete and non-negotiable.
A `control` of `kind: external_reference` is **inadmissible** — the Critic rejects the bundle rather
than scoring it — unless every one of these is populated: `product`, `product_version`,
`scene_source`, `scene_license`, `publication_rights`, `capture_operator`, `capture_date_utc`, and
the full `hardware` block (`cpu`, `gpu`, `gpu_driver`, `ram_gb`, `os`). `00_MASTER_PROTOCOL.md` §4.1
R3 states the principle: "A reference nobody can reproduce is not a reference."

**The default comparison, and why this item is a ceiling check rather than a replacement for it.**
`00_MASTER_PROTOCOL.md` §4.1 R4: "The strongest bar that is always legal to publish is our own prior
build. Every item should produce an `eustress@<commit-a>` vs `eustress@<commit-b>` bundle as its
default comparison, with the external reference used as an internal ceiling check only." `02` §7.3
repeats it: "The always-legal default comparison is `eustress@<commit-a>` vs `eustress@<commit-b>`.
Prefer it." Every one of the 33 recipes `G1.12` authored is built around that default. This item adds
a ceiling, not a replacement.

**What exists.** `G1.02` delivered the reference machine registry, which is where the control block's
`hardware` values come from when the reference is captured on `RM-1`, and which is a list so a
different capture machine can be added. `G1.12` delivered the 33 recipes and
`docs/PROMPTS/harness/RECIPE_SCHEMA.md`. `G1.09` delivered `eustress-capture verify`, which
implements the §7.3 rejection and is the instrument this item's exit criterion uses.

**What a "reference scene" means here, in technical terms the brief must state.** For the comparison
to be meaningful the reference artifact must be capturable under conditions comparable to a harness
bundle: a static scene, an enumerable camera pose, a declared resolution matching the recipe's, a
declared colour pipeline, and enough frames to fill the frame sets the comparison recipe declares.
A marketing render, a video with unknown encoding, or a screenshot at an unknown resolution cannot be
compared, and the brief must say so plainly so the human does not procure the wrong thing.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/harness/EXTERNAL_REFERENCE_BRIEF.md` — new file, the procurement brief
- `docs/PROMPTS/harness/RIGHTS_MEMO_TEMPLATE.md` — new file, the publication-rights memo template
- `docs/PROMPTS/artifacts/G1.14/` — the artifact directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/harness/recipes/` — owned by `G1.12`
- `docs/PROMPTS/harness/reference_machines.json` — owned by `G1.02`
- **Any external surface whatsoever** — website, deck, README, post, pilot document, investor
  material. `00_MASTER_PROTOCOL.md` §6.2 makes every external publication a human decision, and §4.1
  R2 makes a published comparison specifically so.
- **Any file that would constitute a side-by-side comparison naming a third-party product.** `02`
  §7.3: no agent may draft, generate, or stage one.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Substituting a prior Eustress build for the external reference, marking a provenance field
  optional, softening `verify`'s rejection to a warning, or declaring the item passed on the strength
  of the two documents alone are all measurement changes.
- **Do not select the product.** The brief states the *requirements* an acceptable reference must
  meet — scene characteristics, capture conditions, resolution, frame count, licence questions to
  answer — and leaves the selection to the human. This is the front-matter escalation.
- **Do not set `publication_rights`.** It defaults to `internal_calibration_only`. Only the human may
  change it, and only after the memo is complete.
- **Do not draft a comparison.** Not a caption, not a table, not a "for internal use" mock-up. §7.3
  is unconditional.
- The rights memo template must ask questions whose answers are checkable, not invite an opinion. Not
  "is publication acceptable?" but: the exact licence or EULA clause governing benchmarking, quoted
  with its section number; the exact clause governing publication of captured output, likewise;
  whether the artifact's own scene assets carry a separate licence and what it permits; who
  determined this and on what date; and whether counsel reviewed it.
- The brief must state R1 and R2 in full at the top, so a human reading only the brief still knows
  that internal calibration is unrestricted and publication is not.

## 5. Exit criterion

### Criterion
`eustress-capture verify` exits **0** on a bundle whose `sides.control.kind` is
`external_reference` and whose control block populates **all eight** required provenance fields plus
a complete five-key `hardware` block, with `publication_rights` carrying a value and a
`rights_memo_path` pointing to a completed memo; and `verify` exits **non-zero** on the same bundle
with any one of those fields blanked.

### Measurement

Command, part one — the real bundle, produced by the human's capture:

    cd eustress && cargo run --release --bin eustress-capture -- verify \
        --bundle ../docs/PROMPTS/artifacts/bundles/<external-ref-bundle-hash>/ \
        --require-external-provenance \
        --out ../docs/PROMPTS/artifacts/G1.14/reference_control_manifest.json ; echo "EXT_EXIT=$?"

Command, part two — the negative control, which proves the check is real:

    python -c "
    import json, shutil, os
    src='docs/PROMPTS/artifacts/bundles/<external-ref-bundle-hash>/manifest.json'
    dst='docs/PROMPTS/artifacts/G1.14/tampered_manifest.json'
    m=json.load(open(src)); m['sides']['control']['scene_license']=''
    os.makedirs(os.path.dirname(dst), exist_ok=True); json.dump(m, open(dst,'w'))
    print('blanked=scene_license')
    "

    cd eustress && cargo run --release --bin eustress-capture -- verify \
        --manifest ../docs/PROMPTS/artifacts/G1.14/tampered_manifest.json \
        --require-external-provenance ; echo "TAMPER_EXIT=$?"

Command, part three — count the populated fields directly, independently of the verifier:

    python -c "
    import json, sys
    m = json.load(open('docs/PROMPTS/artifacts/G1.14/reference_control_manifest.json'))
    c = m['sides']['control']; h = m['hardware']
    req = ['product','product_version','scene_source','scene_license','publication_rights',
           'capture_operator','capture_date_utc','rights_memo_path']
    hw  = ['cpu','gpu','gpu_driver','ram_gb','os']
    populated = [k for k in req if c.get(k)]
    hw_pop    = [k for k in hw  if h.get(k)]
    print('kind=%s provenance_populated=%d/8 hardware_populated=%d/5 publication_rights=%s'
          % (c.get('kind'), len(populated), len(hw_pop), c.get('publication_rights')))
    sys.exit(0 if c.get('kind')=='external_reference' and len(populated)==8 and len(hw_pop)==5 else 1)
    " ; echo "FIELD_EXIT=$?"

Expected output shape:

    EXT_EXIT=0
    blanked=scene_license
    external reference provenance incomplete: scene_license
    TAMPER_EXIT=1
    kind=external_reference provenance_populated=8/8 hardware_populated=5/5 publication_rights=internal_calibration_only
    FIELD_EXIT=0

Pass condition:

    EXT_EXIT == 0  AND  TAMPER_EXIT != 0  AND  FIELD_EXIT == 0

The countable external fact is `provenance_populated == 8/8` and `hardware_populated == 5/5` on a
control block of `kind: external_reference`. Neither the brief nor the memo existing is any part of
the pass condition — they are the agent's inputs to a human, and a human who never acts on them
leaves this item legitimately unfinished.

## 6. Critic gate

`critic_gate` is `[]`, which is worth stating carefully because this item is the one that makes a
Critic gate possible at all. The item does not produce something scoreable; it produces the *control
side* every future preference judgement is made against. It cannot be scored against itself. The
mechanical criterion in §5 replaces the gate and is correspondingly tight: an exact field count on a
real manifest, plus a demonstrated rejection when a single field is blanked.

## 7. Loop cadence and escalation

```
This item is HUMAN-EXECUTED. The agent half has one approach and one deliverable pair.

Iterations 1-3 : approach A — write EXTERNAL_REFERENCE_BRIEF.md and RIGHTS_MEMO_TEMPLATE.md, then
                 hand off and wait
   -> if the human returns the brief as unactionable, MANDATORY approach change. Rewording a
      section is NOT an approach change; restructuring the brief around a decision the human has
      already made, rather than around requirements they must still choose against, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still unactionable, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : the human has not supplied an artifact and the brief has been through 3 revisions
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: the brief cannot be written without the agent naming a product (see front matter)

WAITING ON THE HUMAN IS NOT AN ITERATION. The item sits at IN_PROGRESS while the human acts, and
that time is charged to neither the token nor the iteration budget.
```

The stall packet must fit one screen and request exactly one of: LOWER `00_MASTER_PROTOCOL.md` §1.1
condition D1 to a self-referential comparison (`eustress@<commit-a>` vs `eustress@<commit-b>`),
stating explicitly that the program's definition of done then contains no external bar; FUND a
specific approach D; DEFER behind a named blocking item; or KILL, stating that D1 remains unowned.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.14/reference_control_manifest.json`

A reader finds the verified control block: `kind`, all eight provenance fields with their values, the
complete five-key `hardware` block, the `publication_rights` value, and the path to the completed
rights memo — together with the verifier's exit code on the real bundle and on the deliberately
blanked one. This file is the W1 evidence for the item and is what a future preference-dimension
scorecard cites as the reason its control side is admissible.

The agent's two deliverables sit alongside it:

- `docs/PROMPTS/harness/EXTERNAL_REFERENCE_BRIEF.md` — states R1 and R2 in full at the top; the
  technical requirements an acceptable reference artifact must meet (static scene, enumerable pose,
  declared resolution matching the comparison recipe, declared colour pipeline, sufficient frames for
  the declared frame sets); the eight provenance fields that must be recorded at capture time and
  cannot be reconstructed afterwards; and the explicit statement that product selection is the
  human's, not the agent's.
- `docs/PROMPTS/harness/RIGHTS_MEMO_TEMPLATE.md` — the checkable questions listed in §4, each with
  space for the quoted clause and its section number, the determiner's name, the date, and whether
  counsel reviewed it. The template's default answer for `publication_rights` is
  `internal_calibration_only` and the template says in its own text that only the human may change it.

## 9. Definition of NOT done

- The brief and the memo template exist and no artifact was ever procured. The exit criterion is a
  populated control block, not a document that describes one.
- The agent selected the product. That is a licensing exposure decision reserved to the human.
- `publication_rights` is set to `publishable` by anyone other than the human, or with no completed
  memo behind it. That single field is the control standing between an internal calibration capture
  and a licence violation.
- Seven of eight provenance fields are populated and the eighth is "to be determined". `02` §7.3
  makes an incomplete block inadmissible; the Critic rejects rather than scores.
- The tamper leg was skipped, so the verifier has never been shown to reject anything. Rejection is
  the whole function.
- The reference is a marketing render, a video of unknown encoding, or a screenshot at an unknown
  resolution. None of those can be compared frame for frame against a harness bundle, and the brief
  exists to prevent exactly that procurement.
- An agent drafted a side-by-side, a caption, or a comparison table "for internal review". `02` §7.3
  is unconditional, and the fact that a comparison is internal today does not stop it being forwarded
  tomorrow.
- The item was marked passed because the human said the rights question was fine. The memo records
  the quoted clause and its section number, because a determination nobody can re-derive is not a
  determination.
````

---

## What this pack does not cover

Three things sit adjacent to G1 and are deliberately not authored here.

**`B10` — headless GPU tier (`--render gpu`).** `02_CAPTURE_HARNESS.md` §9 assigns it to G7, and
`docs/architecture/HEADLESS_RUNTIME.md:269` lists `P6` as unstarted. Until it lands, every capture
requires a desktop session, which means the founder remains the entire regression surface and each
verification cycle costs a serialized 10–15 minute build. `G1.09` is structured so that a headless
GPU tier later requires no change to the recipe format.

**`B11` — CI capture job.** Also G7's, per `02` §9. It depends on `B10` and on a GPU runner.
`00_MASTER_PROTOCOL.md` §1.1 condition D3 is where it is finally required.

**`B12` and `B13` — perceptual diff tool and bundle browser.** `02` §9 marks both non-blocking. They
would let an L1 pre-screen a bundle and detect a null result before spending Critic budget, and would
let a human watch a frame set at rate. If L0 wants them, they belong at `G1.15` and `G1.16`.
