# Government Mode & the Civic Simulation Program

**Status:** Mode and Universe scaffolding **SHIPPED** 2026-07-26; the simulations themselves are
proposal. What exists: the `government` mode with twelve disciplines and 562 declared tool ids
([`modes/government.toml`](../../eustress/crates/engine/modes/government.toml)), free-camera
Space↔Space portals ([`portal.rs`](../../eustress/crates/engine/src/portal.rs)), and the **Tucson**
Universe — an abstract civic temple hub plus one Space per simulation, generated reproducibly by
[`scripts/gen_tucson_universe.py`](../../scripts/gen_tucson_universe.py). What does NOT exist: live
dispatch behind almost any of those 562 buttons, and every model in §5. See §9 for the line.
**Owner:** Engine Core
**Motivating use case:** a 2027 Tucson municipal candidacy running on a techno-libertarian
platform (smaller budget, lower taxes, automation, determinism, radical transparency), which
wants Eustress as its *convergence layer for Government*.
**Cross-refs:**
[`docs/convergence/PARAMETERS_CONVERGENCE_LAYER.md`](../convergence/PARAMETERS_CONVERGENCE_LAYER.md)
(the wire contract this mode would consume — the *same* dormant `parameters.rs` fabric, now with a
second domain asking for it),
[`docs/architecture/DATA_PLATFORM_PLAN.md`](DATA_PLATFORM_PLAN.md) (canonical for Datasets, Runs,
charts, and the collect→fit→simulate→compare loop this program is built on),
[`docs/LAW-ORDER.md`](../LAW-ORDER.md) §3 (the Privacy Line — binding on every simulation below),
[`docs/architecture/CANONICAL_SNAPSHOT_SPEC.md`](CANONICAL_SNAPSHOT_SPEC.md) and
[`docs/architecture/DECENTRALIZATION_PLAN.md`](DECENTRALIZATION_PLAN.md) (hash-anchored
reproducibility),
[`eustress/crates/engine/modes/justice.toml`](../../eustress/crates/engine/modes/justice.toml) and
[`civil.toml`](../../eustress/crates/engine/modes/civil.toml) (the two manifests this one is shaped
after — and deliberately does *not* absorb),
[`eustress/crates/engine/src/studio_modes.rs`](../../eustress/crates/engine/src/studio_modes.rs)
(the loader, allowlists, and registration point).

> **Executive summary** — A Government mode is worth building, but the mode is the *cheap* part.
> A ribbon with twelve disciplines is roughly a day of manifest authoring plus three generator
> runs; it changes nothing about what the engine can actually prove. What the campaign platform
> needs — and what Eustress cannot do today — is to **derive** its headline numbers instead of
> asserting them, from sourced public data, reproducibly, with an audit trail that survives a
> hostile reading. That capability rests on one dormant subsystem (`parameters.rs`, 976 lines,
> zero transport), one unlanded plan (Datasets as first-class nouns), and one property the engine
> has never had to guarantee (bit-reproducible runs anchored to a hash). This document specifies
> the ten universal requirements (§3), proposes the `government` mode with twelve disciplines and
> the manifest (§4), and lays out a sixteen-simulation program in which **every simulation is
> designed to be able to contradict the platform** (§5). §8 lists five places where the engine's own
> record suggests the platform is wrong, or asserts more than it can defend. §9 records what has
> actually shipped versus what remains proposal.

---

## 1. What the campaign actually needs from the engine

The campaign material in `Tucson-Campaign/` is unusually well-organized for this stage: sourced
landscape research, a seven-pillar platform, eight policy proposals carrying real numbers, roll-call
votes on the March and June 2025 camping ordinances, and a stated research rule — *"All research
cited with sources."* That rule is the single most important thing in the folder, because it is the
one commitment the engine can mechanize and opponents cannot.

Read as an engineering spec, the platform makes claims of exactly four kinds:

| Claim kind | Example from the platform | What it takes to defend |
|---|---|---|
| **Accounting** | "$2.4B budget, $17.8M deficit, 30% cut" | Line-item ingest + classification + arithmetic that reruns identically |
| **Throughput** | "permit time 90 → 30 days", "911 response 8 → 4 min" | Queueing / network models over real geometry |
| **Behavioral** | "homeless count −50% in 12 months", "ADUs by right → supply" | Stock-and-flow and elasticity models with stated, citable parameters |
| **Institutional** | "end no-bid contracts", "outcome-tied grants" | Contract/award data, vendor clustering, cost-per-outcome |

Accounting and throughput claims are *fully* defensible with deterministic models. Behavioral
claims are not, and never will be — the honest form is a range with named elasticities and a
sensitivity sweep. Institutional claims are mostly data-availability problems.

The platform currently treats all four kinds identically: as flat assertions with a number attached.
The whole value of routing this through Eustress is that the engine forces the distinction, and
publishes which is which.

## 2. The one thing the obvious build gets wrong

The obvious build is: add `government.toml` with a Mayor submode and a Council submode, wire a
budget chart, and put a live dashboard on `eustress.dev`. That produces a campaign prop. It will be
recognized as one — by the Star's editorial board, by the City Manager's budget office, and by any
opponent who asks the only question that matters:

> *"Can that thing ever tell you you're wrong?"*

If the answer is no, the tool is a slideshow with a physics engine attached, and it is worse than no
tool at all, because it converts "I have a plan" into "I have a plan and a fake machine that agrees
with it."

The load-bearing realization:

> **A civic simulator's value is exactly its capacity to falsify its operator. Eustress's edge
> here is not visualization — it is that the engine already has the three primitives a falsifiable
> public model needs and almost nothing else in civic tech has together: a CoW-branchable database
> (so a policy is a *branch*, and two branches diff), an experiment/run identity with
> `compare_runs` (so a claim is a *run*, and runs are reproducible), and a causal op-log (so every
> number has a traceable derivation). Government mode is the ribbon that points those at a city
> budget. The moat is provenance, not pixels.**

Design consequence, applied throughout §5: **every simulation ships with a stated falsification
condition** — a specific result that would mean the corresponding plank is wrong. A simulation
without one is not admitted to the program.

## 3. Universal engine requirements

These are needed for *any* government deployment, independent of which mode ships. Ordered by
blocker severity. "Current state" is grounded in the tree as of 2026-07-26.

### U1 — Parameters must acquire its first live producer · **HARD BLOCKER**

Every number the campaign cites originates outside Eustress: the FY2025/26 adopted budget, the CAFR,
the PIT count, TPD incident data, County Assessor parcels, Clerk campaign-finance filings, roll-call
minutes. Eustress must never become the system of record for any of it — that is settled doctrine in
[`PARAMETERS_CONVERGENCE_LAYER.md`](../convergence/PARAMETERS_CONVERGENCE_LAYER.md) §2, and it is
even more clearly right for government than for justice: municipal records-retention law, Arizona
public-records statute, and the Clerk's own authority all live elsewhere.

**Current state:** `crates/common/src/parameters.rs` is 976 lines of real, compiling, *inert* code.
`DataSourceType` carries ~55 variants and zero transport — there is no `match source_type { … =>
open connection }` anywhere in the workspace. `ParameterRouter::publish_export` /
`buffer_export` / `drain_pending` have no callers. `InstanceParameters` is a real
`#[derive(Component, Reflect)]` that the live spawn path never inserts. The
`parameter.changed` / `parameter.exports` stream topics are empty channels.

**Gap:** one concrete ingest path — CSV/JSON/Socrata-or-CKAN → provenance-tagged
`InstanceParameters` on a real entity — end to end, with the provenance surviving into the Dataset.
Government is the *second* domain to need this; that is the argument for finally building it rather
than specifying it a third time.

**Note:** Tucson does not currently publish a machine-readable checkbook-level budget. Realistic
first-wave ingest is PDF/XLSX from the adopted budget document plus the Clerk's campaign-finance
filings — which means the extraction layer needs a human-verification step and a "this line was
OCR'd, confidence 0.82" provenance flag. Plan for dirty input; the `pdf` skill and
`PDF_Tools` MCP already in this environment cover the mechanics.

### U2 — Datasets as first-class nouns · **HARD BLOCKER**

A budget is not a scene graph. A line item, a ward, a parcel, a position, an award, a roll-call vote
are all *records* — but they must still be entities, or MindSpace graphs, gizmo selection,
`DataSelection`, and the Properties inspector don't work on them and the whole "one polymorphic
inspector" discipline breaks.

**Current state:** [`DATA_PLATFORM_PLAN.md`](DATA_PLATFORM_PLAN.md) already specifies exactly this
(`Dataset`/`Series`/`Column`/`Run` + `datasets`/`timeseries` Fjall partitions hydrated into an
`eustress-data` Frame). The `eustress-data` crate exists and Polars/Arrow has been on by default
since 2026-06-23. What has *not* landed is the substrate itself and the linked-view discipline.

**Gap:** land the Dataset noun. Government mode is the forcing function — it is ~95% record work
and ~5% geometry, the inverse of every mode shipped so far, and it will surface every place the
engine still assumes "entity implies mesh."

### U3 — Provenance and citation as a required attribute, not a convention

The campaign's own rule is that every claim carries a source. In the engine that must be a
*schema constraint*: every ingested value carries `{source_uri, retrieved_at, extraction_method,
content_hash, confidence}`, and any number rendered in the UI can be right-clicked to its source.
A derived number carries the DAG of inputs that produced it.

**Current state:** the causal op-log is live (durable, tx-ordered mutation stream). Parameters
has sensitivity/handling flags but they are explicitly **advisory metadata only**
(`PARAMETERS_CONVERGENCE_LAYER.md` §2). There is no provenance type and no "cite this number"
affordance anywhere in the UI.

**Gap:** a `Provenance` component + an inspector row. Cheap to build, and it is the single
highest-leverage credibility feature in this entire document.

### U4 — Deterministic, reproducible runs anchored to a hash

"Automation and determinism reigns supreme" is a governance claim, not an aesthetic one. Its
operational meaning: *the same inputs, seed, and engine version produce a bit-identical result, and
anyone can verify that independently.* Without it, a budget model is an opinion in a nicer font.

**Current state:** `CANONICAL_SNAPSHOT_SPEC.md` exists; the `bliss` crate (with `bliss-core` and
`bliss-crypto`) is real, with a tracker and witness ledger. `run_experiment` / `compare_runs` /
`await_simulation` exist as MCP tools. `Date.now()`/`Math.random()` are already banned in workflow
scripts precisely to preserve replay.

**Gap:** a declared reproducibility contract for civic models — seed discipline, no wall-clock, no
un-pinned float nondeterminism in reduction order, engine-version stamping, and a published
`(inputs_hash, engine_version, seed) → result_hash` triple anchored in the witness ledger. This is
what makes a number admissible in a council hearing, and it is the most novel thing on offer:
essentially no municipal fiscal note in the country is reproducible today.

### U5 — Scenario = branch, policy = run

A policy proposal is a counterfactual. The engine already has CoW-branchable WorldDb and run
identity; Government mode needs the *verbs* surfaced: fork the baseline, apply a policy delta,
run, diff against baseline, publish the diff.

**Current state:** CoW branches exist in WorldDb. `compare_runs` exists. The unified `RunId` across
experiment JSON and `SimRecord` is specified in `DATA_PLATFORM_PLAN.md` but not landed.

**Gap:** land the `RunId` unification; add `gov:scenario_manager` / `gov:run_comparator` as the
first-class UI over it.

### U6 — Aggregate-only enforcement, mechanically

Government mode touches constituent data. [`LAW-ORDER.md`](../LAW-ORDER.md) §3 draws the line at the
home and it is absolute: no ambient monitoring, no behavioral prediction from private data, no
person-level modeling. A Government mode makes it *much* easier to violate that than a Justice mode
does, because 311 intake, casework, and permit files all arrive person-level by nature.

**Gap:** a consent/aggregation class that the ingest path *enforces* rather than annotates — a
dataset marked `person_level` cannot be joined into a published model, and the publish path refuses
it. Today the flags are advisory. For this domain, advisory is not enough. Minimum cell-size
suppression (k-anonymity with a stated k) on every published aggregate.

### U7 — The publish path already exists — reuse it

The platform promises a live public dashboard at `budget.tucsonaz.gov`. Eustress already has this
spine, end-to-end verified with the real binary: engine outbox → `api.eustress.dev` Worker → KV →
`eustress.dev/admin/telemetry` with a hierarchy view. That is the same shape as a public budget
dashboard: a generated static skeleton plus live counters merged client-side.

**Gap:** a public (unauthenticated, read-only) variant of that view plus a `gov:publish_dashboard`
action. Do **not** build a second publish stack. Note the standing rule: no production deploys
without explicit permission.

### U8 — Non-geometric entity ergonomics

Twelve disciplines' worth of record-shaped classes need to route through the canonical
`instance_create::create_instance` path without acquiring a mesh, a collider, or a transform gizmo
that means nothing. Grouping, tagging, MindSpace, and Properties must all work on them.

**Gap:** audit `create_instance` and the Properties inspector for mesh assumptions. The
Gaussian-splat and CadPart work already pushed on this; records will push harder.

### U9 — Role gating that can eventually be real

`ModeManifest.required_role` and `SubmodeMeta.required_role` exist. v1 is a client-side,
env-var-seeded `UserRoles` check — honest for a public product, insufficient the moment a discipline
implies authority (a Clerk seat, an Auditor seat). The documented production path is KYC-verified
identity → Cloudflare KV role assignment, imported through Parameters; the `identity` crate exists.

**Recommendation:** ship *all* Government disciplines ungated in v1 and say plainly in the manifest
comment that these are **analysis seats, not authority seats** — anyone may model the Mayor's
budget; no one may sign anything. Gating arrives with real identity, not before.

### U10 — Mode plumbing (small, concrete)

The mechanical cost of a new mode, exactly:

1. **`studio_modes.rs`** — add `(include_str!("../modes/government.toml"), "government")` to the
   built-in load list (currently lines 482–490).
2. **`KNOWN_ICON_IDS`** (`studio_modes.rs:35`) — add `"capitol"`. This is a 10-entry allowlist
   separate from the 118-entry `tool_metadata::TOOL_ICON_IDS` used for *submode* icons.
3. **`mode-icon()`** (`ui/slint/ribbon.slint:649`) — one line returning
   `assets/icons/ui/capitol.svg`. Zero-new-asset option: alias `capitol` to the existing
   `bank.svg`.
4. **One new SVG** at `assets/icons/ui/capitol.svg` (compose from the existing `column.svg` plus a
   dome). All twelve *submode* icons below are already in `TOOL_ICON_IDS` — no other assets needed.
5. **Rerun all three generators, in order** — `gen_tool_metadata.py`, then `gen_tool_icons.py`,
   then `gen_web_hierarchy.py`. A `studio_modes` test asserts every manifest tool id has a
   `tool_metadata` entry, so skipping step 5 fails CI rather than shipping bare buttons.
6. **Extend the `studio_modes` tests** the way `justice`/`military`/`civil` are covered — submode
   count, per-discipline tab divergence, ungated-mode assertion.

---

## 4. The `government` mode

### 4.1 Why a new mode and not a fork

| Existing mode | Why it does not absorb Government |
|---|---|
| **Justice** | Case work — dockets, evidence, parties. Courts are a separate branch and the mode is already correctly scoped to them. Government must not swallow the judiciary; the separation is the point. |
| **Civil** | Civil *engineering* (bridges, terrain, hydrology). Public Works overlaps, and the Government discipline should **hand off** to Civil for design work rather than duplicate `cutl:`/`cwtr:` tooling. |
| **Business** | Firm-shaped: product, manufacturing, P&L. A city has no product and cannot exit. |
| **Legal** | Statute/precedent work. An ordinance drafter belongs in Government; statutory research belongs in Legal. |

Government is its own noun: appropriation, administration, and legislation. It is the first mode
that is ~95% record-and-model work with geometry as a *view* rather than the substrate — which is
precisely why it is the right forcing function for U2.

### 4.2 The twelve disciplines

Icons are validated against `tool_metadata::TOOL_ICON_IDS`; all twelve below already exist.

| # | Discipline | id | icon | prefix | The seat it models |
|---|---|---|---|---|---|
| 1 | **Executive (Mayor)** | `executive` | `flag` | `gexe:` | Citywide agenda, budget authorship, executive action, promise tracking |
| 2 | **Council (Ward)** | `council` | `users` | `gcnl:` | Ward-scoped legislating, roll-call, casework, ward-level impact |
| 3 | **Budget & Finance** | `budget` | `coin` | `gbud:` | Line items, zero-based rebuild, revenue forecasting, tax incidence |
| 4 | **Administration** | `administration` | `flowchart` | `gadm:` | Org chart, positions, automation scoring, attrition paths, service levels |
| 5 | **Clerk & Elections** | `clerk` | `stamp` | `gclk:` | Records, retention, disclosure, campaign finance, hash-anchored publishing |
| 6 | **Procurement & Contracts** | `procurement` | `handshake` | `gprc:` | Solicitation, bid leveling, no-bid registry, vendor clustering, SLA |
| 7 | **Audit & Inspector General** | `audit` | `search` | `gaud:` | The adversarial seat — falsify claims, trace provenance, verify reproducibility |
| 8 | **Permitting & Land Use** | `permitting` | `map` | `gpln:` | Permit queue, zone consolidation, by-right checks, ADU capacity, buildout |
| 9 | **Public Works & Utilities** | `public-works` | `hardhat` | `gpwk:` | Asset registry, deferred maintenance, CIP, in-house-vs-contract |
| 10 | **Public Safety Admin** | `public-safety` | `shield` | `gpsa:` | Deployment, isochrones, station siting, alternative response, open data |
| 11 | **Health & Human Services** | `human-services` | `heart-pulse` | `ghhs:` | Flow modeling, capacity, outcome-tied grants, downstream cost offsets |
| 12 | **Constituent Services (311)** | `constituent` | `lifebuoy` | `gcon:` | Intake, auto-triage, SLA, recurring-issue detection |

Prefix collision check against every prefix currently in use across the nine manifests: `gam` is
gaming, `pl:` is pre-law (legal.toml), `econ:`/`fin:` are student.toml. `gov:` and all twelve
`g****:` prefixes are free.

**Discipline 7 (Audit) is not optional.** It is the discipline that makes the other eleven
credible, and it is the one an opponent will look for. Its tools point at *institutions and money
flows* drawn from public records — awards, disclosures, contract performance — and explicitly not at
officials' private lives. The `Tucson-Campaign/03` and `/05` research to-dos include property records
and social-media audits of named individuals; that work does not belong in this mode and no tooling
below supports it. Keeping the engine on institutional data is both the ethical line and the
strategically stronger one: "here is the award history" survives a fact-check that "here is their
house" does not.

### 4.3 Shared tab: `Jurisdiction`

Civil's pattern — every discipline carries one shared intake tab (`Survey`) plus its own
discipline-flavored tabs — is the right precedent. Government's shared tab is **Jurisdiction**,
present in all twelve disciplines, carrying the `gov:` namespace: boundary/parcel import, the open-
data connector, the provenance inspector, scenario management, run comparison, and the publish
action. It also carries the already-wired `data:import`, so the tab is not entirely aspirational on
day one.

### 4.4 Keep the campaign *out* of Government mode

Tempting and wrong: a "Campaign" discipline with donor CRM, ad scripts, and target lists. A
candidate is not a government, and shipping campaign tooling inside a mode named Government hands an
opponent a free attack — *"he built the campaign machine and the city's budget machine into the same
product"* — regardless of whether any public resource was ever involved.

**Recommendation:** campaign work stays entirely in `Tucson-Campaign/` and, if it ever wants engine
tooling, gets a separate `campaign` mode with its own manifest, its own datasets, and no shared
substrate with Government. The separation should be visible in the file tree, because that is where
it will be audited.

### 4.5 The manifest

The manifest is live at
[`eustress/crates/engine/modes/government.toml`](../../eustress/crates/engine/modes/government.toml)
— all twelve disciplines, five tabs each (the shared Jurisdiction tab plus four discipline tabs),
fifteen sections, and 562 unique tool ids. It is not reproduced here; the file is the source of
truth and a copy in this document would rot on the first edit.

Two structural notes worth keeping in the doc, because both are easy to get wrong:

1. **`effective_custom_tabs` overrides, it does not merge.** Because all twelve disciplines declare
   `[[submode_tabs]]`, and Government is never selectable without a discipline, the mode-level
   `[[tabs]]` block is *unreachable in normal use* — it exists only as a fallback for a submode id
   that fails to resolve. So the shared Jurisdiction tab has to be repeated inside each of the twelve
   submode buckets or data intake silently disappears from every discipline.
   `studio_modes::government_mode_shape` asserts it leads every discipline's tab strip.
2. **`tools = []` appears nowhere.** Unlike the Justice/Civil precedent, every section here carries
   real ids. That is a deliberate difference: an empty section renders "Coming soon", which is honest
   but useless, whereas a named-but-unwired button states the intended surface and gets a generated
   label, tooltip, and icon for free. The honesty lives in §9 instead — 561 of the 562 have no
   dispatch, and that must be said out loud rather than encoded in empty arrays.

## 5. The Civic Simulation Program

Sixteen simulations, three tiers. Each states: **inputs** (with the real source), **mechanism**,
**output**, and — mandatorily — **falsifier**: the specific result that would mean the corresponding
plank is wrong.

Tiering is by *defensibility*, not ambition. Build Tier 0 first; it is small, provable, and it is
what earns the right to be believed about Tier 2.

### Tier 0 — Deterministic and fully defensible

These are arithmetic and queueing. Given correct inputs they are *right*, not *plausible*, and they
reproduce bit-for-bit. Ship these before anything else.

#### S1 · Budget Digital Twin & Zero-Based Reconstruction
*Discipline: Budget & Finance · flagship*

- **Inputs:** FY2025/26 adopted budget ($2.4B), fund structure, CAFR, position roster, the
  $20,975,540 property tax levy, the acknowledged $17.8M deficit.
- **Mechanism:** ingest to line-item granularity → tag each line `{mandated | contractual |
  discretionary}` × `{automatable | partially | not}` → roll up realizable reduction under stated
  rules → project the deficit trajectory over 5 years.
- **Output:** the *derived* maximum defensible cut, with every dollar traceable to a line and a
  source. A public dashboard where any citizen can expand any number to its origin.
- **Falsifier:** if the realizable cut is materially below 30%, **the platform's headline number is
  wrong and must be replaced by whatever the model produces.** A defended 14% beats an undefended
  30% in every forum that matters, and a candidate who publishes a number that came out lower than
  his own campaign literature has just demonstrated the tool is real.
- **Primitives:** U1, U2, U3, U4. This one simulation exercises every hard blocker.

#### S2 · Permit Throughput Queue
*Discipline: Permitting · **build this first***

- **Inputs:** application volume by permit type, review stages, reviewer headcount, current cycle
  times (the 90-day figure), rework/resubmittal rates.
- **Mechanism:** discrete-event multi-server queue per review stage (M/M/c with rework loops);
  automation modeled as a service-rate increase on mechanical checks plus a by-right bypass on
  eligible applications.
- **Output:** cycle-time distribution — not just the mean — under staffing × automation scenarios.
  The specific answer to "what actually gets you to 30 days."
- **Falsifier:** if the binding constraint is a *statutory* review window or an outside agency
  (County health, fire marshal, ADEQ) rather than city staffing, then automation cannot deliver 30
  days and the plank needs a different lever.
- **Why first:** smallest input surface, hardest to dispute, most visible to the small-business and
  homebuilder constituencies, and it produces a real deliverable in weeks rather than quarters.

#### S3 · 911 / Response-Time Isochrones
*Discipline: Public Safety Administration*

- **Inputs:** station locations, road network, historical call volume by space-time cell
  (**aggregate only** — see U6), unit availability.
- **Mechanism:** travel-time isochrones over the real road network + queueing for unit availability;
  station siting as a facility-location optimization.
- **Output:** current vs. achievable response-time surfaces, and what the 8→4 minute target
  actually costs in units, stations, or dispatch policy.
- **Falsifier:** if 4 minutes citywide requires new stations rather than redeployment, the target is
  a *spending* proposal, not an efficiency proposal, and must be presented as one.
- **Note:** this is where Eustress's actual geometric strength shows — real terrain, real roads,
  a rendered isochrone surface. It is also the most persuasive single visual in the program.

#### S4 · Determinism & Reproducibility Harness
*Discipline: Audit · not a policy sim — the meta-sim*

- **Inputs:** every other simulation's `(inputs_hash, engine_version, seed)`.
- **Mechanism:** re-run, compare result hashes, anchor the triple in the `bliss` witness ledger.
  Fail loudly on drift.
- **Output:** a published verification page: anyone downloads the inputs, runs the engine, and gets
  the identical hash.
- **Falsifier:** any nondeterminism found — float reduction order, wall-clock leakage, unpinned
  parallel iteration — invalidates every downstream number until fixed.
- **Why it matters:** essentially no municipal fiscal note in the United States is reproducible. This
  is the most genuinely novel capability in the whole program, and it is the operational meaning of
  "determinism reigns supreme."

#### S16 · Food Access & Desert Retail
*Discipline: Health & Human Services*

- **Inputs:** grocery and full-service retail locations, parcel-level population (aggregate), road
  network, transit routes and headways, vehicle-access rates by block group.
- **Mechanism:** the same isochrone machinery as S3, run against food retail instead of stations —
  travel-time-to-nearest-grocery surfaces computed separately for driving, transit, and walking, then
  overlaid on population.
- **Output:** how many residents sit beyond a defensible access threshold, by mode and by ward, and
  which single new store location or transit change moves that number most.
- **Falsifier:** if travel time is dominated by transit headway rather than store count, this is a
  transit problem wearing a food-policy label, and market-permit or grocery-incentive planks won't
  move it.

### Tier 1 — Model-based, defensible with stated parameters

Honest form is a *range* with named elasticities and a published sensitivity sweep. Never a point
estimate.

#### S5 · Automation Displacement & Transition
*Discipline: Administration*

- **Inputs:** position roster by classification, task decomposition per class, vacancy and attrition
  rates, salary + benefit loading, severance and retraining costs.
- **Mechanism:** per-position automatable-fraction scoring → three paths: attrition-only, hiring-
  freeze-plus-attrition, and RIF → net savings curves with transition costs subtracted.
- **Output:** savings-over-time under each path, and the year each converges.
- **Falsifier:** if attrition-only reaches ~the same steady-state savings on a 3–4 year curve — which
  is the typical result in workforce models — then **layoffs buy speed, not savings.** That reframes
  the platform's "fire what doesn't need to exist" into "stop backfilling what a script can do,"
  which is the same fiscal outcome, is far more defensible in a hearing, and costs vastly less
  politically. Worth knowing before the first debate rather than after.

#### S6 · Tax Incidence & Local Revenue Response
*Discipline: Budget & Finance*

- **Inputs:** assessed values by parcel class, levy structure, sales-tax base, the proposed 10+ year
  resident freeze, owner/renter split by ward.
- **Mechanism:** static incidence by parcel class and ward, then dynamic revenue response under a
  stated elasticity range.
- **Output:** who pays more, who pays less, by ward — and the revenue path.
- **Falsifier:** a freeze for long-tenured residents mechanically shifts burden onto newer owners and
  (through pass-through) renters. **Quantify that shift and publish it.** If it lands
  disproportionately on the lowest-income wards, the plank needs redesign — a homestead exemption or
  a circuit-breaker reaches the same intent without the regressive tail.

#### S7 · Zoning Liberalization & Housing Supply
*Discipline: Permitting · the strongest positive libertarian result available*

- **Inputs:** parcel geometry, current zoning (the 15 zones), lot dimensions, ADU feasibility
  constraints, parking-minimum requirements, construction cost, absorption rates.
- **Mechanism:** by-right capacity analysis per parcel under baseline vs. consolidated zoning →
  feasibility filter on cost/rent → 5-year absorption to a supply path. Rendered as an actual 3D
  buildout, which is literally what this engine is for.
- **Output:** net new units by ward and year under each rule change, ranked by units-per-regulation-
  removed.
- **Falsifier:** if realizable supply is dominated by construction cost, water allocation, or
  interest rates rather than by zoning, then deregulation is necessary but not sufficient, and
  claiming it solves housing overstates it.
- **Why it matters most:** this is the plank where a libertarian result is likely to be *strong* and
  where the model probably *agrees* with the platform. Deregulation producing measurable supply, shown
  as a 3D buildout with parcel-level receipts, is a better argument than any ad script in `07`.

#### S8 · Procurement Competition & Award Integrity
*Discipline: Procurement*

- **Inputs:** historical award records, bidder counts, award amounts, contract modifications, vendor
  registry.
- **Mechanism:** bid-count vs. unit-price curve to estimate the competition premium; embedvec
  similarity clustering over vendor and award records to surface related-entity patterns; change-
  order escalation analysis.
- **Output:** estimated annual savings from converting no-bid and single-bid awards to competitive,
  plus a public award-history browser.
- **Falsifier:** if most no-bid awards are cooperative-purchasing or sole-source-justified
  (proprietary systems, emergency procurement), the addressable pool is far smaller than the total
  and the savings claim shrinks accordingly.
- **Boundary:** clustering surfaces *patterns in public records* for human review. It does not
  allege wrongdoing, and no output should be published as an accusation.

#### S9 · Patrol Deployment Optimization
*Discipline: Public Safety Administration*

- **Inputs:** aggregate incident counts by space-time cell, shift structure, unit counts, overtime
  rates, the documented 18% summer violent-crime spike.
- **Mechanism:** constrained assignment of units to space-time cells maximizing coverage-weighted
  expected-incident intersection, subject to labor rules; compare against current allocation.
- **Output:** the coverage gain from redeployment alone at constant headcount, and the marginal value
  of the next unit.
- **Falsifier:** if the incident surface is too diffuse for redeployment to beat uniform coverage,
  "data-driven deployment" is not the lever and should not be sold as one.
- **Privacy — binding:** aggregate cells only, with minimum-cell-size suppression. No person-level
  data, no individual prediction, no place-based prediction fed back as an enforcement trigger. This
  is the sim most capable of drifting into predictive policing; U6's enforcement (not annotation)
  is what keeps it from doing so.

#### S10 · Transit Fare vs. Farebox Recovery
*Discipline: Budget & Finance · a likely platform correction*

- **Inputs:** SunTran ridership, pre-suspension fare revenue, fare-collection cost (equipment,
  maintenance, cash handling, enforcement, dwell-time effects), federal/regional formula funds
  sensitive to ridership.
- **Mechanism:** ridership elasticity to fare reinstatement → net revenue after collection cost →
  effect on formula funding and on operating cost per boarding.
- **Output:** the true net fiscal effect of reinstating fares.
- **Falsifier — likely to fire:** small transit systems frequently spend a large fraction of gross
  fare revenue collecting it, and ridership loss can reduce formula funds by more than the fares
  recovered. **If that holds in Tucson, the free bus is the fiscally conservative position, and Ad 7
  in `07-social-media-ads.md` ("Cute idea. Terrible math.") is arguing against the candidate's own
  principles.** Run this before that ad ships.

#### S15 · Water Portfolio & Assured Supply
*Discipline: Public Works & Utilities · **the most consequential simulation for Tucson specifically***

- **Inputs:** Central Arizona Project allocation and shortage tiers, groundwater rights and stored
  recharge credits, reclaimed-water volumes, per-capita demand by customer class, system loss
  (non-revenue water), the 100-year assured-water-supply designation, rate structure.
- **Mechanism:** a portfolio balance over sources against demand under Colorado River shortage
  scenarios, with recharge credits drawn down as a stock; demand responds to rate changes under a
  stated price elasticity.
- **Output:** years-of-assured-supply under each shortage tier, the loss-reduction and rate paths
  that extend it, and the growth headroom the designation actually supports.
- **Falsifier:** if assured supply fails a plausible shortage scenario, then **growth policy — not
  permitting speed — is the binding constraint on housing**, which directly limits what S7 can claim.
  A city that cannot water new homes cannot zone its way out.
- **Why it belongs here:** Tucson is a desert city on a contested river. No civic model of Tucson is
  credible without this one, and it was the conspicuous gap in the original fourteen.

### Tier 2 — Research-grade; publish with explicit uncertainty

Genuinely hard. Publishable, but only with ranges, named assumptions, and a visible sensitivity
sweep. Anyone claiming point precision here is selling something.

#### S11 · Homelessness Stock-and-Flow
*Discipline: Health & Human Services*

- **Inputs:** PIT counts (2,218 in 2025; 23% unsheltered drop since 2022), HMIS aggregates, shelter
  and treatment capacity, exits to permanent housing, returns-to-homelessness rate, inflow drivers
  (evictions, discharges, rent burden).
- **Mechanism:** system-dynamics stock-and-flow — inflow, shelter/street stocks, exits, returns —
  calibrated to observed counts, with capacity and enforcement as policy levers.
- **Output:** the count trajectory under each policy mix, and the *binding constraint* on reduction.
- **Falsifier — likely to fire:** if inflow rather than exit capacity dominates, then a 50%
  reduction in 12 months is arithmetically unreachable by any enforcement or capacity policy, and
  the platform target in `08-policy-proposals.md` is unachievable as written. **Finding that and
  publishing it is the single largest credibility win available in this entire program**, because
  every incumbent has promised a headcount reduction and none has published a flow model. "Here is
  why the last six years failed, and here is the number that is actually reachable" is a stronger
  position than a bigger promise.

#### S12 · Enforcement vs. Treatment Cost-per-Exit
*Discipline: Health & Human Services*

- **Inputs:** enforcement-cycle cost (citation, transport, jail-bed days, court time under the June
  2025 ordinance's $250 fine / 10-day jail / 1-year probation structure), treatment-slot cost,
  observed exit rates per pathway, downstream ER and jail utilization.
- **Mechanism:** cost-per-sustained-exit by pathway, including the recidivism/return loop and
  downstream offsets.
- **Output:** a defensible ranking of dollars-per-outcome across enforcement, treatment, and housing
  pathways.
- **Falsifier:** if enforcement's cost-per-sustained-exit exceeds treatment's — the common finding —
  then "clear streets" is defensible on *public-order* grounds but not on *fiscal* grounds, and must
  be argued as the former. Relatedly, the platform's proposed 42% Social Services cut needs to be run
  net of downstream ER and jail costs; a gross cut that raises net spending is not a cut.

#### S13 · Service-Level Regression Guard
*Discipline: Audit · the adversarial counter-simulation*

- **Inputs:** every proposed cut from S1, paired with the service metric that cut could degrade.
- **Mechanism:** for each cut, an explicit service-level model and a pass/fail threshold. A cut that
  degrades its paired metric past threshold is marked **failed** and removed from the total.
- **Output:** the *net defensible* reduction — S1's gross number minus everything S13 rejects.
- **Falsifier:** this simulation exists to fire. If it never rejects anything, it is miscalibrated
  and the 30% figure remains unfalsifiable — which is the failure mode §2 warns about.

#### S14 · Ward-Level Distributional Explorer
*Discipline: Council · political necessity*

- **Inputs:** every output above, joined to ward boundaries.
- **Mechanism:** allocate each cut, fee change, and service change to wards by incidence.
- **Output:** per-ward winners and losers for the entire program.
- **Falsifier:** if the program's benefits concentrate in high-income wards while costs concentrate
  in low-income wards, that is a real finding about the platform, not a presentation problem. A
  council member is elected by one ward and has to answer to it; better to know in advance.

### Program invariants

1. Every published number resolves to a source (U3).
2. Every model states its falsifier, and S13 is designed to fire.
3. Every run is reproducible and hash-anchored (S4).
4. Every published aggregate passes minimum-cell-size suppression (U6).
5. Behavioral claims publish as ranges with named elasticities. Never point estimates.
6. **When a model contradicts the platform, the platform changes.** Not the model. Once.

---

## 6. Build order

| Wave | Lands | Why this order |
|---|---|---|
| **0 · Substrate** | U1 (one live ingest path), U2 (Dataset noun), U3 (`Provenance` component + inspector row) | Nothing above works without these. U3 is disproportionately cheap and is the credibility feature. |
| **1 · First provable result** | S2 (permit queue), S4 (determinism harness), the `government` mode manifest with disciplines 2 / 3 / 8 live | Smallest input surface, fastest real deliverable, and S4 makes everything after it admissible. |
| **2 · The flagship** | S1 (budget twin), S13 (regression guard), U7 (public dashboard), disciplines 1 / 4 / 7 | S1 and S13 must ship *together* — S1 alone is the prop §2 warns about. |
| **3 · Geometry + models** | S3, S7, S9 (isochrones, buildout, deployment) | These are where the engine's 3D strength differentiates it, and they depend on Wave 0's parcel/boundary ingest. |
| **4 · Hard models** | S5, S6, S8, S10, S11, S12, S14 + remaining disciplines | Research-grade; publish only after Wave 1's reproducibility discipline is habitual. |

Wave 1 is the honest MVP: **one simulation, fully sourced, fully reproducible, publicly
verifiable.** That is worth more to the campaign than twelve half-built disciplines, and it is
demonstrable in a single meeting.

---

## 7. Risks and honest limits

1. **A model that always agrees is a prop.** Addressed structurally by mandatory falsifiers and S13,
   but it is a discipline, not a feature — it decays if not enforced.
2. **Determinism is not correctness.** A bit-reproducible model with wrong elasticities is
   *confidently* wrong, which is worse than visibly uncertain. S4 buys auditability, not truth. Only
   Tier 0 is genuinely defensible on mechanism alone.
3. **Input quality gates everything.** Tucson does not publish checkbook-level machine-readable
   spending. Wave 0 is substantially a PDF/XLSX extraction problem with a human-verification step,
   and every downstream number inherits that confidence.
4. **Person-level data must never enter.** 311, casework, and permit files arrive person-level.
   U6 must be *enforcement*, not annotation, before any of those datasets is ingested.
5. **Conflict of interest is real and should be pre-empted.** A candidate building the tool he would
   later ask the city to adopt is an obvious attack. Disclose from day one; note that Eustress is
   **source-available under PolyForm Shield** — the city can read and independently audit the code,
   which is a genuinely strong answer, and one no proprietary civic-tech vendor can give. (Say
   "source-available," never "open source.")
6. **Scope honesty.** Twelve disciplines × ~40 tools is ~480 buttons, of which perhaps 15 would be
   wired in Wave 1. That is consistent with existing practice (~1,400 declared tools across nine
   manifests, ~30 wired) and the placeholder rule keeps it honest — but it should be a deliberate
   decision, not a side effect.
7. **This mode does not confer authority.** It models seats; it signs, files, and authorizes
   nothing. That must stay in the manifest header, because someone will eventually assume otherwise.

---

## 8. Where the engine's own record suggests the platform is wrong

Offered because the campaign's stated rule is sources over assertions, and because these are far
cheaper to fix now than in a debate.

1. **Replace "blockchain for transparent voting" with hash-anchored public records.** Blockchain
   voting is opposed by essentially the entire election-security research community and by the
   county recorders whose cooperation any such pilot would require; proposing it costs credibility
   with the Clerk's office the platform needs as an ally. The defensible version delivers the same
   benefit and *already exists in this repo*: publish a Merkle-anchored hash of every released
   record set through the `bliss` witness ledger, so anyone can prove a record was not altered after
   publication. Same "you can't fudge it" claim, real implementation, no election-integrity fight.

2. **Derive the 30%, then publish whatever number falls out.** It is currently asserted. S1 + S13
   will produce a defensible figure. If that figure is 14%, publishing 14% *with receipts* is a
   stronger position than 30% without them — and the act of revising it downward is the single most
   credible thing a candidate running on determinism can do.

3. **"Publish crime stats by immigration status" cannot be implemented cleanly and should be
   dropped or reframed.** Local law enforcement generally neither collects nor verifies immigration
   status, so the field would be overwhelmingly null; filling it would require *inferring* status
   from person-level attributes, which is exactly the person-level behavioral modeling
   [`LAW-ORDER.md`](../LAW-ORDER.md) §3 forbids — the candidate's own doctrine. The reframe that
   keeps the transparency intent: *publish all incident data as open data, in full, with no derived
   person attributes.* Stronger on transparency, and consistent with the rest of the platform.

4. **Run S10 before the "free bus is terrible math" ad ships.** Fare collection on small systems
   often consumes a large share of gross fare revenue, and ridership loss can cut formula funding by
   more than the fares recovered. If that holds here, the fiscally conservative answer is the free
   bus, and Ad 7 argues against the candidate's own principles on his own evidence.

5. **Model the 42% Social Services cut net, not gross.** S12's downstream-cost term is the whole
   question. A cut that moves spending from a program line to ER and jail lines is a *transfer*, not
   a saving, and an opponent with a calculator will say so. Publish the net figure first.

---

## 9. What has shipped, and what has not

Stated plainly because the gap is the whole risk: a twelve-discipline ribbon over a temple full of
portals *looks* like a working civic platform, and it is not one yet. Anyone demoing it should say so.

### Shipped and verified

| Thing | Where | Verified by |
|---|---|---|
| `government` mode, 12 disciplines, 562 tool ids | [`modes/government.toml`](../../eustress/crates/engine/modes/government.toml) | `studio_modes::government_mode_shape`; the existing all-manifests test covers every id's metadata + composed icon |
| Mode plumbing (icon allowlist, `mode-icon()`, registration, `capitol.svg`) | `studio_modes.rs`, `ui/slint/ribbon.slint`, `assets/icons/ui/capitol.svg` | `builtins_parse` asserts ten built-in modes |
| Generated metadata + 1,999 composed per-tool icons + web hierarchy | `tool_metadata.rs`, `assets/icons/tools/`, `crates/web/assets/tool_hierarchy.json` | three generators re-run; the pre-existing 1,437 ids produced a **zero-line diff**, so the additions are purely additive |
| Free-camera Space↔Space portals | [`portal.rs`](../../eustress/crates/engine/src/portal.rs) | 6 unit tests (Space-name resolution incl. cross-Universe fallback, disarmed-on-start, attribute type-safety) |
| **Tucson** Universe: temple hub + 16 simulation Spaces, 32 portals | `Documents/Eustress/Tucson/`, from [`gen_tucson_universe.py`](../../scripts/gen_tucson_universe.py) | 1,155 TOML files parse; **byte-identical tree hash across re-runs** |

The generator's reproducibility is not incidental. S04 demands that every model in this program be
bit-reproducible; scaffolding that could not meet its own standard would be the first thing an
auditor pointed at. `gen_tucson_universe.py` uses no wall clock, no RNG, and path-derived uuids.

### Not shipped

- **Live dispatch behind the buttons.** Of 562 declared government tool ids, exactly one —
  `data:import`, inherited — has a real handler. The other 561 are declared intentions. This matches
  existing practice (~1,400 declared across the previous nine modes, ~30 wired) and the placeholder
  rule keeps it honest, but it must never be described as working software.
- **Every simulation in §5.** Sixteen designs, zero implementations.
- **All three hard blockers in §3.** U1 (Parameters has no live producer), U2 (no Dataset noun),
  U3 (no `Provenance` component) are exactly as dormant as before. Nothing here changed them; the
  mode makes the case for them more concrete, which was the point.
- **U6 aggregation enforcement.** The 311 and casework disciplines have tools *named* for the privacy
  gate, and no mechanism behind it. Until U6 is enforcement rather than annotation, no person-level
  dataset should be ingested into any of these Spaces.

### Guardrails applied during authoring

The twelve disciplines were authored in parallel and then audited adversarially. That pass rejected
23 tool ids: eight for surveillance risk (a vacancy-risk screener that would have predicted *which
employees might quit*; a neighbourhood directory of residents), three for implying authority rather
than analysis (a fee-schedule *editor*, a salary-schedule *editor*), five as unfalsifiable filler
(an "equity index" with no stated denominator), and seven as cross-discipline duplicates that would
have rendered two identically-labelled buttons computing the same contested number — the failure mode
§2 describes, where a campaign quotes whichever of two answers is more favourable. Every discipline
carries at least one tool able to produce a platform-unfavourable result; that was a pass criterion,
not a preference.

---

*Proposal only. No manifest, icon, allowlist entry, or generator run has been made against the tree
as part of this document. §4.5 and §U10 are written to be executable as-is on approval.*

---

## 10. The Tucson Universe — an abstract civic temple

`scripts/gen_tucson_universe.py` generates a Universe named **Tucson** under
`Documents/Eustress/`: a hub Space called `Temple` plus one Space per simulation, wired together with
portals. Re-runnable with `--force`; refuses to clobber an existing Space otherwise, and only ever
removes the authored `Workspace/` tree, never the engine's own `world.fjalldb` / `header.bin` state.

### 10.1 The idea

The government is not a separate building sitting next to the needs it serves — **the wings ARE the
disciplines**. A central rotunda holds the public record; twelve radial wings each embody one basic
need a municipality actually provides for; each wing's colonnade ends in a portal arch that opens
into the simulation measuring whether the city is meeting that need. §4.2's need → discipline mapping
and this floor plan are the same mapping read at two altitudes.

The reading order along a wing is deliberate:

| Position | What is carved there |
|---|---|
| Wing mouth (r≈30) | The **obligation** the need creates — *"Water reaches every tap. Every gallon is accounted."* |
| Along the colonnade | Four **provisions** — the concrete things government supplies toward it |
| Wing end (r≈79) | The **failure mode** — how a city visibly fails at this need |
| The arch (r=88) | The **portal** into the simulation that measures exactly that failure |

So you walk out past what you owe, past what you provide, past what failure looks like, and step
through into the model that can tell you which one you are living in. The rotunda at the centre is
`The Ledger`, inscribed *"The record is public. Every number answers to its source."* — the citizen
stands at the middle and can see every wing at once, which is the whole architectural argument.

Twelve wings at 30° intervals, each with a maximally-separated accent hue: under radial symmetry
colour is the only wayfinding cue that tells you which wing you are standing in.

### 10.2 The mind-map canopy

Four rings float above the rotunda: **THE PEOPLE** at the apex → the twelve **needs** → the
**disciplines** that serve them → the **simulations** that measure them. Shared disciplines converge
rather than duplicate (Health & Human Services serves both Food and Health, so it is one node placed
at the mean bearing of the needs it serves). Each simulation node drops a thin strand to its own
portal arch below, so the graph visibly anchors into the architecture. Built on the same
`Part`+`ball.glb` node / `cylinder.glb` edge / `BillboardGui`→`TextLabel` pattern as the existing
MindMaps in `Universe1`.

### 10.3 Portals

A portal is **not a new class**. [`portal.rs`](../../eustress/crates/engine/src/portal.rs) reads an
`[attributes]` table off any ordinary instance:

```toml
[attributes]
portal_target = "S01-Budget-Twin"      # destination Space NAME
portal_radius = 8.0                     # trigger radius, metres
portal_arrival = [0.0, 8.0, 0.0]        # camera pivot on arrival
portal_arrival_yaw = 180.0              # arrival bearing, degrees
portal_label = "S01 · Budget Digital Twin"
```

`eustress_common::scene::PortalData` exists but is **never constructed anywhere in the workspace** —
it belongs to the deprecated `.scene.json` path, and reviving it would mean reviving that path.
Attributes are already parsed into a live `Attributes` component by `instance_loader`, so a portal
needs zero schema changes, survives save/load, and is editable from the Properties panel.

The **Studio free camera** flying inside the radius fires the jump — this is an editor navigation
aid, not a Play-mode mechanic. Two guards stop a bounce-back, since an arrival necessarily lands near
the destination's return portal: the trigger arms only once the camera is clear of *every* portal
volume (1.6× hysteresis, and state starts **disarmed** so a camera spawning inside a volume cannot
fire on frame one), plus a 1.25 s cooldown covering the reload. The arrival pose is re-asserted for
eight frames so a space-load path that resets the camera cannot clobber it. Return portals land the
camera at its need's wing mouth, facing back down the wing toward the rotunda.

Space names resolve Universe-locally first, then fall back to a workspace-wide search with a warning,
so a cross-Universe portal degrades to a warning rather than silently doing nothing. An unresolvable
target disarms and toasts instead of warning every frame.

One performance note that is easy to get wrong: `instance_loader` inserts an `Attributes` component
on **every** instance, empty or not — deliberately, so the live component mirrors disk for all
classes. A naive `Query<(&GlobalTransform, &Attributes)>` therefore matches the entire scene, making
the per-frame proximity test O(entities) with a hash lookup each. On a ~119k-entity Space that is a
real frame-time cost for a feature concerning sixteen objects. So a `Changed<Attributes>`-driven
system maintains an `IsPortal` marker and the hot path queries `With<IsPortal>` — O(portals), exact
rather than merely throttled, and free in steady state because change detection only fires on Space
load and Properties-panel edits. Clearing `portal_target` removes the marker, so a retired portal
actually stops firing.

### 10.4 Extending it

Wings, needs, provisions, inscriptions, failure modes, accents, and the sim↔wing assignment are all
one table (`WINGS`) at the top of the generator; the simulation roster is another (`SIMS`). The script
refuses to run if any simulation is unreachable from a wing or reachable from two — a Space with no
way in would otherwise generate silently.
