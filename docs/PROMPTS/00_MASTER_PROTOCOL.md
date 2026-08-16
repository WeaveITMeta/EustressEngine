# 00 — MASTER PROTOCOL

**Status:** Normative. Every prompt in `docs/PROMPTS/` runs under this document.
**Companions:** `01_CRITIC_RUBRIC.md` (scoring contract), `02_CAPTURE_HARNESS.md` (evidence
production), `03_PROMPT_SCHEMA.md` (authoring format), `04_FILE_OWNERSHIP.md` (who may edit a
contested source file).
**Scope of authority:** This file defines who may do what, what "done" means, and when to stop.
Where a downstream prompt conflicts with this file, this file wins.

---

## 1. Mission

Raise Eustress — an AI-native simulation substrate — from *"the founder can demo it"* to
*"a stranger, given only the artifacts, concludes it is better than the alternatives they
already trust."*

Eustress is not a game engine. 3D rendering and the ECS are implementation details in service of
a substrate an AI reasons over and a human edits. Every item in this program must move at least
one of those two faces forward.

### 1.1 Falsifiable definition of done

The program is done when all four of the following hold simultaneously, each verifiable by a
third party from archived artifacts alone:

| # | Condition | Verification |
|---|---|---|
| D1 | **Conditional on `G1.14`.** A blind Critic, shown randomised unlabelled captures of Eustress and of a licensed reference artifact, scores Eustress **≥ 8.0 on every rubric dimension** in `01_CRITIC_RUBRIC.md` and selects the Eustress side on the preference question in **≥ 4 of 5** independent trials. If `G1.14` does not land a licensed control, D1 degrades to **D1′** — see below. | `docs/PROMPTS/artifacts/critic/<item-id>/scorecard.json` |
| D2 | Two independent runs of the same headless recipe produce **byte-identical** recordings and **byte-identical** capture frames. | `docs/architecture/HEADLESS_RUNTIME.md:294` gate, executed and archived |
| D3 | CI executes the workspace test suite and the capture harness on every push, and a red gate blocks merge. | `.github/workflows/ci.yml` diff + a linked failing run |
| D4 | At least one paid or signed pilot exists whose scope is exactly what the archived artifacts demonstrate — no capability claimed in the sales surface that lacks a passing item in this program. | Executed agreement + a scope-to-item-id mapping table |

D1–D3 are engineering gates. D4 is the business gate. **A program that clears D1–D3 and fails D4
has built a beautiful thing nobody bought; a program that clears D4 without D1–D3 has sold
something it cannot verify.** Both failure modes are terminal. Track them jointly.

**D1 depends on a procurement no agent may perform.** The mission sentence — a stranger concludes
Eustress "is better than the alternatives they already trust" — is a comparison, and a comparison
needs the other side. That other side is a licensed reference artifact, and acquiring it is
`G1.14`: selecting the product, obtaining the artifact, performing the capture, and determining
`publication_rights` are all human actions. §4.1 R2 forbids an agent to stage a comparison naming a
third-party product, and §6 makes any spend a human decision. So D1 as written cannot be closed by
this program's own effort. It can only be closed by a human landing `G1.14` first.

**If `G1.14` is refused, deferred, or returns no publishable control, D1 degrades to D1′ and the
program says so out loud:**

> **D1′.** A blind Critic, shown randomised unlabelled captures of `eustress@HEAD` against
> `eustress@<baseline-commit>`, scores the HEAD side **≥ 8.0 on every rubric dimension** in
> `01_CRITIC_RUBRIC.md`. The preference question (`D7`) is **not scored**, and no preference result
> is claimed anywhere.

D1′ is a real gate and a weaker claim, and the difference must never be blurred. It says the
artifact meets an absolute quality bar and improved against its own past. It does **not** say anyone
preferred it to anything, because a forced choice between a build and its own ancestor answers a
different question — and at a 4-of-5 floor a self-comparison clears by chance roughly 19 times in
100. Any external surface written under D1′ states the absolute scores and is silent on preference.
`00_MASTER_PROTOCOL.md` §4.1 R4 keeps the self-comparison as the default bar precisely because it is
always legal to publish; D1′ is that bar promoted to a definition of done, with its ceiling admitted.

Which of D1 and D1′ is in force is a human decision recorded once by L0 in
`docs/PROMPTS/artifacts/ledger.jsonl`. Until it is recorded, assume **D1′** and do not author an
item that gates on the preference question.

### 1.2 Non-goals

Explicitly out of scope for this program, to prevent scope creep at every level:

- Nanite-class virtualized geometry and real-time global illumination. `docs/architecture/WORLD_CLASS_ENGINE.md:20` lists both as not started; they are not a prerequisite for D1.
- Multi-region hosting, SOC 2, ISO 27001, WCAG AA. See §6 constraints.
- Any change to the license. Reframing (§4.5) is in scope; relicensing is a human decision, not an agent decision.
- Consumer monetization funnels. See §4.2.

---

## 2. Agent architecture

Five roles. Four are in the execution chain; the Critic sits outside it and is never in the same
context window as the candidate's self-report.

```
                 ┌──────────────────────────────────────────┐
   HUMAN ◄──────►│  L0  ORCHESTRATOR   (1 instance)         │
   (stalls,      └────────────────┬─────────────────────────┘
    license,                      │ dispatches phase charters
    money,                        ▼
    publishing)   ┌──────────────────────────────────────────┐
                  │  L1  PHASE LEAD     (1 per gauntlet phase)│
                  └────────────────┬─────────────────────────┘
                                   │ dispatches item prompts
                                   ▼
                  ┌──────────────────────────────────────────┐
                  │  L2  SPECIALIST     (1 per prompt item)   │
                  └────────────────┬─────────────────────────┘
                                   │ dispatches bounded sub-tasks
                                   ▼
                  ┌──────────────────────────────────────────┐
                  │  L3  ULTRA-SPECIALIST (ephemeral)         │
                  └──────────────────────────────────────────┘

   ARTIFACTS ────────────────────────────────────────────────►┌───────────┐
   (captures, recordings, measured numbers — NEVER prose)     │  CRITIC   │
                                                              └───────────┘
```

### 2.0 L0 — Orchestrator

**Input contract.** This file; the full prompt library index; the current `docs/AUDIT/MASTER.md`
gap ledger; the set of completed item scorecards.

**Output contract.** (a) A ranked item queue with each item's workload tag, phase tag, and
dependency closure; (b) a dispatched phase charter per active L1; (c) a `STALL` escalation packet
to the human when §5.3 fires; (d) a program-level ledger at
`docs/PROMPTS/artifacts/ledger.jsonl`, one line per item state transition.

**Authority.** May reorder the queue, kill an item, split an item, and declare a phase complete on
receipt of passing scorecards for every item in it.

**May NOT.** Write production code. Edit `01_CRITIC_RUBRIC.md` or any held-out criterion. Score an
artifact. Mark an item passed without a Critic scorecard. Change the license, publish anything
externally, spend money, or deploy to production — all four are human decisions (see §6).

**Concurrency limit.** L0 may have at most **one** L1 in a build-consuming state at a time. The
workspace shares a single `target/` directory; two concurrent cargo builds produce link failures
(LNK2001 / SAC os error 4551) and cost more wall-clock than serial execution. L0 may run
additional L1s only if they are in a documentation, analysis, or artifact-review state.

### 2.1 L1 — Phase lead

One per gauntlet phase (§3.2). Owns the phase's item set end to end.

**Input contract.** The phase charter from L0: phase id, the item ids in scope, the dependency
graph, the phase's exit condition, and the phase's share of the program budget (§7).

**Output contract.** For each item: a dispatched L2 prompt conforming to `03_PROMPT_SCHEMA.md`; the
item's evidence directory path; the Critic invocation record; the item's final state
(`PASSED` | `STALLED` | `KILLED`). For the phase: a phase report listing every item id, its final
score vector, and the workload evidence it produced.

**Authority.** May author new item prompts inside its own phase, provided each conforms to
`03_PROMPT_SCHEMA.md` and declares a falsifiable exit criterion. May re-dispatch a failed item with
a changed approach (§5.2). May request a dependency from another phase via L0.

**May NOT.** Author items outside its phase. Relax an exit criterion, a pass floor, or a Critic
gate. Accept an L2's self-assessment in place of a Critic scorecard. Exceed its budget without an
L0 grant. Merge to `main`.

### 2.2 L2 — Specialist

One per prompt item. This is the level that writes code.

**Input contract.** Exactly one prompt file conforming to `03_PROMPT_SCHEMA.md`, and nothing else.
An L2 must be able to execute **cold** — no conversation history, no prior turn. If an L2 needs
context the prompt does not contain, that is a defect in the prompt, and the L2 must report it as
`PROMPT_UNDERSPECIFIED` rather than improvise.

**Output contract.** (a) The code or document changes, scoped to the files the prompt names;
(b) the evidence artifact the prompt requires, written to the prompt's declared artifact path;
(c) a machine-readable result block: item id, approach id, iteration number, exit-criterion
measurement (the literal command run and its literal output), files touched, and a `BLOCKED` list.

**Authority.** May edit any file inside the prompt's declared scope. May run builds, tests,
benchmarks, and MCP tools. May dispatch L3s for bounded sub-problems. May declare
`PROMPT_UNDERSPECIFIED` or `EXIT_CRITERION_UNMEASURABLE` and hand back.

**May NOT.** Edit files outside its declared scope — if the fix requires it, that is an escalation,
not a licence. Edit a file that `04_FILE_OWNERSHIP.md` assigns to another item, even when the item's
own scope list names it; that file wins over the scope list, and the remedy is a `FILE-OWNERSHIP`
decision packet (`04_FILE_OWNERSHIP.md` §6), never a workaround. Read `01_CRITIC_RUBRIC.md` §4 (held-out criteria). Score its own work. Weaken,
rewrite, or reinterpret its own exit criterion. Delete or regenerate a capture that has already
been hashed into a provenance manifest. Commit to `main`. Modify CI to make a gate pass.

**Anti-reward-hacking constraint.** An L2 that changes a *measurement* rather than the *artifact*
has failed the item, regardless of the resulting number. Loosening a tolerance, shrinking a capture
frame set, excluding a scene, disabling an assertion, or lowering a resolution are all
measurement changes. If the measurement is genuinely wrong, the L2 must report it as
`EXIT_CRITERION_UNMEASURABLE` with evidence and stop.

### 2.3 L3 — Ultra-specialist

Ephemeral. Spawned by an L2 for one bounded sub-problem with a single answer: a targeted grep
across 39 crates, a shader-math derivation, a colour-space conversion check, a one-file refactor
against a named signature, a literature check on a numerical method.

**Input contract.** A single question or a single mechanical transformation, plus every path and
number needed to answer it. Self-contained.

**Output contract.** The answer, plus the evidence for it (paths, line numbers, measured values).
Nothing else. No recommendations, no scope expansion.

**Authority.** Read anything in the repo. Write only where the spawning L2 explicitly named a path.

**May NOT.** Spawn further agents. Touch the build. Interact with the Critic. Return an opinion
where a measurement was asked for.

### 2.4 The Critic

An independent agent, described fully in `01_CRITIC_RUBRIC.md`.

**Input contract.** Only: (a) the rubric; (b) a capture bundle from `02_CAPTURE_HARNESS.md` with
side labels stripped and randomised; (c) the item's declared exit criterion and its measured value,
supplied by L1, not by the L2.

**Output contract.** A scorecard JSON conforming to `01_CRITIC_RUBRIC.md` §6, with a mandatory
evidence citation for every score.

**Authority.** Final say on pass/fail. May fail an artifact that clears every numeric floor
(§5.4). May demand a re-capture if the bundle is malformed.

**May NOT.** Pass an artifact that misses any numeric floor — no exceptions, no "close enough", no
credit for effort. Receive the candidate's self-report, changelog, commit messages, PR description,
or any prose the L2 wrote about its own work. Learn which side is Eustress. See held-out criteria
before scoring the dimensions they belong to.

---

## 3. The two axes

Every item in the library sits at one (workload, phase) coordinate and produces evidence for both.

### 3.1 Axis A — Six business workloads

| ID | Workload | The question it answers | Primary evidence type |
|----|----------|-------------------------|-----------------------|
| **W1** | Provable Quality | Does the artifact beat what the buyer already trusts, judged blind? | Critic scorecards over capture bundles |
| **W2** | Revenue Rail | Can money actually move, end to end, once? | An executed transaction or signed agreement, with its receipt |
| **W3** | Trust & Verifiability | Can a stranger reproduce our claims without us? | Byte-identical reruns; green CI; signed binaries |
| **W4** | Vertical Proof | Does one vertical work all the way through, no unwired buttons? | A recorded end-to-end session in that vertical |
| **W5** | Extension Surface & Merit Ladder | Can someone outside the company build on this, and on what terms? | A third-party extension running unmodified; a written contributor terms document |
| **W6** | Operator Leverage | Does the solo founder's throughput per wall-clock hour go up? | Before/after wall-clock on a fixed task list |

**Workload evidence declaration.** Every prompt declares `workload:` with one primary tag and zero
or more secondary tags, and names in its `artifact:` field the specific file that constitutes that
workload's evidence. An item that cannot name such a file is not an item; it is a wish.

**W2 note — serialisation.** The money rail is one chain, not five tracks. It is four links long, not
five: the Bliss balance ledger is at 0% (`docs/AUDIT/09_ECONOMY.md`); Stripe Connect is gated on KYC;
payouts are gated on banking, banking on an EIN, and the EIN on an LLC still forming (`LAUNCH_PLAN.md`
Streams 2–3).

KYC is **not** gated on jurisdiction data. The Cloudflare Worker's `JURISDICTIONS` ontology holds
**46 country entries** at `infrastructure/cloudflare/api/src/index.js:39-98`, each with its accepted
natural-person identity documents, R2 prefix, and age of majority, behind a fallback rule that covers
every unlisted country. That is the deepest link in the chain and it already exists.

**This changes the queue order.** The first W2 item an L1 dispatches is not "populate the jurisdiction
ontology" — that work is done, and an item written against the old premise would spend its envelope
rediscovering it. The front of the queue is the next unbuilt link: the KYC flow that consumes the
ontology, and behind it the entity formation that everything downstream of Stripe waits on. L0 must
still not queue W2 items in parallel, and must not let a downstream W2 item claim `PASSED` while its
upstream link is 0%.

The near-term W2 surface that does *not* depend on this chain at all is the negotiated commercial
licence (`LICENSE-COMMERCIAL.md`) and paid pilot engagements. With the chain one link shorter, it is
still the only W2 surface an agent can advance without waiting on a human forming a company.

### 3.2 Axis B — Seven gauntlet phases

Ordered. A phase may not open until its predecessors' hard dependencies are `PASSED`.

| ID | Phase | Opens when | Phase exit condition |
|----|-------|------------|----------------------|
| **G1** | Capture & Measurement Harness | Immediately. **Item zero.** | The harness in `02_CAPTURE_HARNESS.md` runs from a single command, on a machine that is not the founder's, and produces a content-addressed bundle with a valid provenance manifest |
| **G2** | Determinism & Numerical Trust | G1 passed | Two runs byte-identical (D2); the time-compression step-drop in `eustress/crates/common/src/simulation/clock.rs:100` is instrumented and surfaced; the Watchman cooldown uses sim time |
| **G3** | Render Fidelity | G1 passed | Critic ≥ 8.0 on materials/lighting from `02` scene set S1–S3 |
| **G4** | Motion & Temporal Stability | G1, G2 passed | Critic ≥ 8.0 on motion; measured frame-time variance floor met on the fixed camera path |
| **G5** | Scale & Streaming | G2 passed | A *measured* entity count at a *measured* frame rate on the fixed harness — replacing the config-default number currently cited at `docs/AUDIT/05_SPACE_STREAMING.md:23` |
| **G6** | Studio UI Craft | G1 passed | Critic ≥ 8.0 on UI craftsmanship; the drain-skip failure class has a regression test that runs in CI |
| **G7** | Agent Loop Closure | G1, G2 passed | The full observe→act→judge loop executes headless in CI with no desktop session |

**G1 is item zero and is not negotiable.** Every other phase's exit condition is stated in terms of
a measurement G1 produces. Without G1 there are no comparable numbers, and every subsequent claim
is an assertion. Do not let any L1 open a phase by "eyeballing it while G1 is in progress."

### 3.3 The matrix

An item is addressed `<phase>.<nn>` and tagged with its workload. The matrix is sparse by design —
not every cell is populated, and L0 should resist the urge to fill it. Representative anchors:

| | W1 Quality | W2 Revenue | W3 Trust | W4 Vertical | W5 Extension | W6 Leverage |
|---|---|---|---|---|---|---|
| **G1** | bundle format | — | provenance manifest | — | — | one-command capture |
| **G2** | — | — | byte-identical gate | sim credibility | — | — |
| **G3** | material/light bar | — | — | — | — | — |
| **G4** | motion bar | — | frame-time variance | — | — | — |
| **G5** | — | — | measured scale claim | — | — | — |
| **G6** | UI craft bar | — | drain-skip regression | unwired-button sweep | plugin surface | `slint_ui.rs` split |
| **G7** | — | — | CI-hosted loop | — | tool surface | headless eyes |

---

## 4. Standing corrections and rules

These apply to every item, every level, every artifact.

### 4.1 Reference-bar legality and reproducibility

**Rule R1 — Internal calibration is unrestricted.** Capturing output from any commercial engine or
platform to calibrate our own quality bar, internally, is permitted and encouraged. A quality bar
you cannot see is not a bar.

**Rule R2 — Publication is restricted to artifacts we are licensed to publish.** No side-by-side
comparison, screenshot, frame, video, or derived score naming a third-party product may appear in
any external surface — website, deck, README, post, pilot document, investor material — unless the
comparison artifact is one the project holds publication rights to. Several commercial engine and
platform EULAs contain benchmarking and publication clauses; assume one applies until counsel says
otherwise. **Only the human may authorise a published comparison.** An agent that finds itself
drafting a public comparison must stop and escalate.

**Rule R3 — A reference nobody can reproduce is not a reference.** Every reference capture entering
the Critic loop must be archived with full provenance: product, exact version/build, scene source
and its licence, hardware, driver version, all quality settings, capture date, and the operator.
The manifest schema is in `02_CAPTURE_HARNESS.md` §7. A reference capture without a complete
manifest is inadmissible — the Critic must reject the bundle, not score it.

**Rule R4 — Prefer self-referencing bars where possible.** The strongest bar that is always legal
to publish is **our own prior build**. Every item should produce an
`eustress@<commit-a>` vs `eustress@<commit-b>` bundle as its default comparison, with the external
reference used as an internal ceiling check only.

### 4.2 Positioning corrections

- **Never call Eustress a game engine.** It is an AI-native simulation substrate. The strongest
  in-tree line is `README.md:12` — built to "simulate the world, not just render a scene."
- **The licence is PolyForm Shield 1.0.0** (`LICENSE`), dual-licensed against
  `LICENSE-COMMERCIAL.md`. Say **source-available**. Never say open source.
- **Do not cite `docs/architecture/THE_LAST_GAME_ENGINE.md`** — line 1 marks it deprecated and it
  describes a pipeline that no longer exists.
- **Physics is Avian.** Never Rapier.
- **Slint is Rust.** `.slint` compiles to Rust. Never frame "Rust-first" as opposed to Slint.
- **Units are meter-native.** Studs are a display unit only.

### 4.3 Documentation standard

Every document this program produces must read as though it were always correct. No "previously we
thought", no changelog residue inside the body, no self-justifying commentary. Corrections are
explained in the conversation and the commit message, never in the doc body.

### 4.4 Number discipline

Never state a measured number that was not measured. Every number is either:

- **MEASURED** — cite where, on what hardware, with what command, in what artifact directory; or
- **TARGET** — labelled `TARGET` inline; or
- **CONFIG DEFAULT** — labelled as such.

The 2.10M-entity figure in `docs/AUDIT/05_SPACE_STREAMING.md:23` is an `active_cap` config default
and must be labelled that way until G5 replaces it with a measurement. The "80–90% cost reduction"
in `docs/architecture/EUSTRESS_FORGE.md` is an unmeasured marketing claim and must not be repeated
in any artifact this program produces.

### 4.5 Correction to the W5 premise

The originating brief calls Workload 5 an "open ecosystem." That framing does not survive contact
with `LICENSE`. PolyForm Shield's Noncompete clause bars providing "any product that competes with
the software or any product the licensor or any of its affiliates provides using the software," and
its Competition clause is deliberately maximal — competition is judged across differing interfaces
and technical platforms, and applies **even when the competing product is provided free of charge**.
The New Products clause additionally freezes an adopter at the versions available when Eustress LLC
enters that adopter's market. There is no CONTRIBUTING file and no CLA or DCO in the repo, so
inbound contribution licensing is undefined, while `README.md` already promises contributors
cash-outable Bliss.

So W5 is not "grow an open-source community." W5 is four concrete things that are available today
under the licence as written:

1. **Extension surface** — plugins, Rune and Luau scripts, MCP tools, and end-products built *with*
   Eustress. These are permitted at no cost and carry no royalty
   (`LICENSE-COMMERCIAL.md`, "When you do NOT need a commercial license").
2. **Merit ladder** — a published path from user to trusted contributor, with the reward denominated
   in something the project can actually deliver today. Note that the cash-outable framing depends
   on the Bliss ledger, which is at 0%.
3. **Contributor licensing** — write the missing `CONTRIBUTING.md` and adopt a DCO or CLA so inbound
   IP is defined. This is the single highest-value, lowest-cost W5 item and it is unblocked now.
4. **Commercial capture path** — a defined route from free adopter to a negotiated commercial
   licence, which is today's only non-serialised revenue surface.

**Requires a human licence decision, and is therefore out of agent scope:** distro or registry
packaging under open-source norms; adoption by organisations whose OSPO blocks non-OSI licences;
third parties hosting Eustress as a service; and any promise that a contributor may build an
adjacent commercial tool.

Describing the project accurately is a separate matter and is agent work. Several customer-facing
surfaces assert that Eustress is open source, which `LICENSE` contradicts —
`docs/marketing/UofA_Center_For_Innovation_Pilot.html:592` and `:665`, and the live Leptos site at
`eustress/crates/web/src/pages/home.rs:434`, `blog_indie_studios.rs`, and
`docs_earning.rs:314`. Correcting a description to match the licence changes no terms and needs no
human decision; `G0.06` owns it and enforces the result with a whole-repository grep verifier.
Changing the licence to match a description is the human decision, and it is `G1.48`'s to force.

### 4.6 PROGRAM-LICENCE-GATE

`G1.48` produces `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`: four costed licence
options, six consequence dimensions each, a recommendation, and `decision: null`. Its verifier
**fails the item if `decision` is non-null** — an agent that records a decision has taken a human-only
action under §6.

So a null decision is the normal, correct output. The gate exists because that output is a fork in
the road, not a filing. **Every item whose artifact is handed to, or published for, someone outside
the company is blocked on it:**

| Gated item | What it would hand a third party |
|---|---|
| `G1.49` | `CONTRIBUTING.md` and a DCO — the inbound-IP terms a contributor signs |
| `G1.50` | The extension surface a third party builds against |
| `G1.51` | The merit ladder, and what a rung is worth |
| `G1.52` | Governance non-negotiables and their violation detectors |
| `G7.12` | `.etask` — the environment and task specification format |
| `G7.22` | `EUSTRESS-PHYS-12`, a public benchmark and its harness |
| `G7.24` | The lab integration guide |
| `G0.04` | Data rights over `.etask`, the benchmark, episode bundles, and models trained on them |
| `G0.07` | The bundle format an outside engineer produces against |
| `G0.10` | A public leaderboard carrying an outside submitter's result |
| `G0.12` | `SECURITY.md` and a disclosure path |

**The rule.** Before an executing agent writes anything on that list, it reads the memo. If
`decision == null`, it **STALLs and escalates** — it does not publish into licence ambiguity, does
not substitute its own reading of `LICENSE`, and does not proceed "against the licence as it
stands". Proceeding against the current terms is the failure this gate exists to prevent: every one
of the four options changes what those artifacts say, and a third party who has already relied on
the wrong answer cannot be un-relied upon.

**The stall is addressed to the human, and it is the point.** The packet uses the §5.3 form, and its
`DECISION REQUESTED` is the licence choice itself — one of `G1.48`'s four option keys — not one of
the four generic options. L0 forwards it unchanged. A queue in which every gated item is stalled on
`G1.48` is the gate working: it converts a memo nobody waits on into the program's single largest
open question, held in front of the one person who may answer it.

Work upstream of the artifact is not blocked. An item may design, build, measure, and test; it
stops at the moment of handing the result outward.

---

## 5. The loop contract

### 5.1 One iteration

```
  1. L1 dispatches the item prompt to a fresh L2.  (fresh = no prior iteration in context)
  2. L2 executes; produces code changes + the declared artifact + a result block.
  3. L1 runs the exit-criterion measurement ITSELF. It does not trust the L2's number.
  4. If the measurement misses the floor  -> FAIL(objective). Go to 5.2.
  5. L1 builds a capture bundle per 02_CAPTURE_HARNESS.md, strips and randomises side labels.
  6. L1 invokes the Critic with: rubric + bundle + measured value. NEVER the L2's prose.
  7. Critic returns a scorecard.
     - Any score below its floor, or any score lacking a citation -> FAIL. Go to 5.2.
     - All floors cleared AND the wow gate (5.4) satisfied         -> PASSED.
     - All floors cleared, wow gate refused                        -> FAIL(subjective). Go to 5.2.
  8. On PASSED: L1 archives the bundle, the scorecard, and the measurement transcript; appends to
     the ledger; closes the item.
```

Step 3 is load-bearing. An L2 reporting its own pass is the single most common way this loop
degrades into theatre.

### 5.2 The escalation ladder — bounded, not "until wowed"

The loop terminates. Always. It terminates on a pass, on a kill, or on a documented stall that
reaches the human with a specific decision request.

```
  APPROACH A
    iteration A1  ─┐
    iteration A2   ├─  ≤ 3 iterations at the current approach
    iteration A3  ─┘
        │ still failing
        ▼
    FORCED APPROACH CHANGE  (mandatory; documented; new approach id)
        - L1 writes an approach-change note: what A tried, the exact failure signature,
          why B is materially different (not "try harder", not "tune the constant").
        - A change of parameter values is NOT an approach change.
        - The new L2 receives the note. It does NOT receive A's code diff.
  APPROACH B
    iterations B1..B3
        │ still failing
        ▼
    FORCED APPROACH CHANGE  ->  APPROACH C
  APPROACH C
    iterations C1..C3
        │ still failing
        ▼
    STALL  (hard ceiling: 9 iterations, 3 approaches)
```

Two additional early-exit triggers, either of which jumps straight to STALL without burning the
remaining budget:

- **No-progress trigger.** Three consecutive iterations whose worst-dimension Critic score moves by
  less than 0.5 and whose exit measurement moves by less than 5%. Grinding without movement is a
  stall wearing a costume.
- **Budget trigger.** The item consumes 150% of its §7 envelope.

### 5.3 Stall protocol

A STALL is a first-class, respectable outcome. It is not a failure of the agent; it is information
the human needs. The L1 emits a stall packet to L0, and L0 escalates it to the human:

```
STALL  <item-id>
─────────────────────────────────────────────────────────────────
FLOOR MISSED       : <dimension or exit criterion>, floor <x>, best achieved <y>
BEST ARTIFACT      : <path to the highest-scoring bundle>
APPROACHES TRIED   : A: <one line>  -> failure signature <one line>
                     B: <one line>  -> failure signature <one line>
                     C: <one line>  -> failure signature <one line>
ROOT CAUSE (best current theory, with evidence path:line or measured number)
CONSUMED           : <tokens> / <wall-clock> vs envelope <budget>
─────────────────────────────────────────────────────────────────
DECISION REQUESTED : exactly ONE of —
   (a) LOWER  the floor to <x'>, and here is the specific consequence of doing so
   (b) FUND   approach D: <description>, estimated <budget>, distinct because <reason>
   (c) DEFER  the item behind blocking item <id>
   (d) KILL   the item, and here is what the program loses
RECOMMENDATION     : <one of a-d> because <one sentence>
```

The packet must fit on one screen. A stall packet that asks the human to "review the situation"
rather than choose from four named options is malformed and must be rewritten before it is sent.

### 5.4 The wow gate — on top of the floor, never instead of it

Two independent conditions govern a pass:

- **Objective floor (necessary).** Every numeric floor in `01_CRITIC_RUBRIC.md` is met or exceeded,
  measured by that rubric's stated measurement method. This condition is not subject to Critic
  discretion, L1 judgement, or human enthusiasm.
- **Wow gate (also necessary).** The Critic affirms the artifact is genuinely excellent, not merely
  compliant, and cites what makes it so.

Therefore:

| Floor | Wow | Result |
|---|---|---|
| miss | any | **FAIL** — the Critic has no authority to pass this |
| meet | refused | **FAIL(subjective)** — Critic must name the specific deficiency and what would fix it |
| meet | affirmed | **PASS** |

The asymmetry is the point. The Critic can refuse to be impressed by something technically
compliant; it can never be talked into passing something that misses a number. A wow refusal that
does not name a specific, addressable deficiency is itself malformed and L1 must reject it and
re-invoke.

### 5.5 Anti-reward-hacking, program level

| Mechanism | Where specified | Enforced by |
|---|---|---|
| Held-out criteria the candidate never sees | `01_CRITIC_RUBRIC.md` §4 | Rubric file section is off-limits to L2/L3 by §2.2 |
| Randomised A/B side assignment per trial | `02_CAPTURE_HARNESS.md` §6 | Harness generates the seed; L1 holds the key |
| Mandatory evidence citation on every score | `01_CRITIC_RUBRIC.md` §5 | Auto-FAIL on an uncited score |
| Critic never receives the candidate's self-report | §2.4 | L1 constructs the Critic input; L2 output never forwarded |
| L1 re-runs the measurement itself | §5.1 step 3 | Loop contract |
| Measurement changes count as item failure | §2.2 | L1 diff review of the item's file scope |
| Fresh L2 per iteration | §5.1 step 1 | No accumulated rationalisation across iterations |

---

## 6. Human-only decisions

No agent at any level may perform these. On encountering one, stop and escalate with a stall packet
or an equivalent decision request:

1. Changing the licence, or publishing any external comparison naming a third-party product (§4.1).
2. Any external publication: website, deck, post, pilot document, investor material.
3. Any spend, any pricing decision, any signed agreement.
4. Production deployment.
5. Any Bliss ledger design touching transferability or cash-out — this is unpriced regulatory
   exposure with no counsel engaged (`docs/AUDIT/09_ECONOMY.md` P4).
6. Merging to `main`.

---

## 7. Budget envelope

Serial builds of 10–15 minutes are the binding constraint on wall-clock, and one build at a time is
a hard rule. Budget accordingly.

| Tier | Typical item | Token envelope | Wall-clock envelope | Max builds | Max iterations |
|------|--------------|----------------|---------------------|------------|----------------|
| **S** | Doc, spec, analysis, no compile | 60k | 1 h | 0 | 3 |
| **M** | Single-crate change, one measurement | 200k | 4 h | 6 | 6 |
| **L** | Cross-crate change, capture + Critic loop | 500k | 2 days | 12 | 9 |
| **XL** | New subsystem (harness, headless GPU tier) | 1.2M | 5 days | 20 | 9 |

Rules:

- Every prompt declares its tier. An undeclared tier defaults to **M** and L1 must correct it.
- **150% of envelope triggers a stall** (§5.2), not a quiet extension.
- Build count, not token count, is usually the real ceiling. An XL item at 20 builds is already
  ~4 hours of pure compile. Design items to batch verification: one build should validate several
  changes, and validation is `cargo run`, not `cargo check`.
- Never kill a build mid-compile. A cancelled build costs more than the build.
- L3 spawns are charged to the parent L2's envelope.
- Program-level ceiling: if cumulative spend crosses 3× the sum of declared envelopes for the
  phase, L0 stalls the entire phase to the human.

---

## 8. Bootstrap sequence

For an operator starting cold:

1. Read this file, then `02_CAPTURE_HARNESS.md`.
2. Execute G1 in full, including its BUILD LIST. Nothing downstream is measurable until it passes.
3. Instantiate the Critic per `01_CRITIC_RUBRIC.md` and run it once against a
   `eustress@HEAD` vs `eustress@HEAD` bundle. Both sides identical. **The Critic must score them
   within 0.5 on every dimension.** If it does not, the Critic is unreliable and must be fixed
   before it judges anything real. This is the Critic's own calibration gate.
4. Open G2 and G3 in that order.
5. Author remaining items per `03_PROMPT_SCHEMA.md`, one file per item, each cold-runnable.
