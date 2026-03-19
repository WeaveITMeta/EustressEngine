# The Cube — Voltec Kinetic Workshop Tracker

## What It Is

The Cube is a self-powered, batteryless IoT tracking module designed for permanent
attachment or embedded installation into workshop tools. It harvests energy entirely from
the physical motion of the tool it is attached to — no batteries, no charging, no maintenance.

Every time a tool moves, the Cube fires a **BLE advertisement** (13 bytes, ~65 μJ —
always within the single-event harvest budget). Workshop gateways receive this signal
from multiple known positions and **triangulate the tool’s precise indoor location**
(±0.3–1m with BLE 5.1 AoA, ±2–5m with RSSI) — enough to resolve which bench, shelf,
or drawer the tool is in.

Simultaneously, the Cube accumulates charge into a **10 mF supercapacitor bank**.
When the bank reaches 35 mJ and the tool has been at rest for 500ms, it fires a
**GPS + LTE-M fix** — an outdoor fallback for when the tool leaves the workshop
(jobsite, van, theft). Both modes run from the same **Nordic nRF9161 SiP**
(GNSS + LTE-M + BLE in one package). One chip. One prototype. No batteries. No compromise.

---

## Physical Form Factor

```
      ┌─────────────────┐
      │  ░░░░░░░░░░░░░  │   18 × 18 × 8 mm  (2mm taller than prototype concept)
      │  ░  THE CUBE  ░ │   Aluminium 6061-T6 housing
      │  ░░░░░░░░░░░░░  │   IP68 rated (dust + waterproof)
      │   [  VOLTEC  ]  │   M3 mounting threads × 4 corners
      └─────────────────┘   OR epoxy embed port (tool OEM integration)
```

**Dimensions:** 18 × 18 × 8 mm *(the extra 2mm vs original concept accommodates the 10 mF supercap bank)*  
**Weight:** 5.1 g  
**Housing:** Aluminium 6061-T6 (impact resistant, non-magnetic, EMI shielded)  
**Mounting options:**
- Surface mount — M3 screws into tapped insert, countersunk flush
- Embedded — drop-in pocket routed into tool handle/body, epoxy sealed
- Retrofit clip — spring-steel strap clip for existing tools (sold separately)

---

## How It Works — Energy Harvesting

The Cube uses a **piezoelectric kinetic energy harvester** stacked with an
**electromagnetic induction coil** (dual-mode harvester):

```
  Tool motion (vibration + shock + angular acceleration)
        │
        ▼
  ┌─────────────────────────────────────────┐
  │  HARVESTER STACK                        │
  │  ┌──────────────┐  ┌──────────────┐    │
  │  │ Piezo array  │  │  EM coil +   │    │
  │  │ PZT-5H discs │  │  NdFeB mass  │    │
  │  └──────┬───────┘  └──────┬───────┘    │
  │         └────────┬─────────┘           │
  │               AC output                │
  └───────────────┬─────────────────────────┘
                  │
                  ▼
  ┌───────────────────────────────────────┐
  │  PMIC (Power Management IC)           │
  │  AEM10941 — rectifier + supercap bank  │
  │  charges 10 mF ceramic supercap bank  │
  │  (10 × 1 mF ceramics in parallel)     │
  │  fires when Vcap ≥ 3.0V              │
  └───────────────┬───────────────────────┘
                  │
                  ▼
  ┌───────────────────────────────────────┐
  │  MCU + RADIO                          │
  │  Nordic nRF9161 SiP                   │
  │  BLE 5.3 + LTE-M + GNSS integrated   │
  │  Dual-mode fire logic (see below)    │
  └───────────────────────────────────────┘
```

## Dual-Mode Fire Logic

Every motion event the nRF9161 firmware executes this decision tree:

```
Tool picked up (departure)         Tool set down (arrival)
        │                                   │
        ▼                                   ▼
  Harvester fires                     Harvester fires
  Bank charges                        Bank charges
        │                                   │
        ▼                                   ▼
  Bank ≥ 3.0V?                        Bank ≥ 3.0V?
  YES (always after ~1 event)         YES (always after ~1 event)
        │                                   │
        ▼                                   ▼
  Fire BLE departure            Fire BLE arrival
  13 bytes, ~65 μJ              13 bytes, ~65 μJ
  → Gateway → MQTT              → Gateway → MQTT
                                            │
                                ┌───────────▼──────────────┐
                                │  Wait 500ms of stillness  │
                                │  (GPS_WAIT_FOR_REST_MS)   │
                                │  Motion restarts? → abort │
                                └───────────┬──────────────┘
                                            │ Tool confirmed at rest
                                ┌───────────▼──────────────┐
                                │  Bank ≥ 2.65V (~35 mJ)?  │
                                │  Every ~130 arrivals      │
                                └───────────┬──────────────┘
                                            │ YES
                                            ▼
                                ┌───────────────────────────┐
                                │  Fire GPS + LTE-M burst   │
                                │  AGPS fix ~800ms, ~20 mJ  │
                                │  LTE-M publish ~1s, ~15 mJ│
                                │  Precise lat/lon → MQTT   │
                                │  = tool's resting position│
                                └───────────────────────────┘
```

**Primary indoor positioning: BLE triangulation via workshop gateways.**  
Every BLE packet (departure + arrival) is received by 2–3 gateways at known positions.
The gateway resolves the tool’s indoor coordinates from signal strength or AoA angle.
This is what tells you the tool is on Bench 3, Drawer 2, or the top shelf.

**GPS only fires after the tool stops moving, and only as an outdoor fallback.**  
A GPS fix takes ~800ms and requires open sky. It is useless indoors. It fires
exclusively on arrival events after 500ms of confirmed rest — capturing the tool’s
resting position when it has left the workshop and there are no gateways to resolve
indoor position. Every GPS fix in the movement log is a verified outdoor resting
coordinate — the jobsite, the van, the job box.

**Energy budget — per event and accumulated:**

| Mode | Energy consumed | Harvest per event | Events to fire |
|---|---|---|---|
| BLE advertisement | ~65 μJ | ~270 μJ | **Every event** (4× margin) |
| GPS + LTE-M | ~35 mJ | ~270 μJ | **Every ~130 events** |

**10 mF supercap bank math:**
```
Capacity:  10 × 1 mF ceramics in parallel = 10 mF
Max voltage: 3.3V
Energy at full charge: ½ × 10×10⁻³ × 3.3² = 54.5 mJ
GPS threshold voltage: √(2 × 35mJ / 10mF) = 2.65V  (64% of full)

At 10 tool uses/day × 0.27 mJ/event harvested:
  Time to reach GPS threshold from empty: 35 mJ / 2.7 mJ/day = ~13 days cold start
  Steady state (top-up): 35 mJ / 2.7 mJ/day = ~13 days between GPS fixes
  At 50 uses/day: GPS fix every ~2.6 days
  At workshop use (100+ moves/day): GPS fix ~daily
```

**What each mode delivers:**
- **BLE every event** — real-time departure/arrival events, **precise indoor position**
  via workshop gateway triangulation (±0.3–1m AoA, ±2–5m RSSI). This is the primary
  tracking system. No gateway present: falls back to flash-store and uploads on next
  BLE contact.
- **GPS + LTE-M when charged (outdoor fallback)** — fires only when the tool has left
  the workshop. Provides lat/lon on jobsites or during transit where no BLE gateways
  exist. ~130 arrivals between GPS events at typical workshop use frequency.

---

## Moving Parts

| Component | Moving? | Wear mechanism | Lifetime |
|---|---|---|---|
| Piezoelectric PZT-5H discs | Flex only (no sliding) | Fatigue cracking after >10⁹ cycles | >100 years at 10 moves/day |
| EM induction coil mass | Linear oscillation, 0.3 mm travel | Bearing wear | Frictionless — magnetic levitation guide |
| All electronics | None | Electromigration, ESD | 50+ years MTBF |
| Housing | None | Corrosion | Al 6061-T6 + anodise = indefinite |

**Effective moving parts: 1** (the EM proof mass — magnetically suspended, zero contact)  
**Wear surfaces: 0**

Best-case scenario: **The Cube outlasts the tool it is attached to.**

---

## Energy Budget — Why BLE + Gateways, Not GPS Per Event

This is the most important design decision in The Cube and it comes down to physics:

| Radio technology | Energy per event | Harvest available | Verdict |
|---|---|---|---|
| GPS + LTE-M (nRF9161) | 20–35 mJ | ~0.27 mJ | ❌ 100× deficit per event |
| Wi-Fi (ESP32 style) | 8–15 mJ | ~0.27 mJ | ❌ 30× deficit |
| BLE 5.3 advertisement | **~65 μJ** | ~0.27 mJ = 270 μJ | ✅ **4× positive margin** |

GPS + LTE-M wakes a 220 mA modem for 1–2 seconds and needs open sky to acquire
a satellite fix. It is too expensive per event, and it does not work indoors at all.
GPS is not a precision indoor positioning system — it is a last-resort outdoor fallback.

BLE advertisement costs ~65 μJ — well inside the harvest budget — and delivers
**sub-metre indoor positioning** when received by 2–3 workshop gateways using
BLE 5.1 Angle-of-Arrival. Gateways are the sensor; The Cube is the beacon.

**The solution to the energy constraint is BLE + gateway infrastructure for indoors,
and accumulated-charge GPS only for outdoor fallback.**

---

## Compressed Wire Format — CubePacket (13 bytes)

The Cube transmits the absolute minimum bytes needed to drive the `KineticChipState`
state machine and correlate BLE arrival packets with deferred GPS fixes.
No JSON, no string fields, no redundancy. Pure binary, big-endian.

```
 Byte  Width  Field            Encoding
 ────  ─────  ───────────────  ──────────────────────────────────────────
  0      1    version          0x02 (merged dual-mode format)
  1      1    flags            bit 0 = event (0=departure, 1=arrival)
                               bit 1 = has_rssi
                               bit 2 = has_temp
                               bit 3 = gps_fired (arrival + rest + bank ≥ 35 mJ)
                               bit 4–7 = reserved (0)
  2–4    3    chip_id_short    Lower 24 bits of chip UUID (3 bytes)
                               Gateway resolves to full UUID via registry
  5–6    2    bank_mv          10 mF supercap bank voltage in mV, u16 big-endian
                               Range 0–3300 mV
  7–8    2    event_seq        Monotonic event counter, u16 big-endian (wraps 65535)
                               Correlates BLE arrival with deferred GPS payload
  9      1    harvested_uj_u8  Harvested energy in μJ / 4 (range 0–1020 μJ)
                               Divide by 4 on decode
 10      1    rssi_u8          RSSI as (rssi_dbm + 200) → 0–255
                               Only valid if flags bit 1 set
 11      1    temp_c_u8        (temp_celsius + 40) → 0–125 (range −40 to +85°C)
                               Only valid if flags bit 2 set
 12      1    reserved         0x00

 Total: 13 bytes
```

**Energy cost of 13-byte BLE payload vs verbose JSON:**

```
JSON (typical):   ~180 bytes → 14.4 μs air time → ~5.3 μJ TX
Binary (13 bytes): 13 bytes  →  1.0 μs air time  → ~0.37 μJ TX

MCU overhead dominates at both sizes (~50 μJ wake/sleep).
The real gain is gateway processing speed and deferred flash storage:
  - Flash write: 180 bytes vs 13 bytes → 14× fewer write cycles
  - Gateway queue: 14× more events fit in RAM before flush
```

**BLE advertising topology:**

```
Cube (nRF9161 SiP)
  │  BLE non-connectable advertisement
  │  Manufacturer-specific data field (13 bytes)
  │  Company ID: 0xFFFF (Voltec / WeaveITMeta)
  │
  ▼
Workshop Gateway (Raspberry Pi 5 + BLE dongle, or phone)
  │  Receives advertisement simultaneously from 2–3 known gateway positions
  │  Resolves chip_id_short → full chip UUID via local registry cache
  │  Triangulates INDOOR position: RSSI (±2–5m) or BLE 5.1 AoA (±0.3–1m)
  │  Decodes CubePacket → IoTTelemetry struct with resolved space_position
  │
  ▼
MQTT Broker (local or cloud)
  Topic: workshop/tools/{full_chip_id}/telemetry
  Payload: JSON IoTTelemetry (expanded from binary by gateway)
  │
  ▼
Eustress Workshop (MQTT subscriber)
  Calls LiveStatusStore::process_kinetic_event()
  KineticChipState state machine advances
  StorageManager moves .tool.toml file
```

---

## MQTT Telemetry Payload (gateway-expanded)

The gateway decodes the 13-byte `CubePacket`, resolves the indoor position from
multi-gateway RSSI or AoA, and publishes a full JSON payload.
The Eustress engine only ever sees the expanded form:

Topic: `workshop/tools/{chip_id}/telemetry`

```json
{
  "chip_id": "cube-a1b2c3d4e5f6",
  "tool_id": "550e8400-e29b-41d4-a716-446655440000",
  "event": "departure",
  "zone_label": "Bench 3",
  "space_position": [2.4, 0.9, 1.1],
  "rssi_dbm": -62,
  "bank_mv": 3140,
  "gps_fired": false,
  "harvested_uj": 216,
  "temp_c": 22.0,
  "gateway_id": "gw-workshop-main",
  "captured_at": "2026-03-18T10:26:00Z"
}
```

`event` is either `"departure"` (tool picked up) or `"arrival"` (tool set down).  
The `KineticChipState` state machine in `status.rs` processes these pairs.

**Indoor position resolution** is the gateway’s primary job:
- **RSSI trilateration** — 2–3 gateways, pre-calibrated `workshop.toml` map → ±2–5m
- **BLE 5.1 AoA** — directional antenna arrays on gateways → ±0.3–1m
- **Zone fallback** — single gateway → room/zone label only

Container-level resolution (which bench, drawer, shelf) requires only ±2m. AoA
achieves bin-level precision. The Cube is the beacon; the gateways are the sensors.

**GPS is not used for indoor positioning.** It fires only when the tool has left
the workshop and no gateways are present (jobsite, van, transit). See Dual-Mode
Fire Logic above.

---

## Voltec Gateway — Indoor Positioning Infrastructure

### Hardware — Voltec Gateway G1

The **Voltec G1** is the first-party branded gateway node. Built on the
**Nordic nRF21540 RF front-end + 4-element patch antenna array**, it is
**BLE 5.1 AoA-capable** — the highest-accuracy indoor BLE positioning available.

```
┌─────────────────────────────────────────┐
│           VOLTEC GATEWAY G1             │
│  Nordic nRF21540 AoA + Compute Module   │
│  4-element patch antenna array          │
│  PoE 802.3af — single cable install     │
│  Magnetic ceiling mount + ¼" thread     │
│  120 × 80 × 25mm  Anodised Al housing  │
└─────────────────────────────────────────┘
```

AoA accuracy: **±0.3–1m** at 10m range (vs ±2–5m for plain RSSI).
That is bench-level → **drawer-level** precision as the gateway count increases.

### Node Count Rules

| Nodes | What you get | Accuracy |
|---|---|---|
| **1 node** | Zone / room label only — "near G1-01" | ±5–15m |
| **2 nodes** | Reduced-accuracy 2D fix (ambiguous arc) | ±1–3m |
| **3 nodes — minimum recommended** | True trilateration, unambiguous 2D fix | ±0.3–1m |
| **4+ nodes** | Overdetermined, handles shelf occlusion | ±0.2–0.5m |

AoA note: 2 AoA nodes intersect two angle vectors → exact point. 3 is robust
against occlusion. **3-node Starter Kit is the correct default recommendation.**

### Pricing

| SKU | Contents | Retail |
|---|---|---|
| **Voltec Gateway G1 (single)** | 1× G1, PoE injector, ceiling mount | **$149** |
| **Voltec Gateway Starter Kit** | 3× G1, PoE switch, setup guide | **$399** |
| **Voltec Gateway Pro Kit** | 5× G1, managed PoE switch, calibration wand | **$699** |

**The Cube:** $39–$59 retail. IP68, kinetic, indefinite service life — justified
premium over Tile Pro ($35) and AirTag ($29) which are battery-dependent.

### Placement

Mount at ceiling height (2.5–4m), spaced so every workshop point is visible to
≥2 nodes. For a 10 × 20m workshop: 3 nodes at ceiling corners/far-wall centre.
After physical install, run the **Eustress Calibration Wand** (walk a Cube through
the space at known grid points, ~10 min) — Studio writes `workshop.toml` with the
fitted path-loss + AoA offset model per gateway.

### Phone as Stationary Node

The Voltec Workshop App supports a **Stationary Gateway Mode**:

1. Open app → Workshop → **Use Phone as Gateway**
2. App prompts: *"Place your phone face-up on a flat surface and do not move it"*
3. User taps their position on the workshop floor plan (or scans a wall QR marker)
4. Phone enters passive BLE scanner mode, publishes RSSI + position to MQTT
5. **Phone accelerometer monitors for movement** — if picked up or shifted, it
   immediately removes itself from the gateway pool:
   `{"gateway_id": "phone-xxx", "status": "offline", "reason": "moved"}`
6. Server drops the phone's contribution until the user re-calibrates

The phone contributes as an **RSSI node (±2–5m)**, not AoA. Its primary value is
acting as the third node for workshops with only 2 fixed gateways installed, giving
them a valid trilateration fix when the phone is stationed.

See `GATEWAY_ARCHITECTURE.md` for full deployment guide, MQTT envelope format,
calibration procedure, and system architecture diagram.

---

## Registration — How a New Cube Joins a Workshop

Registration supports **two paths** — computer and mobile app — both work simultaneously:

### Path A — Computer (Eustress Studio)
1. Open Eustress Studio → Workshop panel → **Register New Cube**
2. Shake the Cube to wake it — it broadcasts a BLE advertisement
3. Studio detects it automatically (no pairing code needed)
4. User types the tool name and selects a tool category
5. Studio generates a new UUID, writes a `.tool.toml` to `tools/` root
6. Sends the UUID + MQTT broker address to the Cube over BLE
7. Cube stores credentials in flash, enters operational mode
8. **Done — under 60 seconds**

### Path B — Mobile App (Voltec Workshop App, iOS + Android)
1. Open the Voltec Workshop app → **+ Add Tool**
2. Shake the Cube to wake it — app detects BLE advertisement
3. User photographs the tool (optional — stored in `spec.extra.photo_url`)
4. User enters tool name and selects a storage location from the workshop map
5. App generates UUID + provisions the Cube over BLE
6. App syncs the new `.tool.toml` to the Eustress workspace via the Workshop REST API
7. **Done — under 45 seconds**

### Path C — OEM Pre-registration (for custom tools with embedded Cube)
Tools manufactured with an embedded Cube can ship pre-registered.
The manufacturer's tooling flashes the UUID and MQTT config at the factory.
When the tool arrives and is first moved in the workshop, it self-registers
by sending a `{"event": "first_seen"}` telemetry burst — Eustress creates
the `.tool.toml` automatically from the embedded manifest.

---

## Mass Production

The Cube is designed for high-volume PCBA (Printed Circuit Board Assembly) manufacture:

| Stage | Method | Target cost at volume |
|---|---|---|
| PCB fabrication | 4-layer rigid PCB, JLC or equivalent | $0.80/unit at 10k |
| Component placement | Pick-and-place PCBA, SMT | $1.20/unit at 10k |
| Harvester stack | PZT disc array press-bonded, automated | $2.10/unit at 10k |
| Housing | Aluminium CNC at 1k, die-cast at 50k+ | $0.60–$1.80/unit |
| Testing + flash | Automated test jig, 30 sec/unit | $0.40/unit |
| **Total BOM + assembly** | | **~$5.10/unit at 10k** |
| **Retail target** | | **$29–$49/unit** |

### Bill of Materials — Key Components

| Component | Part | Qty | Unit Cost (10k) |
|---|---|---|---|
| MCU + Radio + GNSS | Nordic nRF9161-SICA SiP | 1 | $8.50 |
| PMIC + harvester IC | e-peas AEM10941 | 1 | $2.20 |
| Supercapacitor bank | Ceramic 1 mF, 3.3V × 10 in parallel = 10 mF | 10 | $0.45 |
| Piezo discs PZT-5H | 12mm × 0.5mm, 3 stacked | 3 | $0.30 |
| EM proof mass | NdFeB N52, 8mm sphere | 1 | $0.80 |
| EM coil | 200 turn, 28 AWG, 14mm OD | 1 | $0.35 |
| Flash memory | Winbond W25Q16 2MB SPI | 1 | $0.18 |
| Passive components | 0402 resistors/caps | ~30 | $0.40 |
| Housing (die-cast) | Al 6061-T6, anodised | 1 | $0.60 |
| PCB 4-layer | 18×18mm, immersion gold | 1 | $0.80 |

---

## Durability — Best Case Scenario

| Parameter | Value | Basis |
|---|---|---|
| Piezo fatigue life | >10⁹ cycles | PZT-5H published data |
| EM coil resistance change | <0.1% over 10⁸ cycles | Published EM harvester studies |
| nRF9161 MTBF | >500,000 hours | Nordic published |
| Supercap cycle life | >500,000 charge cycles | Murata published |
| Al housing corrosion | Indefinite (anodised) | MIL-A-8625 Type II |
| IP rating | IP68 (1m, 30 min) | IEC 60529 |
| Operating temperature | −40°C to +85°C | nRF9161 spec |
| Shock rating | 1500G, 0.5ms | MIL-STD-810H Method 516 |
| **Designed service life** | **Indefinite** | Zero wear surfaces |

**The Cube has no wear surfaces, no battery to degrade, and no moving parts that
contact each other. The only finite component is the piezo disc stack, rated for
more cycles than any human will use a tool in a lifetime.**

---

## Embedded Tool Module — OEM Integration

For custom tool manufacturers, The Cube is available as an **embedded module**:

```
Tool handle cross-section (e.g. drill):
                                        ┌──────────────────┐
  ┌────────────────────────────┐        │   CUBE MODULE    │
  │    TOOL HANDLE BODY        │        │  18×18×8mm       │
  │                            │        │  epoxy-filled    │
  │    ┌──────────────┐        │        │  pocket          │
  │    │  CUBE POCKET │   ←────┼────────┘                  │
  │    │  routed out  │        │                            │
  │    └──────────────┘        │                            │
  │    (filled with Loctite    │                            │
  │     EA E-40DB epoxy)       │                            │
  └────────────────────────────┘
```

**Optimal placement guidelines:**
- **Drills / impact drivers:** In the handle, behind the grip — high vibration capture
- **Wrenches / spanners:** At the head end — captures torque shock on set-down
- **Saws:** Near the blade guard — captures start/stop vibration
- **Calipers / measuring tools:** In the handle end — captures pick-up motion
- **Stationary machines (CNC, lathe):** On the housing — captures startup vibration

The embed pocket dimensions (18.5 × 18.5 × 8.5 mm) are included in every
Cube datasheet as a reference drawing for tool manufacturers.

---

## Eustress Workshop Integration

The Cube maps directly to the `eustress-workshop` crate:

| Cube concept | Crate type |
|---|---|
| Chip ID | `ToolIotConfig.chip_id` |
| MQTT topic | `ToolIotConfig.mqtt_topic` |
| Departure event | `KineticPhase::Departed` → `OperationalState::InTransit` |
| Arrival event | `KineticPhase::Arrived` → `OperationalState::Available` |
| Position → container | `ContainerIndex::find_container_for_point()` |
| File move | `StorageManager::move_to_active_use()` / `return_from_active_use()` |
| Silence inference | `KineticChipState::infer_state(stale_hours, missing_hours)` |

See `src/status.rs` and `src/storage.rs` for full implementation.
