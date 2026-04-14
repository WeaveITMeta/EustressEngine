# Eustress Scripting API Master Checklist

Comprehensive comparison of Roblox Luau API vs Eustress implementation status for both **Luau** and **Rune** scripting languages.

**Goal**: Feature parity with Roblox scripting API where applicable, with Eustress-specific extensions for physics simulation, AI, and cross-platform deployment.

**Legend**:
- ✅ Implemented
- 🔶 Partial/Stub
- ❌ Not Started
- ➖ Not Applicable (Roblox-specific, won't implement)
- 🔷 Eustress Extension (not in Roblox)

---

## Table of Contents

1. [Data Types](#1-data-types)
2. [Global Functions](#2-global-functions)
3. [Instance API](#3-instance-api)
4. [Workspace API](#4-workspace-api)
5. [Services](#5-services)
6. [Events & Signals](#6-events--signals)
7. [Physics & Constraints](#7-physics--constraints)
8. [UI/GUI](#8-uigui)
9. [Sound & Media](#9-sound--media)
10. [Networking](#10-networking)
11. [Data Persistence](#11-data-persistence)
12. [Animation](#12-animation)
13. [Character & Humanoid](#13-character--humanoid)
14. [Camera](#14-camera)
15. [Input](#15-input)
16. [Eustress Extensions](#16-eustress-extensions)

---

## 1. Data Types

### Vector Types

| Roblox Type | Luau Status | Rune Status | Notes |
|-------------|-------------|-------------|-------|
| `Vector3.new(x, y, z)` | ✅ UserData | ✅ Struct | Both have proper Vector3 type |
| `Vector3.X/Y/Z` fields | ✅ UserData | ✅ Native | |
| `Vector3 + - * /` operators | ✅ Metamethods | ✅ Methods | `add()`, `sub()`, `mul()`, `div()` |
| `Vector3:Dot()` | ✅ | ✅ | |
| `Vector3:Cross()` | ✅ | ✅ | |
| `Vector3.Magnitude` | ✅ | ✅ | |
| `Vector3.Unit` | ✅ | ✅ | |
| `Vector3:Lerp()` | ✅ | ✅ | |
| `Vector2.new(x, y)` | ❌ | ❌ | In shared types module |
| `Vector2int16` | ❌ | ❌ | |
| `Vector3int16` | ❌ | ❌ | |

### Transform Types

| Roblox Type | Luau Status | Rune Status | Notes |
|-------------|-------------|-------------|-------|
| `CFrame.new()` | ✅ UserData | ✅ | Position-only constructor |
| `CFrame.Angles()` | ✅ | ✅ | Euler angles constructor |
| `CFrame.fromEulerAngles()` | ✅ | ✅ | Same as Angles |
| `CFrame.lookAt()` | ✅ | ✅ | |
| `CFrame * CFrame` | ✅ Metamethod | ✅ | `mul()` method |
| `CFrame:Inverse()` | ✅ | ✅ | |
| `CFrame:ToWorldSpace()` | ✅ | ✅ | `point_to_world_space()` |
| `CFrame:ToObjectSpace()` | ✅ | ✅ | `point_to_object_space()` |
| `CFrame.Position` | ✅ | ✅ | |
| `CFrame.LookVector` | ✅ | ✅ | |
| `CFrame.RightVector` | ✅ | ✅ | |
| `CFrame.UpVector` | ✅ | ✅ | |

### Color Types

| Roblox Type | Luau Status | Rune Status | Notes |
|-------------|-------------|-------------|-------|
| `Color3.new(r, g, b)` | ✅ UserData | ✅ | 0-1 floats |
| `Color3.fromRGB(r, g, b)` | ✅ | ✅ | 0-255 integers |
| `Color3.fromHSV(h, s, v)` | ✅ | ✅ | |
| `Color3.fromHex()` | ✅ | ✅ | Eustress extension |
| `Color3:Lerp()` | ✅ | ✅ | |
| `Color3:ToHSV()` | ✅ | ✅ | |
| `BrickColor.new()` | ➖ | ➖ | Deprecated, use Color3 |

### UI Types

| Roblox Type | Luau Status | Rune Status | Notes |
|-------------|-------------|-------------|-------|
| `UDim.new(scale, offset)` | ✅ UserData | ✅ | P1 implemented |
| `UDim2.new(xs, xo, ys, yo)` | ✅ UserData | ✅ | P1 implemented |
| `UDim2.fromScale()` | ✅ | ✅ | |
| `UDim2.fromOffset()` | ✅ | ✅ | |
| `UDim2:Lerp()` | ✅ | ✅ | |
| `Rect.new()` | ❌ | ❌ | |

### Other Types

| Roblox Type | Luau Status | Rune Status | Notes |
|-------------|-------------|-------------|-------|
| `Ray.new(origin, direction)` | ❌ | ❌ | Use Vector3 pair instead |
| `Region3.new()` | ❌ | ❌ | |
| `NumberRange.new()` | ❌ | ❌ | |
| `NumberSequence.new()` | ❌ | ❌ | For particles |
| `ColorSequence.new()` | ❌ | ❌ | For particles |
| `TweenInfo.new()` | ✅ UserData | ✅ | P1 implemented (all 6 params) |
| `Enum.*` | 🔶 Strings | 🔶 Strings | Use string literals |
| `Instance.new()` | ✅ | ✅ | P0 implemented |

---

## 2. Global Functions

| Roblox Function | Luau Status | Rune Status | Notes |
|-----------------|-------------|-------------|-------|
| `print(...)` | ✅ | ✅ `log_info` | Routes to Output panel |
| `warn(...)` | ✅ | ✅ `log_warn` | |
| `error(...)` | 🔶 | ✅ `log_error` | |
| `assert()` | ❌ | ❌ | |
| `type()` | ✅ Native | ❌ | Lua built-in |
| `typeof()` | ✅ | ❌ | Detects Vector3, CFrame, Color3, UDim2, TweenInfo, Instance |
| `tostring()` | ✅ Native | ❌ | |
| `tonumber()` | ✅ Native | ❌ | |
| `pairs()` | ✅ Native | ❌ | |
| `ipairs()` | ✅ Native | ❌ | |
| `next()` | ✅ Native | ❌ | |
| `select()` | ✅ Native | ❌ | |
| `unpack()` | ✅ Native | ❌ | |
| `pcall()` | ✅ Native | ❌ | |
| `xpcall()` | ✅ Native | ❌ | |
| `setmetatable()` | ✅ Native | ➖ | Lua-specific |
| `getmetatable()` | ✅ Native | ➖ | Lua-specific |
| `rawget/rawset` | ✅ Native | ➖ | Lua-specific |
| `require()` | 🔶 Stub | ❌ | ModuleScript loading |
| `wait(n)` | ✅ Stub | ❌ | Deprecated, use task.wait |
| `delay()` | ❌ | ❌ | Deprecated, use task.delay |
| `spawn()` | ❌ | ❌ | Deprecated |
| `tick()` | ❌ | ❌ | Unix timestamp |
| `time()` | ❌ | ❌ | Game time |
| `elapsedTime()` | ❌ | ❌ | |
| `os.time()` | ✅ Native | ❌ | |
| `os.date()` | ✅ Native | ❌ | |
| `os.clock()` | ✅ Native | ❌ | |

---

## 3. Instance API

### Instance Creation & Hierarchy

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Instance.new("ClassName")` | ✅ | ✅ | P0 implemented |
| `instance:Clone()` | ✅ | ✅ | `clone_instance()` |
| `instance:Destroy()` | ✅ | ✅ | |
| `instance:ClearAllChildren()` | ✅ | ❌ | |
| `instance.Parent` | ✅ | ✅ | `parent()` |
| `instance.Name` | ✅ | ✅ | `name()`, `set_name()` |
| `instance.ClassName` | ✅ | ✅ | `class_name()` |
| `instance:IsA("ClassName")` | ✅ | ✅ | `is_a()` |
| `instance:IsDescendantOf()` | ✅ | ❌ | |
| `instance:IsAncestorOf()` | ❌ | ❌ | |

### Instance Finding

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `instance:FindFirstChild()` | ✅ | ✅ | `find_first_child()` |
| `instance:FindFirstChildOfClass()` | ✅ | ✅ | `find_first_child_of_class()` |
| `instance:FindFirstChildWhichIsA()` | ❌ | ❌ | |
| `instance:FindFirstAncestorOfClass()` | ✅ | ❌ | |
| `instance:FindFirstAncestorWhichIsA()` | ✅ | ❌ | Includes inheritance |
| `instance:FindFirstAncestor()` | ✅ | ❌ | Walks Parent chain |
| `instance:FindFirstDescendant()` | ❌ | ❌ | |
| `instance:GetChildren()` | ✅ | ✅ | `get_children()` |
| `instance:GetDescendants()` | ✅ | ❌ | Recursive child traversal |
| `instance:WaitForChild()` | 🔶 Sync | ❌ | Immediate lookup (coroutine yield TODO) |
| `instance:GetFullName()` | ✅ | ❌ | Dot-separated path from root |
| `instance:GetAttribute()` | ✅ | ✅ `instance_get_attribute` | Custom key-value pairs |
| `instance:SetAttribute()` | ✅ | ✅ `instance_set_attribute` | Stored in memory |
| `instance:GetAttributes()` | ✅ | ❌ | Luau returns table of all attrs |

### Instance Events

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `instance.Changed` | ✅ | ❌ | Fires property name on any __newindex |
| `instance.ChildAdded` | ✅ | ❌ | Fires on Parent set |
| `instance.ChildRemoved` | ✅ | ❌ | Fires on Parent change |
| `instance.DescendantAdded` | 🔶 Signal | ❌ | Signal exists, not auto-fired yet |
| `instance.DescendantRemoving` | 🔶 Signal | ❌ | Signal exists, not auto-fired yet |
| `instance.AncestryChanged` | ✅ | ❌ | Fires on Parent reparenting |
| `instance:GetPropertyChangedSignal()` | ❌ | ❌ | |
| `instance:GetAttributeChangedSignal()` | ❌ | ❌ | |

---

## 4. Workspace API

### Raycasting (P0 - Implemented!)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `workspace:Raycast(origin, dir)` | ✅ | ✅ | Just implemented! |
| `workspace:Raycast(origin, dir, params)` | ✅ | ✅ | With RaycastParams |
| `RaycastParams.new()` | ✅ | ✅ | |
| `RaycastParams.FilterType` | 🔶 | ✅ | exclude_mode bool |
| `RaycastParams.FilterDescendantsInstances` | 🔶 Names | ✅ | Uses name strings |
| `RaycastParams.IgnoreWater` | ✅ | ✅ | |
| `RaycastParams.RespectCanCollide` | ✅ | ✅ | |
| `RaycastResult.Instance` | ✅ | ✅ `.instance` | |
| `RaycastResult.Position` | ✅ | ✅ `.position` | |
| `RaycastResult.Normal` | ✅ | ✅ `.normal` | |
| `RaycastResult.Distance` | ✅ | ✅ `.distance` | Eustress extension |
| `RaycastResult.Material` | ✅ | ✅ `.material` | |

### Spatial Queries

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `workspace:GetPartBoundsInBox()` | ✅ | ❌ | AABB overlap against registry |
| `workspace:GetPartBoundsInRadius()` | ✅ | ❌ | Sphere distance check |
| `workspace:GetPartsInPart()` | ✅ | ❌ | AABB overlap, skips self |
| `workspace:Blockcast()` | ✅ | ❌ | Swept AABB (20-step sample) |
| `workspace:Spherecast()` | ✅ | ❌ | Swept sphere-AABB (20-step sample) |
| `workspace:Shapecast()` | ✅ | ❌ | Uses part's own AABB as shape |

### Workspace Properties

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `workspace.Gravity` | ✅ Property | ✅ `workspace_get/set_gravity` | Default 9.80665 m/s² |
| `workspace.CurrentCamera` | ❌ | ❌ | |
| `workspace.DistributedGameTime` | ❌ | ❌ | |
| `workspace.Terrain` | ❌ | ❌ | |

---

## 5. Services

### RunService (P0 - Critical for game loops)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `RunService.Heartbeat` | ✅ | ❌ | Per-frame after physics (needs signal system) |
| `RunService.Stepped` | ✅ | ❌ | Per-frame before physics (needs signal system) |
| `RunService.RenderStepped` | ✅ | ❌ | Per-frame render (needs signal system) |
| `RunService:IsClient()` | ✅ | ✅ `run_service_is_client` | Always true in engine |
| `RunService:IsServer()` | ✅ | ✅ `run_service_is_server` | True on Forge server |
| `RunService:IsStudio()` | ✅ | ✅ `run_service_is_studio` | True in editor |
| `RunService:IsRunning()` | ✅ | ✅ `run_service_is_running` | True during play mode |
| `RunService:BindToRenderStep()` | ✅ | ❌ | Needs signal system |
| `RunService:UnbindFromRenderStep()` | ✅ | ❌ | Needs signal system |

### Players Service

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Players.LocalPlayer` | ✅ | ❌ | Client only |
| `Players:GetPlayers()` | ✅ | ❌ | |
| `Players:GetPlayerByUserId()` | ✅ | ❌ | |
| `Players:GetPlayerFromCharacter()` | ✅ | ❌ | |
| `Players.PlayerAdded` | ✅ | ❌ | Signal |
| `Players.PlayerRemoving` | ✅ | ❌ | Signal |
| `Player.Character` | ✅ | ❌ | |
| `Player.CharacterAdded` | ❌ | ❌ | |
| `Player.UserId` | ✅ | ❌ | |
| `Player.Name` | ✅ | ❌ | |
| `Player.Team` | ✅ | ❌ | |
| `Player:Kick()` | ✅ | ❌ | No-op stub |
| `Player:LoadCharacter()` | ❌ | ❌ | |

### TweenService (P1 - Animation)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `TweenService:Create()` | ✅ | ✅ | P1 implemented |
| `TweenInfo.new()` | ✅ UserData | ✅ | All 6 params |
| `Tween:Play()` | ✅ | ✅ | |
| `Tween:Pause()` | ✅ | ✅ | |
| `Tween:Cancel()` | ✅ | ✅ | |
| `Tween.Completed` | ❌ | ❌ | Signal not wired |
| `Enum.EasingStyle.*` | ✅ Integer | ✅ Integer | 0-10 codes |
| `Enum.EasingDirection.*` | ✅ Integer | ✅ Integer | 0-2 codes |

### UserInputService (P1 - Input)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `UserInputService.InputBegan` | ❌ | ❌ | Signal not wired |
| `UserInputService.InputEnded` | ❌ | ❌ | |
| `UserInputService.InputChanged` | ❌ | ❌ | |
| `UserInputService:IsKeyDown()` | ✅ | ✅ | P1 implemented |
| `UserInputService:IsMouseButtonPressed()` | ✅ | ✅ | |
| `UserInputService:GetMouseLocation()` | ✅ | ✅ | Returns {X,Y} / (x,y) |
| `UserInputService:GetMouseDelta()` | ✅ | ✅ | |
| `UserInputService.MouseBehavior` | ❌ | ❌ | |
| `UserInputService.TouchEnabled` | ❌ | ❌ | |
| `UserInputService.KeyboardEnabled` | ❌ | ❌ | |
| `UserInputService.GamepadEnabled` | ❌ | ❌ | |
| `Enum.KeyCode.*` | ✅ Integer | ✅ Integer | Use raw key codes |
| `Enum.UserInputType.*` | ❌ | ❌ | |

### ContextActionService

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `ContextActionService:BindAction()` | ❌ | ❌ | |
| `ContextActionService:UnbindAction()` | ❌ | ❌ | |
| `ContextActionService:SetPosition()` | ❌ | ❌ | Mobile button |
| `ContextActionService:SetImage()` | ❌ | ❌ | |

### ReplicatedStorage / ServerStorage

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `ReplicatedStorage` access | ✅ | ❌ | P1 implemented |
| `ServerStorage` access | ✅ | ❌ | Server only |
| `ServerScriptService` access | ✅ | ❌ | Server only |

### CollectionService (Tags)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `CollectionService:AddTag()` | ✅ | ✅ | P2 implemented |
| `CollectionService:RemoveTag()` | ✅ | ✅ | |
| `CollectionService:HasTag()` | ✅ | ✅ | |
| `CollectionService:GetTagged()` | ✅ | ✅ | |
| `CollectionService:GetTags()` | ❌ | ❌ | |
| `CollectionService:GetInstanceAddedSignal()` | ❌ | ❌ | |
| `CollectionService:GetInstanceRemovedSignal()` | ❌ | ❌ | |

### Debris Service

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Debris:AddItem(instance, lifetime)` | ✅ Stub | 🔶 Shared | P1 shared service |

### HttpService (Full Roblox Parity)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `HttpService:GetAsync()` | ✅ | ✅ | P2 implemented via ureq |
| `HttpService:PostAsync()` | ✅ | ✅ | JSON content-type |
| `HttpService:RequestAsync()` | ✅ | ✅ | Full: GET/POST/PUT/DELETE/PATCH/HEAD, custom headers |
| `HttpService:JSONEncode()` | 🔶 Strings | 🔶 Strings | Tables not yet supported |
| `HttpService:JSONDecode()` | 🔶 Basic | 🔶 Basic | |
| `HttpService:GenerateGUID()` | ✅ | ✅ | UUID v4, optional curly braces |
| `HttpService:UrlEncode()` | ✅ | ✅ | RFC 3986 compliant |
| `HttpResponse.Success` | ✅ | ✅ | Boolean (2xx = true) |
| `HttpResponse.StatusCode` | ✅ | ✅ | Integer |
| `HttpResponse.StatusMessage` | ✅ | ✅ | String |
| `HttpResponse.Headers` | ✅ | ✅ | Table/HashMap |
| `HttpResponse.Body` | ✅ | ✅ | String |

### MarketplaceService (Eustress Tickets, NOT Robux)

| Eustress API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `MarketplaceService:PromptPurchase(player, productId)` | ✅ | ✅ | Tickets (TKT) currency |
| `MarketplaceService:GetProductInfo(productId)` | ✅ | ✅ | Returns name, price, description |
| `MarketplaceService:PlayerOwnsGamePass(player, passId)` | ✅ | ✅ | |
| `MarketplaceService:GetTicketBalance(player)` | ✅ | ✅ | Eustress extension |
| `MarketplaceService.PromptPurchaseFinished` | 🔶 Signal stub | ❌ | |

### PathfindingService

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `PathfindingService:CreatePath()` | ❌ | ❌ | |
| `Path:ComputeAsync()` | ❌ | ❌ | |
| `Path:GetWaypoints()` | ❌ | ❌ | |
| `Path.Blocked` | ❌ | ❌ | |

### Chat Service

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `TextChatService` | ❌ | ❌ | Modern chat |
| `Chat:Chat()` | ❌ | ❌ | Legacy |

---

## 6. Events & Signals

### task Library (P1 - Implemented)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `task.wait(n)` | ✅ | ✅ | P1 implemented |
| `task.spawn(fn, ...)` | ✅ Immediate | 🔶 Placeholder | Executes immediately (coroutine TODO) |
| `task.defer(fn, ...)` | ✅ Immediate | 🔶 Placeholder | Executes immediately (deferral TODO) |
| `task.delay(n, fn, ...)` | ✅ Immediate | 🔶 Placeholder | Executes immediately (timer TODO) |
| `task.desynchronize()` | ❌ | ❌ | Parallel Luau |
| `task.synchronize()` | ❌ | ❌ | |
| `task.cancel(thread)` | ✅ Stub | ✅ | No-op until coroutine scheduler |

### Remote Events/Functions (P0 - Networking)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `RemoteEvent:FireServer()` | 🔶 Stub | ❌ | Client→Server |
| `RemoteEvent:FireClient()` | 🔶 Stub | ❌ | Server→Client |
| `RemoteEvent:FireAllClients()` | 🔶 Stub | ❌ | Broadcast |
| `RemoteEvent.OnServerEvent` | 🔶 Stub | ❌ | |
| `RemoteEvent.OnClientEvent` | 🔶 Stub | ❌ | |
| `RemoteFunction:InvokeServer()` | 🔶 Stub | ❌ | |
| `RemoteFunction:InvokeClient()` | 🔶 Stub | ❌ | |
| `RemoteFunction.OnServerInvoke` | 🔶 Stub | ❌ | |
| `RemoteFunction.OnClientInvoke` | 🔶 Stub | ❌ | |

### Bindable Events/Functions (Local)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `BindableEvent:Fire()` | 🔶 Stub | ❌ | |
| `BindableEvent.Event` | 🔶 Stub | ❌ | |
| `BindableFunction:Invoke()` | 🔶 Stub | ❌ | |
| `BindableFunction.OnInvoke` | 🔶 Stub | ❌ | |

### Signal/Connection Pattern

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `signal:Connect(fn)` | ✅ | ❌ | Returns Connection with Disconnect() |
| `signal:Fire(...)` | ✅ | ❌ | Eustress extension — fire all connected callbacks |
| `signal:Once(fn)` | ✅ | ❌ | Auto-disconnect after first fire |
| `signal:Wait()` | 🔶 Stub | ❌ | Returns immediately (needs coroutine scheduler) |
| `connection:Disconnect()` | ✅ | ❌ | Removes callback from signal |
| `connection.Connected` | ✅ | ❌ | Boolean property |

---

## 7. Physics & Constraints

### BasePart Properties

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `part.Position` | ✅ __newindex | ✅ `part_set_position` | Fires Changed event |
| `part.Orientation` | ❌ | ✅ `part_set_rotation` | Euler deg → quaternion |
| `part.Size` | ✅ __newindex | ✅ `part_set_size` | Fires Changed event |
| `part.CFrame` | ✅ __newindex | ❌ | Fires Changed event |
| `part.Anchored` | ✅ __newindex | ✅ `part_set_anchored` | Fires Changed event |
| `part.CanCollide` | ✅ __newindex | ✅ `part_set_can_collide` | |
| `part.CanTouch` | ❌ | ❌ | |
| `part.CanQuery` | ❌ | ❌ | Raycast filter |
| `part.Massless` | ✅ __newindex | ❌ | Fires Changed event |
| `part.Transparency` | ✅ __newindex | ✅ `part_set_transparency` | Fires Changed event |
| `part.Color` | ✅ __newindex | ✅ `part_set_color` | Accepts Color3/BrickColor, fires Changed |
| `part.Material` | ✅ __newindex | ✅ `part_set_material` | Fires Changed event |
| `part.Reflectance` | ✅ __newindex | ❌ | Fires Changed event |
| `part.CastShadow` | ❌ | ❌ | |
| `part.AssemblyLinearVelocity` | ❌ | ✅ `part_get_velocity` | Returns (x,y,z) m/s |
| `part.AssemblyAngularVelocity` | ❌ | ❌ | |
| `part:ApplyImpulse()` | ❌ | ✅ `part_apply_impulse` | Queued via PhysicsCommand |
| `part:ApplyAngularImpulse()` | ❌ | ✅ `part_apply_angular_impulse` | Queued via PhysicsCommand |
| `part:GetMass()` | ❌ | ✅ `part_get_mass` | Returns kg (stub: 1.0) |
| `part:GetVelocityAtPosition()` | ❌ | ❌ | |

### BasePart Events

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `part.Touched` | ✅ Signal | ❌ | Signal accessible via __index |
| `part.TouchEnded` | ✅ Signal | ❌ | Signal accessible via __index |

### Constraints

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `WeldConstraint` | ✅ TOML | ❌ | Avian FixedJoint |
| `Motor6D` | ✅ TOML | ❌ | Avian RevoluteJoint |
| `HingeConstraint` | ✅ TOML | ❌ | |
| `PrismaticConstraint` | ✅ TOML | ❌ | |
| `BallSocketConstraint` | ✅ TOML | ❌ | Avian SphericalJoint |
| `SpringConstraint` | ✅ TOML | ❌ | |
| `RopeConstraint` | ✅ TOML | ❌ | |
| `RodConstraint` | ❌ | ❌ | |
| `AlignPosition` | ❌ | ❌ | |
| `AlignOrientation` | ❌ | ❌ | |
| `VectorForce` | ❌ | ❌ | |
| `BodyVelocity` | ❌ | ❌ | Legacy |
| `BodyPosition` | ❌ | ❌ | Legacy |
| `BodyGyro` | ❌ | ❌ | Legacy |
| `BodyForce` | ❌ | ❌ | Legacy |

---

## 8. UI/GUI

### ScreenGui

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `ScreenGui` creation | ✅ | ❌ | Instance.new("ScreenGui") |
| `ScreenGui.Enabled` | ✅ | ❌ | Default true |
| `ScreenGui.DisplayOrder` | ✅ | ❌ | Default 0 |
| `ScreenGui.IgnoreGuiInset` | ✅ | ❌ | Default false |
| `ScreenGui.ResetOnSpawn` | ✅ | ❌ | Default true |
| `ScreenGui.ZIndexBehavior` | ✅ | ❌ | Default "Sibling" |

### GuiObject (Base)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `gui.Position` | ✅ | ❌ | UDim2 default |
| `gui.Size` | ✅ | ❌ | UDim2 default |
| `gui.AnchorPoint` | ✅ | ❌ | Vector2 (as Vector3) |
| `gui.Rotation` | ✅ | ❌ | Frame only, default 0 |
| `gui.Visible` | ✅ | ❌ | Default true |
| `gui.ZIndex` | ✅ | ❌ | Default 1 |
| `gui.BackgroundColor3` | ✅ | ❌ | Color3 default white |
| `gui.BackgroundTransparency` | ✅ | ❌ | Default 0.0 |
| `gui.BorderColor3` | ✅ | ❌ | Frame only |
| `gui.BorderSizePixel` | ✅ | ❌ | Default 1 |
| `gui.ClipsDescendants` | ✅ | ❌ | Frame only |
| `gui.LayoutOrder` | ✅ | ❌ | Default 0 |

### Frame/TextLabel/TextButton/TextBox

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Frame` | ✅ | ❌ | Full property set |
| `TextLabel.Text` | ✅ | ❌ | Default "" |
| `TextLabel.TextColor3` | ✅ | ❌ | Default black |
| `TextLabel.TextSize` | ✅ | ❌ | Default 14 |
| `TextLabel.Font` | ✅ | ❌ | Default SourceSans |
| `TextLabel.TextScaled` | ✅ | ❌ | Default false |
| `TextLabel.TextWrapped` | ✅ | ❌ | Default false |
| `TextLabel.TextXAlignment` | ✅ | ❌ | Default Center |
| `TextLabel.TextYAlignment` | ✅ | ❌ | Default Center |
| `TextLabel.TextTransparency` | ✅ | ❌ | Default 0.0 |
| `TextLabel.RichText` | ✅ | ❌ | Default false |
| `TextButton` | ✅ | ❌ | Full property set |
| `TextButton.AutoButtonColor` | ✅ | ❌ | Default true |
| `TextButton.Activated` | ❌ | ❌ | Click event (TODO) |
| `TextButton.MouseButton1Click` | ❌ | ❌ | Event (TODO) |
| `TextBox` | ✅ | ❌ | Full property set |
| `TextBox.PlaceholderText` | ✅ | ❌ | Default "" |
| `TextBox.ClearTextOnFocus` | ✅ | ❌ | Default true |
| `TextBox.MultiLine` | ✅ | ❌ | Default false |
| `TextBox.TextEditable` | ✅ | ❌ | Default true |
| `TextBox.FocusLost` | ❌ | ❌ | Event (TODO) |
| `TextBox:CaptureFocus()` | ❌ | ❌ | Method (TODO) |

### ImageLabel/ImageButton

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `ImageLabel` | ✅ | ❌ | Full property set |
| `ImageLabel.Image` | ✅ | ❌ | Default "" |
| `ImageLabel.ImageColor3` | ✅ | ❌ | Default white |
| `ImageLabel.ImageTransparency` | ✅ | ❌ | Default 0.0 |
| `ImageLabel.ScaleType` | ✅ | ❌ | Default Stretch |
| `ImageButton` | ✅ | ❌ | Full property set |
| `ScrollingFrame` | ✅ | ❌ | CanvasSize, ScrollBar, etc. |

### Layout Objects

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `UIListLayout` | ✅ | ❌ | FillDirection, SortOrder, Padding |
| `UIGridLayout` | ✅ | ❌ | CellSize, CellPadding, FillDirection |
| `UITableLayout` | ❌ | ❌ | |
| `UIPageLayout` | ❌ | ❌ | |
| `UIPadding` | ✅ | ❌ | Top/Bottom/Left/Right |
| `UICorner` | ✅ | ❌ | CornerRadius default 8px |
| `UIStroke` | ✅ | ❌ | Color, Thickness, Mode |
| `UIGradient` | ❌ | ❌ | |
| `UIAspectRatioConstraint` | ✅ | ❌ | AspectRatio, Type, Axis |
| `UISizeConstraint` | ✅ | ❌ | MinSize, MaxSize |
| `UITextSizeConstraint` | ✅ | ❌ | MinTextSize, MaxTextSize |

### BillboardGui / SurfaceGui

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `BillboardGui` | ✅ | ❌ | Size, StudsOffset, MaxDistance, Adornee |
| `SurfaceGui` | ✅ | ❌ | Face, CanvasSize, SizingMode, Adornee |

---

## 9. Sound & Media

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Sound.SoundId` | ✅ Table | ✅ Struct | P2 implemented |
| `Sound.Volume` | ✅ | ✅ | 0.0-1.0 |
| `Sound.Playing` | ✅ | ✅ | |
| `Sound.Looped` | ✅ | ✅ | |
| `Sound.PlaybackSpeed` | ❌ | ❌ | |
| `Sound.TimePosition` | ❌ | ❌ | |
| `Sound:Play()` | ✅ | ✅ | |
| `Sound:Pause()` | ❌ | ❌ | |
| `Sound:Resume()` | ❌ | ❌ | |
| `Sound:Stop()` | ✅ | ✅ | |
| `Sound.Ended` | ❌ | ❌ | |
| `Sound.Played` | ❌ | ❌ | |
| `SoundService:PlayLocalSound()` | ✅ | ❌ | Luau only |

---

## 10. Networking

### Replication

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| Property replication | ❌ | ❌ | Automatic sync |
| `ReplicatedStorage` | 🔶 Stub | ❌ | Shared assets |
| Network ownership | ❌ | ❌ | Physics authority |
| `part:SetNetworkOwner()` | ❌ | ❌ | |
| `part:GetNetworkOwner()` | ❌ | ❌ | |

---

## 11. Data Persistence

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `DataStoreService:GetDataStore()` | ✅ | ✅ | P2 implemented (AWS DynamoDB) |
| `DataStore:GetAsync()` | ✅ | ✅ | Local cache fallback |
| `DataStore:SetAsync()` | ✅ | ✅ | |
| `DataStore:UpdateAsync()` | ❌ | ✅ Shared | Transform function |
| `DataStore:RemoveAsync()` | ✅ | ✅ | |
| `DataStore:IncrementAsync()` | ✅ | ✅ | Atomic increment |
| `OrderedDataStore:GetSortedAsync()` | ✅ | ✅ | Leaderboards |
| `MemoryStoreService` | ❌ | ❌ | Temporary data |

---

## 12. Animation

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Animator:LoadAnimation()` | ✅ | ❌ | P3 implemented |
| `AnimationTrack:Play()` | ✅ | ❌ | P3 implemented (fadeTime, weight, speed) |
| `AnimationTrack:Stop()` | ✅ | ❌ | P3 implemented (fadeTime) |
| `AnimationTrack:AdjustSpeed()` | ✅ | ❌ | P3 implemented |
| `AnimationTrack:AdjustWeight()` | ✅ | ❌ | P3 implemented (weight, fadeTime) |
| `AnimationTrack.IsPlaying` | ✅ | ❌ | P3 implemented |
| `AnimationTrack.Length` | ✅ | ❌ | P3 implemented |
| `AnimationTrack.Looped` | ✅ | ❌ | P3 implemented |
| `AnimationTrack.Speed` | ✅ | ❌ | P3 implemented |
| `AnimationTrack.TimePosition` | ✅ | ❌ | P3 implemented |
| `AnimationTrack.WeightCurrent` | ✅ | ❌ | P3 implemented |
| `AnimationTrack.WeightTarget` | ✅ | ❌ | P3 implemented |
| `AnimationTrack:GetMarkerReachedSignal()` | 🔶 | ❌ | P3 stub (signal not wired) |
| `AnimationTrack.KeyframeReached` | ❌ | ❌ | Event not wired |
| `AnimationTrack.Stopped` | ❌ | ❌ | Event not wired |
| `AnimationTrack.Priority` | ✅ | ❌ | P3 implemented |

---

## 13. Character & Humanoid

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Humanoid.Health` | ✅ | ❌ | P3 implemented |
| `Humanoid.MaxHealth` | ✅ | ❌ | P3 implemented |
| `Humanoid.WalkSpeed` | ✅ | ❌ | P3 implemented |
| `Humanoid.JumpPower` | ✅ | ❌ | P3 implemented |
| `Humanoid.JumpHeight` | ✅ | ❌ | P3 implemented |
| `Humanoid.HipHeight` | ✅ | ❌ | P3 implemented |
| `Humanoid.AutoRotate` | ✅ | ❌ | P3 implemented |
| `Humanoid.AutoJumpEnabled` | ✅ | ❌ | P3 implemented |
| `Humanoid:TakeDamage()` | ✅ | ❌ | P3 implemented |
| `Humanoid:MoveTo()` | ✅ | ❌ | P3 stub (not wired to pathfinding) |
| `Humanoid:Move()` | ✅ | ❌ | P3 stub (not wired to controller) |
| `Humanoid:ChangeState()` | ✅ | ❌ | P3 stub |
| `Humanoid:GetState()` | ✅ | ❌ | P3 stub (returns Running) |
| `Humanoid.Died` | ❌ | ❌ | Event not wired |
| `Humanoid.Running` | ❌ | ❌ | Event not wired |
| `Humanoid.Jumping` | ❌ | ❌ | Event not wired |
| `Humanoid.MoveToFinished` | ❌ | ❌ | Event not wired |
| `Humanoid.StateChanged` | ❌ | ❌ | Event not wired |
| `HumanoidRootPart` | ❌ | ❌ | |

---

## 14. Camera

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Camera.CFrame` | ✅ | ✅ `camera_get_position` + `camera_get_look_vector` | Split into position + direction |
| `Camera.CameraType` | ✅ | ❌ | |
| `Camera.CameraSubject` | ✅ | ❌ | |
| `Camera.FieldOfView` | ✅ | ✅ `camera_get_fov` / `camera_set_fov` | Degrees |
| `Camera.Focus` | ✅ | ❌ | |
| `Camera.ViewportSize` | ✅ | ✅ via CameraState | Width + height |
| `Camera:ViewportPointToRay()` | ✅ | ❌ | |
| `Camera:ScreenPointToRay()` | ✅ | ✅ `camera_screen_point_to_ray` | Returns ((origin), (direction)) |
| `Camera:WorldToViewportPoint()` | ❌ | ❌ | |
| `Camera:WorldToScreenPoint()` | ✅ | ❌ | |

---

## 15. Input

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Mouse.Hit` | ✅ | ✅ `mouse_get_hit` | (x,y,z) world position |
| `Mouse.Target` | ✅ | ✅ `mouse_get_target` | Entity name string |
| `Mouse.TargetSurface` | ✅ | ❌ | P3 implemented |
| `Mouse.X / Y` | ✅ | ❌ | P3 implemented (Screen position) |
| `Mouse.UnitRay` | ✅ | ❌ | P3 implemented |
| `Mouse.Icon` | ✅ | ❌ | P3 implemented |
| `Mouse.Button1Down` | ❌ | ❌ | Event not wired |
| `Mouse.Button1Up` | ❌ | ❌ | Event not wired |
| `Mouse.Move` | ❌ | ❌ | Event not wired |
| `Mouse.WheelForward` | ❌ | ❌ | |
| `Mouse.WheelBackward` | ❌ | ❌ | |
| `ClickDetector.MouseClick` | ❌ | ❌ | |
| `ClickDetector.MouseHoverEnter` | ❌ | ❌ | |
| `ClickDetector.MouseHoverLeave` | ❌ | ❌ | |
| `ProximityPrompt.Triggered` | ❌ | ❌ | |
| `ProximityPrompt.PromptShown` | ❌ | ❌ | |
| `ProximityPrompt.PromptHidden` | ❌ | ❌ | |

---

## 16. Eustress Extensions

These are Eustress-specific APIs not found in Roblox:

### Physics Simulation (Realism)

| Eustress API | Luau Status | Rune Status | MCP Tool | Notes |
|--------------|-------------|-------------|----------|-------|
| `get_voltage(entity)` | ❌ | 🔶 Stub | ❌ | Battery simulation |
| `get_soc(entity)` | ❌ | 🔶 Stub | ❌ | State of charge |
| `get_temperature(entity)` | ❌ | 🔶 Stub | ❌ | Thermal simulation |
| `get_dendrite_risk(entity)` | ❌ | 🔶 Stub | ❌ | Battery degradation |
| `get_sim_value(key)` | ✅ SimulationService:GetValue | ✅ SIM_VALUES | ✅ get_sim_value | Shared thread-local storage |
| `set_sim_value(key, val)` | ✅ SimulationService:SetValue | ✅ SIM_VALUES | ✅ set_sim_value | Shared thread-local storage |
| `list_sim_values()` | ✅ SimulationService:ListValues | ✅ | ✅ list_sim_values | New in v2 |
| `query_material_properties(name)` | ✅ WorkspaceQuery:QueryMaterial | ✅ | ✅ query_material | Returns roughness, metallic, reflectance |
| `calculate_physics(equation, params)` | ❌ | ❌ | ✅ calculate_physics | 9 equations: kinetic_energy, ideal_gas, Nernst, drag, buoyancy, spring, gravity, heat_conduction, escape_velocity |

### File / Entity Operations

| Eustress API | Luau Status | Rune Status | MCP Tool | Notes |
|--------------|-------------|-------------|----------|-------|
| `query_workspace_entities(class?)` | ✅ WorkspaceQuery:QueryEntities | ✅ | ✅ query_entities | Scans .part.toml + .glb.toml |
| `read_space_file(path)` | ✅ WorkspaceQuery:ReadFile | ✅ | ✅ read_file | Sandboxed to Universe root |
| `write_space_file(path, content)` | ✅ WorkspaceQuery:WriteFile | ✅ | ✅ write_file | Sandboxed, rejects `..` traversal |
| `create_entity(name, class, pos)` | ❌ | ❌ | ✅ create_entity | Writes .part.toml |
| `update_entity(name, props)` | ❌ | ❌ | ✅ update_entity | Modifies .part.toml |
| `delete_entity(name)` | ❌ | ❌ | ✅ delete_entity | Removes .part.toml |

### AI Integration

| Eustress API | Luau Status | Rune Status | MCP Tool | Notes |
|--------------|-------------|-------------|----------|-------|
| `ai_generate_code(prompt)` | ❌ | ❌ | via execute_rune | Claude tool_use generates code |
| `ai_analyze_image(path)` | ❌ | ❌ | ❌ | Vision API planned |

### Workshop / Procedural

| Eustress API | Luau Status | Rune Status | MCP Tool | Notes |
|--------------|-------------|-------------|----------|-------|
| `workshop_submit_idea()` | ❌ | ❌ | ❌ | via Workshop chat |
| `generate_mesh(params)` | ❌ | ❌ | ❌ | Procedural geometry planned |
| `remember(key, value)` | ❌ | ❌ | ✅ remember | Persistent memory across sessions |
| `recall(query)` | ❌ | ❌ | ✅ recall | Search stored memories |
| `stage_file_change(path, content)` | ❌ | ❌ | ✅ stage_file_change | Multi-file diff review |

### Source Control

| Eustress API | Luau Status | Rune Status | MCP Tool | Notes |
|--------------|-------------|-------------|----------|-------|
| `git_status()` | ❌ | ❌ | ✅ git_status | Repository status |
| `git_commit(message)` | ❌ | ❌ | ✅ git_commit | Stage + commit |
| `git_log(count)` | ❌ | ❌ | ✅ git_log | Commit history |
| `git_diff(path?)` | ❌ | ❌ | ✅ git_diff | Uncommitted changes |

### Tag / Collection Management

| Eustress API | Luau Status | Rune Status | MCP Tool | Notes |
|--------------|-------------|-------------|----------|-------|
| `collection_add_tag` | ✅ | ✅ | ✅ add_tag | |
| `collection_remove_tag` | ✅ | ✅ | ✅ remove_tag | |
| `collection_has_tag` | ✅ | ✅ | via get_tagged | |
| `collection_get_tagged` | ✅ | ✅ | ✅ get_tagged_entities | |

### Data Persistence (MCP)

| Eustress API | Luau Status | Rune Status | MCP Tool | Notes |
|--------------|-------------|-------------|----------|-------|
| `datastore_get` | ✅ | ✅ | ✅ datastore_get | |
| `datastore_set` | ✅ | ✅ | ✅ datastore_set | |

### Spatial / Raycast

| Eustress API | Luau Status | Rune Status | MCP Tool | Notes |
|--------------|-------------|-------------|----------|-------|
| `workspace_raycast` | ✅ | ✅ | ✅ raycast | Origin + direction + max_distance |
| `http_request` | ✅ | ✅ | ✅ http_request | GET/POST/PUT/DELETE |
| `measure_distance` | (compute directly) | (compute directly) | ✅ measure_distance | Euclidean 3D distance |
| `list_space_contents` | ❌ | ❌ | ✅ list_space_contents | Services + entities overview |

### Manufacturing Mode (MCP only)

| MCP Tool | Notes |
|----------|-------|
| `normalize_brief` | Convert conversation to ideation_brief.toml |
| `query_manufacturers` | Filter by process, materials, certifications, capacity |
| `query_investors` | Filter by vertical, check size, investor type |
| `allocate_product` | AI scoring: 40% capability, 25% quality, 20% cost, 10% speed, 5% risk |

### Simulation Mode (MCP only)

| MCP Tool | Notes |
|----------|-------|
| `control_simulation` | play, pause, stop, step, set_time_scale |
| `set_breakpoint` | Conditional pause on watchpoint threshold |
| `export_recording` | Time-series data to CSV or JSON |

### XR / Spatial

| Eustress API | Luau Status | Rune Status | Notes |
|--------------|-------------|-------------|-------|
| `xr_get_headset_pose()` | ❌ | ❌ | VR/AR planned |
| `xr_get_controller_pose()` | ❌ | ❌ | |
| `xr_haptic_pulse()` | ❌ | ❌ | |

---

## Priority Summary

### P0 — Ship Blockers (Must Have)

| Category | Items | Status |
|----------|-------|--------|
| **Raycasting** | workspace:Raycast, RaycastParams | ✅ Done |
| **Vector3** | new, operators, magnitude, lerp | 🔶 Partial |
| **CFrame** | new, angles, lookAt, multiply | ❌ Critical |
| **Instance** | new, Clone, Destroy, Parent, FindFirstChild | ❌ Critical |
| **RunService** | Heartbeat, Stepped, RenderStepped | ❌ Critical |
| **task** | wait, spawn, defer, delay | 🔶 Stub |
| **Events** | Connect, Disconnect, Wait | ❌ Critical |
| **BasePart** | Position, CFrame, Velocity, Touched | ❌ Critical |

### P1 — Core Gameplay

| Category | Items | Status |
|----------|-------|--------|
| **Players** | LocalPlayer, GetPlayers, Character | ❌ |
| **Humanoid** | Health, WalkSpeed, MoveTo, Died | ❌ |
| **TweenService** | Create, Play, easing | ❌ |
| **UserInputService** | InputBegan, IsKeyDown | ❌ |
| **Camera** | CFrame, ScreenPointToRay | ❌ |
| **Sound** | Play, Stop, Volume | ❌ |
| **RemoteEvent** | FireServer, OnServerEvent | 🔶 Stub |
| **Color3** | new, fromRGB, Lerp | ❌ |

### P2 — Nice to Have

| Category | Items | Status |
|----------|-------|--------|
| **UI/GUI** | ScreenGui, Frame, TextLabel | ❌ |
| **DataStore** | GetAsync, SetAsync | ❌ |
| **Animation** | LoadAnimation, Play | ❌ |
| **Pathfinding** | CreatePath, ComputeAsync | ❌ |
| **CollectionService** | Tags | ❌ |

### P3 — Future / Optional

| Category | Items | Status |
|----------|-------|--------|
| **MarketplaceService** | Purchases | ➖ N/A |
| **Chat** | TextChatService | ❌ |
| **Terrain** | Voxel terrain | ❌ |

---

## Implementation Estimates

| Priority | Items | Estimated Effort |
|----------|-------|------------------|
| P0 | ~40 APIs | 4-6 weeks |
| P1 | ~60 APIs | 6-8 weeks |
| P2 | ~80 APIs | 8-12 weeks |
| **Total** | ~180 APIs | 18-26 weeks |

---

## Next Steps

1. **Implement CFrame** — Most critical missing type
2. **Implement Instance API** — new, Clone, Destroy, Parent, FindFirstChild
3. **Implement RunService** — Heartbeat, Stepped for game loops
4. **Implement Event/Signal system** — Connect, Disconnect, Wait pattern
5. **Expand Vector3** — operators, Dot, Cross, Magnitude, Lerp
6. **Implement task library** — proper coroutine scheduling

---

*Last updated: March 2026*
*Eustress Engine v0.1.0*
