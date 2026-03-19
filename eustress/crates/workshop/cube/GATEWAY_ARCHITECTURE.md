# Voltec Gateway — Architecture and Deployment Guide

## What the Gateway Does

The Voltec Gateway is a fixed BLE scanning node. It passively receives every
`CubePacket` advertisement broadcast by any Cube in range and publishes the raw
packet plus its own RSSI measurement and AoA angle to the local MQTT broker.

The Eustress Workshop server fuses simultaneous readings from all visible gateways
to resolve each tool's indoor position. **The Cube is the beacon. The gateway is
the sensor.**

---

## Node Count — Minimum Requirements

| Nodes deployed | What you get | Accuracy |
|---|---|---|
| **1 node** | Zone / room only — "near Gateway 1" | ±5–15m (zone label) |
| **2 nodes** | Ambiguous 2D arc intersection — usable with strong priors | ±2–4m |
| **3 nodes (minimum recommended)** | True trilateration, unambiguous 2D fix | ±0.3–1m (AoA) |
| **4+ nodes** | Overdetermined, handles occlusion and multipath | ±0.2–0.5m |

**AoA specifics:** A single AoA node yields an azimuth + elevation angle vector
(a direction, not a distance). Two AoA nodes intersect two angle vectors for an
exact point fix. Three is robust against occlusion from tools on shelves.

**Rule of thumb:** 1 gateway per ~80–100 m² of workshop floor area, 3-node minimum
for any triangulation. A typical single-bay workshop (10 × 20m = 200 m²) needs
3 gateways placed at ceiling height on the perimeter.

---

## Voltec Gateway — First-Party Hardware

The **Voltec Gateway G1** is a branded fixed-install node built on the
Nordic nRF21540 front-end + directional antenna array, AoA-capable out of the box.

```
┌─────────────────────────────────────────────┐
│              VOLTEC GATEWAY G1              │
│                                             │
│   ┌─────────────────────────────────────┐   │
│   │  Nordic nRF21540 RF Front End       │   │
│   │  +20 dBm TX / +13 dBm LNA          │   │
│   │  AoA Direction Finding engine      │   │
│   └─────────────────────────────────────┘   │
│                                             │
│   ┌──────────────┐   ┌─────────────────┐   │
│   │  4-element   │   │  Raspberry Pi   │   │
│   │  patch       │   │  Compute Module │   │
│   │  antenna     │   │  (gateway SW)   │   │
│   │  array       │   │                 │   │
│   └──────────────┘   └─────────────────┘   │
│                                             │
│   PoE (802.3af) — single cable install      │
│   RJ45 + optional Wi-Fi backhaul           │
│   Magnetic ceiling mount + 1/4" thread     │
│   Dimensions: 120 × 80 × 25mm             │
│   Housing: Anodised Al (matches The Cube)  │
└─────────────────────────────────────────────┘
```

### Why nRF21540 + Antenna Array

| Feature | Value |
|---|---|
| AoA azimuth accuracy | ±1–3° → ±0.3–1m at 10m range |
| BLE 5.1 Direction Finding | Built into nRF21540 hardware |
| RF range (indoor) | 40–80m clear line-of-sight |
| Simultaneous Cube tracking | Unlimited (passive scanner) |
| Latency per fix | <500ms (advertising interval limited) |

The nRF21540 is a pure RF front-end — it amplifies and demodulates, feeding IQ
samples to the Compute Module which runs the AoA algorithm and MQTT publisher.

### Pricing Strategy

| SKU | Contents | Cost to build | Retail |
|---|---|---|---|
| **Voltec Gateway G1 (single)** | 1× G1 node, PoE injector, ceiling mount | ~$95 | **$149** |
| **Voltec Gateway Starter Kit** | 3× G1 nodes, PoE switch, setup guide | ~$285 | **$399** |
| **Voltec Gateway Pro Kit** | 5× G1 nodes, managed PoE switch, calibration wand | ~$490 | **$699** |

15–25% premium over raw parts cost is absorbed by:
- Voltec branding and anodised Al housing matching The Cube aesthetic
- Pre-flashed gateway firmware, zero-configuration pairing with Eustress
- Included calibration workflow in the Eustress Studio app
- 3-year warranty and firmware update support

**The Cube retail:** $39–$59 per unit (IP68, kinetic, indefinite life justifies
premium over Tile Pro $35 / AirTag $29 consumer anchors).

---

## Gateway Placement Guidelines

```
Workshop floor plan (10 × 20m example):

   ┌──────────────────────────────────────┐
   │                                      │
   │  GW-01 ●                   ● GW-03  │
   │  (ceiling corner)    (ceiling corner)│
   │                                      │
   │          ░░░░░░░░░░░░░░░             │
   │          ░  WORKBENCHES  ░           │
   │          ░░░░░░░░░░░░░░░             │
   │                                      │
   │              ● GW-02                 │
   │         (ceiling, far wall centre)   │
   │                                      │
   └──────────────────────────────────────┘

Coverage: 3 nodes, ~80m² each = full 200m² coverage
Overlap zone (all 3 visible): workbench area → best accuracy
```

**Placement rules:**
- Mount at ceiling height (2.5–4m) — above shelf occlusion
- Space nodes so every point in the workshop is visible to ≥2 nodes
- Avoid placement directly above metal shelving (reflections degrade AoA)
- Point antenna arrays toward the workshop centre, not walls
- After physical install, run the **Eustress Calibration Wand** procedure (walk
  a Cube around the space at known grid points — takes ~10 minutes)

---

## Phone as Stationary Gateway Node

The Voltec Workshop App (iOS + Android) supports a **Stationary Gateway Mode**:

### How it works

1. User opens the app → Workshop → **Use Phone as Gateway**
2. App prompts: *"Place your phone face-up on a flat surface and do not move it"*
3. User places the phone, taps **Calibrate Position**
4. App uses the workshop map to confirm the phone's position (user taps their
   location on the floor plan, or scans a QR marker on the wall)
5. Phone enters passive BLE scanner mode — receives all Cube advertisements,
   publishes RSSI + position to the local MQTT broker
6. Phone's accelerometer monitors for movement; if the phone is picked up or
   moved, it **immediately removes itself from the gateway pool** and notifies
   the server: `{"gateway_id": "phone-xxx", "status": "offline", "reason": "moved"}`
7. Server drops the phone's contribution from the fusion calculation until it
   re-calibrates

### Accuracy contribution

| Phone mode | Accuracy contribution |
|---|---|
| Stationary, calibrated | Same as a fixed RSSI node: ±2–5m |
| Moving / uncalibrated | **Excluded from calculation** |
| AoA (future, requires phone with BLE 5.1 antenna array) | ±0.5–1m |

The phone acts as an **opportunistic supplemental node**, not a replacement for
fixed gateways. It is most valuable in workshops that only have 2 fixed nodes —
the phone provides the third point needed for a valid trilateration fix.

### ECS representation

The phone gateway is registered as a `StorageUnit`-adjacent `GatewayNode` resource
with a `calibrated: bool` flag and `last_seen_position: Vec3`. When `calibrated`
is false, the node's RSSI readings are excluded from `ContainerIndex::fuse_rssi()`.

---

## MQTT Gateway Envelope

Each gateway publishes two message types:

### 1. Raw scan observation (per received advertisement)

Topic: `workshop/gateway/{gateway_id}/scan`

```json
{
  "gateway_id": "gw-01",
  "gateway_position": [1.2, 0.5, 3.1],
  "chip_id_short": "a1b2c3",
  "rssi_dbm": -68,
  "aoa_azimuth_deg": 34.2,
  "aoa_elevation_deg": -12.1,
  "aoa_valid": true,
  "event_seq": 1042,
  "captured_at": "2026-03-18T14:30:00.123Z"
}
```

`aoa_valid` is false if fewer than 2 antenna elements received the packet cleanly.
`aoa_azimuth_deg` and `aoa_elevation_deg` are only used when `aoa_valid` is true.

### 2. Fused position (published by Eustress server after multi-gateway fusion)

Topic: `workshop/tools/{chip_id}/telemetry`

```json
{
  "chip_id": "cube-a1b2c3d4e5f6",
  "tool_id": "550e8400-e29b-41d4-a716-446655440000",
  "event": "arrival",
  "space_position": [4.2, 1.1, 0.9],
  "position_method": "aoa_trilateration",
  "position_accuracy_m": 0.4,
  "zone_label": "Bench 3",
  "contributing_gateways": ["gw-01", "gw-02", "gw-03"],
  "bank_mv": 3140,
  "gps_fired": false,
  "harvested_uj": 216,
  "temp_c": 22.0,
  "captured_at": "2026-03-18T14:30:00.234Z"
}
```

`position_method` is one of:
- `aoa_trilateration` — ≥2 AoA readings fused (most accurate)
- `rssi_trilateration` — ≥3 RSSI readings fused
- `rssi_bilateration` — 2 RSSI readings (reduced accuracy, flagged)
- `zone_only` — 1 gateway visible (zone label only, no coordinates)
- `gps_outdoor` — tool outside workshop, GPS fix used

---

## Calibration Procedure

### Initial setup (Eustress Studio)

1. Mount all gateways, connect PoE, confirm they appear in Studio → Workshop →
   Gateways panel (auto-discovered via mDNS)
2. Open the floor plan editor — draw the workshop outline and mark each gateway's
   ceiling position
3. Click **Start Calibration Wand**
4. Walk through the workshop holding a registered Cube at chest height, pausing
   3 seconds at each marked grid intersection (1m grid recommended)
5. Studio records RSSI + AoA observations at each known ground-truth position and
   fits a path-loss model + AoA offset correction per gateway
6. Click **Finish Calibration** — Studio writes `workshop.toml` with the fitted model
7. Estimated time: ~10 minutes for a 200 m² workshop

### Re-calibration triggers
- Any gateway is moved
- Major new shelving or equipment added to the workshop
- Accuracy degrades noticeably (Studio flags this automatically when GPS outdoor
  fixes disagree with the last known indoor position by >3m)

---

## System Architecture Summary

```
                    VOLTEC GATEWAY NETWORK
   ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
   │  G1-01   │    │  G1-02   │    │  G1-03   │    │  Phone   │
   │  (fixed) │    │  (fixed) │    │  (fixed) │    │(optional)│
   │  AoA     │    │  AoA     │    │  AoA     │    │  RSSI    │
   └────┬─────┘    └────┬─────┘    └────┬─────┘    └────┬─────┘
        │               │               │               │
        └───────────────┴───────────────┴───────────────┘
                                │ LAN / PoE switch
                                ▼
                   ┌────────────────────────┐
                   │   Local MQTT Broker    │
                   │   (Mosquitto on Pi /   │
                   │    Eustress server)    │
                   └────────────┬───────────┘
                                │
                                ▼
                   ┌────────────────────────┐
                   │   Eustress Workshop    │
                   │   Position Fusion      │
                   │   ContainerIndex       │
                   │   StorageManager       │
                   │   LiveStatusStore      │
                   └────────────────────────┘
                                │
                                ▼
                   Tool files move between
                   container folders in
                   real time as tools move
```

See `src/storage.rs`, `src/status.rs` for the Rust implementation.  
See `cube/EustressEngine_Requirements.md` for ECS simulation mapping.
