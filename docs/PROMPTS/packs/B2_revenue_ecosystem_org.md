# B2 — Revenue Architecture, Ecosystem & Governance, Team & Capital

**Status:** Normative pack. Every item conforms to `docs/PROMPTS/03_PROMPT_SCHEMA.md`.
**Owns:** Workloads **W2** (Revenue Rail), **W4** (Vertical Proof), **W5** (Extension Surface &
Merit Ladder), **W6** (Operator Leverage). Secondary evidence into **W3** (Trust & Verifiability).
**Item Zero:** `G1.40` — nothing else in this pack may start until it is `PASSED`.

## What this pack owns

The path from *no revenue at all* to a durable multi-billion-ARR shape; the contributor ecosystem
that is actually legal under `LICENSE` (PolyForm Shield 1.0.0, source-available); and the
organisation and capital structure that has to carry both. Every item attaches to a verified path
in this repository. No item is a market-sizing exercise.

## Phase mapping rule

`03_PROMPT_SCHEMA.md` §3.1 admits exactly seven phase tags, all named for gauntlet phases. A
business item is assigned **the phase whose evidence gates it**:

- **G1** — the item depends on no engineering evidence and may open immediately. Most of this pack.
- **G2** — the item's credibility rests on numerical trust (determinism, instrumented sim time).
- **G6** — the item is about the sold studio surface being honest (the unwired-button class).

Every item in this pack carries `critic_gate: []` and `capture_recipe: none`. The Critic rubric
scores captured frames; a pricing sheet has no frames. Schema §3.4 permits an empty gate *only*
when the exit criterion is unusually tight, so every item here is gated on a literal command that
exits 0 and prints a value a third party can re-derive from the repository.

## ID band

This pack reserves `<PHASE>.40`–`<PHASE>.59`. No other pack may use that band. IDs are never
reused, never renumbered.

## Measured baseline (established 2026-08-06, repo root `E:/Workspace/EustressEngine`, branch `main`, HEAD `71ccf6fe` with a dirty worktree)

| Fact | Value | How it was measured |
|---|---|---|
| Ribbon tool ids with display metadata | **1999** | `grep -cE '^\s*"[a-z0-9_:.-]+" =>' eustress/crates/engine/src/tool_metadata.rs` |
| Of those, `wired: true` | **30** | `grep -oE 'true\),$' eustress/crates/engine/src/tool_metadata.rs \| wc -l` |
| `government` mode tool ids / wired | **563 / 1** | join script in `G6.40` §5 |
| `civil` mode tool ids / wired | **246 / 1** | same |
| `engineering` mode tool ids / wired | **174 / 22** | same |
| `business` mode tool ids / wired | **155 / 19** | same |
| KYC `JURISDICTIONS` country entries | **46** | `python` regex `^\s{4}([A-Z]{2}): \{` over `infrastructure/cloudflare/api/src/index.js` |
| `docs/manufacturing/` (declared registry data root) | **absent** | `ls docs/manufacturing` → no such directory |
| `CONTRIBUTING.md` | **absent** | `ls CONTRIBUTING.md` → no such file |
| `.github/CODEOWNERS` | **absent** | `.github/` contains only `FUNDING.yml` and `workflows/` |
| `infrastructure/forge/consul/` | **absent** | `infrastructure/forge/` = `README.md nomad scripts terraform` |

**Premise correction, load-bearing for this pack.** `docs/PROMPTS/00_MASTER_PROTOCOL.md:206-209`
and `docs/AUDIT/08_IDENTITY_TRUST.md` state that the Cloudflare Worker `JURISDICTIONS` dict is
**empty**. It is not. `infrastructure/cloudflare/api/src/index.js:39-98` defines **46** country
entries with `natural_ids`, `r2_prefix`, and `age_of_majority`, consumed by `minimumAgeFor()`
(`:102`), `handleJurisdiction()` (`:1177`), `handleKycUpload()` (`:1199`) and the QR handoff
(`:1307`). The "72 countries" marketing figure is still wrong; the correct figure is 46, and KYC
is **not** blocked on an empty dict. Items in this pack use 46.

**Second premise correction.** `docs/AUDIT/09_ECONOMY.md` says `purchase_item` "calls
`state.db.purchase_item()` with no actual SQL." Source inspection shows something worse:
`eustress/crates/backend/src/marketplace.rs:194-215` calls `find_marketplace_item_by_id`,
`has_purchased`, `get_user_balance`, and `purchase_item` on `Database`, and
`eustress/crates/backend/src/db.rs` defines **none of them**. `crates/backend` is a workspace
member (`eustress/Cargo.toml:12`) and CI never builds the workspace, so this has gone unnoticed.
`G1.40` must confirm it with an actual build before any item prices a marketplace.

## Dependency graph

| ID | Title | Tier | Workload | depends_on |
|---|---|---|---|---|
| **G1.40** | Revenue-rail truth ledger **(ITEM ZERO)** | M | W2/W3 | G1.01 |
| G1.41 | Vertical selection and kill list | S | W4 | G1.40 |
| G1.42 | Chargeable-today SKU sheet | S | W2 | G1.40, G1.41 |
| G1.43 | Unit economics of simulation compute | M | W2 | G1.40 |
| G1.44 | Pricing architecture: licence, usage, marketplace | S | W2 | G1.42, G1.43 |
| G1.45 | ARR milestone ladder with capability and org unlocks | S | W2 | G1.42, G1.43, G1.44 |
| G1.46 | Procurement-readiness matrix for a deep-tech buyer | S | W2/W3 | G1.41, G1.42, G0.05 |
| G1.47 | Customer-success health metric from live telemetry | M | W2/W6 | G1.41, G1.42 |
| G1.48 | Licence decision-forcing memo | S | W5 | G1.40 |
| G1.49 | `CONTRIBUTING.md`, DCO, and inbound IP | S | W5 | G1.48 |
| G1.50 | Out-of-tree extension proof and time-to-first-extension | M | W5 | G1.49, G0.03 |
| G1.51 | Merit ladder computable without the Bliss ledger | M | W5 | G1.49 |
| G1.52 | Governance non-negotiables and enforcement points | S | W5/W3 | G1.48 |
| G1.53 | First-hire evaluation tasks from real repo defects | S | W6 | G1.41 |
| G1.54 | Capital strategy on a deep-tech clock | S | W2/W6 | G1.43, G1.45 |
| G1.55 | Decision-rights charter and `CODEOWNERS` | S | W6 | G1.53, G1.52 |
| G1.56 | Founder automation baseline: what to automate first | M | W6 | G1.40 |
| G6.40 | Unwired-button sweep of the sold surface | M | W4 | G1.41, G1.47 |
| G2.40 | Claim-to-evidence map for the sold vertical | S | W4/W3 | G1.41, G6.40, G1.46 |

Nineteen items: 12 at tier S and 7 at tier M. Cumulative envelope: 12 x 60k + 7 x 200k = **2.12M tokens**, **0 builds across the S items and 42 build slots across the M items**.

## Human-only decisions this pack routes to, and never makes

Per `00_MASTER_PROTOCOL.md` §6: changing the licence; any external publication; any spend, price,
or signed agreement; production deployment; any Bliss ledger design touching transferability or
cash-out; merging to `main`. Items `G1.44`, `G1.48`, `G1.54` all terminate in a decision packet,
not a decision.

---

---
id: G1.40
title: Revenue-rail truth ledger with per-capability build evidence
workload: W2
workload_secondary: [W3]
phase: G1
depends_on: [G1.01]
blocks: [G1.41, G1.42, G1.43, G1.48, G1.56]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.40/revenue_rail_ledger.json
escalation: >
  If `cargo build -p eustress-backend` cannot be run to completion for any reason other than a
  compile error in the crate itself (missing system toolchain, network failure fetching crates,
  disk exhaustion), STALL immediately. A ledger whose money-rail rows are asserted from source
  reading alone is exactly the artifact this item exists to replace.
status: DRAFT
notes: >
  Tier M rather than S because the item must actually build the crate that holds the marketplace
  purchase handler. Six build slots is generous for one crate; the surplus exists so the agent can
  also build `eustress-bliss` and `eustress-identity` rather than asserting their state.
---

## 1. Objective

A single machine-checkable ledger exists that records, for every money-touching capability in
Eustress, its verified state, the repository path that proves that state, and whether it can be
charged for today. Every row whose state is `live` was verified by running or building something,
not by reading a document. A reader who has never seen this repository can re-derive every row.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. It is never described as a game engine. The licence is **PolyForm Shield
1.0.0** (`LICENSE`), dual-licensed against `LICENSE-COMMERCIAL.md`. Say **source-available**;
never say open source. Physics is Avian, never Rapier. Units are meter-native; studs are a display
unit only. `.slint` files compile to Rust, so Slint *is* Rust.

**Why this item exists.** Every business decision downstream of here — what to price, which
vertical to sell, what to tell an investor, which hire comes first — is currently made against
documents that disagree with the source. Three examples that are already known to be wrong:

1. `docs/AUDIT/08_IDENTITY_TRUST.md` and `docs/PROMPTS/00_MASTER_PROTOCOL.md:206-209` say the
   Cloudflare Worker `JURISDICTIONS` dict is empty. It contains **46** country entries at
   `infrastructure/cloudflare/api/src/index.js:39-98`, consumed by `minimumAgeFor()` at `:102`.
2. `docs/AUDIT/09_ECONOMY.md` says the marketplace `purchase_item` handler "calls
   `state.db.purchase_item()` with no actual SQL." In fact
   `eustress/crates/backend/src/marketplace.rs:194-215` calls four methods —
   `find_marketplace_item_by_id`, `has_purchased`, `get_user_balance`, `purchase_item` — and
   `eustress/crates/backend/src/db.rs` defines none of them. The crate is a workspace member
   (`eustress/Cargo.toml:12`) and CI never builds the workspace, so nobody has noticed.
3. `docs/marketing/UofA_Center_For_Innovation_Pilot.html` advertises "Open source · MIT-friendly
   licensing," which `LICENSE` contradicts.

**What CI actually does**, so you know why source reading is not evidence:
`.github/workflows/ci.yml` runs `cargo deny check`, a naga WGSL validation that skips any shader
with naga_oil directives, and a `cargo tree` grep. `.github/workflows/linux-engine.yml` runs one
step, `cargo check --package eustress-engine`. `.github/workflows/release.yml` runs
`cargo build --release --package eustress-engine`. **No `cargo test`, no clippy, no workspace
build, anywhere.** Nothing in this repository proves that any crate other than `eustress-engine`
compiles.

**Build reality.** A full engine build takes 10–15 minutes. Only one cargo build may run at a
time — the workspace shares a single `target/` directory and concurrent builds produce link
failures (LNK2001 / SAC os error 4551). Never kill a build mid-compile. `crates/backend` is a much
smaller crate than the engine; expect single-digit minutes.

**Corporate and payments state, as recorded in `LAUNCH_PLAN.md`:** Nevada LLC "In progress
(forming)" (Stream 2); banking "Blocked (needs EIN)" (Stream 3). `docs/launch/PUBLIC_ALPHA_CHECKLIST.md`
still lists the `releases.eustress.dev` R2 bucket as an unchecked `[BLOCKER]`.

**What is known to work and is therefore a candidate for `chargeable_today`:** the desktop studio;
the MCP tool surface (`eustress/crates/tools/src/` plus
`eustress/crates/mcp-server/src/bridge_tools.rs`); CAD agent interface Tier 1
(`cad_describe_part` / `cad_validate` / `cad_measure`); the usage-telemetry pipeline
(`eustress/crates/engine/src/usage_telemetry.rs`, posting to
`https://api.eustress.dev/api/telemetry/usage` at `:193`); and the negotiated commercial licence
(`LICENSE-COMMERCIAL.md`, no published price, contact `licensing@eustress.dev`).

**Prices that exist on paper only:** `docs/monetization/CURRENCY.md:44-48` (Bliss packs $0.99 to
$49.99, Steam 30% cut table at `:52-58`); `docs/monetization/SUBSCRIPTIONS.md:8` marked
"Status: Pre-Release Design".

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.40/` — create it; write the ledger and all build logs here

### Out of scope — do not edit
- Anything under `.github/workflows/` — never modify CI to make a gate pass
- `docs/PROMPTS/01_CRITIC_RUBRIC.md` — you may not read or edit the rubric
- Any capture already hashed into a provenance manifest
- **All Rust source.** This item measures; it does not fix. If `eustress-backend` fails to
  compile, that is the finding, not a task.
- `docs/AUDIT/**` — the audit is a separate ledger with its own pass discipline
- `LAUNCH_PLAN.md`, `README.md`, `LICENSE`, `LICENSE-COMMERCIAL.md`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item, whatever number results.**
  Dropping a required capability key, marking a row `live` on the strength of a design document,
  or recording a build you did not run are all measurement changes. If a required row genuinely
  cannot be resolved, set its state to `unknown` and its `verified_by` to `blocked`, and say why
  in `blocked_reason`. `unknown` is an allowed, honest value; a fabricated `live` is not.
- Do not run more than one cargo build at a time.
- Every `evidence_path` must be repo-relative with forward slashes and must exist. The verifier
  checks this and there is no exception.
- Do not repeat the "80–90% cost reduction" claim from `docs/architecture/EUSTRESS_FORGE.md`; it
  is unmeasured marketing copy.
- Do not repeat "72 countries". The measured value is 46.

## 5. Exit criterion

### Criterion

`docs/PROMPTS/artifacts/B2/G1.40/revenue_rail_ledger.json` contains **all 22 required capability
keys**, every `evidence_path` in it resolves to a file that exists, **no** row in state `absent` or
`spec_only` is marked `chargeable_today`, **at least 4** rows carry `verified_by` of `build` or
`run` with a log file on disk, and the ledger's recorded `jurisdiction_count` equals the value
re-derived from `infrastructure/cloudflare/api/src/index.js` at verification time.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - <<'PY'
    import json, os, re, sys
    P = "docs/PROMPTS/artifacts/B2/G1.40/revenue_rail_ledger.json"
    REQUIRED = {
      "bliss_ledger_debit","marketplace_purchase_handler","stripe_connect","kyc_jurisdictions",
      "subscriptions_backend","refunds","tax_vat","payout_ledger","steam_iap",
      "windows_code_signing","macos_notarisation","releases_r2_bucket",
      "nevada_llc","ein","business_banking","commercial_licence","paid_pilot",
      "usage_telemetry_pipeline","cad_tier1_mcp","mcp_tool_surface","desktop_studio",
      "forge_hosting"}
    STATES = {"absent","spec_only","partial","live","unknown"}
    VERIF  = {"source_read","build","run","external","blocked"}
    d = json.load(open(P, encoding="utf-8"))
    rows = d["rows"]
    keys = {r["capability"] for r in rows}
    fail = []
    miss = REQUIRED - keys
    if miss: fail.append(f"missing capability keys: {sorted(miss)}")
    for r in rows:
        if r["state"] not in STATES: fail.append(f"{r['capability']}: bad state {r['state']}")
        if r["verified_by"] not in VERIF: fail.append(f"{r['capability']}: bad verified_by")
        ep = r.get("evidence_path")
        if not ep or not os.path.exists(ep): fail.append(f"{r['capability']}: evidence_path missing: {ep!r}")
        if r["state"] in ("absent","spec_only") and r.get("chargeable_today"):
            fail.append(f"{r['capability']}: state {r['state']} cannot be chargeable_today")
        if r["state"] == "unknown" and not r.get("blocked_reason"):
            fail.append(f"{r['capability']}: state unknown requires blocked_reason")
    hard = [r for r in rows if r["verified_by"] in ("build","run")]
    if len(hard) < 4: fail.append(f"only {len(hard)} rows verified by build/run, need >= 4")
    for r in hard:
        lp = r.get("log_path")
        if not lp or not os.path.exists(lp) or os.path.getsize(lp) < 200:
            fail.append(f"{r['capability']}: log_path missing or < 200 bytes: {lp!r}")
    src = open("infrastructure/cloudflare/api/src/index.js", encoding="utf-8").read()
    n = len(set(re.findall(r'(?m)^\s{4}([A-Z]{2}): \{', src)))
    if d.get("jurisdiction_count") != n:
        fail.append(f"jurisdiction_count {d.get('jurisdiction_count')} != re-derived {n}")
    print(f"rows={len(rows)} hard_verified={len(hard)} jurisdictions={n} chargeable={sorted(r['capability'] for r in rows if r.get('chargeable_today'))}")
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PY
    echo "EXIT=$?"

Expected output shape:

    rows=22 hard_verified=5 jurisdictions=46 chargeable=['cad_tier1_mcp', 'commercial_licence', 'desktop_studio', 'mcp_tool_surface', 'paid_pilot', 'usage_telemetry_pipeline']
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  the line "RESULT: PASS" is present  AND  jurisdictions=46

Verify by reading the printed line, not by observing that the JSON file exists.

## 6. Critic gate

`critic_gate: []`. The Critic rubric scores captured frames and has no dimension that applies to a
ledger. The replacement is the mechanical criterion in §5, which is unusually tight in three
specific ways: the required-key set is fixed and cannot be trimmed; every evidence path is resolved
against the filesystem at verification time; and `jurisdiction_count` is re-derived from source by
the verifier rather than trusted from the artifact. An agent cannot pass this item by writing
confident prose.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — enumerate from the audit ledger and LAUNCH_PLAN, then verify each
                 row against source and builds
   -> if still failing, MANDATORY approach change. Adding rows is NOT an approach change;
      switching to a source-first enumeration (walk crates/backend, crates/bliss,
      crates/identity, infrastructure/cloudflare and derive the capability list from what
      exists) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the verifier's failure list shrinks by < 2 entries
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: `cargo build -p eustress-backend` cannot be run to completion for an
                   environmental reason (see front matter)
```

Stall packet requests exactly one of: LOWER the required-key set to a named subset with the stated
consequence for `G1.42`; FUND approach D; DEFER behind a named item; KILL.

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.40/revenue_rail_ledger.json`

A reader finds: a top-level `generated_at`, `commit`, `jurisdiction_count`, and `builds` object;
then `rows`, one per capability, each with `capability`, `surface`, `state`, `evidence_path`,
`evidence_line`, `verified_by`, `log_path` (when built or run), `chargeable_today`,
`blocked_reason` (when `unknown`), and `blocks` (the capability keys this one gates). Alongside it,
`docs/PROMPTS/artifacts/B2/G1.40/logs/*.log` — the raw build and run transcripts, one per hard
verification, each containing the literal command line at its head.

This file is the W2 evidence for the item and the input to `G1.41`, `G1.42`, `G1.43`, `G1.48`, and
`G1.56`.

## 9. Definition of NOT done

- The ledger is complete and internally consistent but no build was run, so every row reads
  `source_read`. The `hard_verified >= 4` check exists precisely to catch this.
- `marketplace_purchase_handler` is recorded as `partial` on the strength of the handler existing,
  without a build log showing whether `eustress-backend` compiles at all.
- `kyc_jurisdictions` repeats the "empty dict" claim from the audit. The verifier re-derives 46 and
  fails the item.
- The chargeable set is inflated by marking `forge_hosting` or `subscriptions_backend` chargeable
  because a design document describes them fully.
- Rows are added beyond the 22 required keys to pad the count while a required key is quietly
  renamed. Renaming a required key is a missing key.
- The ledger cites `docs/AUDIT/09_ECONOMY.md` as an `evidence_path` for a code capability. The path
  exists, so the verifier passes it, but a document is not evidence that code works — an audit path
  is only admissible for corporate rows (`nevada_llc`, `ein`, `business_banking`) and even there
  `LAUNCH_PLAN.md` is the better source.
- The item "fixes" `eustress-backend` so the build passes. Rust source is out of scope; a passing
  build achieved by editing code fails the item outright.

---

---
id: G1.41
title: Vertical selection with an evidence-scored kill list
workload: W4
workload_secondary: [W2]
phase: G1
depends_on: [G1.40]
blocks: [G1.42, G1.46, G1.47, G1.53, G6.40, G2.40]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.41/vertical_selection.json
escalation: >
  If two verticals score within 2 points of each other on the weighted total, do not break the tie
  by judgement. STALL and hand the human a two-option decision, because a coin-flip here
  mis-directs G1.46, G1.47, G6.40 and G2.40 for the rest of the program.
status: DRAFT
notes: >
  S tier, zero builds. Every input is a file read or a grep. The whole value of the item is that
  the selection is forced to cite counts, not enthusiasm.
---

## 1. Objective

Exactly one vertical is selected as the sold surface for the next two quarters, chosen by a scored
matrix in which every cell cites a repository path or a measured count. The verticals not selected
carry an explicit kill or freeze decision, so no downstream item spends a build slot on them.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0; say source-available. Physics is Avian. Slint is Rust. Units are meter-native.

**The candidate verticals, and their verified state.**

1. **Government / civic (Tucson).** `docs/architecture/GOVERNMENT_MODE.md`. Scaffold shipped
   2026-07-26: twelve disciplines, a Tucson Universe of 16 Spaces and 32 portals with a
   byte-identical tree hash across re-runs. `docs/architecture/GOVERNMENT_MODE.md:692` opens
   "§9. What has shipped, and what has not"; `:713-714` states that of the declared government tool
   ids exactly one, `data:import`, has a real handler. Measured from source on 2026-08-06:
   `eustress/crates/engine/modes/government.toml` declares **563** unique tool ids of which **1**
   is `wired: true` in `eustress/crates/engine/src/tool_metadata.rs`. The doc itself says the mode
   "must never be described as working software."
2. **Manufacturing.** `docs/development/MANUFACTURING_PROGRAM.md`,
   `docs/development/MANUFACTURING_DEAL_STRUCTURE.md`,
   `docs/development/BLISS_MANUFACTURING_INTEGRATION.md`, `docs/api/EUSTRESS_MANUFACTURING_OVERVIEW.md`.
   Code exists: `eustress/crates/engine/src/manufacturing/mod.rs` (Investor / Manufacturer /
   registry / Bevy plugin) and `eustress/crates/engine/src/workshop/modes/manufacturing.rs`. The
   registry's declared data root is `docs/manufacturing/investors` and
   `docs/manufacturing/manufacturers` (`eustress/crates/engine/src/manufacturing/mod.rs:506-507`)
   and **`docs/manufacturing/` does not exist** — zero investors, zero manufacturers, zero deals.
3. **Engineering / CAD.** `eustress/crates/cad/` with a truck-based kernel;
   `eustress/crates/cad/tests/` exists; CAD agent interface Tier 1 (`cad_describe_part`,
   `cad_validate`, `cad_measure`) has shipped; geometry lives in `eustress/crates/cad/src/measure.rs`.
   Measured 2026-08-06: `eustress/crates/engine/modes/engineering.toml` declares **174** tool ids
   of which **22** are wired — the highest wired count of any mode.
4. **Research / university pilot (V-Cell battery science).** `docs/architecture/VCELL_CASE_STUDY.md`
   is an internal verification walkthrough scripting the Watchman-to-Repairman MCP loop, not a
   customer case study. `docs/AUDIT/19_REALISM_PHYSICS.md` records that the V-Cell
   electrochemistry is a **lumped 0-D model** (Nernst plus Butler-Volmer) with no spatial
   electrochemistry or ion transport, and that `ElectrochemicalState` and `ThermodynamicState` are
   decoupled — temperature does not affect reaction rate. A GTM asset exists:
   `docs/marketing/UofA_Center_For_Innovation_Pilot.html` (and `.pdf`), explicitly free, and
   `LAUNCH_PLAN.md` records the pilot programme as not started.

**Other measured mode counts (same method, 2026-08-06):** `civil` 246 ids / 1 wired; `business`
155 / 19; `student` 238 / 11; `health` 159 / 13; `justice` 92 / 5; `legal` 240 / 6; `military`
169 / 7; `gaming` 22 / 4. Across all metadata, **1999** tool ids and **30** wired.

**Constraint that dominates the choice.** Solo founder, Windows, 10-15 minute serialized builds,
one build at a time. `docs/AUDIT/12_INFRASTRUCTURE.md` Feature 1: the desktop engine is not built
in CI, so the founder is the entire regression surface. This rules out any vertical whose proof
requires SOC 2, ISO 27001, WCAG AA, an SLA, on-call, or a second physical machine.
`docs/AUDIT/06_WEBSITE.md` Feature 15 records that WCAG non-compliance blocks government and
enterprise sales.

**Prerequisite.** `G1.40` has produced
`docs/PROMPTS/artifacts/B2/G1.40/revenue_rail_ledger.json`. Read it. A vertical whose sale depends
on a capability that ledger marks `absent` or `spec_only` cannot score above 2 on the
`chargeable_today` criterion.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.41/` — create it; write the selection and its matrix here

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source, all `.toml` mode manifests, all `.slint` files
- `docs/architecture/GOVERNMENT_MODE.md` and the manufacturing docs — you are scoring them, not
  editing them
- `docs/marketing/**` — an external-facing asset; only the human changes it

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Dropping a criterion,
  re-weighting after seeing the totals, or adding a fifth vertical to rescue a preferred answer are
  all measurement changes. The five criteria and their weights are fixed in §5 and may not be
  altered.
- Every cell score must carry an `evidence` string containing either a repo-relative path or a
  measured count with the command that produced it. A cell whose evidence is an opinion fails.
- Do not select a vertical on the strength of an existing go-to-market asset alone.
  `docs/marketing/` and `docs/development/MANUFACTURING_PROGRAM.md` are sunk cost, not traction.
- Do not re-derive the wired counts by hand. Use the join script in `G6.40` §5 if you want to
  confirm them; otherwise use the values quoted in §2, which are MEASURED.

## 5. Exit criterion

### Criterion

`vertical_selection.json` scores all **4** verticals against all **5** fixed criteria, every one of
the 20 cells carries non-empty `evidence`, exactly **1** vertical has `selected: true`, its
weighted total is the strict maximum, the margin over the runner-up is **> 2.0**, and every
non-selected vertical carries a `disposition` of `kill` or `freeze` with a `revisit_trigger`.

The five criteria and weights are fixed:

| key | weight | scored 0-10 on |
|---|---|---|
| `chargeable_today` | 3 | can a contract be signed for this in 90 days against the `G1.40` ledger |
| `wired_ratio` | 2 | fraction of the vertical's declared surface that actually dispatches |
| `proof_cost_in_builds` | 2 | inverse of how many 10-15 minute builds an end-to-end demo needs |
| `buyer_reachable_solo` | 2 | can one founder reach and close this buyer without a sales team |
| `compliance_floor` | 1 | inverse of the SOC 2 / WCAG / SLA burden the buyer imposes |

### Measurement

Run from the repository root, in Git Bash. Write the verifier below to a scratch file and run it
with `python`; do not retype it.

Command:

    python - << 'PYEOF'
    import json, sys
    P = "docs/PROMPTS/artifacts/B2/G1.41/vertical_selection.json"
    W = {"chargeable_today":3,"wired_ratio":2,"proof_cost_in_builds":2,
         "buyer_reachable_solo":2,"compliance_floor":1}
    d = json.load(open(P, encoding="utf-8"))
    vs = d["verticals"]
    fail = []
    if len(vs) != 4: fail.append("expected 4 verticals, got %d" % len(vs))
    totals = {}
    for v in vs:
        if set(v["scores"]) != set(W): fail.append("%s: criteria set mismatch" % v["key"])
        t = 0
        for k, w in W.items():
            c = v["scores"].get(k, {})
            s = c.get("score")
            if not isinstance(s, (int, float)) or not (0 <= s <= 10):
                fail.append("%s.%s: score out of range: %r" % (v["key"], k, s)); s = 0
            if not str(c.get("evidence", "")).strip():
                fail.append("%s.%s: empty evidence" % (v["key"], k))
            t += s * w
        totals[v["key"]] = t
        if not v.get("selected"):
            if v.get("disposition") not in ("kill", "freeze"):
                fail.append("%s: disposition must be kill or freeze" % v["key"])
            if not str(v.get("revisit_trigger", "")).strip():
                fail.append("%s: missing revisit_trigger" % v["key"])
    sel = [v["key"] for v in vs if v.get("selected")]
    if len(sel) != 1:
        fail.append("expected exactly 1 selected, got %r" % sel)
    else:
        rank = sorted(totals.items(), key=lambda kv: -kv[1])
        if rank[0][0] != sel[0]: fail.append("selected %s is not the max: %r" % (sel[0], rank))
        margin = rank[0][1] - rank[1][1]
        if margin <= 2.0: fail.append("margin %s <= 2.0 - tie, escalate per front matter" % margin)
    print("totals=" + json.dumps(totals, sort_keys=True))
    print("selected=" + (sel[0] if len(sel) == 1 else "NONE"))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    totals={"engineering_cad": 71, "government_civic": 34, "manufacturing": 29, "research_vcell": 48}
    selected=engineering_cad
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  the printed selected key holds the strict maximum
    in the printed totals map

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight in three ways: the criteria
set and weights are fixed by the prompt and re-imposed by the verifier, so re-weighting to reach a
preferred answer fails; every cell must carry evidence; and the > 2.0 margin makes a near-tie an
escalation rather than a quiet judgement call.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — score from the documents and the measured counts in section 2
   -> if still failing, MANDATORY approach change. Re-scoring cells is NOT an approach change;
      switching to a bottom-up enumeration (list the concrete deliverables a signed contract in
      each vertical would require, then score the gap to each) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: top-two margin <= 2.0 (see front matter) — escalate immediately, do not
                   nudge a score to break the tie
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.41/vertical_selection.json`

A reader finds: `generated_at`, `ledger_path` (pointing at the `G1.40` artifact actually read),
and `verticals` — four objects, each with `key`, `name`, `scores` (five criteria, each with
`score` and `evidence`), `selected`, and for the non-selected ones `disposition` and
`revisit_trigger`. Alongside it, `docs/PROMPTS/artifacts/B2/G1.41/kill_list.md` — one paragraph per
frozen or killed vertical stating what stops immediately and what would have to become true to
restart it.

## 9. Definition of NOT done

- Government is selected because the Tucson Universe scaffold and its 563 tool ids look like
  progress. One wired handler out of 563 is the number that matters, and
  `docs/architecture/GOVERNMENT_MODE.md:713` says so.
- Manufacturing is selected because three planning documents and a registry crate exist, while
  `docs/manufacturing/` — the registry's own declared data root — does not.
- The V-Cell research pilot is selected on the strength of
  `docs/marketing/UofA_Center_For_Innovation_Pilot.html`, ignoring that the asset advertises
  MIT-friendly licensing the `LICENSE` contradicts, that the pilot is explicitly free, and that the
  underlying model is lumped 0-D.
- Two verticals are selected "to keep optionality". The verifier requires exactly one.
- The weights are adjusted after the first scoring pass because the totals came out wrong. That is
  a measurement change and fails the item outright.
- Every cell is scored but the `evidence` strings restate the score ("strong", "weak") rather than
  giving a path or a count.
- The kill list is written as "deprioritised for now" with no `revisit_trigger`, so nothing is
  actually stopped and `G6.40` still has four surfaces to sweep.

---

---
id: G1.42
title: Chargeable-today SKU sheet with per-SKU capability backing
workload: W2
workload_secondary: [W4]
phase: G1
depends_on: [G1.40, G1.41]
blocks: [G1.44, G1.45, G1.46, G1.47]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.42/sku_sheet.json
escalation: >
  If fewer than three SKUs survive the backing check, STALL rather than padding. Fewer than three
  chargeable SKUs is itself the finding and the human needs it immediately, because it means the
  only revenue surface is bespoke services and G1.44 through G1.45 must be re-scoped.
status: DRAFT
notes: >
  No prices in this item. This is the SKU boundary — what is being sold, and what proves it can be
  delivered. Price bands are G1.44, and the final price is a human decision per
  00_MASTER_PROTOCOL.md section 6.
---

## 1. Objective

A sheet exists naming every offer Eustress can deliver against a signed contract today, where each
offer is backed by capabilities the `G1.40` ledger marks `partial` or `live` and by nothing else.
An offer that depends on any `absent` or `spec_only` capability does not appear on the sheet.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0 (`LICENSE`), dual-licensed against `LICENSE-COMMERCIAL.md`; say source-available.
Physics is Avian. Slint is Rust. Units are meter-native.

**The two prerequisite artifacts, which you must read.**
- `docs/PROMPTS/artifacts/B2/G1.40/revenue_rail_ledger.json` — per-capability verified state.
- `docs/PROMPTS/artifacts/B2/G1.41/vertical_selection.json` — the one selected vertical.

**What `LICENSE-COMMERCIAL.md` already commits to.** It defines when a commercial licence *is*
needed (offering Eustress or a substantially similar engine, editor, or simulation platform as your
own product or service, hosted or distributed; embedding it where legal or procurement requires
warranties, indemnification, support SLAs, or a perpetual grant independent of the public
repository; obtaining rights the Shield noncompete restricts) and when it is *not* (building,
shipping, and selling end products made **with** Eustress; internal use at a company of any size
including production; modifying and forking for your own products; academic use, research,
evaluation, and personal projects). Terms are "negotiated per organization"; typical terms include
a perpetual grant for a specified version range, priority support, and optional indemnification;
pricing "scales with the scope of rights granted, not with your revenue". Contact is
`licensing@eustress.dev`. **There is no published price and no SKU.**

**The consumer price sheets that exist and must not be reused here.**
`docs/monetization/CURRENCY.md:44-48` prices Bliss packs from $0.99 to $49.99 against a Steam 30%
cut (`:52-58`). `docs/monetization/SUBSCRIPTIONS.md:8` is marked "Status: Pre-Release Design".
Neither prices simulation, CAD, kernel-law validation, or hosted compute — the things the pilot
assets actually sell — and both assume a consumer funnel that is broken at both ends:
`docs/AUDIT/06_WEBSITE.md` Feature 5 records the Play button as a P0 dead end, and Windows
authenticode and macOS notarisation are both at 0% (`docs/AUDIT/12_INFRASTRUCTURE.md`), so every
install shows a SmartScreen or Gatekeeper warning.

**Capabilities known to work.** The desktop studio; the MCP tool surface — 79 tool descriptors in
`eustress/crates/tools/src/`, 24 bridge tools in
`eustress/crates/mcp-server/src/bridge_tools.rs`, and one hand-rolled tool in
`eustress/crates/mcp-server/src/tools.rs`, mode-filtered by
`eustress/crates/tools/src/modes.rs`; CAD agent interface Tier 1 (`cad_describe_part`,
`cad_validate`, `cad_measure`) with geometry in `eustress/crates/cad/src/measure.rs`; the
usage-telemetry pipeline (`eustress/crates/engine/src/usage_telemetry.rs`, posting to
`https://api.eustress.dev/api/telemetry/usage` at `:193`); and the headless batch runner declared
as the `eustress-headless` binary at `eustress/crates/engine/Cargo.toml:60`, implemented in
`eustress/crates/engine/src/bin/headless.rs`.

**Capabilities known not to work, which therefore cannot back a SKU.** Anything requiring hosting:
`docs/AUDIT/12_INFRASTRUCTURE.md` records Vault at 0%, Prometheus and Grafana at 0%, multi-region
at 0%, and no DR runbooks; `infrastructure/forge/` contains only `README.md`, `nomad`, `scripts`,
and `terraform` — there is no `consul` directory. Anything requiring a signed binary. Anything
requiring money to move through the product.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.42/` — create it; write the SKU sheet and its notes here

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source
- `LICENSE`, `LICENSE-COMMERCIAL.md` — the licence text is a human decision
- `docs/monetization/**` — the consumer sheets stay as they are; this item does not reconcile them
- `docs/PROMPTS/artifacts/B2/G1.40/**` and `docs/PROMPTS/artifacts/B2/G1.41/**` — read-only inputs;
  editing an upstream artifact to make this item pass is a measurement change

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Editing the `G1.40` ledger
  to upgrade a capability state, or renaming a capability so the cross-check misses it, are
  measurement changes. If a ledger row is genuinely wrong, report `EXIT_CRITERION_UNMEASURABLE`
  with the evidence and stop.
- **No prices in this artifact.** Every SKU carries a `pricing_basis` string naming the unit
  (per-seat-year, per-simulation-hour, per-engagement, one-time) and `price: null`. Prices are
  `G1.44`; the final number is a human decision.
- Do not create a SKU for anything in a vertical `G1.41` marked `kill` or `freeze`.
- Do not describe Eustress as open source anywhere in the artifact. Source-available.

## 5. Exit criterion

### Criterion

`sku_sheet.json` contains **at least 3** SKUs; **every** `backed_by` capability key resolves to a
row in the `G1.40` ledger whose `state` is `partial` or `live`; **no** SKU has a non-null `price`;
**every** SKU names a `delivery_evidence` path that exists; and **at least one** SKU has both
`requires_no_hosting: true` and `requires_no_signed_binary: true`.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, os, sys
    S = "docs/PROMPTS/artifacts/B2/G1.42/sku_sheet.json"
    L = "docs/PROMPTS/artifacts/B2/G1.40/revenue_rail_ledger.json"
    led = {r["capability"]: r for r in json.load(open(L, encoding="utf-8"))["rows"]}
    d = json.load(open(S, encoding="utf-8"))
    skus = d["skus"]; fail = []
    if len(skus) < 3: fail.append("only %d SKUs, need >= 3 (see escalation)" % len(skus))
    for s in skus:
        if not s.get("backed_by"): fail.append("%s: empty backed_by" % s["key"])
        for cap in s.get("backed_by", []):
            r = led.get(cap)
            if r is None: fail.append("%s: backed_by %r not in ledger" % (s["key"], cap))
            elif r["state"] not in ("partial", "live"):
                fail.append("%s: backed_by %s is state %s" % (s["key"], cap, r["state"]))
        if s.get("price") is not None: fail.append("%s: price must be null in this item" % s["key"])
        if not str(s.get("pricing_basis", "")).strip():
            fail.append("%s: missing pricing_basis" % s["key"])
        de = s.get("delivery_evidence")
        if not de or not os.path.exists(de):
            fail.append("%s: delivery_evidence missing: %r" % (s["key"], de))
    clean = [s["key"] for s in skus
             if s.get("requires_no_hosting") and s.get("requires_no_signed_binary")]
    if not clean: fail.append("no SKU deliverable without hosting AND without a signed binary")
    blob = json.dumps(d).lower()
    for bad in ("open source", "open-source", "rapier", "game engine"):
        if bad in blob: fail.append("forbidden phrase in artifact: %r" % bad)
    print("skus=%d deliverable_without_infra=%r" % (len(skus), clean))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    skus=4 deliverable_without_infra=['commercial_licence_negotiated', 'paid_pilot_engagement']
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  skus >= 3  AND  deliverable_without_infra non-empty

## 6. Critic gate

`critic_gate: []`. The mechanical criterion is tight because it cross-checks every SKU against a
separately produced artifact the agent may not edit, forbids prices outright so the item cannot be
passed by asserting revenue, and scans the artifact for the four phrases that most often creep into
sales copy in this repository.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — enumerate SKUs from the LICENSE-COMMERCIAL.md offer surface and the
                 selected vertical, then test each against the ledger
   -> if still failing, MANDATORY approach change. Adding a SKU is NOT an approach change;
      switching to a capability-first enumeration (start from every ledger row in state
      partial or live and ask what contract each could back) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: fewer than 3 SKUs survive the backing check (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.42/sku_sheet.json`

A reader finds: `generated_at`, `ledger_path`, `vertical` (the key selected by `G1.41`), and
`skus` — each with `key`, `name`, `what_the_buyer_receives`, `backed_by` (ledger capability keys),
`delivery_evidence` (a path that exists), `pricing_basis`, `price: null`, `requires_no_hosting`,
`requires_no_signed_binary`, and `excluded_because` for anything deliberately left off. Alongside
it, `docs/PROMPTS/artifacts/B2/G1.42/rejected_skus.md` — one line per offer considered and dropped,
naming the `absent` or `spec_only` capability that killed it.

## 9. Definition of NOT done

- A hosted-simulation SKU appears because `docs/architecture/EUSTRESS_FORGE.md` describes it fully.
  Vault is 0%, `infrastructure/forge/consul/` does not exist, and no SKU may be backed by a
  `spec_only` capability.
- A subscription SKU appears at $4.99 or $9.99 from `docs/monetization/SUBSCRIPTIONS.md`. That
  document is marked pre-release design, its backend is 0%, and this item carries no prices at all.
- The marketplace appears as a SKU because `eustress/crates/backend/src/marketplace.rs` has a
  purchase handler. The `G1.40` ledger records whether that crate compiles; if it does not, the
  capability is `absent` and the cross-check fails.
- Three SKUs are produced but all three require a signed binary, so `deliverable_without_infra` is
  empty and there is nothing sellable this quarter.
- `delivery_evidence` points at a design document rather than at the code or artifact that does the
  delivering.
- The sheet includes SKUs for a vertical `G1.41` marked `kill`, on the grounds that a buyer might
  still ask.

---

---
id: G1.43
title: Unit economics of simulation compute, from a measured throughput baseline
workload: W2
workload_secondary: [W3]
phase: G1
depends_on: [G1.40]
blocks: [G1.44, G1.45, G1.54]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.43/unit_economics.json
escalation: >
  If neither benchmark binary completes a run on the founder's machine, STALL. A COGS model whose
  throughput input is an estimate is worse than no model, because it will be quoted in a pricing
  conversation and cannot be defended.
status: DRAFT
notes: >
  Cloud instance rates are an external input and are labelled TARGET, not MEASURED. The one thing
  this item must genuinely measure is how much simulated work one machine does per unit of
  wall-clock, because that is the only term in the model that lives in this repository.
---

## 1. Objective

A unit-economics model exists for compute-heavy simulation in which the throughput term is measured
on this repository's own benchmarks, every other term is explicitly labelled `TARGET` or
`CONFIG_DEFAULT`, and the derived cost per simulated hour is recomputable from the model's own
inputs. A pricing conversation can quote it without hedging.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Physics is Avian,
never Rapier. Units are meter-native; studs are a display unit only. Licence is PolyForm Shield
1.0.0; say source-available. Slint is Rust.

**Why COGS is real here and not a rounding error.** Unlike a seat-based SaaS product, the thing a
buyer pays for is simulated time, and simulated time costs CPU and GPU. Any pricing model that does
not know how much simulated work one machine does per wall-clock hour is guessing at gross margin.

**The two benchmarks that exist in this repository and actually run.** Both live in
`eustress/benches/instance-capacity/`, which is a workspace member (`eustress/Cargo.toml:41`) and
declares two binaries (`eustress/benches/instance-capacity/Cargo.toml:8` and `:12`):

- `cargo run --release --bin avian-physics-bench` — 600 steps at 60 Hz with
  `TimeUpdateStrategy::FixedTimesteps(1)`, reporting overall, early-100, and steady-state
  milliseconds per step for two scenarios named `falling` and `static_heavy`. Source:
  `eustress/benches/instance-capacity/src/bin/avian_physics_bench.rs`.
- `cargo run --release --bin instance-capacity` — TOML to bincode+zstd to ECS to GPU to physics,
  doubling N exponentially and reporting a named `StopReason`. Source:
  `eustress/benches/instance-capacity/src/main.rs`.

**The measured number this repository already has, and its limits.**
`docs/development/BENCHMARK_VS_ENGINE_AUDIT.md` records a benchmark at 8K entities and 5,406 FPS
against the engine at 10K entities and roughly 45 FPS — a 120x gap — with about 10,000 draw calls
and one-second stutters traced to `write_instance_changes_system` performing 20,000 synchronous
disk operations. Several of those root causes are marked fixed in that document. Treat the
benchmark figure as a ceiling and the engine figure as the floor; a pricing model must be built on
the engine-side number, because that is what a customer's workload runs on.

**A number you must not reuse.** The 2.10M-entity figure at `docs/AUDIT/05_SPACE_STREAMING.md:23`
is an `active_cap` **CONFIG DEFAULT**, not a measurement. Label it that way or omit it.

**A claim you must not repeat.** The "80–90% cost reduction" in
`docs/architecture/EUSTRESS_FORGE.md` is unmeasured marketing copy.

**A correctness hazard that bounds what "a simulated hour" means.**
`eustress/crates/common/src/simulation/clock.rs` advances `simulation_time_s` by the full
compressed delta, but caps physics ticks at `max_ticks_per_frame` (default 10) and **zeroes the
accumulator** on saturation (`clock.rs:100-102`). At high `time_scale` the clock therefore reports
compressed time while the physics steps that would have covered it are silently discarded, and
`effective_compression()` (`clock.rs:131`) still reports a healthy ratio. **A unit-economics model
must price a *stepped* simulated second, not a *reported* simulated second.** Define your unit as
physics steps executed, and convert to simulated seconds using the fixed timestep
(`Time::<Fixed>::from_hz(60.0)`, pinned in `eustress/crates/engine/src/main.rs`), never using the
clock's own `simulation_time_s`.

**Build reality.** 10–15 minutes for a full engine build; the bench crate is smaller. One cargo
build at a time — the workspace shares a single `target/` and concurrent builds produce link
failures. Never kill a build mid-compile. Validate by running, not by `cargo check`.

**Hosting reality, which caps what you may assume about deployment.**
`docs/AUDIT/12_INFRASTRUCTURE.md` records Vault at 0%, Prometheus and Grafana at 0%, multi-region
at 0%, and no DR runbooks. `infrastructure/forge/` contains `README.md`, `nomad`, `scripts`, and
`terraform` — there is no `consul` directory. So the model prices **compute the customer or the
founder rents**, not a managed service that does not exist.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.43/` — create it; write the model, the raw bench logs, and notes

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source, including the benchmark crate. You may run the benchmarks; you may not tune
  them, change their scenarios, or change their step counts.
- `docs/development/BENCHMARK_VS_ENGINE_AUDIT.md`
- `docs/PROMPTS/artifacts/B2/G1.40/**` — read-only input

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Reducing the bench step
  count, running only the cheaper scenario, or substituting the 5,406 FPS benchmark figure for an
  engine-side measurement are all measurement changes. If a benchmark genuinely cannot run, report
  `EXIT_CRITERION_UNMEASURABLE` with the transcript and stop.
- Every numeric field in the model carries a sibling `<field>_kind` of exactly `MEASURED`,
  `TARGET`, or `CONFIG_DEFAULT`. A `MEASURED` field additionally carries `<field>_command` and
  `<field>_log`, and the log file must exist and contain the command line.
- Cloud and electricity rates are `TARGET`. Do not present a vendor price list as measured.
- Define the priced unit as **physics steps executed**, converted at the pinned 60 Hz fixed
  timestep. Do not use `simulation_time_s`.
- Run the benchmarks one at a time. Do not start a second cargo process while one is running.

## 5. Exit criterion

### Criterion

`unit_economics.json` contains **at least 3** fields with `_kind == "MEASURED"`, each with a
`_command` and an existing `_log` of at least 200 bytes that contains its command string; **every**
numeric field carries a valid `_kind`; and the derived field `cost_per_million_steps_usd`
recomputes from `machine_cost_per_hour_usd` and `steps_per_second_measured` to within **1%**.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, os, sys
    P = "docs/PROMPTS/artifacts/B2/G1.43/unit_economics.json"
    d = json.load(open(P, encoding="utf-8"))
    fail = []
    KINDS = {"MEASURED", "TARGET", "CONFIG_DEFAULT"}
    nums = {k: v for k, v in d.items()
            if isinstance(v, (int, float)) and not isinstance(v, bool)}
    measured = 0
    for k in nums:
        kind = d.get(k + "_kind")
        if kind not in KINDS:
            fail.append("%s: missing or invalid _kind (%r)" % (k, kind)); continue
        if kind == "MEASURED":
            measured += 1
            cmd = d.get(k + "_command"); log = d.get(k + "_log")
            if not cmd: fail.append("%s: MEASURED without _command" % k)
            if not log or not os.path.exists(log) or os.path.getsize(log) < 200:
                fail.append("%s: _log missing or < 200 bytes: %r" % (k, log))
            elif cmd and cmd.split()[0] not in open(log, encoding="utf-8", errors="replace").read():
                fail.append("%s: _log does not contain the command" % k)
    if measured < 3: fail.append("only %d MEASURED fields, need >= 3" % measured)
    try:
        sps = d["steps_per_second_measured"]; mch = d["machine_cost_per_hour_usd"]
        want = (mch / 3600.0) * (1_000_000.0 / sps)
        got = d["cost_per_million_steps_usd"]
        if want == 0 or abs(got - want) / max(abs(want), 1e-12) > 0.01:
            fail.append("cost_per_million_steps_usd=%r but recomputes to %r" % (got, want))
        print("steps_per_second=%r machine_usd_per_hour=%r cost_per_million_steps=%r recomputed=%r"
              % (sps, mch, got, round(want, 6)))
    except KeyError as e:
        fail.append("missing required field for the derivation: %s" % e)
    blob = json.dumps(d).lower()
    for bad in ("rapier", "game engine", "open source", "80-90%", "80–90%"):
        if bad in blob: fail.append("forbidden phrase in artifact: %r" % bad)
    print("MEASURED_fields=%d" % measured)
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    steps_per_second=1428.6 machine_usd_per_hour=0.42 cost_per_million_steps=0.0817 recomputed=0.08167
    MEASURED_fields=4
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  MEASURED_fields >= 3

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because it re-derives the
headline cost figure from the model's own inputs, refuses any unlabelled number, and requires that
each measured number's log file actually contain the command that produced it — so a fabricated
measurement fails on the log, not on the prose.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — run avian-physics-bench, take steady-state ms/step, build the model
   -> if still failing, MANDATORY approach change. Re-running the same bench with different
      arguments is NOT an approach change; switching to instance-capacity and pricing per
      resident entity rather than per physics step is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with MEASURED_fields unchanged and the verifier
                  failure list shrinking by < 2 entries
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: neither benchmark binary completes a run (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.43/unit_economics.json`

A reader finds: `generated_at`, `hardware` (CPU, GPU, RAM, OS), `commit`, then the model fields —
`steps_per_second_measured`, `entities_at_measurement`, `scenario`, `machine_cost_per_hour_usd`,
`cost_per_million_steps_usd`, `simulated_seconds_per_core_hour`, `gross_margin_floor_multiple` —
each with its `_kind` and, where measured, `_command` and `_log`. Alongside it,
`docs/PROMPTS/artifacts/B2/G1.43/logs/*.log` — the raw benchmark transcripts, and
`docs/PROMPTS/artifacts/B2/G1.43/assumptions.md`, one line per `TARGET` value naming who must
confirm it.

## 9. Definition of NOT done

- The model is complete but `steps_per_second_measured` is derived from the 5,406 FPS figure in
  `docs/development/BENCHMARK_VS_ENGINE_AUDIT.md` rather than from a run performed for this item.
- The priced unit is `simulation_time_s` from the clock, so the model prices reported time that the
  step-drop at `eustress/crates/common/src/simulation/clock.rs:100-102` never actually simulated.
- Cloud instance rates are entered as `MEASURED` because they were copied from a vendor page.
- The benchmark is run at a reduced step count so it finishes faster. That is a measurement change.
- Gross margin is computed against a price. There are no prices in this item; margin appears only
  as `gross_margin_floor_multiple`, which `G1.44` consumes.
- The model quotes 2.10M entities as achievable. That figure is a `CONFIG_DEFAULT` from
  `docs/AUDIT/05_SPACE_STREAMING.md:23` and pricing against it overstates capacity by an unknown
  factor.

---

---
id: G1.44
title: Three-part pricing architecture with a COGS-anchored usage floor
workload: W2
workload_secondary: [W5]
phase: G1
depends_on: [G1.42, G1.43]
blocks: [G1.45, G1.54]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.44/pricing_architecture.json
escalation: >
  If any usage-priced SKU cannot clear a 3.0x gross-margin multiple over the G1.43 cost per unit at
  any price a buyer would plausibly pay, STALL. That is a product-shape finding, not a pricing
  finding, and the human must decide whether to change the delivery model before a number is
  published.
status: DRAFT
notes: >
  This item produces price BANDS and the reasoning behind them. Setting the final number is a human
  decision under 00_MASTER_PROTOCOL.md section 6, so the artifact ends in a decision packet with
  named options.
---

## 1. Objective

Every SKU on the `G1.42` sheet carries a price band, an explicit pricing basis, and an expansion
mechanism, where every usage-priced band's floor clears a stated gross-margin multiple over the
measured cost from `G1.43`. The final price remains a human decision, and the artifact ends in a
decision packet that names the options rather than choosing among them.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0 (`LICENSE`), dual-licensed against `LICENSE-COMMERCIAL.md`; say source-available.
Physics is Avian. Slint is Rust. Units are meter-native.

**The three prerequisite artifacts.**
- `docs/PROMPTS/artifacts/B2/G1.40/revenue_rail_ledger.json` — capability states.
- `docs/PROMPTS/artifacts/B2/G1.42/sku_sheet.json` — the SKUs, all with `price: null`.
- `docs/PROMPTS/artifacts/B2/G1.43/unit_economics.json` — `cost_per_million_steps_usd` and
  `gross_margin_floor_multiple`.

**What the licence already fixes about pricing shape.** `LICENSE-COMMERCIAL.md` states that
commercial licences are "negotiated per organization", that typical terms include "a perpetual
grant for a specified version range, priority support, and optional indemnification", and — this is
the load-bearing sentence — that pricing "scales with the scope of rights granted, not with your
revenue from products the Shield license already permits". A revenue-share or royalty model on
customer output therefore contradicts the licence text and is out of bounds for this item.

**What the Shield licence gives away for free, which caps what a seat price can capture.** Per
`LICENSE-COMMERCIAL.md`, at no cost and forever: building, shipping, and selling end products made
*with* Eustress; internal use at a company of any size including production; modifying and forking
for your own products; academic, research, evaluation, and personal use. A buyer who only wants to
*use* the substrate owes nothing. The commercial licence sells **rights** — competition rights,
warranties, indemnification, support SLAs, a perpetual grant — not access.

**What already carries a published consumer price, and why it does not transfer.**
`docs/monetization/CURRENCY.md:44-48` prices Bliss packs at $0.99 to $49.99 against a Steam 30% cut
(`:52-58`). `docs/monetization/SUBSCRIPTIONS.md:8` is "Status: Pre-Release Design". Neither prices
simulation, CAD, kernel-law validation, or hosted compute. Do not import those numbers.

**Marketplace reality.** `docs/AUDIT/09_ECONOMY.md` records the Bliss balance ledger at 0% — no
currency can move — the marketplace at roughly 40%, Steam IAP at 5%, refunds at 0%, tax and VAT at
0%, fraud detection at 0%, and the payout ledger at 10%. A marketplace take rate can therefore be
*designed* in this item, but it cannot be *charged*, and the artifact must say so in the SKU's
`chargeable_today` field, which it inherits from `G1.42`.

**Regulatory boundary you must not cross.** `docs/AUDIT/09_ECONOMY.md` P4 records that the Bliss
dual-nature carries regulatory arbitrage risk needing legal counsel before public launch, and
`00_MASTER_PROTOCOL.md` §6 makes any Bliss ledger design touching transferability or cash-out a
human-only decision. Price the marketplace take rate as a percentage of a transaction; do not
design token mechanics.

**Expansion metric grounding.** The one telemetry stream that is live end to end is
`eustress/crates/engine/src/usage_telemetry.rs`. Its `ClickEvent` (`:41-52`) carries exactly five
fields — `ts`, `tool`, `mode`, `disc`, `wired` — and what crosses the network at session end is an
**aggregate**, not events: `install_id`, `app_version`, `mode`, `counts` (`:211-214`). An expansion
metric defined over any field not in that list is not computable today.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.44/` — create it; write the architecture and the decision packet

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source
- `LICENSE`, `LICENSE-COMMERCIAL.md`, `docs/monetization/**`
- `docs/PROMPTS/artifacts/B2/G1.40/**`, `.../G1.42/**`, `.../G1.43/**` — read-only inputs

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Lowering
  `gross_margin_floor_multiple` in the `G1.43` artifact, or reclassifying a usage SKU as a
  flat-fee SKU to dodge the margin check, are measurement changes.
- The margin floor multiple used by the verifier is **3.0** and is fixed by this prompt. It is not
  read from `G1.43`, so it cannot be tuned.
- Prices are **bands**, `[low, high]`, both non-null, `low <= high`. No single number.
- Do not design a royalty or revenue share on customer output; `LICENSE-COMMERCIAL.md` forbids it.
- Do not design token mechanics, transferability, or cash-out. Human-only.
- The artifact must end in a decision packet naming exactly the options the human chooses among.

## 5. Exit criterion

### Criterion

`pricing_architecture.json` covers **every** SKU key present in the `G1.42` sheet; every SKU has a
`price_band` of two numbers with `low <= high`; **every** SKU whose `pricing_basis` contains the
substring `hour` or `step` has `price_band[0] >= 3.0 *` its `cogs_per_unit_usd`, where
`cogs_per_unit_usd` is present and non-zero; the artifact defines an `expansion_metric` whose
`fields` are a subset of the eight fields that exist in the live telemetry payload; and a
`decision_packet` lists **at least 2 and at most 4** named options each with a `consequence`.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, sys
    A = "docs/PROMPTS/artifacts/B2/G1.44/pricing_architecture.json"
    S = "docs/PROMPTS/artifacts/B2/G1.42/sku_sheet.json"
    TELEMETRY_FIELDS = {"ts", "tool", "mode", "disc", "wired",
                        "install_id", "app_version", "counts"}
    FLOOR = 3.0
    sku_keys = {s["key"] for s in json.load(open(S, encoding="utf-8"))["skus"]}
    d = json.load(open(A, encoding="utf-8"))
    priced = {p["key"]: p for p in d["priced_skus"]}
    fail = []
    missing = sku_keys - set(priced)
    if missing: fail.append("SKUs not priced: %r" % sorted(missing))
    extra = set(priced) - sku_keys
    if extra: fail.append("priced SKUs not on the G1.42 sheet: %r" % sorted(extra))
    checked = 0
    for k, p in priced.items():
        b = p.get("price_band")
        if not (isinstance(b, list) and len(b) == 2 and all(isinstance(x, (int, float)) for x in b)):
            fail.append("%s: price_band must be [low, high] numbers, got %r" % (k, b)); continue
        if b[0] > b[1]: fail.append("%s: price_band low > high" % k)
        basis = str(p.get("pricing_basis", "")).lower()
        if "hour" in basis or "step" in basis:
            c = p.get("cogs_per_unit_usd")
            if not isinstance(c, (int, float)) or c <= 0:
                fail.append("%s: usage-priced but cogs_per_unit_usd missing/zero: %r" % (k, c))
            elif b[0] < FLOOR * c:
                fail.append("%s: floor %r < %rx cogs %r" % (k, b[0], FLOOR, c))
            else:
                checked += 1
    em = d.get("expansion_metric", {})
    ef = set(em.get("fields", []))
    if not ef: fail.append("expansion_metric.fields is empty")
    bad = ef - TELEMETRY_FIELDS
    if bad: fail.append("expansion_metric uses fields not in live telemetry: %r" % sorted(bad))
    opts = d.get("decision_packet", {}).get("options", [])
    if not (2 <= len(opts) <= 4): fail.append("decision_packet needs 2-4 options, got %d" % len(opts))
    for o in opts:
        if not str(o.get("consequence", "")).strip():
            fail.append("decision option %r has no consequence" % o.get("name"))
    blob = json.dumps(d).lower()
    for w in ("royalty", "revenue share", "revenue-share", "open source", "game engine", "rapier"):
        if w in blob: fail.append("forbidden concept or phrase in artifact: %r" % w)
    print("priced=%d usage_skus_margin_checked=%d options=%d" % (len(priced), checked, len(opts)))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    priced=4 usage_skus_margin_checked=1 options=3
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  priced equals the SKU count in the G1.42 sheet

## 6. Critic gate

`critic_gate: []`. The mechanical criterion is unusually tight: the margin multiple is hard-coded in
the verifier rather than read from an artifact the agent can edit; the SKU set must match the
upstream sheet exactly in both directions; the expansion metric is checked against the eight fields
that actually exist in the shipped telemetry payload; and the forbidden-concept scan blocks the
royalty model the licence text rules out.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — price each SKU from its delivery cost and the rights it grants
   -> if still failing, MANDATORY approach change. Adjusting band endpoints is NOT an approach
      change; switching the usage SKU from per-step to per-resident-entity-day, or moving it
      behind a committed-capacity floor, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: a usage SKU cannot clear 3.0x at any plausible buyer price (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.44/pricing_architecture.json`

A reader finds: `generated_at`, the three input artifact paths, `priced_skus` (each with `key`,
`pricing_basis`, `price_band`, `cogs_per_unit_usd` where applicable, `rights_granted`,
`what_expands`), `expansion_metric` (with `name`, `formula`, `fields`, `computable_today`), a
`marketplace` block with `take_rate_pct` and an explicit `chargeable_today: false`, and a
`decision_packet` with two to four named options each carrying a `consequence` and a
`recommendation` field naming one of them. Alongside it,
`docs/PROMPTS/artifacts/B2/G1.44/decision_packet.md` — the same packet on one screen for the human.

## 9. Definition of NOT done

- Every SKU is priced but the usage SKU's floor sits under 3.0x the measured cost, so the first
  large customer is loss-making at exactly the moment volume arrives.
- The marketplace take rate is presented as revenue. The Bliss balance ledger is at 0%; the rate is
  a design, and the artifact must mark it `chargeable_today: false`.
- The expansion metric is monthly active seats. Seats are not in the telemetry payload; the eight
  available fields are `ts`, `tool`, `mode`, `disc`, `wired`, `install_id`, `app_version`, `counts`.
- A royalty on customer output appears, contradicting `LICENSE-COMMERCIAL.md`.
- Consumer prices from `docs/monetization/CURRENCY.md` are carried into an enterprise SKU because
  a number was needed.
- The decision packet asks the human to "review pricing" instead of choosing among named options
  with stated consequences.
- A single price is given instead of a band, which removes the negotiating room the licence's
  scope-of-rights model depends on.

---

---
id: G1.45
title: ARR milestone ladder with per-step capability and organisational unlocks
workload: W2
workload_secondary: [W6]
phase: G1
depends_on: [G1.42, G1.43, G1.44]
blocks: [G1.54]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.45/arr_ladder.json
escalation: >
  If the first milestone (first paying customer) cannot be reached without a capability the G1.40
  ledger marks absent, STALL immediately. The ladder is then not a plan but a wish list, and the
  human must re-scope before any later rung is designed.
status: DRAFT
notes: >
  Five rungs, fixed: first_paying_customer, arr_1m, arr_10m, arr_100m, arr_1b. Each rung must name
  what breaks at that scale, not just what is sold.
---

## 1. Objective

A five-rung revenue ladder exists in which each rung names its account count, average contract
value, the specific capability that must exist to reach it (each cited to a repository path), the
organisational unlock, and the single binding constraint that stops the previous rung from simply
continuing. The arithmetic of every rung is self-consistent and re-derivable.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0; say source-available. Physics is Avian. Slint is Rust. Units are meter-native.

**Inputs you must read.** `docs/PROMPTS/artifacts/B2/G1.40/revenue_rail_ledger.json`,
`docs/PROMPTS/artifacts/B2/G1.42/sku_sheet.json`,
`docs/PROMPTS/artifacts/B2/G1.43/unit_economics.json`,
`docs/PROMPTS/artifacts/B2/G1.44/pricing_architecture.json`.

**The fixed rung keys** — the verifier requires exactly these five, in this order:
`first_paying_customer`, `arr_1m`, `arr_10m`, `arr_100m`, `arr_1b`.

**Starting position, stated plainly.** Revenue today is zero. `docs/AUDIT/09_ECONOMY.md` records
the Bliss balance ledger at 0%, Steam IAP at 5%, Stripe Connect effectively 0% gated on KYC,
subscriptions backend at 0%, refunds 0%, tax and VAT 0%, fraud detection 0%, payout ledger 10%.
`LAUNCH_PLAN.md` Stream 2 records the Nevada LLC as forming and Stream 3 banking as blocked on the
EIN. `docs/launch/PUBLIC_ALPHA_CHECKLIST.md` still carries `releases.eustress.dev` R2 as an
unchecked `[BLOCKER]`. Windows authenticode and macOS notarisation are both 0%
(`docs/AUDIT/12_INFRASTRUCTURE.md`).

**Known capability walls the ladder must place somewhere.** Each of these is a real, cited gap and
at least four of them must appear as a rung's `capability_unlocks` entry:
- CI proves nothing: `.github/workflows/ci.yml` runs `cargo deny`, a naga check that skips
  preprocessed shaders, and a `cargo tree` grep; `.github/workflows/linux-engine.yml` runs
  `cargo check --package eustress-engine`. No `cargo test`, no clippy, no workspace build. There
  are 2,061 `#[test]` functions in `eustress/crates/` and none run in CI.
- The agent has no headless eyes: `eustress/crates/engine/src/bin/headless.rs` is `MinimalPlugins`
  plus `ScheduleRunnerPlugin`, so `ai_camera_capture` and `capture_viewport` need a desktop
  session. `docs/architecture/HEADLESS_RUNTIME.md:269` marks the `--render gpu` tier P6 as new.
- Determinism is asserted, never verified: `eustress/crates/common/src/physics/determinism.rs` is
  57 lines holding a `GlobalRngSeed` and nothing else; the byte-identical gate at
  `docs/architecture/HEADLESS_RUNTIME.md:294` has never been run.
- One file carries the whole studio: `eustress/crates/engine/src/ui/slint_ui.rs` is 23,103 lines
  and every panel's drain logic funnels through it.
- Telemetry has no persistence plan: `docs/AUDIT/10_TELEMETRY.md` records roughly 85M messages per
  second in-process which at 1 KB average and 7-day retention is 73 TB/week, with per-topic
  sampling undefined.
- Foundation-model dispatch is absent: `docs/AUDIT/07_AI_PLATFORM.md` records
  `FoundationModelDispatcher` fully absent, `spatial-llm` a stub with no model calls, and MCP
  `SubscribeTopic` absent.
- The money rail is serial: Bliss ledger 0% blocks debits; Stripe Connect is gated on KYC; payouts
  on banking; banking on an EIN; the EIN on an LLC still forming.

**The organisational constraint.** Solo founder, Windows, 10–15 minute serialized builds, one build
at a time, and the desktop engine is not built in CI (`docs/AUDIT/12_INFRASTRUCTURE.md` Feature 1),
so the founder is the entire regression surface. Any rung above the first must state what stops
being true about that sentence.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.45/` — create it; write the ladder and its narrative

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source
- `LAUNCH_PLAN.md`, `docs/AUDIT/**`
- `docs/PROMPTS/artifacts/B2/G1.40/**`, `.../G1.42/**`, `.../G1.43/**`, `.../G1.44/**`

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Renaming a rung, adding a
  sixth rung, or moving a wall to a later rung to make an earlier one look reachable are all
  measurement changes.
- Every `capability_unlocks` entry carries an `evidence_path` that exists. A rung whose unlocks are
  described only in prose fails.
- Every rung states `binding_constraint` — the one thing that stops the previous rung from simply
  scaling — in a single sentence.
- Do not invent a market size. The ladder is expressed in accounts and ACV, both of which must be
  consistent with the `G1.44` price bands.
- Do not describe Eustress as open source.

## 5. Exit criterion

### Criterion

`arr_ladder.json` contains exactly the five fixed rung keys in order; for each rung
`abs(arr_usd - accounts * acv_usd) / arr_usd <= 0.05`; `arr_usd` and `accounts` are strictly
increasing across rungs; every rung has a non-empty `binding_constraint` and at least one
`capability_unlocks` entry whose `evidence_path` exists; every rung has an `org_unlock` naming a
`headcount` and a `first_role`; and **at least 4 distinct** `evidence_path` values across the whole
ladder point at the known capability walls (each path must exist).

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, os, sys
    P = "docs/PROMPTS/artifacts/B2/G1.45/arr_ladder.json"
    ORDER = ["first_paying_customer", "arr_1m", "arr_10m", "arr_100m", "arr_1b"]
    d = json.load(open(P, encoding="utf-8"))
    rungs = d["rungs"]; fail = []
    if [r["key"] for r in rungs] != ORDER:
        fail.append("rung keys/order must be %r, got %r" % (ORDER, [r.get("key") for r in rungs]))
    prev_arr = prev_acc = -1
    paths = set()
    for r in rungs:
        arr, acc, acv = r.get("arr_usd"), r.get("accounts"), r.get("acv_usd")
        if not all(isinstance(x, (int, float)) for x in (arr, acc, acv)) or arr <= 0:
            fail.append("%s: arr_usd/accounts/acv_usd must be positive numbers" % r.get("key")); continue
        if abs(arr - acc * acv) / arr > 0.05:
            fail.append("%s: arr %r != accounts %r * acv %r (>5%%)" % (r["key"], arr, acc, acv))
        if arr <= prev_arr: fail.append("%s: arr_usd not strictly increasing" % r["key"])
        if acc <= prev_acc: fail.append("%s: accounts not strictly increasing" % r["key"])
        prev_arr, prev_acc = arr, acc
        if not str(r.get("binding_constraint", "")).strip():
            fail.append("%s: empty binding_constraint" % r["key"])
        ul = r.get("capability_unlocks", [])
        if not ul: fail.append("%s: no capability_unlocks" % r["key"])
        for u in ul:
            ep = u.get("evidence_path")
            if not ep or not os.path.exists(ep):
                fail.append("%s: unlock evidence_path missing: %r" % (r["key"], ep))
            else: paths.add(ep)
        org = r.get("org_unlock", {})
        if not isinstance(org.get("headcount"), int) or not str(org.get("first_role", "")).strip():
            fail.append("%s: org_unlock needs integer headcount and non-empty first_role" % r["key"])
    if len(paths) < 4:
        fail.append("only %d distinct existing evidence paths across the ladder, need >= 4" % len(paths))
    blob = json.dumps(d).lower()
    for bad in ("open source", "game engine", "rapier"):
        if bad in blob: fail.append("forbidden phrase in artifact: %r" % bad)
    print("rungs=%d distinct_evidence_paths=%d" % (len(rungs), len(paths)))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    rungs=5 distinct_evidence_paths=7
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  rungs=5  AND  distinct_evidence_paths >= 4

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because the rung set is fixed,
the arithmetic is re-derived, monotonicity is enforced, and every capability unlock must resolve to
a file on disk — which is what stops the ladder from becoming a narrative about markets.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — build the ladder top-down from ACV bands in G1.44
   -> if still failing, MANDATORY approach change. Adjusting account counts is NOT an approach
      change; switching to a bottom-up construction (start from the capability walls, place each
      wall at the revenue level where it first binds, then derive accounts and ACV) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: first_paying_customer requires a capability the G1.40 ledger marks absent
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.45/arr_ladder.json`

A reader finds: `generated_at`, the four input artifact paths, and `rungs` — five objects, each
with `key`, `arr_usd`, `accounts`, `acv_usd`, `sku_mix` (keys from the `G1.42` sheet),
`capability_unlocks` (each with `name`, `evidence_path`, `why_it_binds`), `org_unlock` (with
`headcount`, `first_role`, `what_stops_being_true`), and `binding_constraint`. Alongside it,
`docs/PROMPTS/artifacts/B2/G1.45/ladder.md` — one page a reader can follow without the JSON.

## 9. Definition of NOT done

- The ladder is arithmetically clean but every `capability_unlocks` entry points at a document
  rather than at the code or workflow file that embodies the gap.
- `first_paying_customer` is placed behind Stripe, KYC, and a signed binary, when the SKU sheet
  already contains an offer deliverable without any of them.
- ACVs are chosen to make the arithmetic work rather than being drawn from the `G1.44` bands, so
  the ladder silently prices a SKU three times higher than the pricing item allows.
- Each rung's `org_unlock` names a headcount but not what stops being true about the founder being
  the entire regression surface.
- `binding_constraint` is written as "scale the go-to-market", which is a category, not a
  constraint.
- The $1B rung asserts a market size. The ladder's job is to name the capability and organisational
  unlocks, not to size a market.

---

---
id: G1.46
title: Procurement-readiness matrix for a deep-tech enterprise buyer
workload: W2
workload_secondary: [W3]
phase: G1
depends_on: [G1.41, G1.42, G0.05]
blocks: [G2.40]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.46/procurement_readiness.json
escalation: >
  If more than 6 of the 24 questions must be answered "no" for a SKU marked chargeable_today in the
  G1.42 sheet, STALL. That SKU is not sellable to an enterprise buyer regardless of its technical
  backing, and the human must decide whether to change the buyer or change the SKU.
status: DRAFT
notes: >
  The 24 questions are fixed by this prompt and re-imposed by the verifier so the matrix cannot be
  trimmed to look healthier. This item establishes a baseline count of "no" answers; later items
  reduce it.
---

## 1. Objective

A matrix exists answering the twenty-four questions an enterprise security, legal, and procurement
review actually asks, each answered `yes`, `partial`, or `no` with a repository path as evidence,
and each `no` carrying a remediation with a cost in builds and days. The count of `no` answers is
the baseline number every later trust item is measured against.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0 (`LICENSE`), dual-licensed against `LICENSE-COMMERCIAL.md`; say source-available.
Physics is Avian. Slint is Rust. Units are meter-native.

**Inputs you must read.** `docs/PROMPTS/artifacts/B2/G1.41/vertical_selection.json` (the selected
vertical determines who the buyer is) and `docs/PROMPTS/artifacts/B2/G1.42/sku_sheet.json` (what
they are buying).

**The twenty-four fixed question keys.** The verifier requires exactly these, no more, no fewer:

```
q01_license_osi_approved          q13_incident_response_plan
q02_license_redistribution_terms  q14_uptime_sla_offered
q03_inbound_contributor_ip        q15_support_response_commitment
q04_third_party_dependency_sbom   q16_data_residency_control
q05_dependency_vuln_scanning      q17_telemetry_opt_out
q06_binary_code_signing_windows   q18_pii_handling_documented
q07_binary_notarisation_macos     q19_gdpr_dpa_available
q08_ci_runs_test_suite            q20_subprocessor_list
q09_reproducible_build            q21_source_escrow_available
q10_soc2_or_iso27001              q22_business_continuity_single_founder
q11_pen_test_report               q23_export_control_classification
q12_secrets_management            q24_insurance_and_indemnification
```

**Verified state you already have, which answers several of these before you start.**

- `q01`: PolyForm Shield 1.0.0 is **not** OSI-approved. `LICENSE` is the evidence.
- `q02`: `LICENSE-COMMERCIAL.md` defines when redistribution requires a commercial licence.
- `q03`: there is **no** `CONTRIBUTING.md` and no CLA or DCO in the repository; inbound IP is
  undefined, while `README.md:153` invites PRs and the section above it promises cash-outable Bliss.
- `q05`: `.github/workflows/ci.yml` runs `cargo deny --config ../deny.toml check advisories bans
  sources`. That is real dependency scanning and is the only genuine security gate in CI.
- `q06` and `q07`: Windows authenticode 0%, macOS notarisation 0%
  (`docs/AUDIT/12_INFRASTRUCTURE.md`); `LAUNCH_PLAN.md` lists notarisation as a Critical P0.
- `q08`: CI runs no `cargo test`. `.github/workflows/linux-engine.yml` runs a single
  `cargo check --package eustress-engine`; `.github/workflows/release.yml` runs
  `cargo build --release --package eustress-engine`. There are 2,061 `#[test]` functions in
  `eustress/crates/` and none execute in CI.
- `q09`: the determinism gate at `docs/architecture/HEADLESS_RUNTIME.md:294` (byte-identical
  recordings across two runs) is written and has never been run;
  `eustress/crates/common/src/physics/determinism.rs` is 57 lines holding only a seed resource.
- `q10`: no SOC 2, no ISO 27001. `docs/AUDIT/12_INFRASTRUCTURE.md` records Vault not deployed and
  the Consul directory absent; `infrastructure/forge/` contains only `README.md`, `nomad`,
  `scripts`, `terraform`.
- `q12`: Vault is referenced but not deployed (`docs/AUDIT/12_INFRASTRUCTURE.md`).
- `q13`, `q14`, `q15`: no Prometheus, no Grafana, no alerting, no DR runbooks, no on-call.
- `q17`: telemetry is opt-out-able in the shipped build —
  `eustress/crates/engine/src/usage_telemetry.rs` gates everything on an `enabled` flag mirroring
  `EditorSettings.usage_telemetry_enabled`, records only aggregates over the network, and shows a
  first-run privacy notice. This is a genuine `yes`.
- `q22`: solo founder; `docs/AUDIT/12_INFRASTRUCTURE.md` Feature 1 records that the desktop engine
  is not built in CI, so the founder is the entire regression surface.
- Legal documents that exist and may serve as evidence for the data questions: `docs/legal/GDPR.md`,
  `docs/legal/CCPA.md`, `docs/legal/COPPA.md`, `docs/legal/DMCA.md`, `docs/legal/CSAM.md`,
  `docs/legal/TIDA.md`.

**Do not confuse a written policy with an implemented control.** A `docs/legal/` file is admissible
evidence for `partial`, never for `yes`, unless you can also cite the code or configuration that
enforces it.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.46/` — create it; write the matrix and the remediation plan

### Out of scope — do not edit
- Anything under `.github/workflows/` — in particular, do not add a test job to make `q08` a `yes`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source
- `docs/legal/**`, `LICENSE`, `LICENSE-COMMERCIAL.md`
- `docs/PROMPTS/artifacts/B2/G1.41/**` and `.../G1.42/**` — read-only inputs

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Dropping a question,
  answering `partial` where the honest answer is `no`, or citing a design document as evidence of an
  implemented control are all measurement changes.
- A `yes` requires an `evidence_path` that exists **and** is code, configuration, or a workflow —
  not a plan.
- Every `no` carries `remediation` with `builds_required` (integer) and `days_required` (number).
- Do not describe Eustress as open source. `q01` is a `no` and the artifact must say so plainly.

## 5. Exit criterion

### Criterion

`procurement_readiness.json` answers **exactly** the 24 fixed question keys; every answer is one of
`yes`, `partial`, `no`; every `yes` and every `partial` carries an `evidence_path` that exists;
every `no` carries a `remediation` with integer `builds_required` and numeric `days_required`; and
the artifact's own `summary.n_no` equals the count the verifier computes.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, os, sys
    P = "docs/PROMPTS/artifacts/B2/G1.46/procurement_readiness.json"
    KEYS = ["q01_license_osi_approved","q02_license_redistribution_terms",
      "q03_inbound_contributor_ip","q04_third_party_dependency_sbom",
      "q05_dependency_vuln_scanning","q06_binary_code_signing_windows",
      "q07_binary_notarisation_macos","q08_ci_runs_test_suite","q09_reproducible_build",
      "q10_soc2_or_iso27001","q11_pen_test_report","q12_secrets_management",
      "q13_incident_response_plan","q14_uptime_sla_offered","q15_support_response_commitment",
      "q16_data_residency_control","q17_telemetry_opt_out","q18_pii_handling_documented",
      "q19_gdpr_dpa_available","q20_subprocessor_list","q21_source_escrow_available",
      "q22_business_continuity_single_founder","q23_export_control_classification",
      "q24_insurance_and_indemnification"]
    d = json.load(open(P, encoding="utf-8"))
    ans = {a["key"]: a for a in d["answers"]}
    fail = []
    if set(ans) != set(KEYS):
        fail.append("key set mismatch; missing=%r extra=%r"
                    % (sorted(set(KEYS) - set(ans)), sorted(set(ans) - set(KEYS))))
    n_no = 0
    for k in KEYS:
        a = ans.get(k)
        if a is None: continue
        v = a.get("answer")
        if v not in ("yes", "partial", "no"):
            fail.append("%s: answer must be yes/partial/no, got %r" % (k, v)); continue
        if v in ("yes", "partial"):
            ep = a.get("evidence_path")
            if not ep or not os.path.exists(ep):
                fail.append("%s: %s requires an existing evidence_path, got %r" % (k, v, ep))
        else:
            n_no += 1
            r = a.get("remediation", {})
            if not isinstance(r.get("builds_required"), int):
                fail.append("%s: remediation.builds_required must be an integer" % k)
            if not isinstance(r.get("days_required"), (int, float)):
                fail.append("%s: remediation.days_required must be a number" % k)
    if d.get("summary", {}).get("n_no") != n_no:
        fail.append("summary.n_no=%r but computed %d" % (d.get("summary", {}).get("n_no"), n_no))
    blob = json.dumps(d).lower()
    for bad in ("game engine", "rapier"):
        if bad in blob: fail.append("forbidden phrase in artifact: %r" % bad)
    print("questions=%d n_no=%d n_partial=%d n_yes=%d"
          % (len(ans), n_no,
             sum(1 for a in ans.values() if a.get("answer") == "partial"),
             sum(1 for a in ans.values() if a.get("answer") == "yes")))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    questions=24 n_no=13 n_partial=6 n_yes=5
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  questions=24. The printed `n_no` is the baseline
    this item establishes; it is recorded, not compared against a threshold here.

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because the question set is
fixed by the prompt and cannot be trimmed, every affirmative answer must resolve to a file on disk,
every negative answer must carry a costed remediation, and the artifact's own summary count is
re-derived by the verifier so it cannot be understated.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — answer each question from the repository state given in section 2
   -> if still failing, MANDATORY approach change. Re-answering questions is NOT an approach
      change; switching to an evidence-first sweep (enumerate every workflow file, every legal
      doc, and every settings surface, then map each to the questions it answers) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: more than 6 "no" answers block a SKU marked chargeable_today (front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.46/procurement_readiness.json`

A reader finds: `generated_at`, `vertical`, `buyer_profile`, `answers` (24 objects, each with
`key`, `question` in plain English, `answer`, `evidence_path`, `evidence_note`, and for negatives a
`remediation` with `what`, `builds_required`, `days_required`, `blocked_by`), and a `summary` with
`n_yes`, `n_partial`, `n_no`, and `blocking_for_skus`. Alongside it,
`docs/PROMPTS/artifacts/B2/G1.46/remediation_order.md` — the negatives sorted by days per SKU
unblocked, which is the queue a later item works down.

## 9. Definition of NOT done

- `q01` is answered `partial` because a commercial licence exists. The question is whether the
  licence is OSI-approved. It is not; the answer is `no`.
- `q08` is answered `yes` because 2,061 tests exist. The question is whether **CI runs** them.
- `q17` is answered `no` out of caution when the shipped code genuinely implements opt-out and a
  first-run notice. Understating is as much a measurement error as overstating.
- `q19` is answered `yes` citing `docs/legal/GDPR.md`. A policy document is `partial` at best
  without the enforcing code or configuration.
- A question is renamed or merged so the count comes out lower.
- CI is modified to add a test job so `q08` flips to `yes`. Workflows are out of scope and this is
  the archetypal gate-gaming failure.
- Remediations are written without `builds_required`, so the queue cannot be ordered against the
  10–15 minute build constraint that governs everything else in this program.

---

---
id: G1.47
title: Customer-success health score computable from the live telemetry payload
workload: W2
workload_secondary: [W6]
phase: G1
depends_on: [G1.41, G1.42]
blocks: [G6.40]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.47/health_score.json
escalation: >
  If the health score cannot discriminate between the three fixture accounts by at least 20 points,
  STALL. A metric that assigns similar scores to a thriving account and a dying one is worse than
  none, because it will be trusted in a renewal conversation.
status: DRAFT
notes: >
  Tier M for headroom, but this item may consume zero builds: the scorer is Python and the fixture
  is JSONL. Builds are only needed if the agent chooses to generate a real outbox file by running
  the studio, which is optional and not required by the exit criterion.
---

## 1. Objective

A customer-health score exists that is computed only from fields the shipped telemetry pipeline
actually emits, is implemented as a runnable scorer, and demonstrably separates a healthy account
from an at-risk one on a fixture whose format matches the engine's own outbox. A renewal
conversation can be prepared from it without any field that does not exist.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0; say source-available. Physics is Avian. Slint is Rust. Units are meter-native.

**Why this is hard here.** The product's value is proven over months of simulation work, not in a
trial week, so a renewal decision has to be predicted long before it happens. The only live
instrumentation is a click-usage pipeline; there is no product analytics stack.

**Exactly what the telemetry pipeline emits.** `eustress/crates/engine/src/usage_telemetry.rs`:

- `ClickEvent` (`:41-52`) — five fields: `ts` (unix epoch ms, UTC), `tool` (a manifest tool id such
  as `math:equation_solver`), `mode` (mode id such as `student`), `disc` (submode or discipline id,
  empty if none), `wired` (true when the click actually did something, false for a deliberate
  "dream" button).
- Events accumulate in memory and are appended as JSONL to a **daily file** under
  `dirs::data_local_dir()/Eustress/telemetry` with an `outbox` subdirectory (`:187`, `:206`).
  Per-click timing never leaves the machine.
- What crosses the network at session end is one **aggregate** per session (`:211-214`):
  `install_id`, `app_version`, `mode`, `counts` (a map of tool id to click count). It is posted to
  `https://api.eustress.dev/api/telemetry/usage` (`:193`), overridable by `EUSTRESS_TELEMETRY_URL`.
- A second endpoint takes explicit user feedback text (`:231`): `install_id`, `tool`, `text`.
- Everything is gated on `UsageTelemetry.enabled`, which mirrors
  `EditorSettings.usage_telemetry_enabled`; when false nothing is recorded, buffered, or written.
- `install_id` is a random per-install id and is explicitly never joined to an account id.

**The consequence you must design around.** There is **no account id** in the payload. A health
score is therefore computed per `install_id` and rolled up to an account only by a mapping the
seller maintains outside the product. Say so in the artifact; do not invent an account field.

**What `wired` gives you that most analytics do not.** `eustress/crates/engine/src/tool_metadata.rs`
carries a `wired: bool` per tool id (`:20`, and the match table from `:148`). Measured 2026-08-06:
**1999** tool ids, of which **30** are `wired: true`. A click on an unwired tool is recorded as
demand, not as usage. A health score that counts unwired clicks as engagement will read a
frustrated account as a thriving one — and the inverse, a rising ratio of unwired clicks, is the
single best leading indicator of churn available in this payload.

**Verification caution from prior work in this repository:** verify telemetry by inspecting the
stored data, never by driving the studio UI. Driving the UI is slow, non-reproducible, and is not
required by this item's exit criterion.

**Build reality.** 10–15 minutes for a full engine build; one build at a time; never kill a build
mid-compile. This item does not require a build.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.47/` — create it; write the score spec, the scorer, and the fixture

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source, including `eustress/crates/engine/src/usage_telemetry.rs`. If a needed field
  does not exist, that is a finding for the artifact, not a licence to add it.
- `infrastructure/telemetry-worker/**` — the deployed Worker is production; do not touch it
- Any real telemetry file under the user's local app data. You may read one if it exists; you may
  not modify, move, or delete it.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Adding a field to the
  fixture that the engine does not emit, or widening the discrimination window below 20 points, are
  measurement changes.
- The scorer reads **only** the eight fields named in §2: `ts`, `tool`, `mode`, `disc`, `wired`,
  `install_id`, `app_version`, `counts`. Any other field name in the scorer fails the item.
- The fixture must contain three accounts with clearly different trajectories: one healthy, one
  at-risk, one dormant. Build them by hand; do not generate noise.
- The score is an integer 0–100, where higher is healthier.
- Do not send anything anywhere. This item computes locally.

## 5. Exit criterion

### Criterion

`docs/PROMPTS/artifacts/B2/G1.47/score_health.py` runs against
`docs/PROMPTS/artifacts/B2/G1.47/fixture.jsonl` and emits a score in `[0, 100]` for each of the
three fixture installs; the healthy install scores **at least 20 points above** the at-risk
install, and the at-risk install scores **at least 20 points above** the dormant install; the
scorer references no field outside the allowed eight; and `health_score.json` records the same
three scores the scorer prints.

### Measurement

Run from the repository root, in Git Bash. Two commands; both must be run.

Command:

    python docs/PROMPTS/artifacts/B2/G1.47/score_health.py \
        --input docs/PROMPTS/artifacts/B2/G1.47/fixture.jsonl \
        --out   docs/PROMPTS/artifacts/B2/G1.47/health_score.json
    echo "SCORER_EXIT=$?"

    python - << 'PYEOF'
    import json, re, sys
    S = "docs/PROMPTS/artifacts/B2/G1.47/score_health.py"
    H = "docs/PROMPTS/artifacts/B2/G1.47/health_score.json"
    ALLOWED = {"ts","tool","mode","disc","wired","install_id","app_version","counts"}
    src = open(S, encoding="utf-8").read()
    fail = []
    used = set(re.findall(r'''(?:\.get\(|\[)\s*["']([a-z_][a-z0-9_]*)["']''', src))
    # only flag identifiers that look like telemetry payload keys
    suspect = {u for u in used if u not in ALLOWED and not u.startswith("_")}
    known_locals = {"scores","accounts","by_install","result","installs","totals","health"}
    bad = sorted(suspect - known_locals)
    if bad: fail.append("scorer reads fields outside the live payload: %r" % bad)
    d = json.load(open(H, encoding="utf-8"))
    sc = d["scores"]
    if len(sc) != 3: fail.append("expected 3 install scores, got %d" % len(sc))
    for k, v in sc.items():
        if not isinstance(v, int) or not (0 <= v <= 100):
            fail.append("%s: score must be an int in [0,100], got %r" % (k, v))
    order = d.get("expected_order")
    if not (isinstance(order, list) and len(order) == 3):
        fail.append("health_score.json must declare expected_order of 3 install ids")
    else:
        a, b, c = (sc.get(order[0]), sc.get(order[1]), sc.get(order[2]))
        if None in (a, b, c): fail.append("expected_order names an install with no score")
        else:
            if a - b < 20: fail.append("healthy-at_risk gap %r < 20" % (a - b))
            if b - c < 20: fail.append("at_risk-dormant gap %r < 20" % (b - c))
            print("scores healthy=%d at_risk=%d dormant=%d gaps=%d,%d" % (a, b, c, a - b, b - c))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    SCORER_EXIT=0
    scores healthy=88 at_risk=54 dormant=11 gaps=34,43
    RESULT: PASS
    EXIT=0

Pass condition:

    SCORER_EXIT=0  AND  EXIT=0  AND  "RESULT: PASS" present  AND  both printed gaps >= 20

## 6. Critic gate

`critic_gate: []`. The mechanical criterion is tight because it runs the scorer rather than reading
a description of it, statically checks the scorer against the exact eight-field payload the engine
emits, and demands a 20-point separation in both directions — so a metric that is merely plausible
cannot pass.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — score on wired-click volume and recency
   -> if still failing, MANDATORY approach change. Re-weighting the same signals is NOT an
      approach change; switching the primary signal to the wired-to-unwired click ratio and its
      trend, or to breadth of distinct wired tool ids per week, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the smaller of the two gaps moves < 3 points
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the two gaps cannot both reach 20 points (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.47/health_score.json`

A reader finds: `generated_at`, `payload_fields_used` (a subset of the eight), `formula` in plain
English, `scores` (install id to integer), `expected_order` (healthy, at-risk, dormant install
ids), `account_mapping_note` (stating that `install_id` is never joined to an account inside the
product, so the seller maintains the mapping externally), and `leading_indicator` naming the
wired-to-unwired ratio and what threshold triggers an intervention. Alongside it:
`docs/PROMPTS/artifacts/B2/G1.47/score_health.py` (the scorer) and
`docs/PROMPTS/artifacts/B2/G1.47/fixture.jsonl` (three hand-built accounts).

## 9. Definition of NOT done

- The score is defined over seats, licences, or account ids. None of those exist in the payload;
  `install_id` is explicitly never joined to an account.
- Unwired clicks are counted as engagement, so a user hammering dream buttons in frustration scores
  as the healthiest account in the fixture.
- The scorer is written but never run, and the JSON scores are typed by hand.
- The fixture is generated randomly, so the three trajectories differ by noise rather than by a
  behaviour a seller would recognise.
- The metric requires session duration or per-click timing. Per-click timing deliberately never
  leaves the machine; only session aggregates cross the network.
- The gaps clear 20 points only because the dormant account has zero events. A dormant account
  must have some activity, or the metric has only proven it can detect an empty file.

---

---
id: G1.48
title: Licence decision-forcing memo with four costed options
workload: W5
workload_secondary: [W2]
phase: G1
depends_on: [G1.40]
blocks: [G0.04, G0.07, G0.10, G0.12, G1.49, G1.50, G1.51, G1.52, G7.12, G7.22, G7.24]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json
escalation: >
  If the agent finds itself about to state which option has been chosen, stop and STALL. Changing
  the licence is a human-only decision under 00_MASTER_PROTOCOL.md section 6; this item produces
  the options and a recommendation, never the decision.
status: DRAFT
notes: >
  This is the single item in pack B2 whose entire purpose is to force an explicit human decision.
  Every ecosystem item downstream of it is blocked on the answer, and the pack says so.
---

## 1. Objective

A memo exists laying out exactly four licence options, each scored against the same six
consequence dimensions with a concrete cost, together with the specific in-repository
contradictions the current licence creates. The human can decide from it in one sitting. The memo
recommends; it does not decide.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Physics is Avian.
Slint is Rust. Units are meter-native.

**The licence as it stands.** `LICENSE` is **PolyForm Shield License 1.0.0**, Required Notice
"Copyright (c) 2026 Eustress LLC". It is explicitly dual-licensed against `LICENSE-COMMERCIAL.md`
("Eustress Commercial License", negotiated per organization, no published price, contact
`licensing@eustress.dev`).

At no cost, PolyForm Shield permits copying, distribution, modification, forks, a patent grant,
internal production use at any company size, and building, shipping, and selling products made
*with* Eustress with no royalty (`LICENSE-COMMERCIAL.md`, "When you do NOT need a commercial
license").

It forbids one thing, maximally. The **Noncompete** clause: any purpose is permitted "except for
providing any product that competes with the software or any product the licensor or any of its
affiliates provides using the software." The **Competition** clause defines competition across
differing interfaces and technical platforms — applications can compete with services, libraries
with plugins, frameworks with development tools — and states that products compete **even when
provided free of charge**. The **New Products** clause freezes an adopter at the versions available
when Eustress LLC enters that adopter's market. **No Other Rights** forbids sublicensing and
transfer.

**What that forecloses, concretely.** A contributor cannot safely build an adjacent tool, because
a free plugin can still be judged competing. Distribution channels that require OSI-approved
licences are closed. Organisations whose open-source programme office blocks non-OSI licences
cannot adopt. Third parties cannot host Eustress as a service.

**The two in-repository contradictions this memo must name.**

1. `docs/marketing/UofA_Center_For_Innovation_Pilot.html` contains the literal string
   `Open source · MIT-friendly licensing` and, separately, `open source. Windows, macOS, Linux.`
   `LICENSE` contradicts both. The asset and the licence cannot both stand, and only the human
   chooses which one changes.
2. `README.md:153` invites contribution — "Install it, find something that bugs you, and open a
   PR" — and the section above it promises a merit ladder ending in cash-outable Bliss, while there
   is **no** `CONTRIBUTING.md` and **no** CLA or DCO in the repository, so inbound contribution
   licensing and IP assignment are undefined. Separately, `docs/AUDIT/09_ECONOMY.md` records the
   Bliss balance ledger at 0%, so the promised reward cannot currently be paid.

**The four options this memo must present** — fixed keys, and the verifier requires exactly these:

- `A_status_quo` — Shield 1.0.0 across the whole repository, unchanged; fix the marketing asset.
- `B_split_licence` — a permissively licensed core (a named crate set) with the studio and
  differentiated subsystems remaining under Shield.
- `C_relicense_permissive` — move the whole repository to an OSI-approved permissive licence and
  monetise elsewhere.
- `D_shield_plus_safe_harbour` — keep Shield and publish a binding written exception defining an
  extension safe harbour, so plugin and tool authors have a stated non-compete boundary.

**The six consequence dimensions** — fixed keys, all six required for every option:

- `contributor_inbound_ip` — what becomes true about who owns a merged PR.
- `ospo_adoption` — whether an enterprise open-source programme office can approve adoption.
- `third_party_hosting_risk` — who may run Eustress as a service, and what that costs the company.
- `commercial_licence_revenue` — what happens to the only non-serialised revenue surface today.
- `ecosystem_tooling` — whether a third party can build and distribute an adjacent tool.
- `reversibility` — how hard the option is to undo once contributors have relied on it.

**Human-only boundary.** `00_MASTER_PROTOCOL.md` §6 makes changing the licence, and any external
publication, human-only. This memo is an internal artifact. It must not be published, and it must
not record a decision.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.48/` — create it; write the memo and its one-screen summary

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `LICENSE` and `LICENSE-COMMERCIAL.md` — **under no circumstances**. Editing either is the single
  most serious scope violation available in this pack.
- `README.md`
- `docs/marketing/**` — the contradiction is reported, not fixed; the fix is a human decision
- All Rust source

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Dropping an option,
  dropping a dimension, or softening the Competition clause's plain meaning are measurement
  changes.
- Every one of the 24 consequence cells must be non-empty and must state a concrete consequence,
  not a sentiment. "Better for community" fails; "an OSPO that blocks non-OSI licences can approve
  adoption without an exception request" passes.
- Every option carries a `reversibility_cost` and a `who_must_sign` field.
- The memo contains `decision: null` and `human_decision_required: true`. An artifact that records
  a chosen option fails the item.
- A `recommendation` field naming exactly one option key, with a one-sentence reason, is required.
  Recommending is not deciding.
- Do not describe Eustress as open source anywhere in the memo except when quoting the marketing
  asset's own words as a contradiction.

## 5. Exit criterion

### Criterion

`licence_decision_memo.json` contains **exactly** the four fixed option keys, each with **all six**
fixed consequence dimensions non-empty, plus `reversibility_cost` and `who_must_sign`; it records
`decision: null` and `human_decision_required: true`; it names a `recommendation` that is one of
the four keys with a non-empty `because`; and its `contradictions` list includes an entry whose
`path` is `docs/marketing/UofA_Center_For_Innovation_Pilot.html` and whose `literal` string is
actually present in that file.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, os, sys
    P = "docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json"
    OPTS = {"A_status_quo","B_split_licence","C_relicense_permissive","D_shield_plus_safe_harbour"}
    DIMS = {"contributor_inbound_ip","ospo_adoption","third_party_hosting_risk",
            "commercial_licence_revenue","ecosystem_tooling","reversibility"}
    d = json.load(open(P, encoding="utf-8"))
    opts = {o["key"]: o for o in d["options"]}
    fail = []
    if set(opts) != OPTS:
        fail.append("option keys must be %r, got %r" % (sorted(OPTS), sorted(opts)))
    cells = 0
    for k, o in opts.items():
        c = o.get("consequences", {})
        if set(c) != DIMS:
            fail.append("%s: dimensions must be %r, got %r" % (k, sorted(DIMS), sorted(c)))
        for dim, text in c.items():
            if not str(text).strip(): fail.append("%s.%s: empty consequence" % (k, dim))
            else: cells += 1
        for req in ("reversibility_cost", "who_must_sign"):
            if not str(o.get(req, "")).strip(): fail.append("%s: missing %s" % (k, req))
    if d.get("decision") is not None: fail.append("decision must be null - this item never decides")
    if d.get("human_decision_required") is not True:
        fail.append("human_decision_required must be true")
    rec = d.get("recommendation", {})
    if rec.get("option") not in OPTS: fail.append("recommendation.option must be one of the four keys")
    if not str(rec.get("because", "")).strip(): fail.append("recommendation.because is empty")
    found = False
    for c in d.get("contradictions", []):
        p, lit = c.get("path"), c.get("literal", "")
        if not p or not os.path.exists(p): fail.append("contradiction path missing: %r" % p); continue
        txt = open(p, encoding="utf-8", errors="replace").read()
        if lit and lit in txt:
            if p == "docs/marketing/UofA_Center_For_Innovation_Pilot.html": found = True
        else:
            fail.append("contradiction literal not found in %s: %r" % (p, lit))
    if not found:
        fail.append("no verified contradiction quoted from docs/marketing/UofA_Center_For_Innovation_Pilot.html")
    print("options=%d cells=%d recommendation=%s decision=%r"
          % (len(opts), cells, rec.get("option"), d.get("decision")))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    options=4 cells=24 recommendation=D_shield_plus_safe_harbour decision=None
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  cells=24  AND  decision=None

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight in a way that matters here: it
enforces that all four options and all six dimensions are present and populated, it verifies the
quoted marketing contradiction against the actual bytes of that file, and it fails the item if the
artifact records a decision — which is the specific failure mode an eager agent produces on this
topic.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — analyse each option against the six dimensions from the licence text
   -> if still failing, MANDATORY approach change. Rewriting cells is NOT an approach change;
      switching to a stakeholder-first analysis (write what each of contributor, OSPO buyer,
      hosting competitor, and the company itself can and cannot do under each option, then fill
      the grid from that) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: the agent is about to record a decision (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`

A reader finds: `generated_at`, `current_licence` (file path and licence name), `options` (four,
each with `key`, `summary`, `consequences` across the six dimensions, `reversibility_cost`,
`who_must_sign`), `contradictions` (each with `path`, `literal`, `why_it_conflicts`),
`blocked_downstream` (the item ids in this pack that cannot complete until the decision is made),
`recommendation` (`option` plus `because`), `decision: null`, and `human_decision_required: true`.
Alongside it, `docs/PROMPTS/artifacts/B2/G1.48/memo.md` — the same content on one screen, formatted
for a human reading it once and deciding.

## 9. Definition of NOT done

- The memo picks an option. Changing the licence is human-only; the verifier fails a non-null
  `decision` and so should the reviewer.
- Only three options are presented because one was judged obviously wrong. The set is fixed at
  four so the human sees the full space, including the one the author dislikes.
- The Competition clause is summarised as "you can't resell the engine", which understates it. It
  reaches goods and services across differing interfaces and technical platforms, and applies even
  when the competing product is free.
- The marketing contradiction is described but not quoted, so a reader cannot confirm it without
  opening the file. The verifier requires the literal string and checks it against the file.
- The memo proposes fixing `docs/marketing/UofA_Center_For_Innovation_Pilot.html` and edits it.
  That file is out of scope; the contradiction is reported, and the human chooses which side
  changes.
- `reversibility` is filled in as "hard to reverse" for every option, which conveys nothing. Each
  cell must state what specifically becomes irreversible and for whom.
- The memo recommends an option without naming who must sign it, so the human still cannot act.

---

---
id: G1.49
title: CONTRIBUTING.md with a verbatim DCO and defined inbound IP
workload: W5
workload_secondary: [W3]
phase: G1
depends_on: [G1.48]
blocks: [G1.50, G1.51]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: CONTRIBUTING.md
escalation: >
  If writing the inbound-IP section requires stating anything about Bliss cash-out, transferability,
  or a contributor's right to build an adjacent commercial product, STALL. Those are human-only
  decisions and the G1.48 memo exists precisely to route them.
status: DRAFT
notes: >
  The artifact here is a repository file rather than an entry under docs/PROMPTS/artifacts, because
  the whole point is that a stranger opening the repository finds it. Note that this item is a
  no-op for anyone who has not read G1.48; the licence framing must be correct before the
  contribution terms are written.
---

## 1. Objective

`CONTRIBUTING.md` exists at the repository root, states inbound contribution terms unambiguously,
carries the Developer Certificate of Origin 1.1 verbatim with a stated sign-off mechanism, and
describes the licence correctly. A person considering their first pull request can determine what
they are granting before they write a line.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. It is **never** called a game engine; 3D rendering and the ECS are
implementation details in service of that goal. Physics is **Avian**, never Rapier. `.slint` files
compile to Rust, so Slint **is** Rust — never frame "Rust-first" as opposed to Slint. Units are
meter-native; studs are a display unit only.

**The licence, stated exactly.** `LICENSE` is **PolyForm Shield License 1.0.0**, Required Notice
"Copyright (c) 2026 Eustress LLC", dual-licensed against `LICENSE-COMMERCIAL.md`. The correct term
is **source-available**. It is **not** OSI-approved and it is **not** open source. Under it, at no
cost and with no royalty: copying, distribution, modification, forks, a patent grant, internal
production use at any company size, and building, shipping, and selling products made *with*
Eustress. The one restriction is the Noncompete clause — you may not provide a product that
competes with Eustress or with a product Eustress LLC provides using it — and the Competition
clause makes that restriction reach across differing interfaces and technical platforms, applying
even when the competing product is free.

**The gap this item closes.** There is **no** `CONTRIBUTING.md` in this repository and no CLA or
DCO. Meanwhile `README.md:153` says, in full: "Eustress is source-available and merit-based; that
is the velocity thesis, not a slogan. Install it, find something that bugs you, and open a PR.
Contribution is the on-ramp to the ladder above; rank and Bliss follow the work." So the project
solicits contributions while inbound IP is undefined.

**Why a DCO and not a CLA.** A Developer Certificate of Origin is a per-commit attestation added
with `git commit -s`, which produces a `Signed-off-by:` trailer. It requires no separate signature
collection, no legal-entity tracking, and no contributor database — which matters because the
project is a solo operation with no legal or administrative capacity. A CLA that assigns copyright
would need counsel and a signature workflow, and neither exists. The DCO is the highest-value,
lowest-cost option and it is unblocked today.

**The DCO text.** Use **Developer Certificate of Origin, Version 1.1**, reproduced verbatim,
including its Copyright and licence notice and its four clauses (a) through (d) and the closing
clause about the sign-off being a public record. It is published at `https://developercertificate.org/`.
Do not paraphrase it, do not renumber it, do not add clauses to it. Project-specific terms go in
their own section, outside the quoted certificate.

**The one thing that must not appear.** `README.md` promises a merit ladder ending in cash-outable
Bliss. `docs/AUDIT/09_ECONOMY.md` records the Bliss balance ledger at 0% — no currency can move —
and `00_MASTER_PROTOCOL.md` §6 makes any Bliss design touching transferability or cash-out a
human-only decision. `CONTRIBUTING.md` must therefore not promise Bliss, cash-out, or payment. It
may point at the merit ladder as recognition, which `G1.51` defines.

**Prerequisite — the PROGRAM-LICENCE-GATE.** `G1.48` produced
`docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. Read it. If its `decision` is `null`,
STALL and escalate — do not publish into licence ambiguity. `CONTRIBUTING.md` is the document that
tells an outside contributor what happens to their intellectual property, and every one of the four
options in `G1.48` changes that answer. Writing it against the licence as it stands would commit the
project to inbound terms before the human has chosen the outbound ones, and a contributor who signs
off under those terms cannot be un-signed. The gate is at `00_MASTER_PROTOCOL.md` §4.6.

## 3. Scope

### In scope — files this item may edit
- `CONTRIBUTING.md` — create it at the repository root
- `.github/PULL_REQUEST_TEMPLATE.md` — create it; a sign-off checkbox and a scope statement
- `docs/PROMPTS/artifacts/B2/G1.49/` — create it; the verification transcript

### Out of scope — do not edit
- Anything under `.github/workflows/` — do not add a DCO-check job; that is a separate decision
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `LICENSE`, `LICENSE-COMMERCIAL.md`, `README.md`
- All Rust source
- `docs/PROMPTS/artifacts/B2/G1.48/**` — read-only input

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Paraphrasing the DCO so the
  literal-string check passes on a rewritten version, or moving a forbidden phrase into a file the
  verifier does not scan, are measurement changes.
- Write the document so it reads as though it were always correct. No "previously we had no
  contributing guide", no changelog residue, no self-justifying commentary in the body.
- State inbound licensing explicitly: contributions are licensed inbound on the same terms the
  project ships outbound, and the DCO sign-off is the attestation of the right to do so.
- Do not use the phrases "open source", "open-source", or "MIT" to describe this project. Do not
  call Eustress a game engine. Do not mention Rapier.
- Do not promise Bliss, cash-out, payment, revenue share, or any adjacent commercial right.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion

`CONTRIBUTING.md` exists, is at least 2,000 bytes, contains the DCO 1.1 verbatim markers, states
the inbound-equals-outbound rule, names `git commit -s` and the `Signed-off-by:` trailer, describes
the licence as source-available PolyForm Shield 1.0.0, contains **none** of the forbidden phrases,
and `.github/PULL_REQUEST_TEMPLATE.md` exists containing a sign-off confirmation line.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import os, re, sys
    C = "CONTRIBUTING.md"; T = ".github/PULL_REQUEST_TEMPLATE.md"
    fail = []
    if not os.path.exists(C): fail.append("CONTRIBUTING.md does not exist")
    else:
        txt = open(C, encoding="utf-8").read()
        if len(txt.encode("utf-8")) < 2000: fail.append("CONTRIBUTING.md < 2000 bytes")
        REQUIRED = [
            "Developer Certificate of Origin",
            "Version 1.1",
            "I have the right to submit it under the open source license",
            "Signed-off-by",
            "git commit -s",
            "PolyForm Shield",
            "source-available",
        ]
        for r in REQUIRED:
            if r not in txt: fail.append("CONTRIBUTING.md missing required text: %r" % r)
        for a, b, c, d in [("(a)", "(b)", "(c)", "(d)")]:
            for cl in (a, b, c, d):
                if cl not in txt: fail.append("DCO clause marker %s not found" % cl)
        low = txt.lower()
        # the phrase below is permitted ONLY inside the verbatim DCO clause (a)
        dco_start = low.find("developer certificate of origin")
        for bad in ("game engine", "rapier", "cash out", "cash-out", "cashable", "bliss payout"):
            if bad in low: fail.append("forbidden phrase in CONTRIBUTING.md: %r" % bad)
        for bad in ("open source", "open-source", " mit "):
            for m in re.finditer(re.escape(bad), low):
                # allowed only within the verbatim DCO block (which runs from its heading onward)
                if dco_start < 0 or m.start() < dco_start:
                    fail.append("phrase %r used to describe this project at offset %d"
                                % (bad, m.start()))
        if "inbound" not in low or "outbound" not in low:
            fail.append("inbound/outbound licensing rule not stated")
    if not os.path.exists(T): fail.append(".github/PULL_REQUEST_TEMPLATE.md does not exist")
    else:
        t = open(T, encoding="utf-8").read()
        if "Signed-off-by" not in t and "commit -s" not in t:
            fail.append("PR template does not mention the sign-off")
    print("contributing_bytes=%d template=%s"
          % (os.path.getsize(C) if os.path.exists(C) else 0, os.path.exists(T)))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    contributing_bytes=6412 template=True
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  contributing_bytes >= 2000

Note on the verifier: the phrase "open source license" appears inside DCO clause (a) verbatim and
is permitted **only** at or after the certificate heading. Any use of "open source" before that
point is a description of this project and fails.

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because it checks for the
DCO's own distinctive strings and clause markers rather than for a heading, enforces the
inbound-and-outbound language, and scans for the six phrases this repository's own house rules
forbid — including catching "open source" used anywhere the DCO does not license it.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — write CONTRIBUTING.md around the DCO plus a project-terms section
   -> if still failing, MANDATORY approach change. Rewording sections is NOT an approach change;
      restructuring so the verbatim certificate is a single quoted block at the end and every
      project-specific term precedes it is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: the inbound-IP section cannot be written without a Bliss or adjacent-commercial
                   statement (see front matter)
```

## 8. Artifact

`CONTRIBUTING.md`

A reader finds, in order: what the project is and is not; how to get a build running and the fact
that a full build takes 10–15 minutes and only one may run at a time; what a good first
contribution looks like; the licence section stating PolyForm Shield 1.0.0, source-available, and
the commercial-licence contact; the inbound-equals-outbound rule; the sign-off mechanism with the
literal `git commit -s`; and the Developer Certificate of Origin 1.1 verbatim as the final block.
Alongside it, `.github/PULL_REQUEST_TEMPLATE.md`, and the verification transcript at
`docs/PROMPTS/artifacts/B2/G1.49/verification.txt`.

## 9. Definition of NOT done

- The DCO is summarised in the author's own words. The whole legal value of a DCO is that it is the
  standard text; a paraphrase attests to nothing.
- The document says "open source" while describing the project. The licence is not OSI-approved and
  the repository's house rule is explicit.
- Contributors are told rank and Bliss follow the work. The Bliss ledger is at 0% and cash-out is a
  human-only decision; promising it in the contribution terms creates an obligation the project
  cannot meet.
- The document explains what the project used to lack. Documents must read as though always
  correct.
- A DCO-check workflow is added so the sign-off is enforced. Workflows are out of scope; enforcing
  it is a separate decision with its own item.
- `CONTRIBUTING.md` exists but never states what happens to the copyright in a merged
  contribution, which is the one question a first-time contributor's employer will ask.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.

---

---
id: G1.50
title: Extension loaded from outside the git worktree, with a measured first-run time
workload: W5
workload_secondary: [W6]
phase: G1
depends_on: [G1.49, G0.03]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.50/extension_proof.json
escalation: >
  If the studio cannot be launched to the point of plugin discovery on the founder's machine within
  three attempts, STALL. A documented extension surface that has never been exercised from outside
  the tree is a specification, not a surface.
status: DRAFT
notes: >
  This item proves the extension surface is reachable from outside the git worktree. It does not by
  itself produce W5's declared evidence type, "a third-party extension running unmodified" - that
  needs an author outside the company, which is G0.07's exit criterion. The artifact's w5_evidence
  field is derived from the recorded author identity and the verifier fails on disagreement.
---

## 1. Objective

An extension authored outside the git worktree, in `%LOCALAPPDATA%/Eustress/Plugins/`, loads into
the running studio and registers a visible button, proved by the engine's own discovery log, and the
wall-clock time from "nothing installed" to "extension visible" is measured. The artifact records
who wrote it, and derives from that whether the run is admissible as W5 evidence. A prospective
extension author can be shown that the surface is real rather than described.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0; say source-available. Physics is Avian. Slint is Rust. Units are meter-native.

**The extension surface, as implemented.**
`eustress/crates/engine/src/script_plugin_host.rs` is a script-authored studio plugin host. Its own
module documentation (`:1-22`) states the contract: a `.lua` or `.rune` file dropped into
`%LOCALAPPDATA%/Eustress/Plugins/` is discovered, executed or compiled once, and its registration
calls land on a shared "plugins" tab — **with no Rust recompile**. The registration API is
`plugin:AddSection` and `plugin:AddButton` in Luau, and `plugin_add_section` and
`plugin_add_button` in Rune. Both languages push onto the same `PluginBridge` queue (see
`eustress/crates/common/src/script_plugins`), so there is one drain, one teardown store, and one
Reload Plugins button for both.

Two implementation facts that determine how you write the extension:
- Rune plugins require an explicit top-level `register()` function; Luau chunks execute
  top-to-bottom implicitly (`script_plugin_host.rs` documentation immediately following the Luau
  loader).
- Each Luau plugin gets its own environment table via `Chunk::set_environment`, built by
  `LuauRuntime::build_plugin_environment` in `eustress/crates/common/src/luau/runtime.rs`, with an
  `__index` metatable falling through to the shared globals. The module documentation records that
  `mlua`'s `sandbox(true)` does **not** give per-chunk write isolation, verified against the mlua
  0.10.5 source.

**The discovery log line you will measure against.** `script_plugin_host.rs:284` emits, at `info`
level:

    🔌 Script plugin discovery: {discovered_count} plugin(s) loaded from {plugins_dir:?}

and each successful Luau load additionally emits `🔌 Script plugin loaded (Luau): {plugin_id} ...`.
A failed load raises a notification with `Plugin '{id}' failed to load: {error}` rather than a log
line, so absence of an error is not evidence — you must see the discovery line with a count of at
least 1.

**Why the plugin directory is deliberately global and outside the repository.** The module
documentation (`script_plugin_host.rs:15-22`) records the reason: a plugin is installed editor
tooling, and per-Space scripts would mean an opened Space silently runs privileged plugin code,
because this engine's Luau globals already give any script unrestricted `HttpService` and
`DataStoreService` access. That is a trust-boundary decision, not a convenience, and this item must
not relocate the directory.

**Build reality.** A full engine build takes 10–15 minutes. Only one cargo build at a time; the
workspace shares one `target/` and concurrent builds produce link failures. Never kill a build
mid-compile. Validate with `cargo run`, not `cargo check` — `cargo check` will not exercise plugin
discovery at all.

**What the extension surface is licensed to do.** Under `LICENSE-COMMERCIAL.md`, plugins, Rune and
Luau scripts, MCP tools, and end-products built *with* Eustress are permitted at no cost and carry
no royalty. What is **not** available, and must not be promised to an extension author, is a right
to build an adjacent competing tool — the Shield Competition clause reaches free products. Do not
write any claim about extension authors' commercial rights into the artifact; that boundary is the
subject of `G1.48` and is a human decision.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.50/` — create it; the extension source, the run log, the proof JSON
- `%LOCALAPPDATA%/Eustress/Plugins/` — you may create the directory and place one plugin file
  there, and you must remove it again at the end of the run

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source, including `eustress/crates/engine/src/script_plugin_host.rs`. If the extension
  cannot register without a code change, that is the finding.
- `eustress/crates/common/src/luau/runtime.rs`
- Any other file in `%LOCALAPPDATA%/Eustress/` — in particular, do not touch `telemetry/` or any
  Space directory

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Adding a Rust registration
  for the extension, relocating the plugin directory into the repository, or asserting the load
  from the absence of an error are all measurement changes.
- The extension source must live under `docs/PROMPTS/artifacts/B2/G1.50/` and be **copied** into
  the plugin directory. It must not be authored in place, so the artifact records exactly what was
  loaded.
- The extension must use only the documented registration API. No engine internals.
- Capture the studio's stdout and stderr to a log file; the exit criterion greps that file.
- Time the first run honestly: start the clock when the plugin directory is empty and stop it when
  the discovery line appears. Do not exclude the build.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion

The extension loads from `%LOCALAPPDATA%/Eustress/Plugins/` — a path with **no ancestor inside the
git worktree** — the captured run log contains the discovery line with a count of **at least 1** and
a line naming the extension's own plugin id, the loaded source uses only the four documented
registration calls, `extension_proof.json` records `time_to_first_extension_seconds` as a positive
number and a named `author` with `author_is_repository_contributor` set truthfully, and
`w5_evidence` agrees with that flag.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, os, re, sys, subprocess
    P = "docs/PROMPTS/artifacts/B2/G1.50/extension_proof.json"
    d = json.load(open(P, encoding="utf-8"))
    fail = []
    log = d.get("run_log")
    if not log or not os.path.exists(log):
        fail.append("run_log missing: %r" % log)
    else:
        txt = open(log, encoding="utf-8", errors="replace").read()
        m = re.search(r"Script plugin discovery:\s*(\d+)\s*plugin", txt)
        if not m:
            fail.append("discovery line not found in run log")
        elif int(m.group(1)) < 1:
            fail.append("discovery count is %s, need >= 1" % m.group(1))
        else:
            print("discovered=%s" % m.group(1))
        pid = d.get("plugin_id", "")
        if not pid or pid not in txt:
            fail.append("plugin_id %r does not appear in the run log" % pid)
    # The extension must live outside the git worktree entirely.
    src = d.get("extension_source")
    root = os.path.realpath(subprocess.check_output(
        ["git", "rev-parse", "--show-toplevel"], text=True).strip())
    if not src or not os.path.exists(src):
        fail.append("extension_source missing: %r" % src)
    else:
        real = os.path.realpath(src)
        if os.path.commonpath([real, root]) == root:
            fail.append("extension_source is inside the git worktree (%r); a plugin in the "
                        "repository is a first-party plugin in a different folder" % real)
        expect = os.path.realpath(os.path.join(os.environ.get("LOCALAPPDATA", ""),
                                               "Eustress", "Plugins"))
        if os.path.dirname(real) != expect:
            fail.append("extension_source must sit directly in %r, got %r" % (expect, real))
        print("extension_source=%s (outside worktree)" % real)
        s = open(real, encoding="utf-8").read()
        api = ["plugin:AddSection", "plugin:AddButton", "plugin_add_section", "plugin_add_button"]
        if not any(a in s for a in api):
            fail.append("extension source uses none of the documented registration calls")
        for leak in ("eustress/crates/", "../", "include(", "dofile("):
            if leak in s:
                fail.append("extension source reaches into the repository or the filesystem "
                            "outside the published API: %r" % leak)
    # Authorship is recorded, and the W5 claim is bound to it.
    a = d.get("author", {})
    for k in ("name", "contact", "authored_against"):
        if not str(a.get(k, "")).strip():
            fail.append("author.%s must be a non-empty string" % k)
    if a.get("authored_against") != "published API documentation only":
        fail.append("author.authored_against must be the literal string "
                    "'published API documentation only'")
    ic = a.get("author_is_repository_contributor")
    if ic not in (True, False):
        fail.append("author.author_is_repository_contributor must be a literal true or false")
    if d.get("w5_evidence") != (ic is False):
        fail.append("w5_evidence must equal (author_is_repository_contributor == false); "
                    "got w5_evidence=%r with contributor=%r" % (d.get("w5_evidence"), ic))
    print("author_is_repository_contributor=%r w5_evidence=%r" % (ic, d.get("w5_evidence")))
    t = d.get("time_to_first_extension_seconds")
    if not isinstance(t, (int, float)) or t <= 0:
        fail.append("time_to_first_extension_seconds must be a positive number, got %r" % t)
    else:
        print("time_to_first_extension_seconds=%r" % t)
    blob = json.dumps(d).lower()
    for bad in ("open source", "game engine", "rapier"):
        if bad in blob: fail.append("forbidden phrase in artifact: %r" % bad)
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    discovered=1
    extension_source=C:\Users\<user>\AppData\Local\Eustress\Plugins\ruler.lua (outside worktree)
    author_is_repository_contributor=True w5_evidence=False
    time_to_first_extension_seconds=1042.0
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  discovered >= 1
    AND  extension_source has no ancestor inside the git worktree

**What a pass does and does not license.** A pass proves the extension surface is real: the loader
finds a plugin that was never in the tree, never compiled with the engine, and written against the
published API alone. That is the surface leg of W5 and it is worth having.

A pass does **not** by itself produce W5's declared evidence type, "a third-party extension running
unmodified". If `author_is_repository_contributor` is `true`, `w5_evidence` is `false` and the run
may not be cited — in a deck, a pilot document, the website, or a phase report — as a third party
having built on Eustress. It is the founder demonstrating his own loader, which is a real but
different claim. The item that supplies a genuinely outside author is `G0.07`, whose exit criterion
turns on an engineer outside the company producing an artifact the verifier accepts. The honest
sequence is: this item proves the surface exists; `G0.07` proves someone used it.

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because it greps the engine's
own emitted discovery line rather than trusting a report, requires the plugin id to appear in that
same log, resolves the load path with `os.path.realpath` and fails it if `git rev-parse
--show-toplevel` is an ancestor, and requires the loaded source to use only the documented API with
no path or `dofile` reaching back into the tree — so neither a plugin registered through an engine
code change nor a first-party plugin relocated into a different folder can pass.

The criterion also binds the W5 claim to a recorded author identity rather than to the fact of a
successful load. `w5_evidence` is a derived field, not an assertion: the verifier recomputes it from
`author_is_repository_contributor` and fails on disagreement, so the item cannot pass while claiming
to be something it is not.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — a Luau extension registering one section and one button
   -> if still failing, MANDATORY approach change. Editing the Luau source is NOT an approach
      change; switching to a Rune extension with an explicit top-level register() function is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the discovery line still does not appear
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: the studio cannot be launched to plugin discovery in three attempts
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.50/extension_proof.json`

A reader finds: `generated_at`, `commit`, `language` (`luau` or `rune`), `plugin_id`,
`extension_source` (the absolute path the plugin was loaded from, which has no ancestor inside the
git worktree), `extension_source_sha256`, `run_log`, `time_to_first_extension_seconds`, an `author`
block (`name`, `contact`, `authored_against`, `author_is_repository_contributor`), the derived
`w5_evidence` boolean, and `steps` — the ordered list of what an outside author actually has to do,
which is the input to any future getting-started document. Alongside it:
`docs/PROMPTS/artifacts/B2/G1.50/loaded_source_copy.<ext>` — a byte-for-byte copy of the file that
was loaded, whose sha256 must equal `extension_source_sha256`, so the run is reproducible without
the plugin directory — and `docs/PROMPTS/artifacts/B2/G1.50/logs/studio_run.log`.

The copy is evidence, not the load path. A plugin loaded from inside the artifact directory fails
the exit criterion.

## 9. Definition of NOT done

- The extension registers, but only after a Rust change to `script_plugin_host.rs`. That proves the
  engine can be extended by its author, not by a third party.
- The proof rests on the absence of a `failed to load` notification. Failures surface as
  notifications, not log lines; only the discovery line with a count of at least 1 is evidence.
- The plugin is loaded from a folder inside the repository — `docs/PROMPTS/artifacts/B2/G1.50/`, a
  `plugins/` directory in the tree, anywhere under the worktree root. That is a first-party plugin
  in a different folder, and it proves nothing about a surface an outsider can reach. The load path
  must have no ancestor inside the worktree.
- The plugin loads from `%LOCALAPPDATA%/Eustress/Plugins/` but the artifact keeps no copy, so the
  exact bytes that were loaded are gone the moment the directory is cleaned and nobody can reproduce
  the run.
- The extension reaches back into the repository — a relative path out of the plugin directory, a
  `dofile` of a file in the tree, an import of an internal module. Then it is not authored against
  the published API and the surface it proves is not the one an outsider has.
- `author_is_repository_contributor` is recorded as `false` because the author used a different
  machine, a different account, or wrote it "as if" they were an outsider. The flag records who
  wrote it, not how it was written, and a false entry here manufactures W5 evidence out of nothing.
- The run passes with `w5_evidence: false` and is then cited in a deck, a pilot document, or a
  phase report as a third party having built on Eustress. The flag exists precisely to stop that.
- `time_to_first_extension_seconds` excludes the engine build, making the number look attractive and
  useless — a first-time extension author pays that cost too.
- The extension is left installed in the plugin directory after the run, changing the state of the
  founder's machine for every later item.
- The artifact claims extension authors may build and sell adjacent tools. That right is not granted
  by the Shield Competition clause and is a human decision under `G1.48`.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.

---

---
id: G1.51
title: Merit ladder with rungs computable from repository history alone
workload: W5
workload_secondary: [W6]
phase: G1
depends_on: [G1.49]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.51/merit_ladder.json
escalation: >
  If a rung cannot be defined without referencing Bliss, payment, or any transferable value, STALL.
  The ledger is at 0% and cash-out is a human-only decision; a ladder whose top rung is unpayable
  is a promise the project cannot keep.
status: DRAFT
notes: >
  Tier M for headroom on the scorer, but zero builds: the rung computation runs over git history
  with Python and needs no compile.
---

## 1. Objective

A five-rung contributor ladder exists in which every advancement criterion is computed from
repository history by a runnable script, no rung depends on a currency that cannot move, and
running the script over this repository's actual history assigns distinct rungs to distinct
contributors. A prospective contributor can see exactly what advancement requires.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0; say source-available. Physics is Avian. Slint is Rust. Units are meter-native.

**What the project already promises.** `README.md` describes a "Merit-only ladder": install the
engine, contribute, earn rank, become a Contributor, earn Bliss, publish, and cash out. `README.md:153`
repeats that "rank and Bliss follow the work".

**Why that promise cannot be the ladder.** `docs/AUDIT/09_ECONOMY.md` records the Bliss balance
ledger at **0%** — no currency can move; `purchase_item` has no working debit path; the payout
ledger is at 10%; Stripe Connect is effectively 0% and gated on KYC. `docs/AUDIT/09_ECONOMY.md` P4
additionally records that the Bliss dual-nature carries regulatory arbitrage risk needing legal
counsel before public launch, and `00_MASTER_PROTOCOL.md` §6 makes any Bliss design touching
transferability or cash-out **human-only**. A ladder denominated in Bliss is therefore not
implementable by an agent and not payable by the project.

**What the ladder can be denominated in instead** — things the project can actually deliver today:
merge rights over a named area, review authority, a listed name, direct access to the founder,
priority on issue triage, and early access to builds. All of these are grantable with a
`CODEOWNERS` entry, a repository role, or a published list.

**The measurable substrate.** Repository history. Measured 2026-08-06:
`git rev-list --count HEAD` returns **483**; `git log --format='%aN' | sort | uniq -c | sort -rn`
returns five distinct author names, the largest with 476 commits and the smallest with 1. So a
rung computation over `git log` has real, differentiated input in this repository today.

**What `CONTRIBUTING.md` now says.** `G1.49` created it. Read it; the ladder must not contradict
its inbound-licensing statement, and it must not reintroduce a Bliss promise that
`CONTRIBUTING.md` deliberately omits.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.51/` — create it; the ladder JSON, the rung scorer, the transcript

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `README.md`, `CONTRIBUTING.md`, `LICENSE`, `LICENSE-COMMERCIAL.md`
- All Rust source
- `.github/CODEOWNERS` — `G1.55` owns that file; this item may reference it but not create it
- Git history itself. Do not commit, rebase, amend, tag, or otherwise mutate history to produce
  input for the scorer.

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Creating commits to
  populate the ladder, or lowering a threshold until the founder's own account clears the top rung,
  are measurement changes.
- Every rung's criterion must be computed by the scorer from `git log` output alone. A criterion
  that requires a human judgement call belongs in the rung's `additional_review` field, not in the
  computed threshold.
- No rung may reference Bliss, tokens, payment, cash-out, revenue share, or equity.
- Thresholds must be strictly increasing across rungs on at least one computed dimension.
- The scorer must not shell out to anything but `git`, and must not write outside the artifact
  directory.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion

`docs/PROMPTS/artifacts/B2/G1.51/rung.py` runs over this repository's history and emits a rung for
every distinct author; the ladder defines exactly **5** rungs with strictly increasing thresholds
on a named computed dimension; the run assigns **at least 2 distinct rung values** across the
authors present; no rung text contains a forbidden economic term; and `merit_ladder.json` records
the same assignment the scorer prints.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python docs/PROMPTS/artifacts/B2/G1.51/rung.py \
        --repo . \
        --out docs/PROMPTS/artifacts/B2/G1.51/merit_ladder.json
    echo "SCORER_EXIT=$?"

    python - << 'PYEOF'
    import json, sys
    P = "docs/PROMPTS/artifacts/B2/G1.51/merit_ladder.json"
    d = json.load(open(P, encoding="utf-8"))
    rungs = d["rungs"]; assign = d["assignments"]
    fail = []
    if len(rungs) != 5: fail.append("expected 5 rungs, got %d" % len(rungs))
    dim = d.get("primary_dimension")
    if not dim: fail.append("primary_dimension not named")
    prev = None
    for r in rungs:
        th = r.get("threshold", {}).get(dim)
        if not isinstance(th, (int, float)):
            fail.append("%s: threshold[%r] must be numeric, got %r" % (r.get("key"), dim, th)); continue
        if prev is not None and th <= prev:
            fail.append("%s: threshold %r not strictly greater than previous %r" % (r["key"], th, prev))
        prev = th
        if not str(r.get("grants", "")).strip(): fail.append("%s: empty grants" % r["key"])
    vals = set(assign.values())
    if len(vals) < 2:
        fail.append("scorer assigned only %d distinct rung(s): %r" % (len(vals), sorted(vals)))
    known = {r["key"] for r in rungs}
    for a, v in assign.items():
        if v not in known: fail.append("author %r assigned unknown rung %r" % (a, v))
    blob = json.dumps(d).lower()
    for bad in ("bliss", "cash out", "cash-out", "token", "payout", "revenue share", "equity",
                "open source", "game engine", "rapier"):
        if bad in blob: fail.append("forbidden term in ladder: %r" % bad)
    print("rungs=%d authors=%d distinct_rungs=%d dimension=%s"
          % (len(rungs), len(assign), len(vals), dim))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    SCORER_EXIT=0
    rungs=5 authors=5 distinct_rungs=3 dimension=merged_commits
    RESULT: PASS
    EXIT=0

Pass condition:

    SCORER_EXIT=0  AND  EXIT=0  AND  "RESULT: PASS" present  AND  distinct_rungs >= 2

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because the scorer is executed
against real history rather than described, thresholds must be strictly increasing on a named
dimension, the run must actually differentiate contributors, and the whole artifact is scanned for
the economic terms the project cannot currently honour.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — rungs keyed on merged commit count
   -> if still failing, MANDATORY approach change. Moving thresholds is NOT an approach change;
      switching the primary dimension to distinct crates touched, or to commits that survive
      into the current tree, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with distinct_rungs unchanged at 1
  - Budget      : 300k tokens consumed (150% of the M envelope)
  - Item-specific: a rung cannot be defined without a Bliss or payment reference
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.51/merit_ladder.json`

A reader finds: `generated_at`, `primary_dimension`, `rungs` (five, each with `key`, `name`,
`threshold` keyed by dimension, `grants` stating exactly what authority or access the rung confers,
`additional_review` for any human judgement, and `revocation` stating how the rung is lost), and
`assignments` mapping each author name found in history to a rung key. Alongside it:
`docs/PROMPTS/artifacts/B2/G1.51/rung.py` (the scorer) and
`docs/PROMPTS/artifacts/B2/G1.51/ladder.md` — the public-facing description a contributor reads.

## 9. Definition of NOT done

- The top rung grants Bliss, a payout, or a share of revenue. None of those can move today and all
  of them are human-only decisions.
- The rungs are named and described but no script computes them, so advancement is at the founder's
  discretion and the ladder is decoration.
- The scorer runs but assigns every author the same rung, which proves the thresholds do not
  discriminate on this repository's actual history.
- Thresholds are set so the founder's own account is the only one above rung one, which makes the
  ladder unclimbable and visibly so.
- A rung's `grants` field says "recognition" without naming what changes — no merge right, no
  review authority, no listing, no access.
- No `revocation` is defined, so a rung is permanent regardless of subsequent conduct or inactivity.
- Commits are created during the item to give the scorer better input. Mutating history to feed the
  measurement fails the item outright.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.

---

---
id: G1.52
title: Governance non-negotiables, each with a runnable violation detector
workload: W5
workload_secondary: [W3]
phase: G1
depends_on: [G1.48]
blocks: [G1.55]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.52/governance_invariants.json
escalation: >
  If any proposed non-negotiable is already violated in the current tree and cannot be restated so
  its detector passes without changing code, STALL. A governance document that ships already broken
  teaches everyone to ignore it.
status: DRAFT
notes: >
  Every invariant carries a grep or git command that exits 0 while the invariant holds. This makes
  the governance document executable rather than aspirational, and it is the mechanism G1.55
  attaches decision rights to.
---

## 1. Objective

A governance document exists naming the project's non-negotiable invariants, where each invariant
carries a literal command that returns success while the invariant holds and failure the moment it
is broken. Running every detector today passes. The document can therefore be enforced rather than
merely cited.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — a world model an AI reasons over and a
document a human edits. It is never called a game engine. Licence is PolyForm Shield 1.0.0
(`LICENSE`), dual-licensed against `LICENSE-COMMERCIAL.md`; say source-available, never open
source. Physics is **Avian**, never Rapier. `.slint` compiles to Rust, so Slint **is** Rust. Units
are meter-native; studs are a display unit only.

**Why this item exists.** As headcount grows, the things that make Eustress coherent are the things
most likely to be traded away by someone optimising locally. The repository already carries several
invariants that exist only as habits and would not survive a second engineer:

- **The licence framing.** Any file describing the project as open source contradicts `LICENSE`.
  One already does: `docs/marketing/UofA_Center_For_Innovation_Pilot.html` contains the literal
  string `Open source · MIT-friendly licensing`. It is a marketing asset, out of scope here, and is
  the subject of `G1.48` — but it demonstrates precisely how the invariant fails in practice.
- **One physics engine.** Avian. A second physics dependency would fork the simulation semantics.
- **One creation path.** Entity creation routes through the canonical creation function; a second
  path silently diverges persistence behaviour.
- **One `target/` directory and one build at a time.** Concurrent cargo builds in this workspace
  produce link failures (LNK2001 / SAC os error 4551).
- **Meter-native units.** Studs are display only; a stud-native computation reintroduces a
  conversion bug class the project already paid for.
- **Telemetry is opt-out and aggregate-only.** `eustress/crates/engine/src/usage_telemetry.rs`
  gates all recording on an `enabled` flag, keeps per-click timing on the machine, and sends one
  session aggregate. Weakening this is a trust regression, not a feature.
- **Documents read as though always correct.** No changelog residue in a document body.
- **CI must never be modified to make a gate pass.** This is the archetypal governance failure in an
  agent-run program.

**What a detector looks like.** A single shell command whose exit status encodes the invariant. Two
shapes are permitted and nothing else:

- `grep ...` — for example, a `grep -rL` or a negated match that exits non-zero when a forbidden
  string appears in a scoped path set.
- `git ...` — for example, `git ls-files` piped into a check.

The verifier will execute each detector and require exit status 0 **today**. Design each detector so
0 means "invariant holds".

**Known-good grounding for detectors.** These paths exist and are stable:
`LICENSE`, `LICENSE-COMMERCIAL.md`, `README.md`, `eustress/Cargo.toml`,
`eustress/crates/engine/src/usage_telemetry.rs`, `eustress/crates/engine/src/tool_metadata.rs`,
`eustress/crates/common/src/simulation/clock.rs`, `.github/workflows/ci.yml`,
`.github/workflows/linux-engine.yml`, `.github/workflows/release.yml`.

**Boundary.** Governance that would require a licence change, an external publication, a spend, a
production deployment, or a Bliss design decision is human-only under
`00_MASTER_PROTOCOL.md` §6. Such items may appear in the document as `human_owned: true`, but they
carry no detector and do not count toward the required detector total.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.52/` — create it; the invariants JSON and the human-readable charter

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source, all `.slint` files
- `LICENSE`, `LICENSE-COMMERCIAL.md`, `README.md`, `docs/marketing/**`
- `.github/CODEOWNERS` — `G1.55` owns it

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Scoping a detector to a
  directory that happens to be clean, or writing a detector that always exits 0 regardless of the
  tree, are measurement changes. Each detector must be falsifiable: state, in
  `how_it_would_fail`, the concrete edit that would make it exit non-zero.
- Detectors must start with the literal token `grep` or `git`. No other program.
- A detector must not write to the filesystem.
- Do not fix a violation you discover. If an invariant is already broken, record it with
  `currently_violated: true`, exclude it from the detector count, and report it. Fixing code is out
  of scope.
- Write the charter so it reads as though always correct.
- **PROGRAM-LICENCE-GATE.** Before writing anything this item hands to, or publishes for, someone
  outside the company, read `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json`. If it
  records `decision == null`, STALL and escalate — do not publish into licence ambiguity. `G1.48`
  is forbidden to record a decision; only the human may, under `00_MASTER_PROTOCOL.md` §6. A null
  decision is therefore the expected state, and it is a stop, not a formality to note and pass. Do
  not substitute your own reading of `LICENSE`, and do not proceed "against the licence as it
  stands" — the gate exists because the terms a third party is handed are the thing under decision.

## 5. Exit criterion

### Criterion

`governance_invariants.json` declares **at least 8** invariants; **at least 6** of them carry a
`detector` command starting with `grep` or `git`; **every** declared detector exits **0** when run
from the repository root today; every invariant carries a non-empty `how_it_would_fail` and an
`owner`; and every invariant marked `human_owned: true` carries no detector.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, subprocess, sys
    P = "docs/PROMPTS/artifacts/B2/G1.52/governance_invariants.json"
    d = json.load(open(P, encoding="utf-8"))
    inv = d["invariants"]; fail = []
    if len(inv) < 8: fail.append("only %d invariants, need >= 8" % len(inv))
    detectors = 0
    for i in inv:
        k = i.get("key", "?")
        if not str(i.get("how_it_would_fail", "")).strip():
            fail.append("%s: empty how_it_would_fail" % k)
        if not str(i.get("owner", "")).strip():
            fail.append("%s: empty owner" % k)
        cmd = i.get("detector")
        if i.get("human_owned"):
            if cmd: fail.append("%s: human_owned invariants carry no detector" % k)
            continue
        if not cmd:
            continue
        first = cmd.strip().split()[0]
        if first not in ("grep", "git"):
            fail.append("%s: detector must start with grep or git, got %r" % (k, first)); continue
        r = subprocess.run(cmd, shell=True, capture_output=True, text=True)
        if r.returncode != 0:
            fail.append("%s: detector exited %d (invariant not holding or detector wrong): %s"
                        % (k, r.returncode, cmd))
        else:
            detectors += 1
    if detectors < 6: fail.append("only %d passing detectors, need >= 6" % detectors)
    print("invariants=%d passing_detectors=%d" % (len(inv), detectors))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    invariants=10 passing_detectors=7
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  passing_detectors >= 6  AND  invariants >= 8

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is unusually tight: the verifier does
not read the detectors, it **runs** them, so an invariant that is already violated fails the item,
and a detector that cannot express the invariant fails it too.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — one detector per invariant, each a scoped grep
   -> if still failing, MANDATORY approach change. Rewriting a grep pattern is NOT an approach
      change; switching to git-ls-files-driven detectors that enumerate the tracked file set
      first, or restating the invariant so it is expressible, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with passing_detectors unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: an invariant is already violated and cannot be restated without a code change
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.52/governance_invariants.json`

A reader finds: `generated_at`, and `invariants` — each with `key`, `statement` (one sentence, in
the imperative), `why` (what breaks if it goes), `detector` (a literal `grep` or `git` command, or
absent for human-owned items), `how_it_would_fail` (the concrete edit that flips the detector),
`owner`, `human_owned`, and `currently_violated`. Alongside it,
`docs/PROMPTS/artifacts/B2/G1.52/charter.md` — the same invariants as prose a new engineer reads on
day one, with the detector commands inline so they can be run.

## 9. Definition of NOT done

- The invariants are stated but no detector runs, so the document is a values statement rather than
  a control.
- A detector is written to always succeed — for example, grepping for a string that is trivially
  present — so it passes without constraining anything. `how_it_would_fail` exists to expose this
  and must name a concrete edit.
- A discovered violation is quietly fixed in source. Source is out of scope; the finding is the
  deliverable.
- Every invariant is marked `human_owned` to avoid writing detectors, leaving fewer than six
  enforceable.
- The charter explains the history of each rule. Documents read as though always correct.
- The licence invariant is written as "avoid confusing licence language", which is unenforceable.
  It must be a detector over a scoped file set for the exact forbidden strings.
- The artifact is complete and correct, and is published or handed to a third party while
  `docs/PROMPTS/artifacts/B2/G1.48/licence_decision_memo.json` records `decision == null`. The
  PROGRAM-LICENCE-GATE (`00_MASTER_PROTOCOL.md` §4.6) fires; the item is STALLED, not PASSED.

---

---
id: G1.53
title: Four first-hire evaluation tasks drawn from open defects in this repository
workload: W6
workload_secondary: [W3]
phase: G1
depends_on: [G1.41]
blocks: [G1.55]
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.53/hire_evaluations.json
escalation: >
  If any role's evaluation task cannot be defined without the candidate needing a full engine build
  on their own machine, STALL and hand the human the choice: pay for the candidate's build time,
  supply a prebuilt binary, or replace the task. A take-home nobody can start is a filter on
  patience, not on skill.
status: DRAFT
notes: >
  Every task is a real open defect with a real path. That is deliberate: a candidate who solves it
  has done paid-quality work, and the company learns whether the defect is tractable at the same
  time.
---

## 1. Objective

Four evaluation tasks exist, one per critical first hire, each anchored to a specific open defect in
this repository with paths that resolve, a measurable definition of done, a time box, and a stated
disqualifier. No two tasks touch the same files, so four candidates can be evaluated in parallel
without collision.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0; say source-available. Physics is **Avian**, never Rapier. `.slint` compiles to Rust,
so Slint **is** Rust. Units are meter-native.

**The four fixed role keys** — the verifier requires exactly these:
`physics_simulation`, `agent_systems`, `distributed_systems`, `enterprise_gtm`.

**The open defects available as task material.** Each is real, cited, and unfixed.

1. **Time compression silently drops physics steps.**
   `eustress/crates/common/src/simulation/clock.rs` — `advance()` at `:83` advances
   `simulation_time_s` by the full compressed delta, but physics ticks are capped at
   `max_ticks_per_frame` (default 10) and the accumulator is **zeroed** on saturation (`:100-102`).
   So at high `time_scale` the clock reports compressed time while the steps that would have
   covered it are discarded, and `effective_compression()` (`:131`) still reports a healthy ratio.
   There is no error, no counter, and no watchpoint. Presets up to 7.2e6x are documented in
   `docs/development/SIMULATION_SYSTEM.md`.
2. **Determinism is asserted, never verified.**
   `eustress/crates/common/src/physics/determinism.rs` is 57 lines holding a `GlobalRngSeed` and
   nothing else. The byte-identical-recording gate at
   `docs/architecture/HEADLESS_RUNTIME.md:294` is written and has never been run.
   `docs/AUDIT/11_SIMULATION_DEBUGGER.md` Feature 8 records the Avian deterministic step as
   single-run only, cross-platform untested.
3. **The agent has no headless eyes.**
   `eustress/crates/engine/src/bin/headless.rs` is `MinimalPlugins` plus `ScheduleRunnerPlugin`,
   so `ai_camera_capture` and `capture_viewport` require a desktop session. Its own module
   documentation says capture needs the future `--render gpu` tier, and
   `docs/architecture/HEADLESS_RUNTIME.md:269` still marks P6 as new. The observation half of the
   agent loop therefore cannot run in CI or a container.
4. **Watchman alert cooldown is wall-time, not sim-time.**
   `docs/AUDIT/11_SIMULATION_DEBUGGER.md` records that at 1e6x time compression it misses
   sub-30-second spikes.
5. **Telemetry has no persistence plan.** `docs/AUDIT/10_TELEMETRY.md` records roughly 85M
   messages per second in-process, which at 1 KB average and 7-day retention is 73 TB/week, with
   per-topic sampling undefined. The stream crate is `eustress/crates/stream/` with a Criterion
   bench at `eustress/crates/stream/benches/throughput.rs`.
6. **CI proves nothing.** `.github/workflows/ci.yml` runs `cargo deny`, a naga WGSL check that
   skips any shader with naga_oil directives, and a `cargo tree` grep;
   `.github/workflows/linux-engine.yml` runs a single `cargo check --package eustress-engine`;
   `.github/workflows/release.yml` runs `cargo build --release --package eustress-engine`. There
   are 2,061 `#[test]` functions in `eustress/crates/` and none execute in CI.
7. **One file carries the studio.** `eustress/crates/engine/src/ui/slint_ui.rs` is 23,103 lines.
   Every panel's drain logic funnels through it, including the drain-skip failure class where one
   missing required parameter silently kills every UI click.
8. **`eustress-backend` does not compile.**
   `eustress/crates/backend/src/marketplace.rs:194-215` calls `find_marketplace_item_by_id`,
   `has_purchased`, `get_user_balance`, and `purchase_item` on `Database`, and
   `eustress/crates/backend/src/db.rs` defines none of them. The crate is a workspace member
   (`eustress/Cargo.toml:12`).

**The go-to-market role is not a coding task.** Its evaluation must be anchored to real artifacts:
the selected vertical from `docs/PROMPTS/artifacts/B2/G1.41/vertical_selection.json`, the SKU sheet
at `docs/PROMPTS/artifacts/B2/G1.42/sku_sheet.json` if it exists, and the existing pilot asset
`docs/marketing/UofA_Center_For_Innovation_Pilot.html`. A candidate should be asked to produce
something a buyer would receive, and to identify at least one claim in the existing asset they
would refuse to make. That is the actual job.

**The build constraint that shapes every task.** A full engine build takes 10–15 minutes; only one
cargo build may run at a time; the workspace shares a single `target/`. A take-home that requires
six builds costs the candidate two hours of compile before any thinking.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.53/` — create it; the evaluation pack and per-role briefs

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source. This item designs the tasks; it does not solve them.
- `docs/marketing/**`
- `docs/PROMPTS/artifacts/B2/G1.41/**` and `.../G1.42/**` — read-only inputs

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Reusing one defect across
  two roles, or citing a directory rather than a file so the disjointness check passes, are
  measurement changes.
- Every `repo_paths` entry must be a file that exists. Directories do not count.
- Each task states `definition_of_done` containing a number and a comparator, and a
  `time_box_hours` of 8 or fewer. A take-home longer than one working day is unpaid labour.
- Each task states a `disqualifier`: the specific shortcut that, if taken, ends the evaluation
  regardless of the result.
- Do not solve any of the defects while writing the tasks.

## 5. Exit criterion

### Criterion

`hire_evaluations.json` contains **exactly** the four fixed role keys; every role has **at least
2** `repo_paths` that exist; no path appears under more than one role; every
`definition_of_done` contains a digit; every `time_box_hours` is a number `<= 8`; and every role
has a non-empty `disqualifier` and `what_it_reveals`.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, os, re, sys
    P = "docs/PROMPTS/artifacts/B2/G1.53/hire_evaluations.json"
    ROLES = {"physics_simulation","agent_systems","distributed_systems","enterprise_gtm"}
    d = json.load(open(P, encoding="utf-8"))
    roles = {r["role"]: r for r in d["roles"]}
    fail = []
    if set(roles) != ROLES:
        fail.append("role keys must be %r, got %r" % (sorted(ROLES), sorted(roles)))
    seen = {}
    for k, r in roles.items():
        paths = r.get("repo_paths", [])
        if len(paths) < 2: fail.append("%s: needs >= 2 repo_paths, got %d" % (k, len(paths)))
        for p in paths:
            if not os.path.isfile(p): fail.append("%s: repo_path is not an existing file: %r" % (k, p))
            elif p in seen: fail.append("%s: repo_path %r already used by %s" % (k, p, seen[p]))
            else: seen[p] = k
        dod = str(r.get("definition_of_done", ""))
        if not re.search(r"\d", dod):
            fail.append("%s: definition_of_done has no number: %r" % (k, dod[:60]))
        tb = r.get("time_box_hours")
        if not isinstance(tb, (int, float)) or tb <= 0 or tb > 8:
            fail.append("%s: time_box_hours must be a number in (0, 8], got %r" % (k, tb))
        for f in ("disqualifier", "what_it_reveals"):
            if not str(r.get(f, "")).strip(): fail.append("%s: empty %s" % (k, f))
    blob = json.dumps(d).lower()
    for bad in ("rapier", "game engine", "open source"):
        if bad in blob: fail.append("forbidden phrase in artifact: %r" % bad)
    print("roles=%d distinct_paths=%d" % (len(roles), len(seen)))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    roles=4 distinct_paths=11
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  roles=4  AND  distinct_paths >= 8

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because every cited path is
resolved against the filesystem, paths may not be shared between roles, and each definition of done
must contain a number — which is what stops the tasks degrading into "have a look at the physics
and tell us what you think".

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — one defect per role from the list in section 2
   -> if still failing, MANDATORY approach change. Swapping which defect goes to which role is
      NOT an approach change; switching to a build-free formulation (candidate reasons over a
      supplied recording or log rather than compiling) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: a task cannot be defined without a full engine build on the candidate's machine
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.53/hire_evaluations.json`

A reader finds: `generated_at`, and `roles` — four objects, each with `role`, `title`, `defect`
(the one-sentence statement of the open problem), `repo_paths`, `task` (what the candidate is
asked to produce), `definition_of_done` (with a number and a comparator), `time_box_hours`,
`disqualifier`, `what_it_reveals`, and `reference_answer_sketch` (for the interviewer only).
Alongside it, one brief per role at
`docs/PROMPTS/artifacts/B2/G1.53/briefs/<role>.md` — the version handed to the candidate, which
must not contain `reference_answer_sketch`.

## 9. Definition of NOT done

- The physics task is "improve the simulation clock". The defect is specific — the accumulator is
  zeroed on saturation at `eustress/crates/common/src/simulation/clock.rs:100-102` while
  `simulation_time_s` keeps the full delta — and the task must name what the candidate must
  surface.
- Two roles are both pointed at `eustress/crates/engine/src/bin/headless.rs`, so two candidates
  cannot be run in parallel and their work is not comparable.
- The go-to-market task is a written case study with no artifact a buyer would receive, so it tests
  writing rather than the job.
- `definition_of_done` reads "the fix works". Without a number the interviewer's judgement is the
  only gate, which is exactly the bias this item removes.
- A take-home is scoped at 20 hours. Any time box above 8 hours fails the verifier.
- The candidate briefs include the reference answer sketch, which is in the artifact for the
  interviewer only.
- The tasks are written but every one of them requires the candidate to first get a 10–15 minute
  engine build green on Windows, and nothing in the brief warns them.

---

---
id: G1.54
title: Capital strategy matched to a deep-tech clock, with three costed paths
workload: W2
workload_secondary: [W6]
phase: G1
depends_on: [G1.43, G1.45]
blocks: []
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.54/capital_strategy.json
escalation: >
  If any path's runway does not reach the first revenue milestone from the G1.45 ladder, STALL. A
  financing plan that runs out before the first paying customer is not a financing plan, and the
  human must choose between raising more, cutting burn, or moving the milestone.
status: DRAFT
notes: >
  This is a planning artifact. Any actual raise, term sheet, or spend is a human-only decision
  under 00_MASTER_PROTOCOL.md section 6, so the artifact ends with decision null and a named
  recommendation.
---

## 1. Objective

Three financing paths are costed against the same burn assumption, each showing runway in months,
the milestone from the ARR ladder it reaches, the dilution it costs, and the control it transfers.
The arithmetic is self-consistent and re-derivable, and the artifact ends in a decision the human
makes.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0; say source-available. Physics is Avian. Slint is Rust. Units are meter-native.

**Inputs you must read.** `docs/PROMPTS/artifacts/B2/G1.43/unit_economics.json` (the compute cost
term in burn) and `docs/PROMPTS/artifacts/B2/G1.45/arr_ladder.json` (the milestone each path must
reach). The ladder's five fixed rung keys are `first_paying_customer`, `arr_1m`, `arr_10m`,
`arr_100m`, `arr_1b`.

**Why a SaaS financing clock does not fit.** The value proposition is validated simulation, and a
buyer only learns whether it worked after months of use. Meanwhile the engineering surface is deep:
`docs/architecture/HEADLESS_RUNTIME.md:294` carries a determinism gate that has never been run;
`eustress/crates/engine/src/bin/headless.rs` has no GPU tier so the agent loop cannot run in CI;
`docs/AUDIT/07_AI_PLATFORM.md` records `FoundationModelDispatcher` fully absent and `spatial-llm` a
stub. None of those are quarters-long problems solved by hiring salespeople.

**The three fixed path keys** — the verifier requires exactly these:

- `A_revenue_financed` — no external capital; commercial licences and paid pilots fund the work.
- `B_non_dilutive` — grants, programme credits, and non-dilutive instruments.
  `LAUNCH_PLAN.md` Stream 6 records an AWS Activate submission and a Startup Tucson follow-up as
  live threads.
- `C_priced_equity` — a priced round from institutional investors.
  `docs/architecture/DECENTRALIZATION_PLAN.md` decisions #16 and #26 record a Draper Goren
  relationship and name `eustress-genesis-ua-tech-01` (UA Tech Parks and the Chamber) in the
  genesis set.

**Corporate state, which bounds every path.** `LAUNCH_PLAN.md` Stream 2 records the Nevada LLC as
"In progress (forming)"; Stream 3 records banking as "Blocked (needs EIN)". A single-member LLC
cannot take priced-equity investment without a conversion, and that conversion is a legal step with
a cost and a delay. Record it in path `C_priced_equity` as a `precondition`.

**The regulatory exposure that must be disclosed to any investor, and must appear in this
artifact.** `docs/AUDIT/09_ECONOMY.md` P4 records that the Bliss dual-nature carries regulatory
arbitrage risk needing legal counsel before public launch, and R3.3 records that money-transmission
licensing varies by US state. `docs/architecture/DECENTRALIZATION_PLAN.md` #15 puts peer-to-peer
transfers on-chain. A cash-outable, transferable contribution token issued by a single-member
Nevada LLC is the highest-consequence open item in the repository, and it is a diligence finding
whether or not it is raised first.

**What this artifact is not.** It is not investment advice and it must not read as any. It is the
founder's own financing plan for the founder's own company. It must not recommend that anyone buy,
sell, or hold any security or token.

**Human-only boundary.** Any spend, any signed agreement, and any Bliss design touching
transferability or cash-out are human-only decisions under `00_MASTER_PROTOCOL.md` §6.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.54/` — create it; the strategy JSON and the one-screen packet

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source
- `LAUNCH_PLAN.md`, `docs/architecture/DECENTRALIZATION_PLAN.md`, `docs/AUDIT/**`
- `docs/PROMPTS/artifacts/B2/G1.43/**` and `.../G1.45/**` — read-only inputs

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Lowering the burn
  assumption until a path's runway clears the milestone, or moving the milestone, are measurement
  changes.
- All three paths use the **same** `monthly_burn_usd`. It is declared once at the top level and the
  verifier reads it from there.
- Every number carries a `_kind` of `TARGET` or `MEASURED`. Only figures traceable to
  `unit_economics.json` or to a file in this repository may be `MEASURED`.
- Every path names `milestone_reached` as one of the five ladder rung keys.
- Every path carries `dilution_pct`, `board_seats_transferred`, and `protective_provisions` (a
  list, possibly empty).
- The artifact carries `decision: null`, `human_decision_required: true`, and a `recommendation`
  naming one path key with a one-sentence reason.
- The artifact must contain a `disclosures` list including the Bliss regulatory exposure.

## 5. Exit criterion

### Criterion

`capital_strategy.json` contains **exactly** the three fixed path keys; for every path
`abs(runway_months - raise_usd / monthly_burn_usd) <= 0.5` where `raise_usd` is 0 for the
revenue-financed path and its runway instead derives from `revenue_usd_per_month`; every path's
`milestone_reached` is one of the five ladder rung keys; every numeric field carries a valid
`_kind`; `decision` is null and `human_decision_required` is true; and `disclosures` contains an
entry whose `topic` is `bliss_regulatory_exposure`.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, sys
    P = "docs/PROMPTS/artifacts/B2/G1.54/capital_strategy.json"
    L = "docs/PROMPTS/artifacts/B2/G1.45/arr_ladder.json"
    RUNGS = {r["key"] for r in json.load(open(L, encoding="utf-8"))["rungs"]}
    PATHS = {"A_revenue_financed", "B_non_dilutive", "C_priced_equity"}
    KINDS = {"TARGET", "MEASURED"}
    d = json.load(open(P, encoding="utf-8"))
    burn = d.get("monthly_burn_usd")
    fail = []
    if not isinstance(burn, (int, float)) or burn <= 0:
        fail.append("monthly_burn_usd must be a positive number, got %r" % burn)
    if d.get("monthly_burn_usd_kind") not in KINDS:
        fail.append("monthly_burn_usd_kind must be TARGET or MEASURED")
    paths = {p["key"]: p for p in d["paths"]}
    if set(paths) != PATHS:
        fail.append("path keys must be %r, got %r" % (sorted(PATHS), sorted(paths)))
    for k, p in paths.items():
        for f, v in list(p.items()):
            if isinstance(v, (int, float)) and not isinstance(v, bool):
                if p.get(f + "_kind") not in KINDS:
                    fail.append("%s.%s: missing or invalid _kind" % (k, f))
        rm = p.get("runway_months")
        if not isinstance(rm, (int, float)) or rm <= 0:
            fail.append("%s: runway_months must be positive, got %r" % (k, rm))
        elif burn:
            if k == "A_revenue_financed":
                rev = p.get("revenue_usd_per_month")
                if not isinstance(rev, (int, float)):
                    fail.append("%s: revenue_usd_per_month required" % k)
                elif rev < burn and rm > 24:
                    fail.append("%s: revenue %r below burn %r cannot fund %r months" % (k, rev, burn, rm))
            else:
                raise_usd = p.get("raise_usd")
                if not isinstance(raise_usd, (int, float)):
                    fail.append("%s: raise_usd required" % k)
                elif abs(rm - raise_usd / burn) > 0.5:
                    fail.append("%s: runway_months %r != raise %r / burn %r" % (k, rm, raise_usd, burn))
        if p.get("milestone_reached") not in RUNGS:
            fail.append("%s: milestone_reached must be a G1.45 rung key, got %r"
                        % (k, p.get("milestone_reached")))
        for f in ("dilution_pct", "board_seats_transferred"):
            if f not in p: fail.append("%s: missing %s" % (k, f))
        if not isinstance(p.get("protective_provisions"), list):
            fail.append("%s: protective_provisions must be a list" % k)
    if d.get("decision") is not None: fail.append("decision must be null")
    if d.get("human_decision_required") is not True: fail.append("human_decision_required must be true")
    rec = d.get("recommendation", {})
    if rec.get("path") not in PATHS: fail.append("recommendation.path must be one of the three keys")
    if not str(rec.get("because", "")).strip(): fail.append("recommendation.because is empty")
    topics = {x.get("topic") for x in d.get("disclosures", [])}
    if "bliss_regulatory_exposure" not in topics:
        fail.append("disclosures missing bliss_regulatory_exposure")
    print("paths=%d burn=%r recommendation=%s decision=%r"
          % (len(paths), burn, rec.get("path"), d.get("decision")))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    paths=3 burn=9500 recommendation=A_revenue_financed decision=None
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  paths=3  AND  decision=None

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because the runway arithmetic
is re-derived from a single shared burn figure the paths cannot each redefine, every milestone must
be a key from the separately produced ladder, every number must be labelled, and the artifact fails
if it records a decision or omits the regulatory disclosure.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — build the three paths from a single burn and the ladder milestones
   -> if still failing, MANDATORY approach change. Adjusting raise sizes is NOT an approach
      change; switching to a milestone-first construction (fix the milestone each path must
      reach, then solve for the capital that reaches it) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: no path's runway reaches first_paying_customer (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.54/capital_strategy.json`

A reader finds: `generated_at`, `monthly_burn_usd` with its `_kind` and a `burn_breakdown` naming
the compute term traced to `unit_economics.json`, `paths` (three, each with `key`, `raise_usd` or
`revenue_usd_per_month`, `runway_months`, `milestone_reached`, `dilution_pct`,
`board_seats_transferred`, `protective_provisions`, `preconditions`, and `what_it_costs_in_control`),
`disclosures`, `recommendation`, `decision: null`, and `human_decision_required: true`. Alongside
it, `docs/PROMPTS/artifacts/B2/G1.54/packet.md` — one screen for the human, ending in the four
named choices.

## 9. Definition of NOT done

- Each path uses its own burn assumption, so the three are not comparable and the cheapest-looking
  path is simply the one with the most optimistic burn.
- `milestone_reached` is written as prose ("early traction") rather than as one of the five ladder
  rung keys, so no path can be checked against the revenue plan.
- The priced-equity path omits that a single-member Nevada LLC still forming cannot take the
  investment without a conversion, and states no cost or delay for it.
- The Bliss regulatory exposure is omitted because it is uncomfortable. It is a diligence finding
  regardless, and omitting it from the founder's own plan means discovering it during diligence.
- The artifact recommends an investment action to a reader. This is the founder's own financing
  plan, not advice; the recommendation names a path for the founder and nothing else.
- The artifact records a chosen path. Any raise is a human decision.
- Dilution is given without protective provisions or board seats, so "20%" reads cheap while the
  control transfer is invisible.

---

---
id: G1.55
title: Decision-rights charter enforced by a CODEOWNERS file that matches real paths
workload: W6
workload_secondary: [W5]
phase: G1
depends_on: [G1.53, G1.52]
blocks: []
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: .github/CODEOWNERS
escalation: >
  If a decision class cannot be attached to any path pattern that matches tracked files, STALL
  rather than inventing a pattern. An unmatched CODEOWNERS rule is silently dead and gives false
  assurance that a review gate exists.
status: DRAFT
notes: >
  The charter without CODEOWNERS is a wish; CODEOWNERS without the charter is a list of names.
  The item requires both, and the verifier resolves every pattern against the tracked file set.
---

## 1. Objective

A `CODEOWNERS` file exists in which every ownership pattern matches at least one tracked file, and
a companion charter names each decision class, who decides it, who must be consulted, and what
happens on disagreement. Adding the second engineer does not require re-deciding who owns what.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0; say source-available. Physics is **Avian**, never Rapier. `.slint` compiles to Rust,
so Slint **is** Rust. Units are meter-native.

**Current state.** `.github/` contains exactly two entries: `FUNDING.yml` and `workflows/`. There is
**no** `CODEOWNERS` file. Every decision is currently made by one person, so the cost of the
missing file is zero today and unbounded the day it is not.

**The coherence risks a charter must actually cover**, each real and cited:

- **One file carries the studio.** `eustress/crates/engine/src/ui/slint_ui.rs` is 23,103 lines and
  every panel's drain logic funnels through it. It is both the largest structural liability and the
  file a second engineer is most likely to touch first.
- **One `target/`, one build at a time.** The workspace shares a single `target/` directory;
  concurrent cargo builds produce link failures. This is a scheduling decision, not a preference,
  and it needs a named owner once two people build.
- **One physics engine.** Avian. `eustress/crates/common/src/physics/` and
  `eustress/crates/engine/src/physics/`.
- **Generated files that must not be hand-edited.**
  `eustress/crates/engine/src/tool_metadata.rs` line 1 reads "GENERATED by
  scripts/gen_tool_metadata.py — do not edit by hand." A second engineer editing it by hand loses
  the change on the next regeneration.
- **The mode manifests are the source of truth for the tool surface.**
  `eustress/crates/engine/modes/*.toml` — ten files, from which the generated metadata derives.
- **Money-touching code.** `eustress/crates/backend/`, `eustress/crates/bliss/`,
  `eustress/crates/identity/`, `infrastructure/cloudflare/`.
- **Licence and contribution terms.** `LICENSE`, `LICENSE-COMMERCIAL.md`, `CONTRIBUTING.md`.
- **CI.** `.github/workflows/` — and the standing rule that CI is never modified to make a gate
  pass.

**Prerequisite.** `G1.53` produced `docs/PROMPTS/artifacts/B2/G1.53/hire_evaluations.json` with
four role keys: `physics_simulation`, `agent_systems`, `distributed_systems`, `enterprise_gtm`. The
charter's owners should be expressed in terms of those roles plus `founder`, so it is meaningful
before anyone is hired.

**How CODEOWNERS matching works, for the purposes of this item.** Patterns are gitignore-style. To
keep the charter verifiable, this item restricts patterns to two shapes only: a directory prefix
ending in `/` (for example `eustress/crates/backend/`) or an exact file path (for example
`LICENSE`). No wildcards, no negations. The verifier resolves every pattern against
`git ls-files` and fails any pattern matching zero tracked files.

**Owners.** GitHub CODEOWNERS requires a handle or team. Since the project is a single operator,
use a placeholder handle consistently and record the mapping from handle to role in the charter.
Do not invent handles for people who do not exist.

## 3. Scope

### In scope — files this item may edit
- `.github/CODEOWNERS` — create it
- `docs/PROMPTS/artifacts/B2/G1.55/` — create it; the decision-rights charter and the transcript

### Out of scope — do not edit
- Anything under `.github/workflows/` — CODEOWNERS is not a workflow and is permitted; the workflow
  files themselves are not
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source, all `.slint` files, all `.toml` mode manifests
- `LICENSE`, `LICENSE-COMMERCIAL.md`, `CONTRIBUTING.md`, `README.md`
- `docs/PROMPTS/artifacts/B2/G1.53/**` — read-only input

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Broadening a pattern to `*`
  so it matches everything, or dropping a decision class because no pattern fits it, are
  measurement changes.
- Patterns are directory prefixes ending in `/` or exact file paths. Nothing else.
- Every decision class in the charter maps to at least one CODEOWNERS pattern, and every pattern in
  CODEOWNERS appears in the charter. The mapping is total in both directions.
- Each decision class states `decides`, `consulted`, and `on_disagreement` — the last must name a
  concrete tie-break, not "discuss".
- Do not add a review-enforcement workflow. Enforcement is a separate decision.

## 5. Exit criterion

### Criterion

`.github/CODEOWNERS` exists with **at least 8** rules; **every** rule's pattern matches at least
one file in `git ls-files`; `decision_rights.json` declares **at least 6** decision classes; the
pattern sets in the two files are identical; and every decision class has non-empty `decides`,
`consulted`, and `on_disagreement`.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, subprocess, sys
    CO = ".github/CODEOWNERS"
    CH = "docs/PROMPTS/artifacts/B2/G1.55/decision_rights.json"
    tracked = subprocess.run(["git", "ls-files"], capture_output=True, text=True).stdout.splitlines()
    fail = []
    rules = []
    for ln in open(CO, encoding="utf-8"):
        s = ln.strip()
        if not s or s.startswith("#"): continue
        parts = s.split()
        if len(parts) < 2: fail.append("rule has no owner: %r" % s); continue
        pat, owners = parts[0], parts[1:]
        if "*" in pat or pat.startswith("!"):
            fail.append("pattern must be a directory prefix or exact path, got %r" % pat); continue
        if not all(o.startswith("@") for o in owners):
            fail.append("owners must be @handles, got %r" % owners)
        if pat.endswith("/"):
            n = sum(1 for f in tracked if f.startswith(pat))
        else:
            n = sum(1 for f in tracked if f == pat)
        if n == 0: fail.append("pattern matches zero tracked files: %r" % pat)
        rules.append(pat)
    if len(rules) < 8: fail.append("only %d CODEOWNERS rules, need >= 8" % len(rules))
    d = json.load(open(CH, encoding="utf-8"))
    classes = d["decision_classes"]
    if len(classes) < 6: fail.append("only %d decision classes, need >= 6" % len(classes))
    charter_pats = set()
    for c in classes:
        for f in ("decides", "consulted", "on_disagreement"):
            if not str(c.get(f, "")).strip(): fail.append("%s: empty %s" % (c.get("key"), f))
        ps = c.get("patterns", [])
        if not ps: fail.append("%s: no patterns" % c.get("key"))
        charter_pats.update(ps)
    if charter_pats != set(rules):
        fail.append("pattern sets differ; only_in_charter=%r only_in_codeowners=%r"
                    % (sorted(charter_pats - set(rules)), sorted(set(rules) - charter_pats)))
    print("codeowners_rules=%d decision_classes=%d tracked_files=%d"
          % (len(rules), len(classes), len(tracked)))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    codeowners_rules=11 decision_classes=7 tracked_files=4183
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  codeowners_rules >= 8  AND  decision_classes >= 6

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because every pattern is
resolved against the actual tracked file list, wildcards are rejected so a single `*` cannot
manufacture coverage, and the charter and the CODEOWNERS file must agree exactly in both
directions.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — one decision class per subsystem, patterns from the crate layout
   -> if still failing, MANDATORY approach change. Adding patterns is NOT an approach change;
      switching to a risk-first decomposition (start from the coherence risks in section 2 and
      derive the smallest pattern set that covers each) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: a decision class has no pattern that matches tracked files
```

## 8. Artifact

`.github/CODEOWNERS`

A reader finds an ownership rule per decision class, each a directory prefix or an exact path, each
with at least one `@handle`. Alongside it,
`docs/PROMPTS/artifacts/B2/G1.55/decision_rights.json` — `decision_classes`, each with `key`,
`what_is_being_decided`, `patterns`, `decides`, `consulted`, `on_disagreement`, and
`why_it_matters`; plus `handle_to_role`, mapping each CODEOWNERS handle to a role key from
`G1.53` or to `founder`. And `docs/PROMPTS/artifacts/B2/G1.55/charter.md`, the one-page version a
new engineer reads.

## 9. Definition of NOT done

- CODEOWNERS contains `* @founder`, which technically matches everything and decides nothing. The
  verifier rejects wildcards.
- A rule points at `eustress/crates/engine/src/ui/` but the charter never names the 23,103-line
  `slint_ui.rs` as its own decision class, so the single riskiest file has no explicit owner.
- `on_disagreement` says "escalate to the founder" for every class, which is true today and useless
  the moment the founder is one of the disagreeing parties on a technical call.
- Handles are invented for people who have not been hired, so the file reads as a fiction rather
  than as a plan expressed in roles.
- The generated file `eustress/crates/engine/src/tool_metadata.rs` is owned but the charter does not
  record that it is generated and must not be hand-edited, which is the specific mistake a new
  engineer makes.
- A review-enforcement workflow is added so CODEOWNERS is binding. Workflows are out of scope.

---

---
id: G1.56
title: Operator automation baseline with a measured wall-clock reduction
workload: W6
workload_secondary: [W3]
phase: G1
depends_on: [G1.40]
blocks: []
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G1.56/operator_leverage.json
escalation: >
  If the automated task set cannot reach a 40% total wall-clock reduction without including a cargo
  build in the "before" measurement, STALL. Padding the baseline with compile time makes any
  automation look effective and teaches the wrong lesson about where the founder's hours go.
status: DRAFT
notes: >
  The verifier re-runs each automated task and times it itself, so the "after" numbers cannot be
  self-reported. Builds are budgeted but this item should consume none; the six slots exist only in
  case a task genuinely needs one.
---

## 1. Objective

Six recurring operator tasks are automated behind one runnable entry point, and the total wall-clock
to perform all six drops by at least 40% against a timed manual baseline. The "after" timings are
re-measured by the verifier rather than reported by the agent.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0; say source-available. Physics is Avian. Slint is Rust. Units are meter-native.

**The operator constraint this item exists to relieve.** One founder, Windows, 10–15 minute
serialized builds, one build at a time (the workspace shares a single `target/` and concurrent
builds produce link failures). `docs/AUDIT/12_INFRASTRUCTURE.md` Feature 1 records that the desktop
engine is not built in CI, so the founder is the entire regression surface, and every verification
cycle is a serialized compile.

**The six fixed task keys** — the verifier requires exactly these, and each must be implemented as a
subcommand:

| key | what it must produce |
|---|---|
| `tool_surface_counts` | per-mode tool id counts and wired counts, joining `eustress/crates/engine/modes/*.toml` against `eustress/crates/engine/src/tool_metadata.rs` |
| `workspace_members` | the workspace member list from `eustress/Cargo.toml` with, for each, whether the crate directory exists |
| `ci_gate_summary` | what each file under `.github/workflows/` actually runs, extracted from the workflow YAML |
| `audit_open_gaps` | the count of files under `docs/AUDIT/` and, for each, its declared status line |
| `telemetry_outbox_state` | whether a telemetry outbox directory exists under the local app data path and how many files it holds, read-only |
| `contributor_rungs` | per-author commit counts from `git log`, the same substrate `G1.51` uses |

**Why these six and not others.** Each is a question the founder actually re-answers by hand,
each is answerable from files already in the tree, and none requires the engine to run — so the
baseline measures thinking and typing, not compiling. That is the whole point: automating a compile
does not make a compile faster.

**Grounding facts you can check your implementations against**, measured 2026-08-06:
`eustress/crates/engine/src/tool_metadata.rs` holds **1999** tool ids of which **30** are
`wired: true`; `government` declares **563** ids with **1** wired, `civil` **246 / 1**,
`engineering` **174 / 22**, `business` **155 / 19**, `student` **238 / 11**, `health` **159 / 13**,
`justice` **92 / 5**, `legal` **240 / 6**, `military` **169 / 7**, `gaming` **22 / 4**.
`git rev-list --count HEAD` returns **483** across five distinct author names. `docs/AUDIT/`
contains 20 numbered files plus `MASTER.md` and `TRIAGE_2026-05-22.md`. If your implementation
disagrees with these, your implementation is wrong.

**Read-only rule for the telemetry task.** `eustress/crates/engine/src/usage_telemetry.rs:187`
resolves the telemetry directory as `dirs::data_local_dir()/Eustress/telemetry` with an `outbox`
subdirectory (`:206`). The task counts files there. It must never write, move, or delete anything
in that tree.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile. This
item should consume zero builds.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G1.56/` — create it; the runner, the timings, the transcripts

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- All Rust source
- `scripts/` — do not modify the existing generator scripts
- Anything under the local app data telemetry tree — read-only, always

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Including a cargo build in
  the manual baseline, dropping a task from the six, or making a subcommand return a cached result
  instead of recomputing are all measurement changes.
- Each subcommand must recompute from source every run. No caching.
- The manual baseline must be recorded per task with a transcript showing what was actually done by
  hand. A baseline asserted without a transcript fails.
- The runner is a single Python entry point with a `--task <key>` argument and a `--list` argument.
  It must not shell out to `cargo`.
- Nothing may be written outside the artifact directory.

## 5. Exit criterion

### Criterion

`docs/PROMPTS/artifacts/B2/G1.56/ops.py --list` prints exactly the six fixed task keys; each of the
six runs successfully when timed by the verifier; the sum of verifier-measured task times is **at
most 60%** of the recorded manual baseline total; and every task's `before_seconds` is backed by a
transcript file that exists.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, os, subprocess, sys, time
    R = "docs/PROMPTS/artifacts/B2/G1.56/ops.py"
    T = "docs/PROMPTS/artifacts/B2/G1.56/operator_leverage.json"
    KEYS = ["tool_surface_counts","workspace_members","ci_gate_summary",
            "audit_open_gaps","telemetry_outbox_state","contributor_rungs"]
    fail = []
    lst = subprocess.run([sys.executable, R, "--list"], capture_output=True, text=True)
    if lst.returncode != 0: fail.append("--list exited %d" % lst.returncode)
    printed = [l.strip() for l in lst.stdout.split() if l.strip()]
    if sorted(printed) != sorted(KEYS):
        fail.append("--list must print exactly %r, got %r" % (KEYS, printed))
    d = json.load(open(T, encoding="utf-8"))
    tasks = {t["key"]: t for t in d["tasks"]}
    before_total = 0.0; after_total = 0.0
    for k in KEYS:
        t = tasks.get(k)
        if t is None: fail.append("timings missing task %r" % k); continue
        b = t.get("before_seconds")
        if not isinstance(b, (int, float)) or b <= 0:
            fail.append("%s: before_seconds must be positive, got %r" % (k, b)); continue
        tr = t.get("baseline_transcript")
        if not tr or not os.path.exists(tr):
            fail.append("%s: baseline_transcript missing: %r" % (k, tr))
        before_total += b
        t0 = time.time()
        r = subprocess.run([sys.executable, R, "--task", k], capture_output=True, text=True)
        el = time.time() - t0
        if r.returncode != 0:
            fail.append("%s: subcommand exited %d: %s" % (k, r.returncode, r.stderr[:200])); continue
        if not r.stdout.strip():
            fail.append("%s: subcommand produced no output" % k)
        after_total += el
    if before_total > 0:
        ratio = after_total / before_total
        print("before_total_s=%.1f after_total_s=%.1f ratio=%.3f" % (before_total, after_total, ratio))
        if ratio > 0.60:
            fail.append("ratio %.3f > 0.60 (need >= 40%% reduction)" % ratio)
    else:
        fail.append("no usable baseline total")
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    before_total_s=1860.0 after_total_s=14.3 ratio=0.008
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  ratio <= 0.60

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is unusually tight because the verifier
executes every subcommand and times it itself, so the "after" figure cannot be reported; the task
set is fixed so it cannot be trimmed to the easy ones; and each baseline requires a transcript on
disk.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — one Python module per task behind a single dispatcher
   -> if still failing, MANDATORY approach change. Optimising a subcommand is NOT an approach
      change; changing what a task produces so it answers the founder's actual question in one
      pass (for example, emitting a single combined status report rather than six reports) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations where the number of subcommands exiting 0 does not rise
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: 40% reduction unreachable without a build in the baseline (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G1.56/operator_leverage.json`

A reader finds: `generated_at`, `hardware`, and `tasks` — six objects, each with `key`,
`what_it_answers`, `before_seconds`, `baseline_transcript`, `after_seconds_reported`,
`what_was_automated`, and `still_manual` (the residue the automation does not cover). Alongside it:
`docs/PROMPTS/artifacts/B2/G1.56/ops.py` (the runner) and
`docs/PROMPTS/artifacts/B2/G1.56/baselines/*.txt` (one transcript per task).

## 9. Definition of NOT done

- The baseline includes a 12-minute engine build, so the ratio looks spectacular while none of the
  founder's actual thinking time was recovered.
- A subcommand caches its previous output, so the verifier's re-run is fast and measures nothing.
- `tool_surface_counts` disagrees with the grounding numbers in §2, which means the join is wrong
  and every downstream count the founder trusts would be wrong too.
- The telemetry task writes to or clears the outbox. It is read-only, always.
- Baselines are asserted with no transcript, so nobody can tell whether the manual task was ever
  performed.
- Five tasks are automated and the sixth is declared out of scope. The set is fixed at six.
- The runner shells out to `cargo`, reintroducing the build serialisation this item exists to route
  around.

---

---
id: G6.40
title: Unwired-button sweep with a fully wired demo path for the sold vertical
workload: W4
workload_secondary: [W1]
phase: G6
depends_on: [G1.41, G1.47]
blocks: [G2.40]
tier: M
token_envelope: 200000
wallclock_envelope: 4h
max_builds: 6
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G6.40/demo_path.json
escalation: >
  If fewer than 12 wired tool ids exist in the selected vertical's mode, STALL. The demo path cannot
  be assembled, which means the vertical cannot be demonstrated at all and G1.41's selection must be
  revisited before anything else is built.
status: DRAFT
notes: >
  Phase G6 because this is the sold-surface honesty class, and it depends on the studio surface
  rather than on any business document. The item does not wire anything; it establishes which
  buttons may appear in a buyer-facing session and proves every one of them dispatches.
---

## 1. Objective

The set of ribbon tool ids a buyer will touch in a demonstration of the selected vertical is
enumerated, and every id in it is verified to dispatch — no dream buttons on the demo path. The
full unwired count for that mode is recorded alongside, so the gap is documented rather than
hidden.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0; say source-available. Physics is Avian. Slint is Rust. Units are meter-native.

**The dream-button model, as implemented.** `eustress/crates/engine/src/tool_metadata.rs` is
generated by `scripts/gen_tool_metadata.py` and must not be hand-edited (its line 1 says so). It
declares a `ToolMeta` per ribbon tool id with a `wired: bool` field, documented at `:16-21`: "True
when clicking this actually does something today. False = a deliberate 'dream' button: it renders
fully but has no dispatch arm, so the UI must say so honestly and the click is counted as demand."
The metadata table begins at `:148`. Mode manifests live in `eustress/crates/engine/modes/*.toml`
— ten files.

**Measured 2026-08-06, by the join script in §5:** across all metadata, **1999** tool ids of which
**30** are `wired: true`. Per mode, ids / wired: `government` 563 / 1, `civil` 246 / 1, `legal`
240 / 6, `student` 238 / 11, `engineering` 174 / 22, `military` 169 / 7, `health` 159 / 13,
`business` 155 / 19, `justice` 92 / 5, `gaming` 22 / 4.

**Why this matters commercially.** `docs/architecture/GOVERNMENT_MODE.md:713-714` records that of
the declared government tool ids exactly one, `data:import`, has a real handler, and the document
states the mode "must never be described as working software". A buyer clicking an unwired button
in a live demonstration ends the deal, and the two verticals with existing go-to-market assets are
precisely the two with the worst wired ratios.

**What this item does not do.** It does not wire anything. Wiring 500 handlers is not a business
item and would take months. It establishes the demo path — the ids a buyer-facing session is
permitted to touch — and proves every one of them dispatches.

**Prerequisites.** `docs/PROMPTS/artifacts/B2/G1.41/vertical_selection.json` names the selected
vertical, which determines the mode. `docs/PROMPTS/artifacts/B2/G1.47/health_score.json` records
that a rising ratio of unwired clicks is the churn leading indicator, which is the same signal in
its post-sale form.

**Build reality.** 10–15 minutes per engine build, one at a time, never killed mid-compile. This
item needs no build: the join is a static analysis of two file sets in the tree.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G6.40/` — create it; the demo path, the sweep output, the script

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- `eustress/crates/engine/src/tool_metadata.rs` — it is generated; hand-editing it to flip a
  `wired` flag is the single worst failure available in this item
- `scripts/gen_tool_metadata.py` and `eustress/crates/engine/modes/*.toml`
- All other Rust source and all `.slint` files
- `docs/PROMPTS/artifacts/B2/G1.41/**` and `.../G1.47/**` — read-only inputs

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Flipping a `wired` flag,
  editing a mode manifest to remove unwired ids, or defining the demo path as "whatever is wired"
  without regard to whether those ids form a coherent buyer session are all measurement changes.
- The demo path must be a **session**: an ordered list of steps a buyer is walked through, each
  step naming the tool id it uses and what the buyer sees. A bag of wired ids is not a demo path.
- Every id in the demo path must appear in the selected mode's manifest **and** be `wired: true`.
- Record the full mode counts honestly, including the unwired total, in the same artifact.

## 5. Exit criterion

### Criterion

`demo_path.json` names the selected vertical's mode; its `steps` contain **at least 12** distinct
tool ids; **every** one of them is present in that mode's manifest and is `wired: true` in
`tool_metadata.rs`; the artifact's recorded `mode_totals` match the values the verifier
recomputes; and every step has a non-empty `what_the_buyer_sees`.

### Measurement

Run from the repository root, in Git Bash. This is the same join used to produce the counts in §2.

Command:

    python - << 'PYEOF'
    import json, re, sys
    P = "docs/PROMPTS/artifacts/B2/G6.40/demo_path.json"
    META = "eustress/crates/engine/src/tool_metadata.rs"
    d = json.load(open(P, encoding="utf-8"))
    mode = d["mode"]
    meta_src = open(META, encoding="utf-8").read()
    wired = {m.group(1): (m.group(2) == "true") for m in
             re.finditer(r'"([a-z0-9_]+:[a-z0-9_.-]+)"\s*=>\s*\(.*?,\s*(true|false)\)', meta_src)}
    man = open("eustress/crates/engine/modes/%s.toml" % mode, encoding="utf-8").read()
    ids = set(re.findall(r'"([a-z0-9_]+:[a-z0-9_.-]+)"', man))
    n_ids = len(ids); n_wired = sum(1 for i in ids if wired.get(i))
    fail = []
    steps = d["steps"]
    used = []
    for s in steps:
        tid = s.get("tool_id")
        if not tid: fail.append("step %r has no tool_id" % s.get("n")); continue
        used.append(tid)
        if tid not in ids: fail.append("%s: not declared in %s.toml" % (tid, mode))
        elif not wired.get(tid): fail.append("%s: DREAM BUTTON on the demo path" % tid)
        if not str(s.get("what_the_buyer_sees", "")).strip():
            fail.append("%s: empty what_the_buyer_sees" % tid)
    distinct = len(set(used))
    if distinct < 12: fail.append("only %d distinct tool ids on the demo path, need >= 12" % distinct)
    tot = d.get("mode_totals", {})
    if tot.get("ids") != n_ids or tot.get("wired") != n_wired:
        fail.append("mode_totals %r != recomputed {'ids': %d, 'wired': %d}" % (tot, n_ids, n_wired))
    print("mode=%s ids=%d wired=%d unwired=%d demo_path_distinct=%d"
          % (mode, n_ids, n_wired, n_ids - n_wired, distinct))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    mode=engineering ids=174 wired=22 unwired=152 demo_path_distinct=14
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  demo_path_distinct >= 12  AND  zero DREAM BUTTON
    lines in the failure list

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because it re-derives both the
mode's tool set and every wired flag from the tree at verification time, so an artifact recording
friendly totals fails, and any unwired id on the demo path fails by name.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — build the session from the mode's wired ids
   -> if still failing, MANDATORY approach change. Substituting one wired id for another is NOT
      an approach change; restructuring the session around a different buyer question, or
      drawing on wired ids shared across modes rather than mode-specific ones, is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with demo_path_distinct unchanged
  - Budget      : 300k tokens or 9 builds consumed (150% of the M envelope)
  - Item-specific: fewer than 12 wired ids exist in the selected mode (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G6.40/demo_path.json`

A reader finds: `generated_at`, `vertical`, `mode`, `mode_totals` (`ids`, `wired`, `unwired`),
`steps` (ordered, each with `n`, `tool_id`, `what_the_buyer_sees`, `what_it_proves`), and
`off_path_risks` — the unwired ids most likely to be clicked accidentally during a demonstration,
with what the buyer would experience. Alongside it,
`docs/PROMPTS/artifacts/B2/G6.40/sweep.json` — the full per-mode join for all ten modes, which is
the number the company quotes internally about its own surface.

## 9. Definition of NOT done

- A `wired` flag is flipped in `tool_metadata.rs` so an id qualifies. The file is generated, the
  flag would be lost on regeneration, and the button still would not dispatch.
- The demo path is a list of twelve wired ids with no ordering and no statement of what the buyer
  sees, so it cannot actually be performed.
- The mode totals are copied from this prompt rather than recomputed, and the verifier's
  recomputation disagrees.
- `off_path_risks` is omitted, so the demonstration has a scripted happy path and no answer for the
  buyer who clicks something else.
- The selected mode is quietly swapped for one with a better wired ratio, contradicting `G1.41`.
- The sweep records only the selected mode, so the company still has no honest internal number for
  its whole surface.

---

---
id: G2.40
title: Claim-to-evidence map for the sold vertical, with every unsupported claim struck
workload: W4
workload_secondary: [W3]
phase: G2
depends_on: [G1.41, G6.40, G1.46]
blocks: []
tier: S
token_envelope: 60000
wallclock_envelope: 1h
max_builds: 0
critic_gate: []
capture_recipe: none
artifact: docs/PROMPTS/artifacts/B2/G2.40/claim_evidence_map.json
escalation: >
  If more than half of the extracted claims must be struck, STALL. That is not a copy problem, it is
  a product-claim problem, and the human must decide whether to change the sales surface or change
  the product before anything is rewritten.
status: DRAFT
notes: >
  Phase G2 because a claim about simulation is only as good as the numerical trust behind it, and
  this item is where a claim is either attached to a measured artifact or removed. It reports; it
  never edits an external-facing file.
---

## 1. Objective

Every substantive claim in the project's buyer-facing surfaces is extracted verbatim, checked
against the repository, and marked supported, reworded, or struck — with supported claims carrying
a path to the evidence. The human receives a list of exactly which sentences may be said to a
buyer.

## 2. Context you need (self-contained)

**What Eustress is.** An AI-native simulation substrate — never a game engine. Licence is PolyForm
Shield 1.0.0 (`LICENSE`), dual-licensed against `LICENSE-COMMERCIAL.md`; say source-available.
Physics is Avian. Slint is Rust. Units are meter-native.

**The buyer-facing surfaces to extract from.** All four exist and all four are in scope for
extraction (and none for editing):
- `docs/marketing/UofA_Center_For_Innovation_Pilot.html`
- `README.md`
- `docs/architecture/GOVERNMENT_MODE.md`
- `docs/architecture/EUSTRESS_FORGE.md`

**Claims already known to be unsupported**, so their verdicts are not in doubt:
- `docs/marketing/UofA_Center_For_Innovation_Pilot.html` contains the literal string
  `Open source · MIT-friendly licensing`. `LICENSE` is PolyForm Shield 1.0.0, which is not
  OSI-approved. **Struck.**
- `docs/architecture/EUSTRESS_FORGE.md` contains an "80–90% cost reduction" claim. It is unmeasured
  marketing copy, and `infrastructure/forge/` contains only `README.md`, `nomad`, `scripts`, and
  `terraform` — there is no Consul directory, Vault is not deployed, and there is no monitoring.
  **Struck** unless a measurement exists, and none does.
- The 2.10M-entity figure at `docs/AUDIT/05_SPACE_STREAMING.md:23` is an `active_cap`
  **CONFIG DEFAULT**, not a measurement. Any claim resting on it is **struck or reworded**, never
  supported.
- Any claim that a mode's tool surface works. `docs/architecture/GOVERNMENT_MODE.md:713-714`
  records one real handler among the declared government tool ids, and the document says the mode
  "must never be described as working software".

**Claims that are genuinely supportable, with their evidence.**
- The usage-telemetry pipeline is live end to end: `eustress/crates/engine/src/usage_telemetry.rs`
  posts a session aggregate to `https://api.eustress.dev/api/telemetry/usage` (`:193`), and the
  Worker lives at `infrastructure/telemetry-worker/`.
- Telemetry is opt-out and aggregate-only: same file, gated on `enabled`, per-click timing never
  leaves the machine.
- The extension surface is real: `eustress/crates/engine/src/script_plugin_host.rs` loads `.lua`
  and `.rune` plugins from a user directory with no Rust recompile, and `G1.50` proves it if it has
  run.
- The demo path dispatches: `docs/PROMPTS/artifacts/B2/G6.40/demo_path.json` from the prerequisite
  item.

**The publication rule that binds this item.** `00_MASTER_PROTOCOL.md` §4.1 Rule R2: no
side-by-side comparison, screenshot, frame, video, or derived score naming a third-party product
may appear in any external surface unless the project holds publication rights, and only the human
may authorise a published comparison. If an extracted claim names a third-party product, its
verdict is `struck` and its `reason` must cite this rule. §6 additionally makes any external
publication human-only, which is why this item reports and never edits.

**Number discipline.** Every number in a supported claim must be `MEASURED` with a cited artifact,
`TARGET` labelled inline, or `CONFIG DEFAULT` labelled as such.

## 3. Scope

### In scope — files this item may edit
- `docs/PROMPTS/artifacts/B2/G2.40/` — create it; the claim map and the rewrite suggestions

### Out of scope — do not edit
- Anything under `.github/workflows/`
- `docs/PROMPTS/01_CRITIC_RUBRIC.md`
- Any capture already hashed into a provenance manifest
- **All four source surfaces.** `docs/marketing/**`, `README.md`,
  `docs/architecture/GOVERNMENT_MODE.md`, `docs/architecture/EUSTRESS_FORGE.md`. Editing an
  external-facing file is a human decision; this item produces the list, not the edit.
- All Rust source
- `docs/PROMPTS/artifacts/B2/G1.41/**` and `.../G6.40/**` — read-only inputs

## 4. Approach constraints

- **Changing the measurement instead of the artifact fails this item.** Paraphrasing a claim so it
  no longer matches the source file, or extracting only the easy claims, are measurement changes.
- Every claim's `literal` must appear byte-for-byte in its `source_path`. The verifier checks this.
- A `supported` verdict requires an `evidence_path` that exists and is code, configuration, or a
  measured artifact — never a design document.
- A `reworded` verdict requires a `replacement` string that is itself supportable, and an
  `evidence_path`.
- A `struck` verdict requires a `reason`.
- Extract from all four source files. At least one claim from each.

## 5. Exit criterion

### Criterion

`claim_evidence_map.json` contains **at least 15** claims drawn from **all four** source surfaces;
every claim's `literal` is present in its `source_path`; every `supported` and `reworded` claim
carries an `evidence_path` that exists; every `struck` claim carries a non-empty `reason`; and the
artifact's `summary.n_struck` equals the verifier's count.

### Measurement

Run from the repository root, in Git Bash.

Command:

    python - << 'PYEOF'
    import json, os, sys
    P = "docs/PROMPTS/artifacts/B2/G2.40/claim_evidence_map.json"
    SOURCES = {"docs/marketing/UofA_Center_For_Innovation_Pilot.html", "README.md",
               "docs/architecture/GOVERNMENT_MODE.md", "docs/architecture/EUSTRESS_FORGE.md"}
    d = json.load(open(P, encoding="utf-8"))
    claims = d["claims"]; fail = []
    if len(claims) < 15: fail.append("only %d claims, need >= 15" % len(claims))
    cache = {}
    seen_sources = set()
    n_struck = 0
    for c in claims:
        sp = c.get("source_path"); lit = c.get("literal", "")
        if sp not in SOURCES:
            fail.append("source_path must be one of the four surfaces, got %r" % sp); continue
        seen_sources.add(sp)
        if sp not in cache:
            cache[sp] = open(sp, encoding="utf-8", errors="replace").read()
        if not lit or lit not in cache[sp]:
            fail.append("literal not found in %s: %r" % (sp, lit[:60])); continue
        v = c.get("verdict")
        if v not in ("supported", "reworded", "struck"):
            fail.append("bad verdict %r for %r" % (v, lit[:40])); continue
        if v == "struck":
            n_struck += 1
            if not str(c.get("reason", "")).strip():
                fail.append("struck claim without reason: %r" % lit[:40])
        else:
            ep = c.get("evidence_path")
            if not ep or not os.path.exists(ep):
                fail.append("%s claim without existing evidence_path: %r" % (v, ep))
            if v == "reworded" and not str(c.get("replacement", "")).strip():
                fail.append("reworded claim without replacement: %r" % lit[:40])
    missing = SOURCES - seen_sources
    if missing: fail.append("no claims extracted from: %r" % sorted(missing))
    if d.get("summary", {}).get("n_struck") != n_struck:
        fail.append("summary.n_struck=%r but computed %d"
                    % (d.get("summary", {}).get("n_struck"), n_struck))
    print("claims=%d sources=%d struck=%d" % (len(claims), len(seen_sources), n_struck))
    print("RESULT: " + ("PASS" if not fail else "FAIL"))
    for f in fail: print("  - " + f)
    sys.exit(0 if not fail else 1)
    PYEOF
    echo "EXIT=$?"

Expected output shape:

    claims=19 sources=4 struck=7
    RESULT: PASS
    EXIT=0

Pass condition:

    EXIT=0  AND  "RESULT: PASS" present  AND  claims >= 15  AND  sources=4

## 6. Critic gate

`critic_gate: []`. The mechanical criterion replaces it and is tight because every claim must be
found byte-for-byte in the file it is attributed to, so a claim cannot be softened during
extraction; every affirmative verdict must resolve to a file on disk; and the struck count is
recomputed by the verifier so it cannot be understated.

## 7. Loop cadence and escalation

```
Iterations 1-3 : approach A — read each surface and extract its substantive assertions
   -> if still failing, MANDATORY approach change. Extracting more claims is NOT an approach
      change; switching to an evidence-first sweep (enumerate what the repository can actually
      prove, then find every sentence that asserts more than that) is.
Iterations 4-6 : approach B (fresh agent; receives A's failure signature, NOT A's diff)
   -> if still failing, MANDATORY approach change
Iterations 7-9 : approach C
   -> STALL.  Hard ceiling: 9 iterations, 3 approaches.

Early STALL triggers:
  - No-progress : 3 consecutive iterations with the verifier failure list unchanged
  - Budget      : 90k tokens consumed (150% of the S envelope)
  - Item-specific: more than half of the extracted claims are struck (see front matter)
```

## 8. Artifact

`docs/PROMPTS/artifacts/B2/G2.40/claim_evidence_map.json`

A reader finds: `generated_at`, `vertical`, `claims` (each with `source_path`, `literal`,
`verdict`, `evidence_path`, `replacement`, `reason`, and `number_kind` where the claim contains a
number), and `summary` with `n_supported`, `n_reworded`, `n_struck`, and
`external_publication_note` recording that every change to a source surface is a human decision.
Alongside it, `docs/PROMPTS/artifacts/B2/G2.40/rewrites.md` — for each reworded claim, the original
and the replacement side by side, ready for the human to apply or reject.

## 9. Definition of NOT done

- Claims are paraphrased during extraction, so the verifier cannot find them in the source and,
  more importantly, nobody can tell which sentence in the live asset is affected.
- The "Open source · MIT-friendly licensing" claim is marked `reworded` with a softer phrase
  instead of `struck`. The licence is not OSI-approved; there is no wording that makes the claim
  true.
- The "80–90% cost reduction" claim is marked supported by citing
  `docs/architecture/EUSTRESS_FORGE.md` itself. A document is not evidence for its own claim.
- Only the marketing asset is examined, so `README.md` keeps asserting a capability the map never
  tested.
- A source surface is edited to remove a struck claim. External publication is human-only; the item
  produces the list.
- Every claim is marked supported by pointing at a plausible-looking source file that exists but
  does not actually demonstrate the claim. The evidence path must be the thing that does the work,
  not the directory it lives in.
- Claims containing numbers are marked supported without a `number_kind`, so a `CONFIG DEFAULT`
  travels into a buyer conversation as a measurement.
