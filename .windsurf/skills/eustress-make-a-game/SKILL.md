---
name: eustress-make-a-game
description: >-
  Takes someone from a game idea to a playable, fun game in Eustress, starting from
  nothing. Use when the user says "make a game", "start a new game", "I have a game
  idea", "prototype a game", or names a kind of game (obby, tycoon, wave defense, arena
  shooter, collectathon, time trial, top-down shooter) they want to build in Eustress.
  It pins the idea, picks a starter shape, creates the Space, blocks out the world,
  writes the rules in Luau or Rune, plays it and tunes it until it is fun. For depth on
  one part it hands off to eustress-space-layout, eustress-scripting, eustress-game-ui,
  eustress-characters and eustress-playtest.
---

# Make a game in Eustress, from nothing

The goal is a game the user can play in the first session, and then a fun one, reached by
playing it and changing one thing at a time. Keep the momentum: build a thin slice, play
it, look at it, tune it, show the user.

## 0. What you work with

- **Eustress Studio**, with the user's Space open.
- **The Eustress MCP server**, which gives you tools such as `new_universe`, `new_space`,
  `create_entity`, `update_entity`, `create_script`, `list_scripts`, `read_script`,
  `play_input`, `capture_viewport` and `read_output`. Prefer these tools: the engine's
  file watcher picks up everything they write while Studio runs.
- **Plain files** as the fallback. A Space is folders plus TOML plus script sources, so
  every step below can also be done by writing files (the layout is in
  `eustress-space-layout`).

## 1. Pin the idea in three lines

Write these and show them to the user before building:

- **Verb**: what the player does most. Jump, shoot, build, collect, drive, dodge.
- **Goal**: how a round is won. Reach the flag, survive wave 10, earn $10,000, beat 60 s.
- **Pressure**: what pushes back. A timer, enemies, lava, a shrinking floor, a rival score.

When the user gives only a genre, fill in the three lines with sensible defaults and say
what you picked. Ask one question at most; a default the user can change beats a quiz.

## 2. Pick a starter shape

| Shape | World (parts in Workspace) | Server script owns | Client script owns |
|---|---|---|---|
| Obby | platforms, kill bricks, checkpoints, a finish pad | checkpoint reached (distance check), fall or kill brick sends the player to the last checkpoint, run timer | timer, checkpoint toast |
| Wave defense / arena shooter | an arena, spawn points, cover | waves, NPC enemies (`Humanoid:Move`), hits by `workspace:Raycast`, damage, score | aim at the mouse, fire through a RemoteEvent, HUD |
| Tycoon | a plot, pads with prices, a dropper or income source | cash per second, standing on a pad buys its item, unlocks | cash counter, a BillboardGui price tag over each pad |
| Collectathon | coins spread over a map, a goal | pickup by distance, score, win at N | counter, a sound and a pop on pickup |
| Time trial | a track with ordered gates | gate order, lap and best time | timer, split times |
| Top-down shooter | an arena seen from above | the same as wave defense | an orthographic camera that follows the player |
| Paddle or board game (Pong, puzzles, cards) | a table of Parts, pieces the server moves | all rules; reads each player's input attribute | keys to an attribute, a fixed camera; set `character_auto_loads = false` in `Players/_service.toml` so nobody has a body |

Start with the smallest version: one level, one enemy type, one way to score.

## 3. Make the place

1. `new_universe` with a name, when the game should get its own Universe (optional).
2. `new_space` with the game's name, and `universe` set to the Universe's absolute path
   (for example `C:/Users/<you>/Documents/Eustress/Games`). It scaffolds every service
   folder: Workspace, Lighting, Players, StarterGui, StarterPack, StarterPlayer,
   ReplicatedStorage, ServerStorage, ServerScriptService, SoulService, SoundService and
   the rest. The Workspace starts empty, so the first part you make is the ground.
3. Open the Space in Studio. The user can open it from the Universe browser, or Studio can
   start on it: `eustress-engine --space "<absolute path to the Space>"`.

## 4. Block out the world

Use `create_entity`. Eustress works in meters: `position` and `size` are meters, and a
player character stands about 1.8 m tall.

- Make a container first, then its parts: `create_entity {class: "Model", name: "Map"}`,
  then `create_entity {name: "Floor", parent: "Map", size: [60, 1, 60], position: [0, -0.5, 0], material: "Concrete"}`.
- World parts are anchored by default. Pass `anchored: false` for anything physics should
  move.
- Name what the scripts will look for, and group it: `Workspace/Map`, `Workspace/Spawns`,
  `Workspace/Coins`, `Workspace/Pads`. Scripts find them with `workspace:WaitForChild("Coins")`.
- Look after every few parts: `capture_viewport`. Check proportions against the player
  (a doorway taller than a person, a step lower than a knee), not only placement.
- Measure a jump in Play before spacing platforms (see `eustress-playtest`).
- For hills, caves and water, use Studio's Terrain tab; scripts can reshape that terrain
  during Play (`eustress-scripting`, Terrain).

## 5. Write the rules

Three scripts carry most games. Create them with `create_script` (details and the full API
in `eustress-scripting`):

| Script | `create_script` arguments | Owns |
|---|---|---|
| Config | `language: "luau", kind: "module", parent: "ReplicatedStorage/Shared"` | every number worth tuning |
| GameDirector | `language: "luau", kind: "server", parent: "ServerScriptService"` | rules, state, spawning, damage, scoring |
| ClientController | `language: "luau", kind: "client"` | input, camera, HUD, effects |

The server decides; the client asks and shows. Clients send intent through RemoteEvents
("fire", "buy", "jump pad") and read results from Player attributes and values in
`ReplicatedStorage`. Keep that split even though Play runs one local player today: the
game then carries over to multiplayer unchanged.

A complete first slice, a coin hunt, fits in about 60 lines:

```lua
-- ReplicatedStorage/Shared/Config (module)
local Config = {}
Config.PickupRadius = 2.5
Config.CoinsToWin = 10
return Config
```

```lua
-- ServerScriptService/GameDirector (server)
local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Config = require(ReplicatedStorage:WaitForChild("Shared"):WaitForChild("Config"))
local Coins = workspace:WaitForChild("Coins")

local function setupPlayer(player)
	player:SetAttribute("Coins", 0)
end
Players.PlayerAdded:Connect(setupPlayer)
for _, p in ipairs(Players:GetPlayers()) do -- the player can exist before this script starts
	setupPlayer(p)
end

RunService.Heartbeat:Connect(function()
	for _, player in ipairs(Players:GetPlayers()) do
		local root = player.Character and player.Character:FindFirstChild("HumanoidRootPart")
		if root then
			for _, coin in ipairs(Coins:GetChildren()) do
				if coin:IsA("BasePart") and (coin.Position - root.Position).Magnitude < Config.PickupRadius then
					coin:Destroy()
					local n = player:GetAttribute("Coins") + 1
					player:SetAttribute("Coins", n)
					if n >= Config.CoinsToWin then
						player:SetAttribute("Won", true)
					end
				end
			end
		end
	end
end)
```

```lua
-- StarterPlayerScripts/ClientController (client)
local Players = game:GetService("Players")
local player = Players.LocalPlayer

local gui = Instance.new("ScreenGui")
gui.Name = "HUD"
gui.ResetOnSpawn = false
local label = Instance.new("TextLabel")
label.Size = UDim2.new(0, 240, 0, 48)
label.Position = UDim2.new(0, 16, 0, 16)
label.BackgroundColor3 = Color3.fromRGB(14, 16, 22)
label.BackgroundTransparency = 0.2
label.TextColor3 = Color3.fromRGB(255, 214, 80)
label.TextSize = 28
label.Parent = gui
gui.Parent = player:WaitForChild("PlayerGui")

local function refresh()
	local won = player:GetAttribute("Won")
	label.Text = won and "You win!" or ("Coins: " .. tostring(player:GetAttribute("Coins") or 0))
end
player:GetAttributeChangedSignal("Coins"):Connect(refresh)
player:GetAttributeChangedSignal("Won"):Connect(refresh)
refresh()
```

Then add a floor, scatter ten small `Neon` parts in a `Workspace/Coins` model and press
Play.

## 6. Play it

Follow `eustress-playtest`. The short loop:

1. Start Play: the user presses F5 in Studio (ask them to, when you work over MCP alone),
   or with a shell, `eustress bridge --pid <pid> call action.invoke --params '{"action":"PlayWithCharacter"}'`.
2. `read_output {level: "warn"}` shows script errors and warnings. Fix those first.
3. `play_input` walks and acts: `{down: ["W"]}`, then `{up: ["W"]}`; `{tap: ["Space"]}` jumps;
   `{cursor: [x, y], tap: ["MouseButton1"]}` clicks at a point on the viewport.
4. `capture_viewport` shows you the frame. Look at it: is the HUD readable, is the player
   the right size, did the thing happen?
5. Stop Play (F8, or `"StopPlay"` over the bridge). Stop restores the Space to how it was.

Script edits take effect at the next Play, so stop, edit, and play again.

## 7. Make it fun

Check these every slice:

- **Five seconds**: the player does the verb within five seconds of Play.
- **Feedback on every input**: a sound, a flash, a number, the same frame.
- **A ramp**: waves grow, timers shrink, prices and rewards climb.
- **A reason to go again**: a score, a best time, an unlock.
- **Juice**: sounds (drop `.wav` files in `SoundService`), particles, a little camera shake,
  floating numbers, price tags on BillboardGuis (`eustress-game-ui`).
- **Tuning lives in Config**: change one number per Play, and say which one to the user.

## 8. Grow in slices

Build in this order, and play after each slice:

1. The core loop (the verb works and feels good).
2. Win and lose (the round can end).
3. Progression (waves, levels, purchases).
4. Juice (sound, particles, feedback).
5. UI polish (a HUD that reads at a glance).
6. More content (enemy types, levels, items).

After each slice, show the user a capture and say in one line what changed and what the
next slice is. When something in Eustress itself misbehaves, say so plainly and keep a
list, separate from the game's own bugs.
