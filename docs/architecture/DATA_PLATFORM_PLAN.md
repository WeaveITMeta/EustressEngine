# Data Platform Plan — Logger Pro X and Beyond · Studio as a Data Engine

**Status:** ACTIVE TRACK (2026-06-20). Living plan. This document is canonical for the
data platform (datasets, charts, grids, ETL, analysis, copilot, digital-twin, BI). It does
NOT supersede `docs/architecture/SCALING_ARCHITECTURE.md` — it sits beside it and the two
heavily overlap at the persistence layer. Authority boundary: **scene-graph entity state is
owned by `SCALING_ARCHITECTURE.md` (the `entities`/`tree` partitions); observational /
measured / computed data is owned by THIS doc (the new `datasets`/`timeseries` partitions);
and the *meaning/governance* of data — typed key, domain, unit, consent class, connector
taxonomy, mapping target — is owned by the Eustress Parameters fabric
([`parameters.rs`](eustress/crates/common/src/parameters.rs)), which supplies the *schema* while
this doc supplies the *implementations* that land under it (§3.5).** Parameters' own
"single source of truth for all data flow" claim is scoped accordingly to *semantics and
governance*, not transport — today that fabric is declared-but-dormant scaffolding (§3.5.2).
A `Part`'s position is scene state; a sensor's 100k-sample pressure trace is a Dataset. Where
analysis routes into the solver or FEA, `docs/architecture/CAD_PLATFORM_PLAN.md` remains
canonical for *how* those compute (Phase C solver, Phase D FEA); this doc is canonical for
*how analysis requests are routed into them and how their results become datasets.*
**Owner:** Engine core.
**Last revised:** 2026-06-20.
**Target:** make Eustress Studio **data-centric** — manual entry, data science, BI, ETL,
AND the closed-loop digital-twin (collect → fit → simulate → compare) — all over **one
columnar substrate** (Polars/Arrow in one leaf crate, on by default since 2026-06-23), with **GPU-drawn charts** that
scale to **millions of points** by the same HLOD/Morton/decimation machinery that streams the
3D world. **Logger Pro X is the floor, not the ceiling.**

---

## 0. Read this first — the one idea the obvious build gets wrong

The obvious build is: ship a dataframe panel, a chart widget, a grid widget, and a sidebar
LLM that writes Polars. That is a worse Jupyter-with-Copilot bolted onto a game engine, and
it throws away the only things Eustress has that a notebook never will — **the engine already
produces measured data every tick, already runs experiments, already has a CoW-branchable
database, already has GPU instancing that draws 10K+ unique entities in one call, and already
has an agent living inside the running `World`.**

The single load-bearing realization:

> **"Eustress is already a data engine. We are not adding data collection — we are giving
> *identity and a columnar substrate* to data the engine already produces, and *pointing the
> renderer we already have* at a 2D camera. A `Dataset` becomes a first-class noun in the
> tree; every data surface — grid, chart, notebook, dashboard, 3D overlay — is a linked view
> over that one substrate."**

The mental model is **linked views over one substrate**. There is exactly one source of
truth for selection (`DataSelection`), one source of truth for run/experiment identity (the
existing experiment JSON + `SimRecord`, unified on a single `RunId`), and one columnar store
(the `datasets`/`timeseries` Fjall partitions hydrated into an `eustress-data` `Frame`).
Every panel is a projection. Scrubbing a chart timeline recolors the 3D model through its
recorded history; selecting a part highlights its rows in the grid and its series in the
chart. This is the discipline the telemetry stack already uses (the `WatchPointRegistry` is
the source; `sim_values`, `runtime-snapshot.json`, `telemetry.jsonl` are projections).

Therefore the architecture is, end to end:

| Layer | What it is | Lives where | Owns storage? |
|---|---|---|---|
| **Substrate** | `Dataset`/`Series`/`Column`/`Run` instances + `datasets`/`timeseries` partitions | tree + WorldDb | **Yes** — the source of truth |
| **Frame** | in-memory columnar working set (Polars/Arrow) | `eustress-data` leaf crate | No — hydrated from substrate |
| **Views** | GPU charts, editable grids, forms, notebooks, dashboards, 3D overlays | engine + Slint | No — projections of `DataSelection` + Frames |
| **Compute** | transforms, fits, FFT, FEA, ML | `eustress-data` + Rune/Luau + solver + embedvec | No — produces provenance-stamped Datasets |
| **Loop** | collect → fit → instantiate → run → compare → branch | tools + bridge + CoW branches | No — composes existing verbs |

Everything below is mostly **wiring on top of reality**, not research. Where a primitive
already exists, this plan maps onto it by name and refuses to invent a parallel one.

---

## 0.5 Design invariants (non-negotiable)

These gate every phase. They are referenced by code throughout (`D1`, `D4`, …).

### D1 — An Experiment is a branch (with a stated, honest exception)
An experiment runs on a WorldDb copy-on-write branch
([`branch.rs`](eustress/crates/worlddb/src/branch.rs)) so the measured baseline is never
clobbered; comparing runs is comparing branch digests
([`branch.rs:211`](eustress/crates/worlddb/src/branch.rs)); keeping a run is `commit()`,
discarding is `discard()`. **The tension, stated plainly:** the overlay is an in-RAM
`BTreeMap` ([`branch.rs:116`](eustress/crates/worlddb/src/branch.rs)) explicitly sized for
*perturbation-proportional* writes. A high-rate sensor `Series` (millions of rows) would blow
that overlay. **So D1 holds verbatim for LOW-rate runs and for the materialized
`datasets`-blob level; high-rate `timeseries` rows are NOT branched through the RAM
overlay** — live rows record to the *parent* `timeseries`, and the branch snapshots them into
a branched `datasets` blob at branch time. D1 is the invariant; the blob-level workaround is
the mechanism that makes it survivable. Do not call D1 "free."

### D2 — One columnar leaf (ON BY DEFAULT as of 2026-06-23)
Polars/Arrow live in exactly one crate, `eustress/crates/data` (`eustress-data`), and **never**
in `common`, `cad`, `worlddb`, `stream`, or the non-optional `eustress-tools` — that crate is the
single home of arrow/parquet/polars.

**Reversal (2026-06-23, user decision):** the platform is data-centric, so the `data` feature now
ships in the engine's `default` tier ([`engine/Cargo.toml`](eustress/crates/engine/Cargo.toml),
`default = ["core", "data"]`) — the columnar leaf is part of the everyday `cargo run` build, not
an opt-in flag. The original "off-by-default / byte-for-byte-unchanged default graph" rule is
**retired**, and the `data-graph-purity` CI guard was inverted (now `data-graph-default`) to
assert the leaf **is** present in the default graph. Still enforced:
- **One leaf only**: arrow/parquet/polars never leak into `common`/`cad`/`worlddb`/`stream`/`eustress-tools`.
- **A polars-free public surface**: callers name `eustress_data::Frame`/`Series`/`Column`,
  never `polars::*`/`arrow::*` — the dep stays swappable (the discipline `worlddb` uses for
  `fjall`).

### D3 — Dimensioned at the boundary, never after
Every value entering a `Series` carries a `Dimension`
([`common/src/dimension.rs`](eustress/crates/common/src/dimension.rs), new) and a unit symbol
resolved **at ingest**. A raw `f64` with no unit is a defect, not a default. The on-disk
contract stays the **string symbol** (the established `metadata.unit` /
`WatchPoint.unit` contract), reconstituted to `Dimension` on read. Arithmetic is
runtime-checked: add/sub assert dimension equality, mul/div compose exponent vectors.

### D4 — No third run/dataset schema
Two run records exist and overlap: the experiment JSON file
([`simulation_tools.rs:1600`](eustress/crates/tools/src/simulation_tools.rs)) and the rkyv
`SimRecord`/`WorkshopIterationRecord`
([`sim_record.rs:91,331`](eustress/crates/common/src/sim_record.rs)). The platform **unifies
by joining on existing keys** (`WorkshopIterationRecord.sim_run_id` ↔ `SimRecord.run_id`;
experiment `config`+`timestamp` ↔ branch `exp/<name>-<ts>`). It adds an index, **not** a
third store. The experiment JSON shape stays **byte-stable** so `compare_runs`/
`list_experiments` keep working — a hard back-compat gate.

### D5 — One landing seam (the Recorder); one tool surface (three transports)
All durable ingest flows through **one** subscriber type that reads a `sensor.<name>` stream
topic and commits batched, dimensioned rows to WorldDb — never per-adapter persistence (the
choke point that stops a fifth/sixth/seventh on-disk format). And **every** new capability
ships as a `ToolHandler` that works identically in the in-engine Workshop, the out-of-process
MCP server, and the LSP: a new write op adds both an `op` arm in `drain_sim_commands`
([`plugin.rs:465`](eustress/crates/engine/src/simulation/plugin.rs)) **and** a bridge method
([`protocol.rs`](eustress/crates/engine/src/engine_bridge/protocol.rs)), or in-process and
out-of-process drift. This is the C2-equivalent reconciliation point for the data platform.

### D6 — Data classes are real tree Instances, not app state
`Dataset`/`Series`/`Column`/`Run`/`Chart`/`Dashboard`/`Model` are first-class `ClassName`
variants and `class_schema/*/_instance.toml` templates. The enum variant is **effectively
mandatory, not optional**: without it `from_str` fails → `spawn_instance` falls back to
`ClassName::Part`
([`instance_loader.rs:1800-1802`](eustress/crates/engine/src/space/instance_loader.rs)) →
auto-mesh → the data node renders as a gray block. Dashboards/charts persist as instances,
branch with the world, and are creatable/editable over MCP `create_entity`/`update_entity`.

---

## 1. The core reframe — Eustress already IS a data engine

The differentiators are *closer than they look* because the loop already exists end-to-end:

- [`run_experiment`](eustress/crates/tools/src/simulation_tools.rs) (`:1469`) already does:
  optional git branch `exp/<name>-<ts>` → apply `sim_values` overrides → `run_simulation` →
  poll `runtime-snapshot.json` until not-Playing → read telemetry → compute per-key stats →
  write `<universe>/.eustress/experiments/<name>-<ts>.json` (schema at `:1600`). The
  collect → run → record arc is **done**.
- [`compare_runs`](eustress/crates/tools/src/simulation_tools.rs) (`:1641`) already diffs two
  saved runs per-key with `higher_is_better` direction. The scalar compare arc is **done**.
- [`WatchPointRegistry.history`](eustress/crates/common/src/simulation/watchpoint.rs) (`:160`)
  is a real per-key time-series **with units** — the telemetry source of truth.
- CoW branches ([`branch.rs:1189`](eustress/crates/worlddb/src/branch.rs)) are O(1) to create
  and free to discard — the substrate for "branch and iterate."
- GPU instancing renders 10K+ unique-color instances in one draw call
  ([`instanced_pbr.rs`](eustress/crates/engine/src/rendering/instanced_pbr.rs)); the AI camera
  proves a second `Camera3d → RenderTarget::Image` works
  ([`ai_camera.rs:85-150`](eustress/crates/engine/src/ai_camera.rs)).

What is missing is identity (the `Dataset` noun), the columnar substrate, the *fit*, the
*instantiate-back*, a real query front door, the GPU chart pipeline, the editable grid, and
the dashboard-as-instance surface. That is this plan's scope.

---

## 2. Logger-Pro feature → Eustress primitive → gap

| Logger Pro X capability | Eustress primitive it maps onto | Gap (what this plan builds) |
|---|---|---|
| Sensor data collection (LabQuest/USB/BLE) | `sensor.<name>` stream topics + the Recorder seam | hardware adapters (Subsystem D) + durable `timeseries` partition |
| Live graph as data streams in | GPU chart camera (Subsystem V) + `DataSelection` scrubber | the chart pipeline + linked-views model |
| Data table (manual + collected) | editable `DataGrid` over a `Series` (Subsystem V.6) | net-new custom Slint grid (no table widget exists today) + validation |
| Manual data entry + typed columns | `class_schema` rich-attribute validation (Subsystem D.3) | cell-commit validation pipeline + forms = Properties-rows |
| Curve fit (linear/poly/exp/power) | `eustress-data::fit` routed to the CAD solver (Subsystem C) | `fit_model` tool + typed `Quantity` coefficients |
| Statistics (mean/min/max/integral/derivative) | `TimeSeries::compute_stats` wrapped (Subsystem C.2) | dimension-propagating numerics in `eustress-data` |
| FFT / spectral | `eustress-data::spectral` (rustfft) (Subsystem C.4) | the FFT kernel + uniform-sampling check |
| Calculated columns | Rune/Luau `data` module (Subsystem C.1) | `col`/`put_col`/`map_col` + dimension algebra |
| CSV/text import & export | `eustress-data` Parquet/CSV/JSON I/O (Subsystem B/D) | `ImportConnector` + schema-on-import |
| Multiple runs / overlay / compare | experiment JSON + `compare_runs` (already real) | overlay-measured-vs-simulated chart mode + residual series |
| Replay / playback | `DataSelection.cursor_t` scrub projects to 3D + grid + chart | the projection systems (Subsystem V.4) |
| (beyond Logger Pro) digital twin | `run_experiment` + CoW branch + `instantiate_model` (Subsystem F) | the closed loop + fit-back-into-sim |
| (beyond Logger Pro) BI dashboards | `Dashboard`/`ChartPanel`/`StatCard`/`Grid` instances (Subsystem F.6) | dashboard-as-instance + `build_dashboard` tool |
| (beyond Logger Pro) AI copilot | the in-engine agent over MCP (Subsystem F.2) | fit/anomaly/dashboard-from-a-sentence tools |
| (beyond Logger Pro) 3D-spatial data | `InstanceColor` recolor of live scene (Subsystem V.2f) | metric → color-ramp system + heatmap quad |

---

## 3. Where we are today (grounded baseline)

Real file pointers and measured truths the plan builds on — not aspiration:

- **Run model exists** — [`run_experiment`](eustress/crates/tools/src/simulation_tools.rs)
  (`:1469`), [`compare_runs`](eustress/crates/tools/src/simulation_tools.rs) (`:1641`),
  [`list_experiments`](eustress/crates/tools/src/simulation_tools.rs) (`:1765`); JSON at
  `<universe>/.eustress/experiments/*.json`.
- **rkyv run records exist** —
  [`SimRecord`/`WorkshopIterationRecord`](eustress/crates/common/src/sim_record.rs) (`:91`,
  `:331`) over [`sim_stream.rs`](eustress/crates/common/src/sim_stream.rs) topics.
- **Telemetry source of truth** —
  [`WatchPointRegistry`](eustress/crates/common/src/simulation/watchpoint.rs) (`:160`,
  `WatchPoint` carries `name`/`unit`/`history: VecDeque<DataPoint>`).
- **Stream core is real but durability is OFF** — `init_change_queue` builds `.in_memory()`
  ([`change_queue.rs:169-174`](eustress/crates/common/src/change_queue.rs)); ring is 65,536
  slots (~65 s at 1 kHz); `MmapBackend::read_at` cannot span segments
  ([`mmap.rs:91-98`](eustress/crates/stream/src/storage/mmap.rs)). Durability must live in
  WorldDb, not the stream's own segment log.
- **Stubs to fill** —
  [`query_stream_events`](eustress/crates/tools/src/memory_tools.rs) (`:231-270`) returns a
  canned message; [`raycast`](eustress/crates/tools/src/simulation_tools.rs) (`:344`) is a
  stub (needs an `ecs.raycast` bridge method).
- **Stats already exist twice** —
  [`TimeSeries::compute_stats`](eustress/crates/common/src/simulation/recorder.rs) (`:116`)
  and `compute_telemetry_stats`
  ([`simulation_tools.rs`](eustress/crates/tools/src/simulation_tools.rs)). Do NOT
  re-implement; wrap them.
- **Recorder-tee precedent** —
  [`HistoryStreamPlugin`](eustress/crates/engine/src/history_stream.rs) (`:33-58`) tees
  `UndoStack` onto `history.<kind>`; the durable Recorder copies this.
- **Render precedents** — [`ai_camera.rs`](eustress/crates/engine/src/ai_camera.rs) (off-screen
  `RenderTarget::Image` + readback), [`instanced_pbr.rs`](eustress/crates/engine/src/rendering/instanced_pbr.rs)
  (GPU instancing), [`billboard_pipeline.rs`](eustress/crates/engine/src/billboard_pipeline.rs)
  (custom WGSL + `Transparent3d` + quad expansion).
- **Slint host precedent** — the bottom-slot mode switch
  ([`main.slint:2951-3015`](eustress/crates/engine/ui/slint/main.slint)) shared by
  Output/Timeline; the Properties model/callback/hash-gate/focus-pause discipline
  ([`properties.slint`](eustress/crates/engine/ui/slint/properties.slint),
  [`slint_ui.rs:15107-15277`](eustress/crates/engine/src/ui/slint_ui.rs)).
- **Greenfield columnar** — `polars`/`arrow`/`parquet` appear **nowhere** in the tree (0 of
  1959 crates in `Cargo.lock`); `eustress-data` is referenced nowhere. Clean leaf add.
- **The pressure point** — `color_manifest.parquet` is *deliberately deferred* to a serde_json
  stopgap ([`color_manifest.rs:6`](eustress/crates/roblox-import/src/color_manifest.rs)) to
  avoid pulling parquet into a leaf crate. `eustress-data` gives it a home.

---

## 3.5 Relationship to Eustress Parameters (the integration & governance fabric)

> **Parameters is the typed-metadata, connector-taxonomy, governance, and routing *plane*; the Data Platform is the columnar storage, analysis, visualization, and simulation *plane*. They are two planes over one tree, not two stores. Parameters declares the *schema, taxonomy, and consent rules*; the Data Platform supplies the *implementations* that land under them.** Today the Parameters plane is almost entirely declared-but-dormant scaffolding — so the honest framing is: this doc inherits its *vocabulary*, not its *transport*.

The Eustress Parameters subsystem ([`parameters.rs`](eustress/crates/common/src/parameters.rs)) opens its module doc by calling itself "the single source of truth for all data flow" ([`parameters.rs:1-7`](eustress/crates/common/src/parameters.rs)). This plan claims, in turn, "one columnar substrate" (§0, §Subsystem A). Read literally the two claims collide. They do not actually overlap, because each is the source of truth for a **different kind of thing** — and a grounding sweep shows the Parameters claim is *design intent for a framework that has no live producers, consumers, or transport*, not a description of running behavior.

### 3.5.1 The layer split (who owns what)

| Concern | Owner | Why |
| --- | --- | --- |
| *What a value means* — typed key, domain, unit, consent class, source provenance | **Parameters** (`InstanceParameters` / `GlobalParameters` / `DomainRegistry`, [`parameters.rs:231-302`](eustress/crates/common/src/parameters.rs)) | Parameters is metadata-about-values; it carries `ParameterValue` typing, domain bucketing, and consent. |
| *Where bytes live & how they're queried* — `Dataset`/`Series`/`Column`/`Run`, the Recorder seam, the query front door | **Data Platform** (Subsystem A; D5 Recorder) | The columnar substrate is value-storage, not value-meaning. |
| *Catalog of connector kinds* — the ~55-variant `DataSourceType`, the 7-variant `ExportTargetType` ([`parameters.rs:584-647`](eustress/crates/common/src/parameters.rs), [`:138-154`](eustress/crates/common/src/parameters.rs)) | **Parameters** (taxonomy) | These are an enumerated vocabulary + display/category helpers, nothing more — no variant is matched to open a connection anywhere in the tree. |
| *Working transport for those kinds* — actual CSV/Parquet/REST/SQL read, actual webhook/file/embedvec export | **Data Platform** (Subsystem D import connectors; the Recorder/exporter implementations) | The implementations land *under* the taxonomy the way `class_schema` rows land under a `ClassName`. They do not exist yet; D builds them. |
| *Routing/governance — consent gating, anonymization at export, change-bus topics* | **Parameters** declares the *policy*; **Data Platform** *enforces* it at the materialize/export boundary | Policy is metadata; enforcement is a step in the storage/export path. |

Read as a single rule, **this resolves the dueling claims cleanly**: Parameters is the single source of truth for *what data means and what may be done with it*; the Data Platform is the single source of truth for *where the bytes are and how they are analyzed*. Neither is the source of truth for the other's domain. The phrase "single source of truth for all data flow" is downgraded — per grounding it is the module's aspiration, "design intent, not current behavior" — to "single source of truth for data **semantics and governance**."

### 3.5.2 Honest implementation state (grounded, not aspirational)

The Parameters plane is overwhelmingly **declared but inert**. This section does not contradict that; it builds on top of it.

- **Connectors are taxonomy-only — there is no transport behind them.** `DataSourceType`, `ExportTargetType`, and `DataSourceConfig` carry **zero transport**. A tree-wide search for `DataSourceType::` / `ExportTargetType::` dispatch finds **no `match source_type { … => open connection }` arm anywhere** — the big source enum is read only for `display_name()`/`category()`/`all_variants()` UI helpers ([`parameters.rs:649-666`](eustress/crates/common/src/parameters.rs)); `DataSourceConfig` is constructed in exactly one place (the "Add Parameters" UI action, [`world_view.rs:1293`](eustress/crates/engine/src/ui/world_view.rs)) and read in exactly one place to display the chosen type ([`world_view.rs:630`](eustress/crates/engine/src/ui/world_view.rs)). `ExportTargetType` has **zero variant-matching call sites**.
- **The router/export path is a dead loop.** `ParameterRouter::publish_export`/`buffer_export`/`drain_pending` ([`parameters.rs:355,377,385`](eustress/crates/common/src/parameters.rs)) have **no callers** anywhere in the workspace. The stream bridges `bridge_parameter_changed_to_stream`/`bridge_export_requests_to_stream` ([`parameters.rs:512,536`](eustress/crates/common/src/parameters.rs)) are registered as observers under `ParametersPlugin` but never fire, because `ParameterChangedEvent`/`ExportRequestEvent` are **never emitted** (no `.write`/`.trigger`) anywhere in the tree. Consequently the `parameter.changed`/`parameter.exports`/`parameter.export_requests` topics ([`parameters.rs:315-321`](eustress/crates/common/src/parameters.rs)) are **empty channels** — no producer, no subscriber. (The live MCP/export path runs entirely on the unrelated `mcp.*` topic family.)
- **The typed 3-tier model never touches an entity.** `InstanceParameters` is a `#[derive(Component, Reflect)] #[reflect(Component)]` on paper ([`parameters.rs:231-236`](eustress/crates/common/src/parameters.rs)) but is never inserted, queried, or `register_type`'d; the live spawn path attaches material/thermo/echem/nuclear/tags/attributes/measure_unit but **never `InstanceParameters`** ([`instance_loader.rs:2137-2283`](eustress/crates/engine/src/space/instance_loader.rs)). The typed `ParameterValue` enum is unused at runtime.
- **The only Parameters-family type with a live persistence touchpoint** is `GlobalParametersRegistry`, serialized into the **deprecated, UI-unreachable** `.scene.json` path ([`scene.rs:372-388,803-827`](eustress/crates/engine/src/serialization/scene.rs); deprecation: [`SERIALIZATION_AUDIT.md:37,81-89,173-176`](docs/development/SERIALIZATION_AUDIT.md)) — itself dead code.

**Framing consequence:** because the connector/router/topic machinery is taxonomy-or-dormant, **Parameters supplies the schema/taxonomy/governance and the Data Platform supplies the implementations that land under it.** Where this doc names a "Parameters connector," it means the *enum variant and its consent/domain metadata*; the working code that fetches or writes is a Data-Platform `import_dataset`/`export_dataset` implementation (Subsystem D) registered against that variant — exactly as a `class_schema` row implements a `ClassName`. This is the same discipline as D6 (data classes are real tree Instances, not app state): the Parameters enum is the *name*, the Data Platform supplies the *behavior*.

### 3.5.3 Storage reconciliation (EEP file-first vs WorldDb-primary)

The EEP v2 spec describes a **file-first** Parameters store: a global registry file plus per-domain `.eustress/parameters/global.toml` / `{domain}.toml` ([`EEP_SPECIFICATION.md:1272-1285,1613`](docs/EEP_SPECIFICATION.md)). **That model is both unimplemented and superseded.** Nothing reads or writes any `.eustress/parameters/` path — those strings exist only in the spec prose (verified by source grep) — and the typed 3-tier model behind it was never wired at runtime, so this is *not* "deprecated after working." The broader file-system-first stance was itself superseded by the binary pivot. The canonical direction is **WorldDb-primary**: TOML `.glb.toml` is the *authoring* source of truth, the `.eustress` binary archive is the *primary runtime + distribution* format ([`SERIALIZATION_AUDIT.md:33-34`](docs/development/SERIALIZATION_AUDIT.md)).

Where each tier *actually* lives today, versus where this plan puts it:

| EEP tier | Spec claim (file-first) | Grounded reality today | This plan's target |
| --- | --- | --- | --- |
| **Instance** parameters | typed `InstanceParameters` component, consent-gated | untyped `Option<HashMap<String, toml::Value>>` on `InstanceDefinition` ([`instance_loader.rs:64-66`](eustress/crates/engine/src/space/instance_loader.rs)), folded into the WorldDb archive `extra` cold tail under `__parameters` ([`arch_instance.rs:56,147-150,211,247`](eustress/crates/engine/src/space/arch_instance.rs)), mirrored to the TOML `[parameters]` section, shown read-only in Properties — the typed component is never inserted | stays in the WorldDb archive (the `entities`/`tree` partitions, owned by `SCALING_ARCHITECTURE.md`); the Data Platform reads it as **provenance metadata** on a `Dataset`, never as a parallel store |
| **Domain** registry | per-domain `.eustress/parameters/{domain}.toml` | **does not exist on disk**; `DomainRegistry` is an empty default resource | becomes a **schema overlay** the Data Platform consults for unit/consent defaults at import (§3.5.4), persisted alongside `class_schema`, **not** under `.eustress/parameters/` |
| **Global** registry | `.eustress/parameters/global.toml` | only `GlobalParametersRegistry` in the **dead** `.scene.json` path | folded into WorldDb/`class_schema` discipline; no standalone `.eustress/parameters/` files are introduced |

The Data Platform therefore **does not inherit EEP's file-first storage model.** It honors the WorldDb authority boundary already set in this doc's header: scene/entity state (including the untyped instance `[parameters]` map) is owned by `SCALING_ARCHITECTURE.md`; observational/measured/computed data is owned by THIS doc's new `datasets`/`timeseries` partitions. Parameters metadata rides the former as provenance; Dataset bytes live in the latter.

### 3.5.4 Touchpoints (how the two planes meet — what we build, against what exists)

Each touchpoint names the *aspirational* Parameters seam and the *real* Data-Platform code that implements it, so the integration is concrete rather than another layer of dormant wiring. In every case the Parameters side is the contract and the Data Platform side is the missing body.

1. **`parameter.*` topics as Recorder sources — but route through the live `sensor.<name>` bus, and wire the missing emitters first.** The Recorder (D5) is the single durable writer. The `parameter.changed`/`parameter.exports` topics are empty channels with no emitters and no subscribers ([`parameters.rs:315-321`](eustress/crates/common/src/parameters.rs)); the live export pipeline runs entirely on the `mcp.*` topic family instead. So: when a Parameter value *does* begin changing at runtime, its emitter publishes onto a `sensor.<name>` topic (D.1) that the Recorder already drains — and only *then*, as a thin secondary projection, do we wire the missing producer: trigger `ParameterChangedEvent` (or call `ParameterRouter::publish_export`) so the existing bridge ([`parameters.rs:512`](eustress/crates/common/src/parameters.rs)) finally carries traffic. We do **not** make the Recorder depend on the dead `parameter.*` topics; we give those topics their missing producer by teeing off the live bus, mirroring the `HistoryStreamPlugin` recorder-tee precedent ([`history_stream.rs:33-58`](eustress/crates/engine/src/history_stream.rs)).

2. **Consent + `AnonymizationMode` inherited at materialize/export.** Parameters declares the *policy* (consent class + anonymization on `InstanceParameters` / `DomainRegistry`); the Data Platform *enforces* it at exactly two boundaries it owns: (a) when the Recorder/import path **materializes** a `Series` from a Parameter-tagged source, the column carries forward the source's consent class as `Column` `class_schema` metadata; (b) when `export_dataset` (Subsystem D) writes out, it reads that metadata and applies `AnonymizationMode` before bytes leave the substrate. Because no real export today is keyed off `ExportTargetType` — the working webhook/file/embedvec targets live in the MCP crate over `EepExportRecord` ([`router.rs:204,285`](eustress/crates/mcp/src/router.rs)) on the `mcp.exports` topic — the enforcement hook goes **into the MCP exporter and the new `export_dataset`**, with the Parameters consent class as the policy input. The taxonomy says *what is allowed*; the Data-Platform exporter is the *only* place it is checked.

3. **`ParameterValue::Float` → dimensioned `Series`/`Column` via D3.** A Parameter's `Float` is a bare number; a `Column` is **dimensioned** (D3 — every numeric carries a unit; `Quantity::parse` + dimension-equality asserts). The bridge is mechanical: when a Parameter materializes into a `Column`, its declared domain/unit (from `DomainRegistry`, or the `metadata.unit` already on the instance) becomes the column's `dimension`, and the raw `Float` is converted to canonical SI at the boundary — the same ingest-time discipline D.3/D.4 already apply to manual entry and import. A typeless Parameter `Float` with no declared unit lands as a dimensionless column and is flagged, never silently treated as SI.

4. **Write-back symmetry: Parameters `MappingTargetType` ↔ `instantiate_model`.** Parameters' `DataMapping`/`FieldMapping` ([`parameters.rs`](eustress/crates/common/src/parameters.rs)) describe a *target* a value can be written back into; they are defined-and-serialized only, with no transform engine consuming them. The Data Platform closes this symmetrically: the same column→target mapping that *reads* a scene field into a `Series` is the one that **writes a fitted/simulated result back** onto the entity via the closed-loop `instantiate_model` path (collect → fit → simulate → compare, §Subsystem A/F). Read-mapping and write-mapping share one `FieldMapping` shape so a parameter that flows *in* as observation can flow *out* as a model-derived value through the **same** reflection target — and both go through `eustress_common::instance_create::create_instance` ([`instance_create.rs`](eustress/crates/common/src/instance_create.rs)) / `update_entity` so there is no second write path. This makes `MappingTargetType` the *declaration* and `instantiate_model` write-back the *implementation* — the §3.5.1 split applied to the loop's return leg.

**Net:** the Parameters plane gives this platform a ready-made vocabulary for *meaning, domain, consent, connector kind, and mapping target*. None of it carries live transport today. The Data Platform's job at every touchpoint above is to supply the missing implementation **under** that vocabulary — landing connectors against the taxonomy, enforcement at the export boundary, dimensioning at the materialize boundary, and write-back through the canonical create/update path — so that the "single source of truth for data flow" claim becomes true *as a semantics-and-governance claim*, while "one columnar substrate" stays true as the storage claim. Two planes, one tree, no overlap.

---

## Subsystem A — Data model & substrate (the four nouns)

**Authority:** owns the *shape, storage, and tree identity* of tabular/series data; owns the
**recorder seam** (stream topic → durable Series) and the **query front door** over results.
It does NOT own the live telemetry hot path (that is `EustressStream`).

### A.1 The class hierarchy — `DataService` → `Dataset` → `Series` → `Column`, plus `Run`

First-class **Instances in the tree**, non-3D container/leaf nodes spawned through the
**non-visual branch** of `spawn_instance`
([`instance_loader.rs:1929-1986`](eustress/crates/engine/src/space/instance_loader.rs)),
which attaches `Instance` + `Transform` + `Visibility` + `Attributes` and renders `[Section]`
TOML as Properties rows.

```
DataService                       (Service; container; data-driven, NO Rust enum needed)
├── Dataset "pressure_sweep"      (a logical table / experiment-collection)
│   ├── Series "run_001"          (one observation set — one Run's recording)
│   │   ├── Column "t"     (dtype=f64, dim=TIME)
│   │   ├── Column "psi"   (dtype=f64, dim=PRESSURE, unit="psi")
│   │   └── Column "valid" (dtype=bool)
│   └── Series "run_002" ...
└── Run "run_001"                 (provenance node; points AT a Series)
```

Why both `Series` and `Run`? **`Series` is the data (columns); `Run` is the provenance
(config, branch, wall-time, fitness).** A `Run` references the `Series` it produced. This maps
the two existing systems cleanly: experiment-JSON `config`/`stats` → `Run`;
`final_values`/per-tick telemetry → `Series` columns. One-to-one in the common case; a
Monte-Carlo `SimRecord` with N posteriors → N series.

**`DataService` needs ZERO Rust enum edits** (services are data-driven; unknown service
classes map to `ClassName::Folder` at
[`service_loader.rs:223-227`](eustress/crates/engine/src/space/service_loader.rs)) — only
`common/assets/service_templates/DataService/_service.toml` (`can_have_children = true`),
optionally `service_properties/DataService.toml` and a `default_scene.rs` seed.

**`Dataset`/`Series`/`Column`/`Run` need schema templates + enum variants (required, D6):**
`class_schema/{Dataset,Series,Column,Run}/_instance.toml` (model on `Folder` — no `[asset]`,
no `[transform]`, forcing the non-visual branch) plus the **three exhaustive matches** in
[`classes.rs`](eustress/crates/common/src/classes.rs): add variants to the `enum ClassName`
body (declared at `:212`), then extend `as_str` (~`:504`) and `from_str` (~`:775`).

```toml
# common/assets/class_schema/Column/_instance.toml
[metadata]
class_name = "Column"
archivable = true

# Rendered as Attributes rows by the rich-schema parser (instance_loader.rs:1937-1958).
[Schema]
dtype     = { type = "string", value = "f64", description = "f64|i64|bool|str|datetime" }
unit      = { type = "string", value = "",    description = "Symbol, e.g. \"psi\"" }
dim       = { type = "string", value = "",    description = "SI exponent, \"si:M1L-1T-2\" or \"PRESSURE\"" }
length    = { type = "int",    value = 0,     description = "Row count" }
layout    = { type = "string", value = "row", description = "row|block (timeseries layout, A.7)" }
chunk_ref = { type = "string", value = "",    description = "datasets-partition key prefix" }
```

```toml
# common/assets/class_schema/Run/_instance.toml
[metadata]
class_name = "Run"
archivable = true

[Run]
status      = { type = "string", value = "pending", description = "pending|running|complete|failed" }
branch      = { type = "string", value = "",  description = "Git branch + WorldDb CoW branch digest" }
series_ref  = { type = "string", value = "",  description = "UUID of the Series this Run produced" }
duration_s  = { type = "float",  value = 0.0 }
time_scale  = { type = "float",  value = 1.0 }
wall_time_s = { type = "float",  value = 0.0 }
sim_run_id  = { type = "string", value = "",  description = "Join key to SimRecord.run_id" }
```

**Typed ECS components — only if scripts/systems must query columns typed.** The non-visual
branch attaches generic `Attributes`; for typed queries add `DatasetColumn`/`RunRef`
components in [`classes.rs`](eustress/crates/common/src/classes.rs) and insert them inline in
the non-visual branch (the dormant `ClassSpawner` registry is unused —
[`plugin.rs:79-92`](eustress/crates/engine/src/class_registry/plugin.rs) registers zero
spawners — so wire inline):

```rust
#[derive(Component, Reflect, Serialize, Deserialize, Clone, Debug)]
#[reflect(Component)]
pub struct DatasetColumn {
    pub dtype: ColumnDtype,          // F64 | I64 | Bool | Str | DateTime
    pub dim: Dimension,              // SI exponent vector (A.4)
    pub unit_symbol: Option<String>, // disk contract is the symbol string
    pub length: u64,
    pub chunk_ref: ChunkRef,
}
```

**Hierarchy discipline (a real gap).** The tree does NOT enforce `Column`-only-under-`Series`
(`can_have_children` exists only on services,
[`service_loader.rs:79`](eustress/crates/engine/src/space/service_loader.rs)). **Parent-validity
is advisory at insert and enforced only at materialization** (when a `Series` flushes to
`datasets`, non-`Column` children are dropped with a warning). Gate the materializer, not the
tree editor. Suppress BasePart Properties noise (Color/Material/Transform from the `else`
branch at [`slint_ui.rs:14603-14718`](eustress/crates/engine/src/ui/slint_ui.rs)) by adding an
`is_data_class` predicate beside `is_ui_class`
([`slint_ui.rs:14634-14639`](eustress/crates/engine/src/ui/slint_ui.rs)).

### A.2 The `eustress-data` crate (the columnar leaf, D2)

`eustress/crates/data` (`eustress-data`), added to workspace members at
[`eustress/Cargo.toml:36`](eustress/Cargo.toml), engine depends `optional = true`. Polars must
**never** enter `common`, `cad`, `worlddb`, `stream`, or the non-optional `eustress-tools`.

```toml
# eustress/crates/data/Cargo.toml
[features]
default = []                                          # leaf does nothing unless asked
parquet = ["dep:arrow", "dep:parquet"]                # arrow-rs (NOT arrow2), minimal MVP
frames  = ["parquet", "dep:polars-core", "dep:polars-io"]   # eager DataFrames, NO lazy engine
query   = ["frames", "dep:polars-lazy"]               # heavy, explicit opt-in
hw           = []                                      # hardware trait + registry only
hw-serial    = ["hw", "dep:serialport"]
hw-bluetooth = ["hw", "dep:btleplug"]
hw-osc       = ["hw", "dep:rosc"]
hw-mqtt      = ["hw", "dep:rumqttc"]
```

```toml
# eustress/crates/engine/Cargo.toml  (in `default` since 2026-06-23 — was opt-in)
eustress-data = { path = "../data", optional = true }
data        = ["dep:eustress-data", "eustress-data/parquet"]
data-frames = ["data", "eustress-data/frames"]
data-query  = ["data", "eustress-data/query"]   # visible-in-CI heavy opt-in
```

Polars-free public surface (callers never name `polars`/`arrow`):

```rust
// eustress/crates/data/src/lib.rs — the entire public vocabulary
pub struct Frame { /* opaque: polars::DataFrame OR Vec<Series> behind cfg + UnitTag sidecar */ }
pub struct ColumnSpec { pub name: String, pub dtype: ColumnDtype, pub unit: Option<String>, pub dim: Dimension }

pub fn write_parquet(path: &Path, frame: &Frame) -> Result<()>;
pub fn read_parquet(path: &Path) -> Result<Frame>;
pub fn frame_from_columns(cols: Vec<(ColumnSpec, ColumnData)>) -> Result<Frame>;
pub fn frame_to_chunks(frame: &Frame, target_rows: usize) -> Vec<Chunk>; // → timeseries partition
pub fn chunks_to_frame(chunks: Vec<Chunk>) -> Result<Frame>;
```

This crate is also the home for the deferred `color_manifest.parquet`
([`color_manifest.rs:6`](eustress/crates/roblox-import/src/color_manifest.rs)): `roblox-import`
gains an optional `eustress-data` dep behind a `parquet` feature, keeping the identical
`ColorRow` contract; default emit stays NDJSON for the engine-free build.

**Minimal viable recommendation:** ship `data` (arrow-rs parquet only) first. The named real
use cases — `color_manifest.parquet`, ML training exports, telemetry/profile dumps,
experiment columnarization — are *write-a-columnar-file* tasks, not query tasks. `frames`/
`query` land only when a notebook or BI surface needs joins/group-by/lazy streaming.

### A.3 WorldDb partitions — `datasets` (blobs) + `timeseries` (time-ordered rows)

| Partition | Holds | Access | Key shape |
|---|---|---|---|
| **`datasets`** | materialized columnar blobs (Parquet/Arrow IPC) per (Dataset, Series, Column) chunk | read-whole-column, large values | `chunk_ref` prefix |
| **`timeseries`** | live recorded rows landed off `sensor.<name>` before/instead of materialization | append-at-tail + time-window range scan | timestamp-prefixed |

Key encoding reuses the order-preserving `sort_to_be8(i64)` / `be8_to_sort`
([`fjall_backend.rs:66-73`](eustress/crates/worlddb/src/fjall_backend.rs)) so lexicographic
byte order == numeric order, the `\x1f` delimiter, and the tag/version discipline
([`keys.rs:263-265`](eustress/crates/worlddb/src/keys.rs)):

```rust
pub const TS_KEY_TAG: u8 = 0x14; pub const TS_KEY_VERSION: u8 = 0x01; // timeseries
pub const DS_BLOB_TAG: u8 = 0x15; pub const DS_BLOB_VERSION: u8 = 0x01; // datasets blob

// timeseries: [TS_TAG][TS_VER] series_uuid(16) \x1f sort_to_be8(ts_nanos) \x1f col_id(2)
//   → ascending range scan via partition.range(lo..=hi); EXACT, stops early.
// datasets:   [DS_TAG][DS_VER] dataset_uuid(16) \x1f series_uuid(16) \x1f col_id(2) \x1f chunk_seq_be8
// mesh-indexed (FEA, C.4): dataset_uuid \x1f field_id \x1f sort_to_be8(node_id)
```

**Do NOT use Morton for the time/node axis** — Morton
([`keys.rs:221`](eustress/crates/worlddb/src/keys.rs)) is 3D-spatial; its range scans return
supersets needing post-filter. **Do NOT reuse `ds_range`** (OrderedDataStore,
[`fjall_backend.rs:1090`](eustress/crates/worlddb/src/fjall_backend.rs)) for real
time-series — it scan-alls-then-sorts in memory, explicitly leaderboard-scale (≤ few hundred,
[`fjall_backend.rs:1102-1104`](eustress/crates/worlddb/src/fjall_backend.rs)).

**The one required `eustress-fjall` change.**
[`eustress-fjall/src/lib.rs:393`](eustress/crates/eustress-fjall/src/lib.rs) hardcodes
`PartitionCreateOptions::default()`. Add a `store_with_opts(name, opts)` variant, then create
the two partitions scan-tuned (`.block_size(32*1024)` — fjall says "for scan heavy workloads
use 16-64 KiB" — `.compression(Lz4)`, `.with_kv_separation(..)` for big blobs; KV-sep GC
becomes manual, so the compactor must trigger it). Wiring into `FjallWorldDb` is ~4 boilerplate
edits per the `voxels` template
([`fjall_backend.rs:625-707`](eustress/crates/worlddb/src/fjall_backend.rs)) + default trait
impls on `WorldDb` ([`backend.rs:368-416`](eustress/crates/worlddb/src/backend.rs)) so
non-Fjall backends compile, with a `publish_external` replication delta after each durable
write.

**CoW-branch participation is explicit per-namespace work, NOT automatic (D1).** A new
partition's methods fall to the default trait impls, which on a `BranchHandle` return EMPTY
(not read-through to parent) and no-op/err on write — **broken, not absent**. To make
`timeseries`/`datasets` branch-aware you must, per the established mechanism: (1) add overlay
fields ([`branch.rs:116`](eustress/crates/worlddb/src/branch.rs)); (2) extend `Overlay::len()`
([`branch.rs:148`](eustress/crates/worlddb/src/branch.rs)) and `digest()` with fresh namespace
tags **13/14** (tags 1–12 are in use today, so 13/14 are collision-free —
[`branch.rs:232-296`](eustress/crates/worlddb/src/branch.rs)); (3) override
reads overlay-then-parent, writes overlay-only, iterators merge (templates at
[`branch.rs:940-955`](eustress/crates/worlddb/src/branch.rs)); (4) add a commit-replay block
([`branch.rs:360`](eustress/crates/worlddb/src/branch.rs)). Implement the overlay for
`datasets` (experiments need it); **defer it for `timeseries`** (raw telemetry is rarely
branched and the RAM overlay can't hold millions of rows — D1). `commit()` is explicitly NOT
cross-partition atomic ([`branch.rs:38-42`](eustress/crates/worlddb/src/branch.rs)): replay
tree last so a partial failure leaves orphan blobs (recoverable), not dangling tree refs
(corrupt), and add a `Run.status = failed` reconcile sweep on open.

### A.4 General SI dimension system — extend `Quantity`, not replace it (D3)

The migration-safety bedrock: **the on-disk contract is strings everywhere** — CAD expression
strings ([`feature_tree.rs:32`](eustress/crates/cad/src/feature_tree.rs)
`variables: HashMap<String,String>`), `metadata.unit: Option<String>`
([`instance_loader.rs:706`](eustress/crates/engine/src/space/instance_loader.rs)), and the
binary rkyv core writes `unit: None`
([`world_db_binary.rs:202`](eustress/crates/engine/src/space/world_db_binary.rs)). The serde
derives on `Quantity`/`Unit` are dead weight on disk. **So we can reshape `Quantity` freely;
the only contracts that must hold are `Quantity::parse`, `Unit::from_symbol`, `Unit::symbol`.**

The primitive lives in `common/src/dimension.rs` (**SHIPPED** — wired at
[`common/src/lib.rs:37`](eustress/crates/common/src/lib.rs), beside `pub mod units;`).
**Correction (grounded 2026-06-20):** the earlier claim that "`common` is the lowest crate, so
`cad` imports `Dimension`" is WRONG. `cad` does **not** depend on `common` and must not — it is a
lean `truck`-only kernel ([`cad/Cargo.toml`](eustress/crates/cad/Cargo.toml): no bevy), whereas
`common` pulls all of bevy. So `Dimension` lives in `common` for the **ECS / Data-Platform side**
(the `DatasetColumn` component, the Recorder, the engine boundary), NOT as a shared base that
`cad` imports. The shipped type is **serde + std only and omits `Reflect`** (no bevy dep, no
array-reflect concern) — add `Reflect` only if/when it becomes an ECS component field. The two
unit islands are reconciled at the *engine boundary*, never by a `cad → common` edge.

```rust
// common/src/dimension.rs — SI base-dimension exponent vector [L, M, T, I, Θ, N, J]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, Reflect)]
pub struct Dimension(pub [i8; 7]);   // i8 covers any real derived unit (max |exp| ~4)
impl Dimension {
    pub const DIMENSIONLESS: Self = Self([0;7]);
    pub const LENGTH: Self = Self([1,0,0,0,0,0,0]);
    pub const MASS: Self   = Self([0,1,0,0,0,0,0]);
    pub const TIME: Self   = Self([0,0,1,0,0,0,0]);
    // FORCE = M·L·T⁻²; PRESSURE = M·L⁻¹·T⁻²; ENERGY = M·L²·T⁻² — by composition, no newtypes.
    pub const fn mul(self, o: Self) -> Self;  pub const fn div(self, o: Self) -> Self;
    pub const fn powi(self, n: i8) -> Self;
}
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
pub struct Quantity { pub si_value: f64, pub dim: Dimension }
```

**Reject compile-time (`PhantomData`/typenum) dimensions** — incompatible with serde-from-string
and would force a rewrite of every realism call site. The realism newtypes already give
compile-time safety where wanted; the CAD variable/expression flow needs a *runtime*
`Quantity`. **pH is `DIMENSIONLESS` with a named formatter**, NOT an exponent vector (it is
−log₁₀ of an activity); concentration `mol/m³` composes naturally as `[-3,0,0,0,0,1,0]`.

Migration shape (corrected 2026-06-20 — additive, string-contract-preserving):
1. **DROPPED: do NOT turn `cad::Quantity` into a façade over `common::Dimension`.** That would
   force `cad` to depend on `common` (→ bevy in the lean CAD kernel). `cad` keeps its own
   independent `Quantity`/5-arm `Unit` enum, and the `eval.rs` guards stay exactly as they are —
   **so there is no eval-guard rewrite and no regression risk.** CAD geometry only ever needs
   length/angle; general dimensions buy it nothing. Adding derived-unit symbols (`pa`/`v`/`w`/…)
   to a *cad* parse table is therefore also dropped — those live on the Data-Platform side.
2. The Data Platform gets dimensions from `common::dimension::Dimension::from_unit_symbol`
   (a unit string → `Dimension`) — the **SHIPPED** bridge. Columns store the canonical `si:`
   string (or a named symbol) and resolve it at the engine boundary; the lean `eustress-data`
   leaf carries `ColumnSpec.dimension` as an opaque string (no `common`/bevy dep). The risky
   eval-guard flip is no longer on the Data-Platform path at all.
3. If a realism telemetry tap ever wants to label a Series, the realism newtypes can gain an
   opt-in `impl From<Pascals> for common::Dimension` (or a `dimension()` method) — additive, on
   the `common` side where realism already lives. No `cad` involvement, no big-bang rewrite.
4. Keep `metadata.unit` a `String`. The length-only `units::Unit` enum
   ([`units.rs`](eustress/crates/common/src/units.rs)) stays a *magnitude-conversion* concern and
   is NOT extended with derived rows (pressure/voltage are not lengths). Derived dimensions flow
   through the shipped `Dimension::from_unit_symbol` / `to_si_string` instead: emit a named symbol
   when one exists (human-readable), else the canonical `"si:L1M1T-2"` exponent fallback.

### A.5 Recorder seam + the run unification (D4, D5)

The durable path is **off** in the engine ([`change_queue.rs:169-174`](eustress/crates/common/src/change_queue.rs))
and `MmapBackend::read_at` can't span segments — so do NOT rely on the stream's own segment
log. Instead, add **one** subscriber on `sensor.<name>` that decodes readings into
`timeseries` rows — mirroring the `HistoryStreamPlugin` tee
([`history_stream.rs:33-58`](eustress/crates/engine/src/history_stream.rs)). Keep the callback
trivial (push to a channel; heavy work inline stalls the probe); embed your own sim/monotonic
timestamp (ring replay zeroes the timestamp). The Recorder is the **single** durable subscriber
per topic; charting reads the materialized Series, not the live topic.

Run unification bridges the two existing records into one `RunId`, **not a third store**:

| Source today | Becomes | Join key |
|---|---|---|
| experiment JSON (`simulation_tools.rs:1600`) | a `Run` Instance + provenance fields | `RunId` ← `config`+`timestamp` |
| rkyv `SimRecord` (`sim_record.rs:91`) | same `Run` + N `Series` (one per posterior) | `SimRecord.run_id` |
| `WorkshopIterationRecord` (`sim_record.rs:331`) | a `Run` with `fitness` | `sim_run_id` |
| `WatchPoint.history` (`watchpoint.rs:35`) | the `Series`' `Column` rows | series_uuid |

Keep the experiment JSON schema byte-stable (D4). `compare_runs` evolves to read either the
JSON or the `Run`/`Series` DB form — comparison is then a `final_values` diff (today) or a
branch-digest diff (D1, [`branch.rs:211`](eustress/crates/worlddb/src/branch.rs)).

### A.6 Materialization/compaction + the query front door

A background system compacts closed-Series `timeseries` rows into a `datasets` Parquet blob via
`eustress-data::frame_to_chunks` → `put_dataset_chunk`, updates `Column.chunk_ref`, and triggers
KV-separation GC. This replaces the unbounded `telemetry.jsonl`
([`plugin.rs:571-573`](eustress/crates/engine/src/simulation/plugin.rs)) as the durable store
(JSONL demoted to an optional projection so `tail_telemetry` keeps working). Fill the
`query_stream_events` stub
([`memory_tools.rs:231-270`](eustress/crates/tools/src/memory_tools.rs)) as the front door over
`range_ts` (live) + `iter_dataset_chunks` (materialized) + the rkyv `SimQuery`
([`sim_stream.rs:113`](eustress/crates/common/src/sim_stream.rs)), with live-ring → durable →
legacy-JSONL routing.

### A.7 Open sub-decisions (data model)

1. **Per-row vs block-major `timeseries` layout.** Row-major `(series, ts, col_id)` is many
   tiny keys for wide frames; block-major `(series, ts)` packing all columns is cheaper to
   write/scan but fixes the column set. **Lean block-major for fixed-schema sensor frames,
   row-major for sparse** — decide per Series at creation via `Schema.layout`.
2. **`RunId` derivation when no `SimRecord` exists** — `hash(config, timestamp)` vs a minted
   u128 UUID. Lean UUID, adopt `SimRecord.run_id` when one exists.
3. **Eager-delete `timeseries` on materialization?** Lean: keep until the Run is committed
   (D1), then compact-and-drop on commit.
4. **CoW for `timeseries` specifically** — branch at the materialized `datasets`-blob level,
   never the live rows (D1). **Likely required**, highest-risk interaction.

---

## Subsystem V — Visualization: GPU charts, linked views, editable grids

> **The chart is not a widget. It is a camera.** A chart of a million points is the *same
> problem we already solved for geometry*, pointed at a 2D ortho camera instead of the scene.

### V.0 The render-target decision (LOCKED #1) — corrected against the live compositor

The naive framing said "surface the chart texture the same way Slint consumes the viewport
texture." **That is inverted and must be stated honestly.** The live overlay flows
Slint → Bevy: Slint software-renders into a CPU staging buffer that is `memcpy`'d INTO a Bevy
`Image` ([`slint_ui.rs:2556-2645`](eustress/crates/engine/src/ui/slint_ui.rs)). Slint never
samples a Bevy GPU texture; the true zero-copy `slint::Image::try_from(wgpu::Texture)` bridge
lives only in the **DISABLED** `slint_bevy_adapter.rs` (Skia ICU conflicts on Windows,
[`main.rs:143`](eustress/crates/engine/src/main.rs)). So there are two real paths:

| Approach | Precedent | Cost | Verdict |
|---|---|---|---|
| **B. On-screen `Camera3d` with `Camera.viewport` sub-rect** | the two-camera stack ([`slint_ui.rs:2135-2170`](eustress/crates/engine/src/ui/slint_ui.rs)); `camera.viewport` read at [`default_scene.rs:96`](eustress/crates/engine/src/default_scene.rs) | zero readback, zero texture copy — the 3D already renders full-window under the transparent Slint chrome | **CANONICAL** for the docked chart panel: needs zero new Slint↔Bevy plumbing |
| **A. `RenderTarget::Image` → readback → `slint::Image::from_rgba8`** | [`ai_camera.rs:85-150`](eustress/crates/engine/src/ai_camera.rs) | one off-screen pass + a per-frame **GPU→CPU readback** (the AI camera's ~3.7 MB/frame class) | **Deferred fast-path** for a freely-floatable/clipped panel; NOT "free," NOT the existing image bridge |

**Decision: Approach B is canonical.** A chart `Camera3d` (ortho, `order` between 0 and 300,
dedicated `RenderLayers`, `NoAtmosphere`, **active**, `viewport` synced from `ViewportBounds`)
draws into the docked rectangle under the chrome, exactly as the live viewport composites.
Approach A is the documented future option for a floating dashboard wall, and its real cost
(per-frame readback or re-enabling the blocked Skia adapter) is named, not hand-waved.
**Exit gate:** prove the sub-rect camera renders under the chrome (or a chart texture reaches a
Slint `Image`) before claiming docking parity.

The four hard caveats (production scars from `ai_camera.rs`, non-negotiable):
- **D-V1 — `NoAtmosphere` on the chart camera.** Two `Camera3d`s both carrying `Atmosphere`
  hit a multi-camera prepare-race that aborts wgpu
  ([`ai_camera.rs:133-149`](eustress/crates/engine/src/ai_camera.rs)). A chart has no sky.
- **D-V2 — the chart camera stays active.** An inactive image-target camera panics
  `prepare_mesh_view_bind_groups` ([`ai_camera.rs:18-26`](eustress/crates/engine/src/ai_camera.rs)).
  "Hide the panel" means stop sampling/skip the pass, never deactivate.
- **D-V3 — distinct `Camera.order` + dedicated `RenderLayers`.** Editor picking does
  `find(order == 0)`; a colliding order corrupts selection.
- **D-V4 — ortho `Camera3d`, not `Camera2d`** — `Camera2d` can't render `Mesh3d`/custom
  pipelines, and 3D-spatial overlay charts (V.2f) need a perspective sibling.

### V.1 Crate, classes, partition

Chart **rendering** lives in the engine (`eustress/crates/engine/src/charts/`:
`mod.rs`/`pipeline.rs`/`lod.rs`/`panel_sync.rs`, registered as `ChartPanelPlugin` next to
`BillboardPipelinePlugin`, [`main.rs:468`](eustress/crates/engine/src/main.rs)). Chart **data**
lives in `eustress-data`; with `data` off, `ChartPanelPlugin` compiles but renders an
empty-state. Classes: **`Chart`** (`chart_type`/`series_refs`/`x_column`/`y_columns`/axis
ranges/palette) and **`Dashboard`** (a container of charts + grids with a saved layout) — both
non-visual `ClassName` variants (D6). `find_entities_by_class("Chart")` works for free
(string-keyed index, [`backend.rs:510-554`](eustress/crates/worlddb/src/backend.rs)). Chart
definitions persist as instance attributes in `entities`; chart **source data** is the
`datasets`/`timeseries` partitions — the pipeline owns no storage.

### V.2 Chart taxonomy → GPU technique (Bevy UI is out for the data layer)

| Type | GPU technique | Template |
|---|---|---|
| **a. Scatter** (millions) | GPU instancing: one point quad, instance buffer `[x,y,color]`, one draw | [`instanced_pbr.rs`](eustress/crates/engine/src/rendering/instanced_pbr.rs) |
| **b. Line / time-series** | vertex-pull line shader: polyline as storage buffer, screen-space-thick quads in vertex stage | the quad expansion in [`billboard.wgsl`](eustress/crates/engine/assets/shaders/billboard.wgsl)/[`billboard_pipeline.rs:29-48`](eustress/crates/engine/src/billboard_pipeline.rs) |
| **c. Bar** | instanced quads (counts small) | `instanced_pbr.rs` |
| **d. Histogram** | CPU-bin in `eustress-data` (`group_by`), then bars | binning + (c) |
| **e. Heatmap** (millions of cells) | upload field as `R32Float` texture, one quad, colormap in fragment shader — **O(1) draws** | custom `SpecializedRenderPipeline` on [`billboard_pipeline.rs`](eustress/crates/engine/src/billboard_pipeline.rs) |
| **f. 3D-spatial overlay** | recolor live entities by a metric via `InstanceColor` ([`instanced_pbr.rs:25-34`](eustress/crates/engine/src/rendering/instanced_pbr.rs)) — **zero new pipeline** | the editor viewport itself, recolored by data |

Type (f) is the **load-bearing differentiator**: because charts are cameras into the same
render world, the data platform's "3D-spatial overlay" is the editor viewport recolored by
data — making the closed-loop twin *visible on the model*, not just a side panel. Alpha-blended
points/heatmaps queue in `Transparent3d`; solid scatter/bars in `Opaque3d`
([`billboard_pipeline.rs:42-44`](eustress/crates/engine/src/billboard_pipeline.rs)).

### V.3 Downsampling / LOD — "millions of points" is persistence, not per-frame draw

We never upload 10⁶ points per frame; we reuse the three in-house decimation levers:
- **V.3a HLOD-style decimated proxies (primary).** At ingest/commit, build a pyramid of
  pre-aggregated proxy buffers per series (min/max/mean per bucket at 1×/16×/256×/4096×),
  persisted as extra `timeseries` rows keyed `series_id \x1f lod_level \x1f sort_to_be8(t)`.
  Build-once/persist/visibility-toggle — identical to merged-cell proxies. At draw, pick the
  LOD whose bucket-width ≈ 1 screen pixel. **Min-max decimation (not averaging)** preserves the
  visual envelope — spikes survive, which curve-fit/FFT analysts require.
- **V.3b Morton-locality windowing.** Stream only the visible x-range at the chosen LOD via
  native `partition.range(lo..=hi)` (exact, early-terminating, NOT `ds_range`'s scan-all).
- **V.3c `mesh_optimizer` as budget governor.** `charts/lod.rs` walks up the pyramid until the
  visible-range point count fits a per-chart budget (e.g. 2M instances — a target, unmeasured on
  this hardware). Per frame the GPU sees
  at most ~(panel-width-px × series-count) decimated points; the two numbers are decoupled by
  exactly the HLOD+Morton+meshopt machinery the engine already ships.

### V.4 Linked views — grid ↔ chart ↔ 3D sync at a timestamp

One shared selection resource is the single source of truth; every surface is a projection.

```rust
// engine/src/charts/mod.rs
#[derive(Resource, Default, Clone)]
pub struct DataSelection {
    pub cursor_t: Option<i64>,             // focused instant (sim-time micros / row index)
    pub brush: Option<(i64, i64)>,         // brushed x-range
    pub series: smallvec::SmallVec<[uuid::Uuid; 8]>,    // legend selection
    pub entities: smallvec::SmallVec<[uuid::Uuid; 16]>, // joins to 3D viewport selection
    pub rev: u64,                          // surfaces hash-gate on this to skip no-op pushes
}
```

Three bidirectional edges, all on existing seams: **Chart → everything** (a new
`SlintAction::ChartScrub(x_norm)`/`ChartBrush(x0,x1)` next to the Timeline actions, mapped to
`cursor_t` via the chart x-range); **3D viewport → everything** (a system reads editor
selection into `DataSelection.entities`); **Grid → everything** (`select-row(r)` → timestamp →
`cursor_t`). Projection systems (one per surface, visibility-gated, hash-gated on `rev`,
modeled 1:1 on [`timeline_slint_sync.rs:61`](eustress/crates/engine/src/timeline_slint_sync.rs)):

```rust
fn project_selection_to_3d(sel: Res<DataSelection>, mut q: Query<(&Instance, &mut InstanceColor)>) {
    if !sel.is_changed() { return; }
    for (inst, mut col) in &mut q {
        if let Some(v) = value_at(inst.uuid, sel.cursor_t) { col.0 = ramp(v); } // NO new pipeline
    }
}
```

Because `project_selection_to_3d` writes `InstanceColor`, **scrubbing the chart timeline
animates the 3D model through its recorded history** — the closed-loop twin's "compare" step
rendered directly on geometry. `cursor_t` is **sim-time/monotonic embedded in the payload**,
not wall-clock (ring replay zeroes wall-clock); the grid, chart x-axis, and 3D recolor must all
agree on this one clock. Axis units come from the column's `Dimension`.

### V.5 Slint wiring (the bottom-slot host)

Add a third bottom-slot tab copying Output/Timeline exactly: `chart_panel.slint` (imported into
`main.slint` or `build.rs` won't compile it,
[`build.rs:11`](eustress/crates/engine/build.rs)), a `Chart` arm in `enum BottomPanelMode`
([`timeline_panel.rs:197`](eustress/crates/engine/src/timeline_panel.rs)) +
[`slint_ui.rs:8182`](eustress/crates/engine/src/ui/slint_ui.rs) +
[`timeline_slint_sync.rs:48`](eustress/crates/engine/src/timeline_slint_sync.rs). The Slint
panel is just an `Image` element showing the sub-rect camera region (Approach B) with vector
chrome (axes/legend/scrubber, hand-drawn like
[`timeline_panel.slint:379-427`](eustress/crates/engine/ui/slint/timeline_panel.slint)).
`ChartPanelPlugin` adds `spawn_chart_camera` (D-V1..D-V4), the chart pipeline (RenderApp sub-app
+ `ExtractSchedule` + `RenderSet::PrepareResources`, verbatim shape of
[`instanced_pbr.rs:61-71`](eustress/crates/engine/src/rendering/instanced_pbr.rs)), the LOD
selector, and the projection systems.

### V.6 Editable data grid / spreadsheet + forms

**No table/grid widget exists in the UI today** — the Slint upstream `StandardTableView` is
read-only (single-row selection, non-editable cells) and is not vendored here, so the grid is a
**net-new custom component** built on the Properties panel's proven discipline:

```slint
// engine/ui/slint/data_grid.slint
export struct GridCellData {
    row: int; col: int; text: string; editable: bool;
    kind: string;    // "f64"|"int"|"string"|"bool"|"date"|"enum"
    valid: bool; error: string;     // false → red border + tooltip
}
export component DataGrid inherits Rectangle {
    in property <[GridCellData]> cells;        // flat, hash-gated (slint_ui.rs:15255)
    in property <int> highlight-row;           // ← DataSelection projection (V.4)
    callback edit-cell(int, int, string);
    callback select-row(int);                  // → DataSelection.cursor_t
    callback add-row(int); delete-row(int); add-column(string); delete-column(int);
    callback input-focus-changed(bool);        // ← focus-pause seam (properties.slint:2090)
}
```

Three mandatory disciplines (the Properties panel already solved these — copy them): (1)
**focus-pause** ([`properties.slint:2090`](eustress/crates/engine/ui/slint/properties.slint)) or
every live sync clobbers the cell being typed; (2) **hash-gated whole-model push**
([`slint_ui.rs:15255-15272`](eustress/crates/engine/src/ui/slint_ui.rs)) or re-`set_*` resets
the focused editor; (3) **structural mutations through a staging exclusive system** (the
`PendingFileActions` pattern,
[`file_event_handler.rs:44-49`](eustress/crates/engine/src/ui/file_event_handler.rs)). The
table-of-record is a `Series`/`Column` — the grid writes through `put_ts_row`/materialize, not a
parallel store. **Validation** runs in `eustress-data` on `edit-cell` (parse against `dtype` +
`Dimension`; invalid → red border, edit rejected before it reaches the frame). **Forms** are a
single-row grid reusing the Properties per-type editor rows (`Vec3Row`/`ColorRow`/`BoolRow`/…,
[`properties.slint:2316-2461`](eustress/crates/engine/ui/slint/properties.slint)). **D-V5 —
the nested-layout grid is not column-virtualized**; for wide sheets, render only the visible
viewport window from Rust (the V.3b lever applied to cells).

---

## Subsystem D — Ingestion & ETL (one bus, one recorder, many adapters)

> **Ingestion is not a new pipeline. It is one `Recorder` that subscribes to a `sensor.<name>`
> topic and commits batched, dimensioned rows; every existing source becomes a producer onto
> that bus.** (D5.) **And it is not a new connector taxonomy: the *what-can-I-connect-to* and
> *how-is-it-governed* questions are owned by the Eustress Parameters fabric — D *implements
> transports under that schema*, it does not re-enumerate sources.** (D.0.)

Eustress already ingests through **four disjoint transports** that don't know about each other
(sim scalars → `sim-commands.jsonl`; telemetry → `telemetry.jsonl`; watchpoint history → RAM
`VecDeque`; Monte-Carlo runs → rkyv stream). The platform collapses them: every adapter's job
is reduced to producing well-formed records onto a `sensor.<name>` topic (or, for bulk file
import, writing the `datasets` partition directly). New sources are ~50 lines of adapter, not a
new subsystem.

There is a **second, equally important non-duplication**: the connector *catalog* and its
*governance contract* are not D's to invent — the Eustress Parameters subsystem
([`parameters.rs`](eustress/crates/common/src/parameters.rs)) already declares the canonical
taxonomy of what Studio reads from / writes to (`DataSourceType`, `ExportTargetType`), the
per-source config shape (`DataSourceConfig`, `AuthConfig`), the field-level transform contract
(`DataMapping` / `FieldMapping`), and the privacy posture (`AnonymizationMode`). The honest
status: today those are **taxonomy + UI-picker + config structs with ZERO transport behind
them.** No `DataSourceType::`/`ExportTargetType::` dispatch arm exists anywhere in the tree —
the ~55-variant source enum is read only by `display_name()`/`category()`/`all_variants()` UI
helpers ([`parameters.rs:649-666`](eustress/crates/common/src/parameters.rs)),
`ExportTargetType` has zero variant-matching call sites, the `ParameterRouter` export methods
(`publish_export`/`buffer_export`/`drain_pending`,
[`parameters.rs:355,377,385`](eustress/crates/common/src/parameters.rs)) have no callers, and
the `parameter.changed`/`parameter.exports` stream bridges
([`parameters.rs:512,536`](eustress/crates/common/src/parameters.rs)) never fire because nothing
emits `ParameterChangedEvent`/`ExportRequestEvent`. So **D is the subsystem that gives the
Parameters taxonomy a body**: it builds the transports `DataSourceType`/`ExportTargetType` only
name, drives them from `DataSourceConfig`/`AuthConfig`, and lights up the dead router. D does
**not** consume connectors that already work — none do — it *implements* them under the schema.

Ingestion invariants extend D3/D5 and add **D.0 — defer to Parameters for the connector
catalog & governance**: dimensioned at the boundary (D3); one landing seam (D5); the
source/target taxonomy + auth + mapping + anonymization contract is the Parameters schema, not
a parallel list (D.0); and **backpressure is bounded loss with a counter, never a stall** — the
Recorder subscribes via a drop-on-full `flume` channel and batches on its own thread, exposing a
monotonic `dropped` counter per topic (a dataset that lost samples says so).

### D.0 The Parameters fabric is the connector & governance *schema* (don't fork it; build its body)
The Eustress Parameters subsystem is the **single authority** for *what Studio connects to* and
*how those connections are governed* — as a **schema**, not as running transport. The Data
Platform does not maintain a second connector registry, a second export-target list, a second
auth model, or a second privacy mode — it **implements the transports that the Parameters
schema declares**, by the same logic that keeps D5 from inventing a fifth on-disk format.
Concretely, these Parameters types are **canonical and load-bearing** for all of D, but are
**inert today** and gain behavior only when D supplies it:

- **`DataSourceType`** ([`parameters.rs:584-647`](eustress/crates/common/src/parameters.rs)) —
  the canonical inbound catalog (Postgres, MQTT, OPC-UA, Kafka, FHIR/HL7/DICOM, Parquet, REST,
  CSV, S3/Azure/GCS, …). Today read only for `category()`/`display_name()` UI helpers; **no
  variant is ever matched to open a connection.** D.4 connectors **register against these
  variants** (they are not a separate connector enum); D adds the missing
  `match source_type { … => open transport … }` arm.
- **`ExportTargetType`** ([`parameters.rs:138-154`](eustress/crates/common/src/parameters.rs)) —
  the canonical outbound catalog (Postgres / Firebase / JsonFile / CsvFile / McpServer /
  Webhook / CloudStorage). Today has **zero dispatch call sites**. D will route dataset exports
  through this enum + `ExportRecord` via `ParameterRouter::publish_export`
  ([`parameters.rs:355`](eustress/crates/common/src/parameters.rs)) onto the
  `parameter.exports` topic — finally giving that caller-less path real producers.
- **`DataSourceConfig` / `AuthConfig`** — the canonical per-source config + credential shape.
  The UI already constructs one on "Add Parameters"
  ([`world_view.rs:1293`](eustress/crates/engine/src/ui/world_view.rs)) and surfaces it in the
  inspector ([`world_view.rs:630`](eustress/crates/engine/src/ui/world_view.rs)), but it never
  drives a connection; D consumes the **same** struct to actually drive one, rather than
  defining its own connection params.
- **`DataMapping` / `FieldMapping`** — the canonical schema-on-import transform contract,
  defined-and-serialized only with no consumer. D's `infer_schema` proposal (D.4) **emits a
  `DataMapping`**; the import/transform engine D builds is the first runtime *consumer* of these
  previously serialize-only types.
- **`AnonymizationMode`** — the canonical privacy posture. D applies it at the ingest/export
  boundary (alongside D3 dimensioning and any consent gate) so anonymization is enforced **once,
  in the schema's terms**, not re-implemented per connector.

**Real impl state (from grounding — do not contradict).** Today the only working external
transport in the tree is **not** keyed off these types and lives in two decoupled places, neither
driven by `DataSourceType`/`DataSourceConfig`/`ExportTargetType`: (i) the MCP crate's export
targets — real `reqwest` POST + `tokio::fs::write` + embedvec JSONL
([`router.rs:204,285`](eustress/crates/mcp/src/router.rs)) — which are **live** but operate on
the MCP crate's own `EepExportRecord` over the `mcp.exports` topic; and (ii) the scenarios
adapters' real but **dormant** CSV/REST/file code
([`adapters.rs:329-360`](eustress/crates/engine/src/scenarios/adapters.rs)) whose
`ScenariosPlugin` is **never mounted** into the app and whose only constructors live under
`#[cfg(test)]`. **D's job is to converge them under the Parameters schema:** new transports
register against `DataSourceType`; the live MCP export targets become the **first concrete
`ExportTargetType` backends** (e.g. `McpServer`/`Webhook`/`JsonFile` dispatch through
`ParameterRouter` → the existing MCP subscribers), so the two pipelines stop being parallel.
Anything D builds that needs a new source or target **extends the Parameters enums first**, then
implements the transport — the enum is the contract, D is the body.

### D.1 Sim taps & probes
A **tap** binds a source value to `sensor.<name>` at a declared rate. Sources, priority order:
a `WatchPoint` (primary — carries name+unit+history); a `SimValuesResource` key; an ECS field
by reflection `entity_uuid.Component.field`; a realism newtype bridged via `From`. The producer
side is `SensorTapPlugin`, one system publishing fixed-POD `SensorReading`s via the zero-copy
`send_pod` (<1 µs, alloc-free), each probe its own 65,536-slot ring. `SensorReading` embeds its
own `sim_tick`/`sim_time_us` because ring replay zeroes the wall timestamp
([`topic.rs:160-176`](eustress/crates/stream/src/topic.rs)). Full-rate (`hz=0`) is now allowed
because the Recorder, not the UI, is the durable consumer.

```rust
// eustress/crates/data/src/recorder.rs — the single durable writer (D5)
pub struct Recorder {
    rx: flume::Receiver<OwnedMessage>,   // subscribe_channel: drop-on-full
    series: SeriesRef, batch: arrow::ArrayBuilder,
    dropped: AtomicU64, flush_rows: usize,   // batch (e.g. 4096) → one put_ts_row commit
}
```
Inline `subscribe` is **forbidden** for the Recorder (a slow flush would stall a kHz probe).
**We do NOT enable the stream's own per-message segment storage** — the engine stays
`in_memory()` and durability lives in WorldDb's `timeseries`, where range-reads actually work.
MCP: `create_tap`/`list_taps`/`remove_tap` as op-arms + bridge methods (D5); fill
`query_stream_events` as the real query front door; `tail_telemetry` keeps reading the JSONL
(now a secondary projection that finally gets rotation).

### D.2 Hardware ingest — serial / USB / BLE / OSC / MQTT
**Hardware adapters are not special** — each is a thread producing `SensorReading`s onto a
`sensor.<name>` topic, identical to a sim tap; the Recorder doesn't know the difference. They
live in feature-gated sub-modules of `eustress-data` (`hw-serial`/`hw-bluetooth`/`hw-osc`/
`hw-mqtt`, D2), each declaring channels + units + calibration in a device-template TOML
(`assets/hardware/<device>.toml`) so new probes need **no Rust** — same data-driven discipline
as `class_schema`. **Calibration is ingest-time (D3):** raw ADC → physical happens in the
adapter before the topic, so every downstream consumer sees dimensioned physical units.
**v1 scope: ONE generic line-protocol serial adapter** (covers the LabQuest-style USB-CDC
floor) + the device-template registry; BLE/MQTT/OSC are separate opt-in features with their own
risk budget (device quirks are unbounded and out of the core estimate).

### D.3 Manual data entry
The editable `DataGrid` (V.6) is a third producer landing rows into a `Series`; validation
reuses the rich `{ type, value, description }` `class_schema` form
([`instance_loader.rs:1937-1958`](eustress/crates/engine/src/space/instance_loader.rs)). On
`edit-cell`: parse to `dtype` (reuse `property_string_to_value`,
[`slint_ui.rs:15284`](eustress/crates/engine/src/ui/slint_ui.rs)) → dimension-check (`"5 mm"` →
`Quantity::parse`, assert `dim == column.dimension`, convert to canonical SI) → bounds/regex/
required → commit or reject-and-redden. Forms = single-record validation over the Properties
editor rows. Agent-drivable via `grid_set_cell`/`dataset_append_row` through the **same**
validation (D5), routed through `eustress_common::instance_create::create_instance`
([`instance_create.rs`](eustress/crates/common/src/instance_create.rs)) for new Datasets.

### D.4 Import connectors — implementing the Parameters `DataSourceType` registry, schema-on-import
Connectors are **the runtime implementation of the `DataSourceType` registry**
([`parameters.rs:584-647`](eustress/crates/common/src/parameters.rs)) — they are *not* a second
list of supported sources (D.0), and they do *not* exist yet: the registry is taxonomy-only with
no dispatch arm in the tree, so each connector D ships is **new transport** bound to an existing
`DataSourceType` variant and driven by the entity's `DataSourceConfig`/`AuthConfig`. Adding a
genuinely new source means **adding the variant to `DataSourceType` first**, then writing the
adapter. Bulk import is the one path that **bypasses the stream** (no per-reading topic makes
sense for a 10M-row Parquet) and writes `datasets` directly via `eustress-data`, still producing
the identical `Dataset`/`Series`/`Column` tree. Two-phase, mirroring the `roblox-import`
propose/materialize split
([`import_report.rs`](eustress/crates/roblox-import/src/import_report.rs),
[`materializer.rs`](eustress/crates/roblox-import/src/materializer.rs)): `infer_schema` proposes
column dtypes+units (parsing header unit suffixes like `temp_C`, `force (N)`) **as a
`DataMapping`/`FieldMapping`** (D.0) the user edits in the grid (V.6), then `import` consumes
that mapping, applies the configured `AnonymizationMode` plus any consent gate at the boundary
(D.0), and commits the resolved schema as `Column` `class_schema` rows (D3). First connector
backings for the existing `DataSourceType` variants: CSV (`parquet` feature, chunked), Parquet
(native, best fidelity, + export counterpart = the ML-export path), JSON/JSONL (`serde_json`,
imports the legacy `.eustress/*.jsonl` family so historical logs become queryable Datasets),
REST (`reqwest`/`http_request` + paging + JSONPath — the live-fetch counterpart to the
currently-dormant `RestApiAdapter` at
[`adapters.rs:329-360`](eustress/crates/engine/src/scenarios/adapters.rs), now keyed off
`DataSourceType::REST` + `AuthConfig`). **SQL (D-sql, open):** smallest-viable is
export-to-CSV/Parquet or shell to `sqlite3`/`psql` via `run_bash`; a native `sqlx`/`rusqlite`
driver is a heavy opt-in `import-sql` feature only on a demonstrated live-DB need — and when
built it backs the `DataSourceType` SQL variants (Postgres/MySQL/…), not a private enum.
**Exports** route through `ExportTargetType` + `ExportRecord` via
`ParameterRouter::publish_export`
([`parameters.rs:355`](eustress/crates/common/src/parameters.rs)) onto the `parameter.exports`
topic (today caller-less); the existing **live** MCP webhook/file/console/embedvec targets
([`router.rs`](eustress/crates/mcp/src/router.rs)) become the first concrete `ExportTargetType`
backends so there is one export taxonomy, not two (D.0).
`import_dataset`/`infer_dataset_schema`/`export_dataset` as MCP tools, bridge-routed with disk
fallback (D5), all speaking the Parameters types end-to-end.

---

## Subsystem C — Analysis & compute (Transform · Fit · Solve · Embed)

> **A dataset is not a thing you analyze; it is the substrate the engine already computes on.
> Every analysis is columns → columns (Transform), columns → parameters (Fit), mesh →
> columns (Solve), or columns → vectors (Embed) — all unit-correct, reproducible, and
> re-runnable by a script.**

Compute **never invents a second math stack** — it routes through four existing primitives: the
`Quantity`/`Dimension` system, the Rune/Luau VMs, the CAD solver (Phase C) + FEA (Phase D), and
the `eustress-embedvec` ANN index. A single `Analysis` trait unifies all four:

```rust
// eustress/crates/data/src/compute.rs — polars-free; lives in the data leaf (D2)
pub trait Analysis {
    fn kind(&self) -> &'static str;                       // "fit.linear", "fft.real", ...
    fn check_dims(&self, input: &Frame) -> Result<(), DimError>;  // reject mismatch UP FRONT
    fn run(&self, input: &Frame, params: &AnalysisParams) -> Result<AnalysisResult, AnalysisError>;
}
pub struct AnalysisResult { pub frame: Frame, pub scalars: Vec<NamedQuantity>, pub provenance: Provenance }
```

### C.1 Calculated columns via Rune/Luau
A new `data` Rune/Luau module (`rune_data_module.rs`, registered next to
`register_engine_rune_modules`, [`rune_api.rs:76`](eustress/crates/engine/src/soul/rune_api.rs),
behind `#[cfg(feature = "data")]`) exposes `col`/`put_col`/`map_col`. Two modes: per-row VM eval
(`kelvin = celsius + 273.15K`, dimension-checked by the same algebra the CAD eval guards use)
and **vectorized `map_col`** (arithmetic-only, compiled once, applied whole-column via the
polars eager engine — the hot path for million-row columns, avoiding 10⁶ VM round-trips; output
`Dimension` computed from operands via `Dimension::mul`/`div`/`powi`). **Reproducibility:** a
calculated column stores its *source expression string* + source-column ids as canonical (the
CAD feature-tree discipline,
[`feature_tree.rs:32`](eustress/crates/cad/src/feature_tree.rs)); the computed values are a
cache, re-derivable byte-for-byte.

### C.2 Statistics, derivative, integral, interpolation
Basic reductions already exist twice — **wrap `TimeSeries::compute_stats`**
([`recorder.rs:116`](eustress/crates/common/src/simulation/recorder.rs)), do not
re-implement. New numerics in `eustress-data/src/numerics.rs` (behind `frames`), all
dimension-propagating: `derivative` (dim = y.dim / x.dim → velocity from displacement/time),
`integral` (trapezoid/Simpson, dim = y.dim · x.dim), `interpolate` (linear/cubic/Akima).
In-house, not a heavy stats dep — these are a few hundred lines and the codebase resists
gratuitous deps.

### C.3 Curve-fitting routed through the CAD solver (Phase C)
Linear/poly least-squares are closed-form in `numerics.rs`. **Nonlinear/constrained fits route
through the CAD solver** — a curve fit *is* a least-squares residual minimization, the same
Newton/Levenberg-Marquardt machinery the geometric constraint solver uses. `eustress-data`
declares a `LeastSquares` trait; `eustress-cad` (Phase C) implements it; the engine injects it
behind `#[cfg(all(feature="data", feature="cad"))]` — keeping truck out of the data leaf. Fit
params become a tiny `Dataset` of `{param, value, unit, std_err}` (same flat shape as
experiment files, so `compare_runs` can diff two fits); the fitted curve is a calculated column.

### C.4 FFT / spectral — and FEA-result-IS-a-dataset
FFT (rustfft, behind `frames`, NOT a polars dep) is a transform: time-domain `Column` →
frequency `Column` (dim = `TIME.powi(-1)` = Hz) + magnitude; `check_dims` verifies uniform
sampling or routes through `interpolate` first. **FEA results ARE datasets over a mesh** (Phase
D): one `Series` per field (`stress_von_mises`, `displacement_{x,y,z}`, `temperature`), each a
`Column` indexed by mesh node/element id (key `dataset_id \x1f field_id \x1f sort_to_be8(node_id)`,
A.3), each with a fixed `Dimension`. The payoff: **FEA fields chart, get statistics, get
calculated columns, and feed the twin loop with zero new machinery.** "Safety factor field" is
a calculated column `sf = yield_strength / stress` (dimensionless, correct); a stress heatmap is
a V.2e chart sampling the field texture. **FEA gets the data platform's viewer, not its own.**
`index_kind` metadata (`time`/`mesh_node`/`mesh_element`/`row`) disambiguates.

### C.5 ML & embedding via `eustress-embedvec`
Clustering/similarity/anomaly route through the production HNSW index
([`resource.rs:278`](eustress/crates/embedvec/src/resource.rs) `search`,
[`memory.rs:409`](eustress/crates/embedvec/src/memory.rs) `cosine_similarity`,
[`spatial.rs`](eustress/crates/embedvec/src/spatial.rs) `avg_neighbor_distance`). A
`eustress-data/src/embed.rs` bridge treats each row of selected numeric columns as a
z-score-normalized feature vector: `cluster` (k-means floor, HDBSCAN over the HNSW graph later)
appends a `cluster_id` column; `similarity` powers "similar runs"; `anomaly_score` =
mean-kNN-distance powers the twin's "this run diverges from baseline" alarm. **ML is the
documented exception to D3:** z-score strips dimension by construction, so embeddings/cluster
ids/scores are all `DIMENSIONLESS` and `check_dims` only verifies inputs are numeric+finite. The
7-wheel Color Wheel ML loop is the proof use case.

### C.6 Notebook & report surface
The notebook is **not a new document type** — it is **Workshop markdown**
([`engine/src/workshop/`](eustress/crates/engine/src/workshop/)) where fenced `rune`/`luau`/
`analysis` blocks are executable cells. Cell outputs (table → grid panel, chart → `ChartSpec`
rendered into the chart camera, scalar → log) persist beside the markdown, but the *recipe* is
canonical so "re-run all cells" reproduces every output. A **report** is the same document
output-only; a **dashboard** is a report whose charts subscribe live to a `sensor.<name>` stream
or a twin dataset. This gives the closed loop a home: collect → analyze → simulate → compare,
all in one document.

### C.7 Provenance (cross-cutting invariant)
Every `AnalysisResult` carries a `Provenance { kind, input_refs+content-hash, params,
solver_id, created_at_ms, digest }`, the `digest` a blake3 over (input hashes, params) reusing
the branch-digest pattern ([`branch.rs:211`](eustress/crates/worlddb/src/branch.rs)). Two
analyses with the same digest are guaranteed identical outputs — this caches solves, tells
`compare_runs` two runs are comparable, and lets a notebook skip unchanged cells. **A number
typed by hand has no provenance and cannot be reproduced or twinned** — which is why analysis
routes through stored recipes, never ad-hoc UI math.

---

## Subsystem F — AI copilot, closed-loop digital-twin, and BI (the differentiators)

> **The copilot is not a chat box bolted onto charts. It is a closed-loop operator that already
> has `run_experiment`, `compare_runs`, and CoW branches — the only missing pieces are a query
> front door, a fit/instantiate step that turns measured data back into sim parameters, and
> charts/dashboards as first-class Instances it can read and write.**

### F.1 The five copilot asks → real tools

| Ask | Decomposes to | New glue |
|---|---|---|
| **Import-by-prompt** | `read_file` → `eustress-data::read_csv` → `create_entity` → write `timeseries` | `import_dataset` |
| **Fit-suggestion** | read Dataset → `fit_model` (linear/poly/exp/power/log + FFT) → rank by R²/AIC | `fit_model` → typed `Quantity` coeffs |
| **Anomaly-find** | `find_similar_entities` over per-run embedding; outlier = low-similarity-to-centroid | `find_anomalies` thin tool |
| **Dashboard-from-a-sentence** | resolve series → `create_entity Dashboard` + child `ChartPanel`/`StatCard`/`Grid` | `build_dashboard` |
| **Explain-column** | read Column attrs + stats + embedvec semantic match | `describe_column` |

The copilot never invents tool names — it emits a plan of `{tool, args}` against the registered
`ToolHandler` set, executes via the existing agent loop, and streams progress onto `workshop.*`
topics (D5). Failures route through the Rune-error stream tee so a bad fit surfaces in
Output/Problems, not a silent dead end.

### F.2 The closed loop
A new `closed_loop` orchestrator composes existing verbs (it does NOT replace `run_experiment`):
```
collect:      query_stream_events(sensor.*, as_dataset="measured")          → Dataset
fit:          fit_model(dataset, x, y)                                       → Model (typed Quantity coeffs)
instantiate:  instantiate_model(model, "ReactorCore.cooling_coeff")         → writes Quantity → realism component (dim-checked)
branch:       worlddb.branch()                                              → exp branch (D1, O(1))
run forward:  run_experiment(name, sim_values=fit_params)                    → experiment JSON  (UNCHANGED)
overlay:      ChartPanel: measured + simulated + residual = measured−simulated
compare:      compare_runs(measured, simulated)                             → per-key delta  (UNCHANGED)
iterate:      residual > tol → perturb → new branch → re-run; else commit winner
```
**`instantiate_model` is the genuinely new step** — the inverse of `tail_telemetry`. It writes
a fitted `Model`'s typed coefficients back into the sim, **dimension-checked**: the
`Quantity.dim` must match the target field's declared dimension (`Pascals` → `PRESSURE`) or it
**fails loudly** (F3-equivalent enforcement at the instantiate point, since fit-coefficient
dimensions are ambiguous). Write path obeys D5 (out-of-process appends a `set_sim_value`-style
op; live uses the bridge). The overlay chart (two GPU line series + a residual band) is pure
Subsystem V work and **proves the twin**: drag the chart cursor across a stress-test run and
watch the parts heat up red in the viewport (V.4).

### F.3 BI dashboards as Instances
A dashboard is a tree of data-class instances under `DataService` (D6): `Dashboard` → child
`ChartPanel`/`StatCard`/`Grid`, with a layout-grid attribute. It **persists in WorldDb, branches
with the world (D1), round-trips through save/load, and is editable over MCP** (`update_entity`
to retarget a chart's series). The copilot builds one by emitting `create_entity`/`update_entity`
(reviewable in the diff before it lands), never by mutating hidden UI state. A `Dashboard` cell
may host a **3D-viewport sub-rect** (a chart camera with its `viewport` synced from
`ViewportBounds`) so an IoT/geospatial twin shows probes as billboards over scene geometry,
colored by live `sensor.<name>` value — the bridge to native 3D a 2-D BI tool structurally
cannot do.

---

## Differentiators

What no off-the-shelf data tool (Logger Pro X, Tableau, Jupyter) can give, because they require
the engine, the sim, and the agent to be the same process:

1. **Charts are cameras, not widgets** — the same GPU machinery that streams 10M entities draws
   a million-point chart; "millions of points" is the existing scaling story re-aimed.
2. **Data is visible on the model** — `InstanceColor` recolors the live 3D scene by a metric;
   scrubbing a chart animates the model through its recorded history (V.4).
3. **The digital twin is closed in one process** — collect → fit → `instantiate_model` →
   `run_experiment` → compare → branch, with fit results as typed `Quantity` written back into
   realism components (F.2).
4. **Experiments are branches** — run N candidates off one baseline, compare digests, commit the
   winner; the baseline is never clobbered (D1).
5. **One substrate, linked views** — grid, chart, notebook, dashboard, 3D overlay are all
   projections of one `Dataset` + one `DataSelection`; no parallel models.
6. **The copilot is an operator, not an autocomplete** — it has the tool surface that already
   runs experiments, plus a query front door and a fit/instantiate step.

---

## 4. Risks & Mitigations (honest — including the verdicts that did NOT hold)

1. **Polars build blast radius (verdict: holds, CONDITIONALLY).** True only under leaf
   isolation. *Mitigation:* the safe claim is scoped to the **`data` tier only** (arrow-rs
   `arrow`+`parquet`, `default-features=false`, NO polars); **`data-frames`/`data-query` have
   UNMEASURED compile cost on this hardware** (deps build at `opt-level=3`,
   [`Cargo.toml:126-127`](eustress/Cargo.toml)) and must be wall-clock benchmarked before any
   "won't wreck the build" claim — until then, presumed-expensive, CI-only. Leaf isolation is
   an **enforced invariant** (D2) with a `cargo tree -e features` CI guard. The thrash analysis
   is corrected: the danger is NOT duplicate `ahash`/`hashbrown` versions (the lock already
   tolerates 4 `hashbrown` + 2 `ahash` as separate nodes — proof duplicates don't thrash Bevy);
   it is **feature-unification on the single shared `chrono 0.4.44`/`serde`** — pin to Bevy's
   resolution, verify with `cargo tree -d`. Gravest failure: `data` in `core`, or polars in
   `common`/`cad` — both re-price every clean build of the static-linked `.exe` (dynamic linking
   blocked on Windows).
2. **GPU charts compositing (verdict: holds, but the canonical path was WRONG).** *Mitigation:*
   Approach B (on-screen sub-rect camera) is canonical, not Approach A. The live overlay flows
   Slint → CPU → Bevy `Image` ([`slint_ui.rs:2556-2645`](eustress/crates/engine/src/ui/slint_ui.rs)),
   the OPPOSITE of feeding a Bevy texture to Slint. Approach A needs a per-frame GPU→CPU
   readback or the DISABLED Skia adapter (Windows ICU block,
   [`main.rs:143`](eustress/crates/engine/src/main.rs)) — a real prerequisite, not a copy job.
   Exit gate: prove the sub-rect renders under the chrome (V.0). Keep D-V1..D-V4.
3. **Timeseries partition CoW "for free" (verdict: DOES NOT HOLD — false).** *Mitigation, stated
   plainly:* CoW participation is explicit per-namespace work, not automatic. Until all four
   steps (overlay field + `len`/`digest` tag 13+ + read/write/iter overrides + commit-replay)
   are done, the partition's branch behavior is **BROKEN, not absent**: reads return empty (not
   parent data), writes silently no-op/err via the default trait impls
   ([`backend.rs:368-416`](eustress/crates/worlddb/src/backend.rs)) — which can corrupt an
   experiment branch's view. Implement the overlay for `datasets`; **defer it for
   `timeseries`** and branch at the materialized-blob level (D1). `commit()` is NOT
   cross-partition atomic ([`branch.rs:38-42`](eustress/crates/worlddb/src/branch.rs)) — replay
   tree last + add a `Run.status=failed` reconcile sweep on open. The accurate one-liner:
   *adding the partition is mechanical; making it branch-aware is mechanical-but-real per-method
   work; getting `timeseries` to branch at all requires the blob-level workaround — none of it
   is free.*
4. **Dimension migration "clean / won't break" (verdict: holds, with corrections).**
   *Mitigation:* the additive-string-contract thesis is sound (no serialized `Quantity` hits
   disk). But: (a) the `DisplayUnit` `#[serde(default)]` "risk" is a **non-risk today** —
   `DisplayUnit` is `init_resource`-only, never serialized
   ([`main.rs:345`](eustress/crates/engine/src/main.rs)); frame `#[serde(default)]` as a
   forward-guard only. (b) **Downgrade is NOT safe** — a new symbol (`pa`/`v`/`si:…`) read by an
   OLDER binary hits `from_symbol → None → warns-then-defaults to Meter`
   ([`instance_loader.rs:1813-1819`](eustress/crates/engine/src/space/instance_loader.rs)): the
   load logs a warning but still **semantically retypes** the value (a pressure becomes a length) —
   a corruption that is logged, not loud. Bump a space/schema version so an old
   engine loudly warns, and change the unknown-symbol branch to default-to-DIMENSIONLESS /
   preserve-raw-symbol, never silently-retype-as-length. (c) The eval-guard rewrite must preserve
   Scalar-as-length and bare-number-as-degrees behind a regression test. The honest claim:
   **forward-compatible (old files in the new engine); NOT downgrade-safe without a version gate.**
5. **Full-scope phasing "without ballooning" (verdict: holds, but soften the qualifier).** The
   floor is cheap and grounded (parquet-only + the existing run model); the FULL scope is a large
   multi-crate platform: **1 new crate + ~9 `ClassName` variants across 3 exhaustive matches + 2
   new partitions + per-partition CoW work + a net-new editable grid + a net-new GPU pipeline + N
   hardware drivers.** *Mitigation:* drop "without ballooning"; phase from the floor; the science
   leg (nonlinear fit C.3, FEA-as-dataset C.4) is **BLOCKED on CAD_PLATFORM_PLAN Phase C/D** and
   does NOT ship on the data-platform timeline — the `LeastSquares` trait is the seam so it lights
   up when Phase C lands. Hardware v1 = one serial adapter; BLE/MQTT/OSC deferred with their own
   budget.
6. **Forking the run model into a third store.** *Mitigation:* D4 — `Run` is an index over the
   existing experiment JSON + `SimRecord`; keep `simulation_tools.rs:1600` byte-stable.
7. **`Part`-fallback visibility trap.** *Mitigation:* D6 — ship `ClassName` variants AND templates
   together, or data nodes render as gray blocks
   ([`instance_loader.rs:1800-1802`](eustress/crates/engine/src/space/instance_loader.rs)).
8. **Silent sample loss looks like real data.** *Mitigation:* per-topic `dropped` counter +
   per-tap `seq` gap detector recorded as a quality column; the dataset header surfaces "N
   dropped."
9. **Decimation hides spikes.** *Mitigation:* min-max (shape-preserving), never averaging (V.3a);
   FFT/curve-fit always operate on the full-resolution frame, never the proxy.
10. **Grid whole-model re-push interrupts typing.** *Mitigation:* hash-gate + focus-pause + staged
    structural mutations are mandatory (V.6) — the exact failure the Properties panel solved.

---

## 5. Phased roadmap

Each phase ends on a measured gate. Subsystem A (the `eustress-data` crate + run/dataset model +
partitions) lands first; the science leg is dependency-gated on CAD Phase C/D (P5).

| Phase | Theme | Key work | Exit gate |
|---|---|---|---|
| **P0** ✅ | Columnar floor (D2) | `eustress-data` leaf, arrow-rs parquet only; CI `data-graph-default` guard; migrate `color_manifest.parquet` off serde_json | `eustress-data` present in the default graph; `cargo test -p eustress-data` green; parquet round-trips |
| **P1** | Substrate + dimension (D3,D6) | `DataService`/`Dataset`/`Series`/`Column`/`Run` classes + 3-match `ClassName`; `datasets`/`timeseries` partitions (`store_with_opts`); `common/src/dimension.rs` (shipped) + `Dimension::from_unit_symbol` bridge — cad keeps its own `Quantity`, NO façade/eval rewrite | A `Column` renders non-visual (no gray block); a `Series` materializes to a `datasets` blob; existing `features.toml`/spaces still load |
| **P2** | Query front door + durable collect (D5) | fill `query_stream_events`; `sensor.<name>` Recorder seam (batched, `dropped` counter); one serial hardware adapter + device-template TOML; editable `DataGrid` + validation + forms | A 1 kHz probe records ≥10 min into `timeseries`; `query_stream_events` reads past the 65 s ring window, identical in-engine and out-of-process |
| **P3** | GPU charts + linked views | `charts/` + `ChartPanelPlugin` (Approach B, D-V1..D-V4); scatter/line/heatmap pipelines; LOD pyramid (min-max); `DataSelection` + 3 projection systems | Sub-rect chart renders under the chrome; a 10⁶-point series renders downsampled at >30 FPS (target, unmeasured on this hardware); scrubbing the chart recolors the 3D model |
| **P4** | Analysis + notebook | `numerics` (wrap `TimeSeries`) + `spectral` (rustfft) + linear/poly fit; `data` Rune module (`map_col`); `analysis` Workshop cells; `embed` (k-means + anomaly) | A calculated column over 1M rows < 50 ms via `map_col`; FFT + descriptive stats over a recorded Series; cluster/anomaly columns chart |
| **P5** | Closed loop + BI (gated on CAD P-C/D for nonlinear fit + FEA) | `fit_model`/`instantiate_model` (typed `Quantity`, dim-checked) via injected `LeastSquares`; `closed_loop` orchestrator; overlay-vs-simulated chart; `Dashboard`/`StatCard`/`Grid` + `build_dashboard`; FEA-result datasets | One-sentence "fit + run twin + show residual" produces an overlay + a committable winning branch; "dashboard-from-a-sentence" builds a saved, branch-surviving `Dashboard`; an FEA stress field charts; branching a Dataset with a materialized Series isolates the branch view from a parent mutation (D1 blob-level workaround proven) |
| **P6** | ETL connectors + geospatial twin | CSV/Parquet/JSON/REST import with schema-on-import; legacy JSONL import; 3D-viewport dashboard cell; probe billboards colored by live `sensor.*`; heatmap data-texture quad | Import a 10M-row Parquet via schema-on-import; live IoT twin: probes over scene geometry update color from streaming sensor values |

**Status (2026-06-24):** P0 ✅ (leaf shipped + tested) · P1 ✅ (classes + partitions +
`dimension.rs`) · P2 partial (Recorder seam + producer A shipped; `query_stream_events` fill +
hardware adapter pending) · P3 partial (Chart is a Slint scaffold; GPU pipelines + LOD +
`DataSelection` pending) · P4 partial (`numerics`/`spectral`/`ml` shipped + wired to ribbon
Stats/Fit/FFT/Cluster/Anomaly; the Rune per-cell column engine + notebook pending) · P5/P6 not
started (P5 gated on CAD C/D). `data` is on by default.

---

## 6. Open Decisions

**Resolved (2026-06-23/24, user) — the action-verb spec:** `data` is **on by default** (D2
reversed). Run button **removed** (the experiment "run" is the existing sim Play with Record
armed). **Branch** = snapshot a named `Run` (recorded signals + world snapshot + per-signal
stats; restore-point, NOT live-fork). **Compare** = overlay chart + stat-delta table. **Columns**
= per-cell Rune expression (column refs + neighbor access). **Connect** = REST poll + `sensor.*`
stream topics first (each source a `Connector` instance configured via its Properties), then
MQTT/BLE/SQL/OSC as their own opt-in features. **Overlay** = key-column ↔ entity Name/Tag recolor
by a metric. **Dashboard** = a `Dashboard` instance → center-tab tile grid (in-scene GUI later).
Recorder **producer = both** (property tap shipped; stream-topic bridge ships with Connect).

Still open:
- **D-sql** — native SQL driver behind `import-sql` vs external-tool-via-`run_bash` vs
  export-only. *Lean:* export-only first.
- **D-frames-default** — should `frames` default-on inside `data` once eager-polars compile cost
  is measured here? *Lean:* no — arrow-rs-only floor so the cheapest consumers never pay for polars.
- **D-timeseries-layout** — row-major vs block-major default (A.7-1). Undecided which is default.
  **Must resolve before P2 (durable collect).**
- **D-runid** — `hash(config, timestamp)` vs minted UUID (A.7-2). *Lean:* UUID.
- **D-workbench-shape** — a single `DataWorkbench` resource of named `Frame`s vs a `Frame`
  component per `Column` instance. *Lean:* resource, with the ECS `Column` carrying only metadata
  + a handle. **Must resolve before P4 (frames/analysis).**
- **D-residual-storage** — persist the residual `Series` (reproducible overlay, costs a write) vs
  compute per chart frame. *Lean:* persist, tagged synthetic.
- **D-fft-kernel** — `rustfft` on CPU vs a future GPU compute path. *Lean:* `rustfft`; revisit on
  profiling.
- **D-clustering** — k-means floor vs HDBSCAN over the HNSW graph. *Lean:* k-means ships first,
  HDBSCAN reuses the existing neighbor graph.
- **D-datastore-fjall** — migrate race-prone whole-file JSON `datastore_*` onto Fjall with
  `eustress-data` as the columnar *view* (not the write path). Flagged; arguably Subsystem A's call.
- **D-legacy-jsonl** — deprecate the `.eustress/*.jsonl` writers vs keep as projections. *Lean:*
  keep one release (D4: `tail_telemetry`/`compare_runs` must not break), then revisit.

---

## 7. What is designed vs. real

*(Updated 2026-06-24 — the substrate + Studio integration below shipped behind the now-default
`data` feature; verified by `cargo test -p eustress-data` + repeated `cargo check -p
eustress-engine`.)*

- **Real & verified — the substrate (shipped):** the `eustress-data` leaf
  (`Frame`/`ColumnSpec`/`ColumnData`; `numerics` stats/fit/derivative/integral/interpolate;
  `decimate` min-max LOD; `spectral` FFT; `ml` k-means + kNN-anomaly; `import` CSV/JSON; parquet
  I/O — ~60 passing tests); `eustress-data-store` (`Frame` ⇄ `datasets`/`timeseries` partitions +
  `RecorderBuffer` + `query_timeseries_frame`, tested against a real `FjallWorldDb`); the
  `datasets`/`timeseries` WorldDb partitions + `store_with_opts` + CoW for `datasets` in the
  branch overlay; `common/src/dimension.rs` (SI exponent vector); the
  `DataService`/`Dataset`/`Series`/`Column`/`Run` classes + `ClassName` arms + `binary.rs`
  mapping; the Recorder seam + **producer A** (a selected Part's `Transform` sampled each Play
  frame → `timeseries`). **`data` is ON BY DEFAULT** (D2 reversed; CI guard inverted).
- **Real & wired into the live Studio (new):** the contextual **Data ribbon tab** (after
  Drafting); **Chart** as a closable center-tab that opens/closes like a script
  (`CenterTabType::DataChart`, from Explorer double-click + ribbon); **Data Grid** + **Timeline**
  bottom-panel tabs (right of Output); **Stats/Fit/FFT/Cluster/Anomaly** running the real
  `eustress-data` pipeline on the selected Dataset's `.csv` → Output console; **Import** (file
  picker → parse → a new `Dataset` instance in the Space). 10/17 Data-ribbon verbs functional.
- **Still designed, not built:** the GPU chart pipelines (today's Chart is a Slint scaffold with
  baked geometry; the on-screen sub-rect `Camera3d` draw + `DataSelection` linked-view
  projections + in-engine LOD are pending); the generic dynamic-column editable `DataGrid` fed by
  selection; the per-cell **Rune column** engine; **Branch/Compare** (snapshot capture + Run
  registry + compare view); **Connect** (REST/stream first, then the connector taxonomy) +
  Recorder **producer B**; **Overlay**; **Dashboard**; `closed_loop`/`fit_model`/
  `instantiate_model`; the filled `query_stream_events` front door.
- **Deferred, not rejected:** Approach-A floating dashboard wall (readback fast-path); BLE/MQTT/
  OSC/SQL hardware + database connectors (each its own opt-in feature); **live-fork** whole-world
  CoW (snapshot/restore-point chosen first); column-virtualized grid; `timeseries` CoW (blob-level
  workaround interim); nonlinear fit + FEA datasets (gated on CAD Phase C/D).

---

## 8. The one-paragraph summary

Eustress is already a data engine — it produces measured data every tick, runs branchable
experiments, and draws 10K+ GPU instances in one call. This plan gives that data **identity** (a
`Dataset` noun in the tree), a **columnar substrate** (Polars/Arrow in one leaf crate — on by
default since 2026-06-23 — that never leaks beyond it), **GPU-drawn charts** that scale to millions of points by the
same HLOD/Morton/decimation machinery that streams the 3D world, an **editable grid** for manual
entry, **one-bus ETL** through a single Recorder, **dimension-correct analysis** that routes into
the existing solver/FEA/embedvec rather than forking the math, and a **closed-loop digital twin**
where a fit becomes a typed `Quantity` written back into the sim and an experiment is a CoW
branch. Every surface is a linked view over one substrate and one `DataSelection`. Logger Pro X
is the floor; the ceiling is a self-driving data platform the agent operates from inside the
running engine.
