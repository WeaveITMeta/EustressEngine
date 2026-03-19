# Eustress Workshop Tools — JARVIS Workshop System

## Overview

The `eustress-workshop` crate implements a physical-digital twin workshop system.
Every registered tool is a `.tool.toml` file on disk. The file IS simultaneously:

- The tool's entity definition (spawns as an instanced mesh in the 3D Space)
- The Properties Panel schema (every field is an editable row)
- The AI knowledge source (injected into build guide prompts)
- The IoT binding (chip ID, MQTT topic, GPS config)

The file system IS the database. No migrations, no server, no schema upgrades.
The AI can create, edit, search, and delete `.tool.toml` files directly.

---

## Crate Location

```
eustress/crates/workshop/
├── Cargo.toml
└── src/
    ├── lib.rs           ← Crate root, re-exports
    ├── registry.rs      ← RegisteredTool, ToolIndex, ToolRegistry
    ├── storage.rs       ← StorageUnit, StorageManager, ContainerIndex, GPS file movement
    ├── status.rs        ← IoTTelemetry, LiveStatusStore, ToolStatus
    ├── guide.rs         ← BuildGuide, BuildStep, GuideResolution
    ├── procurement.rs   ← MissingItem, PurchaseList, Alexa + Amazon API
    ├── knowledge.rs     ← Embedded tool knowledge base, AI context builder
    └── twin.rs          ← WorkshopTwinPlugin — Bevy ECS digital twin
```

---

## File Layout (Workshop Project)

The **folder tree IS the physical layout of the workshop**.
Every folder is a storage container. Every `instance.toml` defines what type it is.
Tools sit inside their current container's folder. GPS moves the files automatically.

```
my-workshop/
├── .workshop/
│   └── workshop.toml              ← Workshop metadata, MQTT broker, Space origin GPS
├── tools/
│   ├── instance.toml              ← StorageKind::Zone — "Workshop Floor" (root)
│   ├── milwaukee-m18-drill.tool.toml  ← Currently on the workshop floor (root level)
│   │
│   ├── bench-3/
│   │   ├── instance.toml          ← StorageKind::Bench
│   │   ├── torque-wrench-3_8.tool.toml
│   │   ├── right-shelf/
│   │   │   ├── instance.toml      ← StorageKind::Shelf
│   │   │   └── caliper-150mm.tool.toml
│   │   └── left-drawer/
│   │       ├── instance.toml      ← StorageKind::Drawer
│   │       └── 5mm-drill-bit.tool.toml
│   │
│   ├── tool-cabinet-a/
│   │   ├── instance.toml          ← StorageKind::Cabinet
│   │   ├── bin-1/
│   │   │   ├── instance.toml      ← StorageKind::Bin
│   │   │   └── hex-bolt-m6.tool.toml
│   │   └── top-drawer/
│   │       ├── instance.toml      ← StorageKind::Drawer
│   │       └── combination-wrench-set.tool.toml
│   │
│   ├── cnc-bay/
│   │   ├── instance.toml          ← StorageKind::Zone
│   │   └── shopbot-cnc-router.tool.toml
│   │
│   └── movement_log.toml          ← Append-only audit trail of every tool movement
│
└── guides/
    └── aluminium-bracket-assembly.guide.toml
```

---

## Data Flow

```
Physical Workshop
  └── IoT GPS Chips (per tool)
        ├── MQTT broker  ──→  rumqttc subscriber
        └── HTTP REST    ──→  reqwest poller (fallback)
              └── IoTTelemetry payload
                    ├── LiveStatusStore (in-memory, keyed by tool UUID)
                    │     └── sync_tool_transforms (Bevy Update system)
                    │           └── 3D Space entity Transform updated live
                    └── BuildGuide resolver
                          └── "Grab the torque wrench — currently Bench 3, available"

.tool.toml files (file system)
  └── Rayon parallel scan at startup
        └── ToolIndex (in-memory HashMap + search indexes)
              ├── search_by_name("drill")
              ├── search_by_capability("drilling")
              ├── search_by_tag("cordless")
              └── available_tools()

AI (Claude via Workshop pipeline)
  └── ToolIndex.build_ai_context() + KnowledgeBase.build_context_block()
        └── BuildGuide generated with tool UUIDs + live locations
              └── GuideResolution
                    ├── ResolvedStep[] (rich instruction cards)
                    └── MissingRequirement[]
                          └── PurchaseList
                                ├── render_markdown() → shopping list doc
                                ├── render_alexa_payload() → Alexa Lists API
                                └── amazon_links() → Amazon search URLs
```

---

## `.tool.toml` Schema

Every field maps to a Properties Panel row. New fields can be added to `spec.extra`
without any code change — the Properties Panel renders all key-value pairs from `extra`.

```toml
id = "550e8400-e29b-41d4-a716-446655440000"
name = "Milwaukee M18 Drill"
description = "18V cordless drill/driver with 1/2\" chuck"
category = "power_tool"
home_location = "Bench 3, right shelf"
how_to_use = "Insert bit, set torque collar, squeeze trigger."
safety_notes = [
    "Always wear safety glasses.",
    "Remove battery before changing bits.",
]
tags = ["drill", "cordless", "18v", "milwaukee"]
available = true
registered_at = "2026-01-15T10:00:00Z"
updated_at = "2026-03-18T09:45:00Z"

[[capabilities]]
type = "drilling"
max_diameter_mm = 38

[[capabilities]]
type = "fastening"
max_torque_nm = 60

[spec]
manufacturer = "Milwaukee Tool"
model = "2803-20"
serial_number = "M18-2024-001"
power_source = "battery"
voltage = "18V"
weight_kg = 1.8
year_purchased = 2024
purchase_price_usd = 179.00
amazon_asin = "B09XXXXX"

[spec.extra]
battery_type = "M18 REDLITHIUM"
chuck_size = "1/2 inch"
max_rpm = "0-2000 RPM"
clutch_settings = "24 + drill"

[mesh]
mesh_path = "assets/models/tools/milwaukee-m18-drill.glb"
scale = 1.0
rotation_offset = [0.0, 0.0, 0.0]
cast_shadow = true

[iot]
chip_id = "chip-abc123"
mqtt_topic = "workshop/tools/chip-abc123/telemetry"
poll_interval_secs = 30
has_gps = true
has_status_sensor = true
```

---

## `.guide.toml` Schema

```toml
id = "7f3d9a12-1234-5678-abcd-000000000001"
title = "Aluminium Bracket Assembly Guide"
description = "Step-by-step guide to fabricate and assemble the mounting bracket."
skill_level = "intermediate"
total_estimated_minutes = 45
generated_at = "2026-03-18T10:00:00Z"
updated_at = "2026-03-18T10:00:00Z"

setup_notes = [
    "Clear Bench 3 before starting.",
    "Ensure coolant is available for the drill press.",
]

[[steps]]
index = 1
title = "Mark and centre-punch hole locations"
instruction = "Using the engineer's square, mark the four hole positions on the aluminium flat bar per the drawing. Use a centre punch and hammer to create a dimple at each mark — this prevents the drill bit from wandering."
estimated_minutes = 5
completed = false
safety_notes = []

  [[steps.requirements]]
  type = "tool"
  tool_id = "550e8400-e29b-41d4-a716-446655440001"
  tool_name = "Engineer's Square 300mm"
  required = true

  [[steps.requirements]]
  type = "material"
  name = "Aluminium flat bar 40×5mm"
  quantity = 1
  unit = "pcs"
  in_stock = true

[[steps]]
index = 2
title = "Drill pilot holes"
instruction = "Fit the 5mm HSS drill bit into the Milwaukee M18 Drill. Set the clutch to drill mode (full torque). Drill through each centre-punched mark, applying moderate forward pressure. Clear chips between holes."
estimated_minutes = 10
completed = false
safety_notes = [
    "Wear safety glasses — aluminium chips travel fast.",
    "Clamp the workpiece — never hold aluminium by hand while drilling.",
]

  [[steps.requirements]]
  type = "tool"
  tool_id = "550e8400-e29b-41d4-a716-446655440000"
  tool_name = "Milwaukee M18 Drill"
  required = true

  [[steps.requirements]]
  type = "material"
  name = "5mm HSS drill bit"
  quantity = 1
  unit = "pcs"
  in_stock = true
```

---

## Missing Items and Procurement

When a `BuildGuide` is resolved against the registry, any tool not registered
or any material not in stock becomes a `MissingRequirement`.

These are aggregated into a `PurchaseList` with three export formats:

### Markdown Shopping List
```markdown
# Aluminium Bracket Assembly Guide — Shopping List

*3 items needed — estimated total $47.50*

## Step 2: Drill pilot holes
[ ] 5mm HSS Cobalt Drill Bit Set — https://www.amazon.com/s?k=5mm+HSS+Cobalt+Drill+Bit&tag=eustress-20
[ ] Cutting fluid for aluminium — https://www.amazon.com/s?k=cutting+fluid+aluminium&tag=eustress-20

## Step 4: Fasten brackets
[ ] M5×20mm socket head cap screws (x8) — https://www.amazon.com/dp/B07XXXXX
```

### Alexa Lists API Payload
Sent to the user's Alexa shopping list via the Alexa List Management REST API.
Alexa can then read back the shopping list by voice: *"Alexa, what's on my shopping list?"*

### Amazon Search Links
Direct links per missing item — opened in the browser or sent as notifications.

---

## Bevy Digital Twin Integration

The `WorkshopTwinPlugin` (behind the `bevy-twin` feature flag) integrates the
workshop into a 3D Eustress Space:

| System                    | Schedule  | What it does                                              |
|---------------------------|-----------|-----------------------------------------------------------|
| `spawn_tool_entities`     | Startup   | Spawns a mesh entity for each `.tool.toml`                |
| `spawn_tool_entities`     | Update    | Re-spawns when `WorkshopTwinState.dirty = true`           |
| `despawn_removed_tools`   | Update    | Removes entities for deleted `.tool.toml` files           |
| `sync_tool_transforms`    | Update    | Moves entities to live GPS position from IoT telemetry    |
| `sync_tool_availability`  | Update    | Adds `ToolUnavailable` marker for visual highlighting     |

### Properties Panel Integration

The `ToolComponent` on each entity links back to the `.tool.toml` file.
The Properties Panel can read and write all fields in `RegisteredTool`:
- Basic fields: name, description, home_location, available
- `spec.extra` fields: fully dynamic — add any key-value pair, panel renders it
- IoT config: chip_id, MQTT topic, GPS settings
- Mesh config: mesh path, scale, rotation offset

---

## AI Integration

The AI (Claude via the Workshop `IdeationPipeline`) uses two context sources:

1. **`ToolIndex.build_ai_context()`** — all registered tools with live locations
2. **`KnowledgeBase.build_context_block()`** — embedded know-how for 10 tool families

The `build_guide_system_prompt()` function in `guide.rs` combines these into a
system prompt that instructs Claude to:
- Reference registered tool UUIDs in every step that requires a tool
- Flag unregistered tools as missing (triggering procurement)
- Include accurate how-to instructions from the knowledge base
- Add safety notes for any step involving cutting, drilling, or welding

### Adding Custom Tool Knowledge

To teach the AI about a custom tool type not in the embedded knowledge base,
add a `how_to_use` and `safety_notes` array to the `.tool.toml` file.
These fields override the embedded knowledge base entries for that specific tool.

---

## IoT Chip Architecture

```
Physical Tool
  └── GPS Chip (e.g. Quectel L76K + ESP32)
        └── Firmware broadcasts JSON telemetry every 30s:
              {
                "chip_id": "chip-abc123",
                "lat": 40.7128,
                "lon": -74.0060,
                "state": "available",
                "battery_pct": 87,
                "rssi_dbm": -65
              }

Engine
  └── MQTT Subscriber (rumqttc) subscribes to "workshop/tools/+/telemetry"
        └── Incoming message
              └── chip_to_tool lookup in LiveStatusStore
                    └── IoTTelemetry deserialized and stored
                          └── GPS lat/lon → space_position [x, y, z] conversion
                                └── sync_tool_transforms moves entity in 3D Space
```

### GPS to 3D Space Conversion

GPS coordinates are converted to local Space coordinates using the same
pipeline as `eustress-geo`:
- WGS84 (lat/lon) → UTM (metres, east/north)
- UTM → Space local coordinates (relative to workshop origin defined in `workshop.toml`)

---

## Feature Flags

| Flag          | What it enables                                        | Default |
|---------------|--------------------------------------------------------|---------|
| `iot-mqtt`    | MQTT subscriber for real-time GPS telemetry            | on      |
| `iot-http`    | HTTP polling fallback for REST-based IoT endpoints     | off     |
| `procurement` | Amazon PA-API + Alexa Lists API integration            | on      |
| `bevy-twin`   | Bevy ECS plugin, ToolComponent, digital twin systems   | off     |
| `full`        | All features enabled                                   | off     |

---

## Implementation Phases

### Phase 1 — Foundation (Complete)
- [x] `registry.rs` — TOML-backed ToolRegistry, ToolIndex, parallel scan
- [x] `status.rs` — IoTTelemetry, LiveStatusStore, ToolStatus
- [x] `guide.rs` — BuildGuide, BuildStep, GuideResolution, missing items
- [x] `procurement.rs` — PurchaseList, Amazon links, Alexa payload
- [x] `knowledge.rs` — Embedded knowledge base (10 tool families)
- [x] `twin.rs` — WorkshopTwinPlugin, ToolComponent, live transform sync
- [x] Workspace registration in `eustress/Cargo.toml`

### Phase 2 — Engine Integration
- [ ] Add `eustress-workshop` dependency to `eustress-engine/Cargo.toml`
- [ ] Register `WorkshopTwinPlugin` in the engine's plugin stack
- [ ] Add workshop tool registration to the Slint Properties Panel
- [ ] Wire `IdeationPipeline` to inject `ToolIndex.build_ai_context()` into guide prompts
- [ ] Add workshop tab to the ribbon for guide generation

### Phase 3 — IoT Live Telemetry
- [ ] Implement MQTT subscriber using `rumqttc` async client
- [ ] Implement HTTP polling fallback with `reqwest`
- [ ] GPS → Space coordinate conversion using `eustress-geo` pipeline
- [ ] Eustress Parameters adapter for real-time tool location streaming

### Phase 4 — Procurement Pipeline
- [ ] Implement Amazon PA-API v5 signed request (HMAC-SHA256 SigV4)
- [ ] Implement Alexa Lists REST API OAuth flow (Login with Amazon)
- [ ] Add "Order Missing Items" button to the Workshop build guide panel
- [ ] Workshop panel: missing items list with Amazon links and Alexa send button

### Phase 5 — 3D Digital Twin
- [ ] Tool `.glb` mesh library (generic shapes for each ToolCategory)
- [ ] Workshop Space template (benches, shelves, zones as Eustress scene)
- [ ] Gizmo overlay for tool location labels in the 3D viewport
- [ ] Eustress Parameters live dashboard widget for tool status
