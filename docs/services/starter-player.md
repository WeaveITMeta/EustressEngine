# StarterPlayer

**Category:** Player  
**Class Name:** `StarterPlayer`  
**Learn URL:** `/learn/services/starter-player`

## Overview

StarterPlayer defines the default configuration applied to every player when
they join. It contains two sub-containers: StarterPlayerScripts (scripts cloned
into each Player) and StarterCharacterScripts (scripts cloned into each
Character).

## Properties

### Camera Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `CameraMaxZoomDistance` | float | `128` | Maximum camera distance from character. |
| `CameraMinZoomDistance` | float | `0.5` | Minimum camera distance from character. |
| `CameraMode` | enum | `Classic` | Camera behavior: Classic, LockFirstPerson, LockThirdPerson. |
| `DevCameraOcclusionMode` | enum | `Zoom` | How camera handles occluding geometry. |
| `DevComputerCameraMode` | enum | `UserChoice` | Camera mode override for desktop clients. |
| `DevTouchCameraMode` | enum | `UserChoice` | Camera mode override for touch clients. |
| `EnableMouseLockOption` | bool | `true` | Allow players to toggle mouse lock (Shift-Lock). |

### Character Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `AutoJumpEnabled` | bool | `true` | Character auto-jumps when walking into obstacles. |
| `CharacterJumpHeight` | float | `7.2` | Maximum jump height in studs. |
| `CharacterJumpPower` | float | `50` | Initial upward velocity when jumping. |
| `CharacterMaxSlopeAngle` | float | `89` | Steepest surface angle the character can walk on (degrees). |
| `CharacterWalkSpeed` | float | `16` | Default walk speed in studs per second. |
| `HealthDisplayDistance` | float | `100` | Distance at which health bars become visible. |
| `NameDisplayDistance` | float | `100` | Distance at which player names become visible. |

## Sub-Containers

### StarterPlayerScripts

Scripts placed here are cloned into each `Player` object when the player joins.
Use this for client-side logic that persists across respawns (e.g., camera
controllers, input handlers, HUD managers).

### StarterCharacterScripts

Scripts placed here are cloned into each `Character` model when it spawns. Use
this for character-specific logic (e.g., footstep sounds, animation
controllers). These scripts are destroyed and re-created on each respawn.

## Key Responsibilities

- **Camera Configuration** — Control the default camera behavior for all
  players. `CameraMode` sets the initial mode; players can switch if
  `EnableMouseLockOption` is true.

- **Character Movement** — `CharacterWalkSpeed` and `CharacterJumpHeight`
  define the default movement feel. Adjust these per-genre (e.g., faster for
  action games, slower for horror).

- **Slope Limits** — `CharacterMaxSlopeAngle` prevents characters from walking
  up steep surfaces. Set to `45` for realistic terrain traversal.

- **Display Distances** — `HealthDisplayDistance` and `NameDisplayDistance`
  control when overhead UI elements appear for other players' characters.

## Usage Example (Rune)

```rune
let starter = game.get_service("StarterPlayer");

// First-person only
starter.CameraMode = "LockFirstPerson";
starter.CameraMinZoomDistance = 0;
starter.CameraMaxZoomDistance = 0;

// Slower, more deliberate movement
starter.CharacterWalkSpeed = 10;
starter.CharacterJumpHeight = 5;
```

## Related Services

- [Players](players.md) — Runtime player instances
- [StarterGui](starter-gui.md) — Default GUI for players
- [StarterPack](starter-pack.md) — Default tools for players
