# Parameters Convergence Layer — Architectural Proposal

**Status:** Proposal (documentation only, no implementation) — 2026-07-21
**Owner:** Engine Core
**Cross-refs:** `eustress/crates/common/src/parameters.rs` (the real, existing Parameters types
this proposal's wire contract maps onto — see §3), `docs/architecture/DATA_PLATFORM_PLAN.md` §3.5
(canonical, already-grounded characterization of Parameters as "declared but inert" — this
document defers to it rather than re-deriving it), `docs/architecture/CLASS_REGISTRY.md` (an
unimplemented Wave-1 spec for a cleaner `ClassName`/`ClassSpawner` registry — see §3 for why this
proposal doesn't assume it has shipped), `docs/development/WORKSHOP_TOOLS.md` (two Parameters
TODOs, still unchecked), `crates/engine/modes/justice.toml` and `crates/engine/modes/legal.toml`
(the Justice/Legal ribbons this layer would eventually feed).

> **Executive summary** — External justice and legal systems (case management, records
> management, e-filing, jail management, probation, body-worn-camera platforms) should never
> plug directly into Eustress's entity model. This document proposes an external-facing wire
> contract for **Parameters** — an existing, real, but currently-dormant internal typed system
> (`crates/common/src/parameters.rs`; see §3 for the corrected, grounded picture) — as the *only*
> door those systems walk through. Parameters map into ordinary `ClassName` entities and
> Datasets; nothing downstream (the Justice/Legal ribbons, MindSpace, MCP tools, WorldDb) needs
> to know or care that a given Case or Party originated outside Eustress. This is a
> **documentation deliverable only** — no connector code, no live endpoint, no wire format
> implementation ships alongside it, and any future implementation must build against the
> existing `parameters.rs` types (§5.1), not a parallel system.

---

## 1. Purpose

Eustress is a simulation, visualization, and orchestration substrate — not a system of record for
any external domain. When a justice or legal domain is modeled inside it (see the Justice and
Legal ribbon modes), the temptation is to let Eustress become the place case data *lives*. That
is the wrong shape: it duplicates authority that already exists in county CMS/RMS deployments,
e-filing systems, and jail management platforms, and it puts Eustress in the business of
compliance regimes (CJIS, state records-retention law) it has no reason to take on.

Instead, Eustress positions itself as a **Unified Convergence Layer**: external systems remain
authoritative; Eustress ingests structured, provenance-tagged snapshots and turns them into a
live, simulatable, visualizable, measurable digital twin — reform hypothesis testing, 3D case/
courtroom reconstruction, universal metrics dashboards, multi-stakeholder collaboration — all
built on data that stays traceable back to its system of origin.

## 2. Non-Goals

- Eustress is **never** the system of record for case, docket, evidence, or party data.
- This proposal ships **no** concrete connector to any named external product or standard body.
- No live network endpoint, polling job, or file-watcher is implemented as part of this document.
- Parameters' sensitivity/handling flags (§6) are **advisory metadata only** — actual CJIS,
  HIPAA, or state-privacy-law controls remain the responsibility of the external system of
  record. Eustress does not claim compliance by carrying these flags.
- No write-back path exists or is proposed here. Any future "Eustress writes an authoritative
  change back to an external system" capability would need its own, separately reviewed design —
  it is explicitly out of scope.

## 3. Current State (Honesty Check)

**Correction from an earlier draft of this document:** an initial version of this section
claimed Parameters "is not code today." That was wrong, caught in review, and worth stating
plainly rather than quietly fixing — the corrected picture below changes what this proposal
actually is.

**Parameters is real code — 976 lines at
[`crates/common/src/parameters.rs`](../../eustress/crates/common/src/parameters.rs) — but it is
*declared but inert*.** Its module doc calls it "the single source of truth for all data flow"
across a 3-tier Global → Domain → Instance hierarchy: `ParametersPlugin` (registered),
`GlobalParameters` / `GlobalParametersRegistry`, `DomainRegistry`, `InstanceParameters`,
`ParameterValue`, `ParameterRouter` (`publish_export` / `buffer_export` / `drain_pending`), a
~55-variant `DataSourceType` taxonomy, a 7-variant `ExportTargetType`, and `DataMapping` /
`FieldMapping` / `MappingTargetType`. All of that exists, compiles, and is exercised by nothing:

- `DataSourceType` / `ExportTargetType` carry **zero transport** — no `match source_type { … =>
  open connection }` anywhere in the tree; the big enum is read only for `display_name()` /
  `category()` UI helpers.
- `ParameterRouter::publish_export` / `buffer_export` / `drain_pending` have **no callers**
  anywhere in the workspace; the `parameter.changed` / `parameter.exports` stream topics are
  empty channels — no emitter, no subscriber.
- `InstanceParameters` is a real `#[derive(Component, Reflect)]` but is **never inserted** by
  the live entity-spawn path.
- The only Parameters type with a live (but dead/deprecated) persistence touchpoint is
  `GlobalParametersRegistry`, serialized into the deprecated, UI-unreachable `.scene.json` path.

This is exactly the grounding
[`docs/architecture/DATA_PLATFORM_PLAN.md` §3.5](../architecture/DATA_PLATFORM_PLAN.md) already
established — this document defers to that section as canonical rather than re-deriving it, and
that section is the one to keep current if the dormant-vs-live state changes.

**What this changes about this proposal:** the job here is not to invent Parameters from
nothing — it already has a typed internal shape. The job is to specify the *external-facing
wire contract* (§5) that would give the existing `InstanceParameters` / `ParameterValue` /
`DomainRegistry` / `DataSourceType` types their first live producer, which
`DATA_PLATFORM_PLAN.md` §3.5.4 already identifies as the missing piece ("the Parameters side is
the contract and the Data Platform side is the missing body"). §5's `[[parameter]]` shape below
is written to map onto those existing types field-for-field (see §5.1), not to compete with
them. Any future implementation must build against the existing `parameters.rs` types — a
second, name-colliding Parameters system would be a real regression, not a clean start.

What already exists and this proposal builds on directly:
- **`ClassName` entity system** — every spawned thing in Eustress (Part, Model, Dataset, …) is
  one `ClassName` variant. Today, adding one is genuinely ad hoc: per
  [`docs/architecture/CLASS_REGISTRY.md`](../architecture/CLASS_REGISTRY.md) — itself an
  unimplemented "Wave 1 SPEC (no code)," not shipped behavior — it costs 4-7 simultaneous edits
  across `instance_loader.rs` / `file_loader.rs` / `gui_loader.rs` / `spawn.rs`, silently
  degrades to a `Folder` if one is missed, and WorldDb/Fjall persistence "is not class-aware at
  all." New justice/legal classes (`Case`, `Party`, `DocketEntry`, …) would be added through
  *whichever* extension path exists at implementation time — the current ad hoc one, or the
  cleaner `ClassSpawner` registry CLASS_REGISTRY.md proposes, if and when that spec ships.
- **Dataset as first-class** (`docs/architecture/DATA_PLATFORM_PLAN.md`) — schema, source/
  provenance, live stats, and charts already live on Dataset instances in the Explorer hierarchy.
  Provenance tracking, specifically, is not a new concept Parameters introduces; it extends a
  pattern Datasets already have.
- **WorldDb / Fjall LSM-tree** — data and simulation state already share one persistence
  substrate; Parameters-derived entities would persist the same way anything else does, once a
  live path exists.

## 4. Design Principle

External systems talk **only** to Parameters. They never talk directly to `ClassName` entities,
the Justice/Legal ribbon tools, or WorldDb. Parameters is the sole, documented, versioned
contract standing between "data of unknown shape and provenance" and "a live Eustress entity."

## 5. Parameters Contract (Documentation Only)

A **Parameter** is a typed, named, provenance-tracked value or time-series bindable to an entity
or Dataset. Conceptual shape, for external implementers (no parser ships with this document):

```toml
# Example Parameters declaration — illustrative shape only, not a shipped schema.
[[parameter]]
id = "case.status"
type = "enum"                    # string | int | float | bool | enum | timeseries | json
value = "pretrial"
source = "external-cms-v3"
provenance = { system = "CountyCMS", record_id = "CR-2026-004821", extracted_at = "2026-07-21T17:00:00Z" }
mapping_hint = "Justice.Case.Status"   # optional hint for a future class mapper
sensitivity = "CJIS-moderate"          # advisory only — see §2 Non-Goals
```

Required fields: `id`, `type`, `value`, `source`, `provenance.system`, `provenance.record_id`,
`provenance.extracted_at`. `mapping_hint` and `sensitivity` are optional advisory metadata.

### 5.1 Reconciliation with the existing `parameters.rs` types

This shape is written to map onto the real, existing internal types (§3), not to sit beside
them as a second system:

| This document's field | Existing type (`crates/common/src/parameters.rs`) | Notes |
|---|---|---|
| `id`, `type`, `value` | `InstanceParameters` + `ParameterValue` (typed enum, `:286`) | The wire Parameter becomes one typed `ParameterValue` on an `InstanceParameters` component — the component that today is defined but never inserted (§3). This proposal is what would finally insert it. |
| `source` | `DataSourceType` (~55-variant taxonomy, `:588`) | `source = "external-cms-v3"` would resolve to a `DataSourceType` variant — today those variants carry no transport (§3); an implementation adds the missing `match` arm, it doesn't add a new enum. |
| domain grouping (implicit in `mapping_hint`) | `DomainRegistry` (`:171`) | A justice/legal Parameter set would register as one more domain, the same shape as any other. |
| `provenance` | Not yet modeled in `InstanceParameters` today | The one genuinely new piece: existing `ParameterValue` has no provenance field. Any implementation adds it there rather than inventing a parallel provenance-bearing type. |
| export/routing (not in the example, but implied by "how a value leaves") | `ParameterRouter` (`:332`) | The dead `publish_export`/`buffer_export`/`drain_pending` path is exactly what a Parameters → Class Mapper would finally call — see `DATA_PLATFORM_PLAN.md` §3.5.4's "missing producer" framing. |

Nothing in §5-§9 below should be read as proposing new top-level types where an existing one
already covers the concept — where this document names something not yet in `parameters.rs`
(provenance, being the one real gap), an implementation extends the existing struct rather than
building a parallel one.

Parameters may arrive as:
- **Static snapshots** — a one-time extract.
- **Live streams** — reusing the adapter pattern already sketched for Workshop IoT telemetry.
- **Batch files** — CSV/JSON/Parquet dropped into a watched Parameters inbox (mirrors the
  existing `data:import` ingestion path used elsewhere in the Data Platform).
- **Pull-based** — Eustress periodically requests via a documented Parameters endpoint contract
  (shape only; no endpoint is implemented here).

## 6. Mapping to Entity Classes

A thin, internal **Parameters → Class Mapper** — using whichever class-extension path is live at
implementation time (today, the ad hoc one; `CLASS_REGISTRY.md`'s proposed `ClassSpawner`
registry, if it ships first) — would instantiate or update ordinary `ClassName` entities and
Datasets from incoming Parameters:

| Parameter Domain | Target Class(es) | Notes |
|---|---|---|
| Case / Matter | `Case`, `Matter` (new `ClassName` variants) | Hierarchy under a Justice or Legal Space |
| Parties | `Party`, `Person`, `Organization` | Linked via relationships, not embedded blobs |
| Events / Chronology | `Event`, `TimelineEntry` | Time-series friendly — feeds Metrics tabs directly |
| Evidence / Exhibits | `Evidence`, `Exhibit`, `Dataset` (for digital evidence) | Chain-of-custody itself modeled as a Parameters stream |
| Docket / Filings | `DocketEntry`, `Filing` | |
| Metrics / Outcomes | `MetricSeries`, `Dataset` | Backs the Justice/Legal Metrics tabs' `data:chart`/`data:stats` buttons |
| Facilities / Locations | `Facility`, spatial entities | Geospatial binding already supported |

Whichever path adds these classes (§3's ad hoc process today, or a shipped `ClassSpawner`
registry later), once mapped they are ordinary `ClassName` entities — visible in Explorer,
Properties, and MCP the same way anything else is. WorldDb persistence is the one caveat worth
carrying forward honestly: per `DATA_PLATFORM_PLAN.md` §3.5, Fjall persistence today "is not
class-aware at all," so a new justice/legal class's storage story is not automatic and would
need the same persistence work any other new class needs — this document doesn't presume that
problem is solved. No parallel "justice data" subsystem is created either way.

## 7. Complementary to Existing Standards

Parameters is an adapter layer, not a replacement for any justice-domain standard:

- External systems or middleware transform NIEM 5.x, ECF (Electronic Court Filing), GJXDM, or a
  jurisdiction's local schema into the Parameters shape (§5). Eustress never parses those formats
  directly — that translation is the external system's or an integrator's responsibility.
- `sensitivity` flags on a Parameter are advisory only (§2); real CJIS or privacy controls stay
  with the system of record.
- `provenance` makes every value traceable back to its originating system and record id — this
  is the mechanism that keeps every downstream metric, chart, or reform-hypothesis result
  auditable to its source, which matters more in a justice context than almost any other domain
  Eustress touches.
- No component assumes it can write an authoritative change back to an external system (§2).

## 8. Ingestion Patterns (Shapes Only)

- **File drop** — a watched Parameters inbox directory, same trust model as the existing
  `data:import` file-picker path (user- or process-initiated, never silently network-triggered).
- **REST pull** — documented endpoint *shape* only (method, expected payload schema); no client
  or server code accompanies this document.
- **Stream adapter** — reuses the Workshop IoT streaming pattern already sketched in
  `WORKSHOP_TOOLS.md` Phase 3, once that pattern exists.

## 9. Example Workflow (Illustrative)

"County CMS nightly extract → Parameters inbox → Case + Party entities appear in a Justice
Space." Concretely: the county's CMS exports its nightly delta as Parameters-shaped JSON to a
watched directory; the (future) Parameters → Class Mapper reads it, creates/updates `Case` and
`Party` entities under the active Justice Space, and the Justice ribbon's Metrics tab picks up
the resulting `MetricSeries`/Dataset instances the same way it would for any other Dataset.
Nothing here is implemented; this is the target shape the eventual Parameters adapter should
satisfy.

## 10. Extension Points

A jurisdiction or vendor adds a new Parameter set or class mapping by:
1. Defining new Parameter `id`s and `mapping_hint`s (no core code change).
2. Registering a new `ClassName` variant if the target concept doesn't already exist, via
   whichever extension path is live at the time (today, the ad hoc multi-file edit §3 describes;
   `CLASS_REGISTRY.md`'s proposed registry, if it ships first).
3. Optionally adding ribbon buttons that operate on the new class — the existing mode-manifest
   custom-tab system (`crates/engine/modes/*.toml`) already supports this without core changes.

No jurisdiction-specific logic needs to live in the core engine; a jurisdiction-specific adapter,
if one is ever built, belongs in a separate, optional crate that the core never requires.

## 11. How This Fits the Justice/Legal Ribbon Work

The Justice and Legal ribbons (`crates/engine/modes/justice.toml`, `legal.toml`) operate
exclusively on whatever `ClassName` entities and Datasets exist — today, that's entities created
directly in the Studio; under this proposal, it would equally be entities that arrived via
Parameters. The ribbons do not need to change to support either source. Their Metrics tabs'
`data:chart`/`data:stats`/`data:grid` buttons already operate on Datasets regardless of origin —
Parameters would just be one more legitimate way a Dataset came to exist, with provenance
attached.

## 12. Status

This is a proposal. Nothing in this document is implemented. The next concrete step, if the
direction is approved, is a Wave-1-style spec (mirroring `CLASS_REGISTRY.md`'s format) for the
Parameters → Class Mapper itself — still no external network code, just the internal contract.
