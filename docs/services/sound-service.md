# SoundService

**Category:** Rendering  
**Class Name:** `SoundService`  
**Learn URL:** `/learn/services/sound-service`

## Overview

SoundService controls the global audio environment. It manages the distance
attenuation model, Doppler effect, and rolloff settings. Sound objects in the
world reference these global settings for spatialized 3D audio.

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `AmbientReverb` | enum | `NoReverb` | Environment reverb preset (Cave, Hall, Room, etc.). |
| `DistanceFactor` | float | `3.33` | Scale factor mapping studs to audio distance units. |
| `DopplerScale` | float | `1.0` | Doppler effect intensity. 0 = disabled. |
| `RolloffScale` | float | `1.0` | How quickly sounds fade with distance. Higher = faster. |
| `VolumetricAudio` | bool | `true` | Enable spatial 3D audio processing. |

## Key Responsibilities

- **Ambient Reverb** — Sets a global reverb preset that simulates the acoustic
  properties of the environment. Options include NoReverb, Cave, Hall, Room,
  Forest, and more. This affects all Sound objects in the Space.

- **Distance Attenuation** — `DistanceFactor` scales how distance in studs
  maps to audio distance units. Combined with `RolloffScale`, this controls
  how quickly sounds fade as the listener moves away.

- **Doppler Effect** — `DopplerScale` controls the intensity of pitch shifting
  for moving sound sources. Set to `0` to disable the Doppler effect entirely.

- **Spatial Audio** — When `VolumetricAudio` is true, sounds are processed
  with 3D spatialization (HRTF), providing realistic directional audio
  through headphones.

## Reverb Presets

| Preset | Use Case |
|--------|----------|
| `NoReverb` | Outdoors, open spaces |
| `Cave` | Underground, tunnels |
| `Hall` | Large indoor spaces, churches |
| `Room` | Standard indoor rooms |
| `Forest` | Dense vegetation, muffled |
| `Underwater` | Submerged, heavy filtering |
| `Arena` | Large open venues |
| `Bathroom` | Small tiled rooms |

## Usage Example (Rune)

```rune
let sound_service = game.get_service("SoundService");

// Cave environment
sound_service.AmbientReverb = "Cave";
sound_service.RolloffScale = 1.5;  // Sounds fade faster in caves

// Disable Doppler for a puzzle game
sound_service.DopplerScale = 0;
```

## Related Services

- [Lighting](lighting.md) — Visual environment pairs with audio mood
- [Workspace](workspace.md) — Sound objects are placed in the 3D world
