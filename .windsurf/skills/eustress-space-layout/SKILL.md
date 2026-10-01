---
name: eustress-space-layout
description: >-
  Explains how an Eustress Space is laid out on disk and in the Explorer: the
  Universe and Space folders, every service and what belongs in it, the script
  folder convention (SoulScript folders with a Rojo-suffixed source), which scripts
  run in Play and where, instance folders and `_instance.toml`, meshes, sounds,
  Space-wide settings in `_service.toml`, and plain `.toml` config files. Use when
  the user asks "where do I put this", "why doesn't my script run", "why can't I see
  my script in the Explorer", "how is a Space organised", or when you are about to
  write Space files by hand instead of through the Eustress MCP tools.
---

# How an Eustress Space is laid out

A Space is a folder of folders. Every instance is a folder with an `_instance.toml`
inside, every service is a top-level folder, and scripts are folders too. Studio's file
watcher loads changes while it runs, so a file you write shows up in the Explorer.

## Where Spaces live

```
Documents/Eustress/<Universe>/
  Spaces/
    <Space>/
      Workspace/            the 3D world
      Lighting/  Players/  StarterGui/  StarterPack/
      StarterPlayer/        player settings; its StarterPlayerScripts/ folder holds client scripts
      ReplicatedStorage/  ServerStorage/  ServerScriptService/
      SoulService/  SoundService/  MaterialService/  Teams/  Chat/ ...
      meshes/               .glb files the Space's parts reference (optional)
      .eustress/            Studio's own data (settings, output.log); leave it to Studio
```

`new_space` creates all of this. Each service folder holds a `_service.toml` that marks
it as a service and carries its settings.

## What each service is for

| Service | Put here | Scripts here run in Play? |
|---|---|---|
| Workspace | everything in the world: maps, spawns, props | server scripts: yes |
| ServerScriptService | the game's server scripts | server scripts: yes |
| SoulService | Eustress's own home for scripts; `create_script` defaults server scripts here | server scripts: yes |
| StarterPlayer | player settings (`_service.toml`), custom characters in `Characters/`, and a `StarterPlayerScripts/` folder for client scripts (input, camera, HUD) | client scripts in `StarterPlayerScripts/`: yes, for the local player |
| StarterPlayerScripts | some Spaces have it as a top-level service instead; it works the same | client scripts: yes |
| StarterGui | ScreenGuis copied into each player's PlayerGui | client scripts inside run from the copy |
| ReplicatedStorage | shared modules, RemoteEvents, shared values, templates both sides clone | only when required |
| ServerStorage | server-only templates (enemy meshes, loot) | no |
| SoundService | `.wav` files; each becomes a Sound named after its file | no |
| Lighting | Sky, Atmosphere, Sun, time of day | no |
| StarterCharacterScripts | leave empty for now; put character logic in a client or server script | no |

Parts inside ReplicatedStorage, ServerStorage, ServerScriptService, SoulService,
StarterPack and StarterPlayer are hidden and have no collision during Play: they are
templates. Clone them into Workspace to use them.

## The script convention

Every script is a folder named after the script:

```
ServerScriptService/
  GameDirector/
    _instance.toml
    GameDirector.server.luau     the source
    GameDirector.md              a short human summary (Studio shows it)
```

The `_instance.toml`:

```toml
[metadata]
class_name = "SoulScript"
archivable = true
name = "GameDirector"

[properties]
language = "luau"

[script]
enabled = true
run_context = "Server"
source = "GameDirector.server.luau"
```

| Kind | Source file | `run_context` | Where it usually lives |
|---|---|---|---|
| Luau server script | `<Name>.server.luau` | `"Server"` | ServerScriptService, SoulService, Workspace |
| Luau client script | `<Name>.client.luau` | `"Client"` | StarterPlayerScripts |
| Luau module | `<Name>.module.luau` | `"Module"` | ReplicatedStorage (shared) or ServerStorage (server only) |
| Rune script | `<Name>.rune` | `"Rune"` | ServerScriptService or SoulService |

Rules that keep scripts working:

- **Use `create_script`**. It writes exactly this shape: `create_script {name, code,
  language: "luau", kind: "server" | "client" | "module", parent: "ReplicatedStorage/Shared"}`.
  `kind` picks the suffix and `run_context`; `parent` is any folder inside the Space.
- **Always set `[script] source` and `run_context`** when writing by hand. An empty
  `source` means "no code", and the script is skipped.
- **The suffix and `run_context` must agree.** Play decides server, client or module from
  the source's suffix.
- **A bare `.luau` file dropped straight into a service is not the convention.** It may
  load, but it is not a script folder: it gets no summary, and tools that expect folders
  pass it by. Make a folder.
- **Script edits reach the next Play**, not the one running. Stop, then Play again.
- To find and read scripts: `list_scripts` lists every script source in the Space's
  services as Space-relative paths, and `read_script {name: "GameDirector"}` reads one.

## Instances (parts, models, folders)

A part is a folder with an `_instance.toml`:

```toml
[metadata]
class_name = "Part"
archivable = true
name = "Floor"

[properties]
anchored = true
can_collide = true
color = [0.35, 0.37, 0.4, 1.0]
material = "Concrete"
transparency = 0.0

[transform]
position = [0.0, -0.5, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [60.0, 1.0, 60.0]
```

- `scale` is the part's size in meters. Positions are meters.
- Children are sub-folders: a Model folder holding part folders is a model with parts.
- **Prefer `create_entity`** (it writes this file, a fresh uuid and the right mesh) and
  `update_entity` to change properties. The MCP `write_file` tool refuses
  `_instance.toml` on purpose, so it cannot damage an instance.
- A custom mesh: put the `.glb` in `<Space>/meshes/` and give the part an `[asset]` block.
  The mesh path is relative to the part's own folder:

  ```toml
  [asset]
  mesh = "../../../meshes/zombie_boss.glb"   # from ServerStorage/ZombieMeshes/Boss/
  scene = "Scene0"
  ```

## Sounds

Drop `.wav` files into `SoundService/`. `Cash.wav` becomes a Sound named `Cash`:

```lua
local s = game:GetService("SoundService"):FindFirstChild("Cash"):Clone()
s.Volume = 0.6
s.Parent = game:GetService("SoundService")
s:Play()
```

## Space-wide settings: `_service.toml`

Each service's settings sit in its `_service.toml` under `[properties]`. For example
`StarterPlayer/_service.toml` holds the movement switches every player gets:

```toml
[properties]
jump_enabled = true
sprint_enabled = true
climbing_enabled = true
vaulting_enabled = false
ledge_leap_enabled = false
wall_jump_enabled = true
roll_enabled = false
```

Edit these in Studio's Properties panel (select the service) or in the file. Details in
`eustress-characters`.

`Players/_service.toml` holds `character_auto_loads`: set it to `false` for a game played
with no avatar (Pong, a board game), and Play starts without a body.

## Your own config files

A plain `.toml` file you place in a Space (for level data, tuning tables, notes) is left
alone: Studio does not treat it as an instance and never rewrites it. Only files named
`_instance.toml` and `_service.toml` are instances and services.

## When something is missing from the Explorer

1. Is it a folder with an `_instance.toml` (instances, scripts) or a `_service.toml`
   (services)? Loose files are not instances.
2. Is its `[metadata] class_name` a real class (`Part`, `Model`, `Folder`, `SoulScript`)?
3. Expand the service row: services with children show an arrow.
4. Still missing: check Studio's Output panel for a load warning naming the file.
