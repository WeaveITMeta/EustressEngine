# Lighting System - TOML-Based Architecture

## Overview

The Lighting system in Eustress Engine uses **TOML files** instead of binary entities for all Lighting service children. This provides:
- **Human-readable** configuration files
- **Git-diffable** lighting setups
- **Template-based** Space creation
- **Roblox-inspired** properties with greater utility
- **File-system-first** architecture alignment

## Lighting Children

Every new Space automatically receives 5 TOML files in the `Lighting/` folder:

### 1. Atmosphere.toml
Atmospheric scattering and sky rendering.

**Key Properties:**
- `Density` - Atmospheric thickness (0.0-1.0)
- `Color` - Atmosphere color toward sun
- `Decay` - Light decay color through atmosphere
- `Glare` - Sun glare intensity
- `Haze` - Horizon haze intensity
- `RayleighCoefficient` - Blue sky scattering (Vector3)
- `MieCoefficient` - Haze/fog scattering
- `MieDirectionalFactor` - Forward scattering bias

**Example:**
```toml
[Appearance]
Density = { type = "float", value = 0.5, min = 0.0, max = 1.0 }
Color = { type = "Color3", value = [0.776, 0.863, 1.0] }

[Scattering]
RayleighCoefficient = { type = "Vector3", value = [5.8, 13.5, 33.1] }
```

### 2. Moon.toml
Lunar body with phases and lighting.

**Key Properties:**
- `TextureId` - Custom moon texture asset ID
- `Scale` - Moon size multiplier
- `Phase` - Lunar phase (0=new, 0.5=full, 1.0=new)
- `AutoPhase` - Auto-cycle phases based on time
- `Azimuth` / `Elevation` - Position in sky
- `CastLight` - Enable moonlight
- `LightIntensity` - Moonlight brightness
- `LightColor` - Moonlight color (cool blue-white)

**Example:**
```toml
[Phase]
Phase = { type = "float", value = 0.5, min = 0.0, max = 1.0 }
AutoPhase = { type = "bool", value = false }

[Lighting]
CastLight = { type = "bool", value = true }
LightIntensity = { type = "float", value = 0.1 }
LightColor = { type = "Color3", value = [0.8, 0.85, 1.0] }
```

### 3. Sky.toml
Procedural or textured sky rendering.

**Key Properties:**
- `SkyMode` - "Procedural" or "Textured"
- `CelestialBodiesShown` - Show sun and moon
- `StarCount` - Number of stars (0-10000)
- `SkyColor` / `HorizonColor` - Procedural sky colors
- `CloudCover` - Cloud coverage (0=clear, 1=overcast)
- `CloudColor` / `CloudDensity` - Cloud appearance
- `CloudSpeed` - Cloud movement velocity (Vector2)
- `CloudAltitude` - Cloud layer height in meters

**Example:**
```toml
[Appearance]
SkyMode = { type = "enum", value = "Procedural", options = ["Procedural", "Textured"] }
StarCount = { type = "int", value = 3000, min = 0, max = 10000 }

[Clouds]
CloudCover = { type = "float", value = 0.5 }
CloudSpeed = { type = "Vector2", value = [1.0, 0.5] }
```

### 4. Sun.toml
Solar body with dynamic lighting and time-of-day.

**Key Properties:**
- `TextureId` - Custom sun texture asset ID
- `Scale` - Sun size multiplier
- `Azimuth` / `Elevation` - Position in sky
- `AutoRotate` - Auto-rotate based on Lighting.ClockTime
- `CastLight` - Enable sunlight
- `LightIntensity` - Sunlight brightness
- `ShadowSoftness` - Shadow edge softness
- `SunriseTime` / `SunsetTime` - Time-of-day transitions
- `CoronaIntensity` - Sun glow/corona
- `LensFlareIntensity` - Lens flare effect
- `GodRaysIntensity` - Volumetric light shafts

**Example:**
```toml
[Position]
AutoRotate = { type = "bool", value = true }

[TimeOfDay]
SunriseTime = { type = "float", value = 6.0 }
SunsetTime = { type = "float", value = 18.0 }

[Advanced]
GodRaysIntensity = { type = "float", value = 0.3 }
```

### 5. Skybox.toml (NEW)
Cubemap-based sky rendering with texture IDs.

**Key Properties:**
- `FrontTextureId` / `BackTextureId` - Cubemap face textures (+Z, -Z)
- `LeftTextureId` / `RightTextureId` - Cubemap face textures (-X, +X)
- `TopTextureId` / `BottomTextureId` - Cubemap face textures (+Y, -Y)
- `Color` - Skybox tint color
- `Brightness` - Brightness multiplier
- `Rotation` - Y-axis rotation in degrees
- `AutoRotate` - Auto-rotate skybox
- `BlendMode` - Blend with procedural sky ("Replace", "Additive", "Multiply", "Overlay")
- `Preset` - Built-in presets ("Space", "Desert", "Ocean", "Mountains", "City", "Nebula", "Sunset")
- `HDR` - Use HDR skybox for lighting
- `Distance` - Skybox distance for parallax

**Example:**
```toml
[Textures]
FrontTextureId = { type = "string", value = "rbxasset://textures/sky/space_ft.png" }
BackTextureId = { type = "string", value = "rbxasset://textures/sky/space_bk.png" }
# ... other faces

[Appearance]
Rotation = { type = "float", value = 0.0, min = 0.0, max = 360.0 }
AutoRotate = { type = "bool", value = false }

[Presets]
Preset = { type = "enum", value = "Space", options = ["None", "Space", "Desert", ...] }
```

## Space Creation

When creating a new Space via **File → New**, the system:

1. Creates `Lighting/` folder
2. Copies templates from `assets/lighting_templates/` to `Lighting/`
3. Generates 5 TOML files:
   - `Atmosphere.toml`
   - `Moon.toml`
   - `Sky.toml`
   - `Sun.toml`
   - `Skybox.toml`

**Code Location:** `eustress/crates/engine/src/space/space_ops.rs` - `scaffold_new_space()`

```rust
let lighting_children = ["Atmosphere", "Moon", "Sky", "Sun", "Skybox"];
for child_name in &lighting_children {
    let template_path = template_dir.join(format!("{}.toml", child_name));
    let target_path = space_root.join("Lighting").join(format!("{}.toml", child_name));
    
    if let Ok(template_content) = std::fs::read_to_string(&template_path) {
        write_file(&target_path, &template_content)?;
    }
}
```

## Loading System

The instance loader automatically discovers and loads TOML files in the `Lighting/` folder:

1. **Scan** - `Lighting/` folder is scanned for `.toml` files
2. **Parse** - Each TOML file is parsed into property sections
3. **Spawn** - ECS entities are created with components matching the TOML properties
4. **Sync** - Properties panel displays TOML-defined properties
5. **Write-back** - Edits in Properties panel update the TOML files on disk

**Code Location:** `eustress/crates/engine/src/space/instance_loader.rs`

## Property Format

All Lighting children use the same TOML property format:

```toml
[SectionName]
PropertyName = { 
    type = "float",           # Property type (float, int, bool, string, Color3, Vector2, Vector3, enum)
    value = 1.0,              # Default value
    min = 0.0,                # Optional: minimum value
    max = 10.0,               # Optional: maximum value
    options = ["A", "B"],     # Optional: enum options
    description = "..."       # Optional: tooltip description
}
```

**Supported Types:**
- `float` - Floating-point number
- `int` - Integer
- `bool` - Boolean (true/false)
- `string` - Text string
- `Color3` - RGB color `[r, g, b]` (0.0-1.0)
- `Vector2` - 2D vector `[x, y]`
- `Vector3` - 3D vector `[x, y, z]`
- `enum` - Enumeration with `options` array

## Migration from Binary

**Before (Binary):**
- Lighting children were hardcoded entities spawned at runtime
- Properties stored in ECS components only
- Not git-diffable or human-readable
- Required code changes to add new properties

**After (TOML):**
- Lighting children are TOML files in `Lighting/` folder
- Properties defined in human-readable TOML format
- Fully git-diffable and mergeable
- New properties added by editing TOML templates
- File-system-first architecture alignment

**Backward Compatibility:**
- Old Spaces without TOML files fall back to legacy generation
- `sky_toml()` and `atmosphere_toml()` functions provide fallback
- Gradual migration path for existing Spaces

## Roblox Comparison

Eustress Lighting system is **inspired by Roblox** but with **greater utility**:

### Similarities
- Service-based organization (Lighting service)
- Named children (Atmosphere, Sky, Sun, Moon)
- Property-based configuration
- Time-of-day system with ClockTime

### Enhancements
- **TOML files** instead of binary XML
- **Skybox** with 6-face cubemap support and HDR
- **Cloud system** with coverage, density, speed, altitude
- **Advanced scattering** with Rayleigh/Mie coefficients
- **God rays** (volumetric light shafts)
- **Lens flare** system
- **Auto-phase** moon cycles
- **Preset skyboxes** (Space, Desert, Ocean, etc.)
- **Blend modes** for skybox compositing
- **Git-diffable** configuration files

## Best Practices

### 1. Use Templates
Always start from the auto-generated templates when creating new Spaces. They provide sensible defaults.

### 2. Version Control
Commit Lighting TOML files to git. They're human-readable and merge-friendly.

### 3. Presets
Use Skybox presets for quick environment setup:
```toml
[Presets]
Preset = { type = "enum", value = "Space" }
```

### 4. Time-of-Day
Enable `Sun.AutoRotate` and sync with `Lighting.ClockTime` for dynamic day/night cycles.

### 5. Performance
- Disable `CastLight` on Moon if moonlight isn't needed
- Reduce `StarCount` for better performance
- Use lower `CloudDensity` for faster rendering

## File Structure Example

```
MySpace/
├── Lighting/
│   ├── _service.toml          # Lighting service metadata
│   ├── Atmosphere.toml        # Atmospheric scattering
│   ├── Moon.toml              # Lunar body
│   ├── Sky.toml               # Procedural/textured sky
│   ├── Sun.toml               # Solar body
│   └── Skybox.toml            # Cubemap skybox
├── Workspace/
│   └── Baseplate.part.toml
└── space.toml
```

## Future Enhancements

- **Volumetric clouds** with 3D noise
- **Aurora borealis** effects
- **Weather system** integration
- **HDR skybox** auto-exposure
- **Procedural stars** with constellations
- **Multiple suns/moons** for alien worlds
- **Atmospheric perspective** fog
- **Light pollution** simulation for cities

## References

- **EEP Specification:** `docs/development/EEP_SPECIFICATION.md`
- **File-System-First:** `docs/development/FILE_SYSTEM_FIRST.md`
- **Space Operations:** `eustress/crates/engine/src/space/space_ops.rs`
- **Templates:** `eustress/crates/engine/assets/lighting_templates/`
