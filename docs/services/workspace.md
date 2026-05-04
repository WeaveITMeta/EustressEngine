# Workspace

**Category:** Core  
**Class Name:** `Workspace`  
**Learn URL:** `/learn/services/workspace`

## Overview

Workspace is the top-level service that holds every Part, Model, and Script in
the 3D world. It controls global physics (gravity, air density, wind), the
fallen-parts destroy height, streaming behavior, and collision groups. Every
Space has exactly one Workspace.

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `Gravity` | float | `196.2` | Acceleration due to gravity in studs/s². Earth-like default. |
| `FallenPartsDestroyHeight` | float | `-500` | Y coordinate below which parts are automatically destroyed. |
| `AirDensity` | float | `0.0012` | Air density for aerodynamic drag calculations. |
| `GlobalWind` | Vector3 | `0, 0, 0` | Wind vector (X, Y, Z) affecting particles and cloth. |
| `AllowThirdPartySales` | bool | `false` | Whether third-party marketplace sales are permitted. |
| `SignalBehavior` | enum | `Default` | How RemoteEvent signals are queued and delivered. |
| `TouchesUseCollisionGroups` | bool | `false` | Whether `.Touched` events respect collision group filters. |

## Key Responsibilities

- **Gravity and Physics** — The `Gravity` property sets the downward
  acceleration for all unanchored parts. Set to `0` for zero-gravity
  environments or increase for heavier worlds.

- **GlobalWind** — A world-space wind vector that affects particle emitters,
  cloth simulations, and any physics body with aerodynamic drag enabled.

- **FallenPartsDestroyHeight** — Parts that fall below this Y coordinate are
  automatically destroyed. This prevents runaway parts from consuming memory.

- **AllowThirdPartySales** — Controls whether third-party developers can sell
  items inside your Space via the marketplace.

- **SignalBehavior** — Determines how `RemoteEvent:FireServer()` and
  `RemoteEvent:FireClient()` calls are queued when the recipient is not yet
  ready (e.g., during loading).

- **TouchesUseCollisionGroups** — When enabled, the `.Touched` event only fires
  between parts whose collision groups allow contact.

## Usage Example (Rune)

```rune
// Set gravity to moon-like
let workspace = game.get_service("Workspace");
workspace.Gravity = 32.7;

// Enable wind
workspace.GlobalWind = Vector3.new(10, 0, 5);
```

## Related Services

- [Lighting](lighting.md) — Controls visual environment
- [Players](players.md) — Manages connected players
- [MaterialService](material-service.md) — Material presets for parts in Workspace
