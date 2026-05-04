# Services Documentation

Every Eustress Space ships with a set of **default services** that manage
physics, lighting, players, scripting, data storage, audio, and rendering.
Services are singleton objects — each Space has exactly one instance of each
service, created automatically when the Space loads.

## Categories

### Core
| Service | Description |
|---------|-------------|
| [Workspace](workspace.md) | Root container for all 3D objects, physics, and world settings |
| [Lighting](lighting.md) | Sun, sky, shadows, fog, and ambient illumination |
| [Chat](chat.md) | In-game text chat, bubble chat, and message filtering |

### Player
| Service | Description |
|---------|-------------|
| [Players](players.md) | Runtime tracking of all connected players |
| [StarterPlayer](starter-player.md) | Default camera, character, and control settings |
| [StarterGui](starter-gui.md) | GUI objects cloned into each player's PlayerGui |
| [StarterPack](starter-pack.md) | Tools and items given to each player on spawn |
| [Teams](teams.md) | Team management with colored team assignments |

### Data
| Service | Description |
|---------|-------------|
| [ReplicatedStorage](replicated-storage.md) | Shared storage replicated to all clients |
| [ServerStorage](server-storage.md) | Server-only storage invisible to clients |

### Scripting
| Service | Description |
|---------|-------------|
| [ServerScriptService](server-script-service.md) | Container for server-side scripts |
| [SoulService](soul-service.md) | Rune VM script execution and AI integration |

### Rendering
| Service | Description |
|---------|-------------|
| [MaterialService](material-service.md) | PBR material presets and texture management |
| [SoundService](sound-service.md) | Global audio settings and spatial audio |
| [AdornmentService](adornment-service.md) | Visual adornments, highlights, and billboards |

## Opening the Services Browser

In Eustress Studio, go to **Help → Services Browser** or press the Services
Browser button in the toolbar. The browser shows every service with its
properties, descriptions, and links to this documentation.
