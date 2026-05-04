# ReplicatedStorage

**Category:** Data  
**Class Name:** `ReplicatedStorage`  
**Learn URL:** `/learn/services/replicated-storage`

## Overview

ReplicatedStorage is a container whose contents are visible to both server and
all clients. Place ModuleScripts, models, animations, sounds, and other assets
here that need to be accessible from both server scripts and local scripts.

## Properties

ReplicatedStorage has no configurable properties. It is a pure container
service whose contents are automatically replicated to all connected clients.

## Key Responsibilities

- **Shared Assets** — Models, sounds, animations, and particle effects that
  both server and client scripts need access to should live here.

- **Shared ModuleScripts** — Utility libraries, configuration modules, and
  shared game logic that run on both server and client.

- **RemoteEvents / RemoteFunctions** — The standard location for
  client-server communication objects. Both server scripts and local scripts
  can access RemoteEvents placed here.

- **Replication** — Everything inside ReplicatedStorage is downloaded to every
  client. This means clients **can see all contents** — do not store
  sensitive data or server-only logic here.

## Common Patterns

### Shared Configuration
```rune
// ModuleScript in ReplicatedStorage
let config = {
    max_health: 100,
    walk_speed: 16,
    jump_height: 7.2,
};
return config;
```

### Client-Server Communication
Place a RemoteEvent named "DamageEvent" in ReplicatedStorage. The server
script listens for it; the client fires it when an attack lands.

### Asset Library
Store Model templates (weapons, NPCs, vehicles) here. Server scripts clone
them into Workspace as needed; client scripts can preview them in menus.

## Security Considerations

**ReplicatedStorage is NOT secure.** Clients can read everything inside it.
Never store:
- Secret configuration values (API keys, admin passwords)
- Server-only game logic (anti-cheat, economy calculations)
- Hidden content that players should not see yet

Use [ServerStorage](server-storage.md) for sensitive data.

## Related Services

- [ServerStorage](server-storage.md) — Server-only storage (invisible to clients)
- [ServerScriptService](server-script-service.md) — Server-only scripts
- [Workspace](workspace.md) — Where replicated assets are instantiated
