# Players

**Category:** Player  
**Class Name:** `Players`  
**Learn URL:** `/learn/services/players`

## Overview

Players is a runtime-only service that contains a Player object for each
connected client. It provides events for PlayerAdded/PlayerRemoving and methods
like GetPlayers(). In Studio, it shows no players since you are in edit mode.

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `MaxPlayers` | int | `50` | Maximum number of concurrent players in the server. |
| `RespawnTime` | float | `5.0` | Seconds to wait before respawning a player after death. |
| `CharacterAutoLoads` | bool | `true` | Automatically load character model when player joins. |

## Key Responsibilities

- **Player Tracking** — A `Player` object is automatically created when a
  client connects and removed when they disconnect. Each Player holds
  references to their Character, PlayerGui, Backpack, and other per-player
  containers.

- **MaxPlayers** — Sets the server capacity. Once this limit is reached, new
  connection attempts are rejected or queued.

- **RespawnTime** — After a character dies, the engine waits this many seconds
  before automatically respawning the player at a SpawnLocation.

- **CharacterAutoLoads** — When `true`, the engine automatically creates and
  loads a character model for each player. Set to `false` for lobby systems
  where you want to control when characters appear.

## Events

| Event | Description |
|-------|-------------|
| `PlayerAdded(player)` | Fires when a new player connects. |
| `PlayerRemoving(player)` | Fires just before a player disconnects. |

## Usage Example (Rune)

```rune
let players = game.get_service("Players");

players.PlayerAdded.connect(|player| {
    print("Welcome, " + player.Name);
});

players.PlayerRemoving.connect(|player| {
    print("Goodbye, " + player.Name);
});
```

## Related Services

- [StarterPlayer](starter-player.md) — Default settings for new players
- [StarterGui](starter-gui.md) — GUI cloned into each player's PlayerGui
- [StarterPack](starter-pack.md) — Tools given to each player
- [Teams](teams.md) — Team assignments for players
