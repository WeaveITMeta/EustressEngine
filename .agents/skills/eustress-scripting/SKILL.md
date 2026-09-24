---
name: eustress-scripting
description: >-
  Writes game scripts for Eustress in Luau and Rune that run in Play: the server and
  client split, RemoteEvents and BindableEvents, attributes and shared values, frame
  loops, input and mouse aim, the camera (including a top-down orthographic one),
  raycasts, NPC enemies with Humanoids, tweens, sounds, and the Rune `eustress::dm`
  module that shares the same live world. Includes ready recipes (waves, pickups,
  health and respawn, shops, cooldowns) and the traps that cost real time. Use when
  writing or debugging any Eustress script, when the user asks "how do I script X in
  Eustress", "my script doesn't run", or wants Luau and Rune to work together.
---

# Scripting games in Eustress

Eustress runs two languages on one live world during Play:

- **Luau**, with the Roblox-style API: `game:GetService`, `Instance.new`, `workspace`,
  `Players.LocalPlayer`, events with `:Connect`. Most game logic goes here.
- **Rune**, through `use eustress::dm;`. It reads and writes the same instances, so a part
  Rune recolours reads back recoloured in Luau in the same frame, and a BindableEvent Rune
  fires reaches Luau's `Event:Connect` handlers.

Create scripts with `create_script` (see `eustress-space-layout` for the folder shape):

```
create_script {name: "GameDirector", language: "luau", kind: "server", parent: "ServerScriptService", code: "..."}
create_script {name: "ClientController", language: "luau", kind: "client", code: "..."}
create_script {name: "Config", language: "luau", kind: "module", parent: "ReplicatedStorage/Shared", code: "..."}
create_script {name: "Bounty", language: "rune", parent: "ServerScriptService", code: "..."}
```

## Lifecycle

- Scripts start when Play starts and stop when Play stops; Stop restores every instance to
  how the Space had it, so scripts can create, move and destroy freely.
- Server scripts start before client scripts, each group in name order. Use `WaitForChild`
  for anything another script creates.
- **Edits reach the next Play.** Stop, save, Play again.
- `print` and `warn` go to Studio's Output panel. A runtime error shows there with its
  script's full name, such as `ServerScriptService.GameDirector`, and a line number.

## The server and client split

The server owns the rules. The client reads input, draws the HUD, moves the camera, and
asks the server to act.

```lua
-- Server: make the remotes (find-or-create, so a restart never duplicates them)
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Remotes = ReplicatedStorage:FindFirstChild("Remotes") or Instance.new("Folder")
Remotes.Name = "Remotes"
Remotes.Parent = ReplicatedStorage

local function remote(name)
	local r = Remotes:FindFirstChild(name)
	if not r then
		r = Instance.new("RemoteEvent")
		r.Name = name
		r.Parent = Remotes
	end
	return r
end

local Fire = remote("Fire")       -- client -> server
local Notify = remote("Notify")   -- server -> client

Fire.OnServerEvent:Connect(function(player, origin, direction)
	-- validate, then apply the rules
end)
Notify:FireClient(player, "Wave 3", Color3.fromRGB(255, 220, 90))
```

```lua
-- Client
local Remotes = game:GetService("ReplicatedStorage"):WaitForChild("Remotes")
local Fire = Remotes:WaitForChild("Fire")
Remotes:WaitForChild("Notify").OnClientEvent:Connect(function(text, color)
	-- show a toast
end)
Fire:FireServer(origin, direction)
```

Play runs one local player today, with both sides in the same Studio process. Keep the
split anyway: it keeps the rules in one place and carries over to multiplayer.

### Shared state

- **Per player**: attributes on the Player. `player:SetAttribute("Cash", 120)` on the
  server; `player:GetAttribute("Cash")` and
  `player:GetAttributeChangedSignal("Cash"):Connect(fn)` on the client.
- **Per game**: value objects in a folder, such as `ReplicatedStorage/GameState` holding an
  `IntValue` "Wave", a `StringValue` "Phase" and a `NumberValue` "Timer". Read `.Value`.
- **Numbers to tune**: one `Config` module both sides `require`.
- **Between Luau and Rune**: attributes and BindableEvents. Rune cannot `require` a Luau
  module, so put what Rune needs on an instance as attributes.

## Players and characters

```lua
local Players = game:GetService("Players")

local function setupPlayer(player)
	player:SetAttribute("Cash", 0)
	local function onCharacter(character)
		local humanoid = character:WaitForChild("Humanoid", 5)
		if humanoid then
			humanoid.WalkSpeed = 8          -- meters per second
			humanoid.MaxHealth = 100
			humanoid.Health = 100
		end
	end
	player.CharacterAdded:Connect(onCharacter)
	if player.Character then
		task.spawn(onCharacter, player.Character)
	end
end

Players.PlayerAdded:Connect(setupPlayer)
for _, p in ipairs(Players:GetPlayers()) do -- the player may already be here
	setupPlayer(p)
end
```

- The character's root is `character:FindFirstChild("HumanoidRootPart")`; move it with
  `root.CFrame = CFrame.new(position)`.
- **Respawn by hand**: reset `humanoid.Health` and set the root's CFrame to a spawn point.
  `player:LoadCharacter()` only logs a request today.
- Movement switches (jump, sprint, climb) and custom bodies: `eustress-characters`.

## Frame loops and timing

```lua
local RunService = game:GetService("RunService")
RunService.Heartbeat:Connect(function(dt) end)      -- server: game rules, AI
RunService.RenderStepped:Connect(function(dt) end)  -- client: camera, aim, HUD
```

- `time()` is seconds since Play began; use it for cooldowns (`if time() >= readyAt`).
- `task.wait(s)`, `task.spawn(fn)`, `task.delay(s, fn)` for sequences.
- `game:GetService("Debris"):AddItem(instance, seconds)` removes something later.

## Input, mouse and camera (client)

```lua
local UserInputService = game:GetService("UserInputService")
local player = game:GetService("Players").LocalPlayer
local mouse = player:GetMouse()

UserInputService.InputBegan:Connect(function(input, processed)
	if processed then return end
	if input.KeyCode == Enum.KeyCode.R then
		-- reload
	elseif input.UserInputType == Enum.UserInputType.MouseButton1 then
		-- fire
	end
end)
mouse.WheelForward:Connect(function() end)
```

- `mouse.Hit` is where the cursor points in the world; `mouse.UnitRay` is the ray itself.
- Aim on flat ground: intersect `mouse.UnitRay` with the plane at gun height,
  `t = (h - ray.Origin.Y) / ray.Direction.Y`, point `= ray.Origin + ray.Direction * t`.

Top-down orthographic camera that follows the player:

```lua
local camera = workspace.CurrentCamera
camera.CameraType = Enum.CameraType.Scriptable
camera.Projection = Enum.CameraProjection.Orthographic
camera.OrthographicSize = 32                    -- meters of world visible top to bottom
local OFFSET = Vector3.new(0, 60, 26)            -- a steep view from above, looking north

RunService.RenderStepped:Connect(function(dt)
	local root = player.Character and player.Character:FindFirstChild("HumanoidRootPart")
	if root then
		camera.CFrame = CFrame.lookAt(root.Position + OFFSET, root.Position)
	end
end)
```

## Hits and physics queries

```lua
local params = RaycastParams.new()
params.FilterType = Enum.RaycastFilterType.Exclude
params.FilterDescendantsInstances = { shooterCharacter, workspace.Effects }
local result = workspace:Raycast(origin, direction * range, params)
if result then
	-- result.Instance, result.Position, result.Normal
end
```

`workspace:GetPartBoundsInRadius(center, radius, params)` finds parts near a point
(explosions, auras). For pickups and pads, a distance check against the character's root
each Heartbeat is simpler and fully predictable. `part.Touched` fires for parts that
physics moves into each other.

## NPC enemies

A Model with an unanchored `HumanoidRootPart` and a `Humanoid` walks under script control:

```lua
local function makeEnemy(position)
	local model = Instance.new("Model")
	model.Name = "Zombie"
	local body = Instance.new("Part")      -- or a mesh template cloned from ServerStorage
	body.Name = "HumanoidRootPart"
	body.Size = Vector3.new(1, 2.2, 1)
	body.Color = Color3.fromRGB(90, 160, 80)
	body.Anchored = false
	body.CFrame = CFrame.new(position + Vector3.new(0, 1.15, 0))
	body.Parent = model
	local humanoid = Instance.new("Humanoid")
	humanoid.MaxHealth = 50
	humanoid.Health = 50
	humanoid.WalkSpeed = 3.5
	humanoid.Parent = model
	model.PrimaryPart = body
	model.Parent = workspace.Enemies
	humanoid:Move(Vector3.zero)   -- registers it now, so it stays upright from the start
	return model, humanoid, body
end
```

- Each Heartbeat, steer with `humanoid:Move(direction)` (a unit vector), or send it to a
  point with `humanoid:MoveTo(position)` and listen to `humanoid.MoveToFinished`.
- Keep your own table of live enemies (`{model, humanoid, root, hp, dead}`) and loop over
  it, rather than scanning the workspace every frame.
- When an enemy dies, mark it dead in your table first; your hit code skips the dead.
  Destroy the model after its death effect, with `Debris:AddItem(model, 2)`.

## Effects, tweens and sounds

```lua
local TweenService = game:GetService("TweenService")
TweenService:Create(part, TweenInfo.new(0.25), { Transparency = 1 }):Play()

local s = game:GetService("SoundService"):FindFirstChild("Explosion"):Clone()
s.Volume = 0.6
s.Parent = game:GetService("SoundService")
s:Play()
game:GetService("Debris"):AddItem(s, 4)
```

For things made many times a second (bullets, tracers, sparks), keep a pool of parts and
move them, instead of creating and destroying a part per shot.

## Rune

```rune
use eustress::dm;

// Runs every frame of Play. Each call starts fresh, so keep state in attributes.
pub fn on_update(dt) {
    let events = dm::find_path("ReplicatedStorage.Events");
    if events == 0 {
        return;
    }
    let down = dm::key_down("Q");
    if down == dm::get_attribute_bool(events, "QDown") {
        return; // act on the press, not every held frame
    }
    dm::set_attribute_bool(events, "QDown", down);
    if down {
        let blast = dm::find_child(events, "Shockwave");
        if blast != 0 {
            dm::fire(blast, "Q"); // a BindableEvent Luau listens to
        }
    }
}
```

- Entry points: `main()`, `on_init()`, `on_ready()`, `on_update(dt)`, `on_exit()`.
- Instances are `i64` ids and `0` means none.
- Finding: `dm::root()`, `dm::service(name)`, `dm::find_path("Workspace.Zombies")`,
  `dm::find_child(parent, name)`, `dm::children(i)`, `dm::descendants(i)`,
  `dm::tagged(tag)`, `dm::exists(i)`, `dm::parent(i)`, `dm::instance_name(i)`,
  `dm::class_name(i)`, `dm::is_a(i, class)`.
- Properties: `dm::get_number / set_number`, `get_bool / set_bool`,
  `get_string / set_string`, `get_vector3 / set_vector3`, `get_position / set_position`,
  `dm::set_color(i, r, g, b)` with 0 to 1 channels.
- Attributes: `dm::get_attribute_number / set_attribute_number`, and the `_bool` and
  `_string` versions.
- Making things: `dm::create(class)`, `dm::set_parent(i, parent)`, `dm::clone_instance(i)`,
  `dm::destroy(i)`, `dm::fire(event, message)`.
- World and input: `dm::now()`, `dm::delta()`, `dm::random()`, `dm::random_index(n)`,
  `dm::key_down("Q")`, `dm::mouse_down("MouseButton1")`, `dm::mouse_position()`, `dm::mouse_hit()`,
  `dm::mouse_target()`, `dm::log(text)`.

A good split: Luau runs the game, and Rune adds self-contained systems (a bounty that
picks a random enemy every 25 s, an ability on a key) that talk to Luau through attributes
and BindableEvents.

## Traps that cost real time

- **`UDim2` values have no `==`.** Comparing two of them is always false, so a "set only
  if changed" cache rewrites every frame. Compare `tostring(a) == tostring(b)`, or cache
  the numbers you built it from.
- **Every GUI property write repaints the HUD** (about 13 to 17 ms each time it happens).
  Write a label only when its text actually changes, and keep tweens for rare moments.
- **Changing `CanQuery` after a part spawns has no effect.** To make raycasts pass through
  something (a corpse, your own effects), exclude it in `RaycastParams` or skip it in
  your hit code.
- **The player can exist before your script runs.** Always loop over
  `Players:GetPlayers()` after connecting `PlayerAdded`.
- **`LoadCharacter` does not respawn yet.** Reset Health and move the root.
- **Rune has no memory between frames.** Globals reset every `on_update`; use attributes.
- **An enemy in the air is still "near".** Before a contact hit, check height as well as
  horizontal distance, so enemies flung by an explosion cannot hurt from above.
- **Stuck NPCs**: track each one's progress; when it has barely moved for a second,
  steer it sideways or toward the next waypoint for a moment.
