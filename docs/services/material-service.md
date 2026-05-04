# MaterialService

**Category:** Rendering  
**Class Name:** `MaterialService`  
**Learn URL:** `/learn/services/material-service`

## Overview

MaterialService loads and manages all material definitions used by parts. It
reads `.mat.toml` preset files from the assets directory and provides a
MaterialRegistry of named materials with base color textures, normal maps,
roughness, and metallic values.

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `Use2022Materials` | bool | `true` | Use modern PBR material textures instead of legacy flat colors. |
| `AssetPath` | string | `materials/` | Directory path for material texture assets. |

## Key Responsibilities

- **Built-in PBR Presets** — Ships with material presets for common surfaces:
  Plastic, SmoothPlastic, Metal, DiamondPlate, Wood, WoodPlanks, Marble,
  Granite, Brick, Cobblestone, Concrete, CorrodedMetal, Grass, Ice, Sand,
  Fabric, Pebble, Slate, Limestone, Basalt, CrackedLava, Glacier, Pavement,
  Asphalt, LeafyGrass, Salt, Mud, Snow, Sandstone, Rock, and more.

- **Custom Materials** — Create `.mat.toml` files in the materials directory
  to define custom PBR materials with base color, normal, roughness, metallic,
  and emissive textures.

- **Texture Loading** — Automatically loads and caches texture assets
  referenced by material definitions. Supports PNG, JPEG, and KTX2 formats.

- **UV Mapping** — Materials respect the UV transform calculated from part
  size, ensuring textures tile proportionally to world dimensions.

- **MaterialVariant** — Override the default material for specific parts or
  regions to create themed environments (e.g., snowy versions of grass and
  rock materials).

## Material Definition Format

Materials are defined as `.mat.toml` files:

```toml
[material]
name = "CustomBrick"
base_color_texture = "textures/brick_albedo.png"
normal_map_texture = "textures/brick_normal.png"
roughness = 0.8
metallic = 0.0
```

## Common Patterns

### Applying Materials
```rune
let part = workspace.FindFirstChild("Wall");
part.Material = "Brick";
// Or use a custom material:
part.MaterialVariant = "CustomBrick";
```

### Material Swapping
Use MaterialVariant to swap entire themes at runtime (e.g., summer → winter):
```rune
for part in workspace.GetDescendants() {
    if part.Material == "Grass" {
        part.MaterialVariant = "SnowGrass";
    }
}
```

## Related Services

- [Workspace](workspace.md) — Parts reference materials from MaterialService
- [Lighting](lighting.md) — Lighting affects how materials appear
- [AdornmentService](adornment-service.md) — Visual overlays on material surfaces
