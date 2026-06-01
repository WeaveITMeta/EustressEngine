# The Measurement & Data Foundation

> **Get measurement and data right, and the rest of the supply chain follows.**
>
> This document does three things the user asked for: (1) a **plan for market research** in
> tracking & measurement, (2) the **gold-standard protocols and standards** that research surfaces,
> with how each **works inside Eustress Engine** on the digital twin, and (3) the **strategy to
> solve the data problem across the entire chain.** It deepens [`TRACK_TRACE.md`](TRACK_TRACE.md)
> from "we should measure" into "here is the standards stack, the engine mapping, and the proof."

> **HONESTY MARKERS.** Standards facts are web-researched and cited (with dates), because some have
> hard 2027–2028 deadlines I will not guess at. Eustress primitives are **verified in the codebase**
> and cited to file. Wiring them into a GS1/EPCIS-conformant pipeline is an *integration to build* —
> labeled as such, never claimed as already shipping. Anything I could not verify is in §7 (Gaps).

---

## Table of Contents

1. [The Thesis: The Data Problem Is the Real Problem](#1-the-thesis-the-data-problem-is-the-real-problem)
2. [The Market-Research Plan (How We Keep This True)](#2-the-market-research-plan-how-we-keep-this-true)
3. [The Gold-Standard Stack (Six Layers)](#3-the-gold-standard-stack-six-layers)
4. [Regulatory Forcing Functions — Why Now](#4-regulatory-forcing-functions--why-now)
5. [How Each Layer Works Inside Eustress Engine](#5-how-each-layer-works-inside-eustress-engine)
6. [Solving the Data Problem Across the Whole Chain](#6-solving-the-data-problem-across-the-whole-chain)
7. [Gaps & Honest Unknowns](#7-gaps--honest-unknowns)
8. [Proof of Concept: The Single Most Important Experiment](#8-proof-of-concept-the-single-most-important-experiment)
9. [References](#9-references)

---

## 1. The Thesis: The Data Problem Is the Real Problem

From [`README.md`](README.md) §1 and §8: the largest source of error in any supply chain is the **gap between reported state and real state**, and *"you cannot size a buffer, find a bottleneck, or set an honest price on numbers you have not verified."* The user's strategic bet is correct and worth stating plainly:

> **Measurement and data are not one workstream among seven. They are the substrate the other six
> stages run on. A production plan, a warehouse buffer, an inspection pass, a price — every one is a
> computation on data. Get the data layer honest and shared, and the rest of the chain becomes
> solvable. Leave it broken, and every downstream optimization is optimizing a lie.**

The good news, which the market research below confirms: **the world has already converged on a gold-standard data stack** (GS1 / EPCIS / RAIN RFID / AAS), and **regulation is now forcing its adoption** with hard deadlines (EU Battery Passport, Feb 2027; US food traceability, July 2028). We do not have to invent the standards. We have to (a) adopt the right ones, (b) make them work inside the Eustress digital twin, and (c) solve the one problem the standards alone don't: sharing data across parties who don't trust each other.

---

## 2. The Market-Research Plan (How We Keep This True)

A market-research plan is worthless if it produces a snapshot that rots. This space moves (EPCIS went 1.x → 2.0 in 2022; the EU battery passport deadline is 2027). So the plan is a **living-research discipline**, not a one-time report.

### 2.1 The four questions every entry must answer

For any standard or protocol we evaluate:
1. **Who owns it, and is it actually ratified?** (a draft is not a standard) — capture the standards body, the document number, and the ratification date.
2. **Who is forced to use it, and by when?** (regulation is the real adoption driver) — capture the jurisdiction and the deadline.
3. **How does it represent identity, events, and condition?** (the three things a twin needs).
4. **Can Eustress speak it?** — map to a real engine primitive (§5) or flag the integration gap.

### 2.2 The watch-list (standards bodies & forcing functions to monitor)

| Source | What to watch | Cadence |
|--------|---------------|---------|
| **GS1** (gs1.org, ref.gs1.org) | EPCIS/CBV revisions, Digital Link, Sunrise 2027 progress | Quarterly |
| **ISO / IEC** | ISO/IEC 19987 (EPCIS), IEC 63278 (AAS), ISO 23247 (DT) | On revision |
| **EU Commission** (ESPR, Battery Reg) | DPP delegated acts by product category | Per delegated act |
| **US FDA** | DSCSA (pharma), FSMA 204 (food) enforcement dates | On rule change |
| **Catena-X / Manufacturing-X** | Data-space standards, battery passport apps | Quarterly |
| **RAIN Alliance / IDTA** | RFID standards, AAS submodel templates | Semi-annual |

### 2.3 The honesty rule for research (same as the V-Cell SOTA doc)

Every claim is tagged: **RATIFIED** (a real, dated standard), **PROPOSED** (in draft/consultation), or **VENDOR** (a supplier's marketing of a standard — useful but not authoritative). We cite the primary source and the date. We do not let a vendor blog stand in for a standards document. *(This mirrors `VCELL_SUPPLY_CHAIN.md`'s VERIFIED/PROJECTED/ASPIRATIONAL discipline.)*

---

## 3. The Gold-Standard Stack (Six Layers)

Research converges on a layered stack — identity at the bottom, data-sharing at the top. Each layer has a clear winner. *(Sources in §9; status as of May 2026.)*

### Layer 0 — Identity: **GS1 System** `RATIFIED`
The global namespace. Every item, case, pallet, and location gets a unique, resolvable name:
- **GTIN** (product), **SSCC** (logistic unit/pallet), **GLN** (location), **SGTIN** (serialized item).
- **GS1 Digital Link** — encodes the identifier as a web URI, so one QR resolves to identity *and* to the digital twin / passport. This is the bridge between the physical item and its data.

### Layer 1 — Data carriers: barcodes, **2D**, and **RAIN RFID** `RATIFIED`
- **GS1-128 / GS1 DataMatrix** — linear and 2D barcodes for logistics and regulated goods.
- **GS1 Digital Link QR** — the 2D code positioned for retail and consumer access.
- **RAIN RFID** (UHF, EPC Gen2v2 / ISO/IEC 18000-63) — bulk, no-line-of-sight reads at a portal.
- **GS1 Sunrise 2027** — the forcing function: by end of 2027, retail POS should universally scan 2D codes alongside linear barcodes; pilots run in 48 countries (~88% of world GDP). Dual-barcoding during transition.

### Layer 2 — The event: **EPCIS 2.0 + CBV 2.0** `RATIFIED` (the keystone)
The traceability event standard — **ISO/IEC 19987:2024**, ratified by GS1 in June 2022. It answers **what / when / where / why** (and, in 2.0, **how** — sensor conditions) for every movement, as a shareable record:
- **JSON-LD** serialization + a **REST API** for capture and query.
- Native **IoT sensor data** support (temperature, humidity, shock) attached to events.
- Compatible with **GS1 Digital Link** identifiers.

> EPCIS is the keystone because it is the *interoperable event* — the unit of shared truth. It is the
> standardized form of the EPCIS-style event already sketched in [`TRACK_TRACE.md`](TRACK_TRACE.md) §9.

### Layer 3 — Sensor/IoT transport `RATIFIED` (mix, per environment)
How a reading gets from the thing to the backend (the [`WELL_METER.md`](../../WATER/docs/WELL_METER.md)/[`TRACK_TRACE.md`](TRACK_TRACE.md) hardware family):
- **LoRaWAN** — long-range, low-power field telemetry (the well-meter pattern).
- **MQTT** (+ **Sparkplug B**) — the lightweight pub/sub backbone for IoT fleets.
- **OPC-UA** — the operational-technology / factory-floor standard (manufacturing-stage data).

### Layer 4 — Digital twin model: **Asset Administration Shell (AAS)** `RATIFIED`
The standardized digital twin of an asset — **IEC 63278-1:2023**, the technical foundation of the Industrie 4.0 digital twin (RAMI 4.0). **ISO 23247** is the complementary digital-twin framework for manufacturing. AAS bundles an asset's data and capabilities into **submodels** with defined interfaces, so a twin is interoperable across MES/ERP/PLM/IoT without custom integration. **This is the standard the Eustress twin should conform to** (§5).

### Layer 5 — Data sharing & sovereignty: **Catena-X / Manufacturing-X / Gaia-X / IDS** `RATIFIED`/`PROPOSED`
The hardest layer, and the one that solves the human problem. Federated **data spaces** let parties share data **without surrendering control of it**:
- **Catena-X** — the open automotive data ecosystem (founded 2021), Gaia-X-based, running the **battery passport**; demonstrated cross-border data-space interoperability (with Japan's Ouranos).
- **Manufacturing-X** — the generalization of Catena-X to all manufacturing.
- **Gaia-X / IDS (International Data Spaces)** — the sovereignty substrate (you keep your data; you grant scoped, revocable access).

This layer is the antidote to the **bullwhip effect** and the **trust failures** of [`README.md`](README.md) §5–§6: competitors can share real demand/inventory signals without exposing their books, because sovereignty is built into the protocol.

### The stack at a glance

| Layer | Gold standard | Status | Eustress primitive (§5) |
|-------|---------------|--------|--------------------------|
| 0 Identity | GS1 (GTIN/SSCC/GLN) + Digital Link | RATIFIED | Entity + **tags** |
| 1 Carrier | GS1 DataMatrix / QR / RAIN RFID | RATIFIED | Tag value = GS1 serial |
| 2 Event | **EPCIS 2.0** (ISO/IEC 19987:2024) | RATIFIED | **Stream topics** (append-only) |
| 3 Transport | LoRaWAN / MQTT / OPC-UA | RATIFIED | Bridge ingest → DataStore |
| 4 Twin model | **AAS** (IEC 63278-1:2023) / ISO 23247 | RATIFIED | Entity + realism components + **DataStore submodels** |
| 5 Data space | Catena-X / Gaia-X / IDS | RATIFIED/PROPOSED | Engine **bridge** (scoped JSON-RPC) |

---

## 4. Regulatory Forcing Functions — Why Now

Adoption is no longer optional in key sectors. These are the deadlines that turn "nice to have" into "illegal to ship without." *(Web-researched, dated; verify against the primary regulator before betting capital.)*

| Regulation | Sector | Hard date | What it mandates |
|------------|--------|-----------|------------------|
| **EU Battery Passport** (Battery Reg 2023/1542) | Batteries >2 kWh (EV + industrial) | **18 Feb 2027** | Digital passport via QR; GS1 Digital Link central |
| **EU Digital Product Passport** (ESPR) | Batteries first; textiles, aluminium, tyres 2027; iron/steel 2028 | phased **2026–2030** | DPP per category, 18 mo after each delegated act |
| **US FSMA 204** (Food Traceability Rule) | "Food Traceability List" foods | **20 Jul 2028** (extended) | Critical Tracking Events + Key Data Elements |
| **US DSCSA** | Pharmaceuticals | rolling (verify FDA) | Unit-level serialization + interoperable trace |

> **The V-Cell consequence is direct.** Per [`VCELL_SUPPLY_CHAIN.md`](VCELL_SUPPLY_CHAIN.md), the V-Cell's
> chosen first market is **grid / industrial storage** — and V-Pack M/L (>2 kWh) and the V-Supreme buffer
> (50 kWh) fall squarely under the EU Battery Passport from **Feb 2027.** A battery passport is not a
> compliance afterthought for Voltec; it is a **data product the V-Cell must emit from birth.** Building
> the data foundation now *is* building the passport. The deadline is the gift — it pays for the work.

---

## 5. How Each Layer Works Inside Eustress Engine

The mapping is grounded in **real, verified engine primitives.** The integration (a GS1/EPCIS-conformant pipeline on top of them) is the work to build.

### 5.1 The primitives that already exist *(verified in the codebase)*

| Eustress primitive | File | What it gives the twin |
|--------------------|------|------------------------|
| **WorldDb** (Fjall LSM-tree) | `eustress/crates/worlddb/src/fjall_backend.rs` | Durable persistence for the whole twin |
| **DataStore** (`get/set/remove/update_async`) | `eustress/crates/worlddb/src/datastore.rs` | Per-item key→value; **`update_async` = atomic compare-and-swap** |
| **OrderedDataStore** (`set_sorted`/`get_sorted_page`) | `eustress/crates/worlddb/src/datastore.rs` | Ranked metrics (turns, dwell, depletion) |
| **Stream topics** (`query_stream_events`, `tail_telemetry`) | `eustress/crates/tools/src/*`, MCP | Append-only event log — the EPCIS substrate |
| **Entities + tags** (`create_entity`, `add_tag`, `get_tagged_entities`, `query_entities`) | MCP tools | The thing + its GS1 identity, queryable |
| **Realism components** (Material/Thermodynamic/Electrochemical) | per `WATER`/V-Cell docs | AAS-style submodels, already attached to Parts |
| **Engine bridge** (TCP JSON-RPC) | `eustress/crates/engine/src/engine_bridge/protocol.rs` | The external capture/query interface (EPCIS-REST analog) |

### 5.2 The layer-by-layer mapping

- **Identity (GS1) → entity + tag.** A pallet is an Eustress **entity**; its GTIN/SSCC is an **`add_tag`** value. `get_tagged_entities("urn:epc:id:sscc:…")` finds it. The GS1 Digital Link URI resolves to this entity.
- **Attributes / AAS submodels / passport fields → DataStore.** Battery-passport fields (chemistry, capacity, lot, carbon footprint) are **DataStore** keys, scoped per item. Durable, JSON-valued, and — critically — **`update_async` gives atomic CAS**, so the inventory ledger of [`README.md`](README.md) §2/§7 closes safely under concurrent updates instead of racing.
- **EPCIS events → stream topics.** A custody/condition event (`received`, `shipped`, `inspected`, `cold_chain_break`) is published to a stream topic like `supplychain.event.shipped`. The engine **already emits tool events to stream topics** (`workshop.tool.datastore_set`, `workshop.tool.add_tag`) and queries them via `query_stream_events` — so the EPCIS append-only event log is a *native* pattern, not a new subsystem.
- **Ranked metrics → OrderedDataStore.** "SKUs by turn rate," "bays by dwell time," "lots by scandium consumption" are `OrderedDataStore` queries (`get_sorted_page`).
- **Sensor transport → bridge ingest.** LoRaWAN/MQTT readings arrive over the **engine bridge** (TCP JSON-RPC), are HMAC-verified (the `TRACK_TRACE.md` §8 pattern), then written to DataStore + emitted as stream events.
- **AAS conformance → entity + realism components.** An AAS "submodel" maps to the entity's existing `MaterialProperties` / `ThermodynamicState` / `ElectrochemicalState` components plus DataStore submodels. **The V-Cell already carries these** (per its `.glb.toml` files) — so a V-Cell entity in Eustress is *already a partial AAS digital twin.* Conforming to IEC 63278 is largely a serialization mapping, not a rebuild.
- **Data space (Catena-X) → scoped bridge access.** External partners query the twin through the bridge with **scoped, revocable** permissions — the sovereignty model, implemented as bridge authorization.

### 5.3 Concrete sketch (uses the real APIs)

```rust
// Ingest a verified EPCIS-style event into the Eustress twin.
// Identity → tag; passport field → DataStore (atomic); event → stream topic.
fn ingest_epcis_event(twin: &mut WorldHandle, ev: &EpcisEvent) -> Result<()> {
    // 1. Resolve or spawn the entity by its GS1 identifier (the tag).
    let entity = twin.find_by_tag(&ev.epc).unwrap_or_else(|| twin.spawn(&ev.epc));

    // 2. Update the durable passport/ledger field with ATOMIC compare-and-swap
    //    (DataStore::update_async) so concurrent readers can't corrupt the count.
    twin.datastore("inventory").update_async(&ev.epc, 3, |prior| {
        let mut rec = decode(prior);
        rec.apply(ev.biz_step);           // received / shipped / inspected …
        rec.last_location = ev.biz_loc.clone();  // GLN
        Some(encode(&rec))
    })?;

    // 3. Emit the append-only EPCIS event onto a stream topic (queryable later).
    twin.publish_stream(&format!("supplychain.event.{}", ev.biz_step), ev.to_jsonld())?;

    // 4. Condition alarms become visible state in the 3D scene.
    if ev.sensor.temp_c > COLD_CHAIN_MAX_C {
        twin.add_tag(entity, "ALARM_COLD_CHAIN_BREAK");
    }
    Ok(())
}
```

> Every call above maps to a primitive verified in §5.1. The work is the conformant wrapper
> (GS1 URI parsing, EPCIS JSON-LD, AAS submodel serialization), **not** new engine infrastructure.

---

## 6. Solving the Data Problem Across the Whole Chain

The standards give us identity, events, and a twin model. They do **not**, by themselves, solve the chain-wide data problem. Three principles do.

### 6.1 Federate, don't centralize (the sovereignty principle)
The naive answer — "one big database everyone writes to" — fails because no party will hand a competitor its books, and any single store is a single point of failure and capture. The **data-space model** (Catena-X / Gaia-X / IDS) is the proven answer: **each party keeps its data; it grants scoped, revocable, auditable access.** In Eustress terms, the twin is federated through the bridge with per-partner authorization. This is what finally kills the **bullwhip effect** ([`README.md`](README.md) §5): a supplier can see real downstream demand without the retailer exposing margins.

### 6.2 Reconcile at every handoff (the conservation principle)
Sharing data is not the same as *trusting* it. At every custody transfer, the receiving party reconciles what arrived against what the EPCIS event claimed — the **`mass_conservation_check`** discipline ([`README.md`](README.md) §2), made safe by **`DataStore::update_async`** CAS. A nonzero sink is an alarm, attributed via the EPCIS `recorded_by` field, investigated — never silently overwritten.

### 6.3 Tier every number's truth (the honesty principle)
Carry the measured-vs-reported distinction into the data model itself: every field knows whether it is **sensor-verified**, **operator-asserted**, or **system-default**. A battery passport that says "carbon footprint: 20 kg CO₂/kWh (PROJECTED, modeled)" is honest; one that presents the same number as measured is the compliance-theater failure ([`README.md`](README.md) §5) in regulatory clothing.

### 6.4 The adoption sequence (cheapest, highest-leverage first)

```
1. IDENTITY    — GS1 tags on everything (nearly free; nothing is anonymous)         [Layer 0]
2. EVENTS      — EPCIS events at every custody transfer (the shared unit of truth)  [Layer 2]
3. CONDITION   — sensors on what can spoil/is precious (tier-matched, TRACK_TRACE)  [Layer 3]
4. TWIN        — the Eustress digital twin: walkable 3D + AAS submodels             [Layer 4]
5. FEDERATE    — open scoped access to partners via the data-space model            [Layer 5]
```

Each step is independently valuable and de-risks the next. Identity alone closes phantom-inventory gaps; events alone kill the "it shipped" lie; you reach the twin and the data space having already paid for themselves.

---

## 7. Gaps & Honest Unknowns

What I could **not** verify, and where Eustress is vision vs. built:

- **DSCSA exact enforcement dates** — I did not search the current FDA pharma timeline; treat the date as `verify`. The *requirement* (unit serialization + interoperable trace) is well-established; the *deadline* is not asserted here.
- **AAS ↔ Eustress component mapping is a design, not a shipped feature.** The realism components exist; an IEC 63278 submodel serializer does not yet. Scope it before promising it.
- **Stream-topic retention/query semantics at supply-chain scale** — the stream system is real for workshop/tool events; its behavior under millions of EPCIS events/day is unproven. Load-test before relying on it as the EPCIS repository.
- **GS1 Digital Link resolver** — Eustress has no GS1 URI resolver today; it's a wrapper to build.
- **Battery-passport field schema** — the EU's final required-field list per the 2027 delegated act should be confirmed against the primary source before building the schema.
- **Cost/throughput numbers** for tags/readers in [`TRACK_TRACE.md`](TRACK_TRACE.md) §12 remain illustrative bands, `Unverified` until quoted.

These are the §8-discipline ([`README.md`](README.md)) applied to this document itself: the gaps are named, not hidden.

---

## 8. Proof of Concept: The Single Most Important Experiment

The V-Cell SOTA review names one experiment that validates-or-kills the whole program (synthesize Sc-NASICON, measure σ by EIS). The data foundation has its equivalent — **the experiment that proves the strategy before we scale it:**

> **Instrument one V-Cell lot end-to-end in the Eustress twin, and prove the ledger closes.**

```
GATE EXPERIMENT (the EIS-equivalent for data)
  1. Tag one V-Cell production lot with GS1 SGTIN identities (Layer 0).
  2. Emit an EPCIS event at each step: made → stored → inspected → shipped (Layer 2),
     each written via DataStore.update_async and published to a stream topic.
  3. Attach the AAS/battery-passport submodel (chemistry, capacity, lot) per cell (Layer 4).
  4. Physically count + weigh the precious inputs (scandium) and reconcile against the ledger.

  PASS  if  mass_conservation_check(expected, measured) ≈ 0 within tolerance
            AND every cell resolves from its QR (Digital Link) to its twin record.
  FAIL  if  the ledger won't close, OR the stream system can't sustain the event rate
            → fix the measurement layer before instrumenting anything else.
```

This is deliberately small (one lot, one site) and deliberately decisive: it proves the identity → event → twin → reconcile loop works on real goods, in the real engine, before a cent is spent scaling it — exactly the WATER project's "pilot before megaproject" discipline.

---

## 9. References

### Market research (web-sourced, May 2026)
- EPCIS 2.0 / CBV 2.0 (ISO/IEC 19987:2024): [GS1 EPCIS](https://www.gs1.org/standards/epcis), [EPCIS/CBV 2.0 launch (GS1 PDF)](https://www.gs1.org/docs/epcis/epcis_2-0_launch.pdf)
- GS1 Sunrise 2027 (2D migration): [GS1 US — Sunrise 2027](https://www.gs1us.org/industries-and-insights/by-topic/sunrise-2027), [GS1 2D at retail POS guideline](https://ref.gs1.org/guidelines/2d-in-retail/)
- EU Battery Passport (Feb 2027): [Circularise — EU battery passport requirements](https://www.circularise.com/blogs/eu-battery-passport-regulation-requirements), [CEPS in-depth analysis (PDF)](https://circulareconomy.europa.eu/platform/sites/default/files/2024-03/1qp5rxiZ-CEPS-InDepthAnalysis-2024-05_Implementing-the-EU-digital-battery-passport.pdf)
- EU Digital Product Passport (ESPR, 2026–2030): [Circularise — DPPs across sectors](https://www.circularise.com/blogs/dpps-required-by-eu-legislation-across-sectors), [Hogan Lovells — DPP & battery passport pilot](https://www.hoganlovells.com/en/publications/digital-product-passports-in-the-eu-comprehensive-expansion)
- US FSMA 204 (extended to 20 Jul 2028): [FDA — Food Traceability Rule](https://www.fda.gov/food/food-safety-modernization-act-fsma/fsma-final-rule-requirements-additional-traceability-records-certain-foods), [Federal Register — compliance date extension](https://www.federalregister.gov/documents/2025/08/07/2025-14967/requirements-for-additional-traceability-records-for-certain-foods-compliance-date-extension)
- Asset Administration Shell (IEC 63278-1:2023) / ISO 23247: [IIC/IIoT — Digital Twin & AAS whitepaper (PDF)](https://www.iiconsortium.org/pdf/Digital-Twin-and-Asset-Administration-Shell-Concepts-and-Application-Joint-Whitepaper.pdf)
- Catena-X / Gaia-X data spaces / battery passport: [Catena-X — Digital Product Passport](https://catena-x.net/use-case-cluster/digital-product-passport/), [Catena-X × Ouranos interoperability](https://catena-x.net/news/catena-x-and-ouranos-ecosystem-successfully-demonstrate-data-space-interoperability/)

### Eustress codebase (verified)
- `eustress/crates/worlddb/src/datastore.rs` — DataStore / OrderedDataStore (Roblox-parity, Fjall-backed, `update_async` CAS)
- `eustress/crates/worlddb/src/fjall_backend.rs`, `backend.rs` — WorldDb trait + Fjall LSM backend
- `eustress/crates/engine/src/engine_bridge/protocol.rs` — TCP JSON-RPC bridge (external capture/query)
- `eustress/crates/tools/src/*`, `eustress/crates/mcp-server/src/*` — stream topics, entity/tag/datastore tools
- `eustress/crates/common/src/realism/laws/conservation.rs` — `mass_conservation_check` (the reconciliation primitive)

### Framework documents (this folder)
- [`README.md`](README.md) — Supply Chain Flow Analysis (the §1/§8 truth-gap thesis)
- [`TRACK_TRACE.md`](TRACK_TRACE.md) — the hardware/measurement layer this document gives standards to
- [`VCELL_SUPPLY_CHAIN.md`](VCELL_SUPPLY_CHAIN.md) — the V-Cell application; battery-passport consequence

---

*Document created: May 31, 2026*
*Folder: PRODUCTION — the measurement & data foundation for the supply chain digital twin*
*Standards web-researched and dated; Eustress primitives verified in the codebase; integration scoped as work-to-build*
*Strategy: get identity + events + condition honest and shared, and the rest of the chain follows*
