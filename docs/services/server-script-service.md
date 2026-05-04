# ServerScriptService

**Category:** Scripting  
**Class Name:** `ServerScriptService`  
**Learn URL:** `/learn/services/server-script-service`

## Overview

ServerScriptService holds Script objects that run on the server when the
experience starts. Scripts here have full server authority — they can access
ServerStorage, manage datastores, handle physics, and control game state.
LocalScripts placed here will NOT run.

## Properties

ServerScriptService has no configurable properties. It is a pure container
for server-side Script objects.

## Key Responsibilities

- **Game Initialization** — Place startup scripts here that set up the game
  world, load saved data, and configure services.

- **DataStore Management** — Scripts that read and write persistent player
  data (inventory, progress, currency) belong here.

- **Physics Authority** — Server scripts have authority over physics
  simulation. Anti-cheat validation, damage calculation, and game state
  management all run from here.

- **Event Handlers** — Server-side listeners for RemoteEvents fired by
  clients. Validate all client input here before applying changes.

- **Script Types** — Only `Script` objects run here. `LocalScript` and
  `ModuleScript` objects placed directly in ServerScriptService will not
  execute (though ModuleScripts can be `require()`d by Scripts).

## Common Patterns

### Game Manager
```rune
// ServerScriptService/GameManager (Script)
let players = game.get_service("Players");
let replicated = game.get_service("ReplicatedStorage");

players.PlayerAdded.connect(|player| {
    // Load player data
    let data = load_data(player);
    
    // Set up character
    player.CharacterAdded.connect(|character| {
        character.Humanoid.MaxHealth = data.max_health;
        character.Humanoid.Health = data.max_health;
    });
});
```

### Remote Event Handler
```rune
// Validate client requests on the server
let damage_event = replicated.DamageEvent;
damage_event.OnServerEvent.connect(|player, target, amount| {
    // Validate: is the player close enough to hit?
    let distance = (player.Character.Position - target.Position).Magnitude;
    if distance > 10 {
        return; // Too far — reject
    }
    target.Humanoid.Health -= amount;
});
```

## Security Considerations

- Always validate client input in server scripts
- Never trust values sent via RemoteEvents without checking
- Keep game-critical logic server-side to prevent exploits
- Use rate limiting on RemoteEvent handlers

## Related Services

- [ServerStorage](server-storage.md) — Server-only assets and modules
- [ReplicatedStorage](replicated-storage.md) — Shared assets and RemoteEvents
- [SoulService](soul-service.md) — Rune VM and scripting configuration
- [Players](players.md) — Player connection events
