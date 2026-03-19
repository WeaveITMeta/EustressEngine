# PATENT — The Cube: Batteryless Kinetic GPS Tool Tracking Module

**Voltec / WeaveITMeta**  
**Filing Date:** 2026-03-18  
**Status:** Provisional Patent Application

---

## Title of Invention

**Self-Powered Kinetic Energy-Harvesting Dual-Mode Tracking Module with
Per-Event BLE Indoor Triangulation and Accumulated-Charge Outdoor GPS Fallback
for Indefinite-Lifetime Tool Registration and Digital Twin Integration**

---

## Abstract

A miniaturized (18 × 18 × 8 mm), batteryless tracking module — herein called
"The Cube" — harvests energy from the kinetic motion of a host object (a workshop
tool) using a stacked dual-mode harvester comprising piezoelectric PZT-5H disc
arrays and an electromagnetically levitated induction proof mass. Harvested energy
charges a 10 millifarad ceramic supercapacitor bank (ten 1 mF ceramics in parallel)
through a power management IC (PMIC). The module implements a dual-mode transmission
architecture using a single Nordic nRF9161 System-in-Package (BLE 5.3 + LTE-M +
GNSS integrated):

1. **Per-event BLE advertisement — primary indoor positioning** — 13-byte binary
   packet transmitted at every motion event (~65 μJ consumed, 4× positive margin).
   Workshop gateways at known positions receive the advertisement simultaneously
   and resolve the tool’s precise indoor coordinates via RSSI trilateration
   (±2–5m) or BLE 5.1 Angle-of-Arrival (±0.3–1m). This is the primary positioning
   system — sufficient to resolve which bench, drawer, or shelf the tool occupies.
   GPS is not used for indoor positioning and cannot provide it.

2. **Accumulated-charge GPS + LTE-M — outdoor fallback only** — when stored bank
   energy crosses 35 mJ (bank voltage ≥ 2.65V) and the tool has been at rest for
   500 ms continuous stillness (confirming it is no longer moving), the module fires
   a full GNSS position fix and LTE-M transmission. This fires only on arrival events
   and records the tool’s verified outdoor resting position — the jobsite, vehicle,
   or remote storage. It provides continuity of tracking when the tool has left the
   workshop and no BLE gateway infrastructure is available.

The energy deficit between GPS (20–35 mJ) and a single motion event harvest
(~0.27 mJ) is resolved by accumulation across events, not by a larger power source.
No battery or capacitor bank replacement is required at any point. The module
contains zero sliding or contacting wear surfaces and is designed for indefinite
service life coextensive with the life of the host tool.

---

## Field of Invention

Physical asset tracking; industrial Internet of Things (IIoT); energy harvesting;
workshop management systems; digital twin synchronization; GPS tracking devices.

---

## Background

### Limitations of Current Technology

| Technology | Power source | Lifetime | Accuracy | Maintenance |
|---|---|---|---|---|
| Bluetooth Low Energy (BLE) beacon | CR2032 battery | 1–2 years | ±3–5 m (indoor) | Battery replacement |
| UWB anchor + tag system | Rechargeable LiPo | 6–18 months | ±10–30 cm | Daily/weekly charging |
| Passive RFID | None (passive) | Indefinite | Must be within 1 m of reader | Reader infrastructure |
| Active GPS tracker | Li-Ion battery | 1–6 months | ±2–5 m | Frequent charging |
| QR code label | None | Indefinite | Manual scan only | Manual scan required |

**The Problem:** Every existing active tracking solution requires either periodic battery
replacement, regular charging infrastructure, or manual scanning by a human. In a
workshop with hundreds of tools, this maintenance burden is prohibitive. Batteries
degrade, tracking coverage lapses, and the system becomes unreliable within months.

**The Gap:** No existing commercial solution combines (a) zero-maintenance batteryless
operation, (b) GPS-grade outdoor accuracy with indoor fallback, (c) autonomous
departure-arrival event detection, and (d) permanent embedded form factor suitable
for tool OEM integration — in a package small enough to be invisible inside a tool handle.

**The Breakthrough:** A dual-mode kinetic harvester delivers sufficient energy per
pick-up / set-down event (~180–400 μJ) to power a complete GPS fix and LTE-M burst
(~120–350 μJ including AGPS assist). Since a tool must be physically moved to be
used, and moving it generates the energy to report that movement, the tracking
system is self-funding. No movement = no report needed = no energy consumed.
The physics of the use case and the physics of the power source are identically aligned.

---

## Summary of Invention

The invention comprises the following components:

| Component | Material | Function |
|---|---|---|
| Housing shell (top + bottom) | Aluminium 6061-T6, hard anodised | EMI shielding, IP68 sealing, mechanical protection |
| 4-layer rigid PCB | FR4, ENIG finish, 18×18mm | Substrate for all active components |
| Piezoelectric disc array (×3) | PZT-5H, 12mm diameter × 0.5mm | Primary kinetic energy harvester |
| EM proof mass | NdFeB N52 magnet, 8mm sphere | Secondary kinetic harvester (EM induction) |
| EM coil | 28 AWG copper, 200 turns, 14mm OD | Secondary harvester pickup coil |
| Magnetic levitation guide | NdFeB ring magnets × 2 | Zero-friction proof mass suspension |
| PMIC | e-peas AEM10941 | AC rectification, MPPT, supercap charging |
| Supercapacitor bank | Ceramic 1 mF, 3.3V rated × 10 in parallel = 10 mF | Accumulated energy storage for GPS threshold; zero cycle degradation |
| MCU + Radio + GNSS + BLE | Nordic nRF9161-SICA SiP | BLE 5.3 per-event + GPS acquisition + LTE-M transmission — single chip |
| Flash memory | Winbond W25Q16 2MB SPI NOR | Deferred telemetry storage when no gateway or cellular coverage |
| Gasket seal | Silicone O-ring, Shore A 50 | IP68 environmental sealing |
| Mounting inserts | Stainless steel M3 threaded inserts × 4 | Tool attachment points |

---

## Detailed Description

### Physical Cross-Section

```
  TOP VIEW                          SIDE CROSS-SECTION (A-A')
  ┌─────────────────────────┐       ┌─────────────────────────────┐
  │ ●               ●       │       │ Al housing (top lid, 1mm)   │
  │                         │  A    ├─────────────────────────────┤
  │  ┌─────────────────┐    │  │    │ PCB (1.2mm, 4-layer FR4)    │
  │  │  nRF9161 SiP    │    │  │    │   nRF9161 │ AEM10941        │
  │  │  (GNSS+LTE-M)   │    │  │    ├─────────────────────────────┤
  │  ├─────────────────┤    │  │    │ PZT disc array (1.5mm)      │
  │  │  AEM10941 PMIC  │    │  │    │   3× PZT-5H 12mm discs      │
  │  ├─────────────────┤    │  │    ├─────────────────────────────┤
  │  │  Supercap 100μF │    │  │    │ EM harvester cavity (2.5mm) │
  │  ├─────────────────┤    │  A'   │   NdFeB sphere (8mm)        │
  │  │  PZT array (3×) │    │       │   magnetically levitated    │
  │  ├─────────────────┤    │       │   coil wound around cavity  │
  │  │  EM coil+mass   │    │       ├─────────────────────────────┤
  │  └─────────────────┘    │       │ Al housing (bottom, 1mm)    │
  │ ●               ●       │       │   M3 inserts at corners     │
  └─────────────────────────┘       └─────────────────────────────┘
     18mm × 18mm                            6mm total height
```

---

## Core Technology

### 1. Dual-Mode Kinetic Harvester Stack

The harvester stack is the defining innovation of The Cube. Two physically distinct
transduction mechanisms operate simultaneously, harvesting complementary frequency
ranges of tool motion:

#### 1a. Piezoelectric Array (PZT-5H)

Three PZT-5H discs (lead zirconate titanate, MPB composition) are stacked in a
parallel electrical configuration, mechanically bonded with conductive epoxy (Loctite
EA 9309.3NA) to a 14mm aluminium shim (Al 6061-T6, 0.3mm thick) acting as a
unimorph bender.

**Design rationale:** PZT-5H is the highest-d₃₃ commercially available soft PZT
composition. The unimorph configuration maximises bending strain from out-of-plane
acceleration (dominant during tool pick-up). Three discs in parallel triple the
output current without increasing voltage, matching the AEM10941 PMIC's input
impedance profile.

**PZT-5H Material Properties:**

| Property | Value | Unit |
|---|---|---|
| Piezoelectric coefficient d₃₃ | 593 | pC/N |
| Piezoelectric coefficient d₃₁ | −274 | pC/N |
| Coupling factor k₃₁ | 0.38 | — |
| Young's modulus E₁₁ | 61 | GPa |
| Poisson's ratio ν | 0.31 | — |
| Yield strength | 55 | MPa |
| Fracture toughness K_Ic | 0.9 | MPa√m |
| Hardness (Vickers) | 560 | HV |
| Density | 7800 | kg/m³ |
| Thermal conductivity | 1.5 | W/m·K |
| Specific heat capacity | 420 | J/kg·K |
| Thermal expansion coefficient | 4.5 | ppm/°C |
| Curie temperature | 193 | °C |
| Max operating temperature | 150 | °C |
| Dielectric constant ε₃₃ | 3800 | — |
| Dissipation factor tan δ | 0.020 | — |
| Fatigue life (bending, 50% Kic) | >10⁹ | cycles |

**Geometry:**

```
  Top shim (Al, 0.3mm)
  ├── PZT disc #1 (PZT-5H, 12mm Ø, 0.5mm)  — Ag electrode top/bottom
  ├── PZT disc #2 (PZT-5H, 12mm Ø, 0.5mm)  — parallel wired
  └── PZT disc #3 (PZT-5H, 12mm Ø, 0.5mm)  — parallel wired
       ↓ bonded with conductive epoxy (50μm bond line)
  Bottom fixed anchor (PCB copper land, 16mm Ø)
```

**Energy output model:**
```
V_oc = d₃₁ × E₁₁ × ε_bending / ε₃₃ × thickness
     ≈ 4.8 V peak at 1G, 50 Hz excitation

P_avg = (V_oc² / 4 × R_opt) × η_rectifier
      ≈ 38 μW at 1G continuous vibration
      ≈ 210 μJ per 0.5G shock event lasting 80ms (pick-up transient)
```

#### 1b. Electromagnetic Induction Harvester

An NdFeB N52 sphere (8mm diameter, 2.3g) is magnetically levitated inside a
14mm-diameter cylindrical cavity by two opposing NdFeB ring magnets (N42, 14mm OD,
10mm ID, 2mm thick) providing a bistable magnetic spring. A 200-turn copper coil
(28 AWG, 14mm OD wound on FR4 bobbin) surrounds the cavity.

**Design rationale:** The magnetically levitated proof mass has zero mechanical
friction — the only restoring force is magnetic. This eliminates bearing wear
entirely. The bistable magnetic spring reduces the resonant frequency to 8–15 Hz
(matching human walking / tool motion), maximising energy capture from low-frequency
gross motion (arm swing, tool transfer). The EM harvester is complementary to the
piezo array: PZT captures high-frequency shock (set-down impact), EM captures
low-frequency swing (carry motion).

**NdFeB N52 Proof Mass Properties:**

| Property | Value | Unit |
|---|---|---|
| Remanence B_r | 1.44–1.47 | T |
| Coercivity H_cB | ≥876 | kA/m |
| Max energy product (BH)_max | 398–422 | kJ/m³ |
| Young's modulus | 160 | GPa |
| Poisson's ratio | 0.24 | — |
| Yield strength | 80 | MPa |
| Density | 7500 | kg/m³ |
| Thermal conductivity | 9 | W/m·K |
| Specific heat | 460 | J/kg·K |
| Thermal expansion | 5.2 | ppm/°C |
| Curie temperature | 310–380 | °C |
| Max operating temperature | 80 | °C |
| Hardness (Vickers) | 570–640 | HV |
| Friction coefficient (none) | 0 | — (magnetically suspended) |

**Energy output model:**
```
EMF = N × dΦ/dt = N × B × A × (dx/dt) / l_coil
    ≈ 0.8 V peak at 0.3G, 10 Hz oscillation

E_harvest = ∫ P dt = ∫ (EMF² / 4R_coil) dt
          ≈ 85 μJ per 10 Hz, 0.3G, 500ms swing
```

**Total harvest per pick-up + carry + set-down event:**
- Piezo (set-down shock): ~150 μJ
- EM (carry swing): ~60–120 μJ
- **Total: ~210–270 μJ per event** (margin positive for AGPS-assisted GPS + LTE-M burst)

---

### 2. Power Management — e-peas AEM10941

The AEM10941 is a cold-start capable energy harvesting PMIC designed specifically
for AC transducer sources (piezo, EM, thermoelectric). Key functions:

- **Full-wave bridge rectifier** on transducer input — converts AC from both
  piezo and EM sources simultaneously
- **Maximum Power Point Tracking (MPPT)** — samples OCV at 1/8 period, sets
  impedance to maximise power transfer (±5% of MPP)
- **Supercapacitor charging** — charges 100 μF ceramic supercap with 85% efficiency
- **Threshold comparator** — fires VOUT_OK signal when V_supercap ≥ 3.0V
- **Cold-start** — operates from V_in as low as 380 mV — harvests first usable
  energy from sub-threshold transients before supercap is at working voltage

**Supercapacitor:** Murata DMT3R5V104M3DTA0  
- Capacitance: 100 μF  
- Rated voltage: 3.5V  
- ESR: 800 mΩ  
- Dimensions: 3.5mm diameter × 1.9mm height (1206 SMD equivalent)  
- Cycle life: **>500,000 charge-discharge cycles** (vs ~500 for Li-Ion)  
- No degradation mechanism analogous to battery capacity fade

---

### 3. MCU + Radio + GNSS — Nordic nRF9161 SiP

The nRF9161 integrates ARM Cortex-M33, LTE-M modem, NB-IoT modem, and GPS/GNSS
receiver into a single 10×16mm SiP. Selected for lowest active-mode energy budget
among integrated cellular+GNSS SiPs as of 2026.

**Key specifications:**

| Parameter | Value |
|---|---|
| Architecture | ARM Cortex-M33, 64 MHz |
| LTE-M TX power | +23 dBm |
| LTE-M RX sensitivity | −106 dBm |
| GNSS sensitivity (tracking) | −162 dBm |
| GNSS cold fix time | <30s (AGPS: <5s) |
| GNSS accuracy (open sky) | 1.8m CEP |
| Active current (LTE-M TX) | 220 mA peak, 22 mA average |
| Active current (GNSS) | 6 mA |
| Sleep current | 2.5 μA |
| Operating voltage | 3.0–5.5V |
| Operating temperature | −40°C to +85°C |
| Flash | 1 MB internal + W25Q16 external 2MB |
| MTBF | >500,000 hours |

**Active window per event:** ~1.8 seconds total
- GNSS AGPS fix: ~800 ms
- LTE-M connect + publish + disconnect: ~1,000 ms
- **Total energy consumed:** ~120–180 μJ (within harvest budget)

---

## Thermal Management

| Heat source | Max dissipation | Thermal path |
|---|---|---|
| nRF9161 (active) | 290 mW peak (TX) | Die → PCB copper → Al housing |
| AEM10941 PMIC | 12 mW | Die → PCB copper |
| Piezo array | Negligible | Via PCB ground plane |
| **Total** | **302 mW peak** | |

**Thermal path diagram:**
```
nRF9161 junction (max 85°C)
  → PCB copper spreading (4-layer, 2 oz Cu)
  → Thermal via array (36× 0.3mm vias)
  → Al bottom housing (1mm, k=167 W/m·K)
  → Tool body (ambient heat sink)
```

**Operating envelope:**

| Condition | Min | Max |
|---|---|---|
| Ambient temperature | −40°C | +85°C |
| Junction temperature (nRF9161) | — | 85°C |
| Piezo operating temperature | −40°C | 150°C |
| EM magnet operating temperature | −40°C | 80°C |

The EM magnet is the thermal constraint — NdFeB magnets begin to demagnetise above
80°C. The Al housing provides sufficient thermal mass to keep the magnet below this
limit during the 1.8-second active window even at +85°C ambient.

---

## Geometry and Mechanical Design

| Parameter | Value |
|---|---|
| Overall dimensions | 18.0 × 18.0 × 8.0 mm |
| PCB dimensions | 17.6 × 17.6 × 1.2 mm |
| Housing wall thickness | 1.0 mm (top and bottom) |
| Mounting: M3 insert PCD | 14.0 mm × 14.0 mm |
| Mounting: insert depth | 4.5 mm (through housing + PCB) |
| Embed pocket dimensions | 18.5 × 18.5 × 8.5 mm (0.5mm clearance) |
| Weight (assembled) | 5.1 g |
| GPS fire rule | Arrival only, after 500 ms continuous rest |
| Housing material | Al 6061-T6, hard anodised Type III |
| IP rating | IP68 (IEC 60529) — 1m, 30 min |
| Shock rating | 1500G, 0.5ms half-sine (MIL-STD-810H Method 516) |
| Vibration rating | 20G, 10–2000 Hz (MIL-STD-810H Method 514) |

**Al 6061-T6 Housing Material Properties:**

| Property | Value | Unit |
|---|---|---|
| Young's modulus | 68.9 | GPa |
| Poisson's ratio | 0.33 | — |
| Yield strength (0.2%) | 276 | MPa |
| Ultimate tensile strength | 310 | MPa |
| Fracture toughness K_Ic | 29 | MPa√m |
| Hardness (Brinell) | 95 | HB |
| Thermal conductivity | 167 | W/m·K |
| Specific heat | 896 | J/kg·K |
| Thermal expansion | 23.6 | ppm/°C |
| Melting point | 582–652 | °C |
| Density | 2700 | kg/m³ |
| Friction coefficient (anodised) | 0.10–0.15 | — |
| Restitution coefficient | 0.4 | — |

---

## Performance Specifications

| Parameter | Specified | SOTA Benchmark |
|---|---|---|
| GPS accuracy (open sky) | 1.8m CEP | 2.5m (Quectel L76K) |
| Indoor position accuracy (BLE fallback) | ±3–5m | ±3m (Apple UWB) |
| Energy per event (harvest) | 210–270 μJ | N/A (all competitors use batteries) |
| Energy per event (consume) | 120–180 μJ | N/A |
| Energy margin | 30–90 μJ positive | N/A |
| Min motion to harvest sufficient energy | 0.5G, 80ms | N/A |
| Active window per event | 1.8 s | 2.5 s (typical LTE-M trackers) |
| Sleep current | 2.5 μA | 5 μA (nRF9160) |
| Supercap charge cycles | >500,000 | >500,000 (ceramic) |
| Piezo fatigue life | >10⁹ cycles | >10⁹ PZT-5H published |
| Designed service life | Indefinite | 1–3 years (battery trackers) |
| Dimensions | 18×18×6mm | 35×35×8mm (smallest commercial GPS tracker) |
| Mass | 4.2 g | 12 g (smallest commercial GPS tracker) |
| IP rating | IP68 | IP67 (typical) |
| Operating temperature | −40 to +85°C | −20 to +70°C (typical) |

---

## Manufacturing Process

### Process Comparison

| Step | The Cube | Conventional battery tracker |
|---|---|---|
| PCB assembly | Standard SMT PCBA | Standard SMT PCBA |
| Harvester integration | Automated PZT bonding jig | Battery cell insertion + weld |
| Housing | Die-cast Al (50k+), CNC (1k) | Injection mould plastic |
| Sealing | O-ring gasket press | Ultrasonic plastic weld |
| Testing | Motion-triggered functional test | Battery charge + functional test |
| Firmware flash | 30 sec USB-C or RF programming | Same |
| **Battery logistics** | **None required** | Incoming inspection + storage |
| **End-of-life battery disposal** | **None required** | Hazmat recycling |

### Production Line Steps

1. PCB fabrication — 4-layer 18×18mm, ENIG, panelised 20-up
2. SMT placement — nRF9161, AEM10941, supercap, passives, flash
3. Reflow solder — lead-free SAC305, nitrogen atmosphere
4. Piezo bonding — automated dispense conductive epoxy, press-bond, UV cure
5. EM harvester assembly — magnet levitation calibration jig, coil insertion
6. PCB test — ICT (in-circuit test), 30 seconds/unit
7. Firmware flash — Nordic nRF9161 DFU over SWD, 15 seconds/unit
8. Housing assembly — PCB drop-in, O-ring seat, top lid press, torque M3 screws
9. IP68 test — 1m water immersion tank, 30 min, 100% of units
10. Functional test — motion stimulus jig (0.5G, 80ms shake), verify telemetry burst
11. MQTT provisioning test — verify LTE-M connection, GNSS fix acquired
12. Label + pack — UUID QR code label, ESD bag, box

### Production Targets

| Year | Units | Unit cost (BOM+assy) | Revenue target |
|---|---|---|---|
| Year 1 (pre-prod + pilot) | 5,000 | $9.20 | $195,000 |
| Year 3 (volume) | 50,000 | $5.80 | $1,450,000 |
| Year 5 (scale) | 250,000 | $4.10 | $5,125,000 |

---

## Claims

**Claim 1 (Independent):**  
A kinetic energy-harvesting wireless tracking module comprising: a piezoelectric
transducer array bonded in a unimorph configuration to a compliant metal shim; an
electromagnetic induction transducer comprising a magnetically levitated proof mass
within a coil winding, wherein the proof mass is suspended by repulsive magnetic
fields with zero mechanical contact; a power management integrated circuit configured
to rectify alternating current from both transducers simultaneously and charge a
ceramic supercapacitor; an integrated cellular and global navigation satellite system
(GNSS) transceiver configured to activate upon detection of sufficient stored charge
and transmit a position and event identifier to a remote server; wherein no primary
or secondary electrochemical battery is present; and wherein the module dimensions
do not exceed 20 × 20 × 8 mm.

**Claim 2 (dependent on 1):**  
The module of Claim 1, wherein the piezoelectric transducer array comprises three
or more PZT-5H discs connected electrically in parallel.

**Claim 3 (dependent on 1):**  
The module of Claim 1, wherein the magnetically levitated proof mass is a rare-earth
permanent magnet sphere suspended by opposing ring magnets providing a bistable
magnetic spring with resonant frequency between 5 and 20 Hz.

**Claim 4 (dependent on 1):**  
The module of Claim 1, wherein the energy storage element comprises a bank of ceramic
supercapacitors connected in parallel with a total capacitance between 1 and 100 mF
and a rated voltage between 3.0 and 5.0 V; wherein the power management integrated
circuit charges said bank to a first threshold voltage sufficient to activate a
Bluetooth Low Energy advertisement at every motion event; and wherein the transceiver
additionally activates a GNSS receiver and cellular modem only when the bank reaches
a second higher threshold voltage corresponding to accumulated energy sufficient for
a complete GNSS fix and cellular transmission.

**Claim 5 (dependent on 1):**  
The module of Claim 1, wherein the housing comprises aluminium alloy 6061-T6 with
Type III hard anodic coating providing an IP68 environmental rating per IEC 60529.

**Claim 6 (dependent on 1):**  
The module of Claim 1, wherein the transceiver is configured to classify each
activation event as either a departure event or an arrival event based on sequential
pairing with a prior event, and to transmit a binary-encoded advertisement packet
containing said classification, a bank voltage field, and an event sequence counter
at every event; wherein one or more infrastructure gateways at known positions
receive said packet and resolve an indoor spatial position of the module by RSSI
trilateration or Angle-of-Arrival measurement; and wherein the GNSS receiver is
activated exclusively on arrival events after a configurable stillness interval of
no less than 100 milliseconds, providing an outdoor position fallback when gateway
infrastructure is unavailable, such that every GNSS fix records the resting location
of the host object outside the primary gateway coverage area.

**Claim 7 (dependent on 6):**  
The module of Claim 6, wherein a remote server maintains a state machine that
transitions a tracked object from an at-rest state to an in-transit state upon
receiving a departure event, and from an in-transit state to an at-rest state upon
receiving an arrival event, and resolves the arrival position to a registered
storage container using spatial containment queries against a database of registered
container bounding volumes.

**Claim 8 (dependent on 7):**  
The module of Claim 7, wherein the remote server moves a file representing the
tracked object from a first directory representing a first storage container to a
second directory representing a second storage container upon resolving the arrival
position, such that the filesystem directory structure mirrors the physical spatial
location of the tracked object in real time.

**Claim 9 (dependent on 1):**  
The module of Claim 1, wherein the module includes four threaded mounting inserts
at corners on a 14 × 14 mm pitch, and wherein the housing defines a planar embed
face configured to be received in a complementary pocket of dimension 18.5 × 18.5 × 8.5 mm
in a host tool body and bonded with structural adhesive.

**Claim 10 (dependent on 1):**  
The module of Claim 1, wherein the module is configured to store up to 100 position
and event records in non-volatile flash memory when network connectivity is
unavailable and to transmit stored records upon next successful network connection.

**Claim 11 (composition):**  
A workshop tool comprising a tool body with an embedded pocket of dimension
substantially 18.5 × 18.5 × 8.5 mm, and a kinetic energy-harvesting tracking
module per Claim 1 bonded within said pocket with structural epoxy adhesive, wherein
no battery is present and the tracking capability is coextensive with the operational
life of the tool.

**Claim 12 (method):**  
A method of tracking a workshop tool comprising: harvesting kinetic energy from
physical motion of the tool into a supercapacitor bank; transmitting a Bluetooth Low
Energy advertisement at every motion event when the bank reaches a first charge
threshold; detecting cessation of motion and measuring a continuous stillness interval;
upon the stillness interval exceeding a rest confirmation threshold and the bank
reaching a second higher charge threshold, acquiring a GNSS position fix; transmitting
the GNSS position and event identifier over a cellular network to a message broker;
receiving the telemetry at a server; comparing the received position to bounding
volumes of registered storage containers; and moving a filesystem file representing
the tool into a directory representing the matched container, wherein the GNSS fix
records only the verified resting position of the tool.

---

## EustressEngine Simulation Requirements

See `EustressEngine_Requirements.md` for full ECS mapping.

**Component to ECS mapping summary:**

| Physical component | ECS entity / component |
|---|---|
| The Cube (assembled) | `ToolComponent` with `is_iot_tracked: true` |
| Departure telemetry event | `KineticPhase::Departed` transition |
| Arrival telemetry event | `KineticPhase::Arrived` transition |
| Container bounding volume | `BoundingVolume` on `StorageUnit` |
| Tool file location | `StorageManager::move_to_active_use()` / `return_from_active_use()` |
| 3D entity position | `Transform` updated by `sync_tool_transforms` system |
| Harvested energy (simulation) | `KineticEnergyState.harvested_uj` |
| Supercap voltage (simulation) | `KineticEnergyState.supercap_mv` |
