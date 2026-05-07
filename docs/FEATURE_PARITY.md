# Eustress Feature Parity Checklist

Derived from Roblox binary string analysis — every item represents a feature confirmed to
exist in a shipped production game engine. Use this as a living audit. "Exists" ≠ "complete".

Physics engine: **Avian** (not Rapier).
Last cross-checked: 2026-05-04 against `roblox_binary_strings.md` + `roblox_ida_functions.md`.

---

## 1. Instance Model — Core Object Tree

- [x] `Part` / `BasePart` with full property set
- [x] `MeshPart` with mesh asset reference + `CollisionFidelity` + `DoubleSided`
- [x] `Model` container with `PrimaryPart` + `WorldPivot` + `LevelOfDetail`
- [x] `Folder` — pure organizational container (no physics)
- [ ] `PVInstance` — abstract positioned base (CFrame + Size pivot interface)
- [ ] `Configuration` — named property bag container
- [ ] `StringValue`, `IntValue`, `NumberValue`, `BoolValue`, `ObjectValue`, `Color3Value`, `Vector3Value`, `CFrameValue`, `RayValue`, `BrickColorValue` — value object instances
- [x] CFrame / Vector3 / Color3 / UDim / UDim2 as first-class property types
- [ ] `WedgePart` — wedge primitive geometry
- [ ] `CornerWedgePart` — corner wedge primitive
- [ ] `TrussPart` — truss structural primitive
- [ ] `SpawnLocation` — player respawn point with team color + duration
- [ ] `UnionOperation` — result of CSG union (serialized as mesh)
- [ ] `NegateOperation` — result of CSG negate (serialized as mesh)
- [ ] `SkateboardPlatform` — physics skateboard mount
- [ ] `Platform` — moving platform with physics
- [x] `Archivable` flag (serialization opt-out)
- [x] Parent/child hierarchy
- [ ] `SelectionBox` / `SelectionSphere` — in-world adornments (runtime, not editor-only)
- [ ] `SurfaceSelection` — surface highlight adornment

### BasePart properties (complete list)
- [x] Name, ClassName, Parent, Anchored, Color, Material, Size, CFrame, Position, Rotation
- [ ] `BackSurface/FrontSurface/LeftSurface/RightSurface/TopSurface/BottomSurface` — SurfaceType per face
- [ ] `BackSurfaceInput/FrontSurfaceInput/…` — surface input types
- [ ] `BrickColor` — legacy color system (maps to Color3)
- [ ] `CastShadow` toggle
- [ ] `CollisionGroupId` — physics collision group index
- [ ] `CurrentPhysicalProperties` — read-back of effective physical props
- [ ] `EnableFluidForces` — fluid simulation opt-in
- [ ] `LocalTransparencyModifier` — runtime transparency override layer
- [ ] `Massless` — zero-mass flag (no physics contribution)
- [ ] `PivotOffset` — offset of pivot from geometric center
- [ ] `ReceiveAge` — replication timestamp read-back
- [ ] `Reflectance` — legacy reflectance (0–1)
- [ ] `RenderFidelity` — Automatic / Precise / Performance
- [ ] `ResizeIncrement` / `ResizeableFaces` — studio resize grid
- [ ] `RootPriority` — physics assembly priority
- [ ] `RotVelocity` — legacy angular velocity
- [ ] `Velocity` — legacy linear velocity
- [ ] `AssemblyLinearVelocity` / `AssemblyAngularVelocity` / `AssemblyMass` / `AssemblyCenterOfMass` / `AssemblyRootPart` — assembly physics read-backs
- [ ] `MaterialVariant` — override material with custom `MaterialVariant` instance

---

## 2. Decals & Surface Textures

- [ ] `Decal` — static face texture (Texture asset, Face, Transparency, Color3, ZIndex)
- [ ] `Texture` — tiling face texture (StudsPerTileU, StudsPerTileV, OffsetStudsU, OffsetStudsV)
- [ ] `SurfaceType` enum — Smooth, Glue, Weld, Studs, Inlet, Universal, Hinge, Motor, SteppingMotor, SmoothNoOutlines
- [ ] `MaterialService` — custom material variants registry
- [ ] `MaterialVariant` — custom PBR material definition (ColorMap, NormalMap, MetalnessMap, RoughnessMap)

---

## 3. Handle Adornments (In-World)

These are positioned in the 3D world, not the 2D editor overlay.

- [ ] `Handles` — face handles on a part (NormalId per face, MouseButton1Down/Up, MouseDrag, MouseEnter/Leave/Hover)
- [ ] `ArcHandles` — arc rotation handles (Axes, same events)
- [ ] `BoxHandleAdornment` — box-shaped clickable adornment
- [ ] `ConeHandleAdornment` — cone-shaped adornment
- [ ] `CylinderHandleAdornment` — cylinder adornment
- [ ] `ImageHandleAdornment` — image billboard adornment
- [ ] `LineHandleAdornment` — line adornment with Length + Thickness
- [ ] `SphereHandleAdornment` — sphere adornment
- [ ] `WireframeHandleAdornment` — wireframe mesh adornment
- [ ] `SelectionPartLasso` — rubber-band selection from part
- [ ] `SelectionPointLasso` — rubber-band selection from point

---

## 4. VFX — Particles & Effects

- [ ] `ParticleEmitter` — full particle system (Rate, Lifetime, Speed, Size, Color, Rotation, Texture, SpreadAngle, Acceleration, Drag, LightEmission, LightInfluence, LockedToPart, Orientation, FlipbookLayout/Framerate/Mode, TimeScale, WindAffectsDrag, Squash, VelocityInheritance, ZOffset)
- [ ] `Fire` — flame effect (Color, SecondaryColor, Size, Heat, TimeScale, Enabled)
- [ ] `Smoke` — smoke effect (Color, Density, Enabled, Opacity, RiseVelocity, Size, TimeScale)
- [ ] `Sparkles` — sparkle effect (Color, Enabled, SparkleColor, TimeScale)
- [ ] `Trail` — motion trail between two Attachments (Attachment0/1, Lifetime, Color, Transparency, LightEmission, LightInfluence, MaxLength, MinLength, Texture, TextureLength, TextureMode, WidthScale, FaceCamera, Brightness, Enabled)
- [ ] `Beam` — visual beam between two Attachments (Attachment0/1, Color, Transparency, Width0/1, CurveSize0/1, Segments, Texture, TextureLength, TextureMode, TextureSpeed, LightEmission, LightInfluence, FaceCamera, ZOffset, Brightness, Enabled)
- [ ] `Explosion` — blast impulse (BlastPressure, BlastRadius, DestroyJointRadiusPercent, ExplosionType, Position, Visible)
- [ ] `ForceField` — damage immunity shield (Visible)

---

## 5. Physics & Constraints (Avian)

### Modern constraints
- [ ] `BallSocketConstraint` — spherical joint (TwistLimitsEnabled, TwistUpperAngle, TwistLowerAngle, UpperAngle, Restitution)
- [ ] `HingeConstraint` — motor + servo (LimitsEnabled, LowerAngle, UpperAngle, ActuatorType, AngularSpeed, AngularVelocity, TargetAngle, ServoMaxTorque, MotorMaxTorque, MotorMaxAcceleration, Restitution, AngularResponsiveness)
- [ ] `SpringConstraint` — stiffness + damping (FreeLength, Stiffness, Damping, Coils, Radius, LimitsEnabled, MaxLength, MinLength, MaxForce)
- [ ] `RopeConstraint` — max-length soft constraint
- [ ] `RodConstraint` — fixed-length rigid distance
- [ ] `SliderConstraint` — prismatic (linear) joint
- [ ] `TorsionSpringConstraint` — angular spring
- [ ] `UniversalConstraint` — 2-axis rotation, no twist
- [ ] `WeldConstraint` — rigid body fusion (Active, Part0, Part1)
- [ ] `NoCollisionConstraint` — disable collision between two specific parts
- [ ] `AlignPosition` — PD position controller (RigidityEnabled, MaxForce, MaxVelocity, Responsiveness, ApplyAtCenterOfMass)
- [ ] `AlignOrientation` — PD orientation controller (RigidityEnabled, MaxTorque, MaxAngularVelocity, Responsiveness, ReactionTorqueEnabled)
- [ ] `LinearVelocity` — scripted linear velocity (RelativeTo, MaxForce, VelocityConstraint)
- [ ] `AngularVelocity` — scripted angular velocity (RelativeTo, MaxTorque)
- [ ] `VectorForce` — constant force (RelativeTo, Force, ApplyAtCenterOfMass)
- [ ] `Torque` — constant torque (RelativeTo, Torque)
- [ ] `LineForce` — force along attachment axis (Magnitude, InverseSquareLaw, ApplyAtCenterOfMass)
- [ ] `Plane` — plane constraint (keeps part on a plane)
- [ ] `Attachment` — named anchor points on parts (CFrame, Position, Orientation, Axis, SecondaryAxis, WorldCFrame, WorldPosition, WorldAxis, WorldOrientation, WorldSecondaryAxis)
- [ ] `Bone` — skinned mesh bone (extends Attachment)

### Force appliers (legacy)
- [ ] `BodyForce` — constant world-space force
- [ ] `BodyGyro` — torque to target CFrame
- [ ] `BodyPosition` — PD position controller (legacy)
- [ ] `BodyVelocity` — velocity controller (legacy)
- [ ] `BodyThrust` — local-space thrust
- [ ] `BodyAngularVelocity` — angular velocity controller (legacy)
- [ ] `RocketPropulsion` — guided rocket force

### Legacy joints (pre-constraint system)
- [ ] `Weld` / `ManualWeld` / `ManualGlue` / `Snap` / `Glue` — legacy rigid joints
- [ ] `Motor` / `DynamicRotate` / `RotateP` / `RotateV` / `Rotate` — legacy motor joints
- [ ] `Motor6D` — driven skeletal joint (C0, C1, Part0, Part1, DesiredAngle, MaxVelocity, CurrentAngle)

### Shared constraint properties
- [x] Per-part `Material` density lookup
- [ ] Per-part `CustomPhysicalProperties` (Density, Friction, FrictionWeight, Elasticity, ElasticityWeight)
- [ ] Collision group registry — `PhysicsService` (CreateCollisionGroup, DeleteCollisionGroup, CollisionGroupSetCollidable, CollisionGroupsAreCollidable, SetPartCollisionGroup, GetCollisionGroupId/Name)
- [ ] `RelativeTo` enum — World / Attachment0 / Attachment1 (used by force/velocity constraints)

---

## 6. Humanoid & Characters

- [x] `Humanoid` component + controller system
- [ ] `HumanoidDescription` — declarative avatar spec (Head, Torso, LeftArm, RightArm, LeftLeg, RightLeg colors; HeadScale, HeightScale, WidthScale, DepthScale, ProportionScale, BodyTypeScale; all accessory slot IDs; all animation IDs)
- [ ] R15 bone hierarchy — HumanoidRootPart, UpperTorso, LowerTorso, Head, LeftUpperArm, LeftLowerArm, LeftHand, RightUpperArm, RightLowerArm, RightHand, LeftUpperLeg, LeftLowerLeg, LeftFoot, RightUpperLeg, RightLowerLeg, RightFoot
- [ ] R6 bone hierarchy — HumanoidRootPart, Head, Torso, LeftArm, RightArm, LeftLeg, RightLeg
- [ ] Humanoid state machine — Running, Jumping, Freefall, Swimming, Climbing, Seated, Dead, GettingUp, PlatformStanding, StrafingNoPhysics, RunningNoPhysics
- [ ] `WalkSpeed`, `JumpPower`, `JumpHeight`, `MaxSlopeAngle`, `HipHeight` driving Avian character controller
- [ ] `AutoJumpEnabled`, `AutoRotate`, `BreakJointsOnDeath`, `RequiresNeck` flags
- [ ] `CameraOffset` — camera offset from humanoid root
- [ ] `DisplayName` / `NameDisplayDistance` / `HealthDisplayDistance` / `NameOcclusion` / `DisplayDistanceType` / `HealthDisplayType`
- [ ] `MoveDirection`, `TargetPoint`, `WalkToPoint`, `WalkToPart` — nav inputs/outputs
- [ ] `SeatPart`, `Sit`, `PlatformStand` flags
- [ ] `RootPart` reference
- [ ] `Seat` / `VehicleSeat` — sit attachment with occupant tracking + SeatWeld
- [ ] `Accessory` / `Hat` — attachment to character bones with Handle part
- [ ] `Shirt` / `Pants` / `ShirtGraphic` — surface decal layering by template ID
- [ ] `BodyColors` — per-limb Color3 tinting (HeadColor, TorsoColor, LeftArmColor, RightArmColor, LeftLegColor, RightLegColor)
- [ ] `CharacterMesh` — body part mesh override

---

## 7. Animation

- [x] `AnimationPlayer` (plays tracks)
- [ ] `Animator` — manages multiple concurrent tracks with priority blending
- [ ] `Animation` — asset reference wrapper (AnimationId, Priority, Loop)
- [ ] `AnimationController` — non-humanoid animation host
- [ ] `AnimationTrack` full API — Play, Stop, AdjustSpeed, AdjustWeight, GetMarkerReachedSignal, TimePosition, Length, Looped, Priority, IsPlaying, Speed
- [ ] `KeyframeSequence` — animation clip container (Loop, Priority)
- [ ] `Keyframe` — time-stamped pose snapshot (Time, AddPose, RemovePose, GetPoses)
- [ ] `Pose` — per-joint transform (CFrame, EasingStyle, EasingDirection, Weight, MaskWeight, Subposes)
- [ ] `NumberPose` / `CFramePose` — typed pose sub-variants
- [ ] All animation priority levels — Core, Idle, Movement, Action, Action2, Action3, Action4
- [ ] Standard locomotion slots — Idle, Walk, Run, Jump, Fall, Swim, SwimIdle, Climb, Sit, FreeFall, Land
- [ ] Standard emote slots — ToolNone, ToolSlash, ToolOverhead, Cheer, Dance, Dance2, Dance3, Point, Wave, Laugh
- [ ] `Motor6D.DesiredAngle` driven by animation solver each frame
- [ ] Root motion extraction and application
- [ ] `KeyframeSequenceProvider` service — load KFS asset by ID

---

## 8. Terrain

- [x] Voxel terrain system scaffolded
- [ ] Material-painted voxels (multiple material layers per region)
- [ ] Full terrain material enum — Grass, Sand, Rock, Water, Ice, Ground, SmoothPlastic, Brick, Concrete, Mud, Woodplanks, Sandstone, Glacier, Salt, Limestone, Cracked Lava, Asphalt, Cobblestone, Snow, Sandstone, Ground, Slate, LeafyGrass, plus all BasePart materials
- [ ] Water voxel — WaterWaveSize, WaterWaveSpeed, WaterColor, WaterReflectance, WaterTransparency
- [ ] `Decoration` toggle + `GrassLength` density
- [ ] `SmoothingGrid` toggle
- [ ] Terrain brush — Add, Subtract, Smooth, Flatten, Paint, Replace, SeaLevel, Erode, Grow
- [ ] `Fill` + `Generate` procedural operations
- [ ] `Clear` — wipe entire terrain
- [ ] `MaterialMask` per-brush material filter
- [ ] Heightmap import (PNG → terrain, with BaseMaterial + IgnoreWater params)
- [ ] Heightmap export
- [ ] `TerrainRegion` — save + restore a terrain sub-region
- [ ] `MaxExtents` read-back

---

## 9. Lighting & Atmosphere

- [x] Atmosphere plugin (Bevy atmosphere)
- [x] Sky / HDR skybox
- [x] Property sync (ambient, brightness, etc.)
- [ ] `ClockTime` (0–24 float) → dynamic sun angle via `GeographicLatitude`
- [ ] `TimeOfDay` string parse ("HH:MM:SS") ↔ ClockTime round-trip
- [ ] Rendering technology selection — `Lighting.Technology` (Legacy / Compatibility / ShadowMap / Voxel / Future)
- [ ] `Brightness` — global light intensity multiplier
- [ ] `Ambient` + `OutdoorAmbient` — fill light colors
- [ ] `ColorShift_Top` / `ColorShift_Bottom` — sky hemisphere tint
- [ ] `EnvironmentDiffuseScale` / `EnvironmentSpecularScale` — IBL contribution
- [ ] `ExposureCompensation` — EV offset for auto-exposure
- [ ] `GlobalShadows` toggle
- [ ] `ShadowSoftness` — PCSS penumbra radius
- [ ] `FogColor` / `FogStart` / `FogEnd` → Bevy distance fog material
- [ ] `ColorCorrectionEffect` — Brightness, Contrast, Saturation, TintColor
- [ ] `BloomEffect` — Threshold, Size, Intensity
- [ ] `BlurEffect` — Size (full-screen gaussian)
- [ ] `SunRaysEffect` — Intensity, Spread
- [ ] `DepthOfFieldEffect` — NearIntensity, NearDistance, FarIntensity, FarDistance, FocusDistance, InFocusRadius
- [ ] `Atmosphere` full props — Density, Offset, Color, Decay, Glare, Haze all exposed to properties panel
- [ ] 6-face skybox — SkyboxBk/Dn/Ft/Lf/Rt/Up with custom texture assets
- [ ] `MoonTextureId` + `SunTextureId` + `MoonAngularSize` + `SunAngularSize`
- [ ] `StarCount` — procedural star density
- [ ] `CelestialBodiesShown` toggle

---

## 10. Sound System

- [x] `bevy_audio` feature enabled
- [ ] `Sound` — positioned 3D source (SoundId, Volume, PlaybackSpeed, Pitch, Looped, Playing, RollOffMaxDistance, RollOffMinDistance, RollOffMode, TimeLength, TimePosition, PlayOnRemove, EmitterSize, DistanceFactor, DopplerScale)
- [ ] `SoundGroup` — audio bus with Volume
- [ ] `SoundService` — global audio settings (AmbientReverb, DistanceFactor, DopplerScale, RolloffScale)
- [ ] `RollOffMode` — Inverse, Linear, InverseTapered, LinearSquare
- [ ] `DopplerScale` on moving sources
- [ ] `PlaybackSpeed` / `Pitch` / `TimePosition` runtime control (seek + rate)
- [ ] `ReverbSoundEffect` — room model reverb
- [ ] `EqualizerSoundEffect` — LowGain, MidGain, HighGain
- [ ] `PitchShiftSoundEffect` — Octave shift
- [ ] `ChorusSoundEffect` — Depth, Mix, Rate
- [ ] `DistortionSoundEffect` — Level
- [ ] `EchoSoundEffect` — Delay, Feedback, DryLevel, WetLevel
- [ ] `FlangeSoundEffect` — Depth, Mix, Rate
- [ ] `CompressorSoundEffect` — Threshold, Attack, Release, GainMakeup, SideChain
- [ ] `TremoloSoundEffect` — Depth, Duty, Frequency

---

## 11. GUI System

### Root containers
- [x] `BillboardGui` + `BillboardGuiMarker` (Adornee, Size, StudsOffset, StudsOffsetWorldSpace, ExtentsOffset, ExtentsOffsetWorldSpace, AlwaysOnTop, Brightness, LightInfluence, MaxDistance, DistanceLowerLimit, DistanceUpperLimit, DistanceStep, ResetOnSpawn, ZIndexBehavior)
- [x] `SurfaceGui` + `SurfaceGuiMarker` (Adornee, Face, CanvasSize, PixelsPerStud, AlwaysOnTop, Brightness, LightInfluence, SizingMode, ToolPunchThroughDistance, ZOffset, ClipsDescendants)
- [x] `ScreenGui` + Slint overlay (DisplayOrder, Enabled, IgnoreGuiInset, ResetOnSpawn, ZIndexBehavior, SafeAreaCompatibility, ScreenInsets, ClipToDeviceSafeArea, OnTopOfCoreBlur)

### Widget types
- [ ] `Frame` — basic rectangular container
- [ ] `ScrollingFrame` — CanvasSize, CanvasPosition, ScrollingEnabled, ScrollingDirection, ScrollBarThickness, ScrollBarImageColor3, ScrollBarImageTransparency, TopImage, MidImage, BottomImage, HorizontalScrollBarInset, VerticalScrollBarInset, VerticalScrollBarPosition, AutomaticCanvasSize, ElasticBehavior
- [ ] `TextLabel` — Text, Font, FontFace, TextSize, TextColor3, TextTransparency, TextStrokeColor3, TextStrokeTransparency, TextXAlignment, TextYAlignment, TextScaled, TextWrapped, TextTruncate, TextFits, TextBounds, RichText, LineHeight, MaxVisibleGraphemes, ContentText
- [ ] `TextButton` — all TextLabel props + pressable (MouseButton1Click, MouseButton1Down/Up, MouseButton2Click, GuiState, AutoButtonColor)
- [ ] `TextBox` — all TextLabel props + TextEditable, PlaceholderText, PlaceholderColor3, CursorPosition, SelectionStart, TextDirection
- [ ] `ImageLabel` — Image, ImageColor3, ImageTransparency, ImageRectOffset, ImageRectSize, ResampleMode, ScaleType, SliceCenter, SliceScale, TileSize
- [ ] `ImageButton` — all ImageLabel props + pressable
- [ ] `ViewportFrame` — renders a 3D sub-scene into a 2D GUI surface (CurrentCamera, Ambient, LightColor, LightDirection)
- [ ] `VideoFrame` — plays video asset (Video, Resolution, TimeLength, TimePosition, Looped, Playing, Volume)

### Layout & modifiers
- [ ] `UICorner` — CornerRadius (UDim) on any GuiObject
- [ ] `UIGradient` — Color (ColorSequence), Transparency (NumberSequence), Rotation, Offset, Enabled
- [ ] `UIStroke` — Color, Thickness, Transparency, LineJoinMode, ApplyStrokeMode, Enabled
- [ ] `UIPadding` — PaddingTop, PaddingRight, PaddingBottom, PaddingLeft (all UDim)
- [ ] `UIListLayout` — FillDirection, HorizontalAlignment, VerticalAlignment, SortOrder, Padding
- [ ] `UIGridLayout` — CellSize, CellPadding, FillDirection, FillDirectionMaxCells, HorizontalAlignment, VerticalAlignment, SortOrder, StartCorner
- [ ] `UIPageLayout` — paginated carousel (FillDirection, HorizontalAlignment, VerticalAlignment, SortOrder, Padding, EasingDirection, EasingStyle, Animated, TweenTime, Circular, GamepadInputEnabled)
- [ ] `UITableLayout` — table rows/columns
- [ ] `UIAspectRatioConstraint` — AspectRatio, AspectType, DominantAxis
- [ ] `UISizeConstraint` — MinSize, MaxSize (Vector2)
- [ ] `UIScale` — Scale (float)
- [ ] `UITextSizeConstraint` — MinTextSize, MaxTextSize

### Shared GuiObject properties
- [ ] AbsolutePosition, AbsoluteSize, AbsoluteRotation read-backs
- [ ] Active, Draggable, Interactable, Selectable, Visible, ZIndex, LayoutOrder
- [ ] AnchorPoint (Vector2), Position (UDim2), Size (UDim2), Rotation
- [ ] BackgroundColor3, BackgroundTransparency, BorderColor3, BorderSizePixel, BorderMode
- [ ] ClipsDescendants, SizeConstraint
- [ ] NextSelectionDown/Up/Left/Right — gamepad nav links
- [ ] SelectionImageObject, SelectionOrder
- [ ] AutoLocalize, GuiState

### Text / image specifics
- [ ] `RichText` markup — `<b>`, `<i>`, `<u>`, `<s>`, `<font color size face>`, `<stroke>`, `<br/>`
- [ ] `TextScaled` + `TextFits` auto-sizing with `UITextSizeConstraint`
- [ ] `ZIndexBehavior` (Sibling vs Global) on ScreenGui roots
- [ ] `SelectionImageObject` — custom gamepad focus ring per GuiObject
- [ ] Font asset system — TTF/OTF at runtime, 40+ named typefaces (Arial, Gotham, SourceSans, Roboto, BuilderSans, etc.)

---

## 12. Scripting & Events

- [x] `SoulScript` (Script / LocalScript) — Luau via mlua
- [x] `ModuleScript` — cached via `load_module()`; `require()` path resolution
- [x] `RemoteEvent` / `RemoteFunction` → `EustressEvent` / `EustressFunction`
- [ ] `BindableEvent` / `BindableFunction` — same-context signals (no network hop); `OnInvoke`, `Event`
- [ ] `RemoteEvent:FireAllClients` broadcast path
- [ ] `RunContext` enum on Script — Legacy / Server / Client
- [ ] `Script.Disabled` flag — prevent execution without deletion
- [ ] `Script.LinkedSource` — external source link
- [ ] `ScriptDebugger` + `BreakpointManager` + `Breakpoint` + `Watch` — debugger instance tree
- [ ] Script debugger UI panel — breakpoints list, watches, callstack, locals
- [ ] `ScriptContext.Error` + `ScriptContext.ErrorDetailed` signals
- [ ] `LintSeverity` — Error, Warning, Information, Hint (for analysis callbacks)

### RunService full API
- [x] `RunService.Heartbeat` → Bevy `Update`
- [x] `RunService.Stepped` → `PhysicsUpdate`
- [x] `RunService.RenderStepped` → Bevy `PostUpdate`
- [ ] `RunService.PreSimulation` + `RunService.PostSimulation`
- [ ] `RunService.PreRender`
- [ ] `RunService.IsServer` / `IsClient` / `IsStudio` / `IsRunning` / `IsRunMode`
- [ ] `RunService:BindToRenderStep(name, priority, fn)` — priority-ordered render callbacks
- [ ] `RunService:UnbindFromRenderStep(name)`

### DataModel global API
- [ ] `game:GetService(name)` — lazy service singleton access
- [ ] `game:FindService(name)` — non-creating lookup
- [ ] `game:IsLoaded()` + `game.Loaded` signal
- [ ] `game:WaitForChild(name, timeout)` — yield until child exists
- [ ] `game.DescendantAdded` / `game.DescendantRemoving` signals

### Workspace globals
- [ ] `workspace.Gravity` — Avian gravity vector
- [ ] `workspace.FallenPartsDestroyHeight` — auto-destroy threshold Y
- [ ] `workspace.FilteringEnabled` — always true in Eustress
- [ ] `workspace.CurrentCamera` reference
- [ ] `workspace.StreamingEnabled` + streaming params

---

## 13. Luau Type System & Standard Library

### Roblox types (constructors + methods)
- [ ] `Vector2` + `Vector2int16`
- [ ] `Vector3` + `Vector3int16`
- [ ] `CFrame` — all constructors (from position, from matrix, from quaternion, lookAt, etc.)
- [ ] `Color3` — fromRGB, fromHSV, toHSV
- [ ] `BrickColor` — full palette (100+ named colors)
- [ ] `UDim` + `UDim2`
- [ ] `Rect` (formerly Rect2D)
- [ ] `NumberRange`
- [ ] `NumberSequence` + `NumberSequenceKeypoint`
- [ ] `ColorSequence` + `ColorSequenceKeypoint`
- [ ] `PhysicalProperties` — custom density/friction/elasticity constructor
- [ ] `Ray` — origin + direction
- [ ] `Axes` / `Faces` — face/axis set flags
- [ ] `Region3` + `Region3int16`
- [ ] `TweenInfo` — EasingStyle, EasingDirection, Time, RepeatCount, Reverses, DelayTime
- [ ] `PathWaypoint` — Position + Action
- [ ] `RbxScriptSignal` — Connect, ConnectParallel, Once, Wait
- [ ] `RbxScriptConnection` — Disconnect
- [ ] `Random` — new(seed), NextNumber, NextInteger, NextUnitVector
- [ ] `Enum` + `EnumItem` — reflection (GetEnums, GetEnumItems, Name, Value)
- [ ] `Instance` — new(className), fromExisting(instance)
- [ ] `DataModel` — root game object type

### Standard globals
- [ ] `typeof(v)` — Roblox-aware type string (returns "Vector3", "CFrame", etc.)
- [ ] `newproxy(hasMetatable)` — create opaque userdata
- [ ] `task.spawn` / `task.delay` / `task.wait` / `task.defer` / `task.cancel` — modern scheduler
- [ ] `tick()`, `time()`, `elapsedTime()`, `os.clock()`, `os.time()`, `os.date()`, `os.difftime()`
- [ ] `table.freeze` / `table.isfrozen` / `table.clone` / `table.create` / `table.find` / `table.clear` / `table.move`
- [ ] `string.split(str, sep)`
- [ ] `rawget` / `rawset` / `rawequal` / `rawlen`
- [ ] `math.huge`, `math.pi`, all standard math functions

---

## 14. Services — Platform Layer

- [x] `DataStoreService` — `GetDataStore`, `GetGlobalDataStore`, `GetOrderedDataStore`
- [ ] `DataStore` full API — GetAsync, SetAsync, UpdateAsync, RemoveAsync, IncrementAsync, ListKeysAsync
- [ ] `OrderedDataStore` — sorted leaderboard store (GetSortedAsync)
- [ ] `DataStorePages` — paginated result iterator
- [x] `HttpService` — GetAsync, PostAsync, RequestAsync, JSONEncode, JSONDecode, GenerateGUID, HttpEnabled
- [x] `CollectionService` → `TagService` — AddTag, RemoveTag, HasTag, GetTags, GetTagged, GetInstanceAddedSignal, GetInstanceRemovedSignal
- [ ] `TweenService` — Create(instance, TweenInfo, properties) + `TweenService:GetValue(alpha, style, direction)`; `Tween:Play/Pause/Cancel` + `Tween.Completed`
- [ ] `TeleportService` — Teleport, TeleportToPlaceInstance, TeleportToPrivateServer, TeleportPartyAsync, ReserveServer, GetLocalPlayerTeleportData; `TeleportInitFailed` signal
- [ ] `PathfindingService` — CreatePath; `Path:ComputeAsync`, `Path:GetWaypoints`, `Path:GetBlockedSignal`, `Path.Status`; PathStatus.Success/NoPath
- [ ] `TextChatService` — ChatVersion, CreateDefaultCommands, CreateDefaultTextChannels, OnIncomingMessage, OnBubbleAdded, OnChatWindowAdded, DisplayBubble
- [ ] `TextChannel` — ShouldDeliverCallback, OnIncomingMessage, SendAsync
- [ ] `TextChatMessage` — Text, Status, Metadata
- [ ] `LocalizationService` — GetTranslatorForPlayer, GetTranslatorForLocaleAsync; Translator:Translate, Translator.LocaleId
- [ ] `LocalizationTable` — string table resource (GetTranslator)
- [ ] `BadgeService` — AwardBadge, UserHasBadgeAsync, GetBadgeInfoAsync
- [ ] `GroupService` — GetGroupInfoAsync, GetGroupsAsync, GetAlliesAsync, GetEnemiesAsync
- [ ] `ProximityPrompt` — ActionText, ObjectText, HoldDuration, MaxActivationDistance, RequiresLineOfSight, Exclusivity, UIOffset; Triggered, PromptButtonHoldBegan/Ended, PromptShown/Hidden, TriggerEnded signals
- [ ] `ProximityPromptService` — global MaxPromptsVisible; PromptShown/Hidden/TriggerEnded signals
- [ ] `Debris` — AddItem(instance, lifetime)
- [ ] `InsertService` — LoadAsset(id), LoadAssetVersion(id)
- [ ] `GeometryService` — UnionAsync, SubtractAsync, IntersectAsync (CSG as async API)
- [ ] `ContentProvider` — PreloadAsync(assets), RequestQueueSize
- [ ] `LogService` — GetLogHistory(), MessageOut signal
- [ ] `GuiService` — IsGamepadFocused, SelectedObject, GetGuiInset, CloseInspectMenu
- [ ] `KeyframeSequenceProvider` — GetKeyframeSequenceAsync, RegisterActiveKeyframeSequence
- [ ] `MarketplaceService` → `ShopService` — PromptProductPurchase, PromptGamePassPurchase, PromptPremiumPurchase, UserOwnsGamePassAsync, PlayerOwnsAsset, GetProductInfo; ProcessReceipt, PromptProductPurchaseFinished, PromptGamePassPurchaseFinished signals
- [ ] `AvatarEditorService` — GetInventoryAsync, PromptSetFavorite, PromptAllowInventoryReadAccess
- [ ] `Chat` (legacy) — InjectMessage, FilterStringAsync, FilterStringForBroadcast
- [ ] `VoiceChatService` — IsVoiceEnabledForUserIdAsync, spatial voice channel
- [ ] `TestService` — assert helpers for automated testing
- [ ] `AnalyticsService` / `RbxAnalyticsService` — fire custom analytics events
- [ ] `StudioService` — GetClassIcon, GetClassIcon, InsertIntoStarterPack, IsStudioThemeChanged
- [ ] `UserService` — GetUserInfosByUserIdsAsync

---

## 15. Players, Teams & Containers

### Players service
- [ ] `Players` singleton — LocalPlayer, PlayerAdded, PlayerRemoving, CharacterAutoLoads, MaxPlayers
- [ ] `Players:GetPlayers()` — array of all connected players
- [ ] `Players:GetPlayerByUserId(id)`
- [ ] `Players:GetUserIdFromNameAsync(name)` / `GetNameFromUserIdAsync(id)`
- [ ] `Players:GetCharacterAppearanceInfoAsync(userId)`
- [ ] `Players:GetHumanoidDescriptionFromUserId(userId)` / `GetHumanoidDescriptionFromOutfitId(outfitId)`

### Player object
- [ ] `Player:LoadCharacter()` — respawn character
- [ ] `Player:Kick(message)` — disconnect player
- [ ] `Player.CharacterAdded` / `Player.CharacterRemoving` signals
- [ ] `Player.Chatted` signal
- [ ] Player props — UserId, Name, DisplayName, AccountAge, Character, Team, TeamColor, MembershipType, HasVerifiedBadge, LocaleId, GameplayPaused
- [ ] Camera props on Player — CameraMode, CameraMaxZoomDistance, CameraMinZoomDistance
- [ ] Dev override props — DevCameraOcclusionMode, DevComputerCameraMode, DevComputerMovementMode, DevEnableMouseLock, DevTouchCameraMode, DevTouchMovementMode
- [ ] NeutralHipHeight, HealthDisplayDistance, NameDisplayDistance, ReplicationFocus, RespawnLocation

### Containers
- [ ] `PlayerGui` — per-player GUI root (ResetPlayerGuiOnSpawn)
- [ ] `PlayerScripts` — per-player LocalScript container
- [ ] `Backpack` — per-player tool inventory
- [ ] `StarterGui` — GUI template (ResetPlayerGuiOnSpawn, ShowDevelopmentGui)
- [ ] `StarterPack` — tool template container
- [ ] `StarterPlayer` — player defaults template
- [ ] `StarterPlayerScripts` — LocalScript templates
- [ ] `StarterCharacterScripts` — character LocalScript templates
- [ ] `ReplicatedFirst` — runs before everything else (no wait for game load)
- [ ] `ReplicatedStorage` — shared client+server storage
- [ ] `ServerScriptService` — server-only script container
- [ ] `ServerStorage` — server-only asset storage

### Teams
- [ ] `Teams` service singleton
- [ ] `Team` — Name, Color (BrickColor), AutoAssignable, Score
- [ ] Player.Team / Player.TeamColor assignment

### Tool
- [ ] `Tool` — CanBeDropped, Enabled, Grip (CFrame), GripForward/Pos/Right/Up, ManualActivationOnly, RequiresHandle, ToolTip; Activated, Deactivated, Equipped, Unequipped signals

---

## 16. Input & Controllers

- [x] Keyboard input
- [x] Mouse input
- [ ] `UserInputService` full event API — InputBegan, InputChanged, InputEnded with `InputObject` (KeyCode, UserInputType, UserInputState, Position, Delta)
- [ ] `UserInputService:IsKeyDown(keyCode)` / `IsMouseButtonPressed(btn)` / `IsGamepadButtonDown(gamepad, keyCode)`
- [ ] `UserInputService:GetMouseLocation()` — screen coords Vector2
- [ ] `UserInputService:GetGamepadState(gamepad)` — array of InputObjects
- [ ] `UserInputService:GetConnectedGamepads()` — list of active gamepad enums
- [ ] `UserInputService:SetNavigationGamepad(gamepad)`
- [ ] `UserInputService.MouseEnabled` / `TouchEnabled` / `GamepadEnabled` / `KeyboardEnabled` — capability flags
- [ ] `UserInputService.MouseDeltaSensitivity`
- [ ] Touch events — TouchTap, TouchMove, TouchLongPress, TouchPan, TouchPinch, TouchRotate, TouchSwipe
- [ ] Gamepad — all 14 buttons (ButtonA/B/X/Y, ButtonL1/L2/L3, ButtonR1/R2/R3, ButtonStart, ButtonSelect, DPadLeft/Right/Up/Down) + Thumbstick1/Thumbstick2 axes
- [ ] `ContextActionService:BindAction(name, fn, createTouchButton, ...)` — priority-stacked binding
- [ ] `ContextActionService:BindActionAtPriority(name, fn, createTouchButton, priority, ...)`
- [ ] `ContextActionService:UnbindAction(name)` / `GetBoundActionInfo(name)`
- [ ] `ContextActionService:SetTitle/Description/Image/Position` — mobile button customization
- [ ] `ActionResultType.Sink` / `ActionResultType.Pass`
- [ ] `HapticService` — SetMotor(gamepad, motor, intensity) rumble control
- [ ] `VRService` — IsVREnabled, GetUserCFrameEnabled, GetUserCFrame (Head/LeftHand/RightHand), RecenterUserHeadCFrame; UserCFrameChanged signal
- [ ] `GamepadService` — GamepadConnected, GamepadDisconnected
- [ ] Accelerometer input — UserInputType.Accelerometer
- [ ] Gyro input — UserInputType.Gyro

---

## 17. Camera System

- [x] 3D camera basics
- [ ] Camera type state machine — Fixed, Attach, Watch, Track, Follow, Custom, Scriptable, Orbital
- [ ] `CameraSubject` — camera auto-tracks an Adornee / Humanoid / BasePart
- [ ] Orbital camera — CameraMinZoomDistance, CameraMaxZoomDistance per Player
- [ ] `FieldOfViewMode` — Horizontal / Vertical / Diagonal / MaxAxis
- [ ] `HeadLocked` — HMD lock in VR
- [ ] `NearPlaneZ` — near clip distance
- [ ] `DiagonalFieldOfView` read-back
- [ ] `Focus` CFrame — depth-of-field focus target
- [ ] `CameraMode` — Classic (third-person offset) / LockFirstPerson
- [ ] `CameraPanMode` — Classic / EdgeBump
- [ ] `ViewportSize` read-back
- [ ] `CoordinateFrame` legacy alias for CFrame
- [ ] `HeadScale` — VR head scale

---

## 18. Networking & Replication

- [ ] Server / client split — FilteringEnabled always-on model
- [ ] `RemoteEvent:FireClient(player, ...)` / `FireServer(...)` / `FireAllClients(...)`
- [ ] `RemoteEvent.OnClientEvent` / `OnServerEvent` signals
- [ ] `RemoteFunction:InvokeClient(player, ...)` / `InvokeServer(...)`
- [ ] `RemoteFunction.OnClientInvoke` / `OnServerInvoke` callbacks
- [ ] `NetworkOwnership` — `SetNetworkOwner(player)` / `GetNetworkOwner()` / `GetNetworkOwnershipAuto()`
- [ ] `NetworkServer` / `NetworkClient` singletons
- [ ] Space streaming — StreamingEnabled, StreamingMinRadius, StreamingTargetRadius, `RequestStreamAroundAsync`
- [ ] `StreamingPauseMode` — pause character during stream gap

---

## 19. Studio Editor Tools

- [x] `SelectTool` / `SelectionPlugin`
- [x] `MoveTool` / `TranslateGizmo`
- [x] `ScaleTool` / `ScaleGizmo`
- [x] `RotateTool` / `RotateGizmo`
- [x] Combined transform gizmo
- [x] `UndoStack` — SetWaypoint, Undo, Redo with 30+ named waypoint labels
- [x] `SelectionState` resource + `SelectionChanged` event
- [x] `GizmoPlugin` + `HandleAdornment` system
- [x] `DockPanel` Slint layout (PluginGui / DockWidgetPluginGui equivalent)
- [x] `OutputPanel` stream topic (LogService.MessageOut)
- [x] `EustressTheme` resource / `ThemeColor` enum

### Missing editor features
- [ ] `DragDetector` — physics-based drag (TranslateLine, TranslatePlane, TranslatePlaneOrLine, TranslateViewPlane, RotateAxis, RotateTrackball, BestForDevice, Scriptable; ResponseStyle: Geometric, Physical, Custom)
- [ ] `DragDetector` events — DragStart, DragContinue, DragEnd, DragFrame
- [ ] Selection ID buffer — pixel-pick for click-to-select without raycasting
- [ ] Selection silhouette outline shader — edge-detect + composite blend pass
- [ ] `SelectionBox` / `SelectionSphere` world adornments (runtime, not just editor)
- [ ] `SelectionColorController` — per-selection color tint
- [ ] `SelectionDataModelListener` — reconcile selection when instance tree changes
- [ ] `Selection:Add(instances)` — append to selection (not just Set)
- [ ] Terrain brush tool panel — all 9 modes + strength/size/pivot controls
- [ ] FBX / GLTF import — LOD0–LOD4, skinning, root motion, ScaleFactor
- [ ] `ImportFbxAnimation` — animation retarget onto existing rig
- [ ] `ImportFbxRig` — humanoid rig import
- [ ] Plugin toolbar system — `Plugin:CreateToolbar`, `Plugin:CreateButton`, `Plugin:Activate/Deactivate`
- [ ] `Plugin:GetMouse()` — plugin mouse API (Hit, Target, UnitRay, Button events)
- [ ] `Plugin:OpenScript(script, lineNumber)` — jump to script
- [ ] `Plugin:OpenWikiPage(url)` — open documentation
- [ ] `Plugin:Undo()` / `Plugin:Redo()` — programmatic undo/redo
- [ ] `Plugin:CreatePluginMenu` / `Plugin:CreatePluginAction` — context menu system
- [ ] `Plugin.Unloading` signal — cleanup hook
- [ ] `ChangeHistoryService.OnUndo` / `OnRedo` signals
- [ ] `QWidgetPluginGui` — native Qt widget embed (Eustress: native Slint panel embed)
- [ ] `CoreGui.PromptBlockDialog` / `PromptUnblockDialog`
- [ ] `ScriptEditorService:GetEditorSource(script)` / `UpdateSourceAsync(script, source)`
- [ ] `ScriptEditorService` document events — TextDocumentDidOpen, TextDocumentDidClose, TextDocumentDidChange
- [ ] `ScriptEditorService:RegisterAutocompleteCallback` / `DeregisterAutocompleteCallback`
- [ ] `ScriptEditorService:RegisterScriptAnalysisCallback` / `DeregisterScriptAnalysisCallback`
- [ ] Autocomplete item kinds — Variable, Function, Keyword, Property, Class, Enum, EnumItem, Module, Color, Instance
- [ ] `DebuggerBreakpoint` / `DebuggerWatchExpression` / `DebuggerCallstack` / `DebuggerLocals` UI panels
- [ ] Find + Replace plugin (CommandBar equivalent)
- [ ] `StyleColor.*` full token set wired to theme (25+ color tokens)
- [ ] `StyleFont.*` token set (8+ font role tokens)
- [ ] Modifier states on all theme colors — Default, Disabled, Hover, Pressed, Selected
- [ ] `ProximityPrompt` editor preview in viewport

---

## 20. File Format & Serialization

- [x] TOML-based instance serialization (`_instance.toml`)
- [ ] RBXL binary reader — chunk types: META, SSTR, INST, PROP, PRNT, END, SIGN
- [ ] RBXL binary writer
- [ ] RBXM binary reader (model file variant of RBXL)
- [ ] RBXM binary writer
- [ ] RBXLX (XML) reader
- [ ] RBXLX (XML) writer
- [ ] RBXMX (XML model) reader + writer
- [ ] All binary property type tokens decoded:
  - 0x01 String, 0x02 Bool, 0x03 Int32 (interleaved), 0x04 Float (interleaved), 0x05 Double
  - 0x06 UDim, 0x07 UDim2, 0x08 Ray, 0x09 Faces, 0x0A Axes, 0x0B BrickColor
  - 0x0C Color3, 0x0D Vector2, 0x0E Vector3, 0x10 CFrame, 0x11 CFrame (quaternion)
  - 0x12 Token (Enum), 0x13 Referent, 0x14 Vector3int16, 0x15 NumberSequence
  - 0x16 ColorSequence, 0x17 NumberRange, 0x18 Rect, 0x19 PhysicalProperties
  - 0x1A Color3uint8, 0x1B Int64 (interleaved), 0x1C SharedString, 0x1D Bytecode
  - 0x1E OptionalCFrame, 0x1F UniqueId
- [ ] CFrame special orientation IDs — 36 axis-aligned presets + 0x00 arbitrary (9 floats)
- [ ] `SharedString` dedup table (SSTR chunk) — content-addressed blob store
- [ ] `Bytecode` chunk — embedded Luau compiled bytecode
- [ ] `ProtectedString` — script source obfuscation wrapper
- [ ] `UniqueId` type — 128-bit instance identity (0x1F token)
- [ ] `OptionalCFrame` — nullable CFrame (0x1E token)
- [ ] `BinaryString` property type — raw bytes in XML format (base64)
- [ ] `rbxassetid://` → local asset path resolver
- [ ] `rbxasset://` → built-in engine asset resolver
- [ ] `rbxthumb://` patterns — type=Asset/Avatar/AvatarHeadShot/BadgeIcon/BundleThumbnail/GameIcon/GamePass/GroupIcon/Outfit with w/h params

---

## 21. Enum Completeness

All enum variants must be defined in `eustress_common::classes` with correct integer values.

- [ ] `Material` — Plastic, SmoothPlastic, Wood, WoodPlanks, Brick, Cobblestone, Concrete, CorrodedMetal, DiamondPlate, Fabric, Foil, Glacier, Glass, Granite, Grass, Ground, Ice, LeafyGrass, Limestone, Marble, Metal, Mud, Neon, Pebble, Rock, Salt, Sand, Sandstone, Slate, Snow, Soil, Water, CrackedLava, Asphalt (+ Bronze, Silver, Gold — Eustress additions)
- [ ] `SurfaceType` — Smooth, Glue, Weld, Studs, Inlet, Universal, Hinge, Motor, SteppingMotor, SmoothNoOutlines
- [ ] `MeshType` — Head, Torso, Wedge, Sphere, Cylinder, FileMesh, SpecialMesh, Brick
- [ ] `PartType` — Block, Ball, Cylinder
- [ ] `RigType` — R6, R15
- [ ] `RenderFidelity` — Automatic, Precise, Performance
- [ ] `CollisionFidelity` — Default, Hull, Box, Precise
- [ ] `LevelOfDetail` — Automatic, Disabled, StreamingMesh
- [ ] `Technology` (Lighting) — Legacy, Compatibility, ShadowMap, Voxel, Future
- [ ] `EasingStyle` — Linear, Sine, Back, Bounce, Circular, Cubic, Elastic, Exponential, Quadratic, Quartic, Quintic
- [ ] `EasingDirection` — In, Out, InOut
- [ ] `TweenStatus` — Completed, Cancelled
- [ ] `AnimationPriority` — Core, Idle, Movement, Action, Action2, Action3, Action4
- [ ] `Font` — 40+ typefaces (Arial, ArialBold, SourceSans…Gotham…Roboto…BuilderSans…legacy set)
- [ ] `TextXAlignment` — Left, Center, Right
- [ ] `TextYAlignment` — Top, Center, Bottom
- [ ] `TextDirection` — Auto, LeftToRight, RightToLeft
- [ ] `TextTruncate` — None, AtEnd
- [ ] `ZIndexBehavior` — Sibling, Global
- [ ] `ScaleType` — Stretch, Slice, Tile, Fit, Crop
- [ ] `SizeConstraint` — RelativeXY, RelativeXX, RelativeYY
- [ ] `ScrollingDirection` — X, Y, XY
- [ ] `FillDirection` — Horizontal, Vertical
- [ ] `SortOrder` — LayoutOrder, Name, Custom
- [ ] `HorizontalAlignment` — Center, Left, Right
- [ ] `VerticalAlignment` — Bottom, Center, Top
- [ ] `CameraType` — Fixed, Attach, Watch, Track, Follow, Custom, Scriptable, Orbital
- [ ] `CameraMode` — Classic, LockFirstPerson
- [ ] `RollOffMode` — Inverse, Linear, InverseTapered, LinearSquare
- [ ] `ExplosionType` — NoCraters, Craters
- [ ] `NormalId` — Top, Bottom, Front, Back, Left, Right
- [ ] `AxisId` — X, Y, Z
- [ ] `UserInputType` — Keyboard, MouseButton1/2/3, MouseWheel, MouseMovement, Touch, Gamepad1–8, Focus, Accelerometer, Gyro, TextInput, InputMethod, None
- [ ] `UserInputState` — Begin, Change, End, Cancel, None
- [ ] `KeyCode` — full set (100+ values: all keyboard keys + gamepad buttons + thumbsticks)
- [ ] `Platform` — Windows, OSX, IOS, Android, XBoxOne, PS4, PS5, UWP, Studio, Unknown
- [ ] `DeviceType` — Desktop, Tablet, Phone, Console, Unknown
- [ ] `BrickColor` — full legacy palette (100+ named colors)
- [ ] `DragStyle` — TranslateLine, TranslatePlane, TranslatePlaneOrLine, TranslateViewPlane, RotateAxis, RotateTrackball, BestForDevice, Scriptable
- [ ] `ResponseStyle` — Geometric, Physical, Custom
- [ ] `Exclusivity` (ProximityPrompt) — OneGlobally, OnePerButton, AlwaysShow
- [ ] `ActionResultType` — Sink, Pass

---

## 22. Eustress Advantages (beyond Roblox parity)

Not gaps — differentiators. Don't let these get crowded out by parity work.

- [x] Single-process mode (no forced server/client split for solo games)
- [x] Half-edge mesh edit kernel (`eustress-mesh-edit`) — deformable geometry
- [x] truck-based CAD kernel (`eustress-cad`) — parametric CSG
- [x] Rune scripting (Rust-native) alongside Luau
- [x] `SoulService` — AI-native entity type (first-class NPC brain)
- [x] MCP bridge + Rust plugin API
- [x] Timeline panel — keyframed + procedural animation tracks
- [x] Stream-teed history panel — revert-to-here, not just linear undo
- [x] Workshop panel — markdown AI assistant with @mentions
- [x] eustress-embedvec — production-grade vector embedding
- [ ] SITL/HIL simulation mode (software/hardware-in-the-loop)
- [ ] ARC-AGI-3 world model integration

---

## Progress Summary

> Estimates only — audit each subsystem in a dedicated session for exact counts.
> "Done" = complete implementation, not scaffold.

| Subsystem | Est. Done | Est. Total | Est. % |
|---|---|---|---|
| Instance Model | ~8 | 28 | ~29% |
| Decals & Surface Textures | 0 | 5 | 0% |
| Handle Adornments | 0 | 11 | 0% |
| VFX — Particles & Effects | 0 | 8 | 0% |
| Physics & Constraints | ~1 | 30 | ~3% |
| Humanoid & Characters | ~1 | 18 | ~6% |
| Animation | ~1 | 14 | ~7% |
| Terrain | ~1 | 13 | ~8% |
| Lighting & Atmosphere | ~3 | 23 | ~13% |
| Sound System | ~1 | 16 | ~6% |
| GUI System | ~3 | 38 | ~8% |
| Scripting & Events | ~6 | 20 | ~30% |
| Luau Type System & Stdlib | ~3 | 25 | ~12% |
| Services — Platform Layer | ~3 | 24 | ~13% |
| Players, Teams & Containers | 0 | 22 | 0% |
| Input & Controllers | ~2 | 20 | ~10% |
| Camera System | ~1 | 13 | ~8% |
| Networking & Replication | 0 | 9 | 0% |
| Studio Editor Tools | ~11 | 34 | ~32% |
| File Format & Serialization | ~1 | 20 | ~5% |
| Enum Completeness | ~5 | 36 | ~14% |
| Eustress Advantages | ~10 | 12 | ~83% |
