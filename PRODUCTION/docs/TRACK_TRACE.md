# TRACK_TRACE — Knowing the True State of the Chain

> **Cheap, accurate, long-lasting instrumentation for goods — so the supply chain runs on measured truth, not reported optimism.**
>
> Modeled on the WATER project's `WELL_METER.md`: the same way India cannot manage an aquifer
> it does not measure, a supply chain cannot be managed on inventory counts no one has verified.
> This document is the measurement layer that closes the data gaps in `README.md` §8.

---

## Table of Contents

1. [The Problem: Unmeasured Goods](#1-the-problem-unmeasured-goods)
2. [Inspiration: GS1 / EPCIS / Cold-Chain Logger Industry](#2-inspiration-gs1--epcis--cold-chain-logger-industry)
3. [Design Principles: Cheap, Accurate, Long-Lasting](#3-design-principles-cheap-accurate-long-lasting)
4. [The Four Tiers of Tag](#4-the-four-tiers-of-tag)
5. [What Each Tier Can and Cannot Know](#5-what-each-tier-can-and-cannot-know)
6. [Network Architecture](#6-network-architecture)
7. [Rust Firmware: Active Sensor Tag](#7-rust-firmware-active-sensor-tag)
8. [Rust Backend: Ingest, Verify, Reconcile](#8-rust-backend-ingest-verify-reconcile)
9. [The Event Data Model (EPCIS-style)](#9-the-event-data-model-epcis-style)
10. [The Digital Twin in Eustress](#10-the-digital-twin-in-eustress)
11. [Deployment Strategy](#11-deployment-strategy)
12. [Cost Analysis](#12-cost-analysis)
13. [Integration with the Supply Chain Flow Analysis](#13-integration-with-the-supply-chain-flow-analysis)

---

## 1. The Problem: Unmeasured Goods

From `README.md` — the single most important data gap:

> *"A supply chain almost never knows its own true inventory, lead time, or defect rate. The
> largest source of error is the gap between reported state and real state."*

**Current state in a typical chain:**
- Inventory counts come from a system that is updated by hand, on a delay, by people rewarded for speed.
- Inventory-record inaccuracy affects a large fraction of SKUs in studied retail environments — frequently cited near or above half — *(illustrative; verify in your own environment, see §8 of the master doc).*
- "Shipped" means a label printed, not goods through the gate.
- "Passed inspection" is recorded whether or not anyone looked.
- Condition (temperature, shock, humidity) is unknown between the two points where someone happened to check.

**What measurement unlocks** (the same list as WELL_METER, translated to goods):
- True on-hand inventory → the `InventoryLedger` in `README.md` §7 can actually close.
- True transit-time *distribution* → buffers sized to a real percentile, not a guess.
- True first-pass yield and false-negative rate → the bottleneck and the defect source become visible.
- True condition history → spoilage caught at 2 a.m., not at the customer.
- A defensible record → the basis for honest pricing, fair dispute resolution, and regulatory trust.

The principle is identical to the water project's: **you cannot be honest about a thing you cannot see, and you cannot manage what you will not measure.**

---

## 2. Inspiration: GS1 / EPCIS / Cold-Chain Logger Industry

WELL_METER took the USGS / Tucson Water groundwater-monitoring model and rebuilt it for scale and cost. The supply-chain equivalent already exists and is worth standing on:

### What the existing standards do right
- **GS1 identifiers** (GTIN, SSCC, GLN) give every product, pallet, and location a globally unique name — the prerequisite for any honest count.
- **EPCIS** (Electronic Product Code Information Services) standardizes the *event*: the what / when / where / why of every movement, as a shareable record.
- **Barcodes and QR** are effectively free and universal — the baseline of identity.
- **Passive UHF RFID** allows bulk, no-line-of-sight reads at a portal.
- **Cold-chain data loggers** (pharma, food) already travel with sensitive goods and record condition continuously.

### What a chain built for truth needs differently

| Existing practice | Truth-first adaptation |
|-------------------|------------------------|
| Scan at a few control points | Scan/sense at *every transfer of custody* |
| Counts trusted because the system says so | Counts *verified* against physical/sensor reality; discrepancy = alarm |
| Condition checked at endpoints | Condition logged continuously, alarmed in real time |
| Records siloed per company | Shared event records up and down the chain (kills the bullwhip) |
| "Passed" is a checkbox | "Passed" is an evidenced, attributable, timestamped event |

The standards prove the concept. A chain that *acts* on the data — that treats a discrepancy as a sink to be explained rather than an entry to be edited — is the part that is rare.

---

## 3. Design Principles: Cheap, Accurate, Long-Lasting

The same three constraints WELL_METER ranked, in the same order, because they are the same problem:

**1. Cheap — the binding constraint.** At millions of items, every cent of tag cost is real money. The tier must match the value of what it tracks: you do not put a $5 sensor on a $0.50 can. Identity is nearly free; condition costs money; spend it only where condition matters.

**2. Long-lasting / low-friction.** Servicing is the hidden cost killer (WELL_METER's exact finding). A tag that needs configuration, charging, or special handling will not survive contact with a real warehouse. Prefer passive where possible; reserve powered tags for goods that justify them.

**3. Accurate enough for the decision.** Not laboratory precision — *decision* precision. ±1 unit on a pallet count. ±0.5 °C on a vaccine. Enough to close the ledger and trip the right alarm, no more.

---

## 4. The Four Tiers of Tag

> Costs below are **illustrative order-of-magnitude bands** (industry ballparks), not verified
> quotes. Per the data-integrity ethic, treat them as `Unverified` until you get real quotes for
> your volume and geography.

| Tier | Technology | Knows | Powered? | Illustrative unit cost | Use when |
|------|-----------|-------|----------|------------------------|----------|
| **T0 Identity** | Barcode / QR (printed) | *What it is* | No | ~fractions of a ¢ | Everything, always — the baseline |
| **T1 Presence** | Passive UHF RFID inlay | *What + that it passed a reader* | No (reader-powered) | ~3–15 ¢ | Bulk portal reads, pallet/case level |
| **T2 Condition** | Single-use sensor logger | *What + temperature/shock history* | Yes (sealed cell) | ~$1–20 | Cold chain, fragile, one-way trips |
| **T3 Live** | Active BLE/LoRa sensor tag | *What + where + condition, in real time* | Yes (replaceable/long-life) | ~$5–30+ | High-value, reusable assets, live alarms |

The discipline is **tier-matching**: identity (T0) on everything so nothing is anonymous; presence (T1) at custody transfers so movement is verified; condition (T2) on anything that can spoil; live (T3) only on assets whose real-time state is worth the cost. Over-tagging wastes money; under-tagging reopens the truth gap. Match the instrument to the decision it informs.

---

## 5. What Each Tier Can and Cannot Know

Honesty about the limits of measurement is itself part of the measurement. A tag that is trusted beyond its real capability is a new source of false confidence.

```rust
/// What a given tier of instrumentation can legitimately assert.
/// Claiming more than this is a measurement lie dressed as data.
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
pub enum TagTier { Identity, Presence, Condition, Live }

impl TagTier {
    /// Can this tier verify a physical count by itself?
    pub fn verifies_count(&self) -> bool {
        // Even RFID does NOT guarantee a count: missed reads and stray reads
        // both happen. It raises confidence; it does not close the ledger alone.
        false
    }

    /// Does this tier know the goods' condition (temp/shock)?
    pub fn knows_condition(&self) -> bool {
        matches!(self, TagTier::Condition | TagTier::Live)
    }

    /// Does this tier give real-time location/state without a manual read?
    pub fn knows_realtime(&self) -> bool {
        matches!(self, TagTier::Live)
    }
}
```

Key honest limits:
- **A barcode knows identity, not truth of presence** — a scanned label can be on an empty box.
- **RFID raises count confidence but does not close the ledger** — missed and stray reads are real; that is why §7 of the master doc keeps `mass_conservation_check` as the final arbiter, reconciling sensor counts against the ledger rather than trusting either alone.
- **A condition logger knows its own sensor, not the core of the pallet** — placement matters; one logger is a sample, not a guarantee.
- **No tag detects fraud it was not designed to detect** — a tamper that doesn't trip the seal is invisible. Measurement narrows the truth gap; it never closes it to zero. Stay humble about the residual.

---

## 6. Network Architecture

```
[T0/T1 tag] ──scan/portal read──► [Reader / Dock Gateway] ──► [Site Server]
[T2 logger] ──download at gate───►        │                         │
[T3 sensor] ──BLE/LoRa──────────► [Gateway] ──────────────────────► ▼
                                                          [Track-Trace Backend]
                                                                    │
                                                          ┌─────────┴─────────┐
                                                          ▼                   ▼
                                              [Reconciliation Ledger]   [Eustress Digital Twin]
                                              (mass_conservation_check)  (warehouse as a live scene)
                                                          │                   │
                                                          ▼                   ▼
                                              [Alarms: shrinkage,      [Public ChainDashboard
                                               stockout, cold break]    — README §12]
```

This is the WELL_METER topology (sensor → gateway → backend → public portal) with one addition specific to this engine: the backend feeds a **digital twin in Eustress** (§10), so the warehouse's true state is not just rows in a database — it is a navigable 3D scene a human can walk and a model can reason over.

---

## 7. Rust Firmware: Active Sensor Tag

The T3 live tag is the same hardware *family* as the WELL_METER sensor (ESP32-C3 RISC-V + LoRa, bare-metal Rust), so the firmware discipline carries over directly: no heap, no panics in production, deep-sleep between readings for multi-year battery life. The difference is the payload: condition + custody event instead of water level + flow.

```rust
//! track-trace tag firmware — condition + custody beacon.
//! Same bare-metal Rust / ESP32-C3 / LoRa family as WELL_METER's well sensor.
//! Wakes on interval OR on motion (custody change), reads condition, signs, transmits.

#![no_std]
#![no_main]

/// 11-byte packed payload — deliberately the same size class as WELL_METER's,
/// so it fits a single LoRa frame and the same HMAC-signing path applies.
#[derive(Debug, Clone, Copy)]
pub struct TagPayload {
    /// Last 24 bits of the GS1 serial (full ID resolved at backend from dev EUI).
    pub serial_low: u32,
    /// Temperature in centi-°C, signed (−327.68 … +327.67 °C).
    pub temp_c_centi: i16,
    /// Peak shock since last reading, in 0.1 g units.
    pub peak_shock_dg: u16,
    /// Custody-event counter (increments each motion/transfer wake).
    pub custody_seq: u8,
    /// Status bitfield: tamper | over_temp | over_shock | low_battery.
    pub status: u8,
}

impl TagPayload {
    pub const TAMPER: u8     = 0b0000_0001;
    pub const OVER_TEMP: u8  = 0b0000_0010;
    pub const OVER_SHOCK: u8 = 0b0000_0100;
    pub const LOW_BATT: u8   = 0b0000_1000;

    /// Serialize to 11 bytes, big-endian — same MTU budget as WELL_METER.
    pub fn serialize(&self) -> [u8; 11] {
        let mut b = [0u8; 11];
        b[0] = ((self.serial_low >> 16) & 0xFF) as u8;
        b[1] = ((self.serial_low >> 8) & 0xFF) as u8;
        b[2] = (self.serial_low & 0xFF) as u8;
        b[3] = ((self.temp_c_centi >> 8) & 0xFF) as u8;
        b[4] = (self.temp_c_centi & 0xFF) as u8;
        b[5] = ((self.peak_shock_dg >> 8) & 0xFF) as u8;
        b[6] = (self.peak_shock_dg & 0xFF) as u8;
        b[7] = self.custody_seq;
        b[8] = self.status;
        // b[9..11] reserved for future use / firmware version
        b
    }
}
```

The HMAC-SHA256 truncated-to-4-bytes signing from `WELL_METER.md` §7 applies unchanged — every reading is signed with the tag's device key so a swapped or spoofed tag is caught at the backend. Tamper-evidence is the supply-chain version of WELL_METER's "tampered units detected at backend": the moral failure (someone editing reality) is caught by cryptography, not trust.

---

## 8. Rust Backend: Ingest, Verify, Reconcile

The backend mirrors WELL_METER's (Tokio + Axum + a time-series store), with one stage WELL_METER did not need: **reconciliation against the inventory ledger.** Ingesting an event is not enough — the event must be checked against expected state, and a discrepancy must raise an alarm rather than silently overwrite the record.

```rust
//! track-trace backend — ingest → verify HMAC → reconcile → alarm.
//! The reconcile step is what makes this a TRUTH system and not just a log.

use crate::supply_chain::{InventoryLedger};   // from README.md §7

/// A decoded, signature-verified custody/condition event.
#[derive(Debug, Clone)]
pub struct TrackEvent {
    pub gs1_serial: String,
    pub location_gln: String,      // GS1 location where the event occurred
    pub event_kind: EventKind,
    pub temp_c: f32,
    pub peak_shock_g: f32,
    pub hmac_valid: bool,          // false ⇒ spoofed/swapped tag — do NOT trust the data
    pub timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EventKind { Received, Stored, Picked, Shipped, Inspected }

/// Reconcile a period's events for one location against the physical/sensor count.
/// Returns an alarm if the conservation ledger does not close — exactly the
/// `mass_conservation_check` discipline from the master doc, applied to real events.
pub fn reconcile(
    opening: f32,
    events: &[TrackEvent],
    measured_closing: f32,
    tolerance: f32,
) -> ReconcileResult {
    let units_in = events.iter().filter(|e| e.event_kind == EventKind::Received).count() as f32;
    let units_out = events.iter().filter(|e| e.event_kind == EventKind::Shipped).count() as f32;

    let ledger = InventoryLedger {
        opening_count: opening,
        units_in,
        units_out,
        closing_count_measured: measured_closing,
        tolerance,
    };

    let sink = ledger.unexplained_sink();
    ReconcileResult {
        sink,
        // Untrusted (bad-HMAC) events are reported separately, never silently dropped.
        untrusted_events: events.iter().filter(|e| !e.hmac_valid).count(),
        requires_investigation: ledger.requires_investigation(),
    }
}

#[derive(Debug, Clone)]
pub struct ReconcileResult {
    pub sink: f32,
    pub untrusted_events: usize,
    pub requires_investigation: bool,
}
```

The ethic is in the last field: a nonzero sink **requires investigation**. The system is built so the honest action (find the hole) is the default and the dishonest action (edit the count to match) requires going around the system, leaving a trace.

---

## 9. The Event Data Model (EPCIS-style)

Every movement is an event answering **what / when / where / why** — the EPCIS object-event shape, stored as an immutable, append-only record (you may add a correction; you may not erase history).

```sql
-- Append-only custody/condition event log. Corrections are new rows, not edits.
CREATE TABLE track_events (
    event_id     UUID PRIMARY KEY,
    gs1_serial   TEXT NOT NULL,            -- WHAT (the object)
    occurred_at  TIMESTAMPTZ NOT NULL,     -- WHEN
    location_gln TEXT NOT NULL,            -- WHERE (GS1 location)
    event_kind   TEXT NOT NULL,            -- WHY (received/stored/picked/shipped/inspected)
    temp_c       REAL,                     -- condition (NULL for T0/T1 identity-only)
    peak_shock_g REAL,
    hmac_valid   BOOLEAN NOT NULL,         -- was the tag's signature good?
    recorded_by  TEXT NOT NULL,            -- attribution: who/what asserted this
    corrects     UUID REFERENCES track_events(event_id)  -- if a correction, points at the original
);

CREATE INDEX idx_events_serial_time ON track_events (gs1_serial, occurred_at);
CREATE INDEX idx_events_untrusted ON track_events (hmac_valid) WHERE hmac_valid = FALSE;
```

Two honesty features baked into the schema:
- **`recorded_by`** — every assertion is attributable. The principal–agent and dishonest-reporting failures (master doc §6) are deterred when "it shipped" has a name attached.
- **`corrects`** — history is append-only. You cannot quietly rewrite the past to make the ledger close; a correction is itself a visible, attributable event.

---

## 10. The Digital Twin in Eustress

This is the part specific to this engine, and the reason the folder lives in the EustressEngine repo. The Eustress engine already provides — *verified in the codebase, not assumed* — the primitives a warehouse digital twin needs:

- A **world database** holding live entity state (`eustress-worlddb` / the Fjall-backed store).
- **Entity creation and query** over MCP (`create_entity`, `query_entities`, `find_entity`, `inspect_scene`).
- A **tagging system** (`add_tag`, `get_tagged_entities`) — a pallet can be tagged with its GS1 serial, its lot, its condition status.
- A **key/value datastore** (`datastore_get` / `datastore_set`) for per-entity metadata.
- The **realism crate** (§2 of the master doc) for the conservation math.

> **Honesty marker:** these engine capabilities are real and present. Wiring them specifically to
> a track-and-trace feed is an *integration* not yet built — described here as the vision, in the
> WATER documentation style, not claimed as existing supply-chain functionality.

The vision: each tracked pallet becomes an entity in an Eustress scene whose `Transform` is its real warehouse bay, whose tags carry its GS1 identity and lot, and whose metadata carries its live condition. A human walks the warehouse in 3D and sees the *true* state; a model queries the scene and reasons over it. A cold-chain break lights the offending bay red. The unexplained sink (master doc §2) shows up as the gap between the entities the twin contains and the count the floor reports — visible, spatial, impossible to round to zero.

```rust
/// Map a verified track event onto an Eustress world entity.
/// (Integration sketch — uses the engine's real entity/tag/datastore primitives.)
fn apply_event_to_twin(event: &TrackEvent, world: &mut WorldHandle) {
    let entity = world
        .find_by_tag(&event.gs1_serial)
        .unwrap_or_else(|| world.spawn_pallet(&event.gs1_serial));

    world.set_metadata(entity, "location_gln", &event.location_gln);
    world.set_metadata(entity, "temp_c", &event.temp_c.to_string());
    world.set_metadata(entity, "last_event", &format!("{:?}", event.event_kind));

    // Condition alarm becomes a visible state in the scene.
    if event.temp_c > COLD_CHAIN_MAX_C {
        world.add_tag(entity, "ALARM_COLD_CHAIN_BREAK");
    }
    // A spoofed/swapped tag is never silently trusted into the twin.
    if !event.hmac_valid {
        world.add_tag(entity, "ALARM_UNTRUSTED_TAG");
    }
}
```

---

## 11. Deployment Strategy

Phased the WELL_METER way — pilot small, prove accuracy, then scale — never scale on an unproven measurement.

| Phase | Scope | Goal |
|-------|-------|------|
| **Pilot (1 site)** | T0 on all SKUs, T1 at one dock, T2 on the cold SKUs | Prove the ledger closes; measure the real truth gap |
| **Site rollout** | All docks instrumented; reconcile every shift | First *trusted* on-hand count; first real bottleneck data |
| **Chain rollout** | Tags travel between partners; events shared (EPCIS) | Kill the bullwhip; transit-time distribution becomes real |
| **Twin live** | Eustress digital twin per warehouse; public dashboard | Spatial truth; transparency (master doc §12) |

The installation discipline mirrors WELL_METER's 20-minute per-well procedure: a tag is useless until it is *associated* (GS1 serial ↔ device key ↔ first location) and *sealed*. The association step is where most real-world tracking fails — an unassociated tag is anonymous data.

---

## 12. Cost Analysis

> Illustrative bands again — `Unverified` until quoted for your volume.

| Item | Illustrative cost | Notes |
|------|-------------------|-------|
| T0 identity (print) | ~fractions of a ¢/item | Effectively free; the non-negotiable baseline |
| T1 passive RFID | ~3–15 ¢/item | Case/pallet level; reader infrastructure is the real cost |
| Dock reader / gateway | ~$1–5k each | Amortized across all goods through that dock |
| T2 condition logger | ~$1–20/trip | Only on goods where condition matters |
| T3 live sensor tag | ~$5–30+/asset | Reusable; amortize over many trips |
| Backend + twin | cloud-amortized | The WELL_METER backend pattern; shared cost |

The cost logic is WELL_METER's exactly: **the measurement system costs a small fraction of the value it protects, and prevents a far larger sizing/loss error.** A few cents of tag on a pallet that prevents one cold-chain write-off, one phantom-inventory stockout, or one mispriced-because-shrinkage-was-hidden quarter has effectively infinite ROI — the same argument that made metering 30 million wells cheaper than guessing the aquifer.

---

## 13. Integration with the Supply Chain Flow Analysis

```
TRACK_TRACE measurements              Supply Chain Flow Analysis (README.md)
────────────────────────             ───────────────────────────────────────
Verified on-hand counts ───────────► InventoryLedger closes (§7) — sink found, not hidden
Custody events at gates ───────────► "It shipped" becomes true (§5 Shipping)
Demonstrated throughput ───────────► Bottleneck found honestly (§4)
Condition history ─────────────────► Spoilage caught at 2 a.m. (§5 Warehousing)
Inspection events, attributed ─────► False-negative rate measurable (§5 Inspection)
Shared EPCIS events ───────────────► Bullwhip dies (§5 cross-cutting)
Full event-based landed cost ──────► Honest pricing (§10)
Spatial digital twin ──────────────► Public transparency dashboard (§12)
```

### The critical dependency

From `README.md` §8:

> *"The single highest-value investment is closing the measurement gap before committing capital."*

**TRACK_TRACE is how that gap is closed.** Without it, every number in the master document is `Unverified` (§8), every plan rests on reported optimism, and the human failure modes (§6) operate in the dark. With it, the chain runs on measured truth — and an honest chain is, in the end, also the cheapest one to run, because the most expensive thing any supply chain ever pays for is a lie it believed.

---

*Document created: May 29, 2026*
*Folder: PRODUCTION — measurement/inspection layer for the supply chain flow analysis*
*Conceptual model: GS1 / EPCIS / cold-chain loggers, rebuilt for truth at scale — after WATER/docs/WELL_METER.md*
*Implementation family: bare-metal Rust (ESP32-C3) tag + Axum backend + Eustress world-DB digital twin*
*All cost and accuracy figures are illustrative bands, `Unverified` until measured (master doc §8)*
