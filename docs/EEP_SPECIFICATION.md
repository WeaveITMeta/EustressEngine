# Eustress Export Protocol (EEP) v3.0 Specification

**Version**: 3.0  
**Date**: 2026-07-12 (§16 added; other sections unchanged from v2.0, March 2, 2026)  
**Status**: File-format spec (§1–§15) Active — implemented, referenced by real engine code · §16 AI Training Dataset Export is a Draft Proposal, not yet implemented (see status box in §16)

## Overview

The Eustress Export Protocol (EEP) is a **file-system-first** protocol for serializing all Eustress class instances to human-readable TOML files. AI models access project data directly via MCP file APIs — no proprietary database, no binary project files.

As of v3.0, EEP also specifies an optional, higher-level capability — exporting *converged simulation trajectories* as AI training datasets (§16) — built on top of the same TOML + MCP substrate defined elsewhere in this document. The per-instance TOML schema is unchanged; **`eep_version` in `project.toml` stays `"2.0"`** until §16 actually ships (see [Versioning](#versioning)). v3.0 is a document version, not yet a shipped format version.

### Key Changes from v1.0 → v2.0

| v1.0 | v2.0 |
|------|------|
| JSON export records pushed to MCP | MCP reads TOML files directly from filesystem |
| Only `glb.toml` for BaseParts | TOML schemas for **all 60+ class types** |
| Binary scene format | glTF 2.0 + EXT_eustress (JSON, git-diffable) |
| Push-based export | Pull-based file access |

### What v3.0 Adds

A new §16, **AI Training Dataset Export**, specifying how *converged simulation trajectories* (not just static instance state) can be packaged as Parquet/Hugging Face datasets for training external AI models — a different concern from this document's per-instance TOML format, layered on top of it. See the status box at the start of §16 before treating any of it as current behavior.

---

## Table of Contents

1. [Core Principles](#core-principles)
2. [Architecture](#architecture)
3. [File Naming Conventions](#file-naming-conventions)
4. [TOML Schema Definitions](#toml-schema-definitions)
   - [Common Fields](#common-fields-all-classes)
   - [3D Object Classes](#3d-object-classes) ← Part covers primitives + mesh refs
   - [ChunkedWorld](#chunkedworld-_instancetoml--manifesttoml--chunksechk) ← 10M+ instances
   - [Lighting Classes](#lighting-classes)
   - [Character Classes](#character-classes)
   - [Camera Classes](#camera-classes)
   - [Seat Classes](#seat-classes)
   - [GUI Classes](#gui-classes)
   - [Effect Classes](#effect-classes)
   - [Service Classes](#service-classes)
   - [Orbital Classes](#orbital-classes)
5. [MCP File Access API](#mcp-file-access-api)
6. [Binary and Cache Handling](#binary-and-cache-handling)
7. [Property Serialization Rules](#property-serialization-rules)
8. [Hierarchy Representation](#hierarchy-representation)
9. [Parameters Architecture](#parameters-architecture)
10. [Export and Access Patterns](#export-and-access-patterns)
11. [Consent Model](#consent-model)
12. [Error Handling](#error-handling)
13. [Versioning](#versioning)
14. [Security Considerations](#security-considerations)
15. [Implementation Checklist](#implementation-checklist)
16. [AI Training Dataset Export](#ai-training-dataset-export) ← v3.0, Draft Proposal, not yet implemented
17. [References](#references)
18. [Changelog](#changelog)

---

## Core Principles

| Principle | Description |
|-----------|-------------|
| **File-System-First** | Every instance is a `.toml` file on disk — the path IS the identifier |
| **All Classes Serialized** | Not just BaseParts — Lights, Cameras, Scripts, GUI, Services, everything |
| **Human-Readable** | TOML format with clear property names, git-diffable |
| **MCP File Access** | AI models read files via MCP tools, no special export step |
| **Consented** | Opt-in via `ai = true` flag per instance |
| **Hierarchical** | Parent/child structure preserved via folder hierarchy + `parent` field |

## Architecture

```
Eustress Project (Folder)
        │
        ├── .eustress/
        │   ├── project.toml          ← Project metadata
        │   ├── settings.toml         ← Editor preferences
        │   └── cache/                ← Derived assets (gitignored)
        │
        ├── Workspace/                ← Service folder
        │   ├── Baseplate.part.toml   ← Part instance
        │   ├── Camera.camera.toml    ← Camera instance
        │   └── Player/               ← Model folder (hierarchy)
        │       ├── _model.toml       ← Model properties
        │       ├── HumanoidRootPart.part.toml
        │       └── Humanoid.humanoid.toml
        │
        ├── Lighting/                 ← Service folder
        │   ├── _service.toml         ← Lighting service properties
        │   ├── Sun.sun.toml          ← Sun instance
        │   └── Atmosphere.atmosphere.toml
        │
        ├── src/                      ← Soul scripts
        │   └── main.soul
        │
        └── assets/                   ← Raw assets (meshes, textures)
            └── meshes/
                └── Character.glb

        ↓ MCP File Access API ↓

AI Model reads files via:
  → mcp0_read_text_file(path)
  → mcp0_directory_tree(path)
  → mcp0_search_files(path, pattern)
```

## File Naming Conventions

Every Eustress class instance is stored as a TOML file with a class-specific extension.

### Extension Format

```
{InstanceName}.{class}.toml
```

### Consolidation Rule: Part is the Universal 3D Object

`Part` is the single file type for all solid geometry. `MeshPart`, `UnionOperation`, and `SpecialMesh` **do not exist as separate file types** — they are all `Part` with an optional `mesh` field:

```
mesh absent  → procedural primitive (Block, Ball, Cylinder, Wedge)
mesh present → mesh reference (formerly MeshPart / Union / SpecialMesh)
```

`BasePart` and `PVInstance` are **internal abstract base classes only** — they are never written to disk as files.

### Class Extension Mapping

| Class | Extension | Example | Notes |
|-------|-----------|---------|-------|
| **3D Objects** | | | |
| Part | `.part.toml` | `Baseplate.part.toml` | Covers primitives AND mesh parts |
| Seat | `.seat.toml` | `Chair.seat.toml` | Extends Part; auto-sit trigger |
| VehicleSeat | `.vehicleseat.toml` | `DriverSeat.vehicleseat.toml` | Extends Part; throttle/steer input |
| SpawnLocation | `.spawn.toml` | `SpawnPoint.spawn.toml` | Extends Part; respawn hook |
| Terrain | `.terrain.toml` | `World.terrain.toml` | Voxel heightmap; unique data |
| ChunkedWorld | `_instance.toml` + `manifest.toml` + `chunks/*.echk` | `World/_instance.toml` | 10M+ instances; see [CHUNKED_STORAGE.md](development/CHUNKED_STORAGE.md) |
| **Containers** | | | |
| Model | `_instance.toml` (in folder) | `Player/_instance.toml` | Folder = children |
| Folder | `_instance.toml` (in folder) | `Props/_instance.toml` | Folder = children |
| **Lighting** | | | |
| PointLight | `.pointlight.toml` | `Lamp.pointlight.toml` | |
| SpotLight | `.spotlight.toml` | `Flashlight.spotlight.toml` | |
| SurfaceLight | `.surfacelight.toml` | `Panel.surfacelight.toml` | |
| DirectionalLight | `.dirlight.toml` | `Sun.dirlight.toml` | |
| Star | `.star.toml` | `DaySun.star.toml` | |
| Moon | `.moon.toml` | `NightMoon.moon.toml` | |
| Atmosphere | `.atmosphere.toml` | `Sky.atmosphere.toml` | |
| Clouds | `.clouds.toml` | `Weather.clouds.toml` | |
| Sky | `.sky.toml` | `Skybox.sky.toml` | |
| **Characters** | | | |
| Humanoid | `.humanoid.toml` | `Controller.humanoid.toml` | |
| Animator | `.animator.toml` | `Anim.animator.toml` | |
| **Cameras** | | | |
| Camera | `.camera.toml` | `MainCamera.camera.toml` | |
| **Constraints** | | | |
| WeldConstraint | `.weld.toml` | `Joint1.weld.toml` | Uses `parent` field |
| Motor6D | `.motor6d.toml` | `Shoulder.motor6d.toml` | Uses `parent` field |
| Attachment | `.attachment.toml` | `GripPoint.attachment.toml` | Uses `parent` field |
| **GUI** | | | |
| ScreenGui | `.screengui.toml` | `HUD.screengui.toml` | |
| BillboardGui | `.billboardgui.toml` | `NameTag.billboardgui.toml` | |
| SurfaceGui | `.surfacegui.toml` | `Display.surfacegui.toml` | |
| Frame | `.frame.toml` | `Container.frame.toml` | |
| TextLabel | `.textlabel.toml` | `Title.textlabel.toml` | |
| TextButton | `.textbutton.toml` | `PlayBtn.textbutton.toml` | |
| TextBox | `.textbox.toml` | `Input.textbox.toml` | |
| ImageLabel | `.imagelabel.toml` | `Icon.imagelabel.toml` | |
| ImageButton | `.imagebutton.toml` | `MenuBtn.imagebutton.toml` | |
| ScrollingFrame | `.scrollframe.toml` | `List.scrollframe.toml` | |
| ViewportFrame | `.viewportframe.toml` | `Preview.viewportframe.toml` | |
| VideoFrame | `.videoframe.toml` | `Player.videoframe.toml` | |
| DocumentFrame | `.docframe.toml` | `Manual.docframe.toml` | |
| WebFrame | `.webframe.toml` | `Browser.webframe.toml` | |
| **Effects** | | | |
| ParticleEmitter | `.particles.toml` | `Fire.particles.toml` | Uses `parent` field |
| Beam | `.beam.toml` | `Laser.beam.toml` | Uses `parent` field |
| Decal | `.decal.toml` | `Logo.decal.toml` | Uses `parent` field |
| **Audio** | | | |
| Sound | `.sound.toml` | `Music.sound.toml` | Uses `parent` field |
| **Scripts** | | | |
| SoulScript | `.soul` | `main.soul` | Source file, not TOML |
| **Services** | | | |
| Workspace | `_service.toml` | `Workspace/_service.toml` | |
| Lighting | `_service.toml` | `Lighting/_service.toml` | |
| **Assets** | | | |
| Document | `.document.toml` | `Manual.document.toml` | |
| ImageAsset | `.imageasset.toml` | `Logo.imageasset.toml` | |
| VideoAsset | `.videoasset.toml` | `Intro.videoasset.toml` | |
| Team | `.team.toml` | `RedTeam.team.toml` | |
| **Orbital** | | | |
| SolarSystem | `.solarsystem.toml` | `Sol.solarsystem.toml` | |
| CelestialBody | `.celestial.toml` | `Earth.celestial.toml` | |
| RegionChunk | `.regionchunk.toml` | `Chunk_0_0.regionchunk.toml` | |

---

## TOML Schema Definitions

### Common Fields (All Classes)

Every `.toml` file includes these base fields:

```toml
[instance]
name = "MyInstance"           # Display name
class_name = "Part"           # ClassName enum value
archivable = true             # Save eligibility
ai = false                    # AI training opt-in flag

[metadata]
id = "a1b2c3d4"               # Unique instance ID (auto-generated)
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
creator = "user-uuid"         # Creator ID

[tags]
values = ["prop", "static"]   # String tags for categorization

[attributes]
# Custom key-value pairs (user-defined)
custom_property = "value"
damage_multiplier = 1.5
```

---

### 3D Object Classes

#### Part (`.part.toml`)

`Part` is the universal 3D solid object. The `[asset]` section is **optional** — its presence determines the geometry mode:

**Mode 1 — Procedural Primitive** (no `[asset]` section):

```toml
# Workspace/Baseplate.part.toml
[instance]
name = "Baseplate"
class_name = "Part"
archivable = true
ai = false

[transform]
position = [0.0, -0.5, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]  # Quaternion (x, y, z, w)
scale = [1.0, 1.0, 1.0]

[geometry]
size = [100.0, 1.0, 100.0]      # Bounding dimensions in meters
shape = "Block"                  # Block | Ball | Cylinder | Wedge | CornerWedge | Cone

[appearance]
color = [0.388, 0.373, 0.384, 1.0]
material = "SmoothPlastic"
transparency = 0.0
reflectance = 0.0

[physics]
anchored = true
can_collide = true
can_touch = true
density = 900.0
friction = 0.5
elasticity = 0.3
collision_group = "Default"

[rendering]
cast_shadow = true
locked = false

[metadata]
id = "baseplate-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

**Mode 2 — Mesh Reference** (`[asset]` section present; replaces MeshPart / UnionOperation / SpecialMesh):

```toml
# Workspace/Props/Chair.part.toml
[instance]
name = "Chair"
class_name = "Part"
archivable = true
ai = true

[transform]
position = [3.0, 0.0, 1.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [1.0, 1.0, 1.0]

[asset]
mesh = "assets/meshes/Chair.glb"  # Path to binary mesh (relative to project root)
scene = "Scene0"                  # glTF scene name inside the .glb

[geometry]
size = [1.2, 1.1, 1.2]           # Bounding box (for physics/collision, not render)

[appearance]
color = [1.0, 1.0, 1.0, 1.0]    # Tint applied over mesh materials
material = "Wood"
transparency = 0.0

[physics]
anchored = false
can_collide = true
can_touch = true
density = 600.0
friction = 0.4
elasticity = 0.1
collision_group = "Default"

[rendering]
cast_shadow = true
locked = false

[metadata]
id = "chair-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

#### ChunkedWorld (`_instance.toml` + `manifest.toml` + `chunks/*.echk`)

For large-scale worlds with 10K+ instances, use `ChunkedWorld` instead of individual TOML files. See [CHUNKED_STORAGE.md](development/CHUNKED_STORAGE.md) for the full binary chunk format.

```toml
# Workspace/World/_instance.toml
[instance]
name = "World"
class_name = "ChunkedWorld"
archivable = true
ai = false

[chunked_world]
chunk_size = [256.0, 256.0, 256.0]   # Meters per chunk
min_chunk = [-64, -4, -64]            # World bounds (chunk coords)
max_chunk = [63, 3, 63]
load_radius = 3                       # Chunks to load around camera
unload_radius = 5                     # Hysteresis for unloading
lod_distances = [256.0, 512.0, 1024.0]
compression = "lz4"                   # none | lz4 | zstd

[metadata]
id = "world-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

The `manifest.toml` and binary `.echk` chunk files are documented in [CHUNKED_STORAGE.md](development/CHUNKED_STORAGE.md).

---

#### Model / Folder (`_instance.toml` in folder)

Both `Model` and `Folder` use the same folder-marker file `_instance.toml`. The `class_name` field distinguishes them. The folder on disk IS the container — its children are the sibling files and subfolders within it.

```toml
# Workspace/Player/_instance.toml
[instance]
name = "Player"
class_name = "Model"           # Model | Folder
archivable = true
ai = false

[model]
primary_part = "HumanoidRootPart"  # Name of the pivot Part (Model only)
world_pivot = [0.0, 0.0, 0.0]

[metadata]
id = "player-model-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

```toml
# Workspace/Props/_instance.toml
[instance]
name = "Props"
class_name = "Folder"
archivable = true
ai = false

[metadata]
id = "props-folder-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

---

### Lighting Classes

#### PointLight (`.pointlight.toml`)

```toml
[instance]
name = "TableLamp"
class_name = "PointLight"
archivable = true
ai = false

[transform]
position = [5.0, 3.0, 2.0]

[light]
brightness = 1.0
color = [1.0, 0.95, 0.85, 1.0]       # Warm white
range = 60.0                          # Falloff distance in meters
shadows = true
texture = ""                          # Optional: light cookie texture path

[metadata]
id = "lamp-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

#### SpotLight (`.spotlight.toml`)

```toml
[instance]
name = "Flashlight"
class_name = "SpotLight"
archivable = true
ai = false

[transform]
position = [0.0, 2.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]

[light]
brightness = 2.0
color = [1.0, 1.0, 1.0, 1.0]
range = 100.0
angle = 45.0                          # Cone angle in degrees
shadows = true
texture = ""

[metadata]
id = "spotlight-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

#### Star (`.star.toml`)

```toml
[instance]
name = "Sol"
class_name = "Star"
archivable = true
ai = false

[star]
brightness = 1.0
color = [1.0, 0.95, 0.85, 1.0]
direction = [-0.5, -1.0, -0.3]        # Normalized direction vector
temperature = 5778.0                   # Kelvin (for color calculation)
radius = 696340.0                      # km (for orbital systems)

[metadata]
id = "star-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

#### Atmosphere (`.atmosphere.toml`)

```toml
[instance]
name = "DayAtmosphere"
class_name = "Atmosphere"
archivable = true
ai = false

[atmosphere]
density = 0.3
offset = 0.0
color = [0.7, 0.8, 1.0, 1.0]          # Sky tint
decay_color = [0.9, 0.6, 0.4, 1.0]    # Sunset color
glare = 0.0
haze = 0.0

[metadata]
id = "atmo-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

---

### Character Classes

#### Humanoid (`.humanoid.toml`)

```toml
[instance]
name = "Humanoid"
class_name = "Humanoid"
archivable = true
ai = false

[movement]
walk_speed = 16.0                     # studs/sec
run_speed = 32.0                      # studs/sec
jump_power = 50.0                     # impulse
max_slope_angle = 89.0                # degrees
hip_height = 2.0                      # studs

[state]
can_jump = true
can_move = true
auto_rotate = true
platform_stand = false
sitting = false

[health]
health = 100.0
max_health = 100.0

[metadata]
id = "humanoid-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

---

### Camera Classes

#### Camera (`.camera.toml`)

```toml
[instance]
name = "MainCamera"
class_name = "Camera"
archivable = true
ai = false

[transform]
position = [0.0, 10.0, 20.0]
rotation = [-0.2, 0.0, 0.0, 0.98]

[camera]
field_of_view = 70.0                  # degrees
near_clip = 0.1
far_clip = 10000.0
camera_type = "Custom"                # Custom, Scriptable, Follow, Track, Watch, Attach, Fixed
camera_subject = ""                   # Instance name or ID
max_zoom_distance = 400.0
min_zoom_distance = 0.5
head_locked = false
head_scale = 1.0

[metadata]
id = "camera-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

---

### Seat Classes

#### VehicleSeat (`.vehicleseat.toml`)

```toml
[instance]
name = "DriverSeat"
class_name = "VehicleSeat"
archivable = true
ai = false

[transform]
position = [0.0, 1.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]

[seat]
disabled = false
seat_offset = [0.0, 0.5, 0.0]

[vehicle]
max_speed = 100.0                     # studs/sec
torque = 500.0
turn_speed = 90.0                     # degrees/sec
transmission = "Automatic"            # Automatic, Manual, CVT
gear_ratios = [-3.5, 3.5, 2.5, 1.8, 1.3, 1.0, 0.8]
final_drive_ratio = 3.7
idle_rpm = 800.0
redline_rpm = 7000.0
wheel_radius = 1.0
mass = 1500.0                         # kg
drag_coefficient = 0.3
rolling_resistance = 0.015
brake_force = 2000.0

[metadata]
id = "driverseat-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

---

### GUI Classes

#### ScreenGui (`.screengui.toml`)

```toml
[instance]
name = "HUD"
class_name = "ScreenGui"
archivable = true
ai = false

[gui]
enabled = true
display_order = 1
ignore_gui_inset = false
reset_on_spawn = true

[metadata]
id = "hud-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

#### TextLabel (`.textlabel.toml`)

```toml
[instance]
name = "Title"
class_name = "TextLabel"
archivable = true
ai = false

[layout]
position = [0.5, 0.1]                 # UDim2 scale (X, Y)
position_offset = [0, 0]              # UDim2 offset (X, Y)
size = [0.3, 0.05]                    # UDim2 scale
size_offset = [0, 0]                  # UDim2 offset
anchor_point = [0.5, 0.5]
rotation = 0.0
z_index = 1
visible = true

[text]
text = "Welcome to Eustress"
font = "GothamBold"
text_size = 24
text_color = [1.0, 1.0, 1.0, 1.0]
text_stroke_color = [0.0, 0.0, 0.0, 0.5]
text_stroke_transparency = 0.5
text_x_alignment = "Center"           # Left, Center, Right
text_y_alignment = "Center"           # Top, Center, Bottom
text_wrapped = false
text_scaled = false
rich_text = false

[appearance]
background_color = [0.0, 0.0, 0.0, 0.0]
background_transparency = 1.0
border_color = [0.0, 0.0, 0.0, 1.0]
border_size = 0

[metadata]
id = "title-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

---

### Effect Classes

#### ParticleEmitter (`.particles.toml`)

```toml
[instance]
name = "FireEffect"
class_name = "ParticleEmitter"
archivable = true
ai = false

[emitter]
enabled = true
rate = 50.0                           # particles/sec
lifetime = [1.0, 2.0]                 # min, max seconds
speed = [5.0, 10.0]                   # min, max studs/sec
spread_angle = [0.0, 180.0]           # min, max degrees
rotation = [0.0, 360.0]               # min, max degrees
rotation_speed = [-180.0, 180.0]      # degrees/sec

[appearance]
texture = "assets/textures/fire.png"
color = [[1.0, 0.5, 0.0, 1.0], [1.0, 0.0, 0.0, 0.0]]  # Color sequence
size = [[0.5], [2.0], [0.0]]          # Size sequence
transparency = [[0.0], [0.5], [1.0]]  # Transparency sequence
light_emission = 1.0
light_influence = 0.0

[physics]
acceleration = [0.0, 5.0, 0.0]        # Upward drift
drag = 0.5
velocity_inheritance = 0.0
z_offset = 0.0

[metadata]
id = "fire-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

#### Sound (`.sound.toml`)

```toml
[instance]
name = "BackgroundMusic"
class_name = "Sound"
archivable = true
ai = false

[audio]
sound_id = "assets/audio/music.ogg"
volume = 0.5
pitch = 1.0
looped = true
playing = false
time_position = 0.0

[spatial]
rolloff_mode = "InverseTapered"       # Linear, Inverse, InverseTapered
rolloff_min_distance = 10.0
rolloff_max_distance = 1000.0

[metadata]
id = "music-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

---

### Service Classes

#### Workspace Service (`Workspace/_service.toml`)

```toml
[service]
class_name = "Workspace"
gravity = [0.0, -196.2, 0.0]          # studs/sec² (default Earth gravity)
air_density = 1.225                    # kg/m³
streaming_enabled = false
streaming_min_radius = 64
streaming_target_radius = 1024

[metadata]
id = "workspace-service"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

#### Lighting Service (`Lighting/_service.toml`)

```toml
[service]
class_name = "Lighting"
ambient = [0.5, 0.5, 0.5, 1.0]
outdoor_ambient = [0.7, 0.7, 0.7, 1.0]
brightness = 1.0
color_shift_bottom = [0.0, 0.0, 0.0, 1.0]
color_shift_top = [0.0, 0.0, 0.0, 1.0]
environment_diffuse_scale = 1.0
environment_specular_scale = 1.0
global_shadows = true
clock_time = 14.0                      # 24-hour format (2:00 PM)
geographic_latitude = 41.7

[metadata]
id = "lighting-service"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

---

### Orbital Classes

#### CelestialBody (`.celestial.toml`)

```toml
[instance]
name = "Earth"
class_name = "CelestialBody"
archivable = true
ai = false

[orbital]
mass = 5.972e24                        # kg
radius = 6.371e6                       # meters
semi_major_axis = 1.496e11             # meters (AU)
eccentricity = 0.0167
inclination = 0.0                      # degrees
argument_of_periapsis = 102.9          # degrees
longitude_of_ascending_node = -11.26   # degrees
mean_anomaly_at_epoch = 357.5          # degrees
epoch = "2000-01-01T12:00:00Z"         # J2000

[appearance]
texture = "assets/textures/earth.png"
atmosphere = true
rings = false

[metadata]
id = "earth-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

## MCP File Access API

In EEP v2.0, AI models access project data directly via MCP filesystem tools. No push-based endpoints required — the AI reads files on demand.

### Core MCP Tools for EEP

| Tool | Purpose | Example |
|------|---------|---------|
| `mcp0_directory_tree` | List project structure | `mcp0_directory_tree(path="/project")` |
| `mcp0_read_text_file` | Read TOML instance files | `mcp0_read_text_file(path="/project/Workspace/Baseplate.part.toml")` |
| `mcp0_search_files` | Find files by pattern | `mcp0_search_files(path="/project", pattern="*.part.toml")` |
| `mcp0_list_directory` | List folder contents | `mcp0_list_directory(path="/project/Workspace")` |
| `mcp0_write_file` | Create/update instances | `mcp0_write_file(path="/project/Workspace/NewPart.part.toml", content="...")` |

### Reading Project Structure

```python
# 1. Get project tree
tree = mcp0_directory_tree(path="/project", excludePatterns=[".eustress/cache"])

# 2. Find all Part instances
parts = mcp0_search_files(path="/project", pattern="**/*.part.toml")

# 3. Read specific instance
baseplate = mcp0_read_text_file(path="/project/Workspace/Baseplate.part.toml")

# 4. Parse TOML and extract properties
import toml
data = toml.loads(baseplate)
position = data["transform"]["position"]
color = data["appearance"]["color"]
```

### Filtering by AI Consent

Only read instances where `ai = true`:

```python
# Read instance
content = mcp0_read_text_file(path=instance_path)
data = toml.loads(content)

# Check consent
if data.get("instance", {}).get("ai", False):
    # Include in training data
    process_instance(data)
```

### Writing Instance Files

AI models can create or modify instances:

```python
# Create new Part instance
new_part = """
[instance]
name = "AIGeneratedCube"
class_name = "Part"
archivable = true
ai = true

[transform]
position = [10.0, 5.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [1.0, 1.0, 1.0]

[geometry]
size = [2.0, 2.0, 2.0]
shape = "Block"

[appearance]
color = [0.2, 0.6, 1.0, 1.0]
material = "SmoothPlastic"
transparency = 0.0

[physics]
anchored = false
can_collide = true

[metadata]
id = "ai-gen-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
"""

mcp0_write_file(
    path="/project/Workspace/AIGeneratedCube.part.toml",
    content=new_part
)
```

### Batch Operations

For bulk reads, use search + batch read:

```python
# Find all instances with AI consent
all_tomls = mcp0_search_files(path="/project", pattern="**/*.toml")

# Read multiple files
contents = mcp0_read_multiple_files(paths=all_tomls)

# Filter and process
for path, content in contents.items():
    data = toml.loads(content)
    if data.get("instance", {}).get("ai", False):
        yield data
```

---

## Binary and Cache Handling

### Cache Directory Structure

```
.eustress/cache/
├── manifest.json           ← Cache index (source_hash → cached_path)
├── textures/
│   ├── a1b2c3d4.bc7        ← GPU-compressed texture (BC7/DXT)
│   └── e5f6g7h8.astc       ← Mobile GPU texture (ASTC)
├── meshes/
│   ├── i9j0k1l2.mesh       ← Optimized vertex buffer
│   └── m3n4o5p6.lod        ← LOD chain
├── scripts/
│   └── main.soulc          ← Compiled Soul bytecode
└── thumbnails/
    └── Baseplate.thumb.webp ← Explorer preview
```

### Cache Manifest Format

```json
{
  "version": "1.0",
  "engine_version": "0.16.1",
  "entries": {
    "assets/textures/grass.png": {
      "source_hash": "xxh64:a1b2c3d4e5f6g7h8",
      "source_mtime": "2026-03-02T10:00:00Z",
      "cached_path": "textures/a1b2c3d4.bc7",
      "cached_at": "2026-03-02T10:01:00Z",
      "format": "BC7",
      "size_bytes": 524288
    },
    "assets/meshes/Character.glb": {
      "source_hash": "xxh64:i9j0k1l2m3n4o5p6",
      "source_mtime": "2026-03-02T09:00:00Z",
      "cached_path": "meshes/i9j0k1l2.mesh",
      "cached_at": "2026-03-02T09:01:00Z",
      "format": "OptimizedMesh",
      "size_bytes": 1048576
    }
  }
}
```

### Cache Invalidation Strategy

1. **Fast path**: Check `source_mtime` — if unchanged, cache is valid
2. **Accurate path**: If mtime changed, compute `source_hash` (xxHash64)
3. **Regenerate**: If hash differs, regenerate cached asset
4. **Version check**: If `engine_version` differs, invalidate entire cache

```rust
fn is_cache_valid(entry: &CacheEntry, source: &Path) -> bool {
    let source_meta = fs::metadata(source)?;
    
    // Fast mtime check
    if source_meta.modified()? == entry.source_mtime {
        return true;
    }
    
    // Accurate hash check
    let current_hash = xxhash64(fs::read(source)?);
    current_hash == entry.source_hash
}
```

### Binary Asset References in TOML

TOML files reference binary assets via relative paths:

```toml
[asset]
mesh = "assets/meshes/Character.glb"    # Binary mesh
scene = "Scene0"

[appearance]
texture_diffuse = "assets/textures/character_diffuse.png"
texture_normal = "assets/textures/character_normal.png"
```

The engine:
1. Resolves path relative to project root
2. Checks cache for optimized version
3. Loads from cache (fast) or source (slow, then caches)

### MCP Access to Binary Assets

AI models can read binary asset metadata but not raw binary data:

```python
# Read mesh instance (TOML metadata)
mesh_toml = mcp0_read_text_file(path="/project/Workspace/Character.meshpart.toml")

# Get referenced binary path
data = toml.loads(mesh_toml)
mesh_path = data["asset"]["mesh"]  # "assets/meshes/Character.glb"

# Get file info (size, mtime) without reading binary
info = mcp0_get_file_info(path=f"/project/{mesh_path}")
# Returns: { size: 2097152, modified: "2026-03-02T09:00:00Z", ... }
```

For actual binary processing, use the cache manifest:

```python
# Read cache manifest
manifest = mcp0_read_text_file(path="/project/.eustress/cache/manifest.json")
cache_data = json.loads(manifest)

# Get cached mesh info
mesh_entry = cache_data["entries"].get("assets/meshes/Character.glb")
if mesh_entry:
    print(f"Cached at: {mesh_entry['cached_path']}")
    print(f"Format: {mesh_entry['format']}")
    print(f"Size: {mesh_entry['size_bytes']} bytes")
```

---

## Property Serialization Rules

### Type Mappings

| Rust Type | TOML Type | Example |
|-----------|-----------|---------|
| `f32` | float | `transparency = 0.5` |
| `i32` | integer | `z_index = 1` |
| `bool` | boolean | `anchored = true` |
| `String` | string | `name = "Part"` |
| `Vec3` | array | `position = [1.0, 2.0, 3.0]` |
| `Quat` | array | `rotation = [0.0, 0.0, 0.0, 1.0]` |
| `Color` | array | `color = [1.0, 0.5, 0.0, 1.0]` |
| `Option<T>` | omitted or value | `texture = ""` or omit field |
| `Vec<T>` | array | `gear_ratios = [3.5, 2.5, 1.8]` |
| `enum` | string | `material = "SmoothPlastic"` |

### Enum Serialization

All enums serialize as PascalCase strings:

```toml
# PartType enum
shape = "Block"           # Block, Ball, Cylinder, Wedge, CornerWedge, Cone

# Material enum
material = "SmoothPlastic" # Plastic, SmoothPlastic, Wood, Metal, Glass, etc.

# TransmissionType enum
transmission = "Automatic" # Automatic, Manual, CVT
```

### Transform Serialization

Transforms use separate position/rotation/scale arrays:

```toml
[transform]
position = [10.0, 5.0, -3.0]           # Vec3 (x, y, z)
rotation = [0.0, 0.707, 0.0, 0.707]    # Quat (x, y, z, w) - 90° Y rotation
scale = [1.0, 2.0, 1.0]                # Vec3 (x, y, z)
```

### Color Serialization

Colors use RGBA arrays with values 0.0-1.0:

```toml
color = [1.0, 0.5, 0.0, 1.0]           # Orange, fully opaque
background_color = [0.0, 0.0, 0.0, 0.5] # Black, 50% transparent
```

### Optional Fields

Optional fields can be omitted or set to empty/default:

```toml
# Option 1: Omit the field entirely
# texture = ...  (not present)

# Option 2: Empty string for string options
texture = ""

# Option 3: Explicit null (not recommended, use omission)
# TOML doesn't have null, so omit instead
```

### Nested Tables

Complex properties use nested TOML tables:

```toml
[physics]
anchored = true
can_collide = true

[physics.custom_properties]    # Nested table
density = 2400.0
friction = 0.5
elasticity = 0.3
```

---

## Hierarchy Representation

### The Rule

> **If a class can contain children → it gets a filesystem folder with `_instance.toml` inside.**
> **If a class attaches to a parent but cannot contain children → it uses a `[hierarchy]` `parent` field.**

This keeps the common case (Model/Folder trees) zero-config, while still handling attachment-style relationships.

### Classes that Use Folders

These classes become directories on disk. Their `_instance.toml` is the marker file; every sibling file and subfolder inside is a child.

| Class | Folder Marker |
|-------|---------------|
| Model | `_instance.toml` (`class_name = "Model"`) |
| Folder | `_instance.toml` (`class_name = "Folder"`) |
| ScreenGui | `_instance.toml` (`class_name = "ScreenGui"`) |
| BillboardGui | `_instance.toml` |
| SurfaceGui | `_instance.toml` |
| Frame | `_instance.toml` |
| ScrollingFrame | `_instance.toml` |
| Services (Workspace, Lighting, …) | `_service.toml` |

### Classes that Use `parent` Field

These are leaf nodes or cross-references that live alongside (not inside) their host:

| Class | Why `parent` |
|-------|--------------|
| Attachment | Positioned relative to a specific Part |
| WeldConstraint | Connects two named Parts |
| Motor6D | Joint between two Parts |
| ParticleEmitter | Emits from a Part's position |
| Beam | Spans between two Attachments |
| Decal | Applied to a Part surface |
| Sound | Plays from a Part's position |

### Complete Example

```
MyGame/
├── Workspace/
│   ├── _service.toml                    ← Workspace service
│   ├── Baseplate.part.toml              ← Direct child of Workspace
│   │
│   ├── Player/                          ← Model (folder = container)
│   │   ├── _instance.toml              ← class_name = "Model"
│   │   ├── HumanoidRootPart.part.toml
│   │   ├── Head.part.toml
│   │   ├── Torso.part.toml
│   │   ├── Humanoid.humanoid.toml
│   │   ├── RightHand.part.toml
│   │   └── GripPoint.attachment.toml   ← [hierarchy] parent = "RightHand.part.toml"
│   │
│   └── Props/                           ← Folder (folder = container)
│       ├── _instance.toml              ← class_name = "Folder"
│       ├── Chair.part.toml             ← mesh ref (assets/meshes/Chair.glb)
│       └── Table.part.toml             ← mesh ref (assets/meshes/Table.glb)
│
├── Lighting/
│   ├── _service.toml
│   ├── Sun.sun.toml
│   └── Atmosphere.atmosphere.toml
│
├── StarterGui/
│   ├── _service.toml
│   └── HUD/                             ← ScreenGui (folder = container)
│       ├── _instance.toml              ← class_name = "ScreenGui"
│       ├── HealthBar.frame.toml
│       └── Minimap.frame.toml
│
└── src/
    └── main.soul
```

### `parent` Field Format

The `parent` field is a **path relative to the project root**, pointing to the parent instance file:

```toml
# Workspace/Player/GripPoint.attachment.toml
[instance]
name = "GripPoint"
class_name = "Attachment"

[hierarchy]
parent = "Workspace/Player/RightHand.part.toml"

[attachment]
position = [0.0, 0.0, -0.5]
orientation = [0.0, 0.0, 0.0]
```

For constraints linking two parts:

```toml
# Workspace/Player/ShoulderJoint.motor6d.toml
[instance]
name = "ShoulderJoint"
class_name = "Motor6D"

[hierarchy]
parent = "Workspace/Player/Torso.part.toml"

[motor6d]
part0 = "Workspace/Player/Torso.part.toml"
part1 = "Workspace/Player/LeftUpperArm.part.toml"
c0 = [2.0, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0]   # CFrame (position + rotation)
c1 = [0.0, -0.5, 0.0, 0.0, 0.0, 0.0, 1.0]
max_velocity = 0.5
desired_angle = 0.0
```

### Service Hierarchy

Services are top-level folders. They always exist — the engine creates them if missing:

```
MyGame/
├── Workspace/          _service.toml
├── Lighting/           _service.toml
├── Players/            _service.toml
├── ReplicatedStorage/  _service.toml
├── ServerStorage/      _service.toml
├── StarterGui/         _service.toml
├── StarterPack/        _service.toml
└── Teams/              _service.toml
```

## Parameters Architecture

> **STATUS (2026-06): NOT IMPLEMENTED / SUPERSEDED — design proposal, not current behavior.**
> The typed 3-tier model below (the Global registry at `.eustress/parameters/global.toml`,
> per-domain `{domain}.toml` files, and the typed `InstanceParameters` component with consent
> gating) has **no runtime wiring**. No code reads or writes any `.eustress/parameters/` path
> (verified by source grep); `GlobalParametersRegistry` is touched only by the deprecated,
> UI-unreachable `.scene.json` path; and `InstanceParameters` is never attached to an entity nor
> reflect-registered. The "file-system-first" premise was itself superseded by the
> **WorldDb-primary binary pivot** — TOML `.glb.toml` is the authoring source of truth and the
> `.eustress` binary archive is the primary runtime + distribution format (see
> [`SERIALIZATION_AUDIT.md`](development/SERIALIZATION_AUDIT.md)). What actually persists per
> instance today is a single flat, **untyped** `[parameters]` map (`HashMap<String, toml::Value>`)
> folded into the binary archive's `extra` blob under the key `__parameters` and mirrored to the
> instance TOML — no Global/Domain tier, no typed `ParameterValue`, no consent gating. See
> [`DATA_PLATFORM_PLAN.md` §3.5](architecture/DATA_PLATFORM_PLAN.md) for the canonical
> reconciliation (Parameters = semantics & governance schema; the Data Platform supplies the
> implementations under it).

### 3-Tier Hierarchy

| Level | Scope | Purpose | Storage |
|-------|-------|---------|---------|
| **Global** | System-wide | Data types, connection templates | `.eustress/parameters/global.toml` |
| **Domain** | Logical group | Key-value schema per use case | `.eustress/parameters/{domain}.toml` |
| **Instance** | Per-entity | Specific value applied to entity | `[parameters]` section in instance TOML |

### Built-in Domains

| Domain | Purpose | Keys |
|--------|---------|------|
| `ai_training` | AI training opt-in | `enabled`, `category`, `priority` |
| `spatial_metrics` | Analytics | `views`, `interactions`, `time_spent` |
| `replication` | Network sync | `replicate`, `owner`, `interpolate` |
| `physics_override` | Per-instance physics | `gravity_scale`, `air_resistance` |

### Domain Schema Example (`.eustress/parameters/ai_training.toml`)

```toml
[domain]
id = "ai_training"
name = "AI Training"
description = "Controls AI training data inclusion"
version = 1
requires_ai_consent = true

[keys.enabled]
name = "enabled"
value_type = "Bool"
default = false
required = true
description = "Whether entity is included in AI training"

[keys.category]
name = "category"
value_type = "String"
required = false
description = "Training category (e.g., architecture, nature)"

[keys.priority]
name = "priority"
value_type = "Int"
default = 0
required = false
description = "Training priority (higher = more important)"
```

### Instance Parameters Example

```toml
# Workspace/TemplePillar.part.toml

[instance]
name = "TemplePillar"
class_name = "Part"
ai = true

# ... other sections ...

[parameters.ai_training]
enabled = true
category = "architecture"
priority = 5

[parameters.replication]
replicate = true
owner = "server"
interpolate = true
```

## Export and Access Patterns

> **Note**: the patterns below export current *instance state* (what a Space looks like right now). To export *converged simulation trajectories* — time-series runs packaged as AI training datasets — see [§16 AI Training Dataset Export](#ai-training-dataset-export), a different, v3.0 capability layered on top of this same file substrate.

### Primary Pattern: MCP File Access

In EEP v2.0, the primary access pattern is **pull-based file access**:

```
AI Model ──MCP Tools──> Project Filesystem ──> TOML Files
```

No export step required — AI reads files directly.

### Secondary Patterns (Optional)

For scenarios requiring push-based export:

| Pattern | Use Case | Implementation |
|---------|----------|----------------|
| **File Sync** | Cloud backup | `rsync` / `rclone` to S3/GCS |
| **Git Push** | Version control | Standard `git push` workflow |
| **Archive Export** | Distribution | `eustress pack` → `.eep` archive |
| **Webhook Notify** | Real-time updates | File watcher → HTTP POST |

### Archive Export (`.eep`)

For distribution or offline AI training:

```bash
# Create compressed archive of project
eustress pack --output my-project.eep --include-cache

# Archive contents:
my-project.eep
├── manifest.json           ← File index
├── Workspace/              ← Instance TOML files
├── Lighting/
├── assets/                 ← Binary assets (LZ4 compressed)
└── .eustress/cache/        ← Optional: pre-computed cache
```

### Sync Configuration (`.eustress/sync.toml`)

```toml
[sync]
enabled = true
auto_sync = false

[sync.targets.cloud_backup]
type = "S3"
bucket = "my-eustress-projects"
prefix = "projects/my-game/"
credentials = "aws-profile:eustress"

[sync.targets.git]
type = "Git"
remote = "origin"
branch = "main"
auto_commit = false

[sync.filters]
include = ["**/*.toml", "**/*.soul", "assets/**"]
exclude = [".eustress/cache/**", ".eustress/local/**"]
```

## Consent Model

### Opt-In Only

- Entities are **not included in AI training by default**
- Users must explicitly set `ai = true` in the instance TOML
- Consent is recorded in the `[consent]` section with timestamp

### Consent in Instance TOML

```toml
[instance]
name = "TemplePillar"
class_name = "Part"
ai = true                              # AI training opt-in flag

[consent]
ai_training = true
consented_at = "2026-03-02T10:00:00Z"
consented_by = "user-uuid"
consent_version = 1                    # Consent policy version
```

### Consent Revocation

When `ai = false` is set:
1. AI models should skip this instance when reading files
2. The `[consent]` section is removed or updated
3. File modification triggers cache invalidation

```toml
[instance]
name = "PrivatePillar"
class_name = "Part"
ai = false                             # Not included in AI training

# No [consent] section when ai = false
```

### Bulk Consent Query

AI models can efficiently find consented instances:

```python
# Find all AI-consented instances
all_tomls = mcp0_search_files(path="/project", pattern="**/*.toml")

consented = []
for path in all_tomls:
    content = mcp0_read_text_file(path=path)
    data = toml.loads(content)
    if data.get("instance", {}).get("ai", False):
        consented.append((path, data))

print(f"Found {len(consented)} consented instances")
```

## Error Handling

### File Parsing Errors

When reading TOML files, handle parsing errors gracefully:

```python
try:
    content = mcp0_read_text_file(path=instance_path)
    data = toml.loads(content)
except FileNotFoundError:
    # Instance file doesn't exist
    log_error(f"Instance not found: {instance_path}")
except toml.TomlDecodeError as e:
    # Malformed TOML
    log_error(f"Invalid TOML in {instance_path}: {e}")
```

### Common Error Conditions

| Condition | Cause | Resolution |
|-----------|-------|------------|
| `FileNotFoundError` | Instance deleted or moved | Refresh file list |
| `TomlDecodeError` | Malformed TOML syntax | Report to user for fix |
| `KeyError` | Missing required field | Use defaults or skip |
| `PermissionError` | File locked or no access | Retry or skip |
| `UnicodeDecodeError` | Binary file read as text | Check file extension |

### Validation Rules

Instance TOML files must:
1. Have valid TOML syntax
2. Include `[instance]` section with `name` and `class_name`
3. Include `[metadata]` section with `id`
4. Use correct types for all fields (see Property Serialization Rules)

```python
def validate_instance(data: dict) -> list[str]:
    errors = []
    
    # Required sections
    if "instance" not in data:
        errors.append("Missing [instance] section")
    else:
        if "name" not in data["instance"]:
            errors.append("Missing instance.name")
        if "class_name" not in data["instance"]:
            errors.append("Missing instance.class_name")
    
    if "metadata" not in data:
        errors.append("Missing [metadata] section")
    elif "id" not in data["metadata"]:
        errors.append("Missing metadata.id")
    
    return errors
```

---

## Versioning

### Protocol Version Format

`eep_v{major}.{minor}`

- **Major**: Breaking changes to TOML schema
- **Minor**: Additive changes (new optional fields)

This document is currently at **v3.0** — but that is the *document's* version, not necessarily the *shipped format's* version. §1–§17 (the TOML instance format) are unchanged since v2.0 and remain what real Spaces implement. §16 (AI Training Dataset Export) is new in v3.0 as a draft proposal with zero engine implementation. **The two are allowed to diverge**: a Space's own declared format version should reflect what that Space's TOML actually conforms to, not how far ahead the specification document has been written.

Current shipped format version: `eep_v2.0` (unchanged by this document being at v3.0). Do not bump the format version written by `scaffold_new_space`/`create_universe_folder` to `"3.0"` until §16 has a real implementation to justify it — it's a live field, checked into every real Space's `project.toml`, not just documentation.

### Version Detection

Check `.eustress/project.toml` for the *shipped format* version (independent of this document's own version above):

```toml
[project]
name = "My Game"
eep_version = "2.0"
engine_version = "0.16.1"
```

### Backward Compatibility

- EEP v2.0 readers should handle missing optional fields gracefully
- Use defaults for fields not present in older files
- Warn but don't fail on unknown fields (forward compatibility)

---

## Security Considerations

1. **File Permissions**: Respect OS file permissions
2. **Path Traversal**: Validate paths stay within project root
3. **Input Validation**: Validate all TOML data before use
4. **Consent Verification**: Always check `ai = true` before training
5. **Audit Logging**: Log file access for compliance

### Path Validation

```python
def is_safe_path(project_root: str, file_path: str) -> bool:
    """Ensure path doesn't escape project root."""
    abs_root = os.path.abspath(project_root)
    abs_path = os.path.abspath(os.path.join(project_root, file_path))
    return abs_path.startswith(abs_root)
```

---

## Implementation Checklist

### For Eustress Engine (File Writer)

- [x] Implement TOML serialization for all 60+ class types
- [x] Add `ai` flag to Instance component
- [x] Create file watcher for hot-reload
- [x] Implement cache manifest generation
- [x] Add consent tracking in `[consent]` section
- [ ] Implement `eustress pack` archive command
- [ ] Add file validation on save

### For AI Model (MCP Reader)

- [ ] Implement TOML parsing for all class schemas
- [ ] Add consent filtering (`ai = true` check)
- [ ] Handle file watching for real-time updates
- [ ] Implement batch file reading
- [ ] Add validation for required fields
- [ ] Handle binary asset metadata

---

## AI Training Dataset Export

> **STATUS (2026-07-12): DRAFT PROPOSAL — not implemented.** Everything in this section (the `eep_exporter` crate, the `export_converged_space` MCP tool, the `eustress export` CLI command, the Convergence Judge, the Critic stage) is a design proposal, not current engine behavior. No code in this repo implements any of it (verified: no `eep_exporter`, no `export_converged_space`, no `run_simulations` MCP primitive as described here). It does not affect §1–§17, which remain Active and unchanged. Originally drafted as a standalone "EEP v1.0" proposal by Simbuilder (McKale Olson) with Grok collaborative refinement, 2026-07-12; integrated here as EEP v3.0 §16 and renamed internally (below) to stop colliding with this document's own "EEP" identity, which already means the file-format protocol in §1–§17.

### Overview

This section specifies a protocol for exporting *converged simulation trajectories* — not static instance snapshots, but time-series runs that have passed quality gates — as production-grade training data for external AI models (world models, agents, physics-informed networks, digital twins). Where §1–§17 answer "how does a live AI model read a Space's current state," this section answers "how do we package a *finished, verified simulation run* as a dataset for training a model elsewhere."

It is designed around a closed-loop architecture: state evolves toward an attractor of user intent + functional completeness, and only artifacts that pass rigorous quality gates are eligible for export. It leans on Eustress-native strengths already specified elsewhere in this document and in related docs: ECS attributes and Parameters (§9), realism physics, file-system-first provenance (§1–§8), and Spatial Vortex geometric-semantic representations ([`RECURSIVE_FEEDBACK_LOOP.md`](development/RECURSIVE_FEEDBACK_LOOP.md), [`KERNEL_LAW_SYSTEM.md`](development/KERNEL_LAW_SYSTEM.md), [`RUNE_VM_INTEGRATION.md`](development/RUNE_VM_INTEGRATION.md)).

### Purpose & Goals

**Primary goals:**
1. **Quality over volume** — export only converged, high-signal data, never noisy single-shot generations.
2. **Eustress differentiation** — richly encode every native property (stress tensors, Spatial Vortex flux matrices, entity attributes, run provenance).
3. **Scalability** — efficient columnar storage suitable for trillion-parameter training pipelines.
4. **Interoperability** — align with the Hugging Face Datasets ecosystem while extending it for simulation-specific needs.
5. **Reproducibility & provenance** — full lineage from prompt → generation → repair iterations → execution feedback.

**Non-goals (this proposal, as drafted):**
- Real-time streaming exports.
- Encrypted / private datasets (open/scientific use cases only, for now).
- Automatic dataset card generation (tooling would come later).

### Core Concepts

| Term | Definition |
|------|------------|
| **Space** | A git-diffable project folder representing one simulation workspace (same `Space` as §1–§17) |
| **Trajectory** | Time-series evolution of entity states under kernel laws |
| **Convergence** | State where `Δ ≈ 0` — no stubs, LSP/API clean, metrics within tolerance, no regression |
| **Spatial Vortex** | Geometric-semantic encoding layer (flux matrix, 3-6-9 anchors, sacred geometry mappings) |
| **MCP** | Model Context Protocol — same bridge §5 already specifies, extended here with a proposed export tool |
| **`run_simulations`** | Proposed MCP primitive that executes a Space and returns structured telemetry + state diffs |
| **Training Export Artifact** | A versioned, self-describing package containing one or more converged simulation exports (renamed from the original draft's "EEP Artifact" to avoid colliding with this document's own name) |

### Architecture (Closed-Loop Integration)

This is proposed as the **output stage** of a closed-loop system, not a standalone dump format:

```
User Intent Prompt
        ↓
State Generator (LLM) → Rune + Luau scripts + Scene
        ↓
Critic (AST + LSP + API surface + Spatial Vortex invariants)
        ↓
Execution Verifier (MCP run_simulations + kernel laws)
        ↓
Reflection + Targeted Repair (bounded context)
        ↓
Convergence Judge (Δ ≈ 0 + metrics pass)
        ↓
Trajectory Exporter → Hugging Face Dataset (or local artifact)
```

Only artifacts that reach the Convergence Judge successfully would be eligible for export.

### Data Model & Schema (proposed)

**Recommended storage format:**
- **Primary**: Apache Arrow / Parquet (columnar, compression-friendly, streaming) — same leaf the Data Platform already uses for Datasets (§10's TOML instance model is unrelated to this; this is bulk trajectory data, not per-instance state).
- **Metadata**: embedded Parquet metadata + sidecar `manifest.json`.
- **Sharding**: by time-step, entity class, or Spatial Vortex region for large trajectories.

**Top-level schema:**

```json
{
  "export_schema_version": "1.0",
  "space_id": "string",
  "branch": "string",
  "convergence_id": "string (uuid or git commit)",
  "export_timestamp": "ISO8601",
  "provenance": { "...": "..." },
  "spatial_vortex": { "...": "..." },
  "entities": { "...": "..." },
  "trajectories": { "...": "..." },
  "physics": { "...": "..." },
  "scripts": { "...": "..." },
  "metrics": { "...": "..." }
}
```

(Named `export_schema_version` here, not `eep_version` — that field already means the §1–§17 file-format version in `project.toml` and must not be conflated with this schema's own version.)

**Section detail:**

- **`provenance`** — original user prompt/intent, generation model + temperature + seed, full repair trajectory (iteration count, error types fixed), MCP tool calls and responses, git commit/branch at export time, engine version + realism feature flags.
- **`spatial_vortex`** — flux matrix configuration, 3-6-9 anchor positions, semantic-geometric encodings per entity/region, bi-directional inference seeds (if used).
- **`entities` + attributes + parameters** — full ECS snapshot: entity IDs + class, all registered Attributes/Parameters (per §9's *actual* current shape — a flat untyped `[parameters]` map, not the superseded 3-tier design), transform/rendering/physics bodies, custom components.
- **`trajectories`** (sharded time-series) — position/velocity/rotation/scale over time, stress/strain tensors, material state changes, collision/fracture events, custom telemetry channels.
- **`physics`** (realism extension) — stress tensors (Hooke's law, von Mises, etc.), strain fields, fracture mechanics state, SPH fluid particles (if enabled), thermodynamics/electrochemistry state.
- **`scripts`** — Rune source + compiled module, Luau source + `.d.luau` type definitions (LSP clean), API surface compliance report, optional bytecode/VM state snapshot.
- **`metrics`** (convergence signals) — stub count over iterations, LSP/type error count, simulation metric deltas (stress error, spatial deviation, FPS stability), user-intent alignment score (if defined), final convergence status (`converged`, `max_iterations`, `diverged`).

### Quality & Convergence Gates (proposed, mandatory for export)

An artifact would need to satisfy **all** of the following before export is allowed:

1. **Syntactic completeness** — zero `TODO`/`FIXME`/`pass`/`...`/`NotImplemented` in scripts; full Rune + Luau parsing success.
2. **LSP & API compliance** — zero diagnostics from `luau-lsp` (or equivalent); all exposed engine APIs correctly typed in `.d.luau`.
3. **Execution validation** — `run_simulations` completed without fatal errors; kernel laws respected (no NaN/inf in physics where invalid).
4. **Metric convergence** — `Δ` (state change between last two iterations) below threshold; no regression vs. previous best on branch.
5. **Spatial Vortex consistency** (optional but recommended) — geometric-semantic invariants hold.

### Hugging Face Integration (proposed)

**Dataset naming**: `eustress/<domain>-<use-case>-v<export-schema-version>` — e.g. `eustress/battery-digital-twins-v1.0`.

**Dataset card should include**: export schema version used, convergence criteria applied, Spatial Vortex configuration, realism physics modules enabled, provenance summary, example usage code (loading with `datasets` + Polars).

**Upload strategy**: `datasets` library with `push_to_hub`; prefer incremental commits for long-running simulations; store large trajectory shards as separate configs or Git LFS where appropriate.

### Implementation Notes (proposed engine-side work)

**New components (not yet built):**
- `eep_exporter` crate or module (name TBD to avoid the same collision this integration fixed above — consider `trajectory_exporter` or `training_export`).
- A trajectory-exporter resource/system.
- Integration point in `RunPlugin` or a dedicated `ExportPlugin`.

**Proposed MCP extension:**
```rust
pub async fn export_converged_trajectory(
    space_path: PathBuf,
    convergence_id: Option<String>,
    hf_repo: Option<String>,
) -> Result<ExportResult, ExportError>
```
(renamed from the original draft's `export_converged_space` — this exports a *trajectory*, not the Space's instance state, which §5's existing MCP file-access tools already cover.)

**Proposed CLI:**
```bash
eustress export-trajectory --space ./MyBatteryTwin --converged-only --push-to-hf eustress/battery-v1
```
(renamed from the original draft's `eustress export`, which collides with the real `eustress pack` archive command already specified in §10.)

### Open Questions & Future Work

- Differential/delta exports for very long trajectories.
- On-demand lazy generation via MCP ("dataset as a service").
- Encrypted/access-controlled datasets.
- Automated high-quality dataset card generation.
- Integration with Spatial Vortex for semantic search over exported trajectories.
- Standardized benchmarks for export quality (stress accuracy, intent alignment, etc.).
- Reconcile the `entities`/`physics`/`scripts` schema above against §9's actual current Parameters shape (flat untyped map) before implementation, not the superseded 3-tier design.

### References (this section)

- [Recursive Feedback Loop](development/RECURSIVE_FEEDBACK_LOOP.md)
- [File-System-First Architecture](development/FILE_SYSTEM_FIRST.md)
- [Kernel Law System](development/KERNEL_LAW_SYSTEM.md)
- [Rune VM Integration](development/RUNE_VM_INTEGRATION.md)
- MCP Protocol — proposed, not yet written
- Hugging Face Datasets documentation
- Apache Arrow / Parquet specification
- SWE-agent / OpenDevin-style agent-computer interfaces (MCP inspiration)
- Closed-loop self-repair literature (iterative LLM code repair with execution feedback)

---

## References

- [File-System-First Architecture](development/FILE_SYSTEM_FIRST.md)
- [Asset Instance Architecture](development/ASSET_INSTANCE_ARCHITECTURE.md)
- [Classes Module](../eustress/crates/common/src/classes.rs)
- [Parameters Module](../eustress/crates/common/src/parameters.rs)

---

## Changelog

### v3.0 (2026-07-12)

- **New**: §16 AI Training Dataset Export — a proposal for exporting *converged simulation trajectories* as Parquet/Hugging Face training datasets, distinct from §1–§17's per-instance TOML state format. **Status: Draft Proposal, not implemented** — see the status box at the top of §16.
- **Non-breaking**: §1–§17 (the actual TOML file format) are unchanged. The shipped `eep_version` field in `project.toml` stays `"2.0"` — this document advancing to v3.0 does not change what real Spaces conform to.
- **Integrated from**: a standalone "EEP v1.0" proposal drafted by Simbuilder (McKale Olson) with Grok collaborative refinement (2026-07-12). Renamed several of the original draft's self-referential terms (`EEP Artifact` → `Training Export Artifact`, `eep_version` in the export schema → `export_schema_version`, `export_converged_space` → `export_converged_trajectory`, the `eustress export` CLI verb → `eustress export-trajectory`) because the original draft independently reused the name "EEP" for an unrelated concept, colliding with this document's own identity and with the real `eustress pack` command already specified in §10.

### v2.0 (2026-03-02)

- **Breaking**: File-system-first architecture
- **Breaking**: TOML schemas for all 60+ class types (not just BasePart)
- **New**: MCP File Access API (pull-based, replaces push-based endpoints)
- **New**: Class-specific file extensions (`.part.toml`, `.camera.toml`, etc.)
- **New**: Folder-based hierarchy representation
- **New**: Binary and cache handling documentation
- **New**: Property serialization rules
- **New**: Consent tracking in instance TOML
- **Updated**: Parameters architecture uses TOML storage
- **Updated**: Export patterns (file sync, git, archive)
- **Removed**: Push-based MCP Server endpoints (use file access instead)

### v1.0 (2025-12-30)

- Initial specification
- JSON export record format
- Push-based MCP Server endpoints
- 3-tier parameters architecture
- Consent model
