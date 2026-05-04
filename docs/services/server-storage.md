# ServerStorage

**Category:** Data  
**Class Name:** `ServerStorage`  
**Learn URL:** `/learn/services/server-storage`

## Overview

ServerStorage is a container whose contents are only accessible to server-side
scripts. Clients cannot see, access, or replicate anything stored here. Use it
for server-only modules, secret configurations, and assets that should not be
downloaded by clients.

## Properties

ServerStorage has no configurable properties. It is a pure container service
that is invisible to all clients.

## Key Responsibilities

- **Server-Only Modules** — Game logic that must remain hidden from clients
  (anti-cheat validation, economy calculations, matchmaking algorithms).

- **Secret Configuration** — API keys, admin lists, server-side constants
  that clients should never see.

- **Templates** — Models, weapons, and NPCs that server scripts clone into
  Workspace at runtime. Since they are not replicated until cloned, clients
  cannot inspect them ahead of time.

- **Security** — Anything in ServerStorage is guaranteed invisible to
  exploiters. This is the safest container for sensitive assets.

## Common Patterns

### Server-Only Game Logic
```rune
// Script in ServerScriptService
let economy = require(game.get_service("ServerStorage").EconomyModule);
economy.award_currency(player, 100);
```

### Template Spawning
Store NPC models in ServerStorage. A server script clones them into Workspace
when a wave starts. Clients only see them after they appear in the world.

### Admin Configuration
```rune
// ServerStorage/AdminConfig (ModuleScript)
let config = {
    admin_ids: [12345, 67890],
    ban_list: [],
    debug_mode: false,
};
return config;
```

## Security Considerations

ServerStorage is **fully secure** — clients never download its contents.
Use it for:
- Anti-cheat validation logic
- Economy and progression calculations
- Admin tools and commands
- Any asset that should remain hidden until explicitly spawned

## Related Services

- [ReplicatedStorage](replicated-storage.md) — Shared storage visible to all clients
- [ServerScriptService](server-script-service.md) — Server-only scripts
