# Eustress Export Protocol (EEP) v2.0 Specification

**Version**: 2.0  
**Date**: March 2, 2026  
**Status**: Active

## Overview

The Eustress Export Protocol (EEP) v2.0 is a **file-system-first** protocol for serializing all Eustress class instances to human-readable TOML files. AI models access project data directly via MCP file APIs — no proprietary database, no binary project files.

### Key Changes from v1.0

| v1.0 | v2.0 |
|------|------|
| JSON export records pushed to MCP | MCP reads TOML files directly from filesystem |
| Only `glb.toml` for BaseParts | TOML schemas for **all 60+ class types** |
| Binary scene format | glTF 2.0 + EXT_eustress (JSON, git-diffable) |
| Push-based export | Pull-based file access |

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
16. [References](#references)
17. [Changelog](#changelog)

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

Current version: `eep_v2.0`

### Version Detection

Check `.eustress/project.toml` for EEP version:

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

## References

- [File-System-First Architecture](development/FILE_SYSTEM_FIRST.md)
- [Asset Instance Architecture](development/ASSET_INSTANCE_ARCHITECTURE.md)
- [Classes Module](../eustress/crates/common/src/classes.rs)
- [Parameters Module](../eustress/crates/common/src/parameters.rs)

---

## Changelog

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
