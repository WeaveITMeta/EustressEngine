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
| `Vector3.new(x, y, z)` | 🔶 Table | ✅ Struct | Rune has proper Vector3 type |
| `Vector3.X/Y/Z` fields | 🔶 Table | ✅ Native | |
| `Vector3 + - * /` operators | ❌ | ❌ | Need operator overloading |
| `Vector3:Dot()` | ❌ | ❌ | |
| `Vector3:Cross()` | ❌ | ❌ | |
| `Vector3.Magnitude` | ❌ | ❌ | |
| `Vector3.Unit` | ❌ | ❌ | |
| `Vector3:Lerp()` | ❌ | ❌ | |
| `Vector2.new(x, y)` | ❌ | ❌ | |
| `Vector2int16` | ❌ | ❌ | |
| `Vector3int16` | ❌ | ❌ | |

### Transform Types

| Roblox Type | Luau Status | Rune Status | Notes |
|-------------|-------------|-------------|-------|
| `CFrame.new()` | ❌ | ❌ | Critical for transforms |
| `CFrame.Angles()` | ❌ | ❌ | |
| `CFrame.fromEulerAngles()` | ❌ | ❌ | |
| `CFrame.lookAt()` | ❌ | ❌ | |
| `CFrame * CFrame` | ❌ | ❌ | |
| `CFrame:Inverse()` | ❌ | ❌ | |
| `CFrame:ToWorldSpace()` | ❌ | ❌ | |
| `CFrame:ToObjectSpace()` | ❌ | ❌ | |
| `CFrame.Position` | ❌ | ❌ | |
| `CFrame.LookVector` | ❌ | ❌ | |
| `CFrame.RightVector` | ❌ | ❌ | |
| `CFrame.UpVector` | ❌ | ❌ | |

### Color Types

| Roblox Type | Luau Status | Rune Status | Notes |
|-------------|-------------|-------------|-------|
| `Color3.new(r, g, b)` | ❌ | ❌ | 0-1 floats |
| `Color3.fromRGB(r, g, b)` | ❌ | ❌ | 0-255 integers |
| `Color3.fromHSV(h, s, v)` | ❌ | ❌ | |
| `Color3:Lerp()` | ❌ | ❌ | |
| `BrickColor.new()` | ➖ | ➖ | Deprecated, use Color3 |

### UI Types

| Roblox Type | Luau Status | Rune Status | Notes |
|-------------|-------------|-------------|-------|
| `UDim.new(scale, offset)` | ❌ | ❌ | |
| `UDim2.new(xs, xo, ys, yo)` | ❌ | ❌ | |
| `UDim2.fromScale()` | ❌ | ❌ | |
| `UDim2.fromOffset()` | ❌ | ❌ | |
| `Rect.new()` | ❌ | ❌ | |

### Other Types

| Roblox Type | Luau Status | Rune Status | Notes |
|-------------|-------------|-------------|-------|
| `Ray.new(origin, direction)` | ❌ | ❌ | Use Vector3 pair instead |
| `Region3.new()` | ❌ | ❌ | |
| `NumberRange.new()` | ❌ | ❌ | |
| `NumberSequence.new()` | ❌ | ❌ | For particles |
| `ColorSequence.new()` | ❌ | ❌ | For particles |
| `TweenInfo.new()` | ❌ | ❌ | |
| `Enum.*` | 🔶 Strings | 🔶 Strings | Use string literals |
| `Instance.new()` | 🔶 Stub | ❌ | |

---

## 2. Global Functions

| Roblox Function | Luau Status | Rune Status | Notes |
|-----------------|-------------|-------------|-------|
| `print(...)` | ✅ | ✅ `log_info` | Routes to Output panel |
| `warn(...)` | ✅ | ✅ `log_warn` | |
| `error(...)` | 🔶 | ✅ `log_error` | |
| `assert()` | ❌ | ❌ | |
| `type()` | ✅ Native | ❌ | Lua built-in |
| `typeof()` | ❌ | ❌ | Roblox extension |
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
| `delay()` | ❌ | ❌ | Deprecated |
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
| `Instance.new("ClassName")` | 🔶 Stub | ❌ | Core spawning |
| `instance:Clone()` | ❌ | ❌ | |
| `instance:Destroy()` | ❌ | ❌ | |
| `instance:ClearAllChildren()` | ❌ | ❌ | |
| `instance.Parent` | ❌ | ❌ | |
| `instance.Name` | ❌ | ❌ | |
| `instance.ClassName` | ❌ | ❌ | |
| `instance:IsA("ClassName")` | ❌ | ❌ | |
| `instance:IsDescendantOf()` | ❌ | ❌ | |
| `instance:IsAncestorOf()` | ❌ | ❌ | |

### Instance Finding

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `instance:FindFirstChild()` | ❌ | ❌ | |
| `instance:FindFirstChildOfClass()` | ❌ | ❌ | |
| `instance:FindFirstChildWhichIsA()` | ❌ | ❌ | |
| `instance:FindFirstAncestor()` | ❌ | ❌ | |
| `instance:FindFirstDescendant()` | ❌ | ❌ | |
| `instance:GetChildren()` | ❌ | ❌ | |
| `instance:GetDescendants()` | ❌ | ❌ | |
| `instance:WaitForChild()` | ❌ | ❌ | Async |
| `instance:GetFullName()` | ❌ | ❌ | |
| `instance:GetAttribute()` | ❌ | ❌ | |
| `instance:SetAttribute()` | ❌ | ❌ | |
| `instance:GetAttributes()` | ❌ | ❌ | |

### Instance Events

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `instance.Changed` | ❌ | ❌ | |
| `instance.ChildAdded` | ❌ | ❌ | |
| `instance.ChildRemoved` | ❌ | ❌ | |
| `instance.DescendantAdded` | ❌ | ❌ | |
| `instance.DescendantRemoving` | ❌ | ❌ | |
| `instance.AncestryChanged` | ❌ | ❌ | |
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
| `workspace:GetPartBoundsInBox()` | ❌ | ❌ | AABB query |
| `workspace:GetPartBoundsInRadius()` | ❌ | ❌ | Sphere query |
| `workspace:GetPartsInPart()` | ❌ | ❌ | Overlap query |
| `workspace:Blockcast()` | ❌ | ❌ | Box sweep |
| `workspace:Spherecast()` | ❌ | ❌ | Sphere sweep |
| `workspace:Shapecast()` | ❌ | ❌ | Generic shape sweep |

### Workspace Properties

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `workspace.Gravity` | ❌ | ❌ | Avian gravity |
| `workspace.CurrentCamera` | ❌ | ❌ | |
| `workspace.DistributedGameTime` | ❌ | ❌ | |
| `workspace.Terrain` | ❌ | ❌ | |

---

## 5. Services

### RunService (P0 - Critical for game loops)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `RunService.Heartbeat` | ❌ | ❌ | Per-frame after physics |
| `RunService.Stepped` | ❌ | ❌ | Per-frame before physics |
| `RunService.RenderStepped` | ❌ | ❌ | Per-frame render (client) |
| `RunService:IsClient()` | ❌ | ❌ | |
| `RunService:IsServer()` | ❌ | ❌ | |
| `RunService:IsStudio()` | ❌ | ❌ | |
| `RunService:IsRunning()` | ❌ | ❌ | |
| `RunService:BindToRenderStep()` | ❌ | ❌ | |
| `RunService:UnbindFromRenderStep()` | ❌ | ❌ | |

### Players Service

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Players.LocalPlayer` | ❌ | ❌ | Client only |
| `Players:GetPlayers()` | ❌ | ❌ | |
| `Players:GetPlayerByUserId()` | ❌ | ❌ | |
| `Players:GetPlayerFromCharacter()` | ❌ | ❌ | |
| `Players.PlayerAdded` | ❌ | ❌ | |
| `Players.PlayerRemoving` | ❌ | ❌ | |
| `Player.Character` | ❌ | ❌ | |
| `Player.CharacterAdded` | ❌ | ❌ | |
| `Player.UserId` | ❌ | ❌ | |
| `Player.Name` | ❌ | ❌ | |
| `Player.Team` | ❌ | ❌ | |
| `Player:Kick()` | ❌ | ❌ | |
| `Player:LoadCharacter()` | ❌ | ❌ | |

### TweenService (P1 - Animation)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `TweenService:Create()` | ❌ | ❌ | |
| `TweenInfo.new()` | ❌ | ❌ | |
| `Tween:Play()` | ❌ | ❌ | |
| `Tween:Pause()` | ❌ | ❌ | |
| `Tween:Cancel()` | ❌ | ❌ | |
| `Tween.Completed` | ❌ | ❌ | |
| `Enum.EasingStyle.*` | ❌ | ❌ | |
| `Enum.EasingDirection.*` | ❌ | ❌ | |

### UserInputService (P1 - Input)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `UserInputService.InputBegan` | ❌ | ❌ | |
| `UserInputService.InputEnded` | ❌ | ❌ | |
| `UserInputService.InputChanged` | ❌ | ❌ | |
| `UserInputService:IsKeyDown()` | ❌ | ❌ | |
| `UserInputService:IsMouseButtonPressed()` | ❌ | ❌ | |
| `UserInputService:GetMouseLocation()` | ❌ | ❌ | |
| `UserInputService:GetMouseDelta()` | ❌ | ❌ | |
| `UserInputService.MouseBehavior` | ❌ | ❌ | |
| `UserInputService.TouchEnabled` | ❌ | ❌ | |
| `UserInputService.KeyboardEnabled` | ❌ | ❌ | |
| `UserInputService.GamepadEnabled` | ❌ | ❌ | |
| `Enum.KeyCode.*` | ❌ | ❌ | |
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
| `ReplicatedStorage` access | 🔶 Stub | ❌ | |
| `ServerStorage` access | 🔶 Stub | ❌ | Server only |
| `ServerScriptService` access | 🔶 Stub | ❌ | Server only |

### CollectionService (Tags)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `CollectionService:AddTag()` | ❌ | ❌ | |
| `CollectionService:RemoveTag()` | ❌ | ❌ | |
| `CollectionService:HasTag()` | ❌ | ❌ | |
| `CollectionService:GetTagged()` | ❌ | ❌ | |
| `CollectionService:GetTags()` | ❌ | ❌ | |
| `CollectionService:GetInstanceAddedSignal()` | ❌ | ❌ | |
| `CollectionService:GetInstanceRemovedSignal()` | ❌ | ❌ | |

### Debris Service

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Debris:AddItem(instance, lifetime)` | ❌ | ❌ | Auto-destroy |

### HttpService

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `HttpService:GetAsync()` | ❌ | ❌ | |
| `HttpService:PostAsync()` | ❌ | ❌ | |
| `HttpService:JSONEncode()` | ❌ | ❌ | |
| `HttpService:JSONDecode()` | ❌ | ❌ | |
| `HttpService:GenerateGUID()` | ❌ | ❌ | |

### MarketplaceService

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `MarketplaceService:PromptProductPurchase()` | ➖ | ➖ | Roblox-specific |
| `MarketplaceService:PromptGamePassPurchase()` | ➖ | ➖ | |
| `MarketplaceService:UserOwnsGamePassAsync()` | ➖ | ➖ | |
| `MarketplaceService.PromptPurchaseFinished` | ➖ | ➖ | |

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

### task Library (P0 - Critical)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `task.wait(n)` | 🔶 Stub | ❌ | Yield for n seconds |
| `task.spawn(fn)` | 🔶 Stub | ❌ | New thread |
| `task.defer(fn)` | 🔶 Stub | ❌ | End of frame |
| `task.delay(n, fn)` | ❌ | ❌ | Delayed spawn |
| `task.desynchronize()` | ❌ | ❌ | Parallel Luau |
| `task.synchronize()` | ❌ | ❌ | |
| `task.cancel(thread)` | ❌ | ❌ | |

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
| `signal:Connect(fn)` | ❌ | ❌ | Returns Connection |
| `signal:Once(fn)` | ❌ | ❌ | Auto-disconnect |
| `signal:Wait()` | ❌ | ❌ | Yield until fired |
| `connection:Disconnect()` | ❌ | ❌ | |
| `connection.Connected` | ❌ | ❌ | |

---

## 7. Physics & Constraints

### BasePart Properties

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `part.Position` | ❌ | 🔶 `set_position` | |
| `part.Orientation` | ❌ | 🔶 `set_rotation` | |
| `part.Size` | ❌ | 🔶 `set_size` | |
| `part.CFrame` | ❌ | ❌ | Full transform |
| `part.Anchored` | ❌ | 🔶 `set_anchored` | |
| `part.CanCollide` | ❌ | ❌ | |
| `part.CanTouch` | ❌ | ❌ | |
| `part.CanQuery` | ❌ | ❌ | Raycast filter |
| `part.Massless` | ❌ | ❌ | |
| `part.Transparency` | ❌ | 🔶 | |
| `part.Color` | ❌ | 🔶 `set_color` | |
| `part.Material` | ❌ | 🔶 `set_material` | |
| `part.Reflectance` | ❌ | ❌ | |
| `part.CastShadow` | ❌ | ❌ | |
| `part.AssemblyLinearVelocity` | ❌ | ❌ | |
| `part.AssemblyAngularVelocity` | ❌ | ❌ | |
| `part:ApplyImpulse()` | ❌ | ❌ | |
| `part:ApplyAngularImpulse()` | ❌ | ❌ | |
| `part:GetMass()` | ❌ | ❌ | |
| `part:GetVelocityAtPosition()` | ❌ | ❌ | |

### BasePart Events

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `part.Touched` | ❌ | ❌ | Collision start |
| `part.TouchEnded` | ❌ | ❌ | Collision end |

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
| `ScreenGui` creation | ❌ | ❌ | |
| `ScreenGui.Enabled` | ❌ | ❌ | |
| `ScreenGui.DisplayOrder` | ❌ | ❌ | |
| `ScreenGui.IgnoreGuiInset` | ❌ | ❌ | |

### GuiObject (Base)

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `gui.Position` | ❌ | ❌ | UDim2 |
| `gui.Size` | ❌ | ❌ | UDim2 |
| `gui.AnchorPoint` | ❌ | ❌ | |
| `gui.Rotation` | ❌ | ❌ | |
| `gui.Visible` | ❌ | ❌ | |
| `gui.ZIndex` | ❌ | ❌ | |
| `gui.BackgroundColor3` | ❌ | ❌ | |
| `gui.BackgroundTransparency` | ❌ | ❌ | |
| `gui.BorderColor3` | ❌ | ❌ | |
| `gui.BorderSizePixel` | ❌ | ❌ | |
| `gui.ClipsDescendants` | ❌ | ❌ | |
| `gui.LayoutOrder` | ❌ | ❌ | |

### Frame/TextLabel/TextButton/TextBox

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Frame` | ❌ | ❌ | Container |
| `TextLabel.Text` | ❌ | ❌ | |
| `TextLabel.TextColor3` | ❌ | ❌ | |
| `TextLabel.TextSize` | ❌ | ❌ | |
| `TextLabel.Font` | ❌ | ❌ | |
| `TextLabel.TextScaled` | ❌ | ❌ | |
| `TextLabel.TextWrapped` | ❌ | ❌ | |
| `TextButton.Activated` | ❌ | ❌ | Click event |
| `TextButton.MouseButton1Click` | ❌ | ❌ | |
| `TextBox.Text` | ❌ | ❌ | |
| `TextBox.FocusLost` | ❌ | ❌ | |
| `TextBox:CaptureFocus()` | ❌ | ❌ | |

### ImageLabel/ImageButton

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `ImageLabel.Image` | ❌ | ❌ | Asset ID |
| `ImageLabel.ImageColor3` | ❌ | ❌ | |
| `ImageLabel.ImageTransparency` | ❌ | ❌ | |
| `ImageLabel.ScaleType` | ❌ | ❌ | |

### Layout Objects

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `UIListLayout` | ❌ | ❌ | |
| `UIGridLayout` | ❌ | ❌ | |
| `UITableLayout` | ❌ | ❌ | |
| `UIPageLayout` | ❌ | ❌ | |
| `UIPadding` | ❌ | ❌ | |
| `UICorner` | ❌ | ❌ | |
| `UIStroke` | ❌ | ❌ | |
| `UIGradient` | ❌ | ❌ | |
| `UIAspectRatioConstraint` | ❌ | ❌ | |
| `UISizeConstraint` | ❌ | ❌ | |

### BillboardGui / SurfaceGui

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `BillboardGui` | ❌ | ❌ | 3D UI |
| `SurfaceGui` | ❌ | ❌ | On part surface |

---

## 9. Sound & Media

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Sound.SoundId` | ❌ | ❌ | |
| `Sound.Volume` | ❌ | ❌ | |
| `Sound.Playing` | ❌ | ❌ | |
| `Sound.Looped` | ❌ | ❌ | |
| `Sound.PlaybackSpeed` | ❌ | ❌ | |
| `Sound.TimePosition` | ❌ | ❌ | |
| `Sound:Play()` | ❌ | ❌ | |
| `Sound:Pause()` | ❌ | ❌ | |
| `Sound:Resume()` | ❌ | ❌ | |
| `Sound:Stop()` | ❌ | ❌ | |
| `Sound.Ended` | ❌ | ❌ | |
| `Sound.Played` | ❌ | ❌ | |
| `SoundService:PlayLocalSound()` | ❌ | ❌ | |

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
| `DataStoreService:GetDataStore()` | ❌ | ❌ | |
| `DataStore:GetAsync()` | ❌ | ❌ | |
| `DataStore:SetAsync()` | ❌ | ❌ | |
| `DataStore:UpdateAsync()` | ❌ | ❌ | |
| `DataStore:RemoveAsync()` | ❌ | ❌ | |
| `DataStore:IncrementAsync()` | ❌ | ❌ | |
| `OrderedDataStore` | ❌ | ❌ | Leaderboards |
| `MemoryStoreService` | ❌ | ❌ | Temporary data |

---

## 12. Animation

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Animator:LoadAnimation()` | ❌ | ❌ | |
| `AnimationTrack:Play()` | ❌ | ❌ | |
| `AnimationTrack:Stop()` | ❌ | ❌ | |
| `AnimationTrack:AdjustSpeed()` | ❌ | ❌ | |
| `AnimationTrack:AdjustWeight()` | ❌ | ❌ | |
| `AnimationTrack.KeyframeReached` | ❌ | ❌ | |
| `AnimationTrack.Stopped` | ❌ | ❌ | |
| `AnimationTrack.Priority` | ❌ | ❌ | |

---

## 13. Character & Humanoid

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Humanoid.Health` | ❌ | ❌ | |
| `Humanoid.MaxHealth` | ❌ | ❌ | |
| `Humanoid.WalkSpeed` | ❌ | ❌ | |
| `Humanoid.JumpPower` | ❌ | ❌ | |
| `Humanoid.JumpHeight` | ❌ | ❌ | |
| `Humanoid:TakeDamage()` | ❌ | ❌ | |
| `Humanoid:MoveTo()` | ❌ | ❌ | |
| `Humanoid:Move()` | ❌ | ❌ | |
| `Humanoid.Died` | ❌ | ❌ | |
| `Humanoid.Running` | ❌ | ❌ | |
| `Humanoid.Jumping` | ❌ | ❌ | |
| `Humanoid.MoveToFinished` | ❌ | ❌ | |
| `Humanoid:ChangeState()` | ❌ | ❌ | |
| `Humanoid:GetState()` | ❌ | ❌ | |
| `Humanoid.StateChanged` | ❌ | ❌ | |
| `HumanoidRootPart` | ❌ | ❌ | |

---

## 14. Camera

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Camera.CFrame` | ❌ | ❌ | |
| `Camera.CameraType` | ❌ | ❌ | |
| `Camera.CameraSubject` | ❌ | ❌ | |
| `Camera.FieldOfView` | ❌ | ❌ | |
| `Camera.Focus` | ❌ | ❌ | |
| `Camera:ViewportPointToRay()` | ❌ | ❌ | |
| `Camera:ScreenPointToRay()` | ❌ | ❌ | |
| `Camera:WorldToViewportPoint()` | ❌ | ❌ | |
| `Camera:WorldToScreenPoint()` | ❌ | ❌ | |

---

## 15. Input

| Roblox API | Luau Status | Rune Status | Notes |
|------------|-------------|-------------|-------|
| `Mouse.Hit` | ❌ | ❌ | CFrame at cursor |
| `Mouse.Target` | ❌ | ❌ | Part under cursor |
| `Mouse.X / Y` | ❌ | ❌ | Screen position |
| `Mouse.Button1Down` | ❌ | ❌ | |
| `Mouse.Button1Up` | ❌ | ❌ | |
| `Mouse.Move` | ❌ | ❌ | |
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

| Eustress API | Luau Status | Rune Status | Notes |
|--------------|-------------|-------------|-------|
| `get_voltage(entity)` | ❌ | 🔶 Stub | Battery simulation |
| `get_soc(entity)` | ❌ | 🔶 Stub | State of charge |
| `get_temperature(entity)` | ❌ | 🔶 Stub | Thermal simulation |
| `get_dendrite_risk(entity)` | ❌ | 🔶 Stub | Battery degradation |
| `get_sim_value(key)` | ❌ | 🔶 Stub | Generic sim values |
| `set_sim_value(key, val)` | ❌ | 🔶 Stub | |

### AI Integration

| Eustress API | Luau Status | Rune Status | Notes |
|--------------|-------------|-------------|-------|
| `ai_generate_code(prompt)` | ❌ | ❌ | Claude integration |
| `ai_analyze_image(path)` | ❌ | ❌ | Vision API |

### Workshop / Procedural

| Eustress API | Luau Status | Rune Status | Notes |
|--------------|-------------|-------------|-------|
| `workshop_submit_idea()` | ❌ | ❌ | Ideation system |
| `generate_mesh(params)` | ❌ | ❌ | Procedural geometry |

### XR / Spatial

| Eustress API | Luau Status | Rune Status | Notes |
|--------------|-------------|-------------|-------|
| `xr_get_headset_pose()` | ❌ | ❌ | VR/AR |
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
