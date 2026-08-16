# G0 — Security, Data Rights, and External Reality

**Pack ID:** `G0`
**Phase owned:** `G0` — Security, Rights & External Reality
**Status:** All items authored `DRAFT`. An L1 promotes to `READY`.
**Conforms to:** `docs/PROMPTS/03_PROMPT_SCHEMA.md`

---

## What this pack owns

Three things the rest of the library assumes and never builds.

**Half A — the trust boundary.** Eustress hands a language model `run_bash`, `write_file`,
`git_commit`, and `set_sim_value`; loads `.lua` and `.rune` plugins from a user-writable directory
into a shared VM with unrestricted `HttpService` and `DataStoreService`; and publishes a container
an outside lab is invited to run. None of that has a written threat model, a capability gate, or a
sandbox. `docs/AUDIT/MASTER.md:200` records the intended design — "**C15** *(new P4)* **Plugin
sandbox = WASM-first** — third-party plugins ship as `wasmtime`/`wasmer` modules by default;
permission scoping via capability handshake (ECS-read / ECS-write / file-IO / network); native
`.so`/`.dll` plugins are opt-in for first-party + signed authors only." That design is at 0%:
`grep -rn "wasmtime\|wasmer\|wasi" --include=Cargo.toml eustress/` returns **0 lines** (MEASURED,
2026-08-07, HEAD `71ccf6fe`).

**Half B — the terms.** Nothing in the repository states the licence under which `.etask` files,
the EUSTRESS-PHYS-12 benchmark, episode bundles, or models trained on them may be used. A lab's
counsel reads PolyForm Shield's Competition clause — goods and services compete "even when provided
free of charge" — and stops before running the container the library spends hundreds of builds
preparing. Meanwhile the live customer-facing surface says the opposite of the `LICENSE` file.

**Half C — arrival.** Pack B1 opens at a qualification scorecard and pack B2 at a SKU sheet. Both
presuppose that someone showed up. No item anywhere in the library has an exit criterion of the form
"N people outside this company did X". For a bootstrapped solo founder, arrival is the binding
constraint, and a benchmark with one measured entrant and zero external runs is a demo.

---

## The rule that shapes every item in this pack

**An artifact the executing agent produced is not evidence that anything happened outside this
company.** Five items here have exit criteria that are *countable external facts*: a stranger's
episode bundle, five strangers' discovery transcripts, an external benchmark submission, a measured
arrival count from a named channel, an acknowledged inbound security report. A synthetic fixture may
appear in those items only as a **negative control** — the thing that proves the checker rejects the
wrong shape — never as the pass.

`00_MASTER_PROTOCOL.md` §1.1 D4 — half the program's definition of done — is unreachable by any
prompt in the other seven packs. This pack is where the reachable half of it starts.

---

## `HUMAN-EXECUTED` items

Five items carry `human_executed: true` in their front matter — `G0.07`, `G0.08`, `G0.09`, `G0.10`,
and `G0.12`. In those, the executing agent's whole
deliverable is **the instrument and the ledger**: the script, the schema, the verifier, the empty
ledger with its required fields, and the runbook a human follows. The human then acts — publishes,
sends, deploys, or talks to a person — and records the result in the ledger. The item does not pass
until the ledger holds real rows that the verifier accepts.

Per `00_MASTER_PROTOCOL.md` §6, an agent may never publish externally, deploy to production, spend
money, contact a person, or sign anything. Those constraints are the reason for the split, not an
excuse for the item to stop at the instrument.

---

## Workloads this pack's evidence feeds

| Workload | How this pack produces it |
|---|---|
| **W3 — Trust & Verifiability** | Two named exploits refused at the dispatch boundary (`G0.01`), a generated capability gate (`G0.02`), an egress and secrets boundary (`G0.11`), an answered security questionnaire whose every `yes` cites an existing path (`G0.05`) |
| **W5 — Extension Surface** | The WASM plugin sandbox and its capability handshake (`G0.03`), the data-rights and third-party-use terms (`G0.04`), one external benchmark submission (`G0.10`) |
| **W2 — Revenue Rail** | Five qualified strangers arriving from named channels with archived transcripts (`G0.08`), a measured arrival baseline (`G0.09`) |
| **W1 — Provable Quality** | An episode bundle produced by an engineer outside the company that the verifier accepts (`G0.07`) |
| **W6 — Operator Leverage** | The disclosure path (`G0.12`) and the correctness of the customer-facing licence surface (`G0.06`) |

---

## ITEM ZERO

**`G0.01` — Threat model for the agent tool surface and the plugin loader, with two named exploits
refused at the dispatch boundary.**

Every other security item in this pack cites `docs/security/THREAT_MODEL.md` for its asset list,
trust boundaries, and existing controls. `G0.02`, `G0.03`, `G0.05`, `G0.11`, and `G0.12` all consume
it. Nothing in Half A may start before it.

**Dispatch `G0.06` first anyway.** `G0.06` depends on nothing but `G1.01`, the program-wide
workspace-build gate every build-consuming item carries, costs one tier-M pass, and fixes a live
customer-facing statement that contradicts the `LICENSE` file. It is not item zero because nothing
cites it; it goes first because it is currently wrong in front of people.

---

## Dependency graph

| ID | Title | Tier | Human | Depends on |
|---|---|---|---|---|
| `G0.01` | Agent-surface and plugin-loader threat model, two exploits refused | L | — | `G7.01`, `G7.19` *(ITEM ZERO)* |
| `G0.02` | Capability gate generated from the tool census | L | — | `G0.01` |
| `G0.03` | WASM-first plugin sandbox with a capability handshake | XL | — | `G0.01` |
| `G0.04` | Data rights for `.etask`, the benchmark, episodes, and derived models | S | — | `G7.12`, `G7.22`, `G1.48` |
| `G0.05` | Enterprise security-questionnaire baseline | M | — | `G0.01`, `G0.02` |
| `G0.06` | Every licensing misstatement found and fixed, with a grep verifier | M | — | `G1.01` |
| `G0.07` | An engineer outside the company produces an accepted episode bundle | M | **yes** | `G7.20`, `G7.24`, `G0.04`, `G0.06` |
| `G0.08` | Five qualified strangers complete the discovery instrument | S | **yes** | `G6.33`, `G0.09` |
| `G0.09` | Channel-attributed arrival, measured | M | **yes** | `G6.30`, `G6.31`, `G0.06` |
| `G0.10` | One external submission scored on EUSTRESS-PHYS-12 | M | **yes** | `G7.22`, `G7.23`, `G0.07` |
| `G0.11` | Egress and secrets boundary on the agent surface | L | — | `G0.01`, `G0.02` |
| `G0.12` | `SECURITY.md`, disclosure path, and one acknowledged inbound report | S | **yes** | `G0.01`, `G0.06`, `G1.48` |

Twelve items: 3 at tier S, 5 at tier M, 3 at tier L, 1 at tier XL. Cumulative envelope:
3 × 60k + 5 × 200k + 3 × 500k + 1 × 1.2M = **3.88M tokens**; build slots
3 × 0 + 5 × 6 + 3 × 12 + 1 × 20 = **86**.

The graph is acyclic. `G0.06` is this pack's entry point and reaches outside it only for `G1.01`.
`G0.01` reaches into pack T4 for `G7.01` and `G7.19`; `G0.04` reaches into T4 and B2; `G0.07`,
`G0.08`, `G0.09`, and `G0.10` reach into T4 and B1. No item in this pack is depended upon by an item
that this pack depends on.

---

## Cross-pack amendment an L1 must apply when promoting this pack

`docs/PROMPTS/packs/B2_revenue_ecosystem_org.md:1543` currently reads:

    depends_on: [G1.41, G1.42]

for `G1.46` (procurement-readiness matrix). The security-questionnaire baseline in `G0.05` is the
input that matrix's security section requires, and B2's own dependency graph table at
`docs/PROMPTS/packs/B2_revenue_ecosystem_org.md:82` must be updated to match. The amended line is:

    depends_on: [G1.41, G1.42, G0.05]

An L1 makes this edit; `blocks: [G1.46]` in `G0.05`'s front matter is informational only, because
`03_PROMPT_SCHEMA.md` §3 makes `depends_on` the authoritative edge.

---

## Why every item in this pack has an empty `critic_gate`

`01_CRITIC_RUBRIC.md`'s seven dimensions score rendered frames, motion, studio surfaces, and
simulation believability. None of them can read a refused tool call, an exit code, or a stranger's
signature. `03_PROMPT_SCHEMA.md` §3 permits `critic_gate: []` "for items whose exit criterion is
purely mechanical — but then `exit_criterion` must be unusually tight." Every criterion here is
either a measured refusal, a generated-artifact diff, a path-existence check, or a countable external
fact, and each item's §6 states the specific compensating tightness that replaces the Critic.

---

## How to use this file

Each block below is one complete prompt file. Its heading is the path it must be written to under
`docs/PROMPTS/items/`. The fenced block is the file's entire contents, front matter included. An L2
receives exactly one such block and nothing else.

---

## Cross-pack file ownership

Items in this pack share source files with items in other packs. `docs/PROMPTS/04_FILE_OWNERSHIP.md` names one owner per contested path and is normative; where it conflicts with an item's scope list, it wins. This pack's own dependency-graph table records its internal edges; the cross-pack edges are these:

| Item | Now depends on | Contested path | Effect on this item's scope |
|---|---|---|---|
| `G0.03` | `G1.01` (G1) | `eustress/Cargo.toml` | may append to but not alter it |
| `G0.03` | `G1.03` (G1) | `eustress/crates/engine/Cargo.toml` | may append to but not alter it |
| `G0.11` | `G7.08` (T4) | `eustress/crates/tools/src/simulation_tools.rs` | may no longer edit it |

An item blocked by one of these entries emits a `FILE-OWNERSHIP` decision packet to L0 (`docs/PROMPTS/04_FILE_OWNERSHIP.md` §6). It does not edit the file and does not work around it.

---


---

## `docs/PROMPTS/items/G0.01_agent-surface-threat-model.md`

````markdown
---
id: G0.01
title: Agent-surface and plugin-loader threat model with two exploits refused at dispatch
workload: W3
workload_secondary: [W5]
phase: G0
depends_on: [G7.01, G7.19]
blocks: [G0.02, G0.03, G0.05, G0.11, G0.12]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G0.01/threat_model_enforcement.json
escalation: >
  If refusing either named exploit at the dispatch boundary requires deleting a tool from the
  registry rather than refusing one invocation of it, STALL. Removing `run_bash` or `write_file`
  changes what the substrate is; the item's whole premise is that a legitimate capability can be
  refused for one caller in one context.
status: DRAFT
notes: >
  Tier L rather than M because enforcement lands in the dispatch path shared by the MCP server and
  the Engine Bridge, and each end-to-end verification needs a built `eustress-headless`.
---

## 1. Objective

`docs/security/THREAT_MODEL.md` enumerates the assets, actors, trust boundaries, and existing
controls of two surfaces — the MCP/Engine-Bridge agent tool surface and the script plugin loader —
and for each entry names the control that exists today or states plainly that none does. Two
specific exploits already proven to work against this repository are then **refused before they
execute**: the tool call returns a structured error and the file each exploit targets is
byte-identical before and after.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — a world model an AI
reasons over and a document a human edits. It is never described as a game engine. The licence is
PolyForm Shield 1.0.0, so the project is **source-available**, never open source. The physics engine
is **Avian**, never Rapier. Slint `.slint` files compile to Rust, so Slint *is* Rust. Units are
meter-native; studs are a display unit only.

**Blocked is not detected, and that distinction is this item.** `G7.19` hardened the *scorer*: an
exploit still runs, still writes what it writes, and is then denied credit. Its artifact
`docs/PROMPTS/artifacts/G7.19/exploit_report.json` records each exploit's score before and after
hardening. This item is about the other half — the exploit's tool call never reaching the
filesystem. A threat model whose exit criterion is that the document exists is a filing; a threat
model whose exit criterion is that a previously working attack now returns
`{"success": false, "code": "..."}` and leaves the disk untouched is a control.

**The two exploits you must refuse.** Do not invent them. Open
`docs/PROMPTS/artifacts/G7.19/exploit_report.json` and select the two `per_exploit` rows whose
`class` fields are exactly `"scorer_tampering"` and `"task_file_tampering"`. Record their `name`
values in your artifact — those two names are what the exit criterion is measured against. Both are
recorded in that file as having succeeded against the unhardened baseline, which is what makes them
admissible as evidence here.

**The tool surface, counted from source (MEASURED, HEAD `71ccf6fe`).** 79 `ToolHandler` impls under
`eustress/crates/tools/src/`, 24 bridge tools in `eustress/crates/mcp-server/src/bridge_tools.rs`,
and 1 hand-rolled tool in `eustress/crates/mcp-server/src/tools.rs` — **104 defined**, 90 exposed
under the default mode set (`eustress/crates/tools/src/modes.rs`). `G7.01` produced
`docs/PROMPTS/artifacts/G7.01/tool_conformance.json`, which holds a typed outcome and a measured
latency for every one of the 104. Use it as your asset inventory; do not re-census.

**The four tools that make this urgent, verified in source.**

- `run_bash` — `eustress/crates/tools/src/shell_tools.rs:65`. Its own module documentation at
  `:1-15` calls it "the escape hatch — anything we haven't wrapped as a first-class tool becomes a
  bash invocation." It passes the whole string to `bash -c`. `requires_approval: true` at `:88`.
- `write_file` — `eustress/crates/tools/src/file_tools.rs:232`, `requires_approval: false` at `:243`.
- `stage_file_change` — `eustress/crates/tools/src/diff_tools.rs:172`,
  `requires_approval: false` at `:185`.
- `git_commit` — `eustress/crates/tools/src/git_tools.rs:113`, `requires_approval: true` at `:124`.

**`requires_approval` is advisory and is not enforced on the MCP path.** Three verified facts:

1. `eustress/crates/tools/src/registry.rs:36-38` documents the field as "Whether the tool requires
   user approval before execution in the Workshop agent loop. MCP clients interpret this as a
   **hint** — external IDEs may always require approval."
2. `eustress/crates/mcp-server/src/shared_registry.rs:223` maps it to the MCP `destructiveHint`
   annotation. An annotation is a label sent to the client; it stops nothing on the server.
3. The MCP `tools/call` handler at `eustress/crates/mcp-server/src/main.rs:463-521` reads
   `params.name`, looks the tool up in `tools::all_tools()` and then in
   `shared_registry::try_dispatch`, and executes. **It never reads `requires_approval`.** The only
   place that field gates anything is the in-engine Workshop loop at
   `eustress/crates/engine/src/workshop/claude_bridge.rs:837-860`. An external MCP client that
   ignores the hint therefore has an ungated path to `run_bash`.

**The Engine Bridge widens it further.** `eustress/crates/engine/src/engine_bridge/protocol.rs`
exposes JSON-RPC 2.0 over TCP with `tools.call` and `tools.list`, reachable through
`eustress/crates/bridge-client/`. Discovery is a port file at `<universe>/.eustress/engine.port`.
`action.invoke` is documented at `protocol.rs:2306-2307` with: "some actions are destructive
(Delete/Cut). This is a deliberately-driven test surface (`requires_approval` is handled at the MCP-
tool layer), so the handler does not gate them here." Both layers point at the other one.

**The plugin loader, verified in source.**
`eustress/crates/engine/src/script_plugin_host.rs:204` resolves the plugin directory as
`dirs::data_local_dir()/Eustress/Plugins`, creates it if absent, and loads every `*.lua` and
`*.rune` file it finds. The module documentation at `:19-21` states the trust position in its own
words: "this engine's Luau globals already give any script unrestricted
`HttpService`/`DataStoreService` access". That is accurate —
`eustress/crates/common/src/luau/runtime.rs:1108` injects `HttpService` (`GetAsync`, `PostAsync`,
`RequestAsync` at `:3014`, `:3024`, `:3037`) and `:2995` sets `DataStoreService` on the shared
globals. `build_plugin_environment` at `runtime.rs:520` gives each plugin its own environment table
whose metatable `__index` is `lua.globals()` (`runtime.rs:595-596`) — so per-plugin *write*
isolation exists and *read* access to every injected service is deliberate and total. There is no
signature check, no manifest, no capability declaration, and no allowlist. Dropping a file into a
user-writable directory is the entire installation procedure.

**What does not exist, so you cannot cite it.** There is no `SECURITY.md` and no `docs/SECURITY.md`
at any path in this repository (`ls SECURITY.md docs/SECURITY.md` → no such file). There is no
`CONTRIBUTING.md`. `docs/security/` does not exist; you create it.

**Build reality.** A full engine build takes 10–15 minutes. Only one cargo build may run at a time —
the workspace shares a single `eustress/target/` and concurrent builds produce link failures. Never
kill a build mid-compile. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `docs/security/THREAT_MODEL.md` — create
- `docs/security/threat_model.json` — create; the machine-readable twin the verifier reads
- `eustress/crates/tools/src/` — the dispatch-boundary refusal only; no tool may be deleted
- `eustress/crates/mcp-server/src/` — the `tools/call` and shared-registry dispatch path
- `eustress/crates/agent-eval/` — the refusal-verification binary
- `docs/PROMPTS/harness/checkers/threat_model.py` — create
- `docs/PROMPTS/artifacts/G0.01/` — the artifact directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G7.19/exploit_report.json` and `scripts/agent_eval/exploits/` — frozen
  inputs; editing an exploit so it fails is the exact fraud this item exists to preclude
- `docs/PROMPTS/artifacts/G7.01/tool_conformance.json` — a frozen input
- `eustress/crates/engine/src/script_plugin_host.rs` — the loader is modelled here and changed in
  `G0.03`; this item writes down what it does, it does not rewrite it
- `LICENSE`, `LICENSE-COMMERCIAL.md` — licence terms are a human decision
- The layout of `eustress/crates/agent-eval/` — the directory is owned by `G7.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.
- The layout of `eustress/crates/tools/src/` — the directory is owned by `G7.02`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Editing either exploit, narrowing its target path, marking an exploit out of scope, refusing it by
  deleting the tool it uses, or counting a crashed episode as a refusal are all measurement changes.
  If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The refusal must return a **structured error the caller can act on** — `success: false` with a
  stable machine-readable code — not a panic, not a timeout, and not a silent no-op. A silent no-op
  is the defect class `G7.01` was built to count; producing one here fails the item outright.
- Both exploits must be re-run **unmodified**, from the same definitions under
  `scripts/agent_eval/exploits/`, and both must have been recorded in
  `docs/PROMPTS/artifacts/G7.19/exploit_report.json` as succeeding against the unhardened baseline.
- Every threat entry must name a **control that exists at a `path:line`** or state
  `"control": null`. An entry whose control is a plan, an intention, or a downstream item id is
  `null`. Honest nulls are the point of the document; `G0.02`, `G0.03`, `G0.05`, and `G0.11` are
  scoped from them.
- Do not write a threat model for a surface you did not verify. Every asset must trace to a
  `path:line` in this repository or to a row in `tool_conformance.json`.
- Batch your builds. Twelve slots covers three approaches; one build should validate both refusals.

## 5. Exit criterion

### Criterion
`threat_model.json` carries **at least 24** entries covering **both** surfaces with **at least 8**
entries on the plugin loader, **zero** entries missing an `asset`, `actor`, `trust_boundary`, or
`control` field, and **zero** `yes`-shaped controls citing a path that does not exist. The two
exploits selected by class from `G7.19` are re-run unmodified and **both** are refused: exit code
`2` from the refusal runner for each, a structured error code on each response, and a **byte-
identical** sha256 on each exploit's target file before and after. **Zero** tools were removed from
the registry.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

python docs/PROMPTS/harness/checkers/threat_model.py \
    --model docs/security/threat_model.json \
    --repo-root . \
    --min-entries 24 --min-plugin-entries 8
echo "MODEL_EXIT=$?"

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_suite_fixtures.ps1 -Out $env:EUSTRESS_WORKSPACE

cargo run --release --package eustress-agent-eval --bin eustress-refusal-check -- `
  --headless-bin .\eustress\target\release\eustress-headless.exe `
  --workspace $env:EUSTRESS_WORKSPACE `
  --exploit-report docs/PROMPTS/artifacts/G7.19/exploit_report.json `
  --select-class scorer_tampering `
  --select-class task_file_tampering `
  --registry-count-before 104 `
  --out docs/PROMPTS/artifacts/G0.01/threat_model_enforcement.json
echo "REFUSAL_EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "threat_entries": 27,
  "plugin_loader_entries": 9,
  "controls_null": 14,
  "controls_cited_path_missing": 0,
  "registry_descriptors_before": 104,
  "registry_descriptors_after": 104,
  "refusals": [
    { "exploit_name": "write_over_scorer_output", "class": "scorer_tampering",
      "unhardened_score_in_G7_19": 1.0,
      "tool_invoked": "write_file",
      "response_success": false,
      "error_code": "capability_denied",
      "target_path": "…/scoring_result.json",
      "target_sha256_before": "9f2c…", "target_sha256_after": "9f2c…",
      "target_unchanged": true,
      "runner_exit": 2 },
    { "exploit_name": "rewrite_etask_hash", "class": "task_file_tampering",
      "tool_invoked": "stage_file_change",
      "response_success": false,
      "error_code": "capability_denied",
      "target_sha256_before": "1ab7…", "target_sha256_after": "1ab7…",
      "target_unchanged": true,
      "runner_exit": 2 }
  ],
  "silent_noops": 0,
  "panics": 0,
  "timeouts": 0
}
```

Pass condition:

```
MODEL_EXIT == 0
AND REFUSAL_EXIT == 0
AND threat_entries >= 24
AND plugin_loader_entries >= 8
AND controls_cited_path_missing == 0
AND registry_descriptors_after == registry_descriptors_before
AND len(refusals) == 2
AND every refusals[i].response_success == false
AND every refusals[i].error_code is a non-empty string
AND every refusals[i].target_unchanged == true
AND silent_noops == 0 AND panics == 0 AND timeouts == 0
```

Read the emitted `target_sha256_before` / `target_sha256_after` pair for each refusal. A refusal
where the file changed is a detection, not a block, and fails the item. The runner must exit
non-zero if either exploit executes, if either response is success-shaped, or if the registry
descriptor count moved.

## 6. Critic gate

`critic_gate` is `[]`. No dimension in `01_CRITIC_RUBRIC.md` can read a refused tool call. The
compensating tightness is unusually heavy and deliberately so: two exploits selected **by class from
a frozen upstream artifact** rather than authored here; each required to have succeeded against the
unhardened baseline; refusal proven by a before/after content hash rather than by a return value; a
registry-count invariant that fails the item if any tool was deleted; a zero-tolerance bar on silent
no-ops, panics, and timeouts; and a path-existence check on every cited control so the document
cannot claim protection that is not in the tree.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — refuse at the shared dispatch chokepoint, keyed on the target path
                 relative to the active episode's protected set
   -> if still failing, MANDATORY approach change. Adding one more protected path is NOT an
      approach change; moving the boundary from path-matching to a per-invocation capability
      token issued by the episode runner is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the refused-exploit count unchanged and
                  controls_cited_path_missing unchanged
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: the only available refusal deletes a tool from the registry (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the refusal requirement to a
stated subset with the stated consequence; FUND a specific approach D with an estimate and a reason
it is materially different; DEFER behind a named blocking item; or KILL with a statement of what the
program loses.

## 8. Artifact

`docs/PROMPTS/artifacts/G0.01/threat_model_enforcement.json`, with the human-readable model at
`docs/security/THREAT_MODEL.md` and its machine-readable twin at `docs/security/threat_model.json`.

A reader finds: the entry count and plugin-loader entry count; how many controls are honestly
`null`; the registry descriptor count before and after, proving nothing was removed; and for each of
the two exploits its name, its class, its score against the unhardened baseline in `G7.19`, the tool
it invoked, the structured error it received, and the identical before/after sha256 of the file it
tried to write. This file is the W3 evidence for the item and the input `G0.02`, `G0.03`, `G0.05`,
`G0.11`, and `G0.12` each scope themselves from.

## 9. Definition of NOT done

- `docs/security/THREAT_MODEL.md` is written, thorough, and nothing is refused. That is a filing.
  The document is the cheap half; the two hashes are the item.
- An exploit is refused because its definition was edited. Both exploits run unmodified from
  `scripts/agent_eval/exploits/`, and the runner records their `G7.19` baseline scores to prove they
  were once effective.
- The refusal is a silent no-op: the tool returns a success-shaped response and does nothing. That
  is the exact defect class `G7.01` exists to count, and it is worse than the exploit, because the
  agent proceeds on a false premise.
- `run_bash` or `write_file` is deleted from the registry. That changes what the substrate is to
  close two attacks, and it is the escalation trigger.
- The refusal fires but the target file's sha256 changed — the write happened and the error came
  after. Detection, not a block.
- A threat entry cites a control at a `path:line` that does not exist, or names a downstream item id
  as its control. A planned control is `"control": null`.
- The plugin loader is modelled in one line as "loads plugins" with no trust boundary. It is the
  surface with **zero** controls today, the one `MASTER.md` C15 says should be WASM-first, and the
  one B2's `G1.50` proof depends on; eight entries is the floor for a reason.
- The model covers the MCP path and ignores the Engine Bridge, so `tools.call` over TCP remains an
  unmodelled second door to the same 104 descriptors.
````

---
## `docs/PROMPTS/items/G0.02_capability-gate-from-census.md`

````markdown
---
id: G0.02
title: Capability gate on the mutating tool set, generated from the tool census
workload: W3
workload_secondary: [W6]
phase: G0
depends_on: [G0.01]
blocks: [G0.05, G0.11]
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G0.02/capability_gate_conformance.json
escalation: >
  If the policy cannot be generated from `tool_conformance.json` plus `ToolHandler::read_only()`
  because those two sources disagree about more than 5 of the 104 descriptors, STALL and report the
  disagreeing rows. A hand-adjudicated policy is the artifact this item exists to replace, and
  quietly hand-fixing five rows is how it becomes one.
status: DRAFT
notes: >
  Tier L: enforcement lands in the dispatch path shared by the MCP server and the Engine Bridge, and
  each end-to-end verification needs a built `eustress-headless`.
---

## 1. Objective

Every one of the 104 defined tool descriptors carries exactly one capability, and that mapping is
**generated** from `docs/PROMPTS/artifacts/G7.01/tool_conformance.json` and the handlers' own
`ToolHandler::read_only()` declarations rather than maintained by hand. With no capability grant in
the environment, every mutating descriptor is refused with a structured error and none executes.
Adding a new mutating tool to the registry without a policy entry makes the drift check fail.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine. The
licence is PolyForm Shield 1.0.0, so **source-available**, never open source. Physics is **Avian**.
Slint compiles to Rust. Units are meter-native.

**Why generated and not hand-written.** A hand-maintained allowlist of dangerous tools is correct on
the day it is written and wrong on the day the next tool lands. This repository has 104 descriptors
across 13 files plus a bridge module plus one hand-rolled tool; a list maintained separately from
them will drift, silently, in the direction of permissiveness. The generated policy plus a drift
check that fails on an unmapped descriptor is the only version of this that survives contact with
the next commit.

**The two generation sources, both verified.**

1. `docs/PROMPTS/artifacts/G7.01/tool_conformance.json` — produced by `G7.01`, holds one row per
   defined descriptor with its typed outcome class, whether it is exposed under the default mode
   set, and its measured p50/p95 latency. It is the authoritative census of what exists.
2. `ToolHandler::read_only()` at `eustress/crates/tools/src/registry.rs:99`, surfaced through
   `ToolRegistry::all_tools_annotated()` at `registry.rs:282` and consumed by
   `eustress/crates/mcp-server/src/shared_registry.rs:208-231`, which emits it as the MCP
   `readOnlyHint` annotation. That is a declaration the handler makes about itself, in the same file
   as the handler, so it cannot drift away from the code it describes.

**What is enforced today: nothing.** `requires_approval` at
`eustress/crates/tools/src/registry.rs:36-38` is documented as a hint MCP clients may ignore.
`eustress/crates/mcp-server/src/shared_registry.rs:223` emits it as the `destructiveHint`
annotation. The `tools/call` handler at `eustress/crates/mcp-server/src/main.rs:463-521` never reads
it — it resolves the name against `tools::all_tools()`, falls through to
`shared_registry::try_dispatch`, and executes. `G0.01` produced
`docs/security/threat_model.json`; the entries whose `control` is `null` on the agent-surface half
are the ones this item closes.

**The two dispatch chokepoints you must gate.** There are exactly two, and gating one is half a
gate:

- `eustress/crates/mcp-server/src/main.rs:463` — the MCP `tools/call` arm, covering both the
  hand-rolled `tools::all_tools()` path and `shared_registry::try_dispatch`.
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — the JSON-RPC 2.0 `tools.call` method over
  TCP, reachable through `eustress/crates/bridge-client/` via the port file at
  `<universe>/.eustress/engine.port`. `protocol.rs:2306-2307` documents `action.invoke` as
  ungated on the assumption that approval "is handled at the MCP-tool layer". It is not.

**The capability vocabulary.** Use exactly these six and no others, so the policy is diffable and
`G0.11` can extend the `net` axis without renaming anything:
`read`, `world.write`, `fs.write`, `process.exec`, `vcs.write`, `net`.
Every descriptor maps to exactly one. `read` is granted by default; the other five are not.

**Known mappings you can check your generator against, all verified in source.**
`run_bash` (`eustress/crates/tools/src/shell_tools.rs:65`) → `process.exec`.
`write_file` (`file_tools.rs:232`) and `stage_file_change` (`diff_tools.rs:172`) → `fs.write`.
`git_commit` (`git_tools.rs:113`) and `git_branch` (`git_tools.rs:307`) → `vcs.write`.
`http_request` (`simulation_tools.rs:454`) → `net`; it is the one descriptor
`shared_registry.rs:226` marks `openWorldHint`. `read_file` (`file_tools.rs:32`),
`list_directory` (`file_tools.rs:123`), `git_status` (`git_tools.rs:63`), `git_log`
(`git_tools.rs:194`), and `git_diff` (`git_tools.rs:247`) → `read`.
`delete_entity` (`entity_tools.rs:677` sets `requires_approval: true`) → `world.write`.

**The environment variable.** `EUSTRESS_AGENT_CAPABILITIES`, a comma-separated list. Unset or empty
means `read` only. Follow the precedent of `EUSTRESS_WORKSPACE`
(`eustress/crates/engine/src/space/mod.rs:119 workspace_root()`), which is the established
explicit-override environment hook and the CI/container entry point.

**Build reality.** A full engine build takes 10–15 minutes, one at a time, never killed
mid-compile. Validate with `cargo run`, not `cargo check` — `cargo check` will not catch the
dispatch-registration failures this item is most likely to produce.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/tools/src/` — the capability declaration surface and the policy loader
- `eustress/crates/mcp-server/src/` — the `tools/call` gate
- `eustress/crates/engine/src/engine_bridge/` — the `tools.call` gate
- `eustress/crates/agent-eval/` — the policy generator and the drift check binary
- `eustress/crates/tools/capability_policy.json` — **generated**, committed
- `docs/PROMPTS/artifacts/G0.02/` — the artifact directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G7.01/tool_conformance.json` — a frozen generation input
- `docs/security/threat_model.json` — a frozen input from `G0.01`
- `eustress/crates/engine/src/script_plugin_host.rs` — the plugin surface is `G0.03`
- Any tool's behaviour. This item gates invocation; it does not change what a tool does when
  allowed.
- The layout of `eustress/crates/agent-eval/` — the directory is owned by `G7.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.
- The layout of `eustress/crates/tools/src/` — the directory is owned by `G7.02`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Hand-editing `capability_policy.json` after generation, adding a per-tool exception field,
  classifying a mutating tool as `read` to reduce the refusal count, or making the drift check
  warn instead of fail are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- `capability_policy.json` must be **byte-reproducible** from its two inputs. The verifier
  regenerates it and diffs; a non-empty diff fails the item. That property is what makes the word
  "generated" mean something.
- The refusal must be a structured error with a stable code, not a panic, not a timeout, and never a
  silent no-op.
- Gate **both** chokepoints. A verification that only exercises the MCP path leaves the TCP bridge
  open to the same 104 descriptors and fails the item.
- Do not grant capabilities implicitly from the active `WorkshopMode`
  (`eustress/crates/tools/src/modes.rs`). Mode filtering decides what is *listed*; the capability
  grant decides what may *run*. Conflating them means changing mode silently changes authority.
- Batch your builds. One build should validate the generator, both gates, and the drift check.

## 5. Exit criterion

### Criterion
`capability_policy.json` maps **104** descriptors with **0** unmapped and **0** mapped to more than
one capability, and regenerating it produces a **zero-line diff**. With `EUSTRESS_AGENT_CAPABILITIES`
unset, **100%** of the non-`read` descriptors are refused on **both** the MCP and the bridge
chokepoint, **0** execute, and the world digest is unchanged. With
`EUSTRESS_AGENT_CAPABILITIES=world.write`, **exactly** the `world.write` set executes and every
other non-`read` descriptor is still refused. A synthetic descriptor injected into the registry with
no policy entry makes the drift check exit **non-zero**.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

cargo run --release --package eustress-agent-eval --bin eustress-capability-policy -- `
  --census docs/PROMPTS/artifacts/G7.01/tool_conformance.json `
  --emit eustress/crates/tools/capability_policy.json
git diff --exit-code -- eustress/crates/tools/capability_policy.json
echo "REGEN_DIFF_EXIT=$LASTEXITCODE"

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
pwsh -File scripts/agent_eval/make_suite_fixtures.ps1 -Out $env:EUSTRESS_WORKSPACE

Remove-Item Env:\EUSTRESS_AGENT_CAPABILITIES -ErrorAction SilentlyContinue
cargo run --release --package eustress-agent-eval --bin eustress-capability-check -- `
  --headless-bin .\eustress\target\release\eustress-headless.exe `
  --workspace $env:EUSTRESS_WORKSPACE `
  --policy eustress/crates/tools/capability_policy.json `
  --chokepoint mcp --chokepoint bridge `
  --grant-set "" `
  --grant-set "world.write" `
  --inject-unmapped-descriptor `
  --out docs/PROMPTS/artifacts/G0.02/capability_gate_conformance.json
echo "GATE_EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "descriptors_total": 104,
  "descriptors_unmapped": 0,
  "descriptors_multi_mapped": 0,
  "policy_regenerated_diff_lines": 0,
  "capability_counts": { "read": 61, "world.write": 29, "fs.write": 6,
                         "process.exec": 1, "vcs.write": 6, "net": 1 },
  "runs": [
    { "grant_set": "", "chokepoint": "mcp",
      "non_read_attempted": 43, "refused": 43, "executed": 0,
      "world_digest_before": "c41e…", "world_digest_after": "c41e…",
      "silent_noops": 0, "panics": 0, "timeouts": 0 },
    { "grant_set": "", "chokepoint": "bridge",
      "non_read_attempted": 43, "refused": 43, "executed": 0,
      "world_digest_before": "c41e…", "world_digest_after": "c41e…",
      "silent_noops": 0, "panics": 0, "timeouts": 0 },
    { "grant_set": "world.write", "chokepoint": "mcp",
      "world_write_attempted": 29, "world_write_executed": 29,
      "other_non_read_attempted": 14, "other_non_read_refused": 14 }
  ],
  "drift_check_on_unmapped_descriptor_exit": 6
}
```

Pass condition:

```
REGEN_DIFF_EXIT == 0  AND  GATE_EXIT == 0
AND descriptors_total == 104
AND descriptors_unmapped == 0  AND  descriptors_multi_mapped == 0
AND policy_regenerated_diff_lines == 0
AND for both chokepoints under grant_set "": refused == non_read_attempted AND executed == 0
    AND world_digest_before == world_digest_after
AND under grant_set "world.write": world_write_executed == world_write_attempted
    AND other_non_read_refused == other_non_read_attempted
AND silent_noops == 0 AND panics == 0 AND timeouts == 0 on every run
AND drift_check_on_unmapped_descriptor_exit != 0
```

Read the emitted counts and the world digest pair. "The policy file exists" is not a pass; the
zero-line regeneration diff and the unchanged digest are.

## 6. Critic gate

`critic_gate` is `[]`. No rubric dimension can read a capability refusal. The compensating
tightness: the policy must regenerate byte-identically from two frozen inputs, so it cannot be
hand-tuned; both chokepoints are exercised, so gating one does not pass; the default-deny run must
leave the world digest unchanged, so a refusal that already mutated fails; the grant run must prove
the gate is not simply "refuse everything"; and the injected unmapped descriptor must break the
drift check, which is the only mechanical proof that the policy is derived rather than curated.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — capability declared per handler, policy emitted by joining the
                 census with `read_only()`, enforcement in a shared pre-dispatch guard
   -> if still failing, MANDATORY approach change. Adding a seventh capability name is NOT an
      approach change; moving from a static policy file to a capability token minted per session
      and carried on every call is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with descriptors_unmapped unchanged and
                  refused/non_read_attempted moving < 5%
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: census and `read_only()` disagree on more than 5 descriptors (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G0.02/capability_gate_conformance.json`, with the generated policy at
`eustress/crates/tools/capability_policy.json`.

A reader finds: the descriptor total and the per-capability counts; proof the policy regenerates
with a zero-line diff; for each chokepoint and each grant set, how many non-`read` descriptors were
attempted, refused, and executed, with the world digest before and after; the zero counts for silent
no-ops, panics, and timeouts; and the non-zero exit the drift check produced when an unmapped
descriptor was injected. This is the W3 evidence and the input `G0.05` cites for its access-control
questions and `G0.11` extends along the `net` axis.

## 9. Definition of NOT done

- `capability_policy.json` is committed but was written by hand, so regeneration produces a diff.
  The word "generated" is the entire load-bearing property; a curated list is what already failed.
- The MCP path is gated and the Engine Bridge is not, so `tools.call` over TCP still reaches all 104
  descriptors. Two doors, one lock.
- A refused call returns a success-shaped response with no effect. That is a silent no-op, the
  defect class `G7.01` exists to count.
- Mode filtering is used as the gate. `modes.rs` decides what is listed; a listing filter is not an
  authority boundary, and conflating them means switching mode silently grants authority.
- The default-deny run refuses everything including `read`, so the gate is proven only by making the
  substrate unusable. The `world.write` grant run exists to catch that.
- The drift check warns on an unmapped descriptor instead of failing. A warning is a hint, and this
  item exists because hints do not stop anything.
- A tool was reclassified from mutating to `read` so it would stop appearing in the refusal count.
````

---

## `docs/PROMPTS/items/G0.03_wasm-plugin-sandbox.md`

````markdown
---
id: G0.03
title: WASM-first plugin sandbox with a capability handshake, from a measured zero
workload: W5
workload_secondary: [W3]
phase: G0
depends_on: [G0.01, G1.01, G1.03]
blocks: [G1.50]
tier: XL
token_envelope: 1200000
wallclock_envelope: 5d
max_builds: 20
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G0.03/sandbox_conformance.json
escalation: >
  If a sandboxed plugin cannot register a ribbon section and button without being granted a
  capability equivalent to full host access — that is, if the only working handshake is "all or
  nothing" — STALL. A sandbox whose single capability is everything is a rename, and shipping it
  would let `G0.05` and `G1.46` claim isolation the substrate does not have.
status: DRAFT
notes: >
  Tier XL: a new runtime subsystem. Twenty build slots is the whole budget for three approaches, so
  design each iteration to validate the host, the handshake, and the fuel bound in one build.
---

## 1. Objective

A `.wasm` plugin placed in the plugin directory is instantiated in a `wasmtime` host, declares the
capabilities it needs, registers a ribbon section and button through host imports, and is **denied**
at the host boundary when it calls an import outside its declared set. A plugin that never returns
is terminated by a fuel bound without taking the engine down. The pre-existing unsandboxed `.lua`
and `.rune` loader still works, but only behind an explicit opt-in that is off by default.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine. The
licence is PolyForm Shield 1.0.0 — **source-available**, never open source. Physics is **Avian**.
Slint compiles to Rust. Units are meter-native.

**The design this implements, quoted.** `docs/AUDIT/MASTER.md:200` records cross-cutting concern
C15: "**Plugin sandbox = WASM-first** — third-party plugins ship as `wasmtime`/`wasmer` modules by
default; permission scoping via capability handshake (ECS-read / ECS-write / file-IO / network);
native `.so`/`.dll` plugins are opt-in for first-party + signed authors only. Same model for IDE
extensions (LSP) and Slint custom components."

**Start from zero and say so.** MEASURED at HEAD `71ccf6fe`, 2026-08-07:
`grep -rn "wasmtime\|wasmer\|wasi" --include=Cargo.toml eustress/` returns **0 lines**. There is no
WASM host, no capability handshake, and no signature check anywhere in the workspace. C15 is a
design note, not a partial implementation, and your artifact must record the zero as the baseline
rather than implying incremental progress from something.

**What exists today, verified in source.**
`eustress/crates/engine/src/script_plugin_host.rs:204` resolves the plugin directory as
`dirs::data_local_dir()/Eustress/Plugins`, creates it if missing, reads it (`:213`), and loads every
`*.lua` and `*.rune` file. On success it emits at `info` level, at `:284`:

    🔌 Script plugin discovery: {discovered_count} plugin(s) loaded from {plugins_dir:?}

and per Luau plugin, at `:290`: `🔌 Script plugin loaded (Luau): {plugin_id} ...`. A failed load
raises a notification, `Plugin '{id}' failed to load: {error}`, rather than a log line — so absence
of an error is not evidence of a load.

The module documentation states the trust position in its own words at `:19-21`: "this engine's
Luau globals already give any script unrestricted `HttpService`/`DataStoreService` access". Verified:
`eustress/crates/common/src/luau/runtime.rs:1108` injects `HttpService`, whose `GetAsync`,
`PostAsync`, and `RequestAsync` are set at `:3014`, `:3024`, and `:3037`; `:2995` sets
`DataStoreService` on the shared globals. `LuauRuntime::build_plugin_environment`
(`runtime.rs:520`) gives each plugin its own environment table, but its metatable `__index` is
`lua.globals()` (`runtime.rs:595-596`), so every injected service is reachable by design. The module
documentation at `:23-27` also records, correctly, that `mlua`'s `sandbox(true)` gives **no**
per-chunk write isolation, verified against mlua 0.10.5 source.

Rune plugins compile through `rune::prepare(...).build()` and require an explicit top-level
`register()` function (`script_plugin_host.rs:297-305`). Luau chunks execute top-to-bottom
implicitly. Registration for both lands on one `PluginBridge` queue in
`eustress/crates/common/src/script_plugins/` (files `mod.rs` and `bridge.rs`), drained onto the
single `PLUGINS_TAB_ID = "plugins"` ribbon tab (`script_plugin_host.rs:56`) via the `TabRegistry` at
`eustress/crates/engine/src/studio_plugins/tab_api.rs`. The registration API is `plugin:AddSection`
and `plugin:AddButton` in Luau and `plugin_add_section` / `plugin_add_button` in Rune.

**What depends on you not breaking the old path.** Pack B2's `G1.50` proves the extension surface is
real by dropping a `.lua` or `.rune` file into `%LOCALAPPDATA%/Eustress/Plugins/` and measuring the
wall-clock time to a visible button, reading the `:284` discovery line. Its artifact is
`docs/PROMPTS/artifacts/B2/G1.50/extension_proof.json`. Your opt-in must therefore be a **named,
documented setting** whose exact key and value you record in your artifact, so `G1.50`'s runbook can
set it in one line. Do not remove the legacy loader and do not leave it on by default.

**Where the opt-in lives.** `eustress/crates/engine/src/editor_settings.rs` is the existing editor
settings surface. Add the flag there rather than inventing a second settings store.

**Fuel, not a timeout thread.** `wasmtime`'s fuel metering is the mechanism that terminates a
non-returning guest deterministically. A wall-clock watchdog on another thread is not equivalent and
will not produce a reproducible number.

**Build reality.** A full engine build takes 10–15 minutes. One cargo build at a time — the
workspace shares a single `eustress/target/` and concurrent builds produce link failures. Never kill
a build mid-compile. Adding `wasmtime` will make the first build after the dependency change longer
than usual; budget for it. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/plugin-host/` — a new crate for the WASM host, its capability handshake, and the
  host-import surface
- `eustress/Cargo.toml` — the single line adding `"crates/plugin-host"` to `members`
- `eustress/crates/plugin-host/Cargo.toml` — the `wasmtime` dependency
- `eustress/crates/engine/src/script_plugin_host.rs` — dispatch `.wasm` to the new host; gate the
  legacy `.lua`/`.rune` path behind the opt-in
- `eustress/crates/engine/src/editor_settings.rs` — the opt-in flag
- `eustress/crates/engine/Cargo.toml` — the dependency on the new crate
- `docs/architecture/PLUGIN_SANDBOX.md` — create
- `scripts/plugin_sandbox/` — the fixture plugins and their build script
- `docs/PROMPTS/artifacts/G0.03/` — the artifact directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/security/threat_model.json` — a frozen input from `G0.01`
- `eustress/crates/common/src/luau/runtime.rs` — do not weaken or rewire the Luau service injection;
  the legacy path's exposure is a modelled fact, and this item's answer to it is the opt-in, not a
  half-measure inside the shared VM
- `eustress/crates/tools/src/`, `eustress/crates/mcp-server/src/` — the agent tool surface is
  `G0.02` and `G0.11`
- `docs/AUDIT/MASTER.md` — C15 is the input, not the deliverable
- Any existing entry in `eustress/crates/engine/Cargo.toml` — the file is owned by `G1.03`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.
- Any existing entry in `eustress/Cargo.toml` — the file is owned by `G1.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`) and is append-only for every other pack. Adding a new
  entry is permitted; altering, reordering, or removing an existing one is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Granting the fixture plugin every capability so no denial occurs, replacing the fuel bound with a
  wall-clock sleep, testing the denial against an import the host never provided, or counting a
  plugin that failed to instantiate as "denied" are all measurement changes. If the measurement is
  genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The denial must be at the **host boundary**: the guest calls a real host import that is genuinely
  wired for a granted plugin, and the host refuses because this plugin did not declare it. Denying
  by not linking the import at all proves nothing about the handshake.
- Two fixture plugins, differing **only** in their declared capability set, must produce
  `allowed == 1` and `denied == 1` on the same import. Same binary, different manifest.
- The legacy loader must remain functional under the opt-in and must be **off by default**. Record
  the exact settings key and value in the artifact so `G1.50` can set it.
- Do not claim the WASM path replaces the Luau path. Document what the sandboxed surface can and
  cannot do; a plugin that cannot yet do what `plugin:AddSection` does is a smaller surface, and
  saying so is the honest artifact.
- Batch your builds. Twenty slots across three approaches means roughly six per approach; one build
  should validate the host, both fixtures, and the fuel bound.

## 5. Exit criterion

### Criterion
`wasmtime` appears in exactly one workspace `Cargo.toml` where **0** appeared before. The granted
fixture plugin instantiates, registers **1** ribbon section and **1** button visible in the
`TabRegistry`, and reaches the network host import **1** time successfully. The ungranted fixture —
the same guest binary with a different manifest — is denied on that import **1** time with **0**
successes, and the engine remains alive. A non-returning fixture is terminated by fuel exhaustion in
**≤ 2.0 s**, after which discovery of the remaining plugins **completes**. With the opt-in unset,
legacy `.lua` and `.rune` plugins loaded is **0**; with it set, it is **≥ 1**.

### Measurement

Command:

```
grep -rn "wasmtime\|wasmer\|wasi" --include=Cargo.toml eustress/ | wc -l
echo "WASM_DEP_LINES=$?"

pwsh -File scripts/plugin_sandbox/build_fixtures.ps1
cargo build --release --package eustress-engine

cargo run --release --package eustress-plugin-host --bin eustress-sandbox-check -- `
  --fixtures scripts/plugin_sandbox/fixtures `
  --granted   net_granted.wasm `
  --ungranted net_ungranted.wasm `
  --runaway   infinite_loop.wasm `
  --fuel-budget 200000000 `
  --legacy-optin-key studio.plugins.allow_unsandboxed `
  --engine-log-scan `
  --out docs/PROMPTS/artifacts/G0.03/sandbox_conformance.json
echo "SANDBOX_EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "wasm_dep_lines_before": 0,
  "wasm_dep_lines_after": 1,
  "granted":   { "instantiated": true, "sections_registered": 1, "buttons_registered": 1,
                 "net_import_allowed": 1, "net_import_denied": 0 },
  "ungranted": { "instantiated": true, "sections_registered": 1, "buttons_registered": 1,
                 "net_import_allowed": 0, "net_import_denied": 1,
                 "same_guest_binary_as_granted": true },
  "runaway":   { "terminated_by": "fuel_exhausted", "termination_s": 0.41,
                 "engine_alive_after": true, "remaining_discovery_completed": true },
  "legacy": { "optin_key": "studio.plugins.allow_unsandboxed",
              "loaded_with_optin_unset": 0,
              "loaded_with_optin_set": 2,
              "discovery_line_seen": true },
  "sandboxed_surface_gaps": ["no GetSelection host import yet",
                             "no notification host import yet"]
}
```

Pass condition:

```
SANDBOX_EXIT == 0
AND wasm_dep_lines_before == 0 AND wasm_dep_lines_after >= 1
AND granted.sections_registered == 1 AND granted.buttons_registered == 1
AND granted.net_import_allowed == 1
AND ungranted.net_import_denied == 1 AND ungranted.net_import_allowed == 0
AND ungranted.same_guest_binary_as_granted == true
AND runaway.terminated_by == "fuel_exhausted" AND runaway.termination_s <= 2.0
AND runaway.engine_alive_after == true AND runaway.remaining_discovery_completed == true
AND legacy.loaded_with_optin_unset == 0 AND legacy.loaded_with_optin_set >= 1
AND sandboxed_surface_gaps is a list (may be empty, must be present)
```

Read the emitted counters. `same_guest_binary_as_granted` must be `true` — if the denial came from a
different binary, the handshake was not what refused it.

## 6. Critic gate

`critic_gate` is `[]`. The compensating tightness: the denial and the allowance must come from the
**same guest binary** with different manifests, which is the only way to prove a capability
handshake rather than two differently-built plugins; termination must be attributed to fuel rather
than to a wall-clock watchdog, and the engine must be alive afterwards; the legacy path must be
measured at **exactly zero** with the opt-in unset, so "off by default" is a number and not a claim;
and `sandboxed_surface_gaps` is a required field so the artifact cannot imply parity it does not
have.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — wasmtime host with a manifest-declared capability set checked at
                 import-resolution time, host imports mirroring plugin:AddSection / AddButton
   -> if still failing, MANDATORY approach change. Adding another host import is NOT an approach
      change; moving from import-resolution checks to a per-call capability token threaded through
      a single host dispatch import is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with ungranted.net_import_denied == 0
  - Budget      : 1.8M tokens or 30 builds consumed (150% of the XL envelope)
  - Item-specific: the only working handshake grants full host access (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G0.03/sandbox_conformance.json`, with the design and its honest limits at
`docs/architecture/PLUGIN_SANDBOX.md` and the fixtures under `scripts/plugin_sandbox/fixtures/`.

A reader finds: the WASM dependency count before (0) and after; the granted plugin's registration
counts and its one allowed network import; the ungranted plugin's one denial from the same guest
binary; the fuel-attributed termination time and proof the engine survived it; the exact opt-in
settings key with the legacy load count at zero unset and non-zero set; and the explicit list of
things the sandboxed surface cannot yet do. This is the W5 evidence, the closure for the
plugin-loader half of `G0.01`'s threat model, and the setting `G1.50` reads.

## 9. Definition of NOT done

- A WASM host is added and every plugin is granted every capability, so `net_import_denied` is 0.
  The handshake is the item; the host is the prerequisite.
- The denial is produced by building a second guest that never imported the function. Same binary,
  different manifest, or it proves nothing.
- The runaway plugin is stopped by a watchdog thread. That is a wall-clock artifact of the host
  machine, not a deterministic bound, and it will not reproduce in the container `G7.20` ships.
- The legacy `.lua`/`.rune` loader is deleted. `G1.50` depends on it, the extension surface it
  proves is real, and this item's answer to its exposure is a default-off opt-in.
- The opt-in defaults to on, or is undocumented, so "sandboxed by default" is true in the code and
  false on every existing installation.
- `docs/architecture/PLUGIN_SANDBOX.md` claims parity with the Luau surface. The WASM surface starts
  smaller; `sandboxed_surface_gaps` exists so the artifact says which parts.
- The artifact describes progress from a partial implementation. There was none —
  `wasm_dep_lines_before` is 0 and the document must read from that baseline.
````

---
## `docs/PROMPTS/items/G0.04_data-rights-and-third-party-use.md`

````markdown
---
id: G0.04
title: Data rights for .etask, the benchmark, episode bundles, and models trained on them
workload: W5
workload_secondary: [W2]
phase: G0
depends_on: [G7.12, G7.22, G1.48]
blocks: [G0.07, G0.10]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/legal/data_rights.json
escalation: >
  If `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` has a null `decision`, STALL
  immediately and do not author a matrix. Publishing terms that guess at an undecided licence is
  worse than publishing none, because a lab's counsel will rely on them and the correction is a
  breach conversation.
status: DRAFT
notes: >
  Tier S: a matrix plus a checker, zero compile. The item is short by design and stalls by design.
  Its wallclock envelope covers the agent's authoring pass only; the human licence decision it waits
  on is not metered here.
---

## 1. Objective

`docs/legal/DATA_RIGHTS.md` and its machine-readable twin state, for each of four asset classes and
each of six use-rights, what a third party may and may not do — with a named licence identifier per
class and a clause reference per cell. Every cell is answered. If the licence decision the terms
depend on has not been made, the item stalls and produces nothing.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Physics is **Avian**. Slint compiles to Rust. Units are meter-native.

**The licence as it stands.** `LICENSE` at the repository root is **PolyForm Shield License 1.0.0**,
Required Notice "Copyright (c) 2026 Eustress LLC (https://eustress.dev)", dual-licensed against
`LICENSE-COMMERCIAL.md`. It is **source-available**; it is not OSI-approved and it is never called
open source.

**The clause that stops a lab, quoted from `LICENSE`.** The Noncompete section: "Any purpose is a
permitted purpose, except for providing any product that competes with the software or any product
the licensor or any of its affiliates provides using the software." The Competition section then
defines competition maximally: "Goods and services compete even when they provide functionality
through different kinds of interfaces or for different technical platforms. Applications can compete
with services, libraries with plugins, frameworks with development tools, and so on... Goods and
services compete even when provided free of charge." The New Products section freezes an adopter at
"versions of the software available under these terms beginning when your product first competed".
"No Other Rights" forbids sublicensing and transfer.

**Why that is the whole problem this item exists for.** A frontier lab evaluating an RL environment
is, by any reading of the Competition clause, a company that also provides development tools and
agent platforms. Its counsel reads "compete even when provided free of charge" and stops. The
question they ask before running any container is not about the engine's licence — it is: *if we
run episodes, who owns the episodes, and may we train on them, and may we publish the numbers?*
Nothing in this repository answers that. `LICENSE` governs the software. It says nothing about
data a user generates by running it.

**The four asset classes.** These are exactly the four; do not add or merge.

1. **The `.etask` format and specification.** `G7.12` produced
   `docs/PROMPTS/artifacts/G7.12/etask_spec_v1.md` — a published, versioned specification of what an
   Eustress task is. The question is whether a third party may implement it, extend it, or publish a
   competing implementation.
2. **The EUSTRESS-PHYS-12 benchmark.** `G7.22` produced
   `docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md` — twelve tasks, a submission format, a scoring
   and aggregation rule, a repeated-run policy, and a leaderboard schema. The question is whether a
   third party may run it, cite it, redistribute the task files, or fork the suite.
3. **Episode bundles.** The recordings, observations, actions, and scores a run produces. `G7.17`
   established byte-exact replay, so a bundle is a reproducible artifact and not merely a log. The
   question is who owns a bundle produced on a third party's hardware, and whether Eustress LLC has
   any claim to it.
4. **Models trained on episodes, and weights derived from them.** The question every lab asks
   before the first episode: may the resulting weights be used commercially, published, or shipped
   in a product — and does the Noncompete clause reach through the data into the weights?

**The six use-rights columns.** Exactly these:
`internal_research_use`, `publish_results`, `redistribute_artifact`, `commercial_use_of_derivatives`,
`use_in_a_competing_product`, `attribution_and_notice_obligations`.

Four classes times six columns is **24 cells**. Every cell must be non-null and carry a
`clause_ref` — a section name in `LICENSE`, `LICENSE-COMMERCIAL.md`, or the newly named data licence
— so a reader can check the answer against the text rather than trusting the table.

**The gate you must respect.** `G1.48` is the licence decision-forcing memo. Its artifact is
`docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`, and `G1.48`'s own escalation reads:
"If the agent finds itself about to state which option has been chosen, stop and STALL. Changing the
licence is a human-only decision under `00_MASTER_PROTOCOL.md` section 6; this item produces the
options and a recommendation, never the decision." So `G1.48` deliberately leaves `decision` null
until a human fills it in.

That means **the normal state of this item is STALL**, and that is the correct outcome, not a
failure. `00_MASTER_PROTOCOL.md` §5.3 treats a stall as a respectable result with a one-screen
decision packet. Do not route around the null by picking a licence, by writing "TBD" into a cell, or
by writing terms "pending the licence decision". Any of those publishes into ambiguity, which is the
thing this item exists to prevent.

**Fabrication rules.** You are not a lawyer and must not present this as legal advice. Every cell
cites a clause; where the clause does not resolve the question, the cell's value is
`"unresolved_requires_counsel"` — which is a legitimate answer, is non-null, and must carry a
one-sentence statement of exactly what is unresolved. Do not invent a licence name. Do not assert
that a lab's use is permitted when the Competition clause plainly reaches it.

**What does not exist.** `docs/legal/` does not exist; you create it. There is no `SECURITY.md` and
no `CONTRIBUTING.md` in this repository.

## 3. Scope

### In scope — files this item may edit
- `docs/legal/DATA_RIGHTS.md` — create
- `docs/legal/data_rights.json` — create; the 4×6 matrix
- `docs/PROMPTS/harness/checkers/data_rights.py` — create
- `docs/PROMPTS/harness/checkers/fixtures/licence_memo_undecided.json` — create; the negative
  control, carrying `"decision": null` and `"synthetic": true`
- `docs/PROMPTS/artifacts/G0.04/` — the stall packet, if the item stalls

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `LICENSE`, `LICENSE-COMMERCIAL.md` — the licence is a human decision under
  `00_MASTER_PROTOCOL.md` §6
- `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` — a frozen input; writing a decision
  into it is impersonating the human decision this item waits on
- `docs/PROMPTS/artifacts/G7.12/`, `docs/PROMPTS/artifacts/G7.22/` — frozen inputs

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Filling a cell with "TBD", dropping a column because it is awkward, merging two asset classes,
  choosing a licence yourself, or writing the memo's `decision` field are all measurement changes.
  If the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The synthetic fixture exists **only** as the negative control that proves the stall trigger fires.
  It is never the pass. The pass requires a real `licence_decision_memo.json` with a non-null
  `decision` written by a human.
- Every cell carries a `clause_ref`. A cell whose answer cannot be traced to a clause is
  `"unresolved_requires_counsel"` with a one-sentence statement of what is unresolved.
- Do not describe Eustress as open source anywhere in either file.
- Address the Noncompete reach-through question explicitly for asset class 4. A lab will ask whether
  weights trained on Eustress episodes are encumbered. Silence reads as yes.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion
With a real `licence_decision_memo.json` whose `decision` is non-null, the matrix has **24** cells,
**0** null, **0** containing "TBD" or "pending", **4** non-empty `licence_id` values (one per asset
class), and **24** `clause_ref` values; the checker exits **0**. With the undecided fixture, the
checker exits **4** and names `decision` as the null field, proving the stall trigger fires.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/data_rights.py \
        --matrix docs/legal/data_rights.json \
        --licence-memo docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json
    echo "REAL_EXIT=$?"

    python docs/PROMPTS/harness/checkers/data_rights.py \
        --matrix docs/legal/data_rights.json \
        --licence-memo docs/PROMPTS/harness/checkers/fixtures/licence_memo_undecided.json
    echo "UNDECIDED_EXIT=$?"

    python docs/PROMPTS/harness/checkers/data_rights.py \
        --matrix docs/legal/data_rights.json --describe
    echo "DESCRIBE_EXIT=$?"

Expected output shape:

    asset_classes=4 columns=6 cells=24 null_cells=0 placeholder_cells=0
    licence_ids=4 clause_refs=24 unresolved_requires_counsel=3
    noncompete_reachthrough_addressed=true
    REAL_EXIT=0
    licence memo decision=null -> STALL
      missing field: decision
    UNDECIDED_EXIT=4
    DESCRIBE_EXIT=0

Pass condition:

    REAL_EXIT == 0  AND  UNDECIDED_EXIT == 4  AND  DESCRIBE_EXIT == 0
    AND cells == 24  AND  null_cells == 0  AND  placeholder_cells == 0
    AND licence_ids == 4  AND  clause_refs == 24
    AND noncompete_reachthrough_addressed == true

`UNDECIDED_EXIT == 4` on its own is **not** a pass. It proves the gate works; the item passes only
when `REAL_EXIT == 0` against a memo a human decided.

## 6. Critic gate

`critic_gate` is `[]`. No rubric dimension reads a rights matrix. The compensating tightness: a
fixed 4×6 shape the checker re-imposes so the matrix cannot be trimmed; a per-cell `clause_ref` so
every answer is traceable to text; a placeholder scan that rejects "TBD" and "pending"; a required
`noncompete_reachthrough_addressed` flag, because that is the one question a lab's counsel asks and
the one most easily omitted; and a two-sided checker in which the undecided-licence path must exit 4
rather than being allowed to pass.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — matrix authored per asset class, columns filled from the decided
                 licence and LICENSE clause text
   -> if still failing, MANDATORY approach change. Rewording a cell is NOT an approach change;
      inverting so the six columns are answered first as general policy and then specialised per
      asset class is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with null_cells unchanged and > 0
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: licence_decision_memo.json has decision == null (see front matter) — STALL
                   immediately, on iteration 1, before authoring the matrix
```

The stall packet must fit one screen and request exactly one of: LOWER — publish a narrower
episodes-only grant now and defer the other three classes, with the stated consequence; FUND
outside counsel to resolve the named `unresolved_requires_counsel` questions; DEFER behind `G1.48`
until the human decides the licence; or KILL, with a statement that the training-substrate wedge
ships without terms and what that costs. Recommend one.

## 8. Artifact

`docs/legal/data_rights.json`, with the human-readable terms at `docs/legal/DATA_RIGHTS.md`.

A reader finds: the four asset classes, each with a named licence identifier; the 4×6 grid of
permitted and forbidden uses with a clause reference per cell; the explicit answer on whether
Noncompete reaches through episode data into trained weights; the questions honestly marked
`unresolved_requires_counsel` with what is unresolved in each; and the licence-memo decision hash
the matrix was derived from. This is the W5 evidence and the document `G0.07` and `G0.10` hand to an
external party before they run anything.

If the item stalls, the artifact is instead the one-screen packet at
`docs/PROMPTS/artifacts/G0.04/stall_packet.md`, and `docs/legal/` is not created.

## 9. Definition of NOT done

- Terms are published while `licence_decision_memo.json` has a null `decision`. That is the single
  failure mode this item exists to prevent, and it is worse than shipping nothing.
- A cell says "TBD", "pending the licence decision", or "see LICENSE". The checker rejects the first
  two; the third is a cell that has not been answered.
- Asset class 4 is omitted or answered only for episode data. Whether Noncompete reaches trained
  weights is the question that decides whether a lab starts, and an unanswered question reads as
  yes.
- The matrix is validated only against the synthetic undecided fixture. That proves the gate fires;
  it proves nothing about the terms.
- The agent picks a licence, or writes a `decision` value into `G1.48`'s memo. Changing the licence
  is a human-only decision under `00_MASTER_PROTOCOL.md` §6.
- Eustress is described as open source anywhere in either file.
- Every cell reads `unresolved_requires_counsel`. That is a null in a costume; the flag exists for
  the genuinely hard questions, not as a way to avoid answering the easy ones.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.
````

---

## `docs/PROMPTS/items/G0.05_security-questionnaire-baseline.md`

````markdown
---
id: G0.05
title: Enterprise security-questionnaire baseline, every yes citing an existing path
workload: W3
workload_secondary: [W2]
phase: G0
depends_on: [G0.01, G0.02]
blocks: [G1.46]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G0.05/security_questionnaire.json
escalation: >
  If more than 30 of the 40 questions must be answered `no`, STALL rather than softening a question
  or reclassifying a `no` as `partial`. That result means the substrate cannot be sold into an
  enterprise security review at all this year, and the correct response is a human decision about
  which buyer to pursue, not a friendlier questionnaire.
status: DRAFT
notes: >
  Tier M rather than S because two answers require actually running the workspace dependency
  scanners rather than asserting their state, and `cargo deny` needs a resolved lockfile.
---

## 1. Objective

Forty questions an enterprise security review actually asks are each answered `yes`, `partial`, or
`no`; every `yes` and `partial` cites a repository path that **exists**; every `no` carries a
remediation with a cost in builds and days; and the count of `no` answers is recorded as the
baseline number every later trust item is measured against.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine. The
licence is PolyForm Shield 1.0.0 (`LICENSE`), dual-licensed against `LICENSE-COMMERCIAL.md`; say
**source-available**, never open source. Physics is **Avian**. Slint compiles to Rust. Units are
meter-native.

**Why this is separate from the procurement matrix.** `G1.46` in pack B2 builds a 24-question
procurement-readiness matrix for a deep-tech buyer covering legal, commercial, and support posture.
Its artifact is `docs/PROMPTS/artifacts/B2/G1.46/procurement_readiness.json`. This item is the
security half that matrix's security questions must cite. Do not duplicate `G1.46`'s commercial or
legal questions; answer the security ones properly and let `G1.46` reference this file.

**The evidence rule that makes this instrument non-gameable.** An answer of `yes` or `partial` must
cite a path in this repository, and the checker verifies the path exists. A `yes` citing a document
that has not been written is the exact failure mode of every self-assessed security questionnaire,
and it is the one thing a real reviewer catches. A `no` is cheap and honest; a `yes` costs a file.

**Inputs you must read.** `docs/security/threat_model.json` from `G0.01` — its entries whose
`control` is `null` are `no` answers here, and its `refusals` array is the evidence for the
tool-isolation questions. `docs/PROMPTS/artifacts/G0.02/capability_gate_conformance.json` — the
per-capability counts and the default-deny result are the evidence for the access-control questions.

**What is verifiably absent today, so you do not have to rediscover it.** MEASURED at HEAD
`71ccf6fe`, 2026-08-07: there is no `SECURITY.md` at the repository root and no
`docs/SECURITY.md`; there is no `CONTRIBUTING.md`; `.github/` contains only `FUNDING.yml` and
`workflows/`, so there is no `CODEOWNERS`; `infrastructure/forge/` contains
`README.md nomad scripts terraform` with no `consul/`; `grep -rn "wasmtime\|wasmer\|wasi"
--include=Cargo.toml eustress/` returns 0 lines.

**What does exist and must be credited.** `.github/workflows/ci.yml` has a `security` job
(`ci.yml:20`) that installs `cargo-deny` and runs
`cargo deny --config ../deny.toml check advisories bans sources` (`ci.yml:64`) from the `eustress/`
workspace directory, generating a fresh lockfile first (`ci.yml:49`) because `eustress/Cargo.lock`
is not committed. `deny.toml` lives at the repository root. That is a real supply-chain control and
answers at least one question `yes`.

**What CI does not do.** `.github/workflows/linux-engine.yml:48` runs
`cargo check --package eustress-engine` — one package, not the workspace, and `check` rather than a
test run. `eustress/crates/backend` is a workspace member (`eustress/Cargo.toml:12`), and
`eustress/crates/backend/src/marketplace.rs:194-215` calls `find_marketplace_item_by_id`,
`has_purchased`, `get_user_balance`, and `purchase_item` on `Database`, none of which are defined in
`eustress/crates/backend/src/db.rs`. The workspace almost certainly does not compile. Answer the
"does CI build and test the whole product" question accordingly, and cite those paths.

**The forty question keys, fixed by this prompt and re-imposed by the checker.** Eight domains, five
questions each. Use exactly these keys, no more, no fewer:

- *Access control:* `ac01_agent_tool_authz`, `ac02_default_deny`, `ac03_privilege_separation`,
  `ac04_admin_surface_authn`, `ac05_credential_storage`
- *Code and supply chain:* `sc01_dependency_advisories`, `sc02_dependency_pinning`,
  `sc03_build_reproducibility`, `sc04_binary_signing`, `sc05_sbom`
- *Third-party code execution:* `ex01_plugin_isolation`, `ex02_plugin_provenance`,
  `ex03_script_capability_scope`, `ex04_container_privileges`, `ex05_untrusted_content_handling`
- *Data protection:* `dp01_data_classification`, `dp02_encryption_at_rest`,
  `dp03_encryption_in_transit`, `dp04_data_retention`, `dp05_telemetry_minimisation`
- *Identity:* `id01_authn_mechanism`, `id02_session_management`, `id03_authz_model`,
  `id04_account_recovery`, `id05_kyc_jurisdiction_scope`
- *Operations:* `op01_logging`, `op02_monitoring_alerting`, `op03_backup_restore`,
  `op04_incident_response`, `op05_change_management`
- *Assurance:* `as01_test_coverage_in_ci`, `as02_determinism_verification`,
  `as03_third_party_pentest`, `as04_vulnerability_disclosure`, `as05_certification_posture`
- *Legal and contractual:* `lg01_licence_clarity`, `lg02_data_rights`, `lg03_subprocessors`,
  `lg04_liability_and_warranty`, `lg05_export_and_jurisdiction`

**Two answers you must not get wrong.** `id05_kyc_jurisdiction_scope`: the Cloudflare Worker
`JURISDICTIONS` dictionary is **not** empty — `infrastructure/cloudflare/api/src/index.js:39-98`
defines **46** country entries, consumed by `minimumAgeFor()` at `:102` and `handleJurisdiction()`
at `:1177`. The "72 countries" marketing figure is wrong; the correct figure is 46.
`as02_determinism_verification`: `eustress/crates/common/tests/determinism.rs` exists but is gated
behind a non-default `physics` feature, so it never runs in CI. That is a `partial` at best, and the
gating is the remediation.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile. Most of
this item is documentation; the build slots exist so `cargo deny` and any workspace build attempt
are *run* rather than asserted. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `docs/security/SECURITY_QUESTIONNAIRE.md` — create; the human-readable answers
- `docs/security/security_questionnaire.json` — create; the machine-readable twin
- `docs/PROMPTS/harness/checkers/security_questionnaire.py` — create
- `docs/PROMPTS/artifacts/G0.05/` — the artifact directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass. If an answer would
  improve by changing CI, that is a `no` with a remediation, not an edit.
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/security/threat_model.json`, `docs/PROMPTS/artifacts/G0.02/` — frozen inputs
- `docs/PROMPTS/artifacts/B2/G1.46/` — `G1.46` consumes this file, not the other way round
- Any file under `eustress/crates/` — this item measures posture; it does not improve it
- `deny.toml` — changing the advisory policy to produce a cleaner scan is a measurement change

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Reclassifying a `no` as `partial`, citing a path you intend to create, softening a question,
  dropping a domain, or relaxing `deny.toml` are all measurement changes. If the measurement is
  genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Every `yes` and every `partial` cites at least one repository path, and the checker verifies each
  path exists. Citing a path that does not exist fails the item outright — it is not a typo, it is
  the failure mode.
- Every `no` carries `remediation` (one sentence), `cost_builds` (an integer), and `cost_days` (an
  integer). A `no` with no remediation is an unanswered question.
- `cargo deny` must actually be run, and its exit code recorded. Do not assert the CI job's outcome
  from reading the workflow file.
- Do not improve the posture in this item. A questionnaire that measures a repository the same agent
  just changed measures nothing. Improvements are `G0.03`, `G0.11`, `G0.12`, and later items.
- Answer as a reviewer would read it, not as a founder would like it read. The `no` count is the
  deliverable.

## 5. Exit criterion

### Criterion
Exactly **40** question keys are present and match the fixed list; **0** answers lack a value; **0**
`yes` or `partial` answers cite a path that does not exist; **0** `no` answers lack a remediation
with integer build and day costs; and the `no` count is recorded. `cargo deny` was executed and its
exit code recorded. A negative-control run against a fixture whose `yes` cites a nonexistent path
exits **7**.

### Measurement

Command:

    cd eustress && cargo generate-lockfile && cargo deny --config ../deny.toml check advisories bans sources; echo "DENY_EXIT=$?"; cd ..

    python docs/PROMPTS/harness/checkers/security_questionnaire.py \
        --answers docs/security/security_questionnaire.json \
        --repo-root . \
        --threat-model docs/security/threat_model.json \
        --capability-gate docs/PROMPTS/artifacts/G0.02/capability_gate_conformance.json \
        --out docs/PROMPTS/artifacts/G0.05/security_questionnaire.json
    echo "QUESTIONNAIRE_EXIT=$?"

    python docs/PROMPTS/harness/checkers/security_questionnaire.py \
        --answers docs/PROMPTS/harness/checkers/fixtures/questionnaire_bad_path.json \
        --repo-root .
    echo "BADPATH_EXIT=$?"

Expected output shape:

    questions=40 unanswered=0
    yes=9 partial=11 no=20
    cited_paths=27 cited_paths_missing=0
    no_without_remediation=0
    cargo_deny_exit=0
    baseline_no_count=20
    QUESTIONNAIRE_EXIT=0
    ac01_agent_tool_authz: yes  -> docs/PROMPTS/artifacts/G0.02/capability_gate_conformance.json
    ex01_plugin_isolation: no   -> remediation "ship the WASM host from G0.03" builds=20 days=5
    FAIL fixture: as03_third_party_pentest = yes cites docs/security/pentest_2026.md (missing)
    BADPATH_EXIT=7

Pass condition:

    QUESTIONNAIRE_EXIT == 0  AND  BADPATH_EXIT == 7
    AND questions == 40  AND  unanswered == 0
    AND cited_paths_missing == 0
    AND no_without_remediation == 0
    AND cargo_deny_exit is recorded as an integer
    AND baseline_no_count is recorded as an integer

Read the emitted counts. The item does not pass because the questionnaire exists; it passes because
zero cited paths are missing and every `no` has a costed remediation.

## 6. Critic gate

`critic_gate` is `[]`. No rubric dimension reads a security questionnaire. The compensating
tightness: a fixed 40-key list re-imposed by the checker so the instrument cannot be trimmed to look
healthier; a path-existence check on every `yes` and `partial`, which is the single mechanism that
stops self-assessment from being fiction; a required costed remediation on every `no`; an actually
executed `cargo deny` rather than an asserted one; and a negative-control fixture that must exit 7,
proving the path check works.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — answer per domain from the threat model, the capability-gate
                 conformance file, and direct repository inspection
   -> if still failing, MANDATORY approach change. Re-wording an answer is NOT an approach change;
      inverting so every claimed control is located in the tree FIRST and the answers are derived
      from that inventory is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with cited_paths_missing > 0 and unchanged
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: more than 30 of 40 answers are `no` (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G0.05/security_questionnaire.json`, with the human-readable answers at
`docs/security/SECURITY_QUESTIONNAIRE.md`.

A reader finds: all forty questions with their answers; the yes/partial/no counts; every cited path,
with the count of missing ones at zero; every `no` with its one-sentence remediation and its integer
build and day cost; the executed `cargo deny` exit code; and the baseline `no` count that later
trust items are measured against. `G1.46`'s security questions cite this file rather than
re-answering it.

## 9. Definition of NOT done

- A `yes` cites a document the item intended to write. The path check exists precisely for that, and
  a missing cited path fails the item outright.
- The `no` count is low because ambiguous answers were recorded as `partial`. `partial` still
  requires an existing path; if there is no artifact, the answer is `no`.
- The agent improved the posture and then measured it. A questionnaire over a repository the same
  agent just changed measures the change, not the posture.
- `cargo deny` is reported from reading `.github/workflows/ci.yml` instead of being run. The workflow
  says what should happen; the exit code says what did.
- `id05_kyc_jurisdiction_scope` repeats the "72 countries" figure or the claim that `JURISDICTIONS`
  is empty. It holds 46 entries at `infrastructure/cloudflare/api/src/index.js:39-98`.
- `as02_determinism_verification` is answered `yes` because `eustress/crates/common/tests/
  determinism.rs` exists. It is gated behind a non-default `physics` feature and never runs.
- The questionnaire duplicates `G1.46`'s commercial and legal posture questions, so two artifacts
  answer the same thing and drift apart.
````

---
## `docs/PROMPTS/items/G0.06_licensing-misstatement-sweep.md`

````markdown
---
id: G0.06
title: Every licensing misstatement found and fixed, enforced by a grep verifier
workload: W3
workload_secondary: [W2]
phase: G0
depends_on: [G1.01]
blocks: [G0.07, G0.09, G0.12]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G0.06/licence_phrasing.json
escalation: >
  If correcting the phrasing on the live site would require changing what is actually offered —
  for example if a page promises rights PolyForm Shield does not grant and the honest fix is to
  withdraw an offer rather than reword a sentence — STALL. Changing an offer is a human decision,
  not a copy edit.
status: DRAFT
notes: >
  Tier M rather than S because the corrections land in the Leptos web crate and must be proven to
  still build. Six build slots covers `trunk build --release` across three approaches.
---

## 1. Objective

No customer-facing surface in this repository describes Eustress as open source or MIT-licensed. A
verifier greps the whole tree for the banned phrasings, exits non-zero on any hit not present in an
explicit, reasoned allowlist, and the allowlist is small enough that it cannot be used to make the
problem disappear.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine. The
licence is PolyForm Shield 1.0.0. The correct public phrase is **source-available**. Physics is
**Avian**. Slint compiles to Rust. Units are meter-native.

**Why this is urgent rather than tidy.** `LICENSE` is PolyForm Shield 1.0.0 with a Noncompete clause
that forbids providing any competing product, and a Competition clause stating that goods and
services compete "even when provided free of charge". Telling a prospective adopter the project is
open source or MIT-friendly is not a phrasing slip; it is telling them they have rights the licence
withholds. The people most likely to rely on it — a university partner, a lab evaluator, a company
whose open-source programme office screens licences — are exactly the readers this program is
trying to reach, and the correction lands after they have committed.

**The hits as they stand.** MEASURED at HEAD `71ccf6fe`, 2026-08-07, by
`grep -rniI "open source\|open-source\|MIT-friendly\|MIT license\|MIT licence" <path>`, excluding
`node_modules/`, `eustress/crates/web/dist/`, and `docs/PROMPTS/`:

| Path | Line | Text | Reading |
|---|---|---|---|
| `docs/marketing/UofA_Center_For_Innovation_Pilot.html` | 592 | `Free, open source. Windows, macOS, Linux.` | wrong |
| `docs/marketing/UofA_Center_For_Innovation_Pilot.html` | 665 | `Open source · MIT-friendly licensing · Git-diffable projects · No vendor lock-in` | wrong |
| `eustress/crates/web/src/pages/home.rs` | 426 | section comment `// OPEN SOURCE - build it with us` | wrong |
| `eustress/crates/web/src/pages/home.rs` | 434 | rendered `<span class="section-tag">"OPEN SOURCE"</span>` | wrong, and rendered on the live home page |
| `eustress/crates/web/src/pages/blog_indie_studios.rs` | 292 | `Eustress isn't just open source — it has a built-in economy` | wrong |
| `eustress/crates/web/src/pages/blog_indie_studios.rs` | 319 | `Why "open source" only protects you if the engine is actually …` | judge in context |
| `eustress/crates/web/src/pages/blog_indie_studios.rs` | 421 | `Free and open source. Optional paid support tiers …` | wrong |
| `eustress/crates/web/src/pages/blog_indie_studios.rs` | 520 | `Eustress is an open-source, forkable …` | wrong |
| `eustress/crates/web/src/pages/docs_earning.rs` | 314 | `The ledger is public. The math is open source.` | wrong |
| `eustress/crates/web/src/pages/marketplace.rs` | 27 | code comment `// Open-sourced spaces that can be redistributed` | judge in context |
| `eustress/crates/web/src/pages/license.rs` | 60 | heading `"Why not plain open source?"` | contrastive, legitimate |
| `eustress/crates/web/style/main.css` | 20592 | CSS section comment `Open Source / GitHub section` | judge in context |
| `docs/generative-world-layer.md` | 180 | `Differentiator: open source, agent-as-peer` | wrong |
| `docs/AUDIT/04_ASSET_PIPELINE.md` | 514 | `FBX licensing path (Autodesk SDK / open source)?` | about a third party, legitimate |
| `eustress/crates/engine/ui/slint/publish.slint` | 379 | checkbox `CheckBox { text: "Open Source"; … }` | a user-content publishing option — decide and record |

That is **at least 9** unambiguously wrong hits on customer-facing surfaces. The `publish.slint`
checkbox and its bound properties (`publish.slint:113`, `main.slint:183`, `main.slint:3672`,
`publish.slint:469`) label a choice a *user* makes about *their own* Space, not a claim about
Eustress. Decide whether that is honest, fix it or allowlist it with a reason, and record which.

**The derived PDF.** `docs/marketing/UofA_Center_For_Innovation_Pilot.pdf` is generated from the
HTML and its text is compressed, so `grep` finds nothing in it (MEASURED: 0 hits). Correcting only
the HTML leaves a wrong PDF in the tree. Regenerate it from the corrected HTML, record the command
you used and the new file's sha256 in the artifact, and note in the artifact that the verifier
cannot read PDFs — so the hash is the evidence, not a grep.

**What the site actually may say.** `eustress/crates/web/src/pages/license.rs` already states the
position correctly, including a legitimate contrastive section headed "Why not plain open source?"
that explains the choice. Contrastive uses of the phrase are not the defect; claims are. The
allowlist is how you encode that difference, one entry at a time, each with a reason.

**Build reality.** `eustress/crates/web` is a Leptos crate built with Trunk — it has `Trunk.toml`,
`index.html`, `src/`, `style/`, and `assets/`. Validate the corrections with
`trunk build --release` from `eustress/crates/web`, not `cargo check`. Engine builds take 10–15
minutes; only one cargo build at a time; never kill a build mid-compile.

**Deployment is not yours.** Correcting the source is this item. Publishing the corrected site is an
external publication and a production deployment — both human-only under
`00_MASTER_PROTOCOL.md` §6. Leave the corrected tree; do not deploy.

## 3. Scope

### In scope — files this item may edit
- `docs/marketing/UofA_Center_For_Innovation_Pilot.html`
- `docs/marketing/UofA_Center_For_Innovation_Pilot.pdf` — regenerate from the corrected HTML
- `eustress/crates/web/src/pages/` — the pages listed above
- `eustress/crates/web/style/main.css` — the section comment only
- `eustress/crates/engine/ui/slint/publish.slint`, `eustress/crates/engine/ui/slint/main.slint` —
  only if you decide the publishing checkbox must change
- `docs/generative-world-layer.md`
- `docs/PROMPTS/harness/checkers/licence_phrasing.py` — create
- `docs/PROMPTS/harness/checkers/licence_phrasing_allowlist.json` — create
- `docs/PROMPTS/artifacts/G0.06/` — the artifact directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `LICENSE`, `LICENSE-COMMERCIAL.md` — the licence is a human decision
- `eustress/crates/web/dist/` — build output; it regenerates
- Anything under `infrastructure/extensions/lsp/vscode/node_modules/` — vendored third-party code
  carrying its own MIT notices, which are accurate about those packages
- `docs/PROMPTS/` — this prompt library quotes the banned phrases in order to forbid them
- Anything that describes a **third party's** licence, such as
  `docs/AUDIT/04_ASSET_PIPELINE.md:514` on the Autodesk FBX SDK

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Allowlisting a hit instead of fixing it, narrowing the grep pattern, excluding a directory that
  contains a real hit, or deleting a page rather than correcting it are all measurement changes. If
  the measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- Every allowlist entry carries `path`, `line`, the exact permitted `text`, and a `reason` of at
  least 20 characters. An entry without a reason is a suppression.
- The allowlist may hold at most **6** entries. That ceiling is the whole defence against fixing the
  verifier instead of the copy.
- Record the pre-fix unallowlisted hit count. An item that ends at zero because everything was
  allowlisted and an item that ends at zero because the copy was corrected produce the same final
  number; the baseline is what distinguishes them.
- Rewrite the copy so it is *true and still appealing*. "Source-available under PolyForm Shield:
  free to read, fork, and build on; the one thing it forbids is reselling Eustress against us" is
  accurate and loses nothing. Do not simply delete the sections — a blank space where the licence
  story was is a worse page.
- Do not write "open source" in the replacement copy. Do not call Eustress a game engine. Do not
  write Rapier.
- The verifier must scan the **whole repository** with a small, explicit exclusion list, not an
  allowlist of directories to scan. A scanner you point at three known files finds three known
  files.

## 5. Exit criterion

### Criterion
The verifier scans the repository and reports **0** unallowlisted hits, with **≤ 6** allowlist
entries each carrying a reason of **≥ 20** characters, against a recorded pre-fix baseline of
**≥ 9** unallowlisted hits. The web crate still builds. The regenerated PDF's sha256 is recorded.

### Measurement

Command:

    git stash && python docs/PROMPTS/harness/checkers/licence_phrasing.py \
        --repo-root . --allowlist docs/PROMPTS/harness/checkers/licence_phrasing_allowlist.json \
        --report-only --out /tmp/licence_baseline.json ; echo "BASELINE_EXIT=$?" ; git stash pop

    python docs/PROMPTS/harness/checkers/licence_phrasing.py \
        --repo-root . \
        --allowlist docs/PROMPTS/harness/checkers/licence_phrasing_allowlist.json \
        --baseline /tmp/licence_baseline.json \
        --max-allowlist 6 --min-reason-chars 20 \
        --out docs/PROMPTS/artifacts/G0.06/licence_phrasing.json
    echo "PHRASING_EXIT=$?"

    cd eustress/crates/web && trunk build --release ; echo "WEB_BUILD_EXIT=$?" ; cd ../../..

Expected output shape:

    scanned_files=4127 excluded_dirs=5
    baseline_unallowlisted_hits=11
    BASELINE_EXIT=5
    unallowlisted_hits=0
    allowlist_entries=3
      license.rs:60  "Why not plain open source?"  reason="contrastive heading that explains the licence choice"
      04_ASSET_PIPELINE.md:514  "open source"  reason="describes third-party FBX SDK options, not Eustress"
      main.css:20592 "Open Source / GitHub section"  reason="internal stylesheet comment, never rendered to a reader"
    pdf_regenerated=true pdf_sha256=7c19…
    PHRASING_EXIT=0
    WEB_BUILD_EXIT=0

Pass condition:

    PHRASING_EXIT == 0  AND  WEB_BUILD_EXIT == 0
    AND unallowlisted_hits == 0
    AND baseline_unallowlisted_hits >= 9
    AND allowlist_entries <= 6
    AND every allowlist entry has len(reason) >= 20
    AND pdf_regenerated == true AND pdf_sha256 is a non-empty string

`BASELINE_EXIT` is expected to be non-zero — that is the pre-fix state and it is what proves the
verifier detects the defect it was written for. The pass is `PHRASING_EXIT == 0` **with** a baseline
of at least 9.

## 6. Critic gate

`critic_gate` is `[]`. The compensating tightness: a recorded pre-fix baseline of at least 9, so an
item that allowlisted its way to zero fails; a hard ceiling of 6 allowlist entries; a minimum reason
length on every entry; a whole-repository scan with an explicit exclusion list rather than a
targeted one; a successful `trunk build --release` so the corrections are not merely textual; and a
recorded sha256 for the regenerated PDF, which the grep cannot reach.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — correct each hit in place, allowlist only genuinely contrastive or
                 third-party uses
   -> if still failing, MANDATORY approach change. Adding an allowlist entry is NOT an approach
      change; rewriting the licence story once in `license.rs` and having every other page link to
      it rather than restate it is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with unallowlisted_hits unchanged and > 0
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: an honest correction would require withdrawing an offer (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G0.06/licence_phrasing.json`, with the verifier at
`docs/PROMPTS/harness/checkers/licence_phrasing.py` and the allowlist beside it.

A reader finds: how many files were scanned and which directories were excluded; the pre-fix
unallowlisted hit count with each hit's path and line; the post-fix count at zero; every allowlist
entry with its reason; the decision recorded for the `publish.slint` user-content checkbox; the
`trunk build --release` exit code; and the regenerated PDF's sha256 with the command that produced
it. This is the W3 evidence that the customer-facing licence statement now matches `LICENSE`, and it
is a precondition for `G0.07`, `G0.09`, and `G0.12`, each of which puts a document in front of
someone outside the company.

## 9. Definition of NOT done

- The count reaches zero because the hits were allowlisted. The 6-entry ceiling, the reason
  requirement, and the ≥ 9 baseline exist together for exactly this.
- The HTML is corrected and `docs/marketing/UofA_Center_For_Innovation_Pilot.pdf` still says "Open
  source · MIT-friendly licensing". The PDF is the file that gets emailed.
- The `home.rs` section is deleted rather than rewritten, so the home page now has a hole where the
  "build it with us" story was. The story is true under PolyForm Shield; only the label was wrong.
- The verifier scans a hand-listed set of files. A scanner that only looks where the agent already
  looked cannot catch the next occurrence, which is the entire purpose of leaving it behind.
- The replacement copy says "free and open" or "MIT-like" or "basically open source". The banned
  list is a floor, not the whole standard; the standard is that the reader's belief about their
  rights matches `LICENSE`.
- `trunk build --release` was not run, so the Leptos pages are edited but unproven and the site
  cannot ship.
- The item deployed the corrected site. Publication and production deployment are human-only under
  `00_MASTER_PROTOCOL.md` §6.
````

---

## `docs/PROMPTS/items/G0.07_external-engineer-episode-bundle.md`

````markdown
---
id: G0.07
title: An engineer outside the company produces an episode bundle the verifier accepts
workload: W1
workload_secondary: [W3, W5]
phase: G0
depends_on: [G7.20, G7.24, G0.04, G0.06]
blocks: [G0.10]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
human_executed: true
artifact: docs/PROMPTS/artifacts/G0.07/external_run_ledger.json
escalation: >
  If three separate external engineers each fail to reach a produced bundle and every failure is a
  different undocumented step, STALL and hand back to `G7.24`. The guide, not the recruiting, is
  then the binding constraint, and running a fourth person through a broken guide only burns a
  contact.
status: DRAFT
notes: >
  HUMAN-EXECUTED. The agent builds the verifier, the attestation form, the ledger schema, and the
  runbook; a human invites the engineer and records the outcome. The wallclock envelope covers the
  agent's authoring pass only; the human's calendar time is not metered here.
---

## 1. Objective

At least one engineer with no affiliation to Eustress LLC has, from the published guide alone, run
the container and produced an episode bundle that the bundle verifier accepts — and the run is
recorded in a ledger holding the engineer's attestation, the bundle hash, the guide commit they
followed, and every step where they got stuck.

## 2. Context you need (self-contained)

**HUMAN-EXECUTED.** You build the instrument; a human runs it. You may not contact anyone, invite
anyone, or send anything — `00_MASTER_PROTOCOL.md` §6 makes external contact a human decision. Your
deliverables are the verifier, the attestation form, the ledger schema, the empty ledger, and a
runbook a human can follow without asking you a question. The item does not pass until the ledger
holds at least one real row.

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0 — never open source. Physics is **Avian**. Slint
compiles to Rust. Units are meter-native.

**Why this item exists.** The training-substrate wedge in pack T4 terminates at self-publication.
`G7.22` ships a benchmark, `G7.23` measures Eustress's own agent on it, and `G7.24` writes an
integration guide. No item's exit criterion requires a party outside this company to run a single
episode. A benchmark with one measured entrant and zero external runs is a demo, and the first
external run is the cheapest, earliest fact that changes that.

**What you are handing them.** `G7.20` produced a container that runs episodes with no display
server, no GPU requirement, and no host setup beyond a mounted output directory; its conformance
record is `docs/PROMPTS/artifacts/G7.20/container_conformance.json`. `G7.24` produced
`docs/PROMPTS/artifacts/G7.24/LAB_INTEGRATION.md`, whose own exit criterion was a clean-machine
walkthrough with no undocumented steps. `G0.04` produced `docs/legal/DATA_RIGHTS.md`, which is what
their employer's counsel reads before they run anything. `G0.06` corrected the customer-facing
licence phrasing, which is why it blocks this item: an external engineer reading "open source · MIT-
friendly licensing" and then `LICENSE` learns something about this company that no episode bundle
offsets.

**What "outside the company" means, mechanically.** The engineer is not an employee, contractor,
founder, or family member of Eustress LLC; holds no equity; and is not being paid for the run. The
ledger records `affiliation_none: true` as an attested statement in their own words plus their
employer or affiliation, and the runbook instructs the human to decline any run that fails this. One
qualifying run beats five friendly ones, and a friendly run recorded as external is fabricated
evidence.

**What the run must produce.** An episode bundle in the format `G7.17` established for recording and
byte-exact replay. Your verifier reads the bundle and checks: it replays; its declared task id is one
of the twelve in `G7.22`'s suite; its dropped-tick accounting from `G7.15` marks it `valid` rather
than `invalid`; and its recorded commit matches a commit that exists. Whether the episode *succeeded*
at its task is irrelevant here — a failed episode that produces a valid bundle passes this item. The
claim being tested is "an outsider can operate the substrate", not "an outsider's agent is good".

**Privacy.** Record no more personal data than the ledger schema requires: a name or handle the
engineer consents to being recorded, their affiliation, and the attestation text. No email addresses,
no employer contact details, no anything else. Attestations are stored in the repository, so treat
the ledger as public.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile. Most of
this item is the verifier and the schema; the build slots exist so you can validate your verifier
against a locally produced bundle before a stranger's time is spent on it. Validate with
`cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — the bundle verifier binary
- `docs/PROMPTS/harness/external/EXTERNAL_RUN_RUNBOOK.md` — create; what the human does
- `docs/PROMPTS/harness/external/attestation_form.md` — create; what the engineer signs
- `docs/PROMPTS/harness/external/external_run_schema.json` — create
- `docs/PROMPTS/artifacts/G0.07/external_run_ledger.json` — create; empty at authoring time
- `docs/PROMPTS/artifacts/G0.07/` — the artifact directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G7.24/LAB_INTEGRATION.md` — if the guide is wrong, that is a hand-back to
  `G7.24`, not a licence to patch it here
- `docs/PROMPTS/artifacts/G7.20/`, `docs/legal/` — frozen inputs
- Anything that would send a message, publish a page, or contact a person
- The layout of `eustress/crates/agent-eval/` — the directory is owned by `G7.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Recording a run by anyone affiliated with the company, running the container yourself and logging
  it as external, relaxing the bundle verifier so a malformed bundle passes, or omitting the
  stuck-points list are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- **You may not contact anyone.** No email drafts, no direct messages, no named targets. The runbook
  tells a human how to ask; the human asks.
- The verifier must reject a bundle the engine did not produce. Include a negative-control fixture —
  a hand-edited bundle whose replay diverges — and prove the verifier rejects it, before a stranger
  runs anything.
- Record every step where the engineer got stuck, verbatim, even when they recovered. Those lines
  are the most valuable output of the whole item and they are the thing an eager write-up omits.
- A failed episode that produces a valid bundle is a pass. Do not coach the engineer toward a
  successful score; that contaminates `G0.10`.
- Do not fabricate an attestation, a name, an affiliation, or a quote.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion
`external_run_ledger.json` holds **at least 1** row with `affiliation_none: true`, a non-empty
attestation, a bundle path, and the guide commit followed; the bundle verifier accepts that bundle
with exit **0** and reports `replays_byte_exact: true` and `dropped_tick_status: "valid"`; the row
records **≥ 0** stuck points as an explicit list (present, possibly empty); and the verifier rejects
the tampered negative-control fixture with exit **3**.

### Measurement

Command:

    cargo run --release --package eustress-agent-eval --bin eustress-bundle-verify -- \
        --bundle "$(python -c 'import json;print(json.load(open("docs/PROMPTS/artifacts/G0.07/external_run_ledger.json"))["runs"][0]["bundle_path"])')" \
        --require-replay --require-valid-ticks \
        --suite docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md
    echo "BUNDLE_EXIT=$?"

    cargo run --release --package eustress-agent-eval --bin eustress-bundle-verify -- \
        --bundle docs/PROMPTS/harness/external/fixtures/bundle_tampered \
        --require-replay --require-valid-ticks \
        --suite docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md
    echo "TAMPER_EXIT=$?"

    python docs/PROMPTS/harness/checkers/external_run.py \
        --ledger docs/PROMPTS/artifacts/G0.07/external_run_ledger.json \
        --schema docs/PROMPTS/harness/external/external_run_schema.json \
        --min-runs 1 --require-unaffiliated
    echo "LEDGER_EXIT=$?"

Expected output shape:

    bundle=… task_id=EP12-04 replays_byte_exact=true dropped_tick_status=valid
    commit_exists=true
    BUNDLE_EXIT=0
    bundle=…/bundle_tampered  replay diverged at step 118
    TAMPER_EXIT=3
    runs=1 unaffiliated=1 attestations_present=1
      run[0] handle="…" affiliation="…" guide_commit=… stuck_points=3
    LEDGER_EXIT=0

Pass condition:

    BUNDLE_EXIT == 0  AND  TAMPER_EXIT == 3  AND  LEDGER_EXIT == 0
    AND runs >= 1  AND  unaffiliated == runs
    AND replays_byte_exact == true  AND  dropped_tick_status == "valid"
    AND every run has a non-empty attestation and a stuck_points list

The pass is a real row in the ledger. `TAMPER_EXIT == 3` proves the verifier discriminates; it is
not itself the result.

## 6. Critic gate

`critic_gate` is `[]`. No rubric dimension can read a stranger's attestation. The compensating
tightness: the pass requires an artifact the executing agent could not have produced — a bundle from
a machine it never touched, attested by a person it may not contact; `affiliation_none` must hold on
every counted row; the bundle must replay byte-exactly and carry a `valid` dropped-tick status, so a
hand-assembled directory does not qualify; and the tampered negative control must be rejected before
any of it counts.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — verifier plus runbook plus attestation form; human invites one
                 engineer from an existing professional contact
   -> if still failing, MANDATORY approach change. Inviting a second person the same way is NOT an
      approach change; moving from a private invitation to a publicly posted open call sourced from
      the `G0.09` arrival channels is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with runs == 0
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: three engineers each blocked at a different undocumented step (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER to "an external engineer
reached a running container but not a bundle", with the stated consequence for `G0.10`; FUND a paid
external evaluation slot; DEFER behind a re-opened `G7.24`; or KILL the external-run requirement and
state what the benchmark's credibility loses.

## 8. Artifact

`docs/PROMPTS/artifacts/G0.07/external_run_ledger.json`, with the runbook, attestation form, and
schema under `docs/PROMPTS/harness/external/`.

A reader finds: one row per external run, each with the engineer's consented handle, their
affiliation, the attestation text in their own words, the commit of the guide they followed, the
bundle path and hash, the verifier's verdict, and the verbatim list of every point at which they got
stuck. This is the W1 evidence that someone outside this company operated the substrate, and it is
the precondition for `G0.10`.

## 9. Definition of NOT done

- The ledger's only row is a run by the founder, a contractor, or a friend doing a favour. Then the
  benchmark still has zero external runs and the ledger has made that harder to notice.
- The agent contacted someone. External contact is a human decision under
  `00_MASTER_PROTOCOL.md` §6, and an item that breached it is unusable regardless of its result.
- The bundle is present but does not replay, so the ledger records that a directory appeared rather
  than that an episode ran.
- The stuck-points list is empty on every row because it was never asked for. It is the most useful
  field in the file and the easiest to omit when the run went well enough.
- The engineer was coached through the blocked steps in real time. Then the guide is untested and
  `G7.24`'s exit criterion is retroactively false.
- The item passes on the tampered-fixture rejection alone. That is the negative control.
- Personal data beyond the schema is recorded. The ledger is committed to a repository; treat it as
  public.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.
````

---

## `docs/PROMPTS/items/G0.08_five-qualified-strangers.md`

````markdown
---
id: G0.08
title: Five qualified strangers from named channels complete the discovery instrument
workload: W2
workload_secondary: [W4]
phase: G0
depends_on: [G6.33, G0.09]
blocks: []
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
human_executed: true
artifact: docs/PROMPTS/artifacts/G0.08/discovery_ledger.json
escalation: >
  If twenty qualified arrivals produce fewer than five completed conversations, STALL rather than
  loosening the qualification or counting a partial call. A 25% conversion from arrival to
  conversation is a channel-quality result the human needs, and hiding it inside a relaxed
  definition destroys the only signal the item generates.
status: DRAFT
notes: >
  HUMAN-EXECUTED. The agent builds the ledger, the consent form, and the aggregation checker; a
  human runs the conversations. The wallclock envelope covers the agent's authoring pass only; the
  human's calendar time is not metered here.
---

## 1. Objective

Five or more people with no prior relationship to Eustress LLC, who arrived through a channel named
in the arrival ledger, have each completed the discovery instrument, and their call records — real,
not synthetic — pass the extractor with at least one fully quantified pain apiece. The transcripts
are archived with consent.

## 2. Context you need (self-contained)

**HUMAN-EXECUTED.** You build the instrument's ledger, the consent form, the archival layout, and
the aggregation checker. A human runs the calls. You may not contact anyone, draft outreach, or name
a target — `00_MASTER_PROTOCOL.md` §6 makes external contact a human decision. The item does not
pass until the ledger holds at least five real rows.

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Licence is PolyForm Shield 1.0.0 — **source-available**, never open source. Physics is **Avian**.
Slint compiles to Rust. Units are meter-native.

**Why this item exists.** Pack B1 opens at a qualification scorecard and pack B2 at a SKU sheet.
Both presuppose a pipeline. `G6.32` produced `docs/gtm/DESIGN_PARTNER_QUALIFICATION.md` and
`docs/gtm/qualification_rubric.json`; `G6.33` produced `docs/gtm/DISCOVERY_INSTRUMENT.md`,
`docs/gtm/call_record_schema.json`, and `docs/PROMPTS/harness/checkers/discovery_extract.py`. Both
are validated only against synthetic fixtures the same agent authored. An instrument that has never
met a stranger is a specification, and the founder's real constraint is not the instrument.

**What `G6.33`'s extractor requires of a call record.** A quantified pain needs all four of `value`,
`unit`, `source_quote` (verbatim from the record), and `counterfactual` ("what happens today
instead"). Three of four is a rejection, and `discovery_extract.py` names the specific missing field.
Records also answer the envelope question — whether the prospect's regime is inside
`docs/validation/vcell_envelope.json` — and the capability question covering any need outside the
`G6.31` demo path. Your ledger does not re-implement any of that; it aggregates over records the
existing extractor validates.

**What "stranger" means, mechanically.** No prior working relationship with Eustress LLC or its
founder; not a friend, family member, current or former colleague, investor, or advisor; and not
introduced by one. Each row records `prior_relationship: "none"` as an attested statement plus the
`arrival_channel` they came through, which must match a channel declared in
`docs/PROMPTS/artifacts/G0.09/arrival_ledger.json`. Five conversations with acquaintances is a
different, easier, and much less informative result.

**What "qualified" means, mechanically.** The person passes `qualify.py` from `G6.32` with exit 0 —
no hard disqualifier fired. A disqualified conversation is still worth archiving and still counts
toward the arrival-to-conversation ratio, but it does not count toward the five.

**Consent and privacy.** Every archived transcript needs recorded consent to archive, and the ledger
must be treatable as public. Store the organisation's sector rather than its name unless the person
consents to the name; store no personal contact details; and let a participant withdraw, which
removes their transcript and decrements the count. If a withdrawal drops the count below five, the
item is no longer passing — that is correct behaviour, not a bug.

**No fabrication, at all.** Do not write a plausible transcript. Do not attribute an invented quote
to a real organisation. Do not mark a synthetic record real. `G6.33`'s fixtures carry
`"synthetic": true`; this ledger counts only records with `"synthetic": false`, and the checker
enforces it.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/harness/external/discovery_consent_form.md` — create
- `docs/PROMPTS/harness/external/discovery_ledger_schema.json` — create
- `docs/PROMPTS/harness/checkers/discovery_ledger.py` — create; the aggregation checker
- `docs/PROMPTS/harness/external/DISCOVERY_RUNBOOK.md` — create; what the human does
- `docs/PROMPTS/artifacts/G0.08/discovery_ledger.json` — create; empty at authoring time
- `docs/PROMPTS/artifacts/G0.08/transcripts/` — the archive directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/gtm/DISCOVERY_INSTRUMENT.md`, `docs/gtm/call_record_schema.json`,
  `docs/PROMPTS/harness/checkers/discovery_extract.py` — frozen inputs from `G6.33`
- `docs/gtm/qualification_rubric.json`, `docs/PROMPTS/harness/checkers/qualify.py` — frozen inputs
  from `G6.32`
- `docs/PROMPTS/artifacts/G0.09/arrival_ledger.json` — a frozen input
- Anything that would send a message, publish a page, or contact a person

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Counting an acquaintance, counting a disqualified prospect toward the five, accepting a record
  with three of four pain sub-fields, marking a synthetic record real, or relaxing the
  `arrival_channel` match are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- **You may not contact anyone or draft outreach.** The runbook says how; the human does it.
- Every counted record must carry `"synthetic": false`, and the checker must reject the file if any
  counted record carries `"synthetic": true`.
- Archive every conversation, including disqualified ones and ones that produced no quantified pain.
  The ratio of arrivals to conversations to qualified conversations is the item's second output and
  is more actionable than the five.
- Record consent explicitly, and honour withdrawal by deletion. Do not keep a copy "for the record".
- Do not aggregate away the negatives. A summary that reports five qualified conversations and omits
  the eleven that went nowhere has thrown out the channel-quality signal.

## 5. Exit criterion

### Criterion
`discovery_ledger.json` holds **at least 5** counted rows, each with `synthetic: false`,
`prior_relationship: "none"`, an `arrival_channel` present in `G0.09`'s arrival ledger, a consented
archived transcript path that exists, `qualify.py` exit **0**, and `discovery_extract.py` exit **0**
with **at least 1** quantified pain carrying all four sub-fields. The ledger also records the total
conversations held and the arrival-to-conversation ratio. A negative-control run over `G6.33`'s
synthetic fixtures counts **0** rows and exits **2**.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/discovery_ledger.py \
        --ledger docs/PROMPTS/artifacts/G0.08/discovery_ledger.json \
        --schema docs/PROMPTS/harness/external/discovery_ledger_schema.json \
        --arrival-ledger docs/PROMPTS/artifacts/G0.09/arrival_ledger.json \
        --rubric docs/gtm/qualification_rubric.json \
        --call-schema docs/gtm/call_record_schema.json \
        --min-qualified 5 --require-real --require-consent
    echo "LEDGER_EXIT=$?"

    python docs/PROMPTS/harness/checkers/discovery_ledger.py \
        --ledger docs/PROMPTS/harness/checkers/fixtures/discovery_ledger_synthetic_only.json \
        --schema docs/PROMPTS/harness/external/discovery_ledger_schema.json \
        --arrival-ledger docs/PROMPTS/artifacts/G0.09/arrival_ledger.json \
        --rubric docs/gtm/qualification_rubric.json \
        --call-schema docs/gtm/call_record_schema.json \
        --min-qualified 5 --require-real --require-consent
    echo "SYNTHETIC_EXIT=$?"

Expected output shape:

    conversations_total=17 counted=6 qualified=6 disqualified=11
    synthetic_rows_counted=0 consent_missing=0 transcripts_missing=0
    arrival_channels_used=["…","…"] channels_unknown=0
    quantified_pains_min_per_row=1
    arrival_to_conversation_ratio=0.21
    LEDGER_EXIT=0
    counted=0  all rows synthetic=true
    SYNTHETIC_EXIT=2

Pass condition:

    LEDGER_EXIT == 0  AND  SYNTHETIC_EXIT == 2
    AND counted >= 5
    AND synthetic_rows_counted == 0
    AND consent_missing == 0  AND  transcripts_missing == 0
    AND channels_unknown == 0
    AND quantified_pains_min_per_row >= 1
    AND conversations_total and arrival_to_conversation_ratio are recorded

The pass is five real rows. `SYNTHETIC_EXIT == 2` proves the checker refuses the fixtures pack B1
already validated against; it is not itself the result.

## 6. Critic gate

`critic_gate` is `[]`. No rubric dimension can read a discovery transcript. The compensating
tightness: the pass requires five artifacts the executing agent could not have produced; every
counted row must be non-synthetic, non-acquainted, consented, and traceable to a channel declared in
a separate frozen ledger; qualification and extraction are delegated to two frozen upstream checkers
rather than re-implemented here; the disqualified and unproductive conversations must also be
recorded so the ratio is honest; and the synthetic-fixture run must exit 2, which is the mechanical
statement that a fixture is never the pass.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — human runs conversations from arrivals through the highest-volume
                 channel in the G0.09 ledger
   -> if still failing, MANDATORY approach change. Running more conversations from the same channel
      is NOT an approach change; switching to a different declared channel with a different
      qualification profile is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with counted unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: 20 qualified arrivals yield fewer than 5 conversations (see front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the count to a stated number
with the consequence for `G6.34`'s pilot pre-registration; FUND a paid-recruiting channel with a
budget; DEFER behind a re-opened `G0.09` because arrival volume is the real constraint; or KILL and
state that the qualification and discovery instruments will ship unvalidated against a real person.

## 8. Artifact

`docs/PROMPTS/artifacts/G0.08/discovery_ledger.json`, with consented transcripts under
`docs/PROMPTS/artifacts/G0.08/transcripts/`.

A reader finds: every conversation held, counted or not; for each counted row the arrival channel,
the attested absence of a prior relationship, the consent record, the transcript path, the
qualification result, and the quantified pains the extractor found; and the aggregate funnel —
arrivals, conversations, qualified conversations — with the ratio between them. This is the W2
evidence that the discovery instrument survives contact with a stranger, and the funnel is what
tells the human which channel to spend the next month on.

## 9. Definition of NOT done

- The five are acquaintances, former colleagues, or warm introductions. Then the instrument has been
  validated against people predisposed to be helpful, which is the population it will never meet
  again.
- A synthetic fixture is counted. `G6.33`'s fixtures carry `"synthetic": true` and this checker
  refuses them for exactly that reason.
- The ledger reports only the five successes. The eleven conversations that went nowhere are the
  channel-quality signal, and dropping them turns a funnel into a testimonial.
- A record counts with three of four pain sub-fields. `G6.33`'s extractor rejects those and this
  item delegates to it rather than re-deciding.
- The agent drafted outreach or named a target company. Contacting a person is a human decision under
  `00_MASTER_PROTOCOL.md` §6.
- Transcripts are archived without recorded consent, or a withdrawal is honoured by hiding the row
  rather than deleting the transcript.
- `arrival_channel` is recorded as "inbound" or "referral" rather than a channel declared in
  `G0.09`'s ledger. An unattributable arrival cannot be repeated, which is the only thing the founder
  needs from it.
````

---
## `docs/PROMPTS/items/G0.09_channel-attributed-arrival.md`

````markdown
---
id: G0.09
title: Channel-attributed arrival, measured over a declared window
workload: W2
workload_secondary: [W6]
phase: G0
depends_on: [G6.30, G6.31, G0.06]
blocks: [G0.08]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
human_executed: true
artifact: docs/PROMPTS/artifacts/G0.09/arrival_ledger.json
escalation: >
  If after a full 14-day window every declared channel returns zero unique arrivals, STALL rather
  than extending the window or adding a fourth channel. Zero across three channels is a positioning
  result, not a volume problem, and the human must decide what to change before more time is spent
  publishing into silence.
status: DRAFT
notes: >
  HUMAN-EXECUTED for the publish and deploy steps. The agent adds the channel dimension to the
  existing analytics beacon, builds the ledger and its reader, and writes the runbook; a human
  publishes the artifacts and deploys the Worker. Envelope covers the agent's authoring pass; the
  14-day measurement window is not metered here.
---

## 1. Objective

Arrivals at the Eustress surface are attributable to the channel that produced them, at least three
channels are declared and published, and a measurement window of at least fourteen days records a
non-zero unique-arrival count for at least one of them — from the deployed counter, not from an
estimate.

## 2. Context you need (self-contained)

**HUMAN-EXECUTED for publication and deployment.** Publishing anything externally and deploying to
production are human-only decisions under `00_MASTER_PROTOCOL.md` §6. You build the counter, the
ledger, the reader, and the runbook; a human publishes and deploys and then records the window. The
item does not pass until the ledger holds a real, non-zero, channel-attributed count.

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Licence is PolyForm Shield 1.0.0 — **source-available**, never open source. Physics is **Avian**.
Slint compiles to Rust. Units are meter-native.

**Why this item exists.** Every business item in the library begins after someone has arrived.
`G6.32` qualifies a prospect; `G6.33` interviews one; `G1.42` prices for one. Nothing produces one.
For a bootstrapped solo founder, arrival — not proof — is the binding constraint, and an
unattributed arrival cannot be repeated, which makes it nearly worthless even when it happens.

**The counter that exists, and the dimension it lacks.** `infrastructure/cloudflare/api/src/index.js`
already carries a cookieless pageview beacon: `POST /api/analytics/hit` routed at `:636`, handled by
`handleAnalyticsHit()` at `:2837`. It derives a daily-rotating salted hash of IP plus user-agent
(`:2841-2845`), increments `pv:<day>` (`:2848`), and increments `uv:<day>` once per hashed visitor
per day via a `seen:<day>:<hash>` marker (`:2850-2855`), all in the `ANALYTICS` KV namespace, with a
graceful no-op when the binding is absent (`:2838`) and a catch that never throws into page load
(`:2857-2858`). `handleAdminStats()` at `:2561` reads those counters back.

**It has no channel dimension.** `pv:<day>` and `uv:<day>` are totals. There is no `src` parameter,
no per-channel key, and no way to answer "where did these people come from". That is the gap this
item closes: a `src` field on the beacon, `pvsrc:<day>:<channel>` and `uvsrc:<day>:<channel>` keys
alongside the existing ones, and an admin read that returns per-channel totals over a date range.
Keep the existing keys working — `handleAdminStats` depends on them.

**Privacy constraints you must preserve.** The beacon is cookieless by design and the daily-rotating
salted hash is deliberate: `index.js:2645-2647` records "no cookie, no stable cross-day identifier,
nothing personal stored". Adding a channel dimension must not add a stable identifier, must not
record a referrer URL verbatim, and must not accept an arbitrary string as `src`. Validate `src`
against the declared channel list — the same shape as `telemetryValidId()` at `index.js:2664` — and
drop anything else. A free-text `src` is a user-controlled key written into KV.

**Three channels, and who names them.** The runbook requires **at least three** declared channels,
each with (a) a name, (b) a mechanical counter, and (c) a named human action that produces arrivals
through it. Which three is a human decision — they depend on where the founder actually is. Candidate
shapes, offered as candidates and not as facts about this company: a public artifact published with
its own landing path (the `G7.22` benchmark and `G7.24` guide are the two most credible ones this
program produces); a social account posting under a stated cadence; a written piece placed on a
third-party publication; a named community or forum; a conference or meetup with a talk. The agent's
job is the counter and the ledger schema; the human declares the three.

**What you may honestly point people at.** `G6.30` produced
`docs/PROMPTS/artifacts/G6.30/capability_register.json` — what actually ships. `G6.31` produced
`docs/PROMPTS/artifacts/G6.31/demo_path_sweep.json` — the one demo path with no unwired buttons.
`G0.06` corrected the customer-facing licence statement, which is why it blocks this item: driving
arrivals to a page that says "open source · MIT-friendly licensing" against a PolyForm Shield
`LICENSE` converts a channel into a liability.

**No fabricated traffic.** Do not visit the surface to produce arrivals. The runbook must instruct
the human to exclude their own and any known-internal traffic, and the ledger records the exclusion
method. Self-generated arrivals are the easiest number in this pack to fake and the most useless.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile. Most of
this item is JavaScript and JSON; the build slots exist for `trunk build --release` in
`eustress/crates/web` if the landing path is added there. Validate with `cargo run`, not
`cargo check`.

## 3. Scope

### In scope — files this item may edit
- `infrastructure/cloudflare/api/src/index.js` — the `src` dimension on the beacon and the
  per-channel admin read; the existing `pv:`/`uv:` keys must keep working
- `eustress/crates/web/src/` — the landing path or paths the channels point at
- `docs/gtm/FIRST_CONTACT.md` — create; the channel declarations and the human runbook
- `docs/gtm/arrival_ledger_schema.json` — create
- `docs/PROMPTS/harness/checkers/arrival_ledger.py` — create
- `docs/PROMPTS/artifacts/G0.09/arrival_ledger.json` — create; empty at authoring time

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G6.30/`, `docs/PROMPTS/artifacts/G6.31/` — frozen inputs
- The KYC, identity, tickets, Stripe, and payout handlers in
  `infrastructure/cloudflare/api/src/index.js` — this item touches the analytics beacon and its
  admin read, nothing else
- Any deployment. `wrangler deploy` and `wrangler pages deploy` are human actions.
- Anything that would send a message or publish a page

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Counting the founder's own visits, counting bot traffic, shortening the window, counting pageviews
  as arrivals, or declaring a channel with no mechanical counter are all measurement changes. If the
  measurement is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- **Unique** arrivals, not pageviews. The existing `uv:` semantics — one per hashed visitor per day
  — are the definition; extend them per channel rather than inventing a second one.
- `src` must be validated against the declared channel list before it is used in a KV key. An
  unvalidated `src` is a user-controlled key namespace.
- Do not add a cookie, a stable cross-day identifier, or a verbatim referrer. The beacon's privacy
  properties are documented in the file and are not yours to trade for attribution.
- The existing `pv:<day>` and `uv:<day>` keys must keep incrementing. `handleAdminStats()` reads
  them, and breaking it to add a dimension is a regression, not a feature.
- Declare at least three channels, and for each state the human action that produces arrivals. A
  channel with a counter and no action is a measurement of nothing.
- Do not deploy. Do not publish. Write the runbook and stop.

## 5. Exit criterion

### Criterion
At least **3** channels are declared, each with a counter key and a named human action. Over a
measurement window of at least **14** days, the deployed counter reports at least **25** unique
channel-attributed arrivals (TARGET) across all channels, with at least **1** channel non-zero, and
**0** arrivals attributed to an undeclared `src`. Self- and internal traffic is excluded by a
recorded method. The existing site-wide `uv:<day>` counter is still incrementing.

### Measurement

Command:

    node infrastructure/cloudflare/api/test/analytics_src.test.js
    echo "BEACON_UNIT_EXIT=$?"

    python docs/PROMPTS/harness/checkers/arrival_ledger.py \
        --ledger docs/PROMPTS/artifacts/G0.09/arrival_ledger.json \
        --schema docs/gtm/arrival_ledger_schema.json \
        --min-channels 3 --min-window-days 14 --min-unique-arrivals 25 \
        --require-exclusion-method
    echo "ARRIVAL_EXIT=$?"

    python docs/PROMPTS/harness/checkers/arrival_ledger.py \
        --ledger docs/PROMPTS/harness/checkers/fixtures/arrival_ledger_undeclared_src.json \
        --schema docs/gtm/arrival_ledger_schema.json \
        --min-channels 3 --min-window-days 14 --min-unique-arrivals 25 \
        --require-exclusion-method
    echo "UNDECLARED_EXIT=$?"

Expected output shape:

    beacon: src validated against declared list, invalid src dropped, pv:/uv: still incremented
    BEACON_UNIT_EXIT=0
    channels_declared=3 window_days=21 window_start=… window_end=…
    unique_arrivals_total=41 by_channel={"…":31,"…":8,"…":2}
    channels_nonzero=3 undeclared_src_arrivals=0
    exclusion_method="founder IP range and known-internal hashes excluded at read time"
    sitewide_uv_still_incrementing=true
    ARRIVAL_EXIT=0
    undeclared src 'twitter_old' -> 4 arrivals not attributable
    UNDECLARED_EXIT=3

Pass condition:

    BEACON_UNIT_EXIT == 0  AND  ARRIVAL_EXIT == 0  AND  UNDECLARED_EXIT == 3
    AND channels_declared >= 3
    AND window_days >= 14
    AND unique_arrivals_total >= 25
    AND channels_nonzero >= 1
    AND undeclared_src_arrivals == 0
    AND exclusion_method is a non-empty string
    AND sitewide_uv_still_incrementing == true

25 is a **TARGET**, not a measured figure — no arrival baseline exists for this project. Read the
emitted `unique_arrivals_total` from the deployed counter; a ledger row typed in by hand is not a
measurement.

## 6. Critic gate

`critic_gate` is `[]`. No rubric dimension can read an arrival counter. The compensating tightness:
the number must come from a deployed counter rather than a spreadsheet; at least three channels must
be declared, each with a counter *and* a named human action, so a channel cannot be a hope; the
window has a 14-day floor so a good day cannot stand in for a channel; undeclared-`src` arrivals must
be exactly zero, which is what makes attribution mean something; a recorded exclusion method for
self- and internal traffic; a regression check that the existing site-wide counter still works; and a
negative-control fixture that must exit 3.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — instrument the existing beacon with a validated `src`, publish the
                 benchmark and guide behind per-channel landing paths
   -> if still failing, MANDATORY approach change. Publishing the same artifact again through a
      fourth channel is NOT an approach change; moving from published-artifact channels to a
      direct named-list channel with a counted landing path is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with unique_arrivals_total moving < 5%
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: zero unique arrivals across all three channels after a full window (front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER the arrival target to a stated
number with the consequence for `G0.08`'s five conversations; FUND a paid channel with a budget and
a stated cost-per-arrival ceiling; DEFER behind a positioning item, naming it; or KILL and state that
the business half of this program proceeds without a pipeline.

## 8. Artifact

`docs/PROMPTS/artifacts/G0.09/arrival_ledger.json`, with the channel declarations and the human
runbook at `docs/gtm/FIRST_CONTACT.md`.

A reader finds: the three or more declared channels, each with its counter key and the human action
that feeds it; the measurement window's start and end and its length in days; unique arrivals total
and per channel, read from the deployed counter; the zero count of arrivals from undeclared sources;
the exclusion method for self and internal traffic; and confirmation the pre-existing site-wide
counter still increments. This is the W2 evidence that arrival is now a measured quantity rather than
an assumption, and `G0.08` reads its channel list.

## 9. Definition of NOT done

- The counter is built and never deployed, so the ledger is a schema with no rows. The instrument is
  the cheap half.
- Arrivals are counted as pageviews. One person reloading is not fifteen arrivals, and the existing
  `uv:` semantics already define the honest unit.
- `src` is accepted as free text and written into a KV key. That is a user-controlled key namespace
  in a production Worker.
- The channel dimension is added and `pv:<day>` / `uv:<day>` stop incrementing, breaking
  `handleAdminStats()`.
- A channel is declared with a counter but no human action that produces arrivals through it. That is
  a measurement of nothing, and three of them is a dashboard.
- The founder's own visits are in the count. The exclusion method is a required field for this
  reason.
- A cookie or a stable cross-day identifier is added to make attribution easier. The beacon's privacy
  properties are documented in `index.js:2645-2647` and are not a cost centre.
- The agent deployed the Worker or published the landing pages. Both are human-only under
  `00_MASTER_PROTOCOL.md` §6.
````

---

## `docs/PROMPTS/items/G0.10_external-benchmark-submission.md`

````markdown
---
id: G0.10
title: One submission from outside the company scored on EUSTRESS-PHYS-12
workload: W1
workload_secondary: [W5, W3]
phase: G0
depends_on: [G7.22, G7.23, G0.07]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
human_executed: true
artifact: docs/PROMPTS/artifacts/G0.10/external_submission.json
escalation: >
  If an external submitter's score cannot be reproduced by re-running their archived episode
  recordings, STALL and do not publish the entry. An unreproducible leaderboard row is worse than an
  empty leaderboard, because the first external reader who checks it learns that the benchmark's own
  reproducibility claim is untested.
status: DRAFT
notes: >
  HUMAN-EXECUTED for the invitation and the publication. The agent builds the intake pipeline, the
  reproduction check, and the leaderboard record; a human invites and publishes. Envelope covers the
  agent's authoring pass only.
---

## 1. Objective

At least one EUSTRESS-PHYS-12 submission produced by an organisation other than Eustress LLC has
been ingested, scored by the published harness, reproduced from the submitter's own archived episode
recordings, and recorded as a leaderboard entry alongside the internal baseline — so the benchmark
has more than one measured entrant.

## 2. Context you need (self-contained)

**HUMAN-EXECUTED for invitation and publication.** Contacting a party and publishing externally are
human-only decisions under `00_MASTER_PROTOCOL.md` §6. You build the submission intake, the
reproduction check, the leaderboard record, and the runbook. A human invites and publishes. The item
does not pass until a real external entry is in the record.

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine, and
this artifact is read externally, so the phrasing matters. Source-available under PolyForm Shield
1.0.0 — never "open source". Physics is **Avian**, never Rapier. Slint compiles to Rust. Units are
meter-native.

**Why this item exists.** `G7.22` published EUSTRESS-PHYS-12 with a submission format, a scoring and
aggregation rule, a repeated-run policy, and a leaderboard schema. `G7.23` measured Eustress's own
tool-using agent on it. `G7.24` wrote the integration guide. Every one of those is this company
talking to itself. A benchmark with one measured entrant is a demo of the benchmark, and the
difference between a demo and a benchmark is the second row.

**What you already have.** `docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md` — the specification,
submission format, and leaderboard schema. `docs/PROMPTS/artifacts/G7.23/baseline_agent_results.json`
— the internal baseline, per task and aggregate, across at least three independent trials each, with
replayable recordings. `docs/PROMPTS/artifacts/G7.19/exploit_report.json` — the adversarial suite and
the declared repeated-run policy, which is what stops a submitter running five hundred times and
reporting the best. `docs/PROMPTS/artifacts/G0.07/external_run_ledger.json` — at least one engineer
outside the company who already produced a bundle the verifier accepted; that person is the most
likely first submitter and the ledger records exactly where they got stuck.

**What "outside the company" means, mechanically.** The submitting organisation is not Eustress LLC
or an entity it controls; the submitter is not an employee, contractor, founder, or family member;
and no one at Eustress LLC ran, tuned, or debugged the submitted agent. The record carries
`submitter_affiliation_none: true` as an attested statement plus the organisation's name or, if they
decline, its sector. A submission the founder helped produce is an internal baseline wearing a
different name.

**The reproduction requirement is the whole point.** The submitter's reported score is a claim. Their
archived episode recordings, replayed through the published harness on this machine, are the
measurement. `G7.17` established byte-exact replay and `G7.14` established episode determinism under
seed and tick-rate variation; use them. Record the submitter's reported aggregate, the reproduced
aggregate, and the difference. `G7.22`'s own escalation sets 5 percentage points as the noise floor
above which a leaderboard cannot be published; hold to it here.

**A low external score is the asset.** If an outside agent scores near zero, that is a true and
publishable fact about a hard benchmark, and it is far more credible than a high number nobody can
reproduce. Do not coach the submitter, do not tune for them, and do not withhold a low entry.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile. The build
slots exist so the reproduction runs actually happen. Validate with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/agent-eval/` — the submission intake and reproduction-check binary
- `docs/PROMPTS/harness/external/SUBMISSION_RUNBOOK.md` — create; what the human does
- `docs/PROMPTS/harness/external/submission_schema.json` — create
- `docs/PROMPTS/artifacts/G0.10/external_submission.json` — create; empty at authoring time
- `docs/PROMPTS/artifacts/G0.10/leaderboard.json` — create; internal baseline plus external entries

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/PROMPTS/artifacts/G7.22/`, `docs/PROMPTS/artifacts/G7.23/`,
  `docs/PROMPTS/artifacts/G7.19/` — frozen inputs; changing the scoring rule to accommodate a
  submission is the exact fraud this item must not commit
- `tasks/suite/` — the twelve tasks are frozen once published
- Anything that would send a message, publish a page, or contact a person
- The layout of `eustress/crates/agent-eval/` — the directory is owned by `G7.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Accepting a submission from an affiliated party, adjusting a scoring rule so a submission
  validates, publishing a reported score without reproducing it, or dropping a low entry are all
  measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- **You may not contact anyone.** The runbook says how to invite; a human invites.
- Every published entry must be reproduced from the submitter's own recordings on this machine, and
  the reported-versus-reproduced difference recorded. An entry that cannot be reproduced is not
  published; it is a stall.
- Enforce `G7.19`'s declared repeated-run policy on the submission. A submitter who ran many times
  and reported the best has violated the published rule, and the record says so rather than quietly
  accepting it.
- Do not tune, debug, or run the submitted agent. If the submitter is blocked, the block is recorded
  and handed to `G7.24`.
- Publish a low score exactly as measured.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion
`external_submission.json` holds **at least 1** submission with `submitter_affiliation_none: true`,
covering all **12** tasks, whose reported and reproduced aggregate scores differ by **≤ 5.0
percentage points**, whose recordings replay byte-exactly, and which complies with the declared
repeated-run policy. `leaderboard.json` holds **at least 2** entries — the internal `G7.23` baseline
and the external one. An affiliated-submitter negative-control fixture is rejected with exit **3**.

### Measurement

Command:

    cargo run --release --package eustress-agent-eval --bin eustress-submission-ingest -- \
        --submission docs/PROMPTS/artifacts/G0.10/incoming/ \
        --schema docs/PROMPTS/harness/external/submission_schema.json \
        --suite docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md \
        --repeated-run-policy docs/PROMPTS/artifacts/G7.19/exploit_report.json \
        --reproduce-from-recordings \
        --require-unaffiliated \
        --baseline docs/PROMPTS/artifacts/G7.23/baseline_agent_results.json \
        --out docs/PROMPTS/artifacts/G0.10/external_submission.json \
        --leaderboard docs/PROMPTS/artifacts/G0.10/leaderboard.json
    echo "SUBMISSION_EXIT=$?"

    cargo run --release --package eustress-agent-eval --bin eustress-submission-ingest -- \
        --submission docs/PROMPTS/harness/external/fixtures/submission_affiliated/ \
        --schema docs/PROMPTS/harness/external/submission_schema.json \
        --suite docs/PROMPTS/artifacts/G7.22/EUSTRESS-PHYS-12.md \
        --repeated-run-policy docs/PROMPTS/artifacts/G7.19/exploit_report.json \
        --reproduce-from-recordings --require-unaffiliated
    echo "AFFILIATED_EXIT=$?"

Expected output shape:

```json
{
  "schema_version": 1,
  "submissions": [
    { "submitter_org": "…", "submitter_affiliation_none": true,
      "tasks_covered": 12,
      "reported_aggregate": 0.083,
      "reproduced_aggregate": 0.083,
      "aggregate_delta_pp": 0.0,
      "recordings_replay_byte_exact": true,
      "repeated_run_policy_compliant": true,
      "runs_reported": 3, "runs_permitted": 3 }
  ],
  "leaderboard_entries": 2,
  "internal_baseline_aggregate": 0.117
}
```

Pass condition:

    SUBMISSION_EXIT == 0  AND  AFFILIATED_EXIT == 3
    AND len(submissions) >= 1
    AND every submission has submitter_affiliation_none == true
    AND tasks_covered == 12
    AND abs(aggregate_delta_pp) <= 5.0
    AND recordings_replay_byte_exact == true
    AND repeated_run_policy_compliant == true
    AND leaderboard_entries >= 2

Read the reproduced aggregate, not the reported one. A submission accepted on its own claim is not a
measurement, and `AFFILIATED_EXIT == 3` proves the affiliation check discriminates rather than being
the result.

## 6. Critic gate

`critic_gate` is `[]`. No rubric dimension can read a leaderboard. The compensating tightness: the
pass requires an artifact from an organisation the executing agent has no access to; the score must
be **reproduced locally from the submitter's recordings** rather than accepted, with a 5.0 pp ceiling
inherited from `G7.22`'s own noise floor; all twelve tasks must be covered so a partial run cannot
stand in; the repeated-run policy from the frozen exploit report is enforced; and an affiliated
submission must be rejected with exit 3.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — invite the engineer already recorded in the G0.07 ledger; intake,
                 reproduce, and record
   -> if still failing, MANDATORY approach change. Inviting a second individual is NOT an approach
      change; moving from individual invitation to an announced open call through the highest-volume
      G0.09 channel, with a submission deadline, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with len(submissions) == 0
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: a submitter's score cannot be reproduced from their recordings (front matter)
```

The stall packet must fit one screen and request exactly one of: LOWER to "an external party ran the
full suite without submitting a scored entry", with the consequence for the benchmark's credibility;
FUND a paid external evaluation; DEFER behind a re-opened `G0.09` because reach is the constraint; or
KILL the external-entrant requirement and state plainly that EUSTRESS-PHYS-12 ships with one measured
entrant.

## 8. Artifact

`docs/PROMPTS/artifacts/G0.10/external_submission.json`, with the two-entry leaderboard at
`docs/PROMPTS/artifacts/G0.10/leaderboard.json`.

A reader finds: the submitting organisation or its sector, its attested lack of affiliation, all
twelve task scores, the aggregate the submitter reported, the aggregate reproduced locally from their
own recordings, the difference between them, the byte-exact replay result, and the repeated-run
compliance check with runs reported against runs permitted. This is the W1 evidence that
EUSTRESS-PHYS-12 has more than one measured entrant.

## 9. Definition of NOT done

- The only entry is the internal `G7.23` baseline re-listed under another name. Two rows, one
  entrant.
- The reported score is published without reproducing it from the submitter's recordings. The first
  external reader who checks will find out, and the benchmark's reproducibility claim dies with it.
- A scoring rule, task, or tolerance was adjusted so the submission would validate. The suite is
  frozen once published; a submission that does not validate is a finding about the submission or
  about the guide.
- A low external score was withheld. A hard benchmark with an honest low entry is the asset; a
  benchmark with no entries because none looked good is marketing.
- The submitter was coached, tuned, or debugged by anyone at Eustress LLC. Then the entry measures
  this company again.
- The repeated-run policy was not enforced, so a submitter could run any number of times and report
  the best while the record calls it a single result.
- The agent contacted the submitter or published the leaderboard. Both are human-only under
  `00_MASTER_PROTOCOL.md` §6.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.
````

---

## `docs/PROMPTS/items/G0.11_egress-and-secrets-boundary.md`

````markdown
---
id: G0.11
title: Egress allowlist and secret scrubbing on the agent and script surfaces
workload: W3
workload_secondary: [W5]
phase: G0
depends_on: [G0.01, G0.02, G7.08]
blocks: []
tier: L
token_envelope: 500000
wallclock_envelope: 2d
max_builds: 12
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/G0.11/egress_conformance.json
escalation: >
  If the only way to scrub the environment for `run_bash` is to stop passing the parent environment
  entirely, and that breaks the documented `curl`-driven external-API flows the tool exists for,
  STALL. Choosing between an unusable escape hatch and a leaking one is a design decision about what
  the tool is for.
status: DRAFT
notes: >
  Tier L: enforcement spans the tool crate, the Luau service injection, and the child-process
  spawn path, and each end-to-end verification needs a built engine.
---

## 1. Objective

A canary secret present in the engine's environment never reaches a model through any tool response,
network egress from the agent surface and from scripts is denied by default and permitted only to an
explicitly allowlisted host set, and an attempt to reach a non-allowlisted host returns a structured
error rather than succeeding or hanging.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0 — never open source. Physics is **Avian**. Slint
compiles to Rust. Units are meter-native.

**The three egress paths, all verified in source.**

1. `run_bash` — `eustress/crates/tools/src/shell_tools.rs:65`. The whole command string goes to
   `bash -c`. Its module documentation at `:9-14` names the motivating use case: "orchestrating
   external HTTP APIs via `curl`". It captures combined stdout and stderr, truncated at
   `MAX_OUTPUT_BYTES = 16_384` (`:24`), and returns it to the model. The working directory is
   confined to the Universe root by `resolve_cwd` (`:37-47`), which rejects `..`. **The child
   process inherits the parent environment.** Any API key in the engine's environment is one
   `env` away from the model's context.
2. `http_request` — `eustress/crates/tools/src/simulation_tools.rs:454`. It is the single descriptor
   `eustress/crates/mcp-server/src/shared_registry.rs:226` marks with `openWorldHint`, on the stated
   basis that "everything here targets the local Universe or the local engine; `http_request` is the
   one tool that reaches the open internet." There is no host allowlist.
3. **Luau `HttpService`** — injected at `eustress/crates/common/src/luau/runtime.rs:1108`, with
   `GetAsync` at `:3014`, `PostAsync` at `:3024`, and `RequestAsync` at `:3037`, set on the shared
   globals. `script_plugin_host.rs:19-21` states the consequence in its own words: "this engine's
   Luau globals already give any script unrestricted `HttpService`/`DataStoreService` access."
   `build_plugin_environment` (`runtime.rs:520`) gives each plugin its own environment table whose
   metatable `__index` is `lua.globals()` (`:595-596`), so every plugin reaches all of it.

`embedvec_tools.rs:207` is the one place in the tool crate that marks a network call
`requires_approval: true` with the comment "network call — gate behind user approval" — and as
`G0.01` established, `requires_approval` is not enforced on the MCP path at all
(`eustress/crates/mcp-server/src/main.rs:463-521` never reads it).

**What `G0.02` gave you.** `eustress/crates/tools/capability_policy.json`, generated, with `net` as
one of six capabilities and `http_request` mapped to it, plus enforcement at both the MCP and Engine
Bridge chokepoints and a default-deny grant set. This item does not rebuild that. It adds the second
axis: *given* the `net` capability, **which hosts**, and *given* `process.exec`, **what does the
child see**.

**Why the canary test rather than a code review.** "Secrets are not exposed" is unfalsifiable by
reading. A named canary value placed in the environment, then grepped for across every response of a
full replay of the tool census, is falsifiable: either the string appears or it does not. Use
`G7.01`'s census (`docs/PROMPTS/artifacts/G7.01/tool_conformance.json`) as the invocation list so
the replay covers all 104 descriptors rather than the ones you thought of.

**What must keep working.** `run_bash`'s documented purpose is orchestrating external HTTP APIs via
`curl` — the module documentation names a Hugging Face Gradio flow explicitly and the tool's own
description embeds a three-step submit/poll/download recipe. An allowlist that makes that impossible
has broken the tool rather than secured it. The allowlist is a list; the fix is to make it
configurable and default-deny, not empty and immutable.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile. Validate
with `cargo run`, not `cargo check`.

## 3. Scope

### In scope — files this item may edit
- `eustress/crates/tools/src/shell_tools.rs` — environment scrubbing on the child spawn
- `eustress/crates/common/src/luau/runtime.rs` — the `HttpService` host check only; do not remove
  the service
- `eustress/crates/tools/egress_allowlist.json` — create; the default-deny host list
- `eustress/crates/agent-eval/` — the canary replay and egress-denial runner
- `docs/PROMPTS/artifacts/G0.11/` — the artifact directory

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `eustress/crates/tools/capability_policy.json` — generated by `G0.02`; a frozen input
- `docs/PROMPTS/artifacts/G7.01/tool_conformance.json`, `docs/security/threat_model.json` — frozen
  inputs
- `eustress/crates/engine/src/script_plugin_host.rs` — plugin isolation is `G0.03`
- `infrastructure/cloudflare/api/src/index.js` — server-side egress is a different boundary
- `eustress/crates/tools/src/simulation_tools.rs` — owned by `G7.08` (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). If this item cannot
  reach its exit criterion without editing this file, that is an escalation to L0 under
  FILE-OWNERSHIP, not a licence to edit it.
- The layout of `eustress/crates/agent-eval/` — the directory is owned by `G7.01`
  (`docs/PROMPTS/04_FILE_OWNERSHIP.md`). Adding or editing your own files inside it is
  permitted; moving, renaming, or deleting a file another item owns is an escalation to L0
  under FILE-OWNERSHIP.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Choosing a canary string that no tool would echo anyway, replaying a subset of the census,
  allowlisting the test host used by the denial probe, or truncating responses before the grep are
  all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- The canary must be a distinctive value in a plausibly named variable — for example
  `ANTHROPIC_API_KEY` with a recognisable sentinel body — and it must be present in the parent
  environment for the whole replay. Scrubbing a variable nobody would set proves nothing.
- Denial must be a **structured error**, not a hang and not a silent empty result. A blocked request
  that returns an empty 200-shaped body teaches the model that the host is down.
- Do not remove `HttpService` from the Luau globals. `docs/AUDIT` records it as a parity surface and
  scripts depend on it; the fix is the host check, not the amputation.
- Keep `run_bash`'s documented external-API workflow working with an allowlisted host. Prove it: one
  allowlisted `curl` must succeed in the same run in which a non-allowlisted one is denied.
- The allowlist must be configurable and default-deny, and its default contents recorded in the
  artifact.

## 5. Exit criterion

### Criterion
Replaying every one of the **104** census descriptors with the canary present in the environment
yields **0** responses containing the canary string. From all **3** egress paths, a request to a
non-allowlisted host is denied with a structured error — **3** denials, **0** successes, **0**
timeouts — while an allowlisted host succeeds on all 3 paths. `run_bash`'s documented `curl` flow
succeeds against an allowlisted host in the same run.

### Measurement

Command:

```
cargo build --release --package eustress-engine --bin eustress-headless

$env:EUSTRESS_WORKSPACE = "$PWD\.eustress-fixture"
$env:ANTHROPIC_API_KEY = "sk-canary-G0-11-DO-NOT-LEAK-3f9a2c"
pwsh -File scripts/agent_eval/make_suite_fixtures.ps1 -Out $env:EUSTRESS_WORKSPACE

cargo run --release --package eustress-agent-eval --bin eustress-egress-check -- `
  --headless-bin .\eustress\target\release\eustress-headless.exe `
  --workspace $env:EUSTRESS_WORKSPACE `
  --census docs/PROMPTS/artifacts/G7.01/tool_conformance.json `
  --canary-env ANTHROPIC_API_KEY `
  --allowlist eustress/crates/tools/egress_allowlist.json `
  --probe-path run_bash --probe-path http_request --probe-path luau_httpservice `
  --probe-allowlisted-host `
  --probe-denied-host `
  --out docs/PROMPTS/artifacts/G0.11/egress_conformance.json
echo "EGRESS_EXIT=$LASTEXITCODE"
```

Expected output shape:

```json
{
  "schema_version": 1,
  "commit": "…",
  "census_descriptors_replayed": 104,
  "responses_containing_canary": 0,
  "canary_env_var": "ANTHROPIC_API_KEY",
  "canary_present_in_parent_env": true,
  "allowlist_default_hosts": ["api.eustress.dev"],
  "egress_paths": [
    { "path": "run_bash",           "denied_host_result": "structured_error",
      "denied_error_code": "egress_denied", "allowlisted_host_result": "ok",
      "documented_curl_flow_ok": true },
    { "path": "http_request",       "denied_host_result": "structured_error",
      "denied_error_code": "egress_denied", "allowlisted_host_result": "ok" },
    { "path": "luau_httpservice",   "denied_host_result": "structured_error",
      "denied_error_code": "egress_denied", "allowlisted_host_result": "ok" }
  ],
  "denials": 3, "denied_host_successes": 0, "timeouts": 0
}
```

Pass condition:

```
EGRESS_EXIT == 0
AND census_descriptors_replayed == 104
AND canary_present_in_parent_env == true
AND responses_containing_canary == 0
AND denials == 3 AND denied_host_successes == 0 AND timeouts == 0
AND every egress_paths[i].denied_error_code is a non-empty string
AND every egress_paths[i].allowlisted_host_result == "ok"
AND the run_bash entry has documented_curl_flow_ok == true
```

Read the emitted `responses_containing_canary`. Zero with `canary_present_in_parent_env == false` is
not a pass — it means the canary was never there.

## 6. Critic gate

`critic_gate` is `[]`. No rubric dimension can read an egress denial. The compensating tightness:
the canary must be proven present before its absence counts; the replay covers the full 104-descriptor
census from a frozen upstream artifact rather than a hand-chosen subset; all three egress paths must
deny **and** all three must still permit an allowlisted host, so the item cannot pass by turning the
network off; denial must be a structured error with a code, never a hang or an empty success; and
`run_bash`'s documented external-API workflow must still work.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — explicit environment allowlist on the child spawn, host check at each
                 of the three egress call sites against a shared allowlist loader
   -> if still failing, MANDATORY approach change. Adding another variable name to the scrub list is
      NOT an approach change; moving from a deny-list of variable names to a construct-the-child-
      environment-from-nothing model with an explicit passthrough set is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with responses_containing_canary > 0 and unchanged
  - Budget      : 750k tokens or 18 builds consumed (150% of the L envelope)
  - Item-specific: scrubbing the environment breaks the documented curl flows (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G0.11/egress_conformance.json`, with the default-deny host list at
`eustress/crates/tools/egress_allowlist.json`.

A reader finds: how many descriptors were replayed with a canary secret present in the environment
and how many responses contained it (zero); the canary variable name and proof it was set; the
default allowlist contents; for each of the three egress paths, the structured error code returned
for a non-allowlisted host and the success returned for an allowlisted one; and confirmation that
`run_bash`'s documented external-API workflow still completes. This closes the egress and
credential-exposure entries in `G0.01`'s threat model and supplies the evidence for
`ac05_credential_storage` and `ex03_script_capability_scope` in `G0.05`'s questionnaire.

## 9. Definition of NOT done

- The canary never appears because it was never in the environment. `canary_present_in_parent_env`
  exists for exactly this, and a zero without it is a measurement of nothing.
- The replay covers the tools the agent thought of. The census exists so it covers all 104.
- Egress is closed by making every network call fail. The allowlisted-host success on all three
  paths, and the working `curl` flow, are what distinguish a boundary from an outage.
- A denied request hangs until timeout or returns an empty success. The model then treats the host as
  down and retries, which is worse than a clear refusal.
- `HttpService` is removed from the Luau globals. Scripts depend on it, the parity surface documents
  it, and the fix is a host check.
- Only the tool surface is gated, leaving Luau `HttpService` open — so a `.lua` plugin from
  `%LOCALAPPDATA%` still reaches any host on the internet, which is the widest of the three paths.
- The canary is scrubbed from `run_bash` output by filtering the response string rather than by not
  passing the variable to the child. A filter catches the exact string and misses the base64 of it.
````

---

## `docs/PROMPTS/items/G0.12_security-disclosure-path.md`

````markdown
---
id: G0.12
title: SECURITY.md, a disclosure path, and one acknowledged inbound report
workload: W3
workload_secondary: [W6]
phase: G0
depends_on: [G0.01, G0.06, G1.48]
blocks: []
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
human_executed: true
artifact: docs/PROMPTS/artifacts/G0.12/disclosure_ledger.json
escalation: >
  If the published contact address cannot be reached by an external sender — bounced, filtered, or
  never delivered — STALL rather than substituting an address the founder monitors informally. A
  disclosure path that works only for people who already know how to reach the founder is not a
  disclosure path.
status: DRAFT
notes: >
  HUMAN-EXECUTED for the publication and the acknowledgement. The agent writes SECURITY.md, the
  severity rubric, and the ledger; a human publishes and answers. Envelope covers the agent's
  authoring pass only; the response-time measurement is not metered here.
---

## 1. Objective

`SECURITY.md` at the repository root states what is in scope, how to report a vulnerability, what
response time a reporter can expect, and how severity is assigned — and the path is proven to work:
a person outside Eustress LLC sent a report to the published address and received an acknowledgement,
with both timestamps recorded.

## 2. Context you need (self-contained)

**HUMAN-EXECUTED for publication and acknowledgement.** Publishing externally and sending a message
are human-only decisions under `00_MASTER_PROTOCOL.md` §6. You write the document, the severity
rubric, the ledger schema, and the runbook. A human publishes and answers. The item does not pass
until the ledger holds a real round trip.

**What Eustress is.** An AI-native simulation substrate and world engine — never a game engine.
Source-available under PolyForm Shield 1.0.0 — never open source. Physics is **Avian**. Slint
compiles to Rust. Units are meter-native.

**What is absent.** MEASURED at HEAD `71ccf6fe`, 2026-08-07: `ls SECURITY.md docs/SECURITY.md`
returns no such file for either path. There is no published disclosure address, no stated response
time, no severity rubric, and no coordinated-disclosure policy anywhere in the repository. The
repository is public — `eustress/crates/web/src/pages/license.rs:74` links
`https://github.com/WeaveITMeta/EustressEngine/blob/main/LICENSE` from the live licence page — so
the surface is readable by anyone and reportable by no one.

**Why an acknowledged round trip and not just a file.** A `SECURITY.md` that names an address nobody
has ever sent to is the same artifact as no `SECURITY.md`, right up until the first person tries.
Every other security control in this pack is verified by execution; the disclosure path deserves the
same standard, and it is the cheapest one to verify.

**What the scope section must say.** Be specific, using what `G0.01` established.
`docs/security/threat_model.json` enumerates the agent tool surface and the plugin loader with their
existing controls and their honest nulls. In scope for a report: the MCP and Engine Bridge tool
surface; the script plugin loader; the Cloudflare Worker at
`infrastructure/cloudflare/api/src/index.js`; and the published container from `G7.20`. Out of scope,
and say why: findings that depend on an attacker already having write access to
`%LOCALAPPDATA%/Eustress/Plugins/`, because that path is documented as unsandboxed and its closure is
tracked as `G0.03` rather than treated as a report; and vulnerabilities in vendored third-party
dependencies, which belong upstream and are separately scanned by the `cargo-deny` job at
`.github/workflows/ci.yml:64`.

**Severity, tied to something real.** `G7.19` produced
`docs/PROMPTS/artifacts/G7.19/exploit_report.json` with named exploit classes —
`direct_value_write`, `goal_relocation`, `constraint_deletion`, `predicate_boundary`,
`task_file_tampering`, `scorer_tampering`, `compression_laundering`, `nondeterminism_farming`, and
others. Map your severity bands onto those classes plus the threat-model entries so a reporter can
see where their finding lands rather than waiting to be told.

**What a solo operator can honestly promise.** One person, bootstrapped, on Windows, with 10–15
minute serialized builds, no on-call and no SLA. Do not promise a 24-hour acknowledgement you will
miss; promise what you will meet. A missed published commitment is worse than a modest one, and the
ledger will record the actual elapsed time against whatever you write.

**Privacy.** The ledger is committed and therefore public. Record the reporter's consented handle,
the send and acknowledgement timestamps, and whether the report was in scope. Do not record the
vulnerability details, the reporter's email address, or anything else.

**No fabrication.** Do not write a report yourself and log it as inbound. The sender must be outside
Eustress LLC — the same definition `G0.07` uses: not an employee, contractor, founder, or family
member, holding no equity, unpaid for the send.

## 3. Scope

### In scope — files this item may edit
- `SECURITY.md` — create, at the repository root
- `docs/security/SEVERITY_RUBRIC.md` — create
- `docs/PROMPTS/harness/external/DISCLOSURE_RUNBOOK.md` — create; what the human does
- `docs/PROMPTS/harness/external/disclosure_ledger_schema.json` — create
- `docs/PROMPTS/harness/checkers/disclosure_ledger.py` — create
- `docs/PROMPTS/artifacts/G0.12/disclosure_ledger.json` — create; empty at authoring time

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- `docs/security/threat_model.json`, `docs/PROMPTS/artifacts/G7.19/exploit_report.json` — frozen
  inputs
- `LICENSE`, `LICENSE-COMMERCIAL.md`
- Any file under `eustress/crates/` — this item publishes a path; it does not fix a vulnerability
- Anything that would send a message or publish a page

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Logging a self-sent report as inbound, counting a message from an affiliated person, recording an
  acknowledgement that was never sent, or loosening the published response commitment after missing
  it are all measurement changes. If the measurement is genuinely wrong, report
  `EXIT_CRITERION_UNMEASURABLE` with evidence and stop.
- **You may not send anything or publish anything.** The runbook says how; a human does it.
- Publish a response commitment the operator will actually meet, and let the ledger measure the
  actual elapsed time against it. A published commitment that the first recorded round trip already
  misses is a self-inflicted wound.
- Scope statements must name paths. "The engine" is not a scope; the tool surface, the plugin loader,
  the Worker, and the container are.
- Severity bands must map to the named exploit classes in `G7.19`'s report and to threat-model
  entries, so a reporter can place their own finding.
- Do not describe Eustress as open source in `SECURITY.md`. `G0.06` blocks this item precisely
  because this is a new customer-facing document and it must not reintroduce the defect that item
  removed.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion
`SECURITY.md` exists at the repository root and declares **at least 4** in-scope surfaces each naming
a repository path, **at least 2** out-of-scope categories each with a stated reason, a numeric
acknowledgement commitment in hours, and **at least 3** severity bands mapped to named `G7.19`
exploit classes. The ledger holds **at least 1** round trip from an unaffiliated sender whose
measured acknowledgement time is **≤** the published commitment. `G0.06`'s licence-phrasing verifier
still exits **0** with `SECURITY.md` in the tree.

### Measurement

Command:

    python docs/PROMPTS/harness/checkers/disclosure_ledger.py \
        --security-md SECURITY.md \
        --rubric docs/security/SEVERITY_RUBRIC.md \
        --exploit-report docs/PROMPTS/artifacts/G7.19/exploit_report.json \
        --ledger docs/PROMPTS/artifacts/G0.12/disclosure_ledger.json \
        --schema docs/PROMPTS/harness/external/disclosure_ledger_schema.json \
        --repo-root . \
        --min-in-scope 4 --min-out-of-scope 2 --min-severity-bands 3 \
        --min-roundtrips 1 --require-unaffiliated
    echo "DISCLOSURE_EXIT=$?"

    python docs/PROMPTS/harness/checkers/licence_phrasing.py \
        --repo-root . --allowlist docs/PROMPTS/harness/checkers/licence_phrasing_allowlist.json \
        --max-allowlist 6 --min-reason-chars 20
    echo "PHRASING_EXIT=$?"

Expected output shape:

    in_scope_surfaces=4 all_cite_existing_paths=true
    out_of_scope_categories=3 all_have_reasons=true
    ack_commitment_hours=72
    severity_bands=4 mapped_exploit_classes=6 unmapped_bands=0
    roundtrips=1 unaffiliated=1
      trip[0] sent=2026-…T…Z acked=2026-…T…Z elapsed_hours=19.4 within_commitment=true
              in_scope=true reporter_handle="…"
    DISCLOSURE_EXIT=0
    unallowlisted_hits=0
    PHRASING_EXIT=0

Pass condition:

    DISCLOSURE_EXIT == 0  AND  PHRASING_EXIT == 0
    AND in_scope_surfaces >= 4  AND  all_cite_existing_paths == true
    AND out_of_scope_categories >= 2  AND  all_have_reasons == true
    AND ack_commitment_hours is a positive integer
    AND severity_bands >= 3  AND  unmapped_bands == 0
    AND roundtrips >= 1  AND  unaffiliated == roundtrips
    AND every trip has within_commitment == true

Read the elapsed hours against the published commitment. `SECURITY.md` existing is not the pass; the
round trip is.

## 6. Critic gate

`critic_gate` is `[]`. No rubric dimension can read a disclosure policy. The compensating tightness:
every in-scope surface must cite a path that exists, so the scope cannot be aspirational; every
out-of-scope category needs a stated reason; the acknowledgement commitment is a number the ledger
measures against rather than a sentiment; severity bands must map onto the frozen `G7.19` exploit
classes with zero unmapped; the round trip must come from an unaffiliated sender; and `G0.06`'s
phrasing verifier is re-run so a new customer-facing document cannot reintroduce the licence defect
that item just removed.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — SECURITY.md with a dedicated address, severity mapped to G7.19
                 classes, human sends the runbook to one external contact
   -> if still failing, MANDATORY approach change. Retrying the same address is NOT an approach
      change; moving from a mailbox to the repository host's private-reporting channel, referenced
      from SECURITY.md, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with roundtrips == 0
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: the published address cannot be reached from outside (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/G0.12/disclosure_ledger.json`, with `SECURITY.md` at the repository root and
the severity rubric at `docs/security/SEVERITY_RUBRIC.md`.

A reader finds: the in-scope surfaces with their paths and the out-of-scope categories with their
reasons; the published acknowledgement commitment in hours; the severity bands and the `G7.19`
exploit classes each maps to; and one row per round trip with the sender's consented handle, their
attested lack of affiliation, the send and acknowledgement timestamps, the measured elapsed hours,
and whether the commitment was met. This is the W3 evidence that the disclosure path exists and
works, and it answers `as04_vulnerability_disclosure` in `G0.05`'s questionnaire.

## 9. Definition of NOT done

- `SECURITY.md` is written and nobody has ever sent to the address. That is the artifact this item
  exists to distinguish itself from.
- The round trip was sent by the founder, an advisor, or a friend as a favour. The definition of
  unaffiliated is the same one `G0.07` uses, and a friendly send recorded as external is fabricated
  evidence.
- The acknowledgement commitment is 24 hours because it sounds professional, and the first recorded
  trip took 60. Publish what will be met.
- The scope section says "the engine" or "the platform". A reporter needs to know whether the Worker
  and the container count; four named surfaces with paths is the floor.
- Severity bands are `low/medium/high` with no mapping, so a reporter cannot tell where a
  `scorer_tampering` finding lands and the operator decides after the fact.
- The plugin directory is listed as in scope while `G0.03` is unshipped, so every report is the same
  known finding. It is documented as unsandboxed and tracked; say so and exclude it with the reason.
- `SECURITY.md` calls the project open source, reintroducing the defect `G0.06` removed. The
  phrasing verifier is re-run in the exit criterion for this reason.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.
````
