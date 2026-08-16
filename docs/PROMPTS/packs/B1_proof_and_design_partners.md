# PACK B1 — Technical Proof + Enterprise Design Partners & ROI

**Owns:** turning capability into evidence, and evidence into referenceable customers.

Two halves, one ladder. The first half builds the *reproducible-proof standard* — what an outside
engineer needs to re-run a Eustress claim without talking to anyone: a classified claim ledger,
validation against analytic solutions and published reference data, a declared validity envelope for
the models we actually ship, a public validation report that regenerates itself, and interoperability
with the CAD/PLM stacks an enterprise already owns. **An integration that does not round-trip is not
an integration**, so every interoperability item here carries a numeric round-trip fidelity test.

The second half builds the *design-partner motion* — a shipped-capability register that makes it
mechanically impossible to pitch a spec-only feature, an unwired-button sweep of the one vertical we
will actually demo, a discovery instrument that finds a problem worth paying for, a pilot whose
success metric is pre-registered and hashed **before** the pilot starts, explicit gates for promoting
a simulated result to a physical build decision, a case-study format that cannot be filled in without
a stated counterfactual, and a referenceability packet.

**Workloads this pack feeds:** primary **W1** (Provable Quality) and **W3** (Trust & Verifiability),
with **W4** (Vertical Proof) evidence from the G6 half and **W2** (Revenue Rail) instruments — never
executed transactions, which are human-only — from `G6.34` onward.

**ITEM ZERO: `G1.30`.** Nothing else in this pack may start until the claim ledger exists. Every
other item's exit condition is stated against a claim that `G1.30` classified as `MEASURED`, `TARGET`,
`CONFIG_DEFAULT`, or `UNSUPPORTED`. You cannot prove a claim you have not written down, and the
current repository states numbers in at least four registers without distinguishing them.

**ID block.** This pack allocates `.30`–`.39` within each phase it touches (`G1`, `G2`, `G6`). Other
packs must not use that block in those phases.

## Dependency graph

| ID | Title | Tier | Workload | depends_on |
|---|---|---|---|---|
| `G1.30` | Claim ledger and classification gate — **ITEM ZERO** | S | W3 | — |
| `G1.31` | Reproducible-proof standard and report linter | S | W3 | `G1.30` |
| `G2.30` | Analytic-solution validation suite for kernel laws | M | W1 | `G1.31` |
| `G2.31` | Published-reference validation with cited provenance | M | W1 | `G2.30` |
| `G2.32` | V-Cell validity envelope and out-of-envelope guard | M | W3 | `G2.30` |
| `G2.33` | Public validation report that regenerates itself | M | W3 | `G2.30`, `G2.31`, `G2.32`, `G1.31`, `G1.12` |
| `G2.34` | STEP import wired, with geometric fidelity floors | L | W1 | `G1.31`, `G1.01` |
| `G2.35` | STEP export and full solid round-trip | L | W1 | `G2.34` |
| `G2.36` | USD interoperability record and minimal USDA round-trip | L | W3 | `G2.34` |
| `G2.37` | Unit and coordinate-frame conformance across all interchange paths | M | W3 | `G2.34`, `G2.36`, `G2.35` |
| `G6.30` | Shipped-capability register and demo gate | M | W4 | `G1.30`, `G1.01` |
| `G6.31` | Unwired-button sweep of the declared demo path | M | W4 | `G6.30`, `G1.12` |
| `G6.32` | Design-partner qualification scorecard with disqualifiers | S | W2 | `G6.30` |
| `G6.33` | Discovery instrument that extracts a quantified problem | S | W2 | `G6.32`, `G6.31` |
| `G6.34` | Pre-registered pilot design, hashed before first data | S | W2 | `G6.33`, `G2.33` |
| `G6.35` | Simulation-to-physical promotion gates | S | W4 | `G2.32`, `G6.34` |
| `G6.36` | Case-study format with mandatory counterfactual | S | W2 | `G6.34`, `G6.35` |
| `G6.37` | Referenceability and expansion packet | S | W2 | `G6.36` |

Ordering rationale: cheap diagnostics that cost no compile (`G1.30`, `G1.31`) establish what is being
claimed and what a proof must contain. The `G2.3x` block then proves the claims that survive, cheapest
first (pure-function analytic checks before dataset comparison before interoperability wiring). The
`G6.3x` block cannot begin its partner motion until `G6.30` has mechanically separated shipped
capability from declared capability, because a pilot pitched on a spec-only capability loses a design
partner permanently.

---

## Cross-pack file ownership

Items in this pack share source files with items in other packs. `docs/PROMPTS/04_FILE_OWNERSHIP.md` names one owner per contested path and is normative; where it conflicts with an item's scope list, it wins. This pack's own dependency-graph table records its internal edges together with the program-gate edges of `02_QUEUE.md` §8.2 and §8.4; the cross-pack edges arising from contested paths are these:

| Item | Now depends on | Contested path | Effect on this item's scope |
|---|---|---|---|
| `G2.30` | `G2.02` (T2) | `eustress/crates/common/Cargo.toml` | may append to but not alter it |
| `G2.32` | `G2.08` (T2) | `eustress/crates/engine/src/simulation/electrochemistry.rs` | may no longer edit it |
| `G6.31` | `G6.04` (T3) | `eustress/crates/engine/src/ui/` | may not restructure it |

An item blocked by one of these entries emits a `FILE-OWNERSHIP` decision packet to L0 (`docs/PROMPTS/04_FILE_OWNERSHIP.md` §6). It does not edit the file and does not work around it.

---


---

## `G1.30` — Claim ledger and classification gate  ·  **ITEM ZERO**

````markdown
---
id: G1.30
title: Every customer-facing quantitative claim classified and gated
workload: W3
workload_secondary: [W1]
phase: G1
depends_on: []
blocks: [G1.31, G2.30, G6.30]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G1.30/claim_ledger.json
escalation: >
  If more than 40% of extracted claims resolve to UNSUPPORTED, stop after completing the ledger and
  emit a STALL packet rather than attempting any remediation — remediation is other items' work and
  the ledger's value is that it is complete, not that it is flattering.
status: DRAFT
notes: >
  Item zero for pack B1. Tier S: pure extraction and classification over Markdown and HTML, zero
  cargo builds. Python 3.13 is available on the authoring machine (`python --version` returns
  3.13.14) and `scripts/*.py` is an established repository convention.
---

## 1. Objective

A single machine-readable ledger exists that lists every quantitative or capability claim Eustress
makes in customer-facing material, and assigns each claim exactly one class: `MEASURED`, `TARGET`,
`CONFIG_DEFAULT`, or `UNSUPPORTED`. A checker refuses to exit 0 while any extracted claim is
unclassified or while a `MEASURED` claim lacks a reproduction command. After this item, no other item
in this pack argues about what Eustress claims — it looks the claim up.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — a world model an AI reasons
over and a document a human edits. It is never described as a game engine; 3D rendering and the ECS
are implementation details in service of that goal. The licence is PolyForm Shield 1.0.0
(`LICENSE` at the repository root); the correct phrase is **source-available**, never open source.
Physics is Avian, never Rapier. Units are meter-native; studs are a display unit only.

**Why this item exists.** The repository states numbers in at least four different registers without
distinguishing them, and at least one live partner-facing asset states something the licence
contradicts. Verified examples you will encounter:

- `docs/AUDIT/05_SPACE_STREAMING.md` line 23 cites a 2.10M-entity figure. That number is an
  `active_cap` **configuration default**, not a measurement. It must classify as `CONFIG_DEFAULT`.
- `docs/development/BENCHMARK_VS_ENGINE_AUDIT.md` reports a benchmark at 8K entities and an engine
  at 10K entities with a stated frame-rate gap. Those are measurements and must classify as
  `MEASURED` **only if** you can name the command that reproduces them; otherwise `UNSUPPORTED`.
- `docs/marketing/UofA_Center_For_Innovation_Pilot.html` advertises "Open source · MIT-friendly
  licensing · No vendor lock-in". `LICENSE` is PolyForm Shield 1.0.0, whose Noncompete clause bars
  "providing any product that competes with the software", including free products. That claim
  classifies as `UNSUPPORTED` and is flagged `licence_conflict: true`.
- `docs/architecture/EUSTRESS_FORGE.md` states an 80–90% cost reduction. No measurement backs it in
  the tree; classify `UNSUPPORTED` unless you find and cite the measurement.
- `docs/architecture/GOVERNMENT_MODE.md` §9 states that of 562 declared government tool ids exactly
  one, `data:import`, has a real handler, and that the mode "must never be described as working
  software." The 562 figure is `MEASURED` against `eustress/crates/engine/modes/government.toml`;
  any claim that the twelve disciplines *work* is `UNSUPPORTED`.
- `docs/monetization/SUBSCRIPTIONS.md` prices are `TARGET` (the document is headed
  "Status: Pre-Release Design").

**Files you must scan.** All of these exist and have been verified:
`README.md`, `LAUNCH_PLAN.md`, `RELEASE.md`, `START.md`, everything under `docs/marketing/`
(`UofA_Center_For_Innovation_Pilot.html` is the live partner-facing asset), everything under
`docs/architecture/`, everything under `docs/development/`, and `docs/AUDIT/MASTER.md`.

**One file is corrupt and must be reported, not parsed.** `docs/architecture/USD_NATIVE_FORMAT.md`
is 1336 lines, begins with the bytes `//! # Sou` (Rust doc-comment source, not Markdown), and
contains 5674 NUL bytes. It is unreadable as a document. Record it in the ledger's
`unreadable_sources` array with its byte-length and NUL count. Do not attempt to repair it — that is
`G2.36`'s work.

**Definition of a claim.** A sentence or table cell that asserts (a) a number about performance,
scale, cost, throughput, accuracy, timing, or money, or (b) that a named capability exists, is
supported, is complete, or is compliant. Marketing adjectives with no number and no capability
assertion ("fast", "elegant") are not claims and must not be extracted.

**Class definitions — apply exactly these.**
- `MEASURED` — a number produced by running something, where you can name the exact command, the
  hardware, and the artifact path that holds the result. No command, no `MEASURED`.
- `TARGET` — a number the project intends to hit. Includes anything in a document headed with a
  design/pre-release/proposal status.
- `CONFIG_DEFAULT` — a number that is the default value of a configuration field, cited as though it
  were an outcome. Record the `path:line` of the field definition.
- `UNSUPPORTED` — everything else, including capability claims contradicted by the audit ledger in
  `docs/AUDIT/`, and any number whose origin you cannot establish.

**Build reality.** This item compiles nothing. `max_builds` is 0. If you find yourself running
cargo, you have left the item.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/G1.30/` — create; holds `claim_ledger.json` and `claim_ledger.md`
- `docs/PROMPTS/harness/checkers/claim_ledger_check.py` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- Every file you scan. This item is read-only over the corpus. **Do not fix a single claim.**
  Correcting `docs/marketing/UofA_Center_For_Innovation_Pilot.html` is a human decision about a
  live partner-facing asset and is explicitly forbidden here.
- `docs/architecture/USD_NATIVE_FORMAT.md` — report, do not repair

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Narrowing the file list, dropping a document because it is embarrassing, redefining "claim" to
  exclude the awkward cases, or classifying an unsourced number as `MEASURED` because it is probably
  right are all measurement changes. If the definition of a claim is genuinely unworkable, report
  `EXIT_CRITERION_UNMEASURABLE` with three concrete examples and stop.
- Do not invent a reproduction command. If you cannot run it, the claim is not `MEASURED`.
- Do not editorialise inside the ledger. Each record carries facts and a class, not an opinion about
  whether the claim was reasonable to make.
- Every record must cite `source_path` and `source_line`. A record without a line number is invalid.

## 5. Exit criterion

### Criterion
`docs/PROMPTS/artifacts/G1.30/claim_ledger.json` contains **at least 60** claim records; **100%** of
records carry a class from the four-value enum; **100%** of `MEASURED` records carry a non-empty
`reproduction_command` and a `reproduction_artifact_path`; and the checker exits 0.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/claim_ledger_check.py \
        --ledger docs/PROMPTS/artifacts/G1.30/claim_ledger.json \
        --min-claims 60 \
        --require-repro-for MEASURED \
        --print-summary
    echo "EXIT=$?"

Expected output shape:

    total_claims=74
    by_class MEASURED=9 TARGET=21 CONFIG_DEFAULT=6 UNSUPPORTED=38
    unclassified=0
    measured_missing_repro=0
    records_missing_source_line=0
    unreadable_sources=1
    EXIT=0

Pass condition:

    EXIT=0  AND  total_claims >= 60  AND  unclassified == 0
    AND measured_missing_repro == 0  AND  records_missing_source_line == 0

The checker must be written so that it exits 2 on any violation. Verify by reading the printed
counters and the `EXIT=` line — not by observing that the JSON file appeared.

### Negative control (required)

    python docs/PROMPTS/harness/checkers/claim_ledger_check.py \
        --ledger docs/PROMPTS/harness/checkers/fixtures/claim_ledger_bad.json \
        --min-claims 1 --require-repro-for MEASURED
    echo "EXIT=$?"

`fixtures/claim_ledger_bad.json` is a two-record fixture you author: one record with class
`MEASURED` and no `reproduction_command`, one with no `source_line`. Pass condition: `EXIT=2`. A
checker that cannot fail is not a checker.

## 6. Critic gate

`critic_gate` is `[]`. This item produces no perceptual artifact — its output is a structured ledger
whose correctness is fully decided by the checker plus the negative control in §5. The mechanical
criterion is unusually tight in compensation: minimum record count, total classification coverage,
mandatory `path:line` provenance on every record, mandatory reproduction command on every `MEASURED`
record, and a required demonstration that the checker rejects malformed input.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — manual + regex-assisted extraction over the named corpus
   -> if still failing, MANDATORY approach change. Widening the regex is NOT an approach change;
      switching from pattern extraction to a per-document structured pass is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with total_claims moving < 5% and unclassified > 0
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: > 40% of claims resolve to UNSUPPORTED (see front matter) — finish the ledger,
                   then stall for a human decision on remediation sequencing
```

The stall packet must fit one screen and request exactly one of: `LOWER the floor to <x'>`,
`FUND approach D`, `DEFER behind <id>`, or `KILL`, with a recommendation.

## 8. Artifact

`docs/PROMPTS/artifacts/G1.30/claim_ledger.json`

A reader finds an object with `generated_at`, `corpus` (the list of scanned paths), `records`, and
`unreadable_sources`. Each record has: `id`, `claim_text` (verbatim, ≤ 200 chars), `source_path`,
`source_line`, `class`, `number`, `unit`, `reproduction_command` (nullable), 
`reproduction_artifact_path` (nullable), `config_field_path` (nullable, for `CONFIG_DEFAULT`),
`audit_reference` (nullable, e.g. `docs/AUDIT/09_ECONOMY.md`), and `licence_conflict` (boolean).

A companion `claim_ledger.md` renders the same data as a table grouped by class, for humans. The
JSON is authoritative; the Markdown is generated from it and must not be hand-edited.

## 9. Definition of NOT done

- The ledger has 60+ records but omits `docs/marketing/UofA_Center_For_Innovation_Pilot.html`
  because its claims were uncomfortable. The live partner-facing asset is the single most important
  document in the corpus.
- Numbers are classified but capability claims are not. "STEP/IGES import/export" appears in
  `docs/AUDIT/18_CAD_MESHGEOMETRY.md` feature 10 as 🔴 — a capability claim elsewhere that it works
  is a claim and must be recorded.
- A `MEASURED` class is assigned because the number looks like it came from a benchmark, with
  `reproduction_command` filled in speculatively. A command you did not verify exists is a
  fabrication.
- The checker exits 0 on the real ledger but was never demonstrated to exit 2 on malformed input.
  Without the negative control there is no evidence the gate works.
- `USD_NATIVE_FORMAT.md` was silently skipped rather than recorded in `unreadable_sources`. A
  corrupt document in the architecture directory is itself a finding.
- The item "fixed" one of the claims it found. Read-only means read-only; a corrected corpus makes
  the ledger unreproducible against the commit it was generated from.
````

---

## `G1.31` — Reproducible-proof standard and report linter

````markdown
---
id: G1.31
title: A proof standard an outside engineer can execute without contacting anyone
workload: W3
workload_secondary: [W1]
phase: G1
depends_on: [G1.30]
blocks: [G2.30, G2.33, G2.34]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/validation/PROOF_STANDARD.md
escalation: >
  If the standard cannot be satisfied by any claim currently classified MEASURED in
  docs/PROMPTS/artifacts/G1.30/claim_ledger.json, STALL — a standard that nothing in the repository
  can meet is either wrong or the repository has no measured claims, and a human must decide which.
status: DRAFT
notes: >
  Tier S: specification plus a Python linter, no compile. The standard is normative for every
  validation report produced by items G2.30 through G2.37.
---

## 1. Objective

`docs/validation/PROOF_STANDARD.md` defines, normatively, what a Eustress claim must carry to be
re-runnable by an engineer who has never spoken to anyone at Eustress: environment pinning, an exact
command, a declared tolerance, a declared uncertainty source, and a content hash of the result. A
linter enforces the standard mechanically over any validation report, and is demonstrated to accept
a conforming report and reject a non-conforming one.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Licence
is PolyForm Shield 1.0.0 — say **source-available**. Physics is Avian. Slint compiles to Rust, so
"Rust-first" is never framed as opposed to Slint. Units are meter-native.

**What already exists that you must build on.** `G1.30` produced
`docs/PROMPTS/artifacts/G1.30/claim_ledger.json`, in which every claim carries a class of `MEASURED`,
`TARGET`, `CONFIG_DEFAULT`, or `UNSUPPORTED`, and every `MEASURED` claim carries a
`reproduction_command`. This item defines what makes that reproduction command *sufficient*.

**The reproducibility problem is real and specific in this repository.**
- `.github/workflows/ci.yml` has three jobs: a `cargo deny` advisories/bans/sources check, naga WGSL
  validation that skips any shader carrying naga_oil directives, and a `cargo tree -p eustress-engine`
  grep asserting `eustress-data` is present. `.github/workflows/linux-engine.yml` runs one step,
  `cargo check --package eustress-engine`. There is **no `cargo test`** in CI anywhere, while
  approximately 2,061 `#[test]` functions exist across `eustress/crates/`. An outside engineer
  therefore has no green signal to trust and must run things themselves.
- The desktop engine is not built in CI at all, so the founder is the entire regression surface and
  each verification cycle costs a 10–15 minute serialized build.
- `eustress/crates/common/tests/determinism.rs` exists and is gated `#![cfg(feature = "physics")]`.
  It builds a minimal Avian world with the engine's determinism pins (`Time::<Fixed>::from_hz(60.0)`,
  `SubstepCount(6)`, `SolverConfig::default()`, `Gravity(Vec3::NEG_Y * 9.80665)`), steps
  `FixedUpdate` a fixed number of times, and hashes the end state. It is the closest thing in the
  tree to a conforming proof and is your worked reference — but note that it requires a non-default
  feature to compile, which is exactly the kind of detail the standard must force a report to state.

**Environment facts an outside engineer needs and cannot guess.** The Rust toolchain is pinned by
`eustress/rust-toolchain.toml`. The workspace root for cargo commands is `eustress/`. The workspace
shares a single `target/` directory, so only one cargo build may run at a time; concurrent builds
produce link failures. Validation is done with `cargo run`, not `cargo check` — `cargo check` does
not catch plugin-registration failures. Never kill a build mid-compile.

**What the standard is not.** It is not a style guide and not a template for prose. It is a list of
required fields plus the rule that each field is machine-checkable.

## 3. Scope

### In scope — files this item may edit
- `docs/validation/PROOF_STANDARD.md` — create
- `docs/validation/reports/EXAMPLE_conforming.md` — create (the worked reference)
- `docs/PROMPTS/harness/checkers/report_lint.py` — create
- `docs/PROMPTS/harness/checkers/fixtures/report_nonconforming.md` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G1.30/claim_ledger.json` — it is an input, frozen
- Any file under `eustress/crates/` — this item writes no Rust

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Making a required field optional so the example passes, or relaxing the linter so a
  non-conforming report squeaks through, are measurement changes. If a required field is genuinely
  unsatisfiable, report `EXIT_CRITERION_UNMEASURABLE` naming the field and stop.
- The standard must be satisfiable *today* by at least one existing claim. Write the conforming
  example against `eustress/crates/common/tests/determinism.rs`, not against a hypothetical future
  measurement.
- Do not write a standard that requires infrastructure the project does not have. There is no
  Prometheus, no Grafana, no multi-region deployment, no SOC 2 posture, and no signed binaries
  (Windows authenticode and macOS notarisation are both at 0%). A standard that presumes any of
  those is unusable and fails this item.
- Every required field must be checkable by a program with no network access.

## 5. Exit criterion

### Criterion
`report_lint.py` accepts `docs/validation/reports/EXAMPLE_conforming.md` with **exit 0** and rejects
`docs/PROMPTS/harness/checkers/fixtures/report_nonconforming.md` with **exit 2**, naming **at least 3**
distinct violated required fields; and `PROOF_STANDARD.md` declares **at least 8** required fields,
each with a stated machine check.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/report_lint.py \
        --standard docs/validation/PROOF_STANDARD.md \
        --report docs/validation/reports/EXAMPLE_conforming.md
    echo "GOOD_EXIT=$?"

    python docs/PROMPTS/harness/checkers/report_lint.py \
        --standard docs/validation/PROOF_STANDARD.md \
        --report docs/PROMPTS/harness/checkers/fixtures/report_nonconforming.md
    echo "BAD_EXIT=$?"

    python docs/PROMPTS/harness/checkers/report_lint.py \
        --standard docs/validation/PROOF_STANDARD.md --list-required-fields | wc -l

Expected output shape:

    required_fields=9 violations=0
    GOOD_EXIT=0
    required_fields=9 violations=4
      missing: environment.toolchain
      missing: measurement.command
      missing: result.tolerance
      missing: result.content_hash
    BAD_EXIT=2
    9

Pass condition:

    GOOD_EXIT == 0  AND  BAD_EXIT == 2  AND  violations_on_bad >= 3
    AND required_field_count >= 8

Read the printed counters and both exit codes. Do not infer success from the files existing.

## 6. Critic gate

`critic_gate` is `[]`. The artifact is a specification and a linter; conformance is decided entirely
by the two-sided exit-code test in §5 (accept the conforming report, reject the non-conforming one
for at least three distinct reasons). The tightening compensations are: a minimum required-field
count, the demand that the conforming example be built against a real existing test rather than a
hypothetical, and the requirement that every field be checkable offline.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — field list derived from what determinism.rs already needs to be re-run
   -> if still failing, MANDATORY approach change. Adding one more field is NOT an approach change;
      restructuring the standard around a machine-readable front-matter block is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where BAD_EXIT != 2
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: no claim in the G1.30 ledger can satisfy the standard (see front matter)
```

## 8. Artifact

`docs/validation/PROOF_STANDARD.md`

A reader finds: the required-field table (field name, meaning, machine check, example value); the
rule that every number is labelled `MEASURED`, `TARGET`, or `CONFIG_DEFAULT` inline; the environment
pinning requirements (toolchain from `eustress/rust-toolchain.toml`, OS and version, CPU, GPU and
driver where the measurement touches the GPU, cargo features enabled); the tolerance and uncertainty
declaration rule; the content-hash rule for result artifacts; and the explicit statement that a claim
whose command cannot be run by a stranger is not a claim, it is an assertion.

Alongside it, `docs/validation/reports/EXAMPLE_conforming.md` is the worked reference, built against
`eustress/crates/common/tests/determinism.rs`, and is what `G2.33` will pattern the public report on.

## 9. Definition of NOT done

- The standard is beautiful and nothing in the repository can satisfy it. A standard with zero
  conforming instances has proven nothing and blocks every downstream item.
- The linter passes the conforming example because the example was written to the linter rather than
  to the standard, and the standard says something the linter never checks. Every required field in
  the document must map to a check, and the `--list-required-fields` count must match the document.
- The non-conforming fixture fails for one reason only. Three distinct violations are required
  because a linter that short-circuits on the first error cannot be trusted to find the second.
- The standard requires a signed binary, a green CI badge, or a hosted dashboard. None of those
  exist: CI runs no tests, Windows authenticode is at 0%, macOS notarisation is at 0%.
- The standard omits cargo features. `determinism.rs` is `#![cfg(feature = "physics")]` and silently
  compiles to nothing without it — a report that omits the feature list is not reproducible, and this
  is the most likely real-world failure for an outside engineer.
- The word "open source" appears anywhere in the standard. The licence is PolyForm Shield 1.0.0 and
  the correct term is source-available.
````

---

## `G2.30` — Analytic-solution validation suite for kernel laws

````markdown
---
id: G2.30
title: Kernel laws validated against closed-form analytic solutions with declared tolerances
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G1.31, G2.02]
blocks: [G2.31, G2.32, G2.33]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.30/analytic_validation.json
escalation: >
  If any law deviates from its closed-form solution by more than 10x its declared tolerance, do not
  widen the tolerance — record the deviation, mark that law FAILED in the emitted JSON, and STALL
  with the law named. A wrong law is a finding, not an obstacle.
status: DRAFT
notes: >
  Tier M: one new integration test file in an existing crate, run repeatedly. Six builds is generous
  because `cargo test -p eustress-common` does not rebuild the engine binary.
---

## 1. Objective

An integration test validates at least 12 Eustress kernel-law functions against their closed-form
analytic solutions, each with a tolerance declared in the test rather than inferred from the result,
and emits a machine-readable record of every comparison. The suite fails loudly when a law drifts,
and its JSON output is the primary numerical evidence for the public validation report.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
"gold-collar unlock" is that engineers can rewrite kernel laws to validate their own models — which
means the laws we ship must themselves be validated, or the unlock is a liability.

**Where the laws live — all paths verified.**
`eustress/crates/common/src/realism/laws/` contains `thermodynamics.rs`, `electrochemistry.rs`,
`mechanics.rs`, `conservation.rs`, and the subdirectories `acoustics/`, `biology/`,
`electromagnetism/`, `kinetics/`, `optics/`. The crate package name is `eustress-common`.

Concrete function signatures you can use, verified present:

- `laws/thermodynamics.rs:31` `pub fn ideal_gas_pressure(n: f32, t: f32, v: f32) -> f32`
- `laws/thermodynamics.rs:40` `pub fn ideal_gas_volume(n: f32, t: f32, p: f32) -> f32`
- `laws/thermodynamics.rs:129` `pub fn work_isothermal(n: f32, t: f32, v1: f32, v2: f32) -> f32`
- `laws/thermodynamics.rs:218` `pub fn carnot_efficiency(t_cold: f32, t_hot: f32) -> f32`
- `laws/thermodynamics.rs:227` `pub fn cop_refrigerator(t_cold: f32, t_hot: f32) -> f32`
- `laws/thermodynamics.rs:260` `pub fn heat_conduction_rate(k: f32, area: f32, delta_temp: f32, thickness: f32) -> f32`
- `laws/thermodynamics.rs:292` `pub fn heat_radiation_rate(emissivity: f32, area: f32, t_surface: f32, t_environment: f32) -> f32`
- `laws/electrochemistry.rs:31` `pub fn nernst_potential(e_standard: f32, n: f32, temperature: f32, activity_ratio: f32) -> f32`
- `laws/electrochemistry.rs:41` `pub fn thermal_voltage(temperature: f32) -> f32`
- `laws/electrochemistry.rs:60` `pub fn butler_volmer_symmetric(j0: f32, eta: f32, temperature: f32) -> f32`
- `laws/electrochemistry.rs:67` `pub fn tafel_overpotential(j: f32, j0: f32, alpha: f32, temperature: f32) -> f32`
- `laws/electrochemistry.rs:90` `pub fn electrolyte_asr(thickness: f32, ionic_conductivity: f32) -> f32`
- `laws/electrochemistry.rs:132` `pub fn arrhenius_conductivity(sigma0: f32, e_act: f32, temperature: f32) -> f32`
- `laws/electrochemistry.rs:295` `pub fn sands_time(diffusivity: f32, concentration: f32, current_density: f32) -> f32`

There are approximately 127 `#[test]` functions already under `laws/`. Those are unit tests written
by the implementer. **They are not validation** — a test that asserts a function returns what the
same author thought it should return proves internal consistency, not correctness. This item is
different: each case must state the closed-form expression independently, in the test, in symbols,
and compare against it.

**Feature gating.** `eustress/crates/common/Cargo.toml` declares
`default = ["model-import", "geotiff", "streaming", "units_v1"]`, `physics = ["avian3d"]`, and
`realism = ["physics"]`. Determine whether the laws you exercise compile under default features; if
they require `realism`, the test must be run with `--features realism` and the report must say so.

**Precision reality.** These functions are `f32`. Do not declare a relative tolerance tighter than
`1e-6` for any single operation, or tighter than `1e-4` for a composed expression involving `ln`,
`exp`, or `sinh`. Declaring an unachievable tolerance and then loosening it after seeing the result
is exactly the failure this item exists to prevent — declare the tolerance and its justification
*before* running.

**Existing precedent for the file shape.** `eustress/crates/common/tests/determinism.rs` is the
existing integration test in this crate and shows the conventions (feature gate at the top, plain
`#[test]` functions, no harness dependencies). Follow it.

**Build reality.** Builds are 10–15 minutes for the engine; `cargo test -p eustress-common` is much
cheaper but still serialized against the shared `eustress/target/` directory. Only one cargo
invocation at a time. Never kill a build mid-compile. Batch changes so one run validates several
cases.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/tests/analytic_validation.rs` — create
- `eustress/crates/common/Cargo.toml` — only to add a `[[test]]` entry or a dev-dependency, and only
  if genuinely required; state why in the result block
- `docs/PROMPTS/artifacts/G2.30/` — create; holds the emitted JSON

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/realism/laws/**` — **you may not change a law to make a test pass.**
  If a law is wrong, that is the finding; record it and escalate.
- `docs/validation/PROOF_STANDARD.md` — it is an input, frozen
- Any existing entry in `eustress/crates/common/Cargo.toml` — the file is owned by `G2.02`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Loosening a declared tolerance after seeing a failure, dropping a law from the suite because it
  deviates, choosing input values in a regime where the error happens to vanish, or asserting against
  the function's own output are all measurement changes. If a tolerance is genuinely wrong for
  physical reasons, say so in the record's `tolerance_justification` **before** running and keep the
  original in `tolerance_original`.
- Each case must include at least one input set away from the trivial regime. `carnot_efficiency`
  compared only at `t_cold == t_hot` proves nothing.
- The closed-form expected value must be computed in the test from first principles (constants,
  `f64` arithmetic, then compared to the `f32` result), never hard-coded from a previous run of the
  function under test.
- Physical constants must come from `eustress/crates/common/src/realism/constants.rs` where they
  exist there, so a constant drift is caught rather than masked by a locally re-declared literal.
- You may not modify a law. Not one line.

## 5. Exit criterion

### Criterion
The suite covers **at least 12** distinct law functions across **at least 3** distinct law modules,
every case declares its tolerance before comparison, **100%** of cases pass within their declared
tolerance, and the emitted JSON records every case with its inputs, expected value, actual value,
absolute error, relative error, and declared tolerance.

### Measurement

Command (run from the `eustress/` workspace root):

    cargo test -p eustress-common --features realism --test analytic_validation -- --nocapture
    echo "EXIT=$?"

    python docs/PROMPTS/harness/checkers/analytic_check.py \
        --json docs/PROMPTS/artifacts/G2.30/analytic_validation.json \
        --min-cases 12 --min-modules 3
    echo "CHECK_EXIT=$?"

Expected output shape (from the checker):

    cases=17 modules=4 passed=17 failed=0
    tolerance_declared_before_run=17
    max_relative_error=3.7e-06 (electrochemistry::butler_volmer_symmetric)
    CHECK_EXIT=0

Pass condition:

    EXIT == 0  AND  CHECK_EXIT == 0  AND  cases >= 12  AND  modules >= 3
    AND failed == 0  AND  tolerance_declared_before_run == cases

The test writes `docs/PROMPTS/artifacts/G2.30/analytic_validation.json` as a side effect of running;
the pass is decided by the two exit codes and the printed counters, never by the file's existence.
If `--features realism` turns out to be unnecessary, run without it and record the exact command used
in the JSON's `command` field — the command in the artifact must be the command that was run.

## 6. Critic gate

`critic_gate` is `[]`. Every proposition in this item is a number with a declared tolerance, decided
by two exit codes. There is nothing perceptual to judge. The compensating tightness: minimum case
count, minimum module spread, mandatory pre-declared tolerances, a hard prohibition on editing the
laws, and the requirement that expected values be derived in-test rather than captured from the
function under test.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — direct closed-form comparison per law function
   -> if still failing, MANDATORY approach change. Swapping input values is NOT an approach change;
      moving to a property-based check (e.g. thermodynamic identities that must hold across a
      parameter sweep) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with `passed` moving < 1 case
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: any law deviating > 10x its declared tolerance (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.30/analytic_validation.json`

A reader finds: `command` (the exact cargo invocation, including features), `toolchain` (from
`eustress/rust-toolchain.toml`), `commit`, `os`, and a `cases` array. Each case carries `law_module`,
`law_function`, `source_path_line`, `inputs`, `closed_form_expression` (the symbolic form, as a
string), `expected` (computed in `f64`), `actual` (the `f32` result), `abs_error`, `rel_error`,
`tolerance`, `tolerance_kind` (`absolute` | `relative`), `tolerance_justification`, and
`status` (`PASS` | `FAIL`).

## 9. Definition of NOT done

- Twelve cases exist but eleven are in `thermodynamics.rs`. Three distinct modules is a floor because
  a suite concentrated in one file validates one author's afternoon, not the kernel.
- A case passes because `expected` was copied from a prior run of the function under test. That is a
  regression test, not a validation, and it will pass forever no matter how wrong the law is.
- A tolerance was widened after the first failing run. The JSON must show `tolerance` equal to
  `tolerance_original` for every case, or carry a physical justification written before the run.
- A law was edited to make a case pass. The laws are out of scope; a deviating law is the finding.
- Every case sits in a degenerate regime — zero overpotential, equal reservoir temperatures, unit
  activity ratio — where most of these functions collapse to a constant.
- The suite passes but only compiles under a feature the report never mentions, so an outside
  engineer running the documented command gets zero tests executed and reads that as success.
  `cargo test` prints `0 passed` and exits 0 when a `#![cfg(feature = ...)]` file compiles to nothing.
  The checker must therefore assert `cases >= 12` from the JSON, and the JSON must be regenerated by
  the run, not stale.
````

---

## `G2.31` — Published-reference validation with cited provenance

````markdown
---
id: G2.31
title: Kernel laws checked against published reference values with DOI-level provenance
workload: W1
workload_secondary: [W3]
phase: G2
depends_on: [G2.30]
blocks: [G2.33]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.31/reference_validation.json
escalation: >
  If a required reference dataset cannot be redistributed under a licence compatible with this
  repository, do NOT vendor it. Record it by citation plus SHA-256 of the file the operator supplies
  and mark the case `provenance: external_required`. If that leaves fewer than 6 cases, STALL.
status: DRAFT
notes: >
  Tier M. The hard part is licensing and provenance discipline, not arithmetic. Redistribution of a
  third-party dataset into a PolyForm Shield repository is a human decision.
---

## 1. Objective

At least 6 Eustress kernel-law outputs are compared against values published by an independent,
citable source — not against our own closed-form re-derivation — with each reference carrying a full
citation, an access date, and a content hash. Deviations are reported as measured percentages against
declared acceptance bands, and the resulting JSON is admissible in the public validation report.

## 2. Context you need (self-contained)

**Why this is separate from `G2.30`.** `G2.30` proves our code agrees with the equation we believe.
This item probes whether the equation agrees with the world as someone else measured it. A design
partner in batteries or aerospace will ask the second question, not the first.

**What `G2.30` left you.** `docs/PROMPTS/artifacts/G2.30/analytic_validation.json`, with at least 12
validated law functions across at least 3 modules, each with declared tolerances. Reuse its case
structure; do not re-derive it.

**Candidate law surface, verified present in the tree.**
`eustress/crates/common/src/realism/laws/thermodynamics.rs` (ideal gas, van der Waals at line 80,
Carnot efficiency at 218, conduction at 260, radiation at 292, water phase at 325) and
`eustress/crates/common/src/realism/laws/electrochemistry.rs` (Nernst at 31, Butler–Volmer at 50 and
60, Tafel at 67, Arrhenius conductivity at 132, Nernst–Einstein diffusivity at 138, Sand's time at
295, Monroe–Newman critical current at 311). Package name is `eustress-common`.

**The licensing constraint is load-bearing.** This repository is licensed PolyForm Shield 1.0.0
(`LICENSE`). Vendoring a third-party dataset into it may violate that dataset's own terms and is a
**human decision** — you may not make it. The permitted pattern is: cite the source precisely
(publisher, title, edition or version, table or figure identifier, DOI or stable URL, access date),
record the specific numeric values you compared against inline in the JSON as `reference_value` with
their units, and record a SHA-256 of any file an operator supplies locally without committing that
file. Quoting a handful of numeric values with attribution for verification purposes is what this
item does; wholesale reproduction of a table is not.

**Honest state of the model you will most want to validate.** `docs/AUDIT/19_REALISM_PHYSICS.md`
records, verbatim: V-Cell `Nernst + Butler-Volmer` are **lumped 0-D models** (no spatial
electrochemistry / ion transport) — a validation gap versus real cells; and the particle ECS
`ElectrochemicalState` and `ThermodynamicState` are **decoupled** (no thermal-effect-on-reaction-rate
coupling). This means a full-cell discharge-curve comparison against published cell data **will**
deviate, and that deviation is the honest result. Do not chase it by tuning. Report it, and let
`G2.32` turn it into a declared validity envelope.

**Build reality.** `cargo test -p eustress-common` is serialized against the shared `eustress/target/`
directory. One cargo invocation at a time. Never kill a build mid-compile. Six builds is the budget.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/common/tests/reference_validation.rs` — create
- `eustress/crates/common/tests/data/reference_cases.toml` — create; the reference values and their
  citations, hand-entered, no third-party file vendored
- `docs/PROMPTS/artifacts/G2.31/` — create
- `docs/PROMPTS/harness/checkers/reference_check.py` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/realism/**` — no law may be edited or tuned
- `docs/PROMPTS/artifacts/G2.30/analytic_validation.json` — an input, frozen
- Any vendored third-party dataset file. Do not create one.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Selecting only the reference points where we agree, widening an acceptance band after seeing the
  deviation, or substituting a textbook worked example for a published measurement are measurement
  changes. Declare the band first; report the deviation second.
- A reference is admissible only with publisher, title, identifier, DOI or stable URL, and access
  date. "A standard handbook" is not a citation and fails the checker.
- Do not fabricate a citation. If you cannot verify a source exists, mark the case
  `provenance: external_required` and leave `reference_value` null rather than guessing.
- Do not tune any law, constant, or default to close a gap. The gap is the deliverable.
- Every case must state its acceptance band and the physical reason for that band (measurement
  uncertainty in the source, model simplification, `f32` precision), before the comparison.

## 5. Exit criterion

### Criterion
At least **6** reference cases across at least **2** law modules, **100%** carrying a complete
citation record, **100%** carrying an acceptance band declared before comparison, and every case
reporting a measured `deviation_pct`. Cases outside their band are permitted and expected — they must
be marked `status: OUT_OF_BAND` with a stated cause, not hidden.

### Measurement

Command (run from the `eustress/` workspace root):

    cargo test -p eustress-common --features realism --test reference_validation -- --nocapture
    echo "EXIT=$?"

    python docs/PROMPTS/harness/checkers/reference_check.py \
        --json docs/PROMPTS/artifacts/G2.31/reference_validation.json \
        --min-cases 6 --min-modules 2 --require-citation-fields \
        publisher,title,identifier,url_or_doi,access_date
    echo "CHECK_EXIT=$?"

Expected output shape:

    cases=8 modules=3
    citation_complete=8/8  band_declared_before_run=8/8
    in_band=6 out_of_band=2
    out_of_band_causes: lumped_0d_no_spatial_transport(2)
    CHECK_EXIT=0

Pass condition:

    EXIT == 0  AND  CHECK_EXIT == 0  AND  cases >= 6  AND  modules >= 2
    AND citation_complete == cases  AND  band_declared_before_run == cases
    AND every out_of_band case has a non-empty `cause`

Note the asymmetry deliberately: `out_of_band > 0` does **not** fail this item. A hidden or
unexplained out-of-band case does.

## 6. Critic gate

`critic_gate` is `[]`. The artifact is numeric with an exit-code gate. The compensating tightness:
mandatory five-field citation completeness on every case, bands declared before the run, an explicit
rule that deviations must be surfaced rather than tuned away, and a prohibition on vendoring or
fabricating source data.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — single-point reference comparisons per law
   -> if still failing, MANDATORY approach change. Swapping one reference for another is NOT an
      approach change; moving to a multi-point curve comparison with an RMS deviation metric is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with `citation_complete` unchanged and < cases
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: fewer than 6 admissible cases after licensing review (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.31/reference_validation.json`

A reader finds: `command`, `toolchain`, `commit`, and a `cases` array. Each case carries
`law_module`, `law_function`, `source_path_line`, `inputs`, `reference_value`, `reference_unit`,
`citation` (an object with `publisher`, `title`, `identifier`, `url_or_doi`, `access_date`,
`table_or_figure`), `reference_sha256` (nullable, for an operator-supplied local file),
`acceptance_band_pct`, `band_justification`, `computed_value`, `deviation_pct`, `status`
(`IN_BAND` | `OUT_OF_BAND` | `EXTERNAL_REQUIRED`), and `cause` (required when `OUT_OF_BAND`).

## 9. Definition of NOT done

- Six cases exist and all six agree perfectly. Given that
  `docs/AUDIT/19_REALISM_PHYSICS.md` records the V-Cell model as lumped 0-D with decoupled thermal
  state, perfect agreement across a real reference set is evidence of case selection, not of model
  quality. Expect and report deviation.
- A citation reads "NIST" or "standard tables" with no identifier, DOI, or access date. That is not
  provenance and the checker must reject it.
- A third-party dataset file was committed to the repository to make the test self-contained.
  Redistribution is a human licensing decision and is forbidden here.
- A law, a constant in `realism/constants.rs`, or a default parameter was adjusted to bring a case
  into band. That converts a validation into a curve fit.
- An out-of-band case was quietly dropped from the array between iterations. The case count may only
  grow; deletions must be justified in the result block and the checker should be given the prior
  count to compare against.
- The item reports deviations in absolute units only, so no one can tell whether 0.4 V is a rounding
  artifact or a model failure. `deviation_pct` is required on every case.
````

---

## `G2.32` — V-Cell validity envelope and out-of-envelope guard

````markdown
---
id: G2.32
title: Declared validity envelope for the lumped V-Cell model, enforced at run time
workload: W3
workload_secondary: [W1, W4]
phase: G2
depends_on: [G2.30, G2.08]
blocks: [G2.33, G6.35]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/validation/VCELL_VALIDITY_ENVELOPE.md
escalation: >
  If enforcing the envelope at run time requires editing files outside
  eustress/crates/engine/src/simulation/, STALL rather than widening scope — a guard that reaches
  into the law layer will be rejected on review and wastes the build budget.
status: DRAFT
notes: >
  Tier M. The document is the deliverable; the guard is what makes the document falsifiable. A
  validity envelope no code enforces is marketing.
---

## 1. Objective

The V-Cell electrochemical model carries a written, machine-readable validity envelope stating the
parameter ranges and physical regimes in which its outputs are meaningful, and the engine refuses to
report a V-Cell result silently when a simulation runs outside that envelope. A test proves the guard
fires: an in-envelope run produces no violation, a deliberately out-of-envelope run produces exactly
the expected violation record.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Its
central claim to engineers is that they can validate their own models inside it. That claim survives
only if the models Eustress ships state their own limits.

**The honest state of the V-Cell model — quote from `docs/AUDIT/19_REALISM_PHYSICS.md`:** V-Cell
`Nernst + Butler-Volmer` are **lumped 0-D models** (no spatial electrochemistry / ion transport);
particle ECS `ElectrochemicalState` and `ThermodynamicState` are **decoupled** (no
thermal-effect-on-reaction-rate coupling); `fracture_mesh.rs` exists but has **no integration path to
Avian** — visualisation only. The same audit lists Feature 5 (V-Cell electrochemistry) as ✅ and
Feature 10 (Symbolica symbolic solver) as 🔴, and records requirement R5.1 as "Simplified model —
lumped 1-D RC thermal, no spatial gradients."

**Where the code is — verified paths.**
- `eustress/crates/common/src/realism/laws/electrochemistry.rs` (436 lines) — the pure law functions:
  `nernst_potential` (line 31), `butler_volmer_current` (50), `butler_volmer_symmetric` (60),
  `tafel_overpotential` (67), `electrolyte_asr` (90), `terminal_voltage` (107),
  `arrhenius_conductivity` (132), `ionic_limiting_current` (277), `sands_time` (295),
  `monroe_newman_critical_current` (311), `dendrite_risk` (324).
- `eustress/crates/engine/src/simulation/electrochemistry.rs` (329 lines) — the Bevy integration.
  `ElectrochemistryPlugin` is declared at line 27; `echem::nernst_potential` is called at line 216.

**A second, compounding honesty problem you must account for in the envelope.** The simulation clock
at `eustress/crates/common/src/simulation/clock.rs` advances `simulation_time_s` by the full
compressed delta, but caps physics ticks at `max_ticks_per_frame` (default 10) and **zeroes the
accumulator on saturation** (lines 100–102). At high `time_scale` the clock therefore reports
compressed time while the law steps that would have covered it are discarded, with no error and no
counter. Any V-Cell claim of the form "N cycles simulated in M seconds" rests on this. The envelope
must declare a maximum `time_scale` at which V-Cell results are meaningful, and the guard must record
a violation when a V-Cell simulation runs above it.

**Related known defect, for context only — do not fix it here.** The Watchman alert cooldown is
wall-clock based rather than sim-time based, so at extreme compression it misses sub-30-second spikes
(`docs/AUDIT/11_SIMULATION_DEBUGGER.md`). Note it in the envelope's `known_interactions` section;
fixing it is another item.

**Build reality.** Touching `eustress/crates/engine/` means a 10–15 minute build. One cargo build at
a time; the workspace shares `eustress/target/`. Never kill a build mid-compile. Validate with
`cargo run`, not `cargo check` — `cargo check` will not catch a plugin-registration failure, which is
the most likely way a guard silently never runs. Six builds is the entire budget: design the guard so
one build validates both the in-envelope and out-of-envelope cases.

## 3. Scope

### In scope — files this item may edit
- `docs/validation/VCELL_VALIDITY_ENVELOPE.md` — create
- `docs/validation/vcell_envelope.json` — create; the machine-readable form the guard loads
- `eustress/crates/engine/tests/vcell_envelope_guard.rs` — create
- `docs/PROMPTS/artifacts/G2.32/` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/realism/**` — the laws are not changed by this item
- `eustress/crates/common/src/simulation/clock.rs` — the step-drop defect is instrumented by a
  different item; here you only *declare* the `time_scale` limit and detect crossing it
- `docs/architecture/VCELL_CASE_STUDY.md` — an input, frozen
- `eustress/crates/engine/src/simulation/electrochemistry.rs` — owned by `G2.08` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Widening the envelope so that no realistic run violates it, downgrading a violation to a debug log,
  or making the guard opt-in are measurement changes. If the envelope is genuinely unbounded in some
  dimension, say so explicitly with a physical justification rather than omitting the dimension.
- The envelope must be derived from the model's structure, not from convenience. A lumped 0-D model
  with no ion transport has a defensible upper bound on current density and a defensible statement
  about what it cannot predict (concentration gradients, local hot spots, dendrite initiation site).
  State those.
- The guard must not panic and must not stop a simulation. It records a structured violation and
  marks the result untrusted. An engineer must be able to run outside the envelope deliberately and
  see that the output is flagged.
- Do not add a dependency. Do not add a new crate.
- Batch your verification: one build should validate the guard's positive and negative paths.

## 5. Exit criterion

### Criterion
`docs/validation/vcell_envelope.json` declares **at least 6** bounded dimensions, each with `min`,
`max`, `unit`, and `basis`; and the guard test demonstrates **0** violations on the in-envelope
scenario and **exactly the expected violation set** on the out-of-envelope scenario, including the
`time_scale` dimension.

### Measurement

Command (run from the `eustress/` workspace root):

    cargo test -p eustress-engine --test vcell_envelope_guard -- --nocapture
    echo "EXIT=$?"

    python docs/PROMPTS/harness/checkers/envelope_check.py \
        --envelope docs/validation/vcell_envelope.json \
        --result docs/PROMPTS/artifacts/G2.32/guard_result.json \
        --min-dimensions 6 --require-dimension time_scale
    echo "CHECK_EXIT=$?"

Expected output shape:

    dimensions=7 (current_density, temperature, soc, c_rate, activity_ratio, cell_area, time_scale)
    in_envelope_scenario: violations=0 trusted=true
    out_of_envelope_scenario: violations=2 trusted=false
      violated: current_density (4200.0 A/m^2 > max 1500.0)
      violated: time_scale (100000.0 > max 1000.0)
    CHECK_EXIT=0

Pass condition:

    EXIT == 0  AND  CHECK_EXIT == 0  AND  dimensions >= 6
    AND in_envelope.violations == 0  AND  in_envelope.trusted == true
    AND out_of_envelope.violations == expected_set  AND  out_of_envelope.trusted == false
    AND `time_scale` is present among the declared dimensions

The out-of-envelope scenario is the load-bearing half. A guard that only ever reports zero violations
has not been shown to work.

## 6. Critic gate

`critic_gate` is `[]`. The document's content is judged by the machine-readable envelope it produces
and the two-sided guard test that enforces it. The compensating tightness: a minimum dimension count,
a specifically required dimension (`time_scale`, because that is where the loudest V-Cell claims
break), and a mandatory negative path proving the guard fires.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — envelope as a loaded JSON resource, checked in the electrochemistry
                 system each tick, emitting a violation record
   -> if still failing, MANDATORY approach change. Adding another dimension is NOT an approach
      change; moving the check to a run-scoped summary evaluated at simulation stop is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the out-of-envelope scenario still reports 0
                  violations
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the guard would require edits outside eustress/crates/engine/src/simulation/
                   (see front matter)
```

## 8. Artifact

`docs/validation/VCELL_VALIDITY_ENVELOPE.md`

A reader finds: what the V-Cell model computes (Nernst potential, Butler–Volmer kinetics, ohmic
losses, lumped thermal); what it explicitly does **not** resolve (spatial electrochemistry, ion
transport, concentration gradients, local hot spots, thermal feedback on reaction rate, dendrite
initiation site, mechanical fracture coupling to Avian); the bounded dimension table with basis for
each bound; the maximum `time_scale` at which results are meaningful and why; a `known_interactions`
section naming the clock step-drop behaviour and the wall-clock Watchman cooldown; and the statement
of what a user must do to obtain a trustworthy result.

The machine-readable twin is `docs/validation/vcell_envelope.json`, which the guard loads and
`envelope_check.py` validates. The document is written so that it reads as always having been
correct — no changelog residue, no commentary about prior beliefs.

## 9. Definition of NOT done

- The envelope is written and nothing loads it. A validity envelope no code enforces is marketing,
  and the next pilot will run outside it without anyone noticing.
- The guard exists but the out-of-envelope test was never written, so there is no evidence it fires.
- `time_scale` is omitted because it felt like a clock concern rather than an electrochemistry
  concern. The clock at `clock.rs:100-102` zeroes the accumulator on saturation, so `time_scale` is
  precisely the dimension in which V-Cell claims silently become fiction.
- The document lists what the model does but not what it cannot resolve. The "does not resolve" list
  is the half a design partner in batteries actually needs.
- The guard panics or aborts a simulation on violation. An engineer must be able to run outside the
  envelope on purpose; the requirement is that the result is *flagged*, not prevented.
- Bounds were chosen so that the existing demo scenario passes. Bounds come from the model's
  structure; if the demo violates them, that is a finding for `G6.31`, not a reason to move a bound.
````

---

## `G2.33` — Public validation report that regenerates itself

````markdown
---
id: G2.33
title: A public validation report a stranger can regenerate end to end with one command
workload: W3
workload_secondary: [W1]
phase: G2
depends_on: [G2.30, G2.31, G2.32, G1.31, G1.12]
blocks: [G6.34]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D5, D6]
capture_recipe: docs/PROMPTS/harness/recipes/B1_validation_report.json
artifact: docs/validation/reports/VALIDATION_REPORT_v1.md
escalation: >
  If regeneration on a clean clone requires any credential, network fetch, or non-committed input
  file, STALL — a report that only regenerates on the founder's machine is the exact failure this
  item exists to eliminate.
status: DRAFT
notes: >
  Tier M. Critic-gated on D5 and D6 because the report is the first artifact an outside engineer
  reads, and a report that is numerically correct but incoherent still loses the reader.
---

## 1. Objective

`docs/validation/reports/VALIDATION_REPORT_v1.md` states every currently-provable Eustress numerical
claim, each with its command, its tolerance, its provenance, and its result — and a single script
regenerates every number in it from a clean checkout. Running the script and diffing the regenerated
report against the committed one produces zero differences outside declared volatile fields.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Licence
is PolyForm Shield 1.0.0 — **source-available**, never open source. Physics is Avian. Units are
meter-native.

**What your inputs are, all produced by earlier items in this pack.**
- `docs/PROMPTS/artifacts/G1.30/claim_ledger.json` — every customer-facing claim, classified
  `MEASURED` / `TARGET` / `CONFIG_DEFAULT` / `UNSUPPORTED`.
- `docs/validation/PROOF_STANDARD.md` — the normative required-field list, with
  `docs/PROMPTS/harness/checkers/report_lint.py` enforcing it, and
  `docs/validation/reports/EXAMPLE_conforming.md` as the worked reference.
- `docs/PROMPTS/artifacts/G2.30/analytic_validation.json` — ≥12 laws vs closed form.
- `docs/PROMPTS/artifacts/G2.31/reference_validation.json` — ≥6 laws vs published references.
- `docs/validation/vcell_envelope.json` and `docs/validation/VCELL_VALIDITY_ENVELOPE.md`.

**A pre-existing measurement you may cite if and only if you can re-run it.**
`eustress/crates/common/tests/determinism.rs` (feature-gated `physics`) builds a minimal Avian world
with the engine's pins, steps `FixedUpdate` a fixed number of times, and hashes the end state; two
runs from the same `GlobalRngSeed` must produce byte-identical hashes. Determinism across platforms
has never been demonstrated — `docs/AUDIT/11_SIMULATION_DEBUGGER.md` Feature 8 records "Avian
deterministic step is single-run only; cross-platform untested", and the byte-identical gate written
at `docs/architecture/HEADLESS_RUNTIME.md` is not listed as passed. If you include determinism, it
must be scoped as single-platform and labelled so.

**Numbers you must NOT put in this report.** The 2.10M-entity figure is an `active_cap` configuration
default (`docs/AUDIT/05_SPACE_STREAMING.md` line 23), not a measurement. The Forge 80–90% cost
reduction is unmeasured. The "72 countries" KYC figure is spec-only — the Cloudflare Worker
`JURISDICTIONS` dict is empty (`docs/AUDIT/08_IDENTITY_TRUST.md`). The "~200 MCP tools" figure is
wrong; the verified counts are 79 tool descriptors in `eustress/crates/tools/src/` plus 24 bridge
tools in `eustress/crates/mcp-server/src/bridge_tools.rs` plus one hand-rolled tool, of which about
90 are exposed in a live session after mode filtering (`eustress/crates/tools/src/modes.rs`). If you
state a tool count, state it that way, and label it `MEASURED` with the command that counts it.

**Reproducibility constraints that shape the regeneration script.** The workspace root for cargo is
`eustress/`. The toolchain is pinned by `eustress/rust-toolchain.toml`. The workspace shares one
`target/`, so the script must run cargo invocations **serially**. Builds are 10–15 minutes for the
engine, so the script must not build the engine binary unless a number requires it; prefer
`cargo test -p <crate>`. CI runs no tests at all — `.github/workflows/ci.yml` has three jobs
(`cargo deny`, naga WGSL validation that skips naga_oil shaders, and a `cargo tree` grep) and
`linux-engine.yml` runs `cargo check --package eustress-engine` — so you may not point a reader at a
CI badge as evidence.

**Volatile fields.** Timestamps, wall-clock durations, and host identifiers legitimately differ
between runs. Declare them in a `volatile_fields` list at the top of the regeneration script so the
diff can exclude exactly those and nothing else. Excluding a *number* as volatile fails this item.

## 3. Scope

### In scope — files this item may edit
- `docs/validation/reports/VALIDATION_REPORT_v1.md` — create
- `docs/validation/regenerate.py` — create; the one-command regeneration entry point
- `docs/PROMPTS/harness/recipes/B1_validation_report.json` — create; the capture recipe naming the
  report and its inputs for the Critic bundle
- `docs/PROMPTS/artifacts/G2.33/` — create; holds `regeneration_diff.json`

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- All five input artifacts named in §2 — they are frozen inputs
- `eustress/crates/**` — this item runs measurements, it does not change what is measured

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Adding a number to `volatile_fields` so the diff passes, regenerating only a subset of sections,
  or reporting a cached artifact instead of re-running its command are measurement changes. If a
  number genuinely cannot be regenerated, remove the claim from the report — do not exempt it.
- The report must include a "What we cannot yet prove" section listing at least the four items named
  in §2 as not-includable, with the reason. A validation report that only contains wins reads as
  marketing and will be treated as such by the reader it is written for.
- No network access during regeneration. No credentials. No founder-machine-only paths.
- The regeneration script must run cargo invocations serially and must state the total expected
  wall-clock at the top so a reader knows what they are committing to.
- The report must pass `report_lint.py` against `PROOF_STANDARD.md`.

## 5. Exit criterion

### Criterion
Regeneration from a clean checkout reproduces the committed report with **0** non-volatile
differences, the report passes the proof-standard linter with **0** violations, and the report
contains **at least 15** distinct numbered claims of which **100%** carry a command, a tolerance or
band, and a class label.

### Measurement

Command:

    git clone --local . /tmp/eustress-verify && cd /tmp/eustress-verify
    python docs/validation/regenerate.py --out /tmp/eustress-verify/regenerated.md --serial
    echo "REGEN_EXIT=$?"

    python docs/validation/regenerate.py --diff \
        --committed docs/validation/reports/VALIDATION_REPORT_v1.md \
        --regenerated /tmp/eustress-verify/regenerated.md \
        --emit docs/PROMPTS/artifacts/G2.33/regeneration_diff.json
    echo "DIFF_EXIT=$?"

    python docs/PROMPTS/harness/checkers/report_lint.py \
        --standard docs/validation/PROOF_STANDARD.md \
        --report docs/validation/reports/VALIDATION_REPORT_v1.md
    echo "LINT_EXIT=$?"

On Windows use the PowerShell equivalent for the clone and temp path; the three script invocations
are identical.

Expected output shape:

    claims=18 with_command=18 with_tolerance=18 with_class=18
    non_volatile_diffs=0  volatile_fields_excluded=3 (generated_at, host, wallclock_s)
    REGEN_EXIT=0
    DIFF_EXIT=0
    required_fields=9 violations=0
    LINT_EXIT=0

Pass condition:

    REGEN_EXIT == 0  AND  DIFF_EXIT == 0  AND  LINT_EXIT == 0
    AND non_volatile_diffs == 0  AND  claims >= 15
    AND with_command == claims  AND  with_tolerance == claims  AND  with_class == claims

The clean-clone step is not optional. A regeneration that only works in the working tree has proven
nothing about a stranger's machine.

## 6. Critic gate

Gated on **D5 (simulation believability and numerical trust)** and **D6 (overall coherence)**, floor
**8.0 each**. The mean is irrelevant — either dimension below 8.0 fails the item.

Capture recipe: `docs/PROMPTS/harness/recipes/B1_validation_report.json`, which this item authors. The
recipe must bundle the report, the four input artifacts, and the regeneration diff.

D5 is the gate because the reader being simulated is a domain engineer deciding whether to trust the
numbers, not a reader deciding whether they are impressive. D6 is the gate because a report whose
sections were each written against a different standard reads as assembled rather than designed, and
that is precisely the signal a careful reader uses to decide how much of it to check.

The Critic never sees anything you write about your own work — not this prompt, not commit messages,
not a preamble. Every score it gives cites a specific claim id or measured value. Write the report so
its case survives with the self-report stripped out.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — report authored first, regeneration script written to reproduce it
   -> if still failing, MANDATORY approach change. Excluding one more field is NOT an approach
      change; inverting the flow so the script is authoritative and the report is generated from a
      template is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  non_volatile_diffs moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: regeneration requires a credential, a network fetch, or an uncommitted input
```

## 8. Artifact

`docs/validation/reports/VALIDATION_REPORT_v1.md`

A reader finds: the environment pinning block required by `PROOF_STANDARD.md`; a numbered claim
table where every row carries claim text, class, command, tolerance or acceptance band, result, and
provenance; the analytic-validation summary from `G2.30`; the published-reference summary from
`G2.31` including out-of-band cases and their causes; the V-Cell validity envelope summary from
`G2.32`; the regeneration instructions with expected wall-clock; and a "What we cannot yet prove"
section naming the config-default entity figure, the unmeasured Forge cost claim, the spec-only KYC
jurisdiction claim, and cross-platform determinism.

`docs/PROMPTS/artifacts/G2.33/regeneration_diff.json` is archived alongside it as the W3 evidence.

## 9. Definition of NOT done

- The report regenerates in the working tree but not from a clean clone, because it reads an
  artifact that was never committed. The clean-clone step exists to catch exactly this.
- A number was moved into `volatile_fields` to make the diff pass. Timestamps and host identifiers
  are volatile; a measured value never is.
- The report contains only passing results. Without the out-of-band cases from `G2.31` and the
  "cannot yet prove" section, the reader's first successful falsification of any claim destroys the
  credibility of all of them.
- The report cites the 2.10M-entity figure, the 80–90% Forge cost reduction, "72 countries", or
  "~200 MCP tools". Each is either a configuration default, unmeasured, spec-only, or simply wrong.
- The report claims determinism without scoping it to a single platform and a single run pair.
- The Critic clears D5 but refuses the wow gate, citing that the numbers are trustworthy while the
  document never states what decision they support. That is a legitimate refusal; the item is not
  done.
- Regeneration takes hours because the script rebuilds the engine binary for a number that did not
  require it. A regeneration nobody will run is a regeneration that does not exist.
````

---

## `G2.34` — STEP import wired, with geometric fidelity floors

````markdown
---
id: G2.34
title: STEP import produces a solid whose measured geometry matches the source within stated floors
workload: W1
workload_secondary: [W3, W4]
phase: G2
depends_on: [G1.31, G1.01]
blocks: [G2.35, G2.36, G2.37, G5.02]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.34/step_import_fidelity.json
escalation: >
  If truck-stepio cannot parse a fixture that a second independent STEP reader accepts, STALL with
  the fixture attached rather than hand-editing the fixture to suit the parser. Editing the input to
  fit the importer is how an integration passes its own test and fails a customer's file.
status: DRAFT
notes: >
  Tier L: cross-crate (cad + engine), each iteration costs an engine build. STEP/IGES import-export
  is recorded as 🔴 in docs/AUDIT/18_CAD_MESHGEOMETRY.md feature 10; this item moves only the import
  half, and only to a measured fidelity floor.
---

## 1. Objective

A STEP (AP203/AP214) file dropped into a Space is parsed into a truck solid, tessellated, and
converted to a canonical `.glb`, and the resulting geometry is measured against the source solid's
analytic properties. Volume, surface area, and bounding-box deviations are reported as numbers with
declared floors, and a fixture set of at least five parts exercising planar, cylindrical, filleted,
and multi-body cases passes those floors.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Units
are meter-native; studs are a display unit only. Physics is Avian. The licence is PolyForm Shield
1.0.0 — source-available.

**Why this item exists commercially.** An enterprise design partner already owns a CAD/PLM stack.
Their parts live in STEP. If Eustress cannot ingest their geometry, no simulation claim matters,
because the thing to simulate never arrives. And an integration that does not round-trip is not an
integration — this item does the import half with a fidelity measurement; `G2.35` does export and the
closed loop.

**Honest current state — verified in the tree.**
- `docs/AUDIT/18_CAD_MESHGEOMETRY.md` lists feature 10, "STEP / IGES import / export", as 🔴, and
  names it #5 in its top-8 wiring gaps. It also records open question Q18.3: own implementation or
  wrap FreeCAD via Forge.
- `eustress/crates/cad/Cargo.toml` line 25 already declares `truck-stepio = { workspace = true }`.
  The dependency is pulled in and **there is no code in `eustress/crates/cad/src/` that uses it** —
  a grep for `stepio` across `eustress/crates/**/*.rs` returns only comments.
- `eustress/crates/engine/src/mesh_import.rs` is the import watcher. It declares
  `MeshSourceFormat::{Stl, Obj, Ply, Step, Fbx}` (enum at line ~52, `from_extension` at line 61,
  `"step" | "stp" => Some(MeshSourceFormat::Step)` at line 66), documents the intended backend chain
  ("`truck-stepio::read` → truck solid → `truck-meshalgo` tessellate → GLB", module docs line 27),
  and at line 341 returns:
  `Err("STEP → GLB: truck-stepio parse scaffolded, tessellation + GLB writer pending".into())`.
  The conversion target is `path.with_extension("glb")` (line 238).
- The CAD crate already has a working GLB writer with no external glTF dependency:
  `eustress/crates/cad/src/export_glb.rs`, `pub fn write_glb(path, mesh: &EvalMesh, extras)` and
  `pub fn encode_glb(mesh, extras) -> CadResult<Vec<u8>>`. Positions are meters, Y-up,
  right-handed. **Use it. Do not write a second GLB writer.**
- Tessellation already exists and is tested: `eustress/crates/cad/tests/tessellate.rs` calls
  `eustress_cad::{evaluate_tree, parse_tree, tessellate_solid, EvalMesh, EvalOutput}` and contains a
  `bounds()` helper and an `assert_mesh_well_formed()` helper you should reuse rather than reinvent.
- Measurement primitives exist: `eustress/crates/cad/src/measure.rs` backs the shipped MCP tools
  `cad_measure` (`eustress/crates/tools/src/cad_tools.rs` line 1006), `cad_describe_part` (line 615),
  and `cad_validate_part` (line 802).

**Fixtures.** You must author STEP fixtures rather than vendor customer files. Generate them from the
CAD kernel itself where possible (build a solid, export STEP once `G2.35` exists — but for *this*
item you need STEP inputs first, so author them as text AP203 files or generate them with a tool you
can cite). Every fixture must be accompanied by its analytic ground truth (exact volume, exact
surface area, exact bounding box), computed from the shape's defining dimensions, not from the
importer. A fixture whose ground truth came from the importer proves nothing.

**Precision reality.** Tessellation is an approximation. A tessellated volume is always less than or
equal to a convex source volume and the error scales with chord tolerance. Declare the chord
tolerance used, and set floors relative to it. Do not declare a volume floor tighter than the
tessellation error implies — declare the floor, justify it from the tolerance, and then measure.

**Build reality.** This touches `eustress/crates/cad/` and `eustress/crates/engine/`, so an engine
build is 10–15 minutes. One cargo build at a time; the workspace shares `eustress/target/`. Never
kill a build mid-compile. Validate with `cargo run`, not `cargo check` — `cargo check` will not catch
the plugin/watcher registration failures this item is most likely to produce. Twelve builds is the
entire budget for three approaches; design so one build validates the whole fixture set.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/cad/src/` — a new `step_io.rs` module and its `lib.rs` registration
- `eustress/crates/cad/tests/step_import.rs` — create
- `eustress/crates/cad/tests/fixtures/step/` — create; the fixture set and its ground-truth TOML
- `eustress/crates/engine/src/mesh_import.rs` — wire the STEP arm to the new module
- `docs/PROMPTS/artifacts/G2.34/` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/cad/src/export_glb.rs` — the GLB writer is correct; use it unchanged
- `eustress/crates/common/src/realism/**` and anything under `eustress/crates/common/src/physics/`
- `docs/AUDIT/18_CAD_MESHGEOMETRY.md` — the audit is a ledger, not a scratchpad

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Loosening a fidelity floor after seeing the result, removing the filleted fixture because it is
  hard, coarsening the chord tolerance to make volume error look smaller in relative terms, or
  computing ground truth from the importer's own output are all measurement changes. If a floor is
  genuinely unachievable for tessellation-theoretic reasons, report
  `EXIT_CRITERION_UNMEASURABLE` with the derivation and stop.
- Do not hand-edit a fixture so the parser accepts it. If `truck-stepio` rejects a valid file, that
  is the finding and the escalation trigger.
- Do not add a Python or FreeCAD subprocess dependency. `docs/AUDIT/18_CAD_MESHGEOMETRY.md` Q18.3
  leaves that option open, but taking it is an architecture decision for a human, not an agent.
- Reuse `export_glb::encode_glb`, `tessellate_solid`, and the helpers in
  `eustress/crates/cad/tests/tessellate.rs`. A second implementation of any of them fails review.
- Batch verification: twelve builds across three approaches means one build must validate the entire
  fixture set, not one fixture.

## 5. Exit criterion

### Criterion
At least **5** STEP fixtures — covering planar-only, cylindrical, filleted, multi-body, and a part
with a through-hole — import successfully; for every fixture, `|volume_error| <= 1.0%`,
`|surface_area_error| <= 2.0%`, and every bounding-box extent is within `0.5 mm` of analytic ground
truth, at a declared chord tolerance of `0.05 mm` or finer.

### Measurement

Command (run from the `eustress/` workspace root):

    cargo test -p eustress-cad --test step_import -- --nocapture
    echo "EXIT=$?"

    python docs/PROMPTS/harness/checkers/step_fidelity_check.py \
        --json docs/PROMPTS/artifacts/G2.34/step_import_fidelity.json \
        --min-fixtures 5 \
        --require-kinds planar,cylindrical,filleted,multibody,through_hole \
        --max-volume-error-pct 1.0 \
        --max-area-error-pct 2.0 \
        --max-bbox-error-mm 0.5
    echo "CHECK_EXIT=$?"

Expected output shape:

    fixtures=6 imported=6 failed=0  chord_tolerance_mm=0.02
    kinds_covered=planar,cylindrical,filleted,multibody,through_hole
    worst_volume_error_pct=0.61 (fillet_block)
    worst_area_error_pct=1.44 (fillet_block)
    worst_bbox_error_mm=0.11 (cyl_boss)
    CHECK_EXIT=0

Pass condition:

    EXIT == 0  AND  CHECK_EXIT == 0  AND  fixtures >= 5  AND  failed == 0
    AND all five required kinds covered
    AND worst_volume_error_pct <= 1.0  AND  worst_area_error_pct <= 2.0
    AND worst_bbox_error_mm <= 0.5  AND  chord_tolerance_mm <= 0.05

Ground truth for every fixture must be recorded in the fixture's TOML with a
`ground_truth_source: analytic` field and the defining dimensions it was derived from. The checker
must reject any fixture whose ground truth is marked as importer-derived.

## 6. Critic gate

`critic_gate` is `[]`. Every proposition here is a measured geometric deviation against a declared
floor, gated by exit codes. The compensating tightness: a required fixture-kind matrix (so the suite
cannot consist of five boxes), a maximum chord tolerance (so error cannot be hidden by refinement
choices made after the fact), analytic-only ground truth, and a prohibition on editing fixtures to
suit the parser.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — truck-stepio read -> truck Solid -> tessellate_solid -> encode_glb
   -> if still failing, MANDATORY approach change. Adjusting chord tolerance is NOT an approach
      change; moving from whole-solid tessellation to per-face tessellation with explicit seam
      stitching is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with `imported` unchanged and worst_volume_error_pct
                  moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: truck-stepio rejects a fixture a second independent reader accepts
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.34/step_import_fidelity.json`

A reader finds: `command`, `toolchain`, `commit`, `chord_tolerance_mm`, and a `fixtures` array. Each
fixture carries `name`, `kind`, `step_path`, `ground_truth_source`, `defining_dimensions`,
`analytic_volume_m3`, `analytic_area_m2`, `analytic_bbox_m`, the imported counterparts,
`volume_error_pct`, `area_error_pct`, `bbox_error_mm` per axis, `triangle_count`, `import_ms`, and
`status`.

## 9. Definition of NOT done

- Five fixtures import and all five are rectangular prisms. The kind matrix exists because planar
  geometry exercises none of the surface evaluation a real customer part depends on.
- Ground truth was read back from the importer, so volume error is definitionally zero. The checker
  must reject `ground_truth_source: importer`.
- The importer works in `cargo test` but the file-watcher arm in `mesh_import.rs` still returns the
  "pending" error, so dropping a `.step` file into a Space does nothing. The commercial claim is the
  drop-in path, not the library call.
- A GLB writer was added to the engine crate instead of reusing `cad::export_glb::encode_glb`. Two
  writers guarantee two behaviours and one of them will be wrong at the customer's part.
- Units drifted. `export_glb` writes meters, Y-up, right-handed. STEP files commonly carry
  millimetres. A part that imports at 1000x scale passes a relative volume check only if the ground
  truth was scaled too — check absolute bounding-box extents in millimetres, which is why the bbox
  floor is absolute and not relative.
- A fixture was edited until it parsed. That is how an integration passes its own suite and fails on
  the first customer file.
````

---

## `G2.35` — STEP export and full solid round-trip

````markdown
---
id: G2.35
title: Eustress to STEP to Eustress round-trip holds geometry within measured floors
workload: W1
workload_secondary: [W3, W4]
phase: G2
depends_on: [G2.34]
blocks: [G2.37, G7.18]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.35/step_roundtrip_fidelity.json
escalation: >
  If exporting a feature-tree part requires flattening it to a mesh (losing BRep topology), STALL
  rather than shipping a mesh-in-a-STEP-wrapper. A STEP file that a customer's CAD system cannot
  edit is not an export, and shipping one destroys the integration claim on first contact.
status: DRAFT
notes: >
  Tier L, cross-crate, engine builds per iteration. This is the item that makes "we integrate with
  your CAD stack" a defensible sentence rather than an aspiration.
---

## 1. Objective

A Eustress CAD part can be exported to STEP and re-imported, and the round-tripped solid matches the
original within measured floors on volume, surface area, bounding box, face count, and topological
type. The round-trip is exercised over the same fixture kinds `G2.34` established, and the result is
reported as numbers, not as a claim that export "works".

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Units
are meter-native. Licence is PolyForm Shield 1.0.0 — source-available. Physics is Avian.

**Why round-trip and not export.** An integration that does not round-trip is not an integration.
Export alone can be validated only by opening the file in software we do not control; a round-trip is
self-verifying and can run in a test. It is also the honest test: a customer will export from
Eustress, edit in their own CAD system, and expect to bring it back.

**What `G2.34` left you.** STEP import is wired through `eustress/crates/cad/src/step_io.rs`, with
fidelity floors measured over at least five fixture kinds (planar, cylindrical, filleted, multibody,
through-hole) and recorded in `docs/PROMPTS/artifacts/G2.34/step_import_fidelity.json`. Reuse that
fixture set and its analytic ground truth; do not author a second one.

**Verified surface you are building on.**
- `eustress/crates/cad/Cargo.toml` declares `truck-stepio`, `truck-modeling`, `truck-topology`,
  `truck-geometry`, `truck-meshalgo`, and `truck-shapeops` as workspace dependencies.
- The feature-tree pipeline exists: `eustress_cad::{parse_tree, evaluate_tree, tessellate_solid}`
  with `EvalOutput` and `EvalMesh`, exercised in `eustress/crates/cad/tests/tessellate.rs`.
- `eustress/crates/cad/src/feature_tree.rs`, `feature.rs`, `sketch.rs`, `eval.rs`, `solver.rs`,
  `measure.rs`, `parts_csg.rs`, `templates.rs`, `quantity.rs` are all present.
- Shipped MCP tools that describe a part are `cad_describe_part`, `cad_validate_part`, and
  `cad_measure` (`eustress/crates/tools/src/cad_tools.rs` lines 615, 802, 1006). `cad_export_glb`
  exists at line 328. There is no `cad_export_step` — creating one is in scope for this item.
- `docs/AUDIT/18_CAD_MESHGEOMETRY.md` records STEP/IGES import/export as 🔴 and notes at R5.2 that
  non-manifold detection is mandatory before export to GLB; apply the same rule before STEP export.

**The topology requirement is the crux.** truck represents solids as BRep. Exporting a tessellated
mesh into a STEP wrapper produces a file that opens but cannot be edited parametrically, and a CAD
engineer will identify that within a minute of receiving it. The exported STEP must carry BRep
entities — the round-trip must preserve face count and face-surface types (plane, cylinder, torus,
b-spline), not merely a point cloud with the right bounding box.

**Build reality.** Engine builds are 10–15 minutes. One cargo build at a time; the workspace shares
`eustress/target/`. Never kill a build mid-compile. Validate with `cargo run`, not `cargo check`.
Twelve builds across three approaches — one build must validate the entire round-trip matrix.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/cad/src/step_io.rs` — add the export path alongside the import path
- `eustress/crates/cad/src/lib.rs` — export the new public function
- `eustress/crates/cad/tests/step_roundtrip.rs` — create
- `eustress/crates/tools/src/cad_tools.rs` — add a `cad_export_step` descriptor following the exact
  shape of the existing `cad_export_glb` descriptor at line 328
- `docs/PROMPTS/artifacts/G2.35/` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/cad/tests/fixtures/step/` — the `G2.34` fixture set and its ground truth are
  frozen inputs
- `docs/PROMPTS/artifacts/G2.34/step_import_fidelity.json` — a frozen input
- `eustress/crates/cad/src/export_glb.rs`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Dropping the face-count or surface-type comparison because tessellation is easier, loosening a
  floor after seeing a result, or removing the filleted fixture are measurement changes. If BRep
  export is genuinely impossible with the available truck surface, report
  `EXIT_CRITERION_UNMEASURABLE` naming the missing capability and stop — do not ship a mesh wrapper.
- Run the non-manifold check before writing any STEP file, mirroring the R5.2 rule the audit records
  for GLB export. An exported non-manifold solid must fail, not warn.
- Do not add a subprocess or FFI dependency. Pure Rust through truck.
- Reuse the `G2.34` fixtures and their analytic ground truth.
- The round-trip must be measured against the **original** solid, not against the first import. A
  round-trip measured against its own intermediate is a tautology.

## 5. Exit criterion

### Criterion
Over the frozen `G2.34` fixture set (≥5 kinds), every fixture completes `original → STEP → import`
with `|volume_error| <= 1.0%`, `|surface_area_error| <= 2.0%`, bounding-box extents within `0.5 mm`,
**exact** face-count equality, and **exact** face-surface-type multiset equality; and a deliberately
non-manifold input is **rejected** by the exporter with a distinct error.

### Measurement

Command (run from the `eustress/` workspace root):

    cargo test -p eustress-cad --test step_roundtrip -- --nocapture
    echo "EXIT=$?"

    python docs/PROMPTS/harness/checkers/step_roundtrip_check.py \
        --json docs/PROMPTS/artifacts/G2.35/step_roundtrip_fidelity.json \
        --min-fixtures 5 \
        --require-kinds planar,cylindrical,filleted,multibody,through_hole \
        --max-volume-error-pct 1.0 \
        --max-area-error-pct 2.0 \
        --max-bbox-error-mm 0.5 \
        --require-exact face_count,face_surface_types \
        --require-nonmanifold-rejection
    echo "CHECK_EXIT=$?"

Expected output shape:

    fixtures=6 roundtripped=6 failed=0
    worst_volume_error_pct=0.44  worst_area_error_pct=1.02  worst_bbox_error_mm=0.09
    face_count_mismatches=0  surface_type_mismatches=0
    nonmanifold_input_rejected=true (error=NonManifoldSolid)
    CHECK_EXIT=0

Pass condition:

    EXIT == 0  AND  CHECK_EXIT == 0  AND  failed == 0
    AND face_count_mismatches == 0  AND  surface_type_mismatches == 0
    AND worst_volume_error_pct <= 1.0  AND  worst_area_error_pct <= 2.0
    AND worst_bbox_error_mm <= 0.5  AND  nonmanifold_input_rejected == true

## 6. Critic gate

`critic_gate` is `[]`. Every proposition is a measured geometric or topological equality gated by
exit codes. The compensating tightness: exact topological equality (not a tolerance), a required
fixture-kind matrix, a mandatory non-manifold rejection demonstration, and the rule that the
round-trip is measured against the original solid rather than an intermediate.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — direct truck BRep -> AP203 entity emission via truck-stepio write
   -> if still failing, MANDATORY approach change. Tweaking entity ordering is NOT an approach
      change; moving from whole-solid emission to per-shell emission with explicit topology tables
      is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with face_count_mismatches unchanged and > 0
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: export would require flattening BRep to a mesh (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.35/step_roundtrip_fidelity.json`

A reader finds: `command`, `toolchain`, `commit`, and a `fixtures` array where each entry carries
`name`, `kind`, `original_volume_m3`, `original_area_m2`, `original_bbox_m`, `original_face_count`,
`original_surface_types` (a sorted multiset), the round-tripped counterparts, the three error
metrics, the two exactness booleans, `step_bytes`, `export_ms`, `import_ms`, and `status`. A separate
`nonmanifold_case` object records the rejected input and the exact error variant returned.

## 9. Definition of NOT done

- Volume and bounding box match perfectly because the exporter wrote a tessellated mesh into STEP.
  The face-count and surface-type equality checks exist precisely to catch this, and they are exact
  rather than tolerant for that reason.
- The round-trip was measured against the first import rather than the original solid, making the
  comparison trivially self-consistent.
- Export succeeds on a non-manifold solid. `docs/AUDIT/18_CAD_MESHGEOMETRY.md` R5.2 makes
  non-manifold detection mandatory before GLB export; a STEP export without it hands a customer a
  file their kernel will refuse.
- `cad_export_step` was added as an MCP descriptor but never routed, so the agent-facing surface
  claims a capability the tool does not perform. Follow the `cad_export_glb` descriptor at
  `eustress/crates/tools/src/cad_tools.rs:328` exactly, including its error paths.
- Only the fixtures that round-trip cleanly were kept. The fixture set is frozen from `G2.34`;
  removing one is a measurement change.
- Millimetre/metre confusion cancels between export and import, so relative errors pass while the
  written STEP file is off by 1000x for any external consumer. The absolute bounding-box floor in
  millimetres is the guard against this, and the emitted STEP's declared length unit must be recorded
  in the artifact.
````

---

## `G2.36` — USD interoperability record and minimal USDA round-trip

````markdown
---
id: G2.36
title: A minimal USDA scene round-trips with preserved units, up-axis, and hierarchy
workload: W3
workload_secondary: [W1, W4]
phase: G2
depends_on: [G2.34]
blocks: [G2.37]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.36/usd_roundtrip.json
escalation: >
  If achieving the round-trip requires linking the OpenUSD C++ library or any non-Rust FFI, STALL
  immediately. Adding a C++ toolchain dependency to a workspace whose entire build story is one
  cargo command is an architecture decision for a human, not an agent.
status: DRAFT
notes: >
  Tier L. Deliberately scoped to USDA (the ASCII crate-free form) and to a minimal subset — xform,
  mesh, units, up-axis, hierarchy. USDC (binary crate format) is explicitly out of scope and the
  decision record must say so.
---

## 1. Objective

Eustress writes and reads a minimal USDA scene — nested `Xform` prims, `Mesh` prims with points and
face indices, `metersPerUnit`, and `upAxis` — and a round-trip preserves hierarchy paths, transforms,
vertex positions, and unit/axis metadata within measured floors. A written interoperability record
states exactly which USD subset is supported and which is not, so no one pitches the rest.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Units
are **meter-native**; studs are a display unit only. That fact is the crux of this item, because USD
carries `metersPerUnit` explicitly and most DCC exports write centimetres. Licence is PolyForm
Shield 1.0.0 — source-available. Physics is Avian.

**Honest current state — verified.**
- `eustress/crates/engine/src/usd_loader.rs` is **14 lines**. It contains `UsdLoaderPlugin`, whose
  `fn build(&self, _app: &mut App)` body is the single comment `// TODO: Implement USD loader`. It
  is declared at `eustress/crates/engine/src/lib.rs:158` as `pub mod usd_loader;`. USD support is
  **0%**.
- `eustress/crates/common/src/pointcloud/formats.rs` declares `USDA` (line 70) and `USDC` (line 72)
  as format enum variants, with `from_extension` mapping `"usda"` and `"usdc"` (lines 85–86) and
  descriptions "USD ASCII - Pixar scene format" and "USD Crate - Binary USD" (lines 114–115). Line
  1895 lists `"usdz", "usda", "usdc"` among recognised extensions. These are **recognition only** —
  no parser, no writer.
- `docs/architecture/USD_NATIVE_FORMAT.md` is **corrupt**: 1336 lines, beginning with the bytes
  `//! # Sou` (Rust doc-comment source, not Markdown) and containing 5674 NUL bytes. It is
  unreadable as a document and cannot be trusted as a specification. `G1.30` recorded it in the
  claim ledger's `unreadable_sources`. **This item is where it gets resolved** — either replaced by
  a correct decision record or explicitly retired, with no changelog residue explaining the history.

**What `G2.34` gave you.** A working STEP import path in `eustress/crates/cad/src/step_io.rs`,
tessellation via `eustress_cad::tessellate_solid`, a GLB writer at
`eustress/crates/cad/src/export_glb.rs` whose convention is meters, Y-up, right-handed, and a
fixture discipline where ground truth is analytic and never importer-derived.

**The subset decision is the deliverable, not a limitation to hide.** USD is enormous — layers,
composition arcs, variant sets, references, payloads, instancing, schemas. A truthful record that
Eustress supports flattened single-layer USDA with `Xform` and `Mesh` prims is far more valuable to a
design partner than an unqualified "USD support", because the partner can immediately test whether
their pipeline fits. State the unsupported list explicitly: composition arcs, variants, payloads,
instancing, USDC binary, USDZ packages, materials, and skeletal data.

**Build reality.** Engine builds are 10–15 minutes. One cargo build at a time; the workspace shares
`eustress/target/`. Never kill a build mid-compile. Validate with `cargo run`, not `cargo check` —
`cargo check` will not catch the plugin-registration failure that would leave `UsdLoaderPlugin` a
no-op. Twelve builds across three approaches.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/src/usd_loader.rs` — replace the stub
- `eustress/crates/engine/tests/usd_roundtrip.rs` — create
- `eustress/crates/engine/tests/fixtures/usd/` — create; the USDA fixture scenes
- `docs/architecture/USD_INTEROPERABILITY.md` — create; the decision record
- `docs/architecture/USD_NATIVE_FORMAT.md` — delete or replace, since it is unreadable
- `docs/PROMPTS/artifacts/G2.36/` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/common/src/pointcloud/formats.rs` — the enum variants are correct as recognition
  entries; this item does not change the point-cloud path
- `eustress/crates/cad/src/export_glb.rs`
- `eustress/Cargo.toml` workspace dependencies — no new native or FFI dependency (see escalation)

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Shrinking the fixture scene, dropping the nested-hierarchy case, writing a USDA file with
  `metersPerUnit = 1` and reading it back with the same assumption baked in on both sides, or
  removing the centimetre fixture are measurement changes. If a floor is unachievable, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- No OpenUSD C++ linkage, no FFI, no subprocess. Pure Rust text parsing and emission for the declared
  subset. If that is insufficient, that is the escalation.
- The unit test matrix must include at least one fixture authored in **centimetres**
  (`metersPerUnit = 0.01`) and at least one in **Z-up**, because those are what real DCC exports
  carry and a round-trip that only handles the Eustress-native convention proves nothing about
  interoperability.
- The decision record must be written so it reads as always having been correct — no account of what
  the previous file said, no "we now believe".
- Do not claim USDC, USDZ, materials, variants, or composition. Declaring them unsupported is the
  point.

## 5. Exit criterion

### Criterion
At least **4** USDA fixtures — flat single mesh, nested three-level hierarchy, centimetre-unit scene,
Z-up scene — round-trip with hierarchy prim paths **exactly** preserved, per-vertex position error
`<= 1e-5 m`, per-prim transform component error `<= 1e-5`, and `metersPerUnit` / `upAxis` preserved
exactly; and `docs/architecture/USD_INTEROPERABILITY.md` declares at least **8** explicitly
unsupported USD features.

### Measurement

Command (run from the `eustress/` workspace root):

    cargo test -p eustress-engine --test usd_roundtrip -- --nocapture
    echo "EXIT=$?"

    python docs/PROMPTS/harness/checkers/usd_roundtrip_check.py \
        --json docs/PROMPTS/artifacts/G2.36/usd_roundtrip.json \
        --record docs/architecture/USD_INTEROPERABILITY.md \
        --min-fixtures 4 \
        --require-kinds flat_mesh,nested_hierarchy,centimeter_units,z_up \
        --max-position-error-m 1e-5 \
        --max-transform-error 1e-5 \
        --min-unsupported-declared 8
    echo "CHECK_EXIT=$?"

Expected output shape:

    fixtures=5 roundtripped=5 failed=0
    prim_path_mismatches=0
    worst_position_error_m=2.4e-07  worst_transform_error=1.1e-07
    meters_per_unit_preserved=5/5  up_axis_preserved=5/5
    unsupported_features_declared=11
    CHECK_EXIT=0

Pass condition:

    EXIT == 0  AND  CHECK_EXIT == 0  AND  failed == 0
    AND prim_path_mismatches == 0
    AND worst_position_error_m <= 1e-5  AND  worst_transform_error <= 1e-5
    AND meters_per_unit_preserved == fixtures  AND  up_axis_preserved == fixtures
    AND unsupported_features_declared >= 8

The centimetre fixture must round-trip to the *same physical size*, not the same numeric values.
The checker must compare positions in meters after unit resolution — a pass that comes from writing
and reading the same raw numbers with a unit tag nobody honours is a failure disguised as a pass.

## 6. Critic gate

`critic_gate` is `[]`. The claims are exact string equality on prim paths, exact preservation of two
metadata fields, and two numeric floors — all decided by exit codes. The compensating tightness: a
required fixture-kind matrix that includes the two conventions Eustress does *not* use natively
(centimetres, Z-up), a minimum count of explicitly declared unsupported features, and the requirement
that unit comparison happens in meters after resolution.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — hand-rolled USDA text writer + recursive-descent reader for the
                 declared subset
   -> if still failing, MANDATORY approach change. Adding another prim type is NOT an approach
      change; moving to a two-stage parse (tokenise to a generic prim tree, then bind to Eustress
      types) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with prim_path_mismatches unchanged and > 0
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: any approach requiring OpenUSD C++ or FFI (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.36/usd_roundtrip.json`

A reader finds: `command`, `toolchain`, `commit`, and a `fixtures` array. Each entry carries `name`,
`kind`, `usda_path`, `source_meters_per_unit`, `source_up_axis`, `prim_count`, `prim_paths` before
and after, `worst_position_error_m`, `worst_transform_error`, `meters_per_unit_roundtripped`,
`up_axis_roundtripped`, `bytes_written`, and `status`.

`docs/architecture/USD_INTEROPERABILITY.md` is the companion decision record: the supported subset,
the explicitly unsupported feature list with at least eight entries, the unit and axis conversion
rule (everything resolves to meters, Y-up on ingest), the reason USDC is out of scope, and what a
partner should test first with their own file.

## 9. Definition of NOT done

- The round-trip passes because both writer and reader assume Eustress conventions, so the
  centimetre and Z-up fixtures never actually exercise conversion. Those two fixtures are the item.
- `usd_loader.rs` gained a parser but `UsdLoaderPlugin::build` is still empty, so nothing is
  registered and the studio path does nothing. `cargo check` will not catch this; `cargo run` will.
- `docs/architecture/USD_NATIVE_FORMAT.md` is still in the tree, still corrupt, still cited by
  someone as the USD specification.
- The decision record says "USD support" without the unsupported list. A partner reads that,
  exports a scene with variant sets and material bindings, gets garbage, and the integration claim is
  dead on first contact.
- The unsupported list is padded with eight trivial entries to clear the count. Composition arcs,
  variant sets, references, payloads, instancing, USDC, USDZ, materials, and skeletal data are the
  ones that matter — name those.
- A new C++ or FFI dependency entered the workspace. The build story is one cargo command; that is
  worth more than USD completeness and the decision is not an agent's to make.
````

---

## `G2.37` — Unit and coordinate-frame conformance across all interchange paths

````markdown
---
id: G2.37
title: Every import and export path agrees on meters and on handedness, measured
workload: W3
workload_secondary: [W1]
phase: G2
depends_on: [G2.34, G2.36, G2.35]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G2.37/frame_conformance.json
escalation: >
  If a conformance failure can only be fixed inside a format backend that another item currently
  owns (step_io.rs from G2.34/G2.35, usd_loader.rs from G2.36), record the failure and STALL rather
  than editing it — two items writing the same backend will produce a merge that neither measured.
status: DRAFT
notes: >
  Tier M. This item measures and reports; it fixes only what lives in its own scope list. The value
  is the conformance matrix, which is the single artifact an enterprise integration engineer asks for
  before committing a pipeline.
---

## 1. Objective

A single conformance matrix reports, for every interchange path Eustress supports, whether a known
asymmetric reference object survives with the correct scale, handedness, up-axis, and orientation.
Each cell is a measured number, not a claim, and every failing cell is named with its observed
transform error rather than omitted.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. **Units
are meter-native; studs are a display unit only.** Licence is PolyForm Shield 1.0.0 —
source-available. Physics is Avian.

**Why an asymmetric reference object.** A cube tells you nothing: it survives a transposed rotation
matrix, a mirrored axis, and a swapped up-axis without visible change. The reference object for this
item must be chiral and distinguishable on every axis — for example an L-shaped bracket with three
distinct arm lengths and a single chamfer that breaks the remaining symmetry. Orientation errors in
this repository have historically been silent for exactly this reason.

**Verified paths and their declared conventions.**
- `eustress/crates/cad/src/export_glb.rs` — GLB writer, documented as "positions are meters, Y-up,
  right-handed — matching Eustress / glTF convention". `write_glb(path, mesh, extras)` and
  `encode_glb(mesh, extras)`.
- `eustress/crates/engine/src/mesh_import.rs` — the import watcher. `MeshSourceFormat` covers
  `Stl`, `Obj`, `Ply`, `Step`, `Fbx`; `from_extension` at line 61 maps `"stl"`, `"obj"`,
  `"step" | "stp"`; the conversion target is `path.with_extension("glb")` at line 238. Module docs
  (lines 22–33) state the intended backends: `stl_io` for STL, `tobj` for OBJ, `truck-stepio` for
  STEP, `ply-rs` for PLY, and an external Assimp/FBX2glTF process for FBX which is **not shipped
  in-process** and falls back to "conversion not available".
- STEP import and export are wired by `G2.34` and `G2.35` in `eustress/crates/cad/src/step_io.rs`,
  with fidelity measured in `docs/PROMPTS/artifacts/G2.34/step_import_fidelity.json` and
  `docs/PROMPTS/artifacts/G2.35/step_roundtrip_fidelity.json`.
- USDA is wired by `G2.36` in `eustress/crates/engine/src/usd_loader.rs`, with the conversion rule
  "everything resolves to meters, Y-up on ingest" recorded in
  `docs/architecture/USD_INTEROPERABILITY.md`.

**A rotation defect of exactly this class has occurred in this project before.** In the importer,
CFrame rotations were inverted because a `Matrix3` stored rows while the conversion helper expected
columns, so the transpose silently produced the inverse rotation. Identity-matrix tests could not
catch it. That is why this item requires a chiral reference object and per-axis assertions rather
than a magnitude comparison.

**Format conventions you must handle rather than assume away.** STL is unitless by convention and
usually millimetres in practice. OBJ is unitless. STEP declares its length unit inside the file. USD
declares `metersPerUnit` and `upAxis`. glTF is meters, Y-up, right-handed. A conformance matrix that
does not record the declared source unit per path has not measured anything.

**Build reality.** Engine builds are 10–15 minutes. One cargo build at a time; shared
`eustress/target/`. Never kill a build mid-compile. Validate with `cargo run`, not `cargo check`.
Six builds.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/engine/tests/frame_conformance.rs` — create
- `eustress/crates/engine/tests/fixtures/frame/` — create; the chiral reference object in each
  source format, plus its analytic ground truth
- `docs/PROMPTS/artifacts/G2.37/` — create
- `docs/PROMPTS/harness/checkers/frame_conformance_check.py` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/cad/src/step_io.rs` — owned by `G2.34`/`G2.35`
- `eustress/crates/engine/src/usd_loader.rs` — owned by `G2.36`
- `eustress/crates/cad/src/export_glb.rs`
- Any fixture frozen by `G2.34`, `G2.35`, or `G2.36`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Replacing the chiral reference with a symmetric one, comparing bounding-box magnitudes instead of
  signed per-axis extents, dropping a format because it fails, or normalising both sides through the
  same conversion helper so an error cancels are all measurement changes.
- Assertions must be **signed and per-axis**. A comparison that squares or takes absolute values
  cannot detect a mirrored axis, which is the defect class this item exists for.
- Record the declared source unit for every path. A path whose unit is undeclared by the format
  (STL, OBJ) must record the assumption Eustress applies and flag it as an assumption, not a fact.
- A failing cell is a result. Report it with its observed transform; do not omit the path.
- You may not fix a backend owned by another item. Record and escalate.

## 5. Exit criterion

### Criterion
The matrix covers at least **5** interchange paths (GLB export, STL import, OBJ import, STEP
import, USDA import); every covered path reports signed per-axis extent error `<= 0.5 mm` and
determinant of the recovered orientation `> 0` (no mirroring); every path records its
`declared_source_unit` and `unit_assumption` fields; and **every** path in the matrix is present with
a `PASS` or `FAIL` status — none omitted.

### Measurement

Command (run from the `eustress/` workspace root):

    cargo test -p eustress-engine --test frame_conformance -- --nocapture
    echo "EXIT=$?"

    python docs/PROMPTS/harness/checkers/frame_conformance_check.py \
        --json docs/PROMPTS/artifacts/G2.37/frame_conformance.json \
        --min-paths 5 \
        --require-paths glb_export,stl_import,obj_import,step_import,usda_import \
        --max-axis-error-mm 0.5 \
        --require-positive-determinant \
        --require-fields declared_source_unit,unit_assumption
    echo "CHECK_EXIT=$?"

Expected output shape:

    paths=6 pass=5 fail=1
    worst_axis_error_mm=0.08 (step_import)
    determinant_negative=0
    missing_unit_fields=0
    FAIL: fbx_import  reason=conversion_not_available_in_process
    CHECK_EXIT=0

Pass condition:

    EXIT == 0  AND  CHECK_EXIT == 0  AND  paths >= 5
    AND all five required paths present
    AND for every required path: axis_error_mm <= 0.5 AND determinant > 0
    AND missing_unit_fields == 0

FBX is expected to fail — `mesh_import.rs` documents that FBX conversion is not shipped in-process.
It must appear in the matrix with that reason, which is why the checker requires presence rather than
universal success. A required path failing does fail the item; an optional path failing with a stated
reason does not.

## 6. Critic gate

`critic_gate` is `[]`. Every cell is a signed numeric comparison plus a determinant sign, decided by
exit codes. The compensating tightness: a chiral reference object, signed per-axis assertions, a
determinant check that catches mirroring, mandatory unit-provenance fields, and a requirement that
failing paths appear rather than vanish.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — one chiral fixture authored per source format, compared against
                 analytic ground truth after import
   -> if still failing, MANDATORY approach change. Adding a format is NOT an approach change;
      moving from extent comparison to recovering the full affine transform by least squares over
      labelled feature points is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with `pass` count unchanged
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the fix lies in a backend owned by G2.34, G2.35, or G2.36 (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G2.37/frame_conformance.json`

A reader finds: `command`, `toolchain`, `commit`, `reference_object` (its defining dimensions and why
it is chiral), and a `paths` array. Each entry carries `path_name`, `direction` (`import` |
`export`), `declared_source_unit`, `unit_assumption`, `expected_extents_signed_m`,
`observed_extents_signed_m`, `axis_error_mm` per axis, `orientation_determinant`,
`recovered_up_axis`, `status`, and `reason` (required when `FAIL`).

This matrix is the artifact an enterprise integration engineer asks for before committing a pipeline,
and it is the input `G6.32` uses to decide whether a prospect's file formats are even in play.

## 9. Definition of NOT done

- The reference object is a cube, a sphere, or anything with a symmetry that survives a transposed
  rotation. The historical CFrame-inversion defect in this project passed every identity-based test.
- Errors were compared as magnitudes, so a mirrored axis reads as zero error. Signed per-axis
  comparison plus a positive-determinant check is the requirement.
- FBX was quietly dropped from the matrix because it fails. `mesh_import.rs` documents that FBX
  conversion is not in-process; the honest matrix says so, and a partner with an FBX pipeline learns
  it from the matrix rather than from a failed pilot.
- Both sides of a comparison were routed through the same conversion helper, so a bug in that helper
  cancels. Ground truth must be analytic, from the object's defining dimensions.
- STL or OBJ passed with no `unit_assumption` recorded. Both formats are unitless; the assumption
  Eustress applies is exactly the thing a partner needs to know.
- A backend owned by another item was edited to make a cell pass, producing a change neither item
  measured.
````

---

## `G6.30` — Shipped-capability register and demo gate

````markdown
---
id: G6.30
title: Every declared tool id classified shipped or declared, with a gate that blocks a demo script
workload: W4
workload_secondary: [W3, W2]
phase: G6
depends_on: [G1.30, G1.01]
blocks: [G6.31, G6.32]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G6.30/capability_register.json
escalation: >
  If the shipped-versus-declared determination cannot be made mechanically from source for more than
  5% of ids, STALL rather than hand-classifying the remainder. A register maintained by hand is stale
  within one commit and will eventually let a spec-only capability into a pitch, which is the exact
  failure this item exists to prevent.
status: DRAFT
notes: >
  Tier M. The register must be generated from source on demand, never hand-maintained. Its value is
  that it cannot drift; a hand-written list has none of that value.
---

## 1. Objective

A generated register classifies every tool id declared anywhere in the Eustress mode manifests as
`SHIPPED` (a live handler exists in source) or `DECLARED` (an id with metadata and no dispatch), and a
gate mode refuses — with a non-zero exit — to bless any demo script that references a `DECLARED` id.
After this item, pitching a spec-only capability requires deliberately bypassing a tool, rather than
merely forgetting.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Licence
is PolyForm Shield 1.0.0 — **source-available**, never open source. Physics is Avian. Slint compiles
to Rust. Units are meter-native.

**Why this item is the gate on the entire partner motion.** A pilot pitched on a spec-only capability
loses a design partner permanently — a buyer who clicks a button that does nothing does not file a
bug, they end the evaluation. The repository currently declares far more capability than it dispatches,
and the declaration is honest but scattered.

**The verified numbers you are systematising.**
- `eustress/crates/engine/modes/` contains ten manifests: `business.toml`, `civil.toml`,
  `engineering.toml`, `gaming.toml`, `government.toml`, `health.toml`, `justice.toml`, `legal.toml`,
  `military.toml`, `student.toml`. Tool ids appear in `tools = [...]` arrays inside
  `[[tabs.sections]]` blocks, in the form `prefix:snake_case_name`, e.g.
  `tools = ["data:import", "gov:open_data_connector", "gov:provenance_inspector"]`
  (`government.toml` line 137).
- `docs/architecture/GOVERNMENT_MODE.md` §9 states, verbatim: of 562 declared government tool ids,
  exactly one — `data:import`, inherited — has a real handler; the other 561 are declared intentions;
  and the mode "must never be described as working software." Those two facts are your ground truth
  for validating the generator: `data:import` must classify `SHIPPED`, and a randomly chosen `gov:`
  id must classify `DECLARED`.
- `eustress/crates/engine/src/tool_metadata.rs` is **generated** (by `scripts/gen_tool_metadata.py`)
  and gives every declared id a label, tooltip, and icon. Presence in `tool_metadata.rs` therefore
  proves nothing about dispatch and must not be used as a shipped signal.
- The real MCP tool surface is elsewhere and is much smaller: 79 tool descriptors across
  `eustress/crates/tools/src/` (`cad_tools.rs`, `simulation_tools.rs`, `universe_tools.rs`,
  `git_tools.rs`, `script_tools.rs`, `entity_tools.rs`, `memory_tools.rs`, `embedvec_tools.rs`,
  `file_tools.rs`, `physics_tools.rs`, `spatial_tools.rs`, `shell_tools.rs`, `diff_tools.rs`), each
  declared as a `ToolDefinition` with a `name: "..."` field
  (`eustress/crates/tools/src/registry.rs`); plus 24 bridge tools in
  `eustress/crates/mcp-server/src/bridge_tools.rs` (which includes `invoke_action` at line 1220);
  plus one hand-rolled tool. Mode filtering lives in `eustress/crates/tools/src/modes.rs`.
- `eustress/crates/engine/src/studio_modes.rs` holds the studio-side mode shape.

**The mechanical rule you must define and publish.** An id classifies `SHIPPED` if and only if the
literal id string appears in a dispatch position in Rust source outside the generated metadata file
and outside the manifests themselves — or the id maps to a registered `ToolDefinition.name`. Write
the rule down in the register itself as `classification_rule`, so a reader can re-derive every cell.
A rule that is not published is not auditable.

**Build reality.** This item does not need an engine build to generate the register — it reads source
text. Six builds are budgeted only in case the generator is implemented as a Rust binary; a Python
generator under `scripts/` matches existing convention (`scripts/gen_tool_metadata.py`,
`scripts/gen_tool_icons.py`) and costs zero builds. Prefer the zero-build path.

## 3. Scope

### In scope — files this item may edit
- `scripts/gen_capability_register.py` — create
- `docs/PROMPTS/artifacts/G6.30/` — create; holds `capability_register.json` and
  `capability_register.md`
- `docs/PROMPTS/harness/checkers/fixtures/demo_script_bad.json` — create; the negative-control demo
  script

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/modes/*.toml` — **you may not wire a tool to improve the register's
  numbers.** The register reports; wiring is other work.
- `eustress/crates/engine/src/tool_metadata.rs` — generated
- `docs/architecture/GOVERNMENT_MODE.md` — a ledger, not a scratchpad

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Counting presence in `tool_metadata.rs` as shipped, excluding a manifest, or loosening the
  dispatch-detection rule so more ids classify `SHIPPED` are measurement changes. If the rule is
  genuinely undecidable for a class of ids, add a third class `UNDECIDABLE`, report its count, and
  keep it under the 5% escalation ceiling.
- Do not wire any tool. The register's job is to be true today.
- The generator must be deterministic: two runs on the same commit produce byte-identical JSON apart
  from a declared timestamp field.
- The gate mode must exit non-zero, not warn. A warning in a pitch-preparation workflow is ignored.
- Publish the classification rule inside the artifact.

## 5. Exit criterion

### Criterion
The register covers **100%** of tool ids across all ten mode manifests; `UNDECIDABLE` is **≤ 5%** of
ids; `data:import` classifies `SHIPPED` and a named `gov:`-prefixed id classifies `DECLARED`; two
consecutive generations are byte-identical apart from the declared timestamp; and the gate rejects a
demo script referencing a `DECLARED` id with **exit 2** while accepting an all-`SHIPPED` script with
**exit 0**.

### Measurement

Command:

    python scripts/gen_capability_register.py \
        --manifests eustress/crates/engine/modes \
        --sources eustress/crates \
        --out docs/PROMPTS/artifacts/G6.30/capability_register.json
    echo "GEN_EXIT=$?"

    python scripts/gen_capability_register.py \
        --manifests eustress/crates/engine/modes --sources eustress/crates \
        --out /tmp/register_second.json
    python scripts/gen_capability_register.py --compare \
        docs/PROMPTS/artifacts/G6.30/capability_register.json /tmp/register_second.json \
        --ignore-fields generated_at
    echo "DETERMINISM_EXIT=$?"

    python scripts/gen_capability_register.py --assert-class data:import=SHIPPED
    python scripts/gen_capability_register.py --assert-class gov:provenance_inspector=DECLARED
    echo "GROUNDTRUTH_EXIT=$?"

    python scripts/gen_capability_register.py --gate \
        --register docs/PROMPTS/artifacts/G6.30/capability_register.json \
        --demo-script docs/PROMPTS/harness/checkers/fixtures/demo_script_bad.json
    echo "GATE_BAD_EXIT=$?"

Expected output shape:

    manifests=10 total_ids=1412 shipped=31 declared=1376 undecidable=5 (0.35%)
    classification_rule=dispatch_literal_or_tooldefinition_name
    GEN_EXIT=0
    DETERMINISM_EXIT=0
    GROUNDTRUTH_EXIT=0
    GATE: demo step 3 references 'gov:budget_scenario' -> DECLARED. Blocked.
    GATE_BAD_EXIT=2

Pass condition:

    GEN_EXIT == 0  AND  DETERMINISM_EXIT == 0  AND  GROUNDTRUTH_EXIT == 0
    AND GATE_BAD_EXIT == 2
    AND coverage == 100%  AND  undecidable_pct <= 5.0

Also run the gate against an all-`SHIPPED` script and confirm exit 0. A gate that rejects everything
is as useless as one that rejects nothing.

## 6. Critic gate

`critic_gate` is `[]`. Every proposition is a count, a classification, or an exit code. The
compensating tightness: 100% coverage, a bounded `UNDECIDABLE` fraction, two named ground-truth cells
derived from an independent source (`GOVERNMENT_MODE.md` §9), byte-level determinism across runs, a
published classification rule, and a two-sided gate demonstration.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — manifest parse + literal-string dispatch search across eustress/crates
   -> if still failing, MANDATORY approach change. Broadening the grep is NOT an approach change;
      moving to a registry-driven classification that enumerates ToolDefinition names from source
      and treats manifest ids as a separate namespace with an explicit mapping table is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with undecidable_pct moving < 1 point and still > 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: > 5% undecidable after three approaches (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G6.30/capability_register.json`

A reader finds: `generated_at`, `commit`, `classification_rule` (stated in full), `manifests`
(the ten files with per-file id counts), `totals` (shipped, declared, undecidable, and the undecidable
percentage), and an `ids` array where each entry carries `id`, `manifest`, `mode`, `discipline`,
`tab`, `section`, `class`, and `evidence` — for `SHIPPED`, the `path:line` of the dispatch or
`ToolDefinition`; for `DECLARED`, the statement that no dispatch was found; for `UNDECIDABLE`, the
reason.

`capability_register.md` renders the same data grouped by mode, for humans, and is generated from
the JSON. `--gate` is the mode `G6.31`, `G6.32`, and every future pitch-preparation step calls.

## 9. Definition of NOT done

- The register is hand-written or hand-corrected. It will be stale within one commit and will
  eventually bless a `DECLARED` id.
- `SHIPPED` counts include ids present only in `tool_metadata.rs`. That file is generated and gives
  every declared id a label and icon; presence there is exactly the false signal to avoid.
- The gate warns instead of exiting non-zero. In a pitch-preparation workflow a warning is noise.
- The gate was demonstrated only on the bad script, so there is no evidence it accepts a good one.
- A tool was wired during this item to make the shipped count look better. The register's value is
  that it is true, not that it is flattering.
- `undecidable` is 0% because ambiguous ids were silently assigned `DECLARED`. Under-claiming is
  safer than over-claiming, but a hidden judgement call is still a hidden judgement call — it belongs
  in `UNDECIDABLE` with a reason.
````

---

## `G6.31` — Unwired-button sweep of the declared demo path

````markdown
---
id: G6.31
title: Zero dead controls on the one path a design partner will be shown
workload: W4
workload_secondary: [W1, W6]
phase: G6
depends_on: [G6.30, G6.04, G1.12]
blocks: [G6.33]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: [D4, D6]
capture_recipe: docs/PROMPTS/harness/recipes/B1_vertical_demo_path.json
artifact: docs/PROMPTS/artifacts/G6.31/demo_path_sweep.json
escalation: >
  If clearing the path requires touching eustress/crates/engine/src/ui/slint_ui.rs in more than three
  distinct places, STALL. That file is 23,103 lines, every panel's drain logic funnels through it,
  and the drain-skip failure class it hosts (one missing required parameter silently kills every UI
  click) is invisible to any existing test.
status: DRAFT
notes: >
  Tier M with a Critic gate on D4 and D6, because the deliverable is a path a human is shown, and a
  path that technically works while looking assembled still loses the room.
---

## 1. Objective

One demo path is declared, exercised control by control in the running studio, and every control on
it either performs its stated action or is removed from the path. The sweep report lists every
control touched with its observed outcome, and the path contains zero controls that render, accept a
click, and do nothing.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine — a world model an AI reasons
over and a document a human edits. Never a game engine. Licence is PolyForm Shield 1.0.0 —
source-available. Physics is Avian. **Slint is Rust**: `.slint` files compile to Rust, so never frame
"Rust-first" as opposed to Slint. Units are meter-native; studs are a display unit only.

**What `G6.30` gave you.** `docs/PROMPTS/artifacts/G6.30/capability_register.json` classifies every
declared tool id as `SHIPPED`, `DECLARED`, or `UNDECIDABLE`, with a `--gate` mode that exits 2 when a
demo script references a `DECLARED` id. **Run the gate on your path before you touch the studio.**
Any step the gate rejects is removed from the path, not fixed here.

**Where the studio surface lives — verified.** 60 `.slint` files under
`eustress/crates/engine/ui/slint/`, of which `main.slint` is 4,021 lines. The Rust side is
`eustress/crates/engine/src/ui/` (28 files), and `slint_ui.rs` there is **23,103 lines** — the
largest file in the repository and the largest structural liability. The pattern is a `SlintAction`
queue drained by a single system (`SlintSystems::Drain`). Note that
`eustress/crates/engine/src/ui/slint_main.rs` is **dead code** — there is no `mod slint_main;`
declaration anywhere, so edits there change nothing. Always confirm a `mod` declaration before
trusting an edit.

**The failure class you are hunting.** In the drain system, a single missing required parameter
silently kills *all* UI clicks — the action fails validation, the drain skips, and every button
appears inert with no error surfaced to the user. Logs contain the string `failed validation` when
this happens; grep for it. This is the highest-probability cause of a dead control and it is
invisible to every existing test.

**Known UX debt on the surface, from `docs/AUDIT/02_STUDIO_ENGINE.md`.** The Properties panel **does
not persist edits in the default build** — legacy TOML write-back sits behind an opt-in `toml` cargo
feature and the Fjall mirror writes only `Transform`. Multiplayer Studio is 80% UI and 0% wired.
Selection model, panel suite, class serialisation, play-in-editor, and the engine bridge are all
partial. If your demo path includes editing a property and expecting it to persist, it will fail —
either exclude it or scope it to `Transform`.

**Choosing the path.** Pick exactly one vertical and one narrative, and choose the one with the
highest `SHIPPED` density in the register. Candidates grounded in the tree: the V-Cell simulation
loop described in `docs/architecture/VCELL_CASE_STUDY.md` (baseline → detection → agent repair →
verification → git feedback, with a metrics checklist at its §"Metrics Checklist"); or a CAD path
using the shipped tools `cad_create_part`, `cad_add_feature`, `cad_describe_part`,
`cad_validate_part`, `cad_measure`, `cad_export_glb`
(`eustress/crates/tools/src/cad_tools.rs` lines 63, 1498, 615, 802, 1006, 328). **Do not pick a
government or manufacturing path.** `docs/architecture/GOVERNMENT_MODE.md` §9 records one handler out
of 562 ids; `eustress/crates/engine/src/manufacturing/mod.rs` (529 lines) declares a registry whose
data root `docs/manufacturing/` does not exist, so it has zero investors, zero manufacturers, zero
deals.

**Build reality.** Engine builds are 10–15 minutes. One cargo build at a time; the workspace shares
`eustress/target/`. Never kill a build mid-compile. Validate with `cargo run`, not `cargo check` —
this item is specifically about behaviour that only appears in a running studio. Drive the studio the
way a user would; do not assert on internal state you reached by a back door.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/G6.31/` — create; holds `demo_path_sweep.json` and the declared path
- `docs/PROMPTS/harness/recipes/B1_vertical_demo_path.json` — create; the capture recipe
- `eustress/crates/engine/src/ui/` — **at most three distinct call sites**, only to fix a dead
  control found on the path
- `eustress/crates/engine/ui/slint/` — only the specific component hosting a dead control on the path

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.30/capability_register.json` — a frozen input
- `eustress/crates/engine/src/ui/slint_main.rs` — dead code; editing it changes nothing
- Anything under `eustress/crates/common/src/physics/`
- Any panel not on the declared path. Fixing an off-path control is scope creep that costs a build.
- The layout of `eustress/crates/engine/src/ui/` — the directory is owned by `G6.04`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Shortening the path after finding a dead control, redefining "control" to exclude the broken one,
  declaring a control "informational" so it needs no action, or reporting only the controls that
  worked are all measurement changes. Removing a step because the `G6.30` gate rejected it is
  permitted and expected — that is the gate doing its job, and it must be recorded as
  `removed_by_gate`, not silently dropped.
- Every control on the path must be exercised by driving the studio as a user would: click it,
  observe the result, record what happened. Do not assert on an internal function call.
- When a control appears inert, grep the engine log for `failed validation` before assuming anything
  else. That is the known drain-skip signature.
- Do not fix off-path controls. Do not refactor `slint_ui.rs`.
- Batch verification: six builds total, so one build must validate every fix you found in a pass.

## 5. Exit criterion

### Criterion
The declared path has **at least 12** controls; **0** controls classified `DEAD` (renders, accepts a
click, produces no observable effect and no error); **100%** of controls carry a recorded observed
outcome; and the `G6.30` gate returns exit 0 on the final path script.

### Measurement

Command:

    python scripts/gen_capability_register.py --gate \
        --register docs/PROMPTS/artifacts/G6.30/capability_register.json \
        --demo-script docs/PROMPTS/artifacts/G6.31/demo_path.json
    echo "GATE_EXIT=$?"

    python docs/PROMPTS/harness/checkers/sweep_check.py \
        --sweep docs/PROMPTS/artifacts/G6.31/demo_path_sweep.json \
        --min-controls 12 \
        --max-dead 0 \
        --require-outcome-on-all
    echo "SWEEP_EXIT=$?"

Expected output shape:

    controls=17 ok=15 removed_by_gate=2 dead=0 unobserved=0
    drain_validation_failures_in_log=0
    GATE_EXIT=0
    SWEEP_EXIT=0

Pass condition:

    GATE_EXIT == 0  AND  SWEEP_EXIT == 0
    AND controls >= 12  AND  dead == 0  AND  unobserved == 0

Every `ok` entry must carry an `observed_effect` string describing what changed in the studio — a
spawned entity, a changed value, a written file, a new panel state. "No error" is not an observed
effect and the checker must reject it as an outcome.

## 6. Critic gate

Gated on **D4 (UI craftsmanship)** and **D6 (overall coherence)**, floor **8.0 each**. The mean is
irrelevant — either dimension below 8.0 fails the item.

Capture recipe: `docs/PROMPTS/harness/recipes/B1_vertical_demo_path.json`, which this item authors.
It must capture the path as a frame sequence at each control interaction, so the Critic sees the
studio as the partner would.

D4 is the gate because a path with zero dead controls can still read as assembled — inconsistent
spacing, states that give no feedback, actions that succeed silently. D6 is the gate because a demo
whose steps each work but do not compose into one narrative is the exact experience that reads as
"capable parts that never met".

The Critic never sees your self-report, your commit messages, or any caption. Every score it gives
cites a specific frame. Design the path so its coherence is visible in the frames alone.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — declare the path, gate it, sweep it, fix the dead controls found
   -> if still failing, MANDATORY approach change. Removing one more step is NOT an approach change;
      switching the vertical to the one with higher SHIPPED density in the register is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the worst gated dimension moving < 0.5 AND
                  `dead` moving < 1
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: > 3 distinct edit sites needed in slint_ui.rs (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G6.31/demo_path_sweep.json`

A reader finds: `path_name`, `vertical`, `narrative` (one sentence), `commit`, `build_command`, and a
`controls` array in path order. Each entry carries `step`, `panel`, `control_label`, `tool_id`
(nullable), `register_class` from `G6.30`, `action_taken`, `observed_effect`, `status`
(`OK` | `DEAD` | `REMOVED_BY_GATE`), `log_validation_failures`, and `frame_ref` into the capture
bundle.

`docs/PROMPTS/artifacts/G6.31/demo_path.json` is the machine-readable path script that the `G6.30`
gate consumes and that `G6.33` and `G6.34` reference as the thing a partner is actually shown.

## 9. Definition of NOT done

- The path was shortened until nothing dead remained. Twelve controls is the floor precisely so that
  the path cannot shrink to a single working button.
- A control is recorded `OK` with `observed_effect: "no error"`. Absence of an error is not evidence
  of an effect, and the drain-skip failure class produces exactly that appearance.
- The path includes editing a Properties field other than `Transform` and expecting persistence.
  `docs/AUDIT/02_STUDIO_ENGINE.md` records that the Properties panel does not persist edits in the
  default build.
- The path is a government or manufacturing narrative. One handler out of 562 government ids, and a
  manufacturing registry whose data root does not exist — both are the two verticals where a dead
  button ends a deal, and both currently guarantee one.
- Edits were made to `eustress/crates/engine/src/ui/slint_main.rs`. There is no `mod slint_main;`
  anywhere; nothing changed and a build was spent proving it.
- The sweep passes but the Critic refuses the wow gate, citing that the path works while reading as a
  sequence of unrelated operations rather than one story. That is a legitimate refusal; the item is
  not done.
- `slint_ui.rs` was refactored "while in there". It is 23,103 lines, every panel drains through it,
  and a refactor here burns the build budget for an item whose deliverable is a working path.
````

---

## `G6.32` — Design-partner qualification scorecard with disqualifiers

````markdown
---
id: G6.32
title: A qualification scorecard that disqualifies a prospect on capability we do not have
workload: W6
workload_secondary: [W2, W4]
phase: G6
depends_on: [G6.30]
blocks: [G6.33]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/gtm/DESIGN_PARTNER_QUALIFICATION.md
escalation: >
  If applying the scorecard to the three named target segments disqualifies all three, STALL rather
  than relaxing a disqualifier — that outcome means the shipped surface cannot yet support any design
  partner, and the correct response is a human decision about sequencing, not a softer scorecard.
status: DRAFT
notes: >
  Tier S: a document plus a scoring script, zero compile. Scoring must be mechanical, because a
  scorecard a founder can talk himself past is not a scorecard.
---

## 1. Objective

`docs/gtm/DESIGN_PARTNER_QUALIFICATION.md` defines a scored qualification for design partners in
batteries and manufacturing, aerospace-adjacent, and life sciences, including hard disqualifiers that
fire automatically when a prospect's need maps to a capability the shipped-capability register marks
`DECLARED`. A scoring script applies it mechanically, and is demonstrated to disqualify a synthetic
prospect whose stated need is spec-only.

Passing this item does **not** advance `D4`. `00_MASTER_PROTOCOL.md` §1.1 defines `D4` as at least
one paid or signed pilot whose scope is exactly what the archived artifacts demonstrate. What this
item produces is a scoring script and a synthetic prospect record — a Python checker validating a
fixture the same agent authored. It proves that the script disqualifies the prospect its own
author constructed to be disqualified. That is worth having: an instrument that cannot fail on its
own author's input would be worse than useless, and the fixture is the negative control that shows
the checker discriminates. But a synthetic fixture is never the outcome; it is the test of the
instrument that will later meet a real one.

The item that turns this instrument on external reality is **`G0.08`** — five real, non-synthetic,
unacquainted, consented discovery records from strangers traceable to a declared arrival channel
(`G0.09`). Until `G0.08` passes, nothing here has met a person. And `D4` itself closes only on an
executed agreement, which `00_MASTER_PROTOCOL.md` §6 makes a human action that no agent may take.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
licence is PolyForm Shield 1.0.0 — say **source-available**, never open source. Its Noncompete clause
bars providing any product competing with the software, including free products, and its New Products
clause freezes an adopter at the versions available when competition begins. That matters here: a
prospect who intends to resell a simulation service built on Eustress is a licensing conversation,
not a design partner, and the scorecard must ask.

**What is actually sellable today, verified.** The desktop studio, the MCP tool surface, CAD Tier 1
(`cad_describe_part`, `cad_validate_part`, `cad_measure` at
`eustress/crates/tools/src/cad_tools.rs` lines 615, 802, 1006), and the usage-telemetry pipeline
(engine outbox → `api.eustress.dev` Worker → KV, live end to end). Everything else in the commercial
surface is blocked: per `docs/AUDIT/09_ECONOMY.md` the Bliss balance ledger is 🔴 0% so no currency
can move, Steam IAP is 🔴 5%, Stripe Connect is effectively 🔴 0% gated on KYC, and per
`docs/AUDIT/08_IDENTITY_TRUST.md` the Cloudflare Worker `JURISDICTIONS` dict is **empty** so the "72
countries" claim is spec-only. `LAUNCH_PLAN.md` Stream 2 has the Nevada LLC "In progress (forming)"
and Stream 3 banking "Blocked (needs EIN)". **The only near-term revenue instruments are a negotiated
commercial licence (`LICENSE-COMMERCIAL.md`, which has no published price) and a paid pilot
engagement.** The scorecard must not qualify a prospect whose purchase requires a rail that does not
exist.

**What the operator can actually deliver.** One person, bootstrapped, on Windows, with 10–15 minute
serialized builds and a shared `eustress/target/` that forbids parallel builds. There is no on-call,
no SLA, no Prometheus or Grafana, no DR runbook, no SOC 2 or ISO 27001 posture (Vault is at 0% and the
Consul directory is absent per `docs/AUDIT/12_INFRASTRUCTURE.md`), no WCAG AA conformance (which
`docs/AUDIT/06_WEBSITE.md` Feature 15 records as blocking government and enterprise sales), and no
signed binaries (Windows authenticode 0%, macOS notarisation 0%, so every install shows a SmartScreen
or Gatekeeper warning). A prospect requiring any of those is disqualified today, and saying so early
is worth more than a quarter of hope.

**What `G6.30` gave you.** `docs/PROMPTS/artifacts/G6.30/capability_register.json` with a `--gate`
mode. The scorecard's capability disqualifier calls that gate — it does not maintain its own list.

**Three target segments, and what is honestly available for each.**
- *Batteries and manufacturing.* The V-Cell model is real but lumped 0-D with no spatial
  electrochemistry or ion transport and decoupled thermal state (`docs/AUDIT/19_REALISM_PHYSICS.md`),
  with a declared validity envelope from `G2.32`. The Manufacturing Program
  (`docs/development/MANUFACTURING_PROGRAM.md`, `MANUFACTURING_DEAL_STRUCTURE.md`) is spec plus a
  529-line registry at `eustress/crates/engine/src/manufacturing/mod.rs` whose declared data root
  `docs/manufacturing/` does not exist — zero investors, zero manufacturers, zero deals.
- *Aerospace-adjacent.* CAD Tier 1 plus STEP interoperability from `G2.34`/`G2.35`. Note there is no
  FEA, no CFD, and `fracture_mesh.rs` has no integration path to Avian — it is visualisation only.
- *Life sciences.* `eustress/crates/common/src/realism/laws/biology/` exists; treat it as unproven
  until it appears in a `G2.30` or `G2.31` case with a measured tolerance.

**This item produces an instrument, not a signed partner.** Approaching, negotiating with, or
committing to a real company is a human action. Never contact anyone.

## 3. Scope

### In scope — files this item may edit
- `docs/gtm/DESIGN_PARTNER_QUALIFICATION.md` — create
- `docs/gtm/qualification_rubric.json` — create; the machine-readable scoring definition
- `docs/PROMPTS/harness/checkers/qualify.py` — create
- `docs/PROMPTS/harness/checkers/fixtures/prospect_synthetic_*.json` — create; clearly labelled
  synthetic prospects

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.30/capability_register.json` — a frozen input
- `LICENSE`, `LICENSE-COMMERCIAL.md` — licence terms are a human decision
- `docs/marketing/UofA_Center_For_Innovation_Pilot.html` — a live partner-facing asset

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Weakening a disqualifier so a desirable segment survives, scoring a `DECLARED` capability as
  partial credit, or adding a "founder override" field are measurement changes.
- Every scored dimension must be answerable from a discovery conversation with a yes/no or a number.
  A dimension requiring judgement about "fit" is not scorable and does not belong.
- No fabricated companies, people, or logos. Fixtures must carry `"synthetic": true` and a name that
  cannot be mistaken for a real organisation.
- The capability disqualifier must call the `G6.30` gate rather than embedding its own list.
- Do not contact anyone. Do not draft outreach in this item.

## 5. Exit criterion

### Criterion
The rubric defines **at least 10** scored dimensions and **at least 5** hard disqualifiers, of which
one is capability-based (calls the `G6.30` gate), one is rail-based (requires a payment rail that does
not exist), and one is licence-based (the prospect intends to provide a competing product). The
scorer must disqualify the spec-only synthetic prospect with **exit 3** and qualify the shipped-need
synthetic prospect with **exit 0**.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/qualify.py \
        --rubric docs/gtm/qualification_rubric.json \
        --register docs/PROMPTS/artifacts/G6.30/capability_register.json \
        --prospect docs/PROMPTS/harness/checkers/fixtures/prospect_synthetic_shipped.json
    echo "GOOD_EXIT=$?"

    python docs/PROMPTS/harness/checkers/qualify.py \
        --rubric docs/gtm/qualification_rubric.json \
        --register docs/PROMPTS/artifacts/G6.30/capability_register.json \
        --prospect docs/PROMPTS/harness/checkers/fixtures/prospect_synthetic_speconly.json
    echo "SPEC_EXIT=$?"

    python docs/PROMPTS/harness/checkers/qualify.py \
        --rubric docs/gtm/qualification_rubric.json --describe
    echo "DIMENSIONS_LISTED=$?"

Expected output shape:

    dimensions=12 disqualifiers=6
    prospect=SYNTHETIC-ALPHA score=41/60 disqualifiers_fired=0 -> QUALIFIED
    GOOD_EXIT=0
    prospect=SYNTHETIC-BETA score=48/60 disqualifiers_fired=1
      DQ: capability_not_shipped (need maps to 'gov:budget_scenario' = DECLARED)
    -> DISQUALIFIED
    SPEC_EXIT=3
    DIMENSIONS_LISTED=0

Pass condition:

    GOOD_EXIT == 0  AND  SPEC_EXIT == 3  AND  dimensions >= 10  AND  disqualifiers >= 5
    AND the spec-only prospect is disqualified DESPITE a higher raw score

The last clause is the point: a disqualifier must override score. A prospect who is exciting on every
dimension and needs something that does not exist is still a no.

## 6. Critic gate

`critic_gate` is `[]`. Correctness is decided by the two-sided scorer test plus the structural
minimums on dimensions and disqualifiers. The compensating tightness: three specifically required
disqualifier types, the requirement that the capability disqualifier delegates to the `G6.30` gate
rather than duplicating it, and the requirement that a disqualifier overrides a higher raw score.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — weighted-dimension score plus veto-style disqualifiers
   -> if still failing, MANDATORY approach change. Reweighting is NOT an approach change; moving to
      a staged gate (disqualify first, score only survivors) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where SPEC_EXIT != 3
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: all three target segments disqualify (see front matter)
```

## 8. Artifact

`docs/gtm/DESIGN_PARTNER_QUALIFICATION.md`

A reader finds: the three target segments with what is honestly available in each today; the scored
dimension table (dimension, question asked in discovery, answer type, weight); the hard-disqualifier
list with the rule each encodes; the statement that a disqualifier overrides any score; and the
explicit list of things Eustress cannot supply today — SLA, on-call, SOC 2, WCAG AA conformance,
signed binaries, and any flow requiring a live payment rail.

`docs/gtm/qualification_rubric.json` is the machine-readable twin that `qualify.py` consumes.

## 9. Definition of NOT done

- The scorecard has no capability disqualifier, so a prospect can qualify on a need the register
  marks `DECLARED`. That is the single failure mode this pack exists to prevent.
- A "founder override" or "strategic exception" field exists. A scorecard you can talk past is a
  narrative device, not an instrument.
- Fixtures use plausible real company names. Synthetic means unmistakably synthetic.
- The document lists what Eustress can do but not what it cannot supply. The cannot-supply list is
  what makes an early no possible, and an early no is the second-most valuable outcome of discovery.
- Dimensions require judgement ("cultural fit", "innovation appetite") rather than a discoverable
  answer. Those cannot be scored consistently by one person across a quarter.
- The item drafted outreach, named a real target, or contacted anyone. Approaching a company is a
  human action.
````

---

## `G6.33` — Discovery instrument that extracts a quantified problem

````markdown
---
id: G6.33
title: A discovery instrument whose output is a quantified problem with a stated counterfactual
workload: W6
workload_secondary: [W2, W4]
phase: G6
depends_on: [G6.32, G6.31]
blocks: [G6.34]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/gtm/DISCOVERY_INSTRUMENT.md
escalation: >
  If the extractor cannot produce a complete quantified-pain record from the worked synthetic call
  without inventing a number, STALL — an instrument that requires the interviewer to supply the
  quantification has moved the hard part back onto the founder and will not survive a real call.
status: DRAFT
notes: >
  Tier S. The instrument is a script plus an extractor; it is validated against synthetic call
  records, never against fabricated real customers. Running a real discovery call is a human action.
---

## 1. Objective

`docs/gtm/DISCOVERY_INSTRUMENT.md` is a discovery script in which every question maps to a required
field of a structured call record, and an extractor validates that a completed record contains at
least one quantified pain — a number, a unit, a verbatim source quote, and an explicit counterfactual
("what happens today instead"). The extractor rejects a record whose pain has no counterfactual.

Passing this item does **not** advance `D4`. `00_MASTER_PROTOCOL.md` §1.1 defines `D4` as at least
one paid or signed pilot whose scope is exactly what the archived artifacts demonstrate. What this
item produces is an extractor and a synthetic call record — a Python checker validating a fixture
the same agent authored. It proves that the extractor rejects the record its own author
constructed to be rejected. That is worth having: an instrument that cannot fail on its own
author's input would be worse than useless, and the fixture is the negative control that shows the
checker discriminates. But a synthetic fixture is never the outcome; it is the test of the
instrument that will later meet a real one.

The item that turns this instrument on external reality is **`G0.08`** — five real, non-synthetic,
unacquainted, consented discovery records from strangers traceable to a declared arrival channel
(`G0.09`). Until `G0.08` passes, nothing here has met a person. And `D4` itself closes only on an
executed agreement, which `00_MASTER_PROTOCOL.md` §6 makes a human action that no agent may take.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Licence
is PolyForm Shield 1.0.0 — source-available. Units are meter-native. Physics is Avian.

**Why the counterfactual is the load-bearing field.** A case study without a counterfactual is
unfalsifiable: "we simulated 10,000 cycles" means nothing unless the reader knows what the partner
would otherwise have done and what that would have cost. Every downstream artifact in this pack —
the pilot pre-registration in `G6.34`, the promotion gates in `G6.35`, the case study in `G6.36` —
consumes the counterfactual this instrument captures. If it is not captured in discovery, it is
reconstructed later, and a reconstructed counterfactual is a guess dressed as evidence.

**What `G6.32` gave you.** `docs/gtm/DESIGN_PARTNER_QUALIFICATION.md` and
`docs/gtm/qualification_rubric.json`, with `qualify.py` applying hard disqualifiers including a
capability disqualifier that calls the `G6.30` shipped-capability gate. Every scored dimension in that
rubric is answerable from a discovery conversation — this instrument is where those answers come
from, so the field set must be a superset of the rubric's inputs.

**What is honestly demonstrable in a discovery call today.** The desktop studio, the MCP tool
surface, CAD Tier 1, the usage-telemetry pipeline, and the single demo path cleared by `G6.31` and
recorded at `docs/PROMPTS/artifacts/G6.31/demo_path.json`. Nothing else. The instrument must include
the question that surfaces whether the prospect's problem needs anything beyond that, because
answering it late is how a design partner is lost permanently.

**Model honesty you must build into the questions.** The V-Cell electrochemical model is lumped 0-D
with no spatial electrochemistry or ion transport, and its `ElectrochemicalState` and
`ThermodynamicState` are decoupled (`docs/AUDIT/19_REALISM_PHYSICS.md`). `G2.32` produced
`docs/validation/VCELL_VALIDITY_ENVELOPE.md` and `docs/validation/vcell_envelope.json` declaring the
regimes in which its results are meaningful. A discovery question must establish whether the
prospect's regime is inside that envelope, because a pilot that begins outside it cannot produce a
credible result no matter how it is run.

**Fabrication rules.** You will author a worked synthetic call record to validate the extractor. It
must carry `"synthetic": true`, a name that cannot be mistaken for a real organisation, and no real
person's name or title. Do not write a fictitious quote and attribute it to a real company. Do not
contact anyone; running a real discovery call is a human action.

## 3. Scope

### In scope — files this item may edit
- `docs/gtm/DISCOVERY_INSTRUMENT.md` — create
- `docs/gtm/call_record_schema.json` — create
- `docs/PROMPTS/harness/checkers/discovery_extract.py` — create
- `docs/PROMPTS/harness/checkers/fixtures/call_record_synthetic_complete.json` — create
- `docs/PROMPTS/harness/checkers/fixtures/call_record_synthetic_no_counterfactual.json` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/gtm/qualification_rubric.json` and `docs/gtm/DESIGN_PARTNER_QUALIFICATION.md` — frozen inputs
- `docs/validation/vcell_envelope.json` — a frozen input
- Any file under `eustress/crates/`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Making `counterfactual` optional, accepting a qualitative pain as quantified, or letting the
  extractor infer a number the interviewee did not state are measurement changes.
- Every question in the script must map to exactly one required field, and every required field must
  have a question. The extractor's `--describe` output must show a bijection.
- A quantified pain requires all four of: `value`, `unit`, `source_quote` (verbatim, from the record),
  and `counterfactual`. Three of four is a rejection.
- Never invent a number. If the interviewee did not quantify it, the field is null and the record is
  incomplete — that is a true result about the call, not a defect in the instrument.
- No real organisations, no real people, no fabricated attributions.

## 5. Exit criterion

### Criterion
The instrument defines **at least 14** questions with a **1:1** mapping to required record fields;
the extractor accepts the complete synthetic record with **exit 0** and reports **at least 1**
quantified pain with all four sub-fields populated; and it rejects the no-counterfactual record with
**exit 2**, naming `counterfactual` as the missing field.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/discovery_extract.py --describe \
        --instrument docs/gtm/DISCOVERY_INSTRUMENT.md \
        --schema docs/gtm/call_record_schema.json
    echo "MAPPING_EXIT=$?"

    python docs/PROMPTS/harness/checkers/discovery_extract.py \
        --schema docs/gtm/call_record_schema.json \
        --record docs/PROMPTS/harness/checkers/fixtures/call_record_synthetic_complete.json \
        --require-quantified-pain 1
    echo "GOOD_EXIT=$?"

    python docs/PROMPTS/harness/checkers/discovery_extract.py \
        --schema docs/gtm/call_record_schema.json \
        --record docs/PROMPTS/harness/checkers/fixtures/call_record_synthetic_no_counterfactual.json \
        --require-quantified-pain 1
    echo "BAD_EXIT=$?"

Expected output shape:

    questions=16 required_fields=16 unmapped_questions=0 unmapped_fields=0
    MAPPING_EXIT=0
    record=SYNTHETIC-ALPHA synthetic=true
    quantified_pains=2
      pain[0] value=6 unit=weeks_per_design_iteration quote_len=88 counterfactual=present
      pain[1] value=180000 unit=usd_per_scrapped_build counterfactual=present
    envelope_question_answered=true
    GOOD_EXIT=0
    record=SYNTHETIC-GAMMA  quantified_pains=0
      REJECT pain[0]: missing field 'counterfactual'
    BAD_EXIT=2

Pass condition:

    MAPPING_EXIT == 0  AND  GOOD_EXIT == 0  AND  BAD_EXIT == 2
    AND questions >= 14  AND  unmapped_questions == 0  AND  unmapped_fields == 0
    AND quantified_pains_on_good >= 1 with all four sub-fields populated
    AND the rejection names 'counterfactual' specifically

## 6. Critic gate

`critic_gate` is `[]`. The instrument's quality is decided by the bijection check plus the two-sided
extractor test. The compensating tightness: a minimum question count, a strict 1:1 question-to-field
mapping in both directions, a four-part definition of quantified pain, a required envelope question,
and a rejection that must name the specific missing field rather than failing generically.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — question list authored first, schema derived from it
   -> if still failing, MANDATORY approach change. Adding questions is NOT an approach change;
      inverting so the schema is authored first from the G6.32 rubric inputs and the questions are
      generated to fill it is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with unmapped_fields > 0 and unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: a complete record cannot be produced without inventing a number
```

## 8. Artifact

`docs/gtm/DISCOVERY_INSTRUMENT.md`

A reader finds: the call structure with time budget; every question in order, each annotated with the
record field it fills and why that field is needed downstream; the quantification follow-ups that
convert a qualitative complaint into a number with a unit; the counterfactual question in its
required form ("what do you do today instead, and what does that cost you"); the envelope question
that establishes whether the prospect's regime is inside `docs/validation/vcell_envelope.json`; the
capability question that surfaces any need outside the `G6.31` demo path; and the rule that a null
field is a valid, honest outcome.

`docs/gtm/call_record_schema.json` is the machine-readable twin. The two synthetic fixtures are the
worked examples and the negative control.

## 9. Definition of NOT done

- `counterfactual` is optional, or a pain passes with three of four sub-fields. Everything downstream
  — pre-registration, promotion gates, the case study — consumes it, and reconstructing it later
  produces a guess dressed as evidence.
- The extractor infers a number from a qualitative statement. "It takes forever" is not six weeks,
  and an instrument that guesses will produce a case study a partner refuses to be quoted in.
- The script has questions with no corresponding field, or fields no question fills. Either direction
  breaks the instrument: the first wastes call time, the second produces incomplete records.
- The synthetic fixture names a plausible real company, or attributes an invented quote to a real
  organisation.
- The envelope question is missing, so a pilot can be scoped in a regime the V-Cell model explicitly
  does not resolve. That is discovered at the end of the pilot instead of the start.
- The item drafted outreach or simulated a real call as though it had happened. Running a discovery
  call is a human action; this item builds the instrument.
````

---

## `G6.34` — Pre-registered pilot design, hashed before first data

````markdown
---
id: G6.34
title: A pilot whose success metric is agreed and hashed before any pilot data exists
workload: W6
workload_secondary: [W2, W3, W4]
phase: G6
depends_on: [G6.33, G2.33]
blocks: [G6.35, G6.36]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/gtm/PILOT_PREREGISTRATION.md
escalation: >
  Countersignature by a real design partner is a human action and must never be performed or
  simulated by an agent. If the item cannot be completed without a countersigned document, STALL and
  hand the template to the human — the template, the checker, and the ordering proof are the agent
  deliverable.
status: DRAFT
notes: >
  Tier S. The mechanism is what makes a case study credible: a metric chosen after seeing the data is
  not a result, and the only defence is a timestamped, hashed pre-registration.
---

## 1. Objective

A pilot pre-registration template exists in which the success metric, its threshold, its measurement
command, and the counterfactual are fixed and content-hashed before any pilot data is collected, and
a checker proves the ordering: the pre-registration's commit must strictly precede the first
pilot-data commit, and its recorded hash must match the document at read time.

Passing this item does **not** advance `D4`. `00_MASTER_PROTOCOL.md` §1.1 defines `D4` as at least
one paid or signed pilot whose scope is exactly what the archived artifacts demonstrate. What this
item produces is an ordering checker and a synthetic pre-registration — a Python checker
validating a fixture the same agent authored. It proves that the checker reads two commit
timestamps in the order its own author committed them. That is worth having: an instrument that
cannot fail on its own author's input would be worse than useless, and the fixture is the negative
control that shows the checker discriminates. But a synthetic fixture is never the outcome; it is
the test of the instrument that will later meet a real one.

The item that turns this instrument on external reality is **`G0.08`** — five real, non-synthetic,
unacquainted, consented discovery records from strangers traceable to a declared arrival channel
(`G0.09`). Until `G0.08` passes, nothing here has met a person. And `D4` itself closes only on an
executed agreement, which `00_MASTER_PROTOCOL.md` §6 makes a human action that no agent may take.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Licence
is PolyForm Shield 1.0.0 — source-available. Physics is Avian. Units are meter-native.

**Why pre-registration is the whole mechanism.** A pilot whose success metric is chosen after the
results arrive proves nothing, and a sophisticated buyer knows it. The credibility of every case
study this pack produces rests on being able to show a stranger that the metric was fixed first. That
requires three things a document alone cannot provide: a content hash, a timestamp that cannot be
back-dated by editing the file, and a checkable ordering against the data.

**What you can rely on for ordering.** The repository is a git repository on branch `main`. Commit
timestamps and commit ordering are available via `git log`. A pre-registration committed before the
first data commit, with its SHA-256 recorded in the commit that introduces it, gives an outside
reviewer a verifiable ordering without any external service.

**Inputs from earlier items.**
- `docs/gtm/DISCOVERY_INSTRUMENT.md` and `docs/gtm/call_record_schema.json` (`G6.33`) — the
  counterfactual and the quantified pain the pilot metric must be derived from.
- `docs/validation/reports/VALIDATION_REPORT_v1.md` and `docs/validation/regenerate.py` (`G2.33`) —
  the pilot's measurement command must conform to `docs/validation/PROOF_STANDARD.md` and must be
  runnable by the partner, not only by Eustress.
- `docs/validation/vcell_envelope.json` (`G2.32`) — the pilot must be scoped inside the declared
  validity envelope, or explicitly declare that it runs outside it and what that costs.
- `docs/PROMPTS/artifacts/G6.31/demo_path.json` (`G6.31`) — the pilot may only depend on capability
  on the cleared demo path.

**What is honestly available to promise in a pilot.** The desktop studio, the MCP tool surface, CAD
Tier 1, the usage-telemetry pipeline, STEP interoperability from `G2.34`/`G2.35`, and the validation
report from `G2.33`. Not available: any flow requiring a live payment rail (the Bliss balance ledger
is 0%, Stripe Connect is gated on KYC, and the Cloudflare Worker `JURISDICTIONS` dict is empty), any
SLA or on-call commitment, any SOC 2 or WCAG AA claim, and signed binaries (Windows authenticode 0%,
macOS notarisation 0%). The pilot terms must state that installation shows an unsigned-binary warning
— a partner discovering that on day one is a bad first hour.

**Human-only boundary.** Signing, countersigning, sending, or agreeing to any document with a real
organisation is a human action. This item produces the template, the checker, and a synthetic worked
instance. It never contacts anyone and never simulates a countersignature.

## 3. Scope

### In scope — files this item may edit
- `docs/gtm/PILOT_PREREGISTRATION.md` — create; the template and its rules
- `docs/gtm/prereg_schema.json` — create
- `docs/gtm/preregistrations/SYNTHETIC-ALPHA.json` — create; the worked synthetic instance
- `docs/PROMPTS/harness/checkers/prereg_check.py` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- All inputs named in §2 — frozen
- `LICENSE-COMMERCIAL.md` — commercial terms are a human decision
- Any real partner document. There are none, and you may not create one.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Allowing the metric to be amended after data exists, making the hash advisory, permitting a
  "clarification" field that redefines the threshold, or letting the checker pass when ordering
  cannot be established are measurement changes.
- The success metric must be a single number with a comparator and a literal measurement command
  that the **partner** can run. A metric only Eustress can measure is not a pilot metric.
- The pre-registration must name what would count as failure, explicitly, before the pilot starts.
  A pre-registration with no failure condition is a press release.
- Amendments are permitted but must be additive and separately hashed, and the checker must report
  amendment count. Silently editing the original must fail the hash check.
- Never simulate a countersignature. The synthetic instance is marked `"synthetic": true` and
  `"countersigned": false`.

## 5. Exit criterion

### Criterion
The schema requires **at least 9** fields including `success_metric`, `threshold`, `comparator`,
`measurement_command`, `failure_condition`, `counterfactual`, `envelope_scope`, `content_sha256`, and
`prereg_commit`; the checker verifies that the recorded hash matches the document, that
`prereg_commit` strictly precedes every commit listed in `data_commits`, and exits **2** when given a
tampered instance and **2** when given an instance whose data commit precedes the pre-registration.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/prereg_check.py \
        --schema docs/gtm/prereg_schema.json \
        --prereg docs/gtm/preregistrations/SYNTHETIC-ALPHA.json \
        --repo . --verify-hash --verify-ordering
    echo "GOOD_EXIT=$?"

    python docs/PROMPTS/harness/checkers/prereg_check.py \
        --schema docs/gtm/prereg_schema.json \
        --prereg docs/PROMPTS/harness/checkers/fixtures/prereg_tampered.json \
        --repo . --verify-hash
    echo "TAMPER_EXIT=$?"

    python docs/PROMPTS/harness/checkers/prereg_check.py \
        --schema docs/gtm/prereg_schema.json \
        --prereg docs/PROMPTS/harness/checkers/fixtures/prereg_data_first.json \
        --repo . --verify-ordering
    echo "ORDER_EXIT=$?"

Expected output shape:

    fields_required=11 fields_present=11
    content_sha256 matches document: true
    prereg_commit=71ccf6fe (2026-08-05T18:22:04Z)
    data_commits=3 earliest=2026-08-09T09:01:55Z  ordering_ok=true
    amendments=0
    GOOD_EXIT=0
    HASH MISMATCH: recorded 9f2c... document 41ab...
    TAMPER_EXIT=2
    ORDERING VIOLATION: data commit 2026-08-04 precedes prereg commit 2026-08-05
    ORDER_EXIT=2

Pass condition:

    GOOD_EXIT == 0  AND  TAMPER_EXIT == 2  AND  ORDER_EXIT == 2
    AND fields_required >= 9  AND  ordering_ok == true on the good instance

Both negative controls are required. A pre-registration mechanism that has never been shown to detect
tampering or reordering is a formatting convention, not a guarantee.

## 6. Critic gate

`critic_gate` is `[]`. The mechanism is cryptographic and temporal, and both properties are decided by
exit codes. The compensating tightness: a minimum required-field set naming the specific fields, a
mandatory failure condition, a hash verification against the live document, an ordering proof against
git history, and two independent negative controls.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — JSON pre-registration with SHA-256 over the canonicalised document,
                 ordering verified via git log
   -> if still failing, MANDATORY approach change. Changing the hash algorithm is NOT an approach
      change; moving to a signed commit or a git-note-anchored record is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where TAMPER_EXIT != 2
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: completion would require a countersigned document (see front matter)
```

## 8. Artifact

`docs/gtm/PILOT_PREREGISTRATION.md`

A reader finds: why pre-registration exists, in one paragraph, phrased for the partner rather than
for us; the required-field table; the rule that the metric must be measurable by the partner with a
literal command; the mandatory failure condition; the amendment rule (additive, separately hashed,
counted); the ordering rule and how a stranger verifies it with `git log`; the scope rule tying the
pilot to the `G6.31` demo path and the `G2.32` validity envelope; and the disclosure block covering
unsigned binaries, no SLA, and no payment rail.

`docs/gtm/preregistrations/SYNTHETIC-ALPHA.json` is the worked instance, marked synthetic and
uncountersigned. Real pre-registrations land in the same directory once a human signs them.

## 9. Definition of NOT done

- The template exists but nothing verifies ordering, so a pre-registration written after the results
  is indistinguishable from one written before. The ordering proof is the entire value.
- The hash is recorded but never checked against the live document, so an edit after the fact goes
  unnoticed.
- There is no failure condition. A pre-registration that can only succeed is a press release, and a
  sophisticated partner will read it as one.
- The measurement command can only be run by Eustress. The partner must be able to reproduce the
  number, or the case study rests on our word.
- An agent produced a countersigned document, a partner name, or a signature block filled in. Signing
  is a human action and a fabricated countersignature is a fabricated record.
- The pilot scope includes capability outside the `G6.31` cleared demo path or outside the `G2.32`
  validity envelope, so the pilot is committed to something that cannot be delivered credibly.
````

---

## `G6.35` — Simulation-to-physical promotion gates

````markdown
---
id: G6.35
title: Explicit numeric gates for promoting a simulated result to a physical build decision
workload: W4
workload_secondary: [W3, W1]
phase: G6
depends_on: [G2.32, G6.34]
blocks: [G6.36]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/gtm/SIM_TO_PHYSICAL_GATES.md
escalation: >
  If any gate's evidence requirement cannot be satisfied by an artifact that exists in this
  repository today, mark that gate BLOCKED with the named missing artifact and STALL rather than
  writing a gate nothing can pass. A ladder whose first rung is unreachable will simply be skipped.
status: DRAFT
notes: >
  Tier S. This is the artifact that converts "the simulation says so" into a defensible engineering
  decision, and the artifact a design partner's own engineers will scrutinise hardest.
---

## 1. Objective

`docs/gtm/SIM_TO_PHYSICAL_GATES.md` defines an ordered ladder of gates, each with a numeric criterion
and a named evidence artifact, that a simulated result must clear before it is allowed to justify a
physical build, a tooling order, or a test-article commitment. A checker evaluates a candidate result
against the ladder and reports the highest gate cleared, refusing to advance past a gate whose
evidence artifact is absent.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. The
gold-collar promise is that engineers validate their own models inside it — which only means anything
if there is a stated rule for when a simulated result is allowed to change a physical decision.
Licence is PolyForm Shield 1.0.0 — source-available. Physics is Avian. Units are meter-native.

**What earlier items give each gate its evidence.**
- `docs/PROMPTS/artifacts/G2.30/analytic_validation.json` — laws vs closed-form solutions, ≥12 cases,
  declared tolerances.
- `docs/PROMPTS/artifacts/G2.31/reference_validation.json` — laws vs published references with
  citations, acceptance bands, and out-of-band causes.
- `docs/validation/vcell_envelope.json` and `docs/validation/VCELL_VALIDITY_ENVELOPE.md` — the
  declared regimes in which the V-Cell model's outputs are meaningful, plus a run-time guard that
  marks an out-of-envelope run untrusted.
- `docs/validation/reports/VALIDATION_REPORT_v1.md` plus `docs/validation/regenerate.py` — a report a
  stranger regenerates from a clean clone.
- `docs/gtm/preregistrations/*.json` — the pre-registered metric, hashed before data.

**The two honesty problems every gate must account for.**
1. *The model is lumped.* `docs/AUDIT/19_REALISM_PHYSICS.md` records V-Cell Nernst and Butler–Volmer
   as **lumped 0-D models** with no spatial electrochemistry or ion transport, and
   `ElectrochemicalState` and `ThermodynamicState` as **decoupled** — no thermal effect on reaction
   rate. It also records that `fracture_mesh.rs` has no integration path to Avian and is
   visualisation only. A gate that permits a physical commitment on a failure mode the model cannot
   resolve is worse than no gate.
2. *Compressed time can silently discard steps.* `eustress/crates/common/src/simulation/clock.rs`
   advances `simulation_time_s` by the full compressed delta while capping physics ticks at
   `max_ticks_per_frame` (default 10) and **zeroing the accumulator on saturation** (lines 100–102).
   At high `time_scale` the clock reports compressed time while the law steps that would have covered
   it are discarded — no error, no counter. Every "N cycles simulated" claim depends on this, so a
   gate must require that the run's `time_scale` was inside the declared envelope and that the run
   is marked trusted by the `G2.32` guard.

**Determinism is not yet established across runs on different machines.**
`docs/AUDIT/11_SIMULATION_DEBUGGER.md` Feature 8 records "Avian deterministic step is single-run
only; cross-platform untested", and the byte-identical gate written in
`docs/architecture/HEADLESS_RUNTIME.md` is not listed as passed. A gate requiring cross-platform
reproduction is therefore `BLOCKED` today and must be marked so rather than quietly dropped.

**Suggested ladder shape (you must justify whatever you ship).** `P0` model provenance — the laws in
play appear in `G2.30` with declared tolerances. `P1` reference agreement — the relevant law appears
in `G2.31` within band, or out of band with a stated cause. `P2` envelope conformance — the run is
inside `vcell_envelope.json` and the guard marks it trusted. `P3` pre-registration — the metric was
hashed before the data, verified by `prereg_check.py`. `P4` independent reproduction — a second party
regenerates the number from a clean clone. `P5` physical correlation — one physical measurement
agrees with the simulated prediction within a stated band. Only `P5` authorises a build commitment,
and `P5` cannot be cleared from inside this repository.

**Human-only boundary.** Authorising a physical build, a tooling order, or a test article is a human
decision. This item defines the gates and the checker; it never issues a promotion.

## 3. Scope

### In scope — files this item may edit
- `docs/gtm/SIM_TO_PHYSICAL_GATES.md` — create
- `docs/gtm/promotion_gates.json` — create; the machine-readable ladder
- `docs/PROMPTS/harness/checkers/promotion_check.py` — create
- `docs/PROMPTS/harness/checkers/fixtures/candidate_*.json` — create; synthetic candidate results

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- Every evidence artifact named in §2 — all frozen inputs
- `eustress/crates/common/src/simulation/clock.rs` — the step-drop defect is another item's work;
  here you only require that a run stayed inside the declared `time_scale` bound

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Merging two gates so a candidate skips one, accepting a missing evidence artifact as "not
  applicable", or letting a gate pass on a run the `G2.32` guard marked untrusted are measurement
  changes.
- Every gate must name a specific evidence artifact path and a numeric criterion. A gate whose
  criterion is "reviewed and approved" is not a gate.
- A gate whose evidence artifact does not exist today must be marked `BLOCKED` with the missing
  artifact named — not omitted, and not softened until it passes.
- The ladder is ordered and non-skippable: the checker must report the highest **contiguous** gate
  cleared, so clearing `P4` while failing `P2` yields `P1`.
- The document must state, in the partner's language, which failure modes the model cannot resolve and
  therefore which physical decisions this ladder must never be used to justify.

## 5. Exit criterion

### Criterion
The ladder defines **at least 5** ordered gates, **100%** with a numeric criterion and a named
evidence artifact path; the checker returns the correct highest contiguous gate for three synthetic
candidates — one clearing all available gates, one failing an intermediate gate while satisfying a
later one, and one whose run is marked untrusted by the envelope guard — and exits **2** on the
untrusted candidate.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/promotion_check.py --describe \
        --gates docs/gtm/promotion_gates.json
    echo "DESCRIBE_EXIT=$?"

    python docs/PROMPTS/harness/checkers/promotion_check.py \
        --gates docs/gtm/promotion_gates.json \
        --candidate docs/PROMPTS/harness/checkers/fixtures/candidate_full.json
    echo "FULL_EXIT=$?"

    python docs/PROMPTS/harness/checkers/promotion_check.py \
        --gates docs/gtm/promotion_gates.json \
        --candidate docs/PROMPTS/harness/checkers/fixtures/candidate_skips_p2.json
    echo "SKIP_EXIT=$?"

    python docs/PROMPTS/harness/checkers/promotion_check.py \
        --gates docs/gtm/promotion_gates.json \
        --candidate docs/PROMPTS/harness/checkers/fixtures/candidate_untrusted_run.json
    echo "UNTRUSTED_EXIT=$?"

Expected output shape:

    gates=6 with_numeric_criterion=6 with_evidence_path=6 blocked=1 (P4: cross-platform determinism)
    DESCRIBE_EXIT=0
    candidate=SYNTHETIC-FULL highest_contiguous=P3 (P4 BLOCKED, P5 requires physical measurement)
    FULL_EXIT=0
    candidate=SYNTHETIC-SKIP highest_contiguous=P1  (P2 failed: time_scale 100000 > envelope max 1000)
    SKIP_EXIT=0
    candidate=SYNTHETIC-UNTRUSTED  run trusted=false -> no gate cleared
    UNTRUSTED_EXIT=2

Pass condition:

    DESCRIBE_EXIT == 0  AND  FULL_EXIT == 0  AND  SKIP_EXIT == 0  AND  UNTRUSTED_EXIT == 2
    AND gates >= 5  AND  with_numeric_criterion == gates  AND  with_evidence_path == gates
    AND the skip candidate reports P1, NOT a higher gate

The skip case is the load-bearing test. A ladder that lets a candidate claim `P4` while failing `P2`
is a scoring rubric, not a gate ladder.

## 6. Critic gate

`critic_gate` is `[]`. Every gate is a numeric criterion over a named artifact, and the ladder's
semantics are decided by three candidate evaluations plus a structural describe check. The
compensating tightness: universal numeric criteria and evidence paths, mandatory `BLOCKED` marking
rather than omission, contiguity enforcement, and a required untrusted-run rejection.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — ordered gate list evaluated in sequence against evidence artifacts
   -> if still failing, MANDATORY approach change. Reordering gates is NOT an approach change;
      moving from artifact-presence evaluation to a claim-level evaluation where each gate binds to
      a specific claim id in the G1.30 ledger is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the skip candidate still reports a gate above P1
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: a gate's evidence artifact does not exist in the repository (see front matter)
```

## 8. Artifact

`docs/gtm/SIM_TO_PHYSICAL_GATES.md`

A reader finds: the ladder, in order, each gate with its question, its numeric criterion, its evidence
artifact path, and its status (`AVAILABLE` | `BLOCKED` with the missing artifact named); the
contiguity rule; the explicit list of failure modes the shipped models cannot resolve — spatial
electrochemistry, ion transport, concentration gradients, thermal feedback on reaction rate, dendrite
initiation site, and mechanical fracture coupling to Avian — with the statement that no gate in this
ladder authorises a physical decision resting on any of them; the `time_scale` requirement and why;
and the statement that only the physical-correlation gate authorises a build commitment and that it
cannot be cleared from inside this repository.

`docs/gtm/promotion_gates.json` is the machine-readable twin consumed by `promotion_check.py`.

## 9. Definition of NOT done

- A gate's criterion is "engineering review sign-off". That is a meeting, not a gate, and it is
  exactly the step a schedule-pressured programme skips.
- A gate whose evidence does not exist was quietly removed instead of marked `BLOCKED`. Cross-platform
  determinism is untested today; the ladder must say so rather than pretend the rung is not needed.
- Gates can be cleared out of order, so a candidate with an impressive `P4` artifact and a failing
  `P2` reports `P4`. Contiguity is the mechanism.
- A run marked untrusted by the `G2.32` envelope guard clears any gate. The guard exists precisely so
  a compressed-time run that discarded steps cannot launder itself into a build decision.
- The document omits the model's unresolvable failure modes, so a partner's engineer reads the ladder
  as a general warrant. The unresolvable list is what makes the ladder trustworthy.
- The item issued a promotion, named a real programme, or implied authorisation. Authorising a
  physical build is a human decision.
````

---

## `G6.36` — Case-study format with mandatory counterfactual

````markdown
---
id: G6.36
title: A case-study format that cannot be filled in without a stated counterfactual
workload: W6
workload_secondary: [W2, W3, W4]
phase: G6
depends_on: [G6.34, G6.35]
blocks: [G6.37]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/gtm/CASE_STUDY_FORMAT.md
escalation: >
  If a case study cannot be assembled from artifacts that exist without at least one number sourced
  only to a conversation, STALL. A case study whose headline number has no artifact behind it will be
  challenged by the first buyer who takes it seriously, and losing that exchange costs more than the
  case study earns.
status: DRAFT
notes: >
  Tier S. The linter is the deliverable as much as the format — a template with optional fields
  becomes a testimonial, and a testimonial does not survive a technical buyer.
---

## 1. Objective

`docs/gtm/CASE_STUDY_FORMAT.md` defines a hard-number case-study format in which every headline claim
carries a value, a unit, a source artifact path, a class label, and an explicit counterfactual, and a
linter rejects any case study missing any of those on any claim. A synthetic worked case study passes;
a testimonial-style draft fails with named violations.

Passing this item does **not** advance `D4`. `00_MASTER_PROTOCOL.md` §1.1 defines `D4` as at least
one paid or signed pilot whose scope is exactly what the archived artifacts demonstrate. What this
item produces is a linter, a synthetic worked case study, and a synthetic testimonial draft — a
Python checker validating a fixture the same agent authored. It proves that the linter separates
two documents its own author wrote to be separable. That is worth having: an instrument that
cannot fail on its own author's input would be worse than useless, and the fixture is the negative
control that shows the checker discriminates. But a synthetic fixture is never the outcome; it is
the test of the instrument that will later meet a real one.

The item that turns this instrument on external reality is **`G0.08`** — five real, non-synthetic,
unacquainted, consented discovery records from strangers traceable to a declared arrival channel
(`G0.09`). Until `G0.08` passes, nothing here has met a person. And `D4` itself closes only on an
executed agreement, which `00_MASTER_PROTOCOL.md` §6 makes a human action that no agent may take.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Licence
is PolyForm Shield 1.0.0 — say **source-available**, never open source. Physics is Avian. Slint is
Rust. Units are meter-native; studs are a display unit only.

**Why the counterfactual is mandatory.** "Simulated 10,000 charge cycles in 12 minutes" is not a
result. The result is: the partner would otherwise have run a physical cycling campaign taking N
weeks and costing $X, and would have discovered the same failure mode at week M — or not at all. Only
the second form is a business case, and only the second form survives a procurement review.

**The numbers you are permitted to headline, and their sources.**
- Cycles or steps simulated, and elapsed wall-clock — but **only** with the run's `time_scale` and its
  trusted flag from the `G2.32` envelope guard. The simulation clock at
  `eustress/crates/common/src/simulation/clock.rs` advances `simulation_time_s` by the full compressed
  delta while capping physics ticks at `max_ticks_per_frame` (default 10) and zeroing the accumulator
  on saturation (lines 100–102), so a compressed-time cycle count with no trusted flag is unsupported.
- Failures caught before hardware — but only when tied to a specific watchpoint or breakpoint record
  and to a gate in `docs/gtm/promotion_gates.json`.
- Time and cost saved — only against the counterfactual captured by the `G6.33` discovery instrument,
  and only when the partner supplied the baseline. A baseline we estimated is `TARGET`, not
  `MEASURED`, and must be labelled.
- Any number reproduced by `docs/validation/regenerate.py` from `G2.33`.

**The numbers you may never headline.** The 2.10M-entity figure (an `active_cap` configuration
default at `docs/AUDIT/05_SPACE_STREAMING.md` line 23), the Forge 80–90% cost reduction (unmeasured),
"72 countries" for KYC (the Cloudflare Worker `JURISDICTIONS` dict is empty), "~200 MCP tools" (the
verified figure is 79 tool descriptors in `eustress/crates/tools/src/` plus 24 bridge tools in
`eustress/crates/mcp-server/src/bridge_tools.rs` plus one hand-rolled, ~90 exposed after mode
filtering), and any government-mode capability (`docs/architecture/GOVERNMENT_MODE.md` §9: one live
handler out of 562 declared ids, and the mode "must never be described as working software").

**Inputs.** `docs/gtm/PILOT_PREREGISTRATION.md` and `docs/gtm/prereg_schema.json` (`G6.34`);
`docs/gtm/SIM_TO_PHYSICAL_GATES.md` and `docs/gtm/promotion_gates.json` (`G6.35`);
`docs/gtm/DISCOVERY_INSTRUMENT.md` and `docs/gtm/call_record_schema.json` (`G6.33`);
`docs/PROMPTS/artifacts/G1.30/claim_ledger.json` (`G1.30`) for the class vocabulary.

**Human-only boundary.** Publishing a case study naming a real organisation, or quoting a real person,
requires that organisation's approval. This item produces the format, the linter, and a synthetic
worked example marked `"synthetic": true`. Never name a real company. Never invent a quote.

## 3. Scope

### In scope — files this item may edit
- `docs/gtm/CASE_STUDY_FORMAT.md` — create
- `docs/gtm/case_study_schema.json` — create
- `docs/gtm/case_studies/SYNTHETIC-ALPHA.md` — create; the worked example
- `docs/PROMPTS/harness/checkers/case_study_lint.py` — create
- `docs/PROMPTS/harness/checkers/fixtures/case_study_testimonial.md` — create; the negative control

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- All inputs named in §2 — frozen
- `docs/marketing/UofA_Center_For_Innovation_Pilot.html` — a live partner-facing asset whose licence
  language is a human decision

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Making `counterfactual` optional for "qualitative" claims, allowing `source_artifact` to be a
  conversation, or adding a headline field the linter does not check are measurement changes.
- Every claim in a case study requires all five of: `value`, `unit`, `source_artifact` (a path that
  exists), `class` (`MEASURED` | `TARGET` | `CONFIG_DEFAULT`), and `counterfactual`. Four of five is a
  rejection.
- The linter must verify that `source_artifact` paths actually exist on disk, not merely that the
  field is non-empty.
- The format must include a "what we could not measure" section, required and non-empty.
- No real company names, no invented quotes, no fabricated logos or letterhead.

## 5. Exit criterion

### Criterion
The schema requires **5** fields per claim; the worked synthetic case study contains **at least 4**
claims, all passing, with **100%** of `source_artifact` paths resolving on disk; and the linter
rejects the testimonial-style negative control with **exit 2**, reporting **at least 3** distinct
violations including at least one missing `counterfactual` and one unresolvable `source_artifact`.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/case_study_lint.py \
        --schema docs/gtm/case_study_schema.json \
        --case-study docs/gtm/case_studies/SYNTHETIC-ALPHA.md \
        --repo-root . --verify-artifact-paths --min-claims 4
    echo "GOOD_EXIT=$?"

    python docs/PROMPTS/harness/checkers/case_study_lint.py \
        --schema docs/gtm/case_study_schema.json \
        --case-study docs/PROMPTS/harness/checkers/fixtures/case_study_testimonial.md \
        --repo-root . --verify-artifact-paths --min-claims 4
    echo "BAD_EXIT=$?"

    python docs/PROMPTS/harness/checkers/case_study_lint.py \
        --schema docs/gtm/case_study_schema.json --check-banned-claims \
        --case-study docs/gtm/case_studies/SYNTHETIC-ALPHA.md
    echo "BANNED_EXIT=$?"

Expected output shape:

    claims=5 complete=5 artifact_paths_resolved=5/5 violations=0
    could_not_measure_section=present
    GOOD_EXIT=0
    claims=2 complete=0 violations=4
      claim[0]: missing 'counterfactual'
      claim[0]: missing 'unit'
      claim[1]: source_artifact 'internal notes' does not resolve
      claim[1]: missing 'class'
    BAD_EXIT=2
    banned_claims_found=0
    BANNED_EXIT=0

Pass condition:

    GOOD_EXIT == 0  AND  BAD_EXIT == 2  AND  BANNED_EXIT == 0
    AND claims_on_good >= 4  AND  artifact_paths_resolved == claims_on_good
    AND violations_on_bad >= 3 including a missing counterfactual and an unresolvable artifact path

## 6. Critic gate

`critic_gate` is `[]`. Correctness is decided by the two-sided linter test plus the banned-claim
scan. The compensating tightness: a five-field claim requirement, on-disk verification of every source
artifact path, a required non-empty "what we could not measure" section, a minimum claim count on the
worked example, and a banned-claim list drawn from numbers this repository has verified to be
configuration defaults, unmeasured, or spec-only.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — front-matter claim array plus prose, linted against the schema
   -> if still failing, MANDATORY approach change. Adding a field is NOT an approach change;
      generating the prose from the claim array so an unlinted sentence cannot exist is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where BAD_EXIT != 2
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: the worked example needs a number with no artifact behind it (see front matter)
```

## 8. Artifact

`docs/gtm/CASE_STUDY_FORMAT.md`

A reader finds: the five required fields per claim with their definitions; the counterfactual
question in its required form and why it is not optional; the class vocabulary inherited from the
`G1.30` ledger; the rule that every `source_artifact` must resolve on disk; the banned-claim list
with the reason each entry is banned; the required "what we could not measure" section; the approval
rule (no real organisation named, no real person quoted, without that organisation's written
permission — a human step); and the structure of the narrative itself: problem, counterfactual,
intervention, pre-registered metric, result, gate cleared, and what remains unproven.

`docs/gtm/case_studies/SYNTHETIC-ALPHA.md` is the worked example.

## 9. Definition of NOT done

- `counterfactual` is optional on some claim type. The counterfactual is the business case; without
  it a case study is a testimonial and a technical buyer discounts it entirely.
- `source_artifact` accepts a free-text string, so "internal analysis" passes. Path resolution on
  disk is the check.
- A cycle-count headline appears with no `time_scale` and no trusted flag. The clock discards physics
  steps on saturation with no error, so an unqualified compressed-time cycle count is unsupported by
  construction.
- The banned-claim scan is absent or advisory, so the 2.10M-entity configuration default reappears as
  a headline in the first real case study.
- The "what we could not measure" section is present but empty. A case study with no stated limit is
  read as one that was not examined.
- A real company was named, a quote was invented, or approval was assumed. That is a fabricated
  record, and it is the one mistake in this pack that cannot be walked back.
````

---

## `G6.37` — Referenceability and expansion packet

````markdown
---
id: G6.37
title: A referenceability packet a partner can approve in one pass, plus a scored expansion path
workload: W6
workload_secondary: [W2, W4]
phase: G6
depends_on: [G6.36]
blocks: []
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/gtm/REFERENCEABILITY_PACKET.md
escalation: >
  If the packet cannot be assembled without asking a partner to approve a claim whose source artifact
  does not resolve, STALL. Sending a partner a claim we cannot substantiate is how a good reference
  becomes a bad one in a single email.
status: DRAFT
notes: >
  Tier S. Sending anything to a real partner is a human action. This item builds the packet, the
  approval checklist, and the expansion scoring; a human transmits it.
---

## 1. Objective

`docs/gtm/REFERENCEABILITY_PACKET.md` defines exactly what a design partner is asked to approve —
claim by claim, each with its source artifact — at three escalating levels of disclosure, and defines
a scored expansion path from a completed pilot to a second engagement. A checker verifies that every
claim in a proposed packet is traceable to a linted case study and that no claim exceeds the approval
level it was granted.

Passing this item does **not** advance `D4`. `00_MASTER_PROTOCOL.md` §1.1 defines `D4` as at least
one paid or signed pilot whose scope is exactly what the archived artifacts demonstrate. What this
item produces is a traceability checker and a synthetic packet — a Python checker validating a
fixture the same agent authored. It proves that the checker resolves references its own author
wrote to resolve. That is worth having: an instrument that cannot fail on its own author's input
would be worse than useless, and the fixture is the negative control that shows the checker
discriminates. But a synthetic fixture is never the outcome; it is the test of the instrument that
will later meet a real one.

The item that turns this instrument on external reality is **`G0.08`** — five real, non-synthetic,
unacquainted, consented discovery records from strangers traceable to a declared arrival channel
(`G0.09`). Until `G0.08` passes, nothing here has met a person. And `D4` itself closes only on an
executed agreement, which `00_MASTER_PROTOCOL.md` §6 makes a human action that no agent may take.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate / world engine, never a game engine. Licence
is PolyForm Shield 1.0.0 — say **source-available**, never open source. Physics is Avian. Units are
meter-native.

**Why levelled approval.** Partners grant different amounts of disclosure, and conflating them
destroys references. Three levels, each a strict superset of the last:
`L1_ANONYMISED` (industry and scale only, no organisation identity), `L2_NAMED` (organisation may be
named, specific numbers still withheld), `L3_FULL` (organisation named with specific numbers and an
attributed quote). A claim approved at `L1` used in an `L2` context is a breach, and the checker must
catch it mechanically rather than relying on memory.

**Inputs.** `docs/gtm/CASE_STUDY_FORMAT.md`, `docs/gtm/case_study_schema.json`, and
`docs/PROMPTS/harness/checkers/case_study_lint.py` (`G6.36`); `docs/gtm/PILOT_PREREGISTRATION.md`
(`G6.34`); `docs/gtm/SIM_TO_PHYSICAL_GATES.md` (`G6.35`);
`docs/gtm/DESIGN_PARTNER_QUALIFICATION.md` (`G6.32`).

**What expansion can honestly be sold into today.** The desktop studio, the MCP tool surface, CAD
Tier 1 (`cad_describe_part`, `cad_validate_part`, `cad_measure` at
`eustress/crates/tools/src/cad_tools.rs` lines 615, 802, 1006), STEP interoperability from `G2.34`
and `G2.35`, the usage-telemetry pipeline, and a negotiated commercial licence
(`LICENSE-COMMERCIAL.md`, which has no published price). What cannot be sold: anything requiring a
live payment rail — `docs/AUDIT/09_ECONOMY.md` puts the Bliss balance ledger at 🔴 0% with no
currency able to move, Steam IAP at 🔴 5%, and Stripe Connect effectively 🔴 0% gated on KYC, while
`docs/AUDIT/08_IDENTITY_TRUST.md` records the Cloudflare Worker `JURISDICTIONS` dict as empty. Also
unavailable: hosted operation (`docs/AUDIT/12_INFRASTRUCTURE.md` — Vault 0%, Consul directory absent,
Prometheus and Grafana 0%, multi-region 0%, no DR runbooks), any SLA or on-call, SOC 2 or ISO 27001,
WCAG AA conformance, and signed binaries.

**The licence shapes what expansion can even be offered.** PolyForm Shield's Noncompete clause bars
providing any product that competes with the software, including free products, across different
interfaces and platforms. Its New Products clause freezes an adopter at the versions available when
competition begins. An expansion motion that invites a partner to build and distribute an adjacent
commercial tool is a licence change, which is a **human decision** and must never be authored as an
agent item. Expansion here means: more seats, more disciplines within the shipped surface, deeper
integration into their existing stack, and a negotiated commercial licence.

**Human-only boundary.** Sending, requesting approval, negotiating, or signing anything with a real
organisation is a human action. This item produces the packet and the checker. It contacts no one and
names no real company.

## 3. Scope

### In scope — files this item may edit
- `docs/gtm/REFERENCEABILITY_PACKET.md` — create
- `docs/gtm/reference_levels.json` — create
- `docs/gtm/EXPANSION_SCORECARD.md` — create
- `docs/PROMPTS/harness/checkers/reference_check.py` — create
- `docs/PROMPTS/harness/checkers/fixtures/packet_synthetic_*.json` — create

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- All inputs named in §2 — frozen
- `LICENSE`, `LICENSE-COMMERCIAL.md` — licence terms are a human decision
- `docs/marketing/UofA_Center_For_Innovation_Pilot.html` — a live partner-facing asset

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Collapsing the three approval levels into one, allowing a claim without a case-study reference, or
  making the level check advisory are measurement changes.
- Every claim in a packet must reference a claim id in a case study that passes
  `case_study_lint.py`. Unlinted claims may not enter a packet.
- The level check must be mechanical: a claim carries the level at which it was approved, and using
  it at a higher level must fail.
- The expansion scorecard must only contain motions that are deliverable today, and must state for
  each what would have to ship first if it is not.
- Never name a real organisation. Never draft an email to a real recipient. Never assume approval.

## 5. Exit criterion

### Criterion
Three approval levels are defined with a strict superset relation; the checker accepts a synthetic
packet whose every claim is traceable to a linted case study and used at or below its approved level
with **exit 0**; and rejects, with **exit 2**, both a packet containing an untraceable claim and a
packet using an `L1_ANONYMISED` claim in an `L2_NAMED` context, naming the offending claim id in each
case.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/reference_check.py \
        --levels docs/gtm/reference_levels.json \
        --case-study docs/gtm/case_studies/SYNTHETIC-ALPHA.md \
        --packet docs/PROMPTS/harness/checkers/fixtures/packet_synthetic_ok.json
    echo "GOOD_EXIT=$?"

    python docs/PROMPTS/harness/checkers/reference_check.py \
        --levels docs/gtm/reference_levels.json \
        --case-study docs/gtm/case_studies/SYNTHETIC-ALPHA.md \
        --packet docs/PROMPTS/harness/checkers/fixtures/packet_synthetic_untraceable.json
    echo "UNTRACEABLE_EXIT=$?"

    python docs/PROMPTS/harness/checkers/reference_check.py \
        --levels docs/gtm/reference_levels.json \
        --case-study docs/gtm/case_studies/SYNTHETIC-ALPHA.md \
        --packet docs/PROMPTS/harness/checkers/fixtures/packet_synthetic_level_breach.json
    echo "BREACH_EXIT=$?"

Expected output shape:

    levels=3 (L1_ANONYMISED < L2_NAMED < L3_FULL) superset_relation_ok=true
    packet=SYNTHETIC-OK claims=4 traceable=4 level_violations=0
    GOOD_EXIT=0
    packet=SYNTHETIC-UNTRACEABLE  UNTRACEABLE claim 'c7' has no case-study source
    UNTRACEABLE_EXIT=2
    packet=SYNTHETIC-BREACH  LEVEL VIOLATION claim 'c2' approved L1_ANONYMISED used at L2_NAMED
    BREACH_EXIT=2

Pass condition:

    GOOD_EXIT == 0  AND  UNTRACEABLE_EXIT == 2  AND  BREACH_EXIT == 2
    AND levels == 3  AND  superset_relation_ok == true
    AND each rejection names the specific offending claim id

## 6. Critic gate

`critic_gate` is `[]`. The packet's integrity is a traceability property and a level-ordering
property, both decided by exit codes across three synthetic packets. The compensating tightness:
three levels with a verified superset relation, mandatory traceability to a linted case study, a
mechanical level check, and rejections that must name the offending claim rather than failing
generically.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — per-claim approval records with a level field, checked against the
                 linted case study
   -> if still failing, MANDATORY approach change. Adding a level is NOT an approach change;
      moving to a packet generated from the case study's claim array so an unapproved claim cannot
      be typed in is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where BREACH_EXIT != 2
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: a packet claim's source artifact does not resolve (see front matter)
```

## 8. Artifact

`docs/gtm/REFERENCEABILITY_PACKET.md`

A reader finds: the three approval levels with their exact disclosure scope; the per-claim approval
record format; the one-pass approval checklist a partner receives, in which every claim appears with
its number, its unit, its source artifact, and its counterfactual, so approval is a single reading
rather than a negotiation; the level-ordering rule and how it is enforced; the rule that only claims
from a case study passing `case_study_lint.py` may enter a packet; and the statement that
transmission and approval are human actions.

`docs/gtm/EXPANSION_SCORECARD.md` is the companion: the expansion motions that are deliverable today
(more seats, more disciplines within the shipped surface, deeper integration into the partner's
existing stack via the `G2.34`/`G2.35` STEP path, a negotiated commercial licence), each scored on
partner readiness and on what would have to ship first; plus the explicit non-motions — anything
requiring a payment rail, hosted operation, an SLA, a compliance certification, or a licence change.

## 9. Definition of NOT done

- The packet has one approval level, so an anonymised agreement is later read as permission to name
  the partner. That single mistake ends a reference and usually the relationship.
- A claim can enter a packet without passing `case_study_lint.py`, so a number with no source
  artifact reaches a partner for approval.
- The level check warns instead of failing. In a workflow whose whole purpose is not to over-claim,
  a warning is worse than nothing because it creates the appearance of a control.
- The expansion scorecard lists motions requiring a payment rail, hosted operation, an SLA, or a
  compliance certification. None of those exist, and offering them converts a good reference into a
  missed commitment.
- The expansion path invites the partner to build and distribute an adjacent commercial tool.
  PolyForm Shield's Noncompete clause bars competing products even when provided free of charge;
  changing that is a human decision.
- The item drafted an email, named a real organisation, or recorded an approval that no human gave.
````

---

## Pack completion note

Every prompt in this pack is authored `status: DRAFT`. An L1 promotes each to `READY` after
confirming, per `docs/PROMPTS/03_PROMPT_SCHEMA.md` §5, that the cited paths exist at the commit the
item opens against, that no `depends_on` cycle has been introduced, and that the tier envelopes match
§3.3.

Two standing reminders for whoever executes these items:

**Reproducing is not the same as agreeing.** Several items here are designed to surface disagreement
— `G2.31` expects out-of-band cases against published references, `G2.37` expects FBX to fail, and
`G6.30` expects the declared-capability count to dwarf the shipped count. Those results are the
deliverable. An item that returns only agreement has usually selected its cases.

**Every human-only boundary in the `G6` block is hard.** Contacting an organisation, signing or
countersigning anything, publishing a case study naming a real company, quoting a real person, and
changing licence terms are human decisions under `docs/PROMPTS/00_MASTER_PROTOCOL.md` §6. The agent
deliverable in each case is the instrument and its checker, never the executed act.
