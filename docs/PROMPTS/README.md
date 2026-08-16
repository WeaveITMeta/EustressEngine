# The Eustress Gauntlet — prompt library

```
1. Read 00_MASTER_PROTOCOL.md. It wins over everything else in this directory.
2. Open 02_QUEUE.md, take the first item whose depends_on are all PASSED.
3. Invoke the `eustress-gauntlet` skill with that item id. It hands the agent one item block.
4. When the item hits its exit criterion, invoke `eustress-critic` if critic_gate is non-empty.
5. PASS -> record it and take the next item. FAIL 9 times or 3 approaches -> STALL to the human.
```

## What this library is

159 executable work items that take Eustress — an AI-native simulation substrate, never a game
engine — from *"the founder can demo it"* to *"a stranger, given only the artifacts, concludes it is
better than the alternatives they already trust."*

Every item is a self-contained prompt, runnable **cold**: the executing agent receives the item and
nothing else — no conversation history, no prior turn, no access to the author. Every item ends at a
falsifiable exit criterion with a literal command, and leaves behind a named file as evidence.

It is a program, not a backlog: a definition of done (`00_MASTER_PROTOCOL.md` §1.1), a bounded loop
that always terminates (§5.2), an independent blinded judge (`01_CRITIC_RUBRIC.md`), and a list of
things no agent may do (§6).

## The two axes

Every item sits at exactly one **(workload, phase)** coordinate and produces evidence for both.

**Axis A — six business workloads.** What the work is *for*.

| Tag | Workload | Evidence type |
|---|---|---|
| `W1` | Provable Quality | Critic scorecards over capture bundles |
| `W2` | Revenue Rail | An executed transaction or signed agreement plus its receipt |
| `W3` | Trust & Verifiability | Byte-identical reruns; green CI; signed binaries |
| `W4` | Vertical Proof | A recorded end-to-end session in one vertical |
| `W5` | Extension Surface & Merit Ladder | A third-party extension running unmodified; contributor terms |
| `W6` | Operator Leverage | Before/after wall-clock on a fixed task list |

**Axis B — seven gauntlet phases** (plus `G0`, the trust boundary and external-reality phase). Ordered:
a phase may not open until its predecessors' hard dependencies are `PASSED`.

`G0` security, rights & external reality · `G1` capture & measurement harness (**item zero**) ·
`G2` determinism & numerical trust · `G3` render fidelity · `G4` motion & temporal stability ·
`G5` scale & streaming · `G6` studio UI craft · `G7` agent loop closure.

**How a phase item declares its workload evidence.** Three contractual front-matter fields:
`workload:` (exactly one primary tag), `workload_secondary:` (zero or more), and `artifact:` — which
must name a **specific path**, not a category. That file *is* the primary workload's evidence: what
the phase report cites and what a third party opens to check the claim. `## 8. Artifact` in the body
says what a reader finds there. An item that cannot name such a file is not an item; it is a wish.

The matrix is sparse by design. Not every (workload, phase) cell is populated and nobody should try
to fill it.

## File map

| Path | What it is |
|---|---|
| `00_MASTER_PROTOCOL.md` | Normative. Roles, authority, the loop contract, the stall protocol, human-only decisions, budget envelopes. Wins over every other file here. |
| `01_CRITIC_RUBRIC.md` | The Critic's entire contract: seven dimensions, anchors, citation forms, the auto-FAIL rules, the scorecard JSON schema. **§4 is held-out — readable by the Critic and L1 only.** |
| `02_CAPTURE_HARNESS.md` | How evidence is produced: seven harness invariants, the six frozen scenes S1–S6, the camera paths, blinding and side randomisation, the provenance manifest, and the B1–B14 build list. |
| `02_QUEUE.md` | The live run order: ready queue, wave groupings, and the stall register. Authored and maintained by L0 — a runner never invents it. Until L0 has authored it, `eustress-gauntlet` runs only against an item id a human names explicitly, and `eustress-wave` does not run at all. |
| `03_PROMPT_SCHEMA.md` | The authoring format every item conforms to: front matter, the nine body sections, tier envelopes, the authoring checklist. |
| `04_FILE_OWNERSHIP.md` | Which item owns a source file claimed by more than one pack. Normative — it wins over an item's own scope list. |
| `packs/*.md` | The 159 items, grouped by concern. Nine files. `*.bak` files are not packs; ignore them. |
| `harness/checkers/*.ps1` | Per-item verification scripts referenced by exit criteria. |
| `.state/progress.json` | Runner state: per-item status, iteration count, artifact hash, stall record, and the single build slot. Created by `eustress-gauntlet` if absent. |
| `artifacts/` | Where items write their evidence. Bundles are gitignored; manifests and scorecards are committed. |

The nine packs, and the IDs each holds — the only index of where an item lives:

| Pack | n | IDs |
|---|---|---|
| `G0_security_and_external_reality.md` | 12 | `G0.01`–`G0.12` |
| `G1_capture_harness.md` | 14 | `G1.01`–`G1.14` |
| `T1_rendering_and_content.md` | 19 | `G3.01`–`G3.13`, `G4.01`–`G4.04`, `G5.01`–`G5.02` |
| `T2_physics_and_simulation.md` | 22 | `G2.01`–`G2.18`, `G5.20`–`G5.23` |
| `T3_studio_ux.md` | 15 | `G6.01`–`G6.15` |
| `T4_agent_loop_and_training_substrate.md` | 24 | `G7.01`–`G7.24` |
| `T5_robustness_and_cohesion.md` | 16 | `G7.30`–`G7.45` |
| `B1_proof_and_design_partners.md` | 18 | `G1.30`–`G1.31`, `G2.30`–`G2.37`, `G6.30`–`G6.37` |
| `B2_revenue_ecosystem_org.md` | 19 | `G1.40`–`G1.56`, `G2.40`, `G6.40` |

IDs are never reused or renumbered, even when an item is killed.

## Design decision: items live in packs, not in one file each

`03_PROMPT_SCHEMA.md` §2 specifies one item per file at `docs/PROMPTS/items/<ID>_<slug>.md`. That
directory does not exist and will not be created. Items live inside nine pack files, each item as a
complete, conforming, self-contained block.

This does **not** relax §0's rule that the executing agent "receives exactly one thing." The
`eustress-gauntlet` skill extracts a single item block by ID — from the `---` line above `id: <ID>`
to the last line before the next item's front matter — and hands **only that block** to the executing
agent, which never sees the pack, its siblings, or its header. §0 is satisfied where it matters: in
what lands in the executing context.

The packs exist because the alternative is 159 near-duplicate files whose shared context — the honest
state-of-the-world section, the invariants, the cross-pack ownership edges — would be copied 159
times and drift 159 ways. A pack header is written once and every item in it inherits one correct
version. **Do not reintroduce the split**: it recreates the drift and buys nothing, because
extraction already delivers the isolation §0 asks for.

## Running one item

Invoke the `eustress-gauntlet` skill. It:

1. Reads `02_QUEUE.md` and `.state/progress.json` (creating the latter if absent).
2. Picks the next item whose `depends_on` are all `PASSED` and whose program gates are clear —
   principally `PROGRAM-LICENCE-GATE` (`00_MASTER_PROTOCOL.md` §4.6), which blocks every item that
   hands an artifact to someone outside the company until a human records a licence decision.
3. Refuses to start a build-consuming item while another build holds the build slot. The workspace
   shares one `target/`; two concurrent cargo builds produce link failures and cost more wall-clock
   than running them in series.
4. Extracts that item's block by ID and follows **only** that block.
5. Runs to the exit criterion, running the literal command and reading the emitted value — never
   inferring success from a file appearing.
6. Writes the artifact at the item's declared `artifact:` path.
7. Hands off to `eustress-critic` if `critic_gate` is non-empty.

The skill never edits an exit criterion, and never edits CI to make a gate pass. Changing the
measurement instead of the artifact fails the item whatever number results.

## Running a wave

A wave is a set of items L0 has grouped in `02_QUEUE.md` to run together. Invoke `eustress-wave`. It
confirms the wave's items have **disjoint file ownership** per `04_FILE_OWNERSHIP.md`, runs tier-`S`
items (docs, specs, analysis — zero builds) concurrently, serialises every build-consuming item
through the single build slot, collects per-item results, and emits one consolidated wave scorecard.

A wave **stops on the first item that stalls**. It does not route around it. A stall is a decision
request addressed to a human, and continuing past one buries it.

## The Critic gate

The Critic is an independent, hostile, blinded evaluator outside the execution chain, never in the
same context as the candidate's self-report. It scores seven dimensions 0–10 against anchored scales.
**The floor is 8.0 on every gated dimension** — not the mean, not the median. A 9.8 average with one
6.5 is a FAIL.

Two conditions govern a pass and both are necessary: the **objective floor** (every numeric floor met
or exceeded — not subject to Critic discretion, L1 judgement, or human enthusiasm) and the **wow
gate** (the Critic affirms the artifact is genuinely excellent rather than merely compliant, and
cites what makes it so). The asymmetry is the point: **the Critic may refuse an item that clears
every number; it may never pass one that misses a number.** A wow refusal that does not name a
specific, addressable deficiency is malformed and is re-run.

Every score carries a citation — `frame:0072`, `path/file.rs:118`, `ft_p99=22.4ms`,
`series:cell_voltage t=412.0 value=3.71 unit=V`, `manifest:reference.build_version`. **An uncited
score auto-fails the entire scorecard**, not just that dimension. So does a citation that does not
resolve, and so does any candidate-authored prose in the Critic's input (→ `INPUT_CONTAMINATED`, do
not score).

Before judging anything real the Critic runs the identity trial in `01_CRITIC_RUBRIC.md` §7: two
byte-identical captures of the same commit, scored blind, every dimension within 0.5. A Critic that
spreads wider is measuring noise, and every score it has produced is void.

## Escalation and STALL

The loop is bounded. It terminates on a pass, on a kill, or on a documented stall that reaches a
human with a specific decision request.

```
Iterations 1-3 : approach A
   -> MANDATORY approach change. A parameter change is NOT an approach change.
Iterations 4-6 : approach B  (fresh agent; receives A's failure signature, NOT A's diff)
   -> MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.
```

Three early triggers jump straight to STALL without burning the remaining budget: **no progress**
(three consecutive iterations with the worst gated dimension moving < 0.5 *and* the exit measurement
moving < 5%), **budget** (150% of the tier envelope consumed), and the **item-specific** trigger in
the item's own `escalation:` field.

A STALL is a first-class, respectable outcome — information a human needs, not an agent failure. It
produces a one-screen packet in the `00_MASTER_PROTOCOL.md` §5.3 form: the floor missed and the best
value achieved, the path to the best artifact, the three approaches with their failure signatures,
the best root-cause theory with an evidence `path:line` or measured number, consumption against
envelope, and a `DECISION REQUESTED` that is **exactly one of** — `LOWER` the floor to a stated value
with its consequence · `FUND` approach D with an estimate and why it is materially different ·
`DEFER` behind a named blocking item · `KILL` with what the program loses.

A packet asking the human to "review the situation" instead of choosing among those four is
malformed and is rewritten before it is sent. Every stall also becomes a row in the stall register in
`02_QUEUE.md`, so an open decision is visible without reading a transcript.

Two neighbouring escalations use the same shape and the same hand-back discipline:
`FILE-OWNERSHIP` (`04_FILE_OWNERSHIP.md` §6) when an item cannot reach its criterion without editing
a file another item owns, and `PROMPT_UNDERSPECIFIED` / `EXIT_CRITERION_UNMEASURABLE` when the item
itself is the defect.

**Human-only decisions** (`00_MASTER_PROTOCOL.md` §6) are not an escalation ladder — they are a full
stop: changing the licence; publishing any external comparison naming a third-party product; any
external publication at all; any spend, pricing decision, or signed agreement; production deployment;
Bliss ledger design touching transferability or cash-out; and merging to `main`.
