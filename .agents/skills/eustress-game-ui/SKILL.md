---
name: eustress-game-ui
description: >-
  Builds game UI in Eustress with Luau: a HUD on a ScreenGui (health, ammo, cash, wave,
  timers, toasts, a death screen), and BillboardGuis that float over parts in the world
  (price tags on shop pads, name plates, damage numbers). Covers layout with UDim2,
  readable text, colours, when to repaint, and the rules that keep the HUD fast. Use
  when the user asks for a HUD, a scoreboard, a menu, labels over objects, price tags,
  health bars, "the UI is ugly", or "the screen flickers".
---

# Game UI in Eustress

Two surfaces:

- **ScreenGui** in the player's `PlayerGui`: the HUD, drawn flat over the view. Build it in
  the client script.
- **BillboardGui** attached to a part: a small panel that floats over the part and always
  faces the camera. Build it wherever the part is made (usually the server script).

Both take the usual children, such as `Frame`, `TextLabel` and `TextButton`
(`MouseButton1Click` and `Activated` fire on click).

## A HUD that reads at a glance

Put the HUD together once at the start of the client script, with small helpers:

```lua
local player = game:GetService("Players").LocalPlayer

local PANEL = Color3.fromRGB(14, 16, 22)
local WHITE = Color3.fromRGB(255, 255, 255)
local ACCENT = Color3.fromRGB(255, 196, 64)

local gui = Instance.new("ScreenGui")
gui.Name = "HUD"
gui.ResetOnSpawn = false

local function frame(props)
	local f = Instance.new("Frame")
	f.Name = props.Name or "Frame"
	f.Size = props.Size
	f.Position = props.Position or UDim2.new(0, 0, 0, 0)
	f.AnchorPoint = props.AnchorPoint or Vector2.new(0, 0)
	f.BackgroundColor3 = props.Color or PANEL
	f.BackgroundTransparency = props.Transparency or 0
	f.BorderSizePixel = 0
	f.Parent = props.Parent or gui
	return f
end

local function label(props)
	local l = Instance.new("TextLabel")
	l.Name = props.Name or "Label"
	l.Size = props.Size
	l.Position = props.Position or UDim2.new(0, 0, 0, 0)
	l.BackgroundTransparency = 1
	l.Text = props.Text or ""
	l.TextColor3 = props.TextColor or WHITE
	l.TextSize = props.TextSize or 18
	l.Font = Enum.Font.GothamBold
	l.TextXAlignment = props.Align or Enum.TextXAlignment.Center
	l.Parent = props.Parent or gui
	return l
end

-- Bottom-left: health bar. Top-center: wave. Top-right: cash.
local hpBack = frame({ Size = UDim2.new(0, 260, 0, 18), Position = UDim2.new(0, 16, 1, -34), Color = Color3.fromRGB(34, 38, 48) })
local hpFill = frame({ Size = UDim2.new(1, 0, 1, 0), Color = Color3.fromRGB(90, 220, 110), Parent = hpBack })
local wave = label({ Size = UDim2.new(0, 300, 0, 40), Position = UDim2.new(0.5, -150, 0, 12), TextSize = 30, TextColor = ACCENT })
local cash = label({ Size = UDim2.new(0, 200, 0, 32), Position = UDim2.new(1, -216, 0, 12), TextSize = 26, Align = Enum.TextXAlignment.Right })

gui.Parent = player:WaitForChild("PlayerGui")
```

Layout with `UDim2.new(xScale, xOffset, yScale, yOffset)`: scale is a fraction of the
screen, offset is pixels. Anchor to corners with scale 0 or 1 plus a pixel inset, as above,
so the HUD holds its shape at any window size.

### Updating it without slowing the game

Every GUI property write repaints the HUD, and a repaint costs roughly 13 to 17 ms. Write
only when a value changed:

```lua
-- UDim2 and other userdata have no value equality (two equal UDim2s compare
-- unequal), so they are compared by their text.
local lastSet = setmetatable({}, { __mode = "k" })
local function set(obj, prop, value)
	local cache = lastSet[obj]
	if not cache then
		cache = {}
		lastSet[obj] = cache
	end
	local key = if type(value) == "userdata" then tostring(value) else value
	if cache[prop] ~= key then
		cache[prop] = key
		obj[prop] = value
	end
end

-- A bar fill, rounded to `steps` so it repaints only when it visibly moves.
local function setBar(fill, fraction, steps)
	local q = math.floor(math.clamp(fraction, 0, 1) * steps + 0.5) / steps
	set(fill, "Size", UDim2.new(q, 0, 1, 0))
end

game:GetService("RunService").RenderStepped:Connect(function()
	local humanoid = player.Character and player.Character:FindFirstChild("Humanoid")
	if humanoid then
		setBar(hpFill, humanoid.Health / humanoid.MaxHealth, 40)
	end
	set(cash, "Text", "$" .. tostring(player:GetAttribute("Cash") or 0))
end)
```

Better still for values that change rarely: connect to
`player:GetAttributeChangedSignal("Cash")` and update only then.

- Round numbers you show (`math.floor`), or a label repaints every frame for a digit
  nobody can read.
- Keep tweens (`TweenService`) for rare moments: a wave banner, a purchase, a death screen.
  A HUD that tweens every frame repaints every frame.
- Avoid full-screen flashes between rounds; players read them as flicker. Announce with a
  banner that slides or fades in one area instead.

### Toasts

```lua
local toast = label({ Size = UDim2.new(0, 520, 0, 36), Position = UDim2.new(0.5, -260, 0.22, 0), TextSize = 24 })
local toastUntil = 0
game:GetService("ReplicatedStorage"):WaitForChild("Remotes"):WaitForChild("Notify").OnClientEvent:Connect(function(text, color, seconds)
	toast.Text = text
	toast.TextColor3 = color or WHITE
	toast.Visible = true
	toastUntil = time() + (seconds or 2)
end)
-- in RenderStepped: if toast.Visible and time() > toastUntil then toast.Visible = false end
```

## BillboardGuis over parts

A price tag over a shop pad:

```lua
local function padTag(pad, title, price)
	local tag = Instance.new("BillboardGui")
	tag.Name = "Tag"
	tag.Size = UDim2.new(5, 0, 1.6, 0)                   -- 5 m wide, 1.6 m tall
	tag.StudsOffsetWorldSpace = Vector3.new(0, 2.2, 0)   -- lift in world meters
	tag.AlwaysOnTop = true
	tag.MaxDistance = 150
	local plate = Instance.new("Frame")
	plate.Size = UDim2.new(1, 0, 1, 0)
	plate.BackgroundColor3 = Color3.fromRGB(14, 16, 22)
	plate.BackgroundTransparency = 0.25
	plate.BorderSizePixel = 0
	plate.Parent = tag
	local what = Instance.new("TextLabel")
	what.Size = UDim2.new(1, 0, 0.55, 0)
	what.BackgroundTransparency = 1
	what.Text = title
	what.TextColor3 = Color3.fromRGB(255, 255, 255)
	what.TextSize = 22
	what.Font = Enum.Font.GothamBold
	what.ZIndex = 2
	what.Parent = plate
	local cost = Instance.new("TextLabel")
	cost.Size = UDim2.new(1, 0, 0.45, 0)
	cost.Position = UDim2.new(0, 0, 0.55, 0)
	cost.BackgroundTransparency = 1
	cost.Text = price == 0 and "FREE" or ("$" .. price)
	cost.TextColor3 = Color3.fromRGB(255, 214, 90)
	cost.TextSize = 20
	cost.Font = Enum.Font.GothamBold
	cost.ZIndex = 2
	cost.Parent = plate
	tag.Parent = pad                                     -- or set tag.Adornee = pad
	return tag
end
```

The rules that make billboards readable:

- **Size is in meters.** In a BillboardGui's `Size`, scale 1 is one meter (50 px per meter),
  so `UDim2.new(5, 0, 1.6, 0)` is a 5 m by 1.6 m panel.
- **Lift with `StudsOffsetWorldSpace`.** `StudsOffset` is in the part's own axes and gets
  multiplied by the part's size: on a thin 0.18 m pad a 2 m offset comes out at 0.36 m.
- **`AlwaysOnTop = true`** when the tag sits on or inside its part, or the part hides it.
- **`MaxDistance`** around 100 to 200 m, so a wide shot is free of label clutter.
- **One or two short lines per tag** (about 20 characters each). Put longer text in the HUD.
- **Big labels**: above about 4 m, set a fixed `TextSize` instead of `TextScaled`; scaled
  text stops growing at 72 px and reads smaller on a big panel.
- Hide a tag with `tag.Enabled = false`, or `Destroy()` it when the item is bought.
- Damage numbers: a small BillboardGui with one TextLabel, parented to the hit part, raised
  and faded over half a second, then removed with `Debris:AddItem(tag, 0.6)`.

## Look at it

After any UI change, Play and `capture_viewport`. Check: can you read every number from
arm's length, does anything overlap, is the HUD clear of the action in the middle of the
screen, and does the frame rate hold while it updates.
