# MaterialService Architecture

> Filesystem-first `.mat.toml` material definitions, Explorer integration with property editing,
> AI texture generation via discrete diffusion (MDLM), runtime blending, physics-reactive materials.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Filesystem Layout](#2-filesystem-layout)
3. [The .mat.toml Specification](#3-the-mattoml-specification)
4. [Material Registry](#4-material-registry)
5. [Explorer and Property Panel Integration](#5-explorer-and-property-panel-integration)
6. [Part Material Resolution](#6-part-material-resolution)
7. [Texture Import Methods](#7-texture-import-methods)
8. [AI Material Generation — Discrete Diffusion](#8-ai-material-generation--discrete-diffusion)
9. [Runtime Blending](#9-runtime-blending)
10. [Physics-Reactive Materials](#10-physics-reactive-materials)
11. [Implementation Phases](#11-implementation-phases)

---

## 1. Overview

### Problem

Current material system: hardcoded `Material` enum (18 presets) mapped to scalar PBR values.
No textures. No customization. No user-defined materials. `pbr_materials.rs` is a stub.

### Solution

**MaterialService** — a dynamic service folder containing `.mat.toml` files. Each file defines
a complete PBR material with optional texture maps. Materials appear as clickable Explorer items,
editable via Properties panel. Parts reference materials by name.

### Eustress versus Roblox

| Capability | Roblox | Eustress |
|---|---|---|
| Material presets | 18 fixed slots | 18 defaults + unlimited user `.mat.toml` files |
| Custom textures | MaterialVariant (limited) | Full PBR set (color, normal, metallic/roughness, emissive, occlusion, depth) |
| Material editing | Property grid only | Explorer item + Properties panel + live hot-reload |
| AI generation | Cloud-only, limited | Local discrete diffusion in Rust (MDLM), offline capable |
| Runtime blending | None | Weighted multi-material blend per vertex/face |
| Physics reactivity | None | Materials respond to force, temperature, wear in real-time |
| File format | Proprietary binary | Human-readable TOML, git-diffable |

---

## 2. Filesystem Layout

MaterialService is discovered dynamically via `_service.toml`.

```
SpaceN/
├── MaterialService/
│   ├── _service.toml              ← Service marker
│   ├── Plastic.mat.toml           ← Default preset override
│   ├── RustyMetal.mat.toml        ← User-defined custom material
│   ├── GlowingCrystal.mat.toml   ← AI-generated material
│   └── textures/                  ← Texture assets (relative paths)
│       ├── rusty_metal_color.png
│       ├── rusty_metal_normal.png
│       ├── rusty_metal_mr.png     ← G=roughness, B=metallic (glTF convention)
│       └── rusty_metal_ao.png
├── Workspace/
│   └── Baseplate.part.toml        ← References material = "RustyMetal"
```

### _service.toml

```toml
[service]
class_name = "MaterialService"
icon = "materialservice"
description = "PBR material definitions (.mat.toml files)"
```

---

## 3. The .mat.toml Specification

Filename (minus `.mat.toml`) is the material name. If `preset` is set, omitted PBR fields inherit
from the `Material` enum's `pbr_params()`.

```toml
# MaterialService/RustyMetal.mat.toml

[material]
name = "RustyMetal"
preset = "Metal"                             # Optional — inherit defaults from enum
description = "Weathered steel with rust patches"
tags = ["metal", "weathered", "industrial"]

[pbr]
base_color = [0.45, 0.32, 0.25, 1.0]        # RGBA linear
metallic = 0.7
roughness = 0.65
reflectance = 0.5
emissive = [0.0, 0.0, 0.0, 0.0]
alpha_mode = "Opaque"                         # Opaque | Blend | Mask
alpha_cutoff = 0.5
ior = 1.5
specular_transmission = 0.0
diffuse_transmission = 0.0
thickness = 0.0
double_sided = false
unlit = false

[textures]
base_color = "textures/rusty_metal_color.png"
normal = "textures/rusty_metal_normal.png"
metallic_roughness = "textures/rusty_metal_mr.png"
emissive = ""
occlusion = "textures/rusty_metal_ao.png"
depth = ""

[textures.settings]
normal_map_scale = 1.0
flip_normal_map_y = false
parallax_depth_scale = 0.05

[tiling]
uv_scale = [2.0, 2.0]
uv_offset = [0.0, 0.0]
triplanar = false
triplanar_sharpness = 1.0

[physics]
density = 7850.0                              # kg/m³
friction_static = 0.6
friction_kinetic = 0.4
restitution = 0.3
young_modulus = 200e9
yield_strength = 250e6
thermal_conductivity = 50.2
specific_heat = 490.0
melting_point = 1811.0

[blending]
blend_group = "metals"
blend_weight = 1.0
splatmap_channel = "R"

[reactive]
temperature_color_gradient = [
    { threshold = 400.0, color = [0.45, 0.32, 0.25, 1.0] },
    { threshold = 800.0, color = [0.8, 0.2, 0.05, 1.0] },
    { threshold = 1200.0, color = [1.0, 0.6, 0.1, 1.0] },
    { threshold = 1600.0, color = [1.0, 1.0, 0.8, 1.0] },
]
temperature_emissive_start = 600.0
impact_roughness_rate = 0.001
impact_roughness_max = 0.95
wear_color_darken_rate = 0.0001
wear_roughness_rate = 0.0002
wetness_roughness_multiplier = 0.5
wetness_color_darken = 0.2

[generation]
prompt = "weathered steel surface with rust patches and scratches"
negative_prompt = "smooth, clean, new"
model = "eustress-mdlm-pbr-v1"
seed = 42
steps = 50
resolution = 1024
generated_at = "2026-03-13T08:00:00Z"

[custom]
corrosion_resistance = 0.3
electrical_conductivity = 6.99e6
```

### Minimal Example (Preset Override)

```toml
[material]
preset = "SmoothPlastic"

[pbr]
base_color = [0.2, 0.8, 0.3, 1.0]
```

---

## 4. Material Registry

```rust
#[derive(Resource, Default)]
pub struct MaterialRegistry {
    /// Name → Bevy material handle
    materials: HashMap<String, Handle<StandardMaterial>>,
    /// Name → parsed definition (for property editing)
    definitions: HashMap<String, MaterialDefinition>,
    /// Name → source .mat.toml path (for writeback)
    source_paths: HashMap<String, PathBuf>,
}
```

### Lifecycle

1. **Space load** — file_loader scans MaterialService/, parses each `.mat.toml`, creates
   `StandardMaterial` with textures via AssetServer, inserts into registry, spawns Explorer entity
2. **Part spawn** — reads `material = "RustyMetal"`, queries registry, clones handle
3. **Hot-reload** — file watcher detects `.mat.toml` change → re-parse → update registry + asset
4. **Property edit** — user changes slider → update definition → serialize TOML → write to disk

---

## 5. Explorer and Property Panel Integration

Materials appear under MaterialService in the Explorer tree:

```
🎨 MaterialService
  🎨 Plastic
  🎨 RustyMetal
  🎨 GlowingCrystal
```

Each material entity has `Instance` with `class_name = ClassName::MaterialDefinition`,
`LoadedFromFile`, and `Name`. Clicking selects it; Properties panel shows editable fields grouped
as: Material, PBR (color picker, sliders), Textures (path + thumbnail + browse), Tiling, Physics,
Blending, Reactive, Generation.

Edits write directly to `.mat.toml` on disk (same writeback pattern as `.part.toml`).

---

## 6. Part Material Resolution

### Resolution Order

1. **MaterialRegistry** — exact name match against loaded `.mat.toml` files
2. **Material enum fallback** — `Material::from_string()` for 18 built-in presets
3. **Default** — `Material::Plastic`

### Properties Panel Material Selector

Dropdown shows: Custom Materials (from MaterialService) → Built-in Presets (enum) → "New Material..."

---

## 7. Texture Import Methods

| Format | Extension | Notes |
|---|---|---|
| PNG | `.png` | General purpose, lossless |
| JPEG | `.jpg` | Photographic, smaller |
| TGA | `.tga` | Legacy |
| DDS | `.dds` | GPU-compressed (BC1–BC7) |
| KTX2 | `.ktx2` | Modern GPU-compressed, cross-platform |

**Workflows:** Manual file drop, Properties panel browse button, drag-and-drop onto Explorer,
AI generation output, glTF texture extraction.

**Cache:** Source stays as-is (filesystem-first). Derived `.eustress/cache/textures/{hash}.ktx2`
for GPU-compressed loading.

---

## 8. AI Material Generation — Discrete Diffusion

### Why Discrete Diffusion Over Stable Diffusion

- **Runs locally** — no API key, no cloud, works offline
- **Native Rust** — no Python, no ONNX bridge, single binary
- **Discrete token space** — PBR textures as structured discrete problem (palette indices)
- **Multi-map coherence** — generates all 5 PBR maps in one coordinated pass

### Model Comparison

| Model | Venue | Fit for PBR Generation |
|---|---|---|
| **MDLM** | NeurIPS 2024 | **Best fit.** Simplest architecture. Masked diffusion maps naturally to "fill in texture tokens." Clean training loss. |
| **SEDD** | ICML 2024 Best Paper | Elegant score-matching theory but harder to implement in pure Rust. |
| **LLaDA** | Feb 2025 | 8 billion parameters — too large for local consumer hardware (16+ gigabytes VRAM). |
| **DiffuLLaMA** | ICLR 2025 | Converts autoregressive → diffusion. We train from scratch, so less relevant. |

### Architecture: eustress-mdlm-pbr

~50-100 million parameter MDLM variant for PBR texture generation:

```
Text Prompt → [Text Encoder (16M params)] → conditioning
  → [MDLM Denoiser (40M params, UNet-transformer)] → T denoising steps
  → [Discrete Token Grid: 512×512 × 5 maps × palette indices]
  → [Detokenize] → 5 PNG files (color, normal, metallic_roughness, occlusion, emissive)
  → Auto-generate .mat.toml referencing textures
```

- **~5-15 seconds** on consumer GPU (RTX 3060+)
- **~60 seconds** CPU fallback
- **Training data:** MatSynth (4000+ CC0 PBR), AmbientCG, PolyHaven, synthetic augmentation

### Rust Implementation

```rust
// eustress/crates/diffusion/src/lib.rs — behind `diffusion` feature flag
// Uses burn or candle (Rust ML frameworks) for GPU acceleration via wgpu/CUDA

pub struct MdlmPbrGenerator {
    model: MdlmModel,           // .safetensors weights
    tokenizer: PbrTokenizer,    // Pixel values → discrete palette tokens
    scheduler: MaskScheduler,   // Cosine masking schedule
    device: Device,             // CPU or CUDA
}

impl MdlmPbrGenerator {
    pub fn generate(&self, prompt: &str, config: GenerationConfig)
        -> Result<GeneratedMaterial, DiffusionError>;
}
```

### Studio User Flow

1. Right-click MaterialService → "Generate Material..."
2. Enter text prompt + seed + resolution
3. Progress bar (background thread)
4. Preview in Properties panel
5. "Accept" writes `.mat.toml` + textures, "Regenerate" tries new seed

---

## 9. Runtime Blending

Multiple materials blend on a single mesh. Roblox allows one material per Part.

### Blend Modes

- **Splatmap** — RGBA texture where each channel = weight for a material (up to 4 per map)
- **Vertex Attribute** — per-vertex weights as vertex colors (procedural: mud at wall base)
- **Height-Based** — depth/displacement maps create natural transitions at blend boundaries

### Part Configuration

```toml
[material_blend]
enabled = true
splatmap = "textures/wall_splatmap.png"
materials = ["StoneBrick", "Moss", "Dirt"]
blend_sharpness = 2.0
height_blend = true
```

### Shader

Custom `MaterialExtension` in WGSL that samples multiple PBR texture sets, blends by
splatmap weights with optional height-based sharpening at transitions.

---

## 10. Physics-Reactive Materials

Materials change appearance in response to physical simulation state.

### Reactive Channels

| Stimulus | PBR Response | Example |
|---|---|---|
| **Temperature** | Color gradient (room → red-hot → white-hot), emissive glow, roughness decrease | Heated metal glowing |
| **Impact Force** | Roughness increase, micro-displacement | Dented surface getting rougher |
| **Wear/Friction** | Color darkening, roughness increase over time | Worn floor path |
| **Wetness** | Roughness halved, color darkened | Rain on concrete |

### ECS Components

```rust
#[derive(Component, Default)]
pub struct MaterialReactiveState {
    pub accumulated_wear: f32,
    pub accumulated_impact: f32,
    pub wetness: f32,                         // 0.0–1.0
    pub current_temperature: f32,             // Kelvin (from ThermodynamicState)
}
```

### System

`update_reactive_materials` runs each frame, reads `MaterialReactiveState` + `ReactiveRules`
from the `.mat.toml`, interpolates PBR parameters, and updates the `StandardMaterial` asset.
Temperature comes from `ThermodynamicState` (already in realism module). Impact/wear accumulate
from physics collision events. Wetness from a weather/water system.

---

## 11. Implementation Phases

### Phase 1: Foundation (Priority)
1. Create `material_loader.rs` — parse `.mat.toml`, produce `Handle<StandardMaterial>`
2. Create `MaterialRegistry` resource
3. Add `FileType::Material` handler in `file_loader.rs`
4. Add `ClassName::MaterialDefinition` variant
5. Spawn `.mat.toml` as Explorer entities under MaterialService
6. Create `_service.toml` template for MaterialService in space scaffolding
7. Create default `.mat.toml` files for all 18 enum variants in `assets/material_templates/`
8. Wire Part material resolution: registry-first, enum fallback

### Phase 2: Property Editing
9. Properties panel: detect MaterialDefinition class, show PBR fields
10. Color picker for base_color/emissive
11. Slider widgets for metallic, roughness, reflectance
12. Texture path fields with browse button and thumbnail preview
13. Writeback system: property edits → serialize TOML → write `.mat.toml`
14. Hot-reload: file watcher updates StandardMaterial when `.mat.toml` changes

### Phase 3: Runtime Blending
15. `BlendedMaterial` component and `MaterialBlendExtension` shader
16. Splatmap loading and channel assignment
17. Height-based blend transitions
18. Vertex attribute blend support

### Phase 4: Physics-Reactive
19. `MaterialReactiveState` component
20. `update_reactive_materials` system
21. Temperature → color/emissive/roughness interpolation
22. Impact/wear accumulation from collision events
23. Wetness from water contact

### Phase 5: AI Generation (Discrete Diffusion)
24. Create `eustress/crates/diffusion/` crate (behind feature flag)
25. Implement MDLM architecture in Rust using `burn` or `candle`
26. PBR tokenizer (pixel palette encoding/decoding)
27. Cosine mask scheduler
28. Text encoder (small transformer)
29. UNet-transformer denoiser
30. Training pipeline (separate from engine, Python for data prep)
31. Model weight loading from `.safetensors`
32. Studio UI: "Generate Material..." dialog
33. Background generation with progress callback
34. Auto-generate `.mat.toml` from diffusion output

### Phase 6: Polish
35. Material thumbnail rendering (offscreen render for Explorer icons)
36. Material search/filter in Explorer
37. Material duplication (right-click → "Duplicate Material")
38. Material import from glTF embedded materials
39. KTX2 texture cache pipeline
