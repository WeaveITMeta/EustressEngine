# StarterGui

**Category:** Player  
**Class Name:** `StarterGui`  
**Learn URL:** `/learn/services/starter-gui`

## Overview

StarterGui holds ScreenGui, BillboardGui, and SurfaceGui objects that are
automatically copied into each player's PlayerGui when they join or respawn.
This is the standard way to create HUD elements, health bars, inventory
screens, and menus.

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `ScreenCompatibilityMode` | enum | `TextScaleDpi` | GUI scaling mode for different screen sizes. |
| `ShowDevelopmentGui` | bool | `false` | Display developer-only GUI elements. |
| `ResetOnSpawn` | bool | `true` | Reset player GUI to StarterGui contents on respawn. |

## Key Responsibilities

- **GUI Distribution** — Every ScreenGui placed inside StarterGui is
  automatically cloned into each player's PlayerGui container. This happens
  on join and, if `ResetOnSpawn` is true, on every respawn.

- **Screen Scaling** — `ScreenCompatibilityMode` determines how GUI elements
  scale across different screen sizes and DPI settings. `TextScaleDpi` is
  the modern default that respects display scaling.

- **Development GUI** — `ShowDevelopmentGui` toggles visibility of GUIs
  marked as developer-only. Use this for debug overlays that should not
  appear in production.

- **Respawn Behavior** — When `ResetOnSpawn` is true, the player's GUI is
  destroyed and re-cloned from StarterGui on each character respawn. Set to
  `false` for persistent HUD elements that survive death.

## Common Patterns

### Health Bar HUD
Place a ScreenGui containing a health bar Frame inside StarterGui. The bar
updates via a LocalScript that reads `Character.Humanoid.Health`.

### Inventory System
Create a ScreenGui with an inventory grid. Use `ResetOnSpawn = false` so
the inventory persists across deaths.

### Loading Screen
Add a ScreenGui with `DisplayOrder = 999` that covers the entire screen.
A LocalScript inside removes it after assets finish loading.

## Usage Example (Rune)

```rune
let starter_gui = game.get_service("StarterGui");

// Keep GUI across respawns
starter_gui.ResetOnSpawn = false;
```

## Related Services

- [Players](players.md) — Each player has a PlayerGui container
- [StarterPlayer](starter-player.md) — Camera and character defaults
- [StarterPack](starter-pack.md) — Tools given alongside GUI
