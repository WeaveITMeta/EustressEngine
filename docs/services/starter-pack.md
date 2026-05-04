# StarterPack

**Category:** Player  
**Class Name:** `StarterPack`  
**Learn URL:** `/learn/services/starter-pack`

## Overview

StarterPack contains Tool objects that are cloned into every player's Backpack
when they join or respawn. Use this for default weapons, building tools, or any
item players should start with.

## Properties

StarterPack has no configurable properties. It is a pure container service.

## Key Responsibilities

- **Tool Distribution** — Every Tool object placed inside StarterPack is
  automatically cloned into each player's Backpack. This happens on join and
  on every respawn.

- **Default Loadout** — Use StarterPack for items every player should have
  from the start (e.g., a flashlight, a starter weapon, a building tool).

- **Respawn Behavior** — Tools are re-cloned fresh on each respawn. If a
  player drops or destroys a tool, they get it back after dying.

## Common Patterns

### Starter Weapon
Place a Tool with a Handle (Part) and a damage Script inside StarterPack.
Every player spawns with the weapon in their Backpack.

### Building Tool
Create a Tool that spawns parts on click. Place it in StarterPack so all
players can build by default.

### Flashlight
A Tool with a SpotLight attached to the Handle. Players can equip it to
illuminate dark environments.

## Usage Example (Rune)

```rune
// StarterPack is a container — add Tool objects to it in Studio.
// At runtime, you can also add tools programmatically:
let starter_pack = game.get_service("StarterPack");
let sword = create_tool("Sword");
sword.Parent = starter_pack;
// Now all new players will receive this sword
```

## Related Services

- [Players](players.md) — Each player has a Backpack container
- [StarterPlayer](starter-player.md) — Character and camera defaults
- [StarterGui](starter-gui.md) — GUI counterpart to StarterPack
