# 02 — THE QUEUE

**Status:** Normative for sequencing. Binding on L0 when dispatching work.
**Companions:** `00_MASTER_PROTOCOL.md` (authority, budget tiers, gates), `03_PROMPT_SCHEMA.md`
(item format), `04_FILE_OWNERSHIP.md` (who may edit a contested path), `01_CRITIC_RUBRIC.md`
(the D-dimensions named in the Critic-gate column), `02_CAPTURE_HARNESS.md` (bundle format).
**Scope of authority:** This file decides what runs next and what waits. It does not change any
item's exit criterion, floor, scope list, or envelope. Where an item's own front matter conflicts
with a wave assignment here, the front matter wins and this file is regenerated.

**Library size:** nine packs, **159 items**, in `docs/PROMPTS/packs/`.

| Pack | File | Items | Builds | Envelope (working days) | Tokens |
|:--|:--|--:|--:|--:|--:|
| G0 | `G0_security_and_external_reality.md` | 12 | 86 | 13.9 | 3.88M |
| G1 | `G1_capture_harness.md` | 14 | 116 | 18.3 | 5.02M |
| T1 | `T1_rendering_and_content.md` | 19 | 218 | 36.5 | 9.30M |
| T2 | `T2_physics_and_simulation.md` | 22 | 212 | 32.0 | 8.70M |
| T3 | `T3_studio_ux.md` | 15 | 140 | 22.1 | 5.96M |
| T4 | `T4_agent_loop_and_training_substrate.md` | 24 | 264 | 46.1 | 11.86M |
| T5 | `T5_robustness_and_cohesion.md` | 16 | 122 | 16.6 | 4.96M |
| B1 | `B1_proof_and_design_partners.md` | 18 | 78 | 10.5 | 3.38M |
| B2 | `B2_revenue_ecosystem_org.md` | 19 | 42 | 5.0 | 2.12M |
| **Total** | | **159** | **1,278** | **201.0** | **55.18M** |

---

## 1. Global item zero — `G1.01`, `PASSED`

**`G1.01` — `cargo build --workspace` exits 0.** Pack G1, tier M, 6 builds, 4 h envelope,
artifact `docs/PROMPTS/artifacts/G1.01/workspace_build.json`.

**`PASSED` 2026-08-08** — approach D, 3 iterations, **6 of 6 builds consumed**. `BUILD_EXIT=0`,
and a second incremental run against the same `target/` with no `cargo clean` between them gave
`BUILD_EXIT_RERUN=0`. 40 workspace members, `rustc 1.95.0`, commit `71ccf6fe`. Nothing else in this
library started before it, and nothing needs to wait on it again.

**Why it preceded even the harness.** The harness is the instrument the rest of the program measures
with — and an instrument is built by compiling it. **All 1,278 of this library's build slots were
budgeted against a workspace nobody had ever compiled.** Every number produced before `G1.01` passed
would have been produced by a build that might not be the build the program was describing, and
every such number would have had to be thrown away and re-measured. The cost of getting this wrong
was never 6 builds; it was the fraction of 1,278 already spent when someone first ran
`cargo build --workspace` and it failed.

### 1.1 What `G1.01` found

**Three members were broken, from three unrelated causes.**

| # | Member | Cause | Remedy |
|--:|:--|:--|:--|
| 1 | `eustress-backend` | `crates/backend/src/marketplace.rs` called four `Database` methods that `crates/backend/src/db.rs` never defined — `find_marketplace_item_by_id`, `has_purchased`, `get_user_balance`, `purchase_item` | Removed from `[workspace] members`; 41 → 40 |
| 2 | `benches/instance-capacity` | Eight wgpu-29 API drifts the Bevy 0.19 migration missed | Fixed in place |
| 3 | `eustress-server` | Used `serde_json::Value` without declaring the dependency | Fixed in place |

Two details are worth carrying forward.

`get_user_balance` was the one method of the four that could not honestly be written: there is no
balance table, column, or method anywhere in `db.rs`, and `db.rs:5` records that user identity is
managed by Cloudflare KV rather than by this database. Where a Bliss balance lives is a design
decision, not a compile fix. The member is unmodified on disk and simply no longer built, so
re-listing it is a one-line revert the moment a human makes that decision.

The `instance-capacity` failure carries a trap worth naming: `RenderPipelineDescriptor.multiview`
was **renamed** to `multiview_mask`, but rustc reports it as `E0560: no field named multiview`,
which reads as removed. A field that reports as absent may have moved.

### 1.2 The shared root

**No CI job compiles more than one package, and no CI job runs `cargo test` at all.**

| Workflow | What it actually runs |
|:--|:--|
| `.github/workflows/ci.yml` | `cargo deny` (advisories/bans/sources), a naga shader pass, a `cargo tree` grep |
| `.github/workflows/linux-engine.yml:48` | `cargo check --package eustress-engine` |
| `.github/workflows/release.yml` | the same lone package |

That is the whole of it. Three of forty members could rot unnoticed because nothing ever built them,
and the workspace's entire test suite — **2,070 `#[test]` and `#[tokio::test]` functions across
`eustress/crates` and `eustress/benches`** — has never executed in CI. `G7.33` inventories the gap
and `G7.34` closes the test half of it; the workspace-build half is closed by `G1.01`'s own
measurement being reproducible on demand, not yet by a CI job.

**The reasoning that put this first was vindicated, not invalidated.** Three broken members from
three unrelated causes is precisely what "a workspace nobody has compiled" buys, and it cost 6
builds to find rather than an unbounded fraction of 1,278 to discover mid-programme.

### 1.3 What the gate becomes

`G1.01`'s escalation clause STALLs if a second broken member appears. It fired — see the stall
register in §7 — and the human answered FUND, which is the clause working as designed.

The dependency graph carries the sequencing rather than this section decreeing it. Every
build-consuming item in the library reaches `G1.01` transitively through a declared edge, so
WORKSPACE-BUILD-GATE (§8.4) is clear program-wide and stays clear as items are added. Four items are
dependency-free — `G1.01`, `G1.02`, `G1.30`, `G6.01` — and three of those four are tier-S censuses
that consume no build at all.

---

## 2. Program cost ledger

Stated up front because nothing else in the library states it.

### 2.1 The totals

| Quantity | Total | Basis |
|:--|--:|:--|
| Items | 159 | count of front-matter blocks across the nine packs |
| Build slots (`max_builds`) | **1,278** | sum of declared `max_builds` |
| Token envelope | **55.18M** | sum of declared `token_envelope` |
| Wall-clock envelope | **201.0 working days** | sum of declared `wallclock_envelope`, S=1 h, M=4 h, L=2 d, XL=5 d, 8 h/day |
| Wall-clock envelope, hours | **4,280 h** | the same sum with a calendar day of 24 h |

Tier distribution: **28 S** (0 builds each), **61 M** (6), **61 L** (12), **9 XL** (20).

### 2.2 Spent to date

Five items are retired: all four of W0, plus `G0.06` from W1 (§4.3).

| Item | Tier | Builds budgeted | Builds consumed | Outcome |
|:--|:--|--:|--:|:--|
| `G1.01` | M | 6 | **6** | `PASSED` — three broken members found and remedied (§1.1) |
| `G1.02` | S | 0 | 0 | `PASSED` — RM-1 pinned, all seven T1 frame-cost triggers resolve to it |
| `G6.01` | S | 0 | 0 | `PASSED` — UX baseline census, 60 panels / 1,999 ribbon tools / 20 click-depths |
| `G1.30` | S | 0 | 0 | `PASSED` — 104-claim ledger; the remediation debt is deferred, not written off (§7) |
| `G0.06` | M | 6 | **2** | `PASSED` — 6,315 files scanned, 27 unallowlisted hits → 0, verifier authored |
| **Retired** | | **12** | **8** | **five items** |

| Quantity | Budgeted | Consumed | Remaining |
|:--|--:|--:|--:|
| Items | 159 | 5 | **154** |
| Build slots | 1,278 | 8 | **1,270** |
| Token envelope | 55.18M | 0.58M | **54.60M** |
| Wall-clock envelope | 201.0 d | 1.4 d | **199.6 d** |

Two of the five consumed a build slot; the other three are tier S and consumed none. **W0 is now
closed.** Its four items are the four that declare no dependency at all — `G1.01`, `G1.02`, `G1.30`,
`G6.01` — and all four have passed, spending 6 of the wave's 6 budgeted slots. `G0.06` accounts for
the other 2 slots spent and sits in W1, where its `G1.01` edge puts it; it came in 4 builds under its
envelope.

### 2.3 The binding constraint

`00_MASTER_PROTOCOL.md` §7: a full engine build is 10–15 minutes, and **only one build may run at a
time** against the shared `target/` directory.

| | Low (10 min/build) | High (15 min/build) |
|:--|--:|--:|
| 1,278 builds, pure serial compile | **213 h** | **320 h** |
| 1,270 builds still to spend | **212 h** | **318 h** |
| as 24 h calendar days of nothing but compiling | 8.8 | 13.2 |
| as 8 h working days of nothing but compiling | 26.5 | 39.7 |

`G1.01` is the one line of this table that is now a measurement rather than a projection: 6 builds,
`cargo build --workspace` twice, `1m 43s` on the second incremental pass. A full cold engine build
remains the 10–15 minute figure `00_MASTER_PROTOCOL.md` §7 records.

Compile time is not the ceiling. The unspent envelope is **199.6 working days**.
`00_MASTER_PROTOCOL.md` §7 makes 150% of envelope a stall trigger rather than a quiet extension, so
the realistic ceiling if items generally run long is **299 working days** — from 2026-08-10 that is
**2027-05-17** at envelope and **2027-10-04** at the stall ceiling, before a single stall packet is
answered by a human.

### 2.4 The consequence, in one sentence

**Executing this library end to end is a ten-to-fourteen-month single-threaded program, so the only
question that matters is which thirteen of the 154 remaining items produce the first artifact a
stranger can check — and the answer is §5, not §3.**

---

## 3. The full ordered table

All 159 items, ordered by **effective wave** (§4), then by phase, then by item number. This is the
reference index; every item in every pack appears exactly once.

**Column notes.**
- **Wave** — the earliest wave the item can start, computed from `depends_on`. It is not a schedule;
  it is a floor, and it is exactly the longest dependency chain from the item back to an item that
  declares no dependency. W0 is therefore the four dependency-free items and nothing else (§4.3);
  `G1.01` ran first and alone within it.
- **Workload** — primary tag, then secondary tags after `+`, exactly as declared.
- **depends_on** — declared edges, verbatim from the front matter, in front-matter order. The two
  program gates that used to live outside the graph are now inside it: a build-consuming item carries
  an edge that reaches `G1.01`, and a recipe-citing item carries a `G1.12` edge (§8.2, §8.4). Every
  ordering constraint the library asserts is in this column — a `blocks` entry no longer carries one
  on its own (§8.7). §9 verifies the agreement mechanically, row by row.
- **Builds** — declared `max_builds`. Tier S is always 0.
- **Critic gate** — declared `critic_gate` dimensions from `01_CRITIC_RUBRIC.md`.

**Markers.** ★ on the §5 critical path. § blocked by PROGRAM-LICENCE-GATE (§8.1). ✔ `PASSED`.
◐ `PARTIAL` — work begun and not finished (§7).

| Wave | ID | Title | Pack | Workload | Phase | depends_on | Tier | Builds | Critic gate |
|:--|:--|:--|:--|:--|:--|:--|:--|--:|:--|
| W0 | `G1.01` ★ ✔ | cargo build --workspace exits 0 | G1 | W3 +W6 | G1 | — | M | 6 | — |
| W0 | `G1.02` ★ ✔ | Pinned reference machine RM-1 and the manifest hardware block | G1 | W3 | G1 | — | S | 0 | — |
| W0 | `G1.30` ✔ | Every customer-facing quantitative claim classified and gated | B1 | W3 +W1 | G1 | — | S | 0 | — |
| W0 | `G6.01` ✔ | Studio UX baseline census — the measured before-state of the editor surface | T3 | W6 +W1/W3 | G6 | — | S | 0 | — |
| W1 | `G0.06` ✔ | Every licensing misstatement found and fixed, enforced by a grep verifier | G0 | W3 +W2 | G0 | `G1.01` | M | 6 | — |
| W1 | `G1.03` ★ | Harness scene set S1-S6 emitted by a seeded generator | G1 | W3 +W1 | G1 | `G1.01` | L | 12 | — |
| W1 | `G1.05` ★ | AI camera capture accepts width, height, and output path | G1 | W3 +W1/W6 | G1 | `G1.01` | M | 6 | — |
| W1 | `G1.07` ★ | Independent physics-step counter exported into the recording and the manifest | G1 | W3 +W1 | G1 | `G1.01` | M | 6 | — |
| W1 | `G1.08` ★ | Per-frame tick-indexed frame-time CSV with p50/p99/max/cv | G1 | W3 +W1 | G1 | `G1.01` | M | 6 | — |
| W1 | `G1.31` | A proof standard an outside engineer can execute without contacting anyone | B1 | W3 +W1 | G1 | `G1.30` | S | 0 | — |
| W1 | `G1.40` | Revenue-rail truth ledger with per-capability build evidence | B2 | W2 +W3 | G1 | `G1.01` | M | 6 | — |
| W1 | `G6.03` | Drain-contract regression test and removal of the dead parallel drain | T3 | W3 +W6 | G6 | `G6.01` `G1.01` | M | 6 | — |
| W1 | `G6.30` | Every declared tool id classified shipped or declared, with a gate that blocks a demo script | B1 | W4 +W3/W2 | G6 | `G1.30` `G1.01` | M | 6 | — |
| W1 | `G7.01` | MCP tool-surface conformance census and reliability baseline | T4 | W3 +W6 | G7 | `G1.01` | L | 12 | — |
| W1 | `G7.30` ◐ | Session lifecycle beacon and measured crash-free session baseline | T5 | W3 +W6 | G7 | `G1.01` | M | 6 | — |
| W2 | `G1.04` ★ | Camera path files CP-A/B/S/U and the tick-driven path player | G1 | W3 +W1 | G1 | `G1.03` | M | 6 | — |
| W2 | `G1.06` ★ | Captures fire at declared simulation tick indices | G1 | W3 | G1 | `G1.05` | M | 6 | — |
| W2 | `G1.41` | Vertical selection with an evidence-scored kill list | B2 | W4 +W2 | G1 | `G1.40` | S | 0 | — |
| W2 | `G1.43` | Unit economics of simulation compute, from a measured throughput baseline | B2 | W2 +W3 | G1 | `G1.40` | M | 6 | — |
| W2 | `G1.48` § | Licence decision-forcing memo with four costed options | B2 | W5 +W2 | G1 | `G1.40` | S | 0 | — |
| W2 | `G1.56` | Operator automation baseline with a measured wall-clock reduction | B2 | W6 +W3 | G1 | `G1.40` | M | 6 | — |
| W2 | `G2.01` ★ | Simulation evidence harness and physics baseline ledger | T2 | W3 +W1 | G2 | `G1.03` | L | 12 | — |
| W2 | `G2.34` | STEP import produces a solid whose measured geometry matches the source within stated floors | B1 | W1 +W3/W4 | G2 | `G1.31` `G1.01` | L | 12 | — |
| W2 | `G6.32` | A qualification scorecard that disqualifies a prospect on capability we do not have | B1 | W6 +W2/W4 | G6 | `G6.30` | S | 0 | — |
| W2 | `G7.02` | Zero silent no-ops on the agent tool surface | T4 | W3 +W6 | G7 | `G7.01` `G1.05` | L | 12 | — |
| W2 | `G7.03` | Headless render tier — the agent sees without a desktop session | T4 | W3 +W1/W6 | G7 | `G7.01` `G1.03` `G1.05` `G7.30` | XL | 20 | — |
| W2 | `G7.11` | Retire the FoundationModelDispatcher claim and state the real model boundary | T4 | W3 +W5 | G7 | `G7.01` | S | 0 | — |
| W2 | `G7.33` | CI truth ledger — what ci.yml gates today versus what it must gate | T5 | W3 +W6 | G7 | `G7.30` | S | 0 | — |
| W3 | `G1.09` ★ | eustress-capture binary producing a content-addressed bundle with a valid manifest | G1 | W3 +W6/W1 | G1 | `G1.02` `G1.04` `G1.06` `G1.07` `G1.08` | XL | 20 | — |
| W3 | `G1.42` | Chargeable-today SKU sheet with per-SKU capability backing | B2 | W2 +W4 | G1 | `G1.40` `G1.41` | S | 0 | — |
| W3 | `G1.49` § | CONTRIBUTING.md with a verbatim DCO and defined inbound IP | B2 | W5 +W3 | G1 | `G1.48` | S | 0 | — |
| W3 | `G1.52` § | Governance non-negotiables, each with a runnable violation detector | B2 | W5 +W3 | G1 | `G1.48` | S | 0 | — |
| W3 | `G1.53` | Four first-hire evaluation tasks drawn from open defects in this repository | B2 | W6 +W3 | G1 | `G1.41` | S | 0 | — |
| W3 | `G2.02` ★ | Determinism gate — byte-identical Avian reruns, runnable by command | T2 | W3 | G2 | `G2.01` | M | 6 | — |
| W3 | `G2.17` | Transient conduction validated against the analytic semi-infinite slab | T2 | W3 +W1 | G2 | `G2.01` | M | 6 | — |
| W3 | `G2.18` | Kernel-law declaration surface — a Rune-declared law equals the native kernel | T2 | W5 +W1 | G2 | `G2.01` | M | 6 | — |
| W3 | `G2.35` | Eustress to STEP to Eustress round-trip holds geometry within measured floors | B1 | W1 +W3/W4 | G2 | `G2.34` | L | 12 | — |
| W3 | `G2.36` | A minimal USDA scene round-trips with preserved units, up-axis, and hierarchy | B1 | W3 +W1/W4 | G2 | `G2.34` | L | 12 | — |
| W3 | `G5.20` | Avian throughput curve — measured ms/step versus body count | T2 | W1 +W3 | G5 | `G2.01` | M | 6 | — |
| W3 | `G7.04` | One-iteration agent-loop latency baseline | T4 | W6 +W3 | G7 | `G7.02` | M | 6 | — |
| W3 | `G7.05` | Every agent mutation is transactional and undoable | T4 | W3 +W6 | G7 | `G7.02` `G1.05` | L | 12 | — |
| W3 | `G7.07` | The human gate is enforced, not hinted | T4 | W3 +W6 | G7 | `G7.02` `G1.05` | M | 6 | — |
| W3 | `G7.31` | Structured JSON log stream and one-command `eustress diag` support bundle | T5 | W6 +W3 | G7 | `G7.30` `G7.03` | M | 6 | — |
| W3 | `G7.34` | cargo test runs in CI on the default branch and is green | T5 | W3 +W6 | G7 | `G7.33` | M | 6 | — |
| W3 | `G7.35` | The client binary is built and startup-smoke-tested in CI on all three platforms | T5 | W3 +W4 | G7 | `G7.33` | M | 6 | — |
| W4 | `G1.10` ★ | Blinding subcommand with metadata scrub, leak checklist, and separated key | G1 | W3 +W1 | G1 | `G1.09` | L | 12 | — |
| W4 | `G1.11` ★ | Determinism verification mode and the two-capture byte-identity selftest | G1 | W3 +W1 | G1 | `G1.09` `G2.02` | L | 12 | — |
| W4 | `G1.12` ★ | All 33 cited capture recipes authored once in a single library-wide format | G1 | W3 +W1/W6 | G1 | `G1.02` `G1.09` | L | 12 | — |
| W4 | `G1.44` | Three-part pricing architecture with a COGS-anchored usage floor | B2 | W2 +W5 | G1 | `G1.42` `G1.43` | S | 0 | — |
| W4 | `G1.47` | Customer-success health score computable from the live telemetry payload | B2 | W2 +W6 | G1 | `G1.41` `G1.42` | M | 6 | — |
| W4 | `G1.51` § | Merit ladder with rungs computable from repository history alone | B2 | W5 +W6 | G1 | `G1.49` | M | 6 | — |
| W4 | `G1.55` | Decision-rights charter enforced by a CODEOWNERS file that matches real paths | B2 | W6 +W5 | G1 | `G1.53` `G1.52` | S | 0 | — |
| W4 | `G2.03` | Determinism through the full headless engine stack | T2 | W3 +W1 | G2 | `G2.02` `G1.07` `G7.03` | L | 12 | — |
| W4 | `G2.30` | Kernel laws validated against closed-form analytic solutions with declared tolerances | B1 | W1 +W3 | G2 | `G1.31` `G2.02` | M | 6 | — |
| W4 | `G2.37` | Every import and export path agrees on meters and on handedness, measured | B1 | W3 +W1 | G2 | `G2.34` `G2.36` `G2.35` | M | 6 | — |
| W4 | `G5.21` | Frame-time distribution under physics load | T2 | W1 +W3 | G5 | `G5.20` `G1.08` | L | 12 | — |
| W4 | `G6.02` | UI interaction-latency instrument with per-frame input-to-present trace | T3 | W3 +W1/W6 | G6 | `G6.01` `G1.04` `G7.31` | M | 6 | — |
| W4 | `G7.06` | Op-log replay reconstructs the world exactly | T4 | W3 +W6 | G7 | `G7.05` | L | 12 | — |
| W4 | `G7.32` | Typed panic classifier and durable crash record | T5 | W3 +W6 | G7 | `G7.30` `G7.03` `G7.31` | M | 6 | — |
| W4 | `G7.36` | Clippy and rustfmt gate at deny-warnings on a named crate set with a written expansion ladder | T5 | W3 +W6 | G7 | `G7.34` | M | 6 | — |
| W4 | `G7.39` | Machine-readable stuck-phase record, reported safely with no desktop session | T5 | W3 +W6 | G7 | `G7.31` `G7.03` | M | 6 | — |
| W4 | `G7.40` | Machine-readable microprofiler output and a perf-assert gate that can fail | T5 | W3 +W6 | G7 | `G7.31` `G1.08` | M | 6 | — |
| W5 | `G1.13` ★ | One command produces a valid bundle from a clean checkout with no operator-local state | G1 | W3 +W6 | G1 | `G1.10` `G1.11` `G1.12` | L | 12 | — |
| W5 | `G1.14` | HUMAN-EXECUTED external reference control artifact and publication-rights determination | G1 | W1 +W3 | G1 | `G1.02` `G1.12` | S | 0 | — |
| W5 | `G1.45` | ARR milestone ladder with per-step capability and organisational unlocks | B2 | W2 +W6 | G1 | `G1.42` `G1.43` `G1.44` | S | 0 | — |
| W5 | `G2.04` | Time-compression fidelity instrumentation | T2 | W3 +W1 | G2 | `G2.01` `G1.07` `G1.12` | M | 6 | D5 |
| W5 | `G2.07` | Conservation invariants as a runnable suite | T2 | W3 +W1 | G2 | `G2.01` `G1.12` | M | 6 | D5 |
| W5 | `G2.08` | Chemistry-agnostic 0-D cell model validated against a public dataset | T2 | W1 +W3 | G2 | `G2.01` `G1.12` | L | 12 | D5 |
| W5 | `G2.15` | Beam FEA validated against the closed-form Euler-Bernoulli solution | T2 | W1 +W3 | G2 | `G2.01` `G1.12` | L | 12 | D5 |
| W5 | `G2.16` | Reactor control loop — measured step response with overshoot and settling bounds | T2 | W1 +W3 | G2 | `G2.03` `G1.12` | M | 6 | D5 |
| W5 | `G2.31` | Kernel laws checked against published reference values with DOI-level provenance | B1 | W1 +W3 | G2 | `G2.30` | M | 6 | — |
| W5 | `G3.01` | Deterministic, parameterised reference capture path | T1 | W3 +W1/W6 | G3 | `G1.03` `G1.05` `G1.11` | M | 6 | — |
| W5 | `G6.04` | The studio UI never blocks on the engine — p99 drain cost under 2 ms | T3 | W1 +W3/W6 | G6 | `G6.02` `G6.03` `G1.12` | L | 12 | D4 |
| W5 | `G6.06` | Pre-click honesty for the 1,969 unwired ribbon tools | T3 | W4 +W1 | G6 | `G6.01` `G1.12` | M | 6 | D4 D6 |
| W5 | `G6.09` | One selection identity across both palettes and the 3D viewport | T3 | W1 +W3 | G6 | `G6.01` `G1.12` | M | 6 | D2 D6 |
| W5 | `G6.40` | Unwired-button sweep with a fully wired demo path for the sold vertical | B2 | W4 +W1 | G6 | `G1.41` `G1.47` | M | 6 | — |
| W5 | `G7.08` | Fork, rehearse, commit — with zero residue | T4 | W6 +W3 | G7 | `G7.06` | L | 12 | — |
| W5 | `G7.09` | Independent agent camera with deterministic captures | T4 | W3 +W1 | G7 | `G7.03` `G1.05` `G1.12` | L | 12 | D6 |
| W5 | `G7.12` § | .etask — the environment and task specification format | T4 | W5 +W3/W4 | G7 | `G7.01` `G1.12` `G1.48` | L | 12 | D6 |
| W5 | `G7.15` | Dropped-tick accounting invalidates dishonest episodes | T4 | W3 +W5 | G7 | `G7.01` `G1.07` `G1.12` | M | 6 | D5 |
| W5 | `G7.41` | Startup and shutdown correctness contract with measured cold-start and clean-exit proof | T5 | W3 +W6 | G7 | `G7.32` `G7.03` | M | 6 | — |
| W6 | `G1.54` | Capital strategy matched to a deep-tech clock, with three costed paths | B2 | W2 +W6 | G1 | `G1.43` `G1.45` | S | 0 | — |
| W6 | `G2.05` | Bounded-error integration under time compression | T2 | W1 +W3 | G2 | `G2.04` `G2.03` `G1.07` `G1.12` | L | 12 | D5 |
| W6 | `G2.06` | Sim-time alerting for Watchman and breakpoints | T2 | W1 +W3 | G2 | `G2.04` `G1.12` | M | 6 | D5 |
| W6 | `G2.09` | Single-particle model with solid-phase diffusion and diffusion overpotential | T2 | W1 +W3 | G2 | `G2.08` `G1.12` | L | 12 | D5 |
| W6 | `G2.14` | Fracture to Avian — a crack criterion drives a real rigid-body split | T2 | W1 +W3 | G2 | `G2.07` `G1.07` `G1.12` | L | 12 | D5 D6 |
| W6 | `G2.32` | Declared validity envelope for the lumped V-Cell model, enforced at run time | B1 | W3 +W1/W4 | G2 | `G2.30` `G2.08` | M | 6 | — |
| W6 | `G3.02` | render-probe image-analysis binary and PBR reference baseline | T1 | W1 +W3 | G3 | `G3.01` `G1.01` | L | 12 | — |
| W6 | `G4.01` | Frame-time distribution instrumentation with p50/p95/p99/max | T1 | W1 +W3/W6 | G4 | `G3.01` `G1.05` `G1.08` | M | 6 | — |
| W6 | `G5.22` | Multi-variant experiment throughput with a noise floor that makes ranking meaningful | T2 | W6 +W1 | G5 | `G2.03` `G2.08` `G7.08` `G1.12` | L | 12 | D5 |
| W6 | `G6.05` | The Properties panel persists every edit it visually accepts | T3 | W1 +W4/W3 | G6 | `G6.01` `G6.03` `G6.04` `G1.12` | L | 12 | D4 |
| W6 | `G6.07` | Command palette — every top-20 command reachable in at most two clicks | T3 | W6 +W1 | G6 | `G6.01` `G6.02` `G6.04` `G1.12` | L | 12 | D4 |
| W6 | `G6.10` | Designed empty states and error states in the eight primary panels | T3 | W1 +W4 | G6 | `G6.01` `G6.04` `G6.09` `G1.12` | M | 6 | D4 D6 |
| W6 | `G6.11` | Accessibility annotations and deterministic focus order across ten panels | T3 | W1 +W5 | G6 | `G6.01` `G6.04` `G6.09` `G1.12` | L | 12 | D4 |
| W6 | `G6.13` | Split slint_ui.rs — no studio UI file over 3,000 lines, with behaviour held constant | T3 | W6 +W3 | G6 | `G6.03` `G6.04` | XL | 20 | — |
| W6 | `G6.31` | Zero dead controls on the one path a design partner will be shown | B1 | W4 +W1/W6 | G6 | `G6.30` `G6.04` `G1.12` | M | 6 | D4 D6 |
| W6 | `G7.10` | Detect and propose — machine-readable failure proposals | T4 | W6 +W3/W4 | G7 | `G7.04` `G7.06` `G2.04` `G1.12` | L | 12 | D5 |
| W6 | `G7.13` | env.reset and env.step over the Engine Bridge | T4 | W5 +W3/W6 | G7 | `G7.02` `G7.12` `G1.05` | L | 12 | — |
| W6 | `G7.37` | Property-based Space save/load round-trip with a measured fidelity floor | T5 | W3 +W1 | G7 | `G7.34` `G1.03` `G7.08` | L | 12 | — |
| W7 | `G0.09` | Channel-attributed arrival, measured over a declared window | G0 | W2 +W6 | G0 | `G6.30` `G6.31` `G0.06` | M | 6 | — |
| W7 | `G2.10` | P2D electrolyte transport (Doyle-Fuller-Newman) | T2 | W1 +W3 | G2 | `G2.09` `G1.12` | XL | 20 | D5 D6 |
| W7 | `G2.33` | A public validation report a stranger can regenerate end to end with one command | B1 | W3 +W1 | G2 | `G2.30` `G2.31` `G2.32` `G1.31` `G1.12` | M | 6 | D5 D6 |
| W7 | `G3.03` | PBR energy conservation and specular response on the reference grid | T1 | W1 | G3 | `G3.02` `G1.12` | L | 12 | D2 |
| W7 | `G3.05` | Shadow cascade quality and contact grounding | T1 | W1 | G3 | `G3.02` `G1.12` | L | 12 | D2 |
| W7 | `G6.08` | Keyboard-first coverage of the top 20 commands with visible conflict resolution | T3 | W1 +W6 | G6 | `G6.04` `G6.07` `G1.12` | M | 6 | D4 |
| W7 | `G6.12` | Information density and typographic hierarchy in Explorer, Properties, and Output | T3 | W1 | G6 | `G6.09` `G6.10` `G1.12` | L | 12 | D4 D6 |
| W7 | `G6.33` | A discovery instrument whose output is a quantified problem with a stated counterfactual | B1 | W6 +W2/W4 | G6 | `G6.32` `G6.31` | S | 0 | — |
| W7 | `G7.14` | Episode determinism under seed and tick-rate variation | T4 | W3 +W5 | G7 | `G7.13` `G7.15` `G2.02` | L | 12 | — |
| W7 | `G7.16` | Observation and action spaces generated from code, with a drift check | T4 | W5 +W3 | G7 | `G7.13` `G1.05` | M | 6 | — |
| W7 | `G7.18` | Physically verified outcome scoring | T4 | W4 +W1/W3 | G7 | `G7.12` `G7.13` `G2.35` `G1.12` | XL | 20 | D5 D6 |
| W7 | `G7.20` | Containerised, GUI-free lab integration | T4 | W5 +W3/W6 | G7 | `G7.03` `G7.13` | L | 12 | — |
| W7 | `G7.38` | World-container integrity verifier and the truth about a Space in git | T5 | W3 +W4 | G7 | `G7.37` `G7.08` | M | 6 | — |
| W7 | `G7.42` | Fault injection and typed, actionable recovery for every user-facing failure | T5 | W6 +W3 | G7 | `G7.32` `G7.39` `G6.13` `G1.12` | L | 12 | D4 D6 |
| W7 | `G7.43` | Always-on regression fleet that re-runs every earlier phase gate on a schedule | T5 | W3 +W6 | G7 | `G7.34` `G7.35` `G7.40` `G7.36` `G7.37` `G7.39` `G7.41` | L | 12 | — |
| W8 | `G0.08` | Five qualified strangers from named channels complete the discovery instrument | G0 | W2 +W4 | G0 | `G6.33` `G0.09` | S | 0 | — |
| W8 | `G2.11` | Thermal coupling with temperature-dependent transport, validated on a temperature sweep | T2 | W1 +W3 | G2 | `G2.10` `G1.12` | L | 12 | D5 |
| W8 | `G2.12` | P2D numerical convergence and charge conservation | T2 | W3 +W1 | G2 | `G2.10` | M | 6 | — |
| W8 | `G3.04` | Exposure and tone response that survives the post-stack | T1 | W1 +W6 | G3 | `G3.02` `G3.03` `G1.05` `G1.12` | L | 12 | D2 D1 |
| W8 | `G3.09` | Material service authoring round-trip from file to pixels and back | T1 | W6 +W1 | G3 | `G3.03` `G1.12` | L | 12 | D2 D4 |
| W8 | `G6.14` | Time-to-first-successful-task for a first-time professional user | T3 | W1 +W4/W6 | G6 | `G6.05` `G6.06` `G6.07` `G6.08` `G6.10` `G1.12` | L | 12 | D4 D6 |
| W8 | `G6.34` | A pilot whose success metric is agreed and hashed before any pilot data exists | B1 | W6 +W2/W3/W4 | G6 | `G6.33` `G2.33` | S | 0 | — |
| W8 | `G7.17` | Episode recording and byte-exact replay | T4 | W3 +W5/W6 | G7 | `G7.14` `G7.06` `G1.05` | L | 12 | — |
| W8 | `G7.19` | Anti-gaming — the adversarial exploit suite | T4 | W3 +W1/W4 | G7 | `G7.18` `G1.12` | L | 12 | D5 |
| W8 | `G7.21` | Throughput — episodes per CPU-hour, measured | T4 | W6 +W3 | G7 | `G7.13` `G7.20` | M | 6 | — |
| W8 | `G7.44` | On-prem and air-gapped reliability profile measured against the cloud-connected profile | T5 | W3 +W4 | G7 | `G7.31` `G7.38` | M | 6 | — |
| W9 | `G0.01` | Agent-surface and plugin-loader threat model with two exploits refused at dispatch | G0 | W3 +W5 | G0 | `G7.01` `G7.19` | L | 12 | — |
| W9 | `G2.13` | P2D under time compression — performance envelope with the error bound held | T2 | W1 +W6 | G2 | `G2.11` `G2.12` `G2.05` `G1.12` | L | 12 | D5 |
| W9 | `G3.07` | Reflections that respond to the room, not to the sky | T1 | W1 | G3 | `G3.03` `G3.04` `G1.05` `G1.12` | L | 12 | D2 |
| W9 | `G4.02` | Anti-aliasing path that does not require MSAA | T1 | W1 +W3 | G4 | `G4.01` `G3.04` `G1.03` `G1.05` `G1.12` | L | 12 | D3 D2 |
| W9 | `G5.02` | Asset import and export round-trip inside a stated fidelity budget | T1 | W3 +W1/W6 | G5 | `G3.09` `G2.34` `G1.12` | L | 12 | D6 |
| W9 | `G5.23` | Physics regression gate binary that can actually fail | T2 | W3 +W6 | G5 | `G2.02` `G2.07` `G2.12` `G2.17` `G1.03` | M | 6 | — |
| W9 | `G6.15` | Blind UI-craft scorecard — the pack's proof artifact | T3 | W1 +W3 | G6 | `G6.04` `G6.06` `G6.09` `G6.10` `G6.11` `G6.12` `G6.13` `G6.14` `G1.12` | L | 12 | D1 D4 D6 D7 |
| W9 | `G6.35` | Explicit numeric gates for promoting a simulated result to a physical build decision | B1 | W4 +W3/W1 | G6 | `G2.32` `G6.34` | S | 0 | — |
| W9 | `G7.22` § | EUSTRESS-PHYS-12 — public benchmark and reproducible harness | T4 | W1 +W3/W5 | G7 | `G7.18` `G7.19` `G7.20` `G7.21` `G1.12` `G1.48` | XL | 20 | D5 D6 |
| W9 | `G7.45` | The ten-minute cohesion journey — cold install to publish, recorded and judged as one artifact | T5 | W4 +W1/W3 | G7 | `G1.13` `G7.30` `G7.31` `G7.32` `G7.34` `G7.35` `G7.37` `G7.38` `G7.39` `G7.40` `G7.41` `G7.42` `G7.43` `G7.44` | XL | 20 | D1 D4 D5 D6 |
| W10 | `G0.02` | Capability gate on the mutating tool set, generated from the tool census | G0 | W3 +W6 | G0 | `G0.01` | L | 12 | — |
| W10 | `G0.03` | WASM-first plugin sandbox with a capability handshake, from a measured zero | G0 | W5 +W3 | G0 | `G0.01` `G1.01` `G1.03` | XL | 20 | — |
| W10 | `G0.04` § | Data rights for .etask, the benchmark, episode bundles, and models trained on them | G0 | W5 +W2 | G0 | `G7.12` `G7.22` `G1.48` | S | 0 | — |
| W10 | `G0.12` § | SECURITY.md, a disclosure path, and one acknowledged inbound report | G0 | W3 +W6 | G0 | `G0.01` `G0.06` `G1.48` | S | 0 | — |
| W10 | `G3.06` | Screen-space ambient occlusion that grounds objects without haloing | T1 | W1 | G3 | `G4.02` `G3.05` `G1.05` `G1.12` | L | 12 | D2 D6 |
| W10 | `G3.08` | Volumetric atmosphere and fog that agree with the light | T1 | W1 | G3 | `G3.04` `G3.07` `G1.05` `G1.12` | L | 12 | D2 D6 |
| W10 | `G3.10` | Gaussian splats and meshes composite as one scene | T1 | W1 +W3 | G3 | `G3.07` `G3.04` `G1.12` | L | 12 | D2 D6 |
| W10 | `G4.03` | Temporal stability under camera motion — no ghosting, no crawl | T1 | W1 +W3 | G4 | `G4.02` `G4.01` `G1.05` `G1.12` | L | 12 | D3 |
| W10 | `G6.36` | A case-study format that cannot be filled in without a stated counterfactual | B1 | W6 +W2/W3/W4 | G6 | `G6.34` `G6.35` | S | 0 | — |
| W10 | `G7.23` | Measured baseline agent results on EUSTRESS-PHYS-12 | T4 | W1 +W3/W4 | G7 | `G7.22` `G7.09` `G1.12` | L | 12 | D5 D1 |
| W11 | `G0.05` | Enterprise security-questionnaire baseline, every yes citing an existing path | G0 | W3 +W2 | G0 | `G0.01` `G0.02` | M | 6 | — |
| W11 | `G0.11` | Egress allowlist and secret scrubbing on the agent and script surfaces | G0 | W3 +W5 | G0 | `G0.01` `G0.02` `G7.08` | L | 12 | — |
| W11 | `G1.50` § | Extension loaded from outside the git worktree, with a measured first-run time | B2 | W5 +W6 | G1 | `G1.49` `G0.03` | M | 6 | — |
| W11 | `G3.11` | Lighting presets that are one action and measurably distinct | T1 | W6 +W1 | G3 | `G3.04` `G3.05` `G3.08` `G3.09` `G1.12` | M | 6 | D1 D4 |
| W11 | `G4.04` | Render-cascade tier transitions that are not visible as pop | T1 | W1 +W3 | G4 | `G4.03` `G1.12` | L | 12 | D3 D6 |
| W11 | `G6.37` | A referenceability packet a partner can approve in one pass, plus a scored expansion path | B1 | W6 +W2/W4 | G6 | `G6.36` | S | 0 | — |
| W11 | `G7.24` § | Lab integration guide | T4 | W5 +W3 | G7 | `G7.20` `G7.22` `G1.12` `G1.48` `G7.23` | M | 6 | D6 D1 |
| W12 | `G0.07` § | An engineer outside the company produces an episode bundle the verifier accepts | G0 | W1 +W3/W5 | G0 | `G7.20` `G7.24` `G0.04` `G0.06` | M | 6 | — |
| W12 | `G1.46` | Procurement-readiness matrix for a deep-tech enterprise buyer | B2 | W2 +W3 | G1 | `G1.41` `G1.42` `G0.05` | S | 0 | — |
| W12 | `G3.12` | The first-three-seconds opening frame | T1 | W1 | G3 | `G3.11` `G4.03` `G3.06` `G7.39` `G1.12` | L | 12 | D1 D6 |
| W12 | `G5.01` | HLOD proxy fidelity at the swap boundary | T1 | W1 +W3 | G5 | `G4.04` `G1.12` | L | 12 | D3 D2 |
| W13 | `G0.10` § | One submission from outside the company scored on EUSTRESS-PHYS-12 | G0 | W1 +W5/W3 | G0 | `G7.22` `G7.23` `G0.07` | M | 6 | — |
| W13 | `G2.40` | Claim-to-evidence map for the sold vertical, with every unsupported claim struck | B2 | W4 +W3 | G2 | `G1.41` `G6.40` `G1.46` | S | 0 | — |
| W13 | `G3.13` | Flagship scene that survives a blind comparison | T1 | W1 +W3/W4 | G3 | `G3.12` `G5.01` `G5.02` `G3.10` `G1.12` | XL | 20 | D1 D2 D3 D5 D6 D7 |

---

## 4. Wave grouping

A **wave** is a set of items whose declared dependencies and §8 gates are all satisfied by earlier
waves. Items inside one wave may run concurrently **on everything except the build slot**.

### 4.1 What makes concurrency safe

Two things, and only these two.

**File disjointness is already resolved.** `04_FILE_OWNERSHIP.md` converted every cross-pack path
collision into a `depends_on` edge — 32 contested paths, 44 items given new edges, 52 scope lists
edited. On an `EXCLUSIVE` path only the owner may edit at all; the path was struck from every other
claimant's in-scope list. The edges hold in the wave grid too: **for all seventeen `EXCLUSIVE` rows,
the owner sits in a strictly earlier wave than every claimant in another pack**, so a non-owner
always reads the shape its owner finished. Intra-pack contention is ordered by each pack's own
closed acyclic graph. Same-wave items therefore do not collide on behaviour files, and no wave
assignment here needs to re-litigate that.

**Tier S consumes no build slot.** 28 items are tier S: 0 builds, 60k tokens, 1 h. They are the
cheap parallelism. Any number of them may run at once, alongside the single build-consuming item
that holds the slot. In the table above, a tier S row is exactly a row with `0` in the Builds
column.

### 4.2 What forces serialisation

**The build slot.** One build at a time, program-wide. Within a wave, the build-consuming items are
a queue, not a set. The wave's build column is therefore a wall-clock floor for that wave that no
amount of parallelism removes.

**The residual shared-path risk.** `04_FILE_OWNERSHIP.md` §2 leaves `APPEND-ONLY` and `DIRECTORY`
paths in every claimant's scope on purpose — removing them would make most of the program
unexecutable. Two non-owners in the *same* wave can therefore still touch one file. Four such pairs
exist, and L0 orders each pair within its wave rather than running them together:

| Wave | Path | Disposition | Owner (`04`) | Items that must be ordered |
|:--|:--|:--|:--|:--|
| W2 | `eustress/crates/engine/Cargo.toml` | APPEND-ONLY | `G1.03` | `G2.01`, `G7.03` |
| W5 | `eustress/crates/engine/src/engine_bridge/protocol.rs` | APPEND-ONLY | `G1.05` | `G3.01`, `G7.09` |
| W6 | `eustress/crates/engine/src/engine_bridge/protocol.rs` | APPEND-ONLY | `G1.05` | `G4.01`, `G7.13` |
| W9 | `eustress/crates/engine/Cargo.toml` | APPEND-ONLY | `G1.03` | `G4.02`, `G5.23` |

Ordering within the pair is L0's call; both orders are legal. What is not legal is dispatching both
at once, because the second agent measures against a manifest or module list the first is midway
through changing.

`eustress/crates/agent-eval/` — a DIRECTORY path owned by `G7.01` — is no longer on this list.
`G0.07` and `G0.11` both claim it, and they now sit in different waves: `G0.11` at W11, `G0.07` at
W12 behind `G7.24`. The graph orders that pair without L0 having to.

### 4.3 The waves

| Wave | Items | Tier S (free) | Build-consuming | Builds | Serial compile (h) | Cumulative builds |
|:--|--:|--:|--:|--:|--:|--:|
| W0 | 4 — **4 retired** | 3 — **3 retired** | 1 — **1 retired** | 6 — **6 spent** | 1–2 | 6 |
| W1 | 11 — **1 retired** | 1 | 10 — **1 retired** | 72 — **2 spent** | 12–18 | 78 |
| W2 | 13 | 5 | 8 | 80 | 13–20 | 158 |
| W3 | 17 | 4 | 13 | 110 | 18–28 | 268 |
| W4 | 17 | 2 | 15 | 126 | 21–32 | 394 |
| W5 | 19 | 2 | 17 | 144 | 24–36 | 538 |
| W6 | 18 | 1 | 17 | 182 | 30–46 | 720 |
| W7 | 15 | 1 | 14 | 154 | 26–39 | 874 |
| W8 | 11 | 2 | 9 | 90 | 15–23 | 964 |
| W9 | 10 | 1 | 9 | 118 | 20–30 | 1,082 |
| W10 | 10 | 3 | 7 | 92 | 15–23 | 1,174 |
| W11 | 7 | 1 | 6 | 48 | 8–12 | 1,222 |
| W12 | 4 | 1 | 3 | 30 | 5–8 | 1,252 |
| W13 | 3 | 1 | 2 | 26 | 4–7 | 1,278 |

**W0 is exactly the four items that declare no dependency** — `G1.01`, `G1.02`, `G1.30`, `G6.01` —
and it is closed. `G1.01` ran first and alone (§1); the other three are tier-S censuses that consume
no build. Every other item in the library, including the three whose only edge is `G1.01`, has a
floor of W1 or later, because a wave floor counts declared edges and an item with one edge cannot
sit in the same wave as the item it waits for.

**W0 state: four of four retired, 6 of 6 build slots spent.**

| Item | Tier | State |
|:--|:--|:--|
| `G1.01` | M | ✔ `PASSED` — 6 of 6 builds |
| `G1.02` | S | ✔ `PASSED` |
| `G6.01` | S | ✔ `PASSED` |
| `G1.30` | S | ✔ `PASSED` — remediation debt carried behind `G1.31` (§7) |

**W1 is where the program now stands: one of eleven retired, 2 of 72 build slots spent.** Ten of its
eleven items are build-consuming — the widest build queue in the first half of the library — and the
one free item is `G1.31`.

| Item | Tier | State |
|:--|:--|:--|
| `G0.06` | M | ✔ `PASSED` — 2 of 6 builds |
| `G7.30` | M | ◐ `PARTIAL` — code written, never compiled, no artifact (§7) |
| `G1.40` | M | not started |
| the other eight | L/M/S | not started |

W1 closes on the build slot: `G7.30` needs it to finish, `G1.40` needs it to begin, and eight more
items are queued behind them.

Membership, split by whether the item needs the build slot:

| Wave | Tier S — run wide, no build slot | Build-consuming — queue for the slot |
|:--|:--|:--|
| W0 | `G1.02` ✔ `G1.30` ✔ `G6.01` ✔ | `G1.01` ✔ (first, alone) |
| W1 | `G1.31` | `G0.06` ✔ `G1.03` `G1.05` `G1.07` `G1.08` `G1.40` `G6.03` `G6.30` `G7.01` `G7.30` ◐ |
| W2 | `G1.41` `G1.48` `G6.32` `G7.11` `G7.33` | `G1.04` `G1.06` `G1.43` `G1.56` `G2.01` `G2.34` `G7.02` `G7.03` |
| W3 | `G1.42` `G1.49` `G1.52` `G1.53` | `G1.09` `G2.02` `G2.17` `G2.18` `G2.35` `G2.36` `G5.20` `G7.04` `G7.05` `G7.07` `G7.31` `G7.34` `G7.35` |
| W4 | `G1.44` `G1.55` | `G1.10` `G1.11` `G1.12` `G1.47` `G1.51` `G2.03` `G2.30` `G2.37` `G5.21` `G6.02` `G7.06` `G7.32` `G7.36` `G7.39` `G7.40` |
| W5 | `G1.14` `G1.45` | `G1.13` `G2.04` `G2.07` `G2.08` `G2.15` `G2.16` `G2.31` `G3.01` `G6.04` `G6.06` `G6.09` `G6.40` `G7.08` `G7.09` `G7.12` `G7.15` `G7.41` |
| W6 | `G1.54` | `G2.05` `G2.06` `G2.09` `G2.14` `G2.32` `G3.02` `G4.01` `G5.22` `G6.05` `G6.07` `G6.10` `G6.11` `G6.13` `G6.31` `G7.10` `G7.13` `G7.37` |
| W7 | `G6.33` | `G0.09` `G2.10` `G2.33` `G3.03` `G3.05` `G6.08` `G6.12` `G7.14` `G7.16` `G7.18` `G7.20` `G7.38` `G7.42` `G7.43` |
| W8 | `G0.08` `G6.34` | `G2.11` `G2.12` `G3.04` `G3.09` `G6.14` `G7.17` `G7.19` `G7.21` `G7.44` |
| W9 | `G6.35` | `G0.01` `G2.13` `G3.07` `G4.02` `G5.02` `G5.23` `G6.15` `G7.22` `G7.45` |
| W10 | `G0.04` `G0.12` `G6.36` | `G0.02` `G0.03` `G3.06` `G3.08` `G3.10` `G4.03` `G7.23` |
| W11 | `G6.37` | `G0.05` `G0.11` `G1.50` `G3.11` `G4.04` `G7.24` |
| W12 | `G1.46` | `G0.07` `G3.12` `G5.01` |
| W13 | `G2.40` | `G0.10` `G3.13` |

### 4.4 How to read the shape

Tier S thins after W5 without disappearing: **17 of the 28 free items sit in W0–W5** and the other
eleven are spread one to three per wave across W6–W13, so no wave is entirely build-bound. The
cheap parallelism is front-loaded, not exclusive to the front.

What the back half really costs is builds. **W6 onward holds 78 items and 740 of the 1,278 build
slots — 123 to 185 hours of pure compile** — and the eleven free items scattered through it overlap
none of that, because a tier-S item consumes no build slot and therefore removes no compile time
from the queue. W6 alone is 182 builds, the heaviest wave in the library.

**The tail is worth reading closely.** W12 and W13 hold seven items and only 56 builds, so they look
like a taper — but two of the seven are `G0.07` and `G0.10`, the outward-facing pair §6.3 explicitly
does **not** defer: an engineer outside the company producing a bundle the verifier accepts, and an
outside submission scored on the public benchmark. Those are the program's two most externally
legible G0 results and the graph floors them dead last, `G0.07` at W12 behind `G7.24` and `G0.10` at
W13 behind `G0.07`. Nothing is wrong with the arithmetic; the chain is real and every edge in it is
declared. It is the shape L0 should look at, because a program whose external proof arrives last has
no external proof for most of its length.

---

## 5. The critical path

**Target: `G1.13` — one command produces a valid capture bundle from a clean checkout with no
operator-local state.** Fifteen items, two of them retired; **thirteen remain**. This is the single
most useful thing in this file.

### 5.1 Why `G1.13` is the first externally-credible artifact

"Externally credible" means a party outside the company verifies the claim without talking to anyone
here. The library already defines what that takes: `00_MASTER_PROTOCOL.md` §3.2 sets G1's exit
condition as a **content-addressed bundle with a valid provenance manifest**, produced by a single
command. Until that bundle exists, an outsider has a number and our word for it.

`G1.13` also closes the first of the four definition-of-done conditions in `00_MASTER_PROTOCOL.md`
§1.1 that this program can close by its own effort:

- **D2** — two independent runs, byte-identical recordings and byte-identical capture frames — is
  exactly `G1.11`'s two-capture selftest running inside `G1.13`'s clean checkout.
- **D1** is conditional on `G1.14`, a human procurement (§8.3), and degrades to D1′ otherwise.
- **D3** needs CI to execute the workspace test suite and the harness — `G7.34`, `G7.35`, `G7.43`.
- **D4** is a signed agreement, a human action under `00_MASTER_PROTOCOL.md` §6.

So D2 is the first one an agent can finish, and `G1.13` is the item that finishes it in a form a
stranger can rerun from a clone.

### 5.2 The chain

Fifteen items, in dependency order. Every one is on the path; removing any one breaks it. The two
retired links are the two that had no dependencies, so the path now starts at `G1.03`.

| # | ID | Title | Tier | Builds | Why it is on the path |
|--:|:--|:--|:--|--:|:--|
| 1 | `G1.01` ✔ | `cargo build --workspace` exits 0 | M | 6 | §1 — `PASSED`, 6 builds spent |
| 2 | `G1.02` ✔ | Pinned reference machine `RM-1` and the manifest hardware block | S | 0 | The manifest carries a hardware block; without a named machine no threshold in the library is checkable — `PASSED`, RM-1 pinned |
| 3 | `G1.03` | Harness scene set `S1`–`S6` from a seeded generator | L | 12 | There is nothing to capture until a frozen scene exists |
| 4 | `G1.05` | AI camera capture accepts width, height, and output path | M | 6 | `ai_camera.rs:43-44` hardcodes 1280×720 and `request_capture` at `:158` takes only a path |
| 5 | `G1.04` | Camera path files `CP-A/B/S/U` and the tick-driven path player | M | 6 | A capture with no declared camera path is not reproducible |
| 6 | `G1.06` | Captures fire at declared simulation tick indices | M | 6 | Byte-identity requires the frame to be taken at the same tick, not the same wall-clock moment |
| 7 | `G1.07` | Independent physics-step counter in the recording and manifest | M | 6 | `clock.rs:100-102` zeroes the accumulator on saturation, so the clock can report time never stepped |
| 8 | `G1.08` | Per-frame tick-indexed frame-time CSV with p50/p99/max/cv | M | 6 | The bundle's measurement payload |
| 9 | `G2.01` | Simulation evidence harness and physics baseline ledger | L | 12 | `G2.02` cannot state a determinism result without a baseline to state it against |
| 10 | `G2.02` | Determinism gate — byte-identical Avian reruns, runnable by command | M | 6 | `common/tests/determinism.rs` is gated behind a non-default `physics` feature and never runs |
| 11 | `G1.09` | `eustress-capture` binary producing a content-addressed bundle with a valid manifest | XL | 20 | The artifact itself; three items in T3 and T5 already invoke this binary and cannot be measured without it |
| 12 | `G1.10` | Blinding subcommand with metadata scrub, leak checklist, separated key | L | 12 | An unblinded bundle cannot be handed to a Critic or an outsider |
| 13 | `G1.11` | Determinism verification mode and the two-capture byte-identity selftest | L | 12 | This is D2 |
| 14 | `G1.12` | All 33 cited capture recipes authored once in one format | L | 12 | 53 items cite a recipe across 33 paths under a directory that does not exist, and every one of them now declares a `G1.12` edge (§8.2) |
| 15 | `G1.13` | One command, clean checkout, valid bundle, no operator-local state | L | 12 | The terminus |

Written as an ordered chain:

The same chain as edges, exactly as the front matter declares them:

```
G1.02  <- (none)
G1.01  <- (none)
G1.03  <- G1.01
G1.05  <- G1.01
G1.07  <- G1.01
G1.08  <- G1.01
G1.04  <- G1.03
G2.01  <- G1.03
G1.06  <- G1.05
G2.02  <- G2.01
G1.09  <- G1.02, G1.04, G1.06, G1.07, G1.08
G1.10  <- G1.09
G1.12  <- G1.02, G1.09
G1.11  <- G1.09, G2.02
G1.13  <- G1.10, G1.11, G1.12
```

`G1.11` takes two edges — `G1.09` for the capture side, `G2.02` for the physics side — which is why
`G2.01` and `G2.02`, both T2 items, sit on a G1 critical path at all.

**The chain did not shorten.** `G1.01` landing removed a link from the front of the path but not a
step from its length: `G1.03`, `G1.05`, `G1.07` and `G1.08` all hung on `G1.01` and are now all
immediately runnable, four ways wide but still queued behind a single build slot. Nor did closing the
harness gate shorten it. Every chain member's edges are exactly what they were — `G1.12` already
declared `[G1.02, G1.09]` — so the 52 `G1.12` edges the rest of the library now carries changed the
graph around the path without touching the path itself.

What changed is the depth. **The deepest remaining chain to `G1.13` is five items**:
`G1.03` → `G1.04` → `G1.09` → `G1.10` → `G1.13` on the capture side, and
`G1.03` → `G2.01` → `G2.02` → `G1.11` → `G1.13` on the physics side, the two converging only at the
terminus. Five waves, thirteen items, one build slot.

**And it did not change when `blocks` became authoritative.** Seventeen ordering constraints that
had lived only inside a blocker's `blocks` list were declared as `depends_on` edges and every wave
floor in this file was re-derived from the result (§8.7). Thirty items moved. **None of them is on
this chain.** No item in the fifteen gained an edge, and no item any of the fifteen depends on moved
a wave: `G1.03` `G1.05` `G1.07` `G1.08` sit at W1, `G1.04` `G1.06` `G2.01` at W2, `G1.09` `G2.02` at
W3, `G1.10` `G1.11` `G1.12` at W4, and `G1.13` at W5, exactly as before. The seventeen missing edges
were all in the commercial, security and CI regions of the graph, and the capture spine runs clear of
every one of them. The chain, its length, its cost and its dates in §5.3 and §5.4 stand unchanged.

The program's overall depth also held at 13 waves, though its terminus is not unique: `G3.13`,
`G0.10` and `G2.40` all floor at W13 (§4.4). All three of those deepest chains open on the same four
links — `G1.01` → `G1.03` → `G1.04` → `G1.09` — and diverge only at `G1.09`, into rendering via
`G1.11`, and into the substrate and its security face via `G1.12`. **The longest path in the library,
whichever terminus you measure to, begins with the first four steps of this critical path.** That is
the strongest argument §5 has for its own dispatch priority: those four items are not merely first,
they are the prefix of everything.

### 5.3 What it costs

| Quantity | Whole chain | Still to spend |
|:--|--:|--:|
| Items | 15 of 159 (9.4%) | **13** of 154 remaining (8.4%) |
| Build slots | 134 of 1,278 (10.5%) | **128** of 1,270 remaining (10.1%) |
| Pure serial compile | 22–34 h | **21–32 h** |
| Wall-clock envelope | 20.6 working days | **20.0 working days** |
| At the 150% stall trigger | 30.9 working days | **30.0 working days** |
| Token envelope | 5.66M of 55.18M | **5.40M** of 54.60M |

### 5.4 The honest date range

From **2026-08-10**, working days only, one build at a time, counting only what is left:

| Scenario | Working days | Finish |
|:--|--:|:--|
| Critical path holds the build slot exclusively; only tier S work runs alongside | 20 | **2026-09-07** |
| One or two items consume the 150%-of-envelope stall allowance | 30 | **2026-09-21** |
| Queue runs W0–W5 broadly — 76 items left, 526 builds, 73.2 days of envelope | 73–110 | **2026-11-19 to 2027-01-11** |

**Every row is the same arithmetic on a different item set.** The day count is the summed
`wallclock_envelope` of the items still to run in that set, and where a row gives a range the upper
bound is the 150% stall trigger from `00_MASTER_PROTOCOL.md` §7 applied to the same sum. Rows 1 and 2
take the §5 critical path — 20.0 days at envelope, 30.0 at the trigger. Row 3 takes everything whose
wave floor is W5 or earlier, minus the five retired items — 73.2 days at envelope, 109.8 at the
trigger. Nothing here is scaled by an assumed concurrency factor, because the build slot is the only
resource the library actually meters and it is already serial.

**These are envelope arithmetic, not forecasts.** They assume every item passes inside its declared
envelope on the first pass. `00_MASTER_PROTOCOL.md` §7 caps each item at 9 iterations and 3
approaches, and a floor miss produces a STALL addressed to a human (§7 of this file) whose response
time is not in any envelope in this library. `G1.01` is the evidence for how that assumption behaves:
it took 3 iterations and two approaches and still landed inside its 6-build envelope, but it did so
by way of a stall packet a human had to answer. The one number here that is a measurement rather than
a projection is 128 builds; at the 10–15 minute build the operator already measures, that is 21–32
hours of compile that cannot be parallelised or skipped.

**The lever.** Scenario 1 finishes eleven to eighteen weeks earlier than scenario 3 and produces the
same artifact. The difference is entirely a dispatch decision: give the critical path the build slot
and put everything else in §6.

### 5.5 What `G1.13` does not prove

Stated plainly so nobody over-claims on it.

- **Not machine independence.** `G1.13` measures a clean checkout with `HOME`/`USERPROFILE`
  redirected to an empty temp dir and every `EUSTRESS_*` variable cleared — on the same box. A
  GPU-driver-dependent render difference survives that undetected. `G1.14` frames the second-machine
  question as an explicit human decision.
- **Not headless.** D2 says "the same headless recipe". `G7.03` builds the headless render tier and
  `G2.03` carries determinism through the full headless stack. Extending the chain to `G2.03` adds
  `G7.30`, `G7.01`, `G7.03` and `G2.03` — **4 items, 50 builds, 9.5 working days** — for 19 items,
  184 builds, 30.1 days total. Until that lands, every capture needs a desktop session.
- **Not quality.** The bundle proves the instrument reproduces. It says nothing about whether what
  it captured is good. That is D1/D1′ and it starts at `G3.01`.

---

## 6. The deferred set

### 6.1 The proportions, recomputed across all nine packs

| Measure | Items | % of 159 | Builds | % of 1,278 |
|:--|--:|--:|--:|--:|
| Agentic training substrate (T4) | 24 | 15.1% | 264 | 20.7% |
| …plus its external face in G0 (`G0.04`, `G0.07`, `G0.10`) | 27 | 17.0% | 276 | 21.6% |
| …of which face outward at all | 11 | 6.9% | — | — |
| Rendering + physics-visual + studio UI (T1 + T2 + T3) | 56 | 35.2% | 570 | 44.6% |

The eleven outward-facing substrate items are `G7.11`, `G7.12`, `G7.13`, `G7.16`, `G7.20`, `G7.22`,
`G7.23`, `G7.24`, `G0.04`, `G0.07`, `G0.10` — the ones whose artifact a party outside the company
reads, runs, or submits against. The other sixteen T4 items build the machinery those eleven expose.

**The ratio is wrong, and §6.2 is the correction.** The stated wedge holds **20.7%** of the build
budget. The visual surface holds **44.6%** — more than twice as much — and 170 of those builds sit
on T1 items that cannot be measured at all until a capture bundle exists. Three of the eleven
outward-facing items (`G0.04`, `G0.07`, `G0.10`) live in G0 rather than in the substrate pack
itself, which is why deferring against pack boundaries would cut the wrong way and the groups in
§6.2 are drawn against artifacts instead.

### 6.2 What is deferred

Nothing is deleted. Every item below keeps its ID, its front matter, and its place in the table in
§3. Deferred means: **not dispatched, not counted against the next milestone, and re-entered at the
named condition.**

**49 items · 408 build slots · 68–102 hours of pure compile · 69.5 working days of envelope.**
That is **31.9% of the program's build budget** held back behind §5.

| Group | Items | Builds | Compile deferred | Envelope |
|:--|--:|--:|--:|--:|
| **DEFER-1** — render fidelity tail | 14 | 170 | 28–42 h | 29.5 d |
| **DEFER-2** — electrochemistry depth ladder | 5 | 62 | 10–16 h | 11.5 d |
| **DEFER-3** — studio UI craft tail | 8 | 92 | 15–23 h | 16.0 d |
| **DEFER-4** — scale and streaming | 6 | 60 | 10–15 h | 9.0 d |
| **DEFER-5** — commercial and organisational tail | 16 | 24 | 4–6 h | 3.5 d |
| **Total** | **49** | **408** | **68–102 h** | **69.5 d** |

---

**DEFER-1 — render fidelity tail.** `G3.03` `G3.04` `G3.05` `G3.06` `G3.07` `G3.08` `G3.09` `G3.10`
`G3.11` `G3.12` `G3.13` `G4.02` `G4.03` `G4.04` — 14 items, 170 builds, 28–42 h of compile.

*Why.* Every one of these gates on a blind Critic score, and all fourteen cite a capture recipe. Not
one can be measured before `G1.13`. Running them earlier produces scores against an unpinned
instrument, which is the failure this program exists to stop. `G3.13` additionally carries **D7**,
the preference dimension, which under D1′ is not scored at all (§8.3).

*Re-enabled by.* `G1.13` `PASSED`, **and** L0 recording D1 versus D1′ in
`docs/PROMPTS/artifacts/ledger.jsonl`. `G3.13` additionally needs `G1.14` to have landed a licensed
control; under D1′ it must be re-scoped off D7 before it is dispatched at all.

*Held back separately:* `G3.01` and `G3.02` are **not** deferred. `G3.01` is T1's item zero and
`G3.02` is the `render-probe` image-analysis binary — both are instrument work that the rest of T1
and the Critic depend on, and both belong immediately behind `G1.13`.

---

**DEFER-2 — electrochemistry depth ladder.** `G2.09` `G2.10` `G2.11` `G2.12` `G2.13` — 5 items,
62 builds, 10–16 h of compile. Single-particle model, P2D electrolyte transport
(Doyle-Fuller-Newman), thermal coupling, numerical convergence, and P2D under time compression.

*Why.* This is the deepest engineering ladder in the library and it is speculative until someone
needs it. `G2.08` — the chemistry-agnostic 0-D cell model validated against a public dataset — is
enough to support `G2.32`'s validity envelope and `G2.33`'s public validation report. The four rungs
above it exist to serve a design partner who does not yet exist. Coupling in
`eustress/crates/engine/src/simulation/electrochemistry.rs` runs in both directions today —
temperature enters the rate laws at `:208-219` and `:245`, heat returns at `:279-292` — so the gap
this ladder closes is a lumped thermal node with hardcoded constants at `:281` and `:288`, not an
absent coupling.

*Re-enabled by.* `G2.08` `PASSED`, **and** a `G6.34` pilot whose hashed success metric names a
quantity only P2D can produce. Absent that, the ladder is capability nobody has asked for.

---

**DEFER-3 — studio UI craft tail.** `G6.07` `G6.08` `G6.10` `G6.11` `G6.12` `G6.13` `G6.14` `G6.15`
— 8 items, 92 builds, 15–23 h of compile. Command palette, keyboard coverage, empty and error
states, accessibility annotations, typographic hierarchy, the `slint_ui.rs` split,
time-to-first-task, and the blind UI-craft scorecard.

*Why.* Eustress is an AI-native simulation substrate; the studio is one of its two faces, not the
product. `G6.13` alone is XL, 20 builds, to split a 23,103-line file — real debt, but debt that pays
off in *future* editing speed, and this program has 201 days of envelope in which future editing
speed is not the constraint. `G6.15` carries **D7** and inherits §8.3.

*Not deferred:* `G6.01` through `G6.06` and `G6.09` stay in the queue — the drain-contract
regression test (`G6.03`), the p99 drain-cost floor (`G6.04`), Properties-panel persistence
(`G6.05`), pre-click honesty for the 1,969 unwired ribbon tools (`G6.06`), and one selection identity
(`G6.09`). Those are correctness, not craft: a panel that silently discards an edit and a button that
lies about being wired are defects a design partner hits in the first session.

*Re-enabled by.* `G6.04` and `G6.05` `PASSED`, **and** a named first-time professional user
available for `G6.14`'s time-to-first-successful-task measurement. `G6.14` measures a human; without
one it cannot be run, only simulated, and a simulated result here is worth nothing.

---

**DEFER-4 — scale and streaming.** `G5.01` `G5.02` `G5.20` `G5.21` `G5.22` `G5.23` — 6 items,
60 builds, 10–15 h of compile.

*Why.* G5's phase exit condition is a *measured* entity count at a *measured* frame rate, replacing
the `active_cap` config default cited at `docs/AUDIT/05_SPACE_STREAMING.md:23`. That correction is
already applied at source and in `docs/AUDIT/MASTER.md`, so the standing misstatement is closed. The
measurement that replaces it needs `G1.08`'s frame-time series and `G1.09`'s bundle to mean anything,
and nothing in §5 or in the substrate spine consumes a scale number.

*Re-enabled by.* `G1.13` `PASSED`, **and** either a pilot scope that states an entity count, or a
measured frame-time regression that `G7.40`'s perf-assert gate actually fires on.

---

**DEFER-5 — commercial and organisational tail.** `G1.44` `G1.45` `G1.46` `G1.47` `G1.50` `G1.51`
`G1.53` `G1.54` `G1.55` `G1.56` `G6.32` `G6.33` `G6.34` `G6.35` `G6.36` `G6.37` — 16 items,
24 builds, 4–6 h of compile.

*Why.* Cheap in builds, expensive in the thing this program is shortest of: sequence. Ten of the
sixteen are tier S. `G1.50` and `G1.51` sit behind PROGRAM-LICENCE-GATE and will STALL on dispatch,
which is the gate working (§8.1) but is not progress. `G6.32` through `G6.37` build a qualification
scorecard, a discovery instrument, a pilot metric format, a promotion gate, a case-study format and a
referenceability packet — six artifacts validated against fixtures the same agent authored. They are
legitimate negative controls, and they do not advance D4. **D4 closes on an executed agreement**, a
human action under `00_MASTER_PROTOCOL.md` §6.

*Not deferred:* `G1.30` `G1.31` `G1.40` `G1.41` `G1.42` `G1.43` `G1.48` `G1.49` `G1.52` stay. The
claim ledger and proof standard (`G1.30`/`G1.31`) are prerequisites for anything published; the
revenue-rail truth ledger and vertical selection (`G1.40`/`G1.41`) decide what the program is for;
`G1.48` is the licence memo the entire gate hangs on and must be authored early so the human has it
in hand. `G0.08` — five qualified strangers from named channels completing the discovery instrument,
traceable to a `G0.09` channel — is also not deferred: it is the item that actually advances D4.

*Re-enabled by.* A recorded answer to `G1.48` in the human's own hand, **and** `G0.08` `PASSED`.

### 6.3 What is not deferred

**95 items, 736 builds.** The critical path (§5) is followed immediately by the agent-loop and
training-substrate spine — all 24 T4 items, all 12 G0 items, T5's CI and reliability work, and the
G2 evidence ladder through `G2.08` — plus `G3.01`, `G3.02`, `G4.01`, and the G6 correctness items.
That set contains every one of the eleven outward-facing substrate items, both remaining
definition-of-done conditions an agent can close (D2 via `G1.13`, D3 via `G7.34`/`G7.35`/`G7.43`),
and the only item that advances D4 by contact with a real stranger (`G0.08`).

---

## 7. Stall register

Every STALL raised under `00_MASTER_PROTOCOL.md` §5.3, every FILE-OWNERSHIP block under
`04_FILE_OWNERSHIP.md` §6, and every PROGRAM-LICENCE-GATE stall under §8.1 lands here the moment L0
forwards it. One row per raised stall, plus a row for any item left `PARTIAL`, which is the one state
that blocks nothing and is easiest to lose. A stall leaves the register only when the human's
decision is recorded — never by an agent deciding it has gone stale.

| Prompt ID | Iterations spent | Blocker | Decision requested from the human | Date raised |
|:--|--:|:--|:--|:--|
| `G1.30` | 1 (approach A) | ~~61.5% UNSUPPORTED against a 40% floor.~~ **RESOLVED 2026-08-08 — human chose DEFER.** The §5 criterion had already passed; the ledger stands as complete. Remediation of the 64 unsupported claims is deferred behind `G1.31` and carried as an open debt, **not** written off. The 40% floor was deliberately NOT lowered. `G1.30` → `PASSED`. | Answered — DEFER. Closed. | 2026-08-08 |
| `G1.01` | 3 (approaches A, D) | ~~Item-specific escalation: workspace members other than `crates/backend` failed to compile.~~ **RESOLVED 2026-08-08.** Human granted FUND approach D. Three broken members found and remedied: `eustress-backend` (unlisted), `instance-capacity` (8 wgpu-29 drifts fixed), `eustress-server` (missing `serde_json` dep). `BUILD_EXIT=0`, rerun `0`, 40 members, 6 of 6 builds. | Answered — FUND. Closed. | 2026-08-07 |
| `G7.30` | 1 (approach A, incomplete) | **NOT A STALL — `PARTIAL`, recorded here so the state is visible in one place.** `usage_telemetry.rs` carries 475 uncommitted lines (an `EndReason` enum, a panic beacon); `eustress/crates/engine/src/bin/session-baseline.rs` is 388 lines and is declared in `eustress/crates/engine/Cargo.toml`; **none of it has been compiled**; no artifact exists under `docs/PROMPTS/artifacts/G7.30/`; and the criterion needs ≥20 recorded sessions with ≥3 unclean, while `%LOCALAPPDATA%\Eustress\telemetry\sessions` has never been created, so the count is zero. | **None — no decision is owed.** This is unfinished work awaiting a free build slot, not a blocker awaiting a human. | 2026-08-10 |

**The `G7.30` row is the exception that proves the register's rule.** Every other row names a
decision a human owes the program; that one names none. It is here because a `PARTIAL` item is the
easiest state in the program to lose track of — it looks like progress from the ledger and like a
pass from the file system — and because the only thing standing between it and a verdict is the
build slot. It leaves this register when it passes or fails, not when a person answers anything.

**Filling a row.** `Iterations spent` is the count against the tier's ceiling in
`00_MASTER_PROTOCOL.md` §7 — 3 for S, 6 for M, 9 for L and XL — with the hard ceiling of 9
iterations and 3 approaches that every item's §7 carries. `Blocker` is one line: the floor missed
and the best value achieved, or the contested path and its owner, or `G1.48 decision == null`.
`Decision requested` is exactly one of the four named options in the relevant packet form — never
"review the situation". `Date raised` is ISO-8601.

**Reading the register.** A register with many rows against one item is an item that needs re-scoping.
A register with many rows against many items in one wave is a wave dispatched too early. A register
that stays empty while items keep passing is the outcome this program is least likely to see and
should be treated with suspicion, not satisfaction.

---

## 8. Program gates

Six named gates. An item reaches `READY` only after every gate that applies to it is clear. A gate is
not a suggestion an agent weighs; it is a precondition, and an item dispatched through an unclear
gate is a program error regardless of what it produces.

§8.7 is not a seventh gate. It is the invariant the other six stand on: that the dependency graph is
the whole of the library's ordering truth, with nothing asserted beside it.

### 8.1 PROGRAM-LICENCE-GATE

**Specified in** `00_MASTER_PROTOCOL.md` §4.6. **Clears when** a human records a decision in
`docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`.

`G1.48` produces four costed licence options with `decision: null`, and its own verifier **fails the
item if `decision` is non-null** — recording a decision is a human-only action under
`00_MASTER_PROTOCOL.md` §6. A null decision is therefore the correct output and the gate is the fork
it opens.

**Eleven items are gated:** `G1.49`, `G1.50`, `G1.51`, `G1.52`, `G7.12`, `G7.22`, `G7.24`, `G0.04`,
`G0.07`, `G0.10`, `G0.12`. Each hands an artifact to, or publishes one for, someone outside the
company under licence terms.

**The graph now carries all eleven, and still cannot retire the gate.** Every one of the eleven
reaches `G1.48` through declared edges. Seven always did — `G1.49`, `G1.52` and `G0.04` name it
directly; `G1.50` and `G1.51` inherit it through `G1.49`; `G0.07` through `G0.04` and `G0.10` through
`G0.07`. The other four — `G7.12`, `G7.22`, `G7.24`, `G0.12` — now name it directly too, because
`G1.48`'s own `blocks` list asserted exactly those four constraints and §8.7 made that list
reciprocal. The single largest open question in the program is no longer, in graph terms, blocking
nothing.

Adding those four edges did not retire this gate and could not have. `G1.48`'s
correct output is `decision: null`, so an item can satisfy every declared edge, find `G1.48`
`PASSED`, and still be looking at an undecided licence. What clears this gate is a human act, not an
item passing, and a dependency graph has no way to express that. The decree is the mechanism here,
not the backstop.

**Effect on dispatch.** Work upstream of the artifact is not blocked — an item may design, build,
measure and test. It stops at the moment of handing the result outward, and STALLs. The
`DECISION REQUESTED` line names one of `G1.48`'s four option keys, not one of the four generic
options in §5.3. A queue in which all eleven are stalled on `G1.48` is the gate working.

**Queue consequence.** `G1.48` is tier S, 0 builds, 1 h, and its floor is W2 — it waits only on
`G1.40`, the revenue-rail truth ledger, which waits only on `G1.01`. It should be one of the first
items the program produces, so the human has the memo in hand long before the eleven gated items
arrive. The margin is wide: the earliest-floored gated item is `G1.49` at W3 and the latest is
`G0.10` at W13, so declaring the four new edges cost the schedule nothing. The gate was always early
enough. It simply was not binding anything.

### 8.2 HARNESS-RECIPE-GATE

**Clears when** `G1.09` and `G1.12` are both `PASSED`.

**53 items across seven packs cite a `capture_recipe`, over 33 distinct paths, every one under
`docs/PROMPTS/harness/recipes/` — a directory that does not exist.** `G1.12` authors all 33 once in
one format; `G1.09` builds the `eustress-capture` binary that consumes them.

**The graph enforces this.** Fifty-two of the 53 declare a direct `G1.12` edge; the fifty-third,
`G7.45`, reaches both owners through `G1.13`. Because `G1.12` itself declares `[G1.02, G1.09]`, a
single `G1.12` edge satisfies both halves of the gate, which is why a separate `G1.09` edge appears
nowhere — it would be a redundant edge carrying no information. Verified mechanically in §9: zero
recipe-citing items fail to reach both owners.

**The decree is the backstop.** Any item whose front matter names a `capture_recipe` other than
`none` is not `READY` until `G1.09` and `G1.12` have passed, whether or not its edge survives a
future edit to its pack. An item that reaches the owners by edge and an item that reaches them by
this clause are equally blocked; the difference is only that the first can be checked by a script.

An agent that finds its recipe absent **does not author one.** Five packs independently instruct
their agent to write the missing recipe "against the schema in §8 of that document", which would
produce five mutually incompatible formats authored by five agents who never meet. That instruction
is superseded here: a missing recipe is a `FILE-OWNERSHIP`-shaped escalation naming `G1.12` as owner.

### 8.3 CRITIC-CALIBRATION-GATE and the D1/D1′ record

**Clears when** two things are on record.

**First, calibration.** `00_MASTER_PROTOCOL.md` §8 step 3 requires the Critic to be run once against
an `eustress@HEAD` versus `eustress@HEAD` bundle — both sides identical — and to score them **within
0.5 on every dimension**. Until that has been executed and archived, the Critic is an uncalibrated
instrument and no `critic_gate` result from it means anything. **53 items carry a `critic_gate`**;
none is `READY` before this clears. The gate needs `G1.09` and `G1.10`, so in practice it clears
in W5.

**Second, D1 versus D1′.** `00_MASTER_PROTOCOL.md` §1.1: which of D1 and D1′ is in force is a human
decision recorded once by L0 in `docs/PROMPTS/artifacts/ledger.jsonl`. **Until it is recorded, assume
D1′, and do not dispatch an item that gates on the preference question.** Two items still carry
**D7**: `G3.13` and `G6.15`. Both are `BLOCKED`, not `READY`, until either `G1.14` lands a licensed
control with `publication_rights` determined, or the item is re-scoped off D7 under a human decision.

### 8.4 WORKSPACE-BUILD-GATE

**Clears when** `G1.01` is `PASSED`. **It is clear** — `G1.01` passed 2026-08-08 (§1).

**The graph enforces this.** All 131 build-consuming items reach `G1.01` transitively; thirteen name
it directly and the rest inherit it. Verified mechanically in §9: zero build-consuming items fail to
reach it. The gate is now a property of the library rather than an instruction laid over it.

**The decree is the backstop, and it is still worth keeping.** Global and unconditional: no item is
dispatched before `G1.01` is `PASSED`, edge or no edge. §1 records what happened the one time this
was checked — three broken members, none of them the one the library predicted, in a workspace where
per-package CI had been green throughout. A per-package green while the workspace is red is exactly
the kind of number this program exists to stop publishing, and that failure mode does not require a
missing edge to recur. It requires only that someone add a member and no one build it.

### 8.5 FILE-OWNERSHIP-GATE

**Specified in** `04_FILE_OWNERSHIP.md`. **Clears per path**, when that path's owner is `PASSED`.

32 contested paths, one owner each. An item reaches `READY` only when the owner of every
`EXCLUSIVE` path in its scope has passed. `APPEND-ONLY` and `DIRECTORY` paths do not block, but bind:
a claimant may add its own entry and may not alter, reorder, or remove an entry it did not add, and
same-wave pairs on those paths are ordered by L0 rather than run together (§4.2).

An agent that hits an out-of-scope entry naming `04_FILE_OWNERSHIP.md` emits the `FILE-OWNERSHIP`
packet in §6 of that file. It does not edit the file and does not work around it. Only L0 may grant a
bounded exception, and granting oneself one is an out-of-scope edit that
`00_MASTER_PROTOCOL.md` §2.2 forbids outright.

### 8.6 HUMAN-DECISION-GATE

**Specified in** `00_MASTER_PROTOCOL.md` §6. **Clears** only by a human act, item by item.

Six classes of action no agent at any level may perform: changing the licence or publishing any
external comparison naming a third-party product; any external publication; any spend, pricing
decision, or signed agreement; production deployment; any Bliss ledger design touching
transferability or cash-out; and merging to `main`.

Two items in this library are defined by such an act and are marked accordingly:

| Item | The human act | What waits on it |
|:--|:--|:--|
| `G1.14` | Select the reference product, acquire the artifact, perform the capture, determine `publication_rights` — a spend and a third-party comparison | D1; without it D1′ is in force and the preference question is never scored |
| `G0.08` | Five qualified strangers, from named `G0.09` channels, complete the discovery instrument | D4 — and D4 itself closes only on an executed agreement, which is also this gate |

`G1.14` is tier S with 0 builds and sits in W5. Its cost is not compute; it is a decision and a
purchase, and no amount of agent effort substitutes.

### 8.7 `blocks` is authoritative, and it is a mirror

**`depends_on` is the single source of ordering truth in this library.** Every wave floor in §3,
every wave membership in §4.3, and every reachability claim in §9 is computed from that field and
from nothing else.

**`blocks` is a readability aid, not a second constraint channel.** It exists so a reader looking at
an item can see what waits on it without grepping the other eight packs. It carries no ordering
constraint of its own, because **every `blocks` entry has a reciprocal `depends_on` on the blocked
item**: if `X` declares `blocks: [Y]`, then `Y` declares `depends_on` reaching `X`. 342 `blocks`
pairs are declared across the nine packs; 319 have the reciprocal edge stated directly on the blocked
item and the remaining 23 reach the blocker through a chain that already exists, so adding a direct
edge would only restate what the graph says. **Zero pairs are uncovered at any depth.**

**Why this is written down.** A `blocks` entry with no reciprocal is invisible to everything that
schedules work. §3's depends_on column is verbatim front matter, §4 computes wave floors from that
column, and neither reads `blocks` — so an unmirrored `blocks` entry asserts an ordering constraint
that no gate, no floor and no verification row will ever see. That is not a cosmetic asymmetry: it
lets an item be scheduled at or before something the library has explicitly said must precede it.

**`graph_check.py` is where this is enforced.**
`docs/PROMPTS/harness/checkers/graph_check.py` re-derives the library graph from the pack files
alone, reading no summary and trusting no declared total — including the totals in this section. It
performs nine checks and exits `0` clean, `1` on a defect, `2` on a usage or environment error.

**Check 8 is BLOCKS-RECIPROCITY**: for every `blocks: [Y]` on `X`, it fails unless `Y` reaches `X`
by following `depends_on` forward, and it classifies each violation by severity — undeclared but
correctly ordered, a wave inversion, or a same-wave pair that would run concurrently. **Check 9 is
WAVE-COLUMN AGREEMENT**: for every row in §3 and every membership list in §4.3, it fails unless the
stated wave equals the floor recomputed from the declared graph. Together they close the two ways
this section's invariant could rot — the edge disappearing, and the schedule drifting from the edge.

Current state: **342 `blocks` pairs, 0 violations; 159 wave rows, 0 mismatches.**

Both checks are validated against mutants rather than asserted. Stripping a single reciprocal edge
fires check 8 alone while checks 1–7 and 9 stay silent; corrupting one Wave cell fires check 9 alone.
That isolation is the evidence that check 8 catches what the original seven could not — the seven
reported `CLEAN` for as long as the defect existed, which is what a blind spot looks like from the
inside.

One property of the checker does hold today and is worth naming, because it bounds the risk of the
repair itself: cycle detection already runs over the **merged** `depends_on` + `blocks` graph, and
`blocks: [Y]` on `X` contributes the identical edge a reciprocal `depends_on` would. Declaring a
reciprocal therefore adds no edge the merged graph did not already carry, and the merged graph was
already acyclic. That is the structural reason all seventeen previously-uncovered pairs were
declarable without a single refusal, and why the cycle count is still zero.

**What it cost the schedule.** Seventeen pairs had no reciprocal at any depth. Six of those were live
wave inversions — the blocked item floored at or before the item declared to block it — and those six
are the reason this mattered rather than being tidy-up. Re-deriving every floor from the repaired
graph moved 30 items in total, because moving an item moves everything downstream of it.

| Blocker | Blocked | Blocked floor, before | after | Was |
|:--|:--|:--|:--|:--|
| `G0.05` W11 | `G1.46` | W3 | **W12** | inverted by 8 waves |
| `G0.03` W10 | `G1.50` | W3 | **W11** | inverted by 7 waves |
| `G6.31` W6 | `G6.33` | W3 | **W7** | inverted by 3 waves |
| `G7.37` W6 | `G7.43` | W5 | **W7** | inverted by 1 wave |
| `G7.23` W10 | `G7.24` | W10 | **W11** | same wave — would have run concurrently |
| `G7.41` W5 | `G7.43` | W5 | **W7** | same wave — would have run concurrently |

**No blocker moved.** All six corrections landed on the blocked item, which is what an append-only
reciprocity repair must do — it adds edges and never relaxes one. `G7.43` appears twice because it
took four new edges at once and its floor is set by the latest of them, `G7.37` at W6. The furthest
single move in the library is `G1.46`, W3 → W12, nine waves.

The other eleven pairs were correct by luck rather than by declaration: the blocked item already sat
after its blocker, so the new edge is not what sets its floor. Ten of the eleven therefore moved
nothing at all. **The eleventh is the one that shows why luck is not a scheduling policy.**
`G1.46` blocks `G2.40`, and at the old floors — `G1.46` W3, `G2.40` W5 — that read as satisfied. But
`G1.46` was itself one of the six inversions. The moment it moved to W12, the newly declared edge
carried `G2.40` to W13. Had that edge stayed unmirrored, `G2.40` would have sat at W5, seven waves
ahead of an item it is declared to wait for, and no check in this library would have said a word.

An ordering constraint that happens to be satisfied is not the same as one that is enforced. The
eleven are declared for the same reason the six are.

---

## 9. Verification

Performed mechanically over the front matter of all 159 items in `docs/PROMPTS/packs/`, parsed from
the files rather than from any pack's own summary.

| Check | Result |
|:--|:--|
| Items parsed | 159 across 9 packs — G0 12, G1 14, T1 19, T2 22, T3 15, T4 24, T5 16, B1 18, B2 19 |
| Duplicate item IDs | none |
| **Every `depends_on` ID resolves to a real item ID in some pack** | **yes — 0 unresolved** |
| Combined nine-pack dependency graph | acyclic — 0 cycles over the merged `depends_on` + `blocks` graph |
| Items with no declared dependency | 4 — `G1.01`, `G1.02`, `G1.30`, `G6.01`. These four are W0 and nothing else is |
| `blocks` pairs declared across the nine packs | 342 |
| **`blocks` pairs with no reciprocal `depends_on` at any depth** | **0 of 342** — 319 direct, 23 transitive (§8.7) |
| Wave floors in §3 and §4.3 re-derived from `depends_on` alone | yes — longest chain to a dependency-free item, 159 of 159 rows |
| Deepest chain in the library | 13 edges — three items floor at W13: `G0.10`, `G2.40`, `G3.13` |
| Deepest chain to `G1.13`, the §5 target | 5 edges — unchanged by the `blocks` repair (§5.2) |
| **Build-consuming items not reaching `G1.01`** | **0 of 131** — WORKSPACE-BUILD-GATE (§8.4) is in the graph |
| **Recipe-citing items not reaching both `G1.09` and `G1.12`** | **0 of 53** — HARNESS-RECIPE-GATE (§8.2) is in the graph |
| §3 rows whose `depends_on` cell differs from the front matter | 0 of 159 |
| Tier envelope tuples | 4 distinct, all matching `00_MASTER_PROTOCOL.md` §7 — S/60k/1h/0, M/200k/4h/6, L/500k/2d/12, XL/1.2M/5d/20 |
| Sum of `max_builds` | 1,278 |
| Sum of `token_envelope` | 55.18M |
| Sum of `wallclock_envelope` | 201.0 working days (4,280 h at a 24 h day) |
| Items carrying a `critic_gate` | 53 — D1 8, D2 12, D3 5, D4 15, D5 22, D6 24, D7 2 |
| Items citing a `capture_recipe` | 53, over 33 distinct paths |

**Unresolved dependency IDs: none.** Every ID named in any `depends_on` field across the nine packs
resolves to an item that exists in one of them.

**Three conditions the graph expresses, checked rather than assumed:**

- **Every recipe-citing item reaches both `G1.09` and `G1.12`.** Fifty-two carry a direct `G1.12`
  edge; `G7.45` inherits both through `G1.13`. HARNESS-RECIPE-GATE (§8.2) is the backstop, not the
  mechanism.
- **Every build-consuming item reaches `G1.01`.** Thirteen name it directly; the other 118 inherit
  it. WORKSPACE-BUILD-GATE (§8.4) is likewise a backstop.
- **Every `blocks` entry has a reciprocal `depends_on`.** 319 of 342 pairs are stated directly on the
  blocked item and 23 are implied by a chain that already exists. This one has no backstop decree
  behind it and no check in `graph_check.py` yet (§8.7): it is true because it was derived and
  repaired, and it will stay true only while someone keeps deriving it.

**The third condition is the one to distrust.** The first two are enforced by a script on every run
and by a written decree if the script ever misses. The third is currently enforced by nothing, and
it is the condition whose failure is silent — an unmirrored `blocks` entry changes no count in this
table, breaks no gate, and produces a queue that looks well-formed while scheduling an item ahead of
something declared to precede it. Six of the seventeen repaired pairs did exactly that before this
revision.

Both conditions live in the packs' own front matter, and this file's §3 table is regenerated from
that front matter rather than maintained alongside it — which is why the third check above exists,
and why a disagreement between a row here and the block it describes is a defect in this file and
never in the pack.

**One count worth keeping straight.** A census that asks "which items carry a *direct* edge to
`G1.01`" gets 13; a census that asks "which items *reach* `G1.01`" gets all 131 that consume a build.
The gate is a reachability question, and an item is not exempt because its edge is inherited.
