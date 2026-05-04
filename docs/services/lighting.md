# Lighting

**Category:** Core  
**Class Name:** `Lighting`  
**Learn URL:** `/learn/services/lighting`

## Overview

Lighting is the global illumination service. It drives the sun position (via
ClockTime and GeographicLatitude), ambient color, shadow quality, exposure, fog,
and atmosphere effects. It also hosts child objects like Atmosphere, Sky, and
post-processing effects.

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `Ambient` | Color3 | `0, 0, 0` | Ambient light color applied everywhere regardless of sun. |
| `Brightness` | float | `2` | Overall scene brightness multiplier. |
| `ClockTime` | float | `14.0` | Time of day (0–24). Controls sun position and sky color. |
| `ColorShift_Bottom` | Color3 | `0, 0, 0` | Color tint applied to surfaces facing downward. |
| `ColorShift_Top` | Color3 | `0, 0, 0` | Color tint applied to surfaces facing upward. |
| `EnvironmentDiffuseScale` | float | `1` | Multiplier for environment map diffuse contribution. |
| `EnvironmentSpecularScale` | float | `1` | Multiplier for environment map specular reflections. |
| `ExposureCompensation` | float | `0` | EV bias for HDR tone mapping. Positive = brighter. |
| `FogColor` | Color3 | `0.75, 0.75, 0.75` | Color of distance fog. |
| `FogEnd` | float | `100000` | Distance at which fog reaches full opacity. |
| `FogStart` | float | `0` | Distance at which fog begins to appear. |
| `GeographicLatitude` | float | `41.7` | Latitude for sun angle calculation. Affects shadow length. |
| `GlobalShadows` | bool | `true` | Whether shadow maps are computed for directional light. |
| `ShadowSoftness` | float | `0.2` | Blur radius for shadow edges. 0 = sharp, 1 = very soft. |

## Key Responsibilities

- **Time-of-Day Cycle** — `ClockTime` ranges from 0 to 24 and determines the
  sun's position in the sky. Combine with `GeographicLatitude` to get
  realistic shadow angles for your scene's location.

- **Shadows** — `GlobalShadows` toggles shadow map rendering. `ShadowSoftness`
  controls the penumbra blur radius for softer, more natural shadows.

- **Fog** — `FogStart` and `FogEnd` define the distance range over which fog
  fades in. `FogColor` sets the fog's tint. Use dense fog for horror or
  atmospheric scenes.

- **Exposure** — `ExposureCompensation` adjusts the HDR tone mapping curve.
  Increase for brighter outdoor scenes; decrease for darker interiors.

- **Environment Maps** — `EnvironmentDiffuseScale` and
  `EnvironmentSpecularScale` control how much the skybox contributes to
  indirect lighting and reflections.

## Child Objects

Lighting can contain these child objects for advanced effects:

- **Atmosphere** — Realistic atmospheric scattering (Rayleigh + Mie)
- **Sky** — Custom skybox with six-face textures or procedural sky
- **BloomEffect** — HDR bloom post-processing
- **ColorCorrectionEffect** — Color grading and LUT
- **SunRaysEffect** — God rays from the sun
- **DepthOfFieldEffect** — Camera focal blur

## Usage Example (Rune)

```rune
let lighting = game.get_service("Lighting");

// Sunset scene
lighting.ClockTime = 18.5;
lighting.FogColor = Color3.new(1.0, 0.6, 0.3);
lighting.FogEnd = 500;
lighting.ShadowSoftness = 0.5;
```

## Related Services

- [Workspace](workspace.md) — Physics and world settings
- [SoundService](sound-service.md) — Audio environment pairs with lighting mood
