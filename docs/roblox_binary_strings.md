# Roblox Binary Format — Likely Strings Reference

Comprehensive catalog of string literals found in Roblox binary files (`.rbxl`, `.rbxm`, `.rbxlx`, `.rbxmx`)
and the Roblox engine binary, organized by subsystem. Used for Eustress import/export compatibility and
format reverse-engineering.

---

## 1. File Format Headers & Markers

```
<roblox!
</roblox>
<roblox version=4
<roblox xmlns:xmime="http://www.w3.org/2005/05/xmlmime"
        xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
        xsi:noNamespaceSchemaLocation="http://www.roblox.com/roblox.xsd"
        version=4>
RBXL
RBXM
RBXLX
RBXMX
PROP
INST
PRNT
META
SSTR
SIGN
END\0
<Item class="
" referent="
</Item>
<Properties>
</Properties>
<string name="
<int name="
<float name="
<double name="
<bool name="
<token name="
<int64 name="
<BinaryString name="
<ProtectedString name="
<CoordinateFrame name="
<Vector3 name="
<Vector2 name="
<Color3 name="
<Color3uint8 name="
<UDim name="
<UDim2 name="
<Ray name="
<Faces name="
<Axes name="
<BrickColor name="
<NumberSequence name="
<ColorSequence name="
<NumberRange name="
<Rect2D name="
<PhysicalProperties name="
<SharedString name="
<OptionalCoordinateFrame name="
<Bytecode name="
<X>
<Y>
<Z>
<W>
<R>
<G>
<B>
<A>
<Scale>
<Offset>
<CX>
<CY>
<XY>
<X0>
<Y0>
<X1>
<Y1>
<min>
<max>
<t>
<n>
<i>
```

---

## 2. Class Names (Instance Types)

### Core / Abstract
```
Instance
PVInstance
Model
Folder
Configuration
StringValue
IntValue
NumberValue
BoolValue
ObjectValue
Color3Value
Vector3Value
CFrameValue
BrickColorValue
RayValue
```

### World & Physics
```
Workspace
Part
MeshPart
SpecialMesh
UnionOperation
NegateOperation
WedgePart
CornerWedgePart
TrussPart
SpawnLocation
Seat
VehicleSeat
SkateboardPlatform
Platform
Decal
Texture
SelectionBox
SelectionSphere
Handles
ArcHandles
SurfaceSelection
BoxHandleAdornment
ConeHandleAdornment
CylinderHandleAdornment
ImageHandleAdornment
LineHandleAdornment
SphereHandleAdornment
WireframeHandleAdornment
SelectionPartLasso
SelectionPointLasso
BodyForce
BodyGyro
BodyPosition
BodyThrust
BodyVelocity
BodyAngularVelocity
RocketPropulsion
Constraint
BallSocketConstraint
HingeConstraint
RodConstraint
RopeConstraint
SliderConstraint
SpringConstraint
TorsionSpringConstraint
UniversalConstraint
WeldConstraint
Motor6D
ManualWeld
ManualGlue
Snap
Glue
Weld
Attachment
Bone
```

### Humanoid & Characters
```
Humanoid
HumanoidDescription
HumanoidRootPart
Accessory
Hat
Shirt
Pants
ShirtGraphic
CharacterMesh
BodyColors
Pose
KeyframeSequence
Keyframe
NumberPose
CFramePose
Animator
Animation
AnimationController
AnimationTrack
```

### Terrain
```
Terrain
TerrainRegion
```

### Lighting & Atmosphere
```
Lighting
Sky
Atmosphere
ColorCorrectionEffect
BloomEffect
BlurEffect
SunRaysEffect
DepthOfFieldEffect
```

### Sound
```
Sound
SoundService
SoundGroup
EqualizerSoundEffect
ReverbSoundEffect
ChorusSoundEffect
DistortionSoundEffect
EchoSoundEffect
FlangeSoundEffect
PitchShiftSoundEffect
CompressorSoundEffect
TremoloSoundEffect
```

### GUI / UI
```
ScreenGui
SurfaceGui
BillboardGui
Frame
ScrollingFrame
TextLabel
TextButton
TextBox
ImageLabel
ImageButton
ViewportFrame
VideoFrame
UIAspectRatioConstraint
UICorner
UIGradient
UIGridLayout
UIListLayout
UIPadding
UIPageLayout
UIScale
UISizeConstraint
UIStroke
UITableLayout
UITextSizeConstraint
SelectionImageObject
```

### Scripting
```
Script
LocalScript
ModuleScript
RemoteEvent
RemoteFunction
BindableEvent
BindableFunction
ScriptDebugger
BreakpointManager
Breakpoint
Watch
```

### Services
```
ReplicatedFirst
ReplicatedStorage
ServerScriptService
ServerStorage
StarterGui
StarterPack
StarterPlayer
StarterPlayerScripts
StarterCharacterScripts
Teams
Team
Chat
LocalizationService
LocalizationTable
Players
Player
PlayerGui
PlayerScripts
Backpack
Tool
NetworkClient
NetworkServer
RunService
UserInputService
ContextActionService
TweenService
PathfindingService
DataStoreService
GlobalDataStore
OrderedDataStore
DataStore
DataStorePages
HttpService
MarketplaceService
BadgeService
GroupService
AssetService
InsertService
CollectionService
TeleportService
GeometryService
MaterialService
MaterialVariant
TextService
TextChannel
TextChatService
TextChatMessage
VoiceChatService
PolicyService
AvatarEditorService
CoreGui
CoreScript
RbxAnalyticsService
TestService
AnalyticsService
StudioService
PluginManager
Plugin
PluginGui
PluginToolbar
PluginToolbarButton
PluginMenu
PluginAction
DockWidgetPluginGui
QWidgetPluginGui
Selection
```

### Input & Controllers
```
VRService
HapticService
GamepadService
KeyboardService
MouseService
TouchInputService
```

### Camera
```
Camera
```

### Joints & Constraints (detailed)
```
JointInstance
DynamicRotate
RotateP
RotateV
Rotate
Motor
Weld
Snap
Glue
ManualWeld
ManualGlue
```

### Advanced Physics
```
Explosion
ForceField
Fire
Smoke
Sparkles
Trail
Beam
ParticleEmitter
LineForce
VectorForce
Torque
AlignOrientation
AlignPosition
AngularVelocity
LinearVelocity
Plane
```

### Video & Streaming
```
VideoFrame
```

---

## 3. Property Names (by class group)

### BasePart (all Part variants)
```
Name
ClassName
Archivable
Parent
Anchored
AssemblyAngularVelocity
AssemblyCenterOfMass
AssemblyLinearVelocity
AssemblyMass
AssemblyRootPart
BackSurface
BackSurfaceInput
BottomSurface
BottomSurfaceInput
BrickColor
CFrame
CastShadow
CollisionGroupId
Color
CurrentPhysicalProperties
CustomPhysicalProperties
EnableFluidForces
FrontSurface
FrontSurfaceInput
LeftSurface
LeftSurfaceInput
LocalTransparencyModifier
Locked
Mass
Massless
Material
MaterialVariant
PivotOffset
Position
ReceiveAge
Reflectance
RenderFidelity
ResizeIncrement
ResizeableFaces
RightSurface
RightSurfaceInput
RootPriority
RotVelocity
Rotation
Size
Transparency
TopSurface
TopSurfaceInput
Velocity
```

### MeshPart
```
MeshId
TextureID
CollisionFidelity
DoubleSided
HasJointOffset
JointOffset
MeshSize
InitialSize
```

### SpecialMesh
```
MeshType
MeshId
TextureId
Scale
Offset
VertexColor
```

### Model
```
LevelOfDetail
ModelLod
PrimaryPart
WorldPivot
```

### Humanoid
```
AutoJumpEnabled
AutoRotate
BreakJointsOnDeath
CameraOffset
DisplayDistanceType
DisplayName
HealthDisplayDistance
HealthDisplayType
HipHeight
JumpHeight
JumpPower
MaxHealth
MaxSlopeAngle
MoveDirection
NameDisplayDistance
NameOcclusion
PlatformStand
RequiresNeck
RigType
RootPart
SeatPart
Sit
TargetPoint
WalkSpeed
WalkToPart
WalkToPoint
Health
```

### HumanoidDescription
```
BackAccessory
ClimbAnimation
FaceAccessory
FallAnimation
FrontAccessory
GraphicTShirt
HairAccessory
HatAccessory
Head
HeadColor
HeadScale
LeftArm
LeftArmColor
LeftLeg
LeftLegColor
NeckAccessory
Pants
ProportionScale
RightArm
RightArmColor
RightLeg
RightLegColor
RunAnimation
Shirt
ShouldersAccessory
SwimAnimation
Torso
TorsoColor
WaistAccessory
WalkAnimation
WidthScale
HeightScale
DepthScale
BodyTypeScale
```

### Lighting
```
Ambient
Brightness
ClockTime
ColorShift_Bottom
ColorShift_Top
EnvironmentDiffuseScale
EnvironmentSpecularScale
ExposureCompensation
FogColor
FogEnd
FogStart
GeographicLatitude
GlobalShadows
OutdoorAmbient
ShadowSoftness
Technology
TimeOfDay
```

### Atmosphere
```
Density
Offset
Color
Decay
Glare
Haze
```

### Sky
```
CelestialBodiesShown
MoonAngularSize
MoonTextureId
SkyboxBk
SkyboxDn
SkyboxFt
SkyboxLf
SkyboxRt
SkyboxUp
StarCount
SunAngularSize
SunTextureId
```

### Sound
```
SoundId
Volume
PlaybackSpeed
Pitch
Looped
Playing
RollOffMaxDistance
RollOffMinDistance
RollOffMode
TimeLength
TimePosition
PlayOnRemove
EmitterSize
DistanceFactor
DopplerScale
```

### GUI (shared)
```
AbsolutePosition
AbsoluteRotation
AbsoluteSize
Active
AnchorPoint
AutoLocalize
BackgroundColor3
BackgroundTransparency
BorderColor3
BorderMode
BorderSizePixel
ClipsDescendants
Draggable
GuiState
Interactable
LayoutOrder
NextSelectionDown
NextSelectionLeft
NextSelectionRight
NextSelectionUp
Position
Rotation
Selectable
SelectionImageObject
SelectionOrder
Size
SizeConstraint
Visible
ZIndex
```

### TextLabel / TextButton / TextBox
```
ContentText
CursorPosition
Font
FontFace
LineHeight
MaxVisibleGraphemes
PlaceholderColor3
PlaceholderText
RichText
SelectionStart
Text
TextBounds
TextColor3
TextDirection
TextEditable
TextFits
TextScaled
TextSize
TextStrokeColor3
TextStrokeTransparency
TextTransparency
TextTruncate
TextWrapped
TextXAlignment
TextYAlignment
```

### ImageLabel / ImageButton
```
Image
ImageColor3
ImageRectOffset
ImageRectSize
ImageTransparency
ResampleMode
ScaleType
SliceCenter
SliceScale
TileSize
```

### BillboardGui
```
Active
Adornee
AlwaysOnTop
Brightness
ClipsDescendants
CurrentDistance
DistanceLowerLimit
DistanceStep
DistanceUpperLimit
Enabled
ExtentsOffset
ExtentsOffsetWorldSpace
LightInfluence
MaxDistance
ResetOnSpawn
Size
SizeOffset
StudsOffset
StudsOffsetWorldSpace
ZIndexBehavior
```

### SurfaceGui
```
Adornee
AlwaysOnTop
Brightness
CanvasSize
ClipsDescendants
Enabled
Face
LightInfluence
PixelsPerStud
SizingMode
ToolPunchThroughDistance
ZOffset
```

### ScreenGui
```
ClipToDeviceSafeArea
DisplayOrder
Enabled
IgnoreGuiInset
OnTopOfCoreBlur
ResetOnSpawn
SafeAreaCompatibility
ScreenInsets
ZIndexBehavior
```

### ScrollingFrame
```
AutomaticCanvasSize
BottomImage
CanvasPosition
CanvasSize
ElasticBehavior
HorizontalScrollBarInset
MidImage
ScrollBarImageColor3
ScrollBarImageTransparency
ScrollBarThickness
ScrollingDirection
ScrollingEnabled
TopImage
VerticalScrollBarInset
VerticalScrollBarPosition
```

### UICorner
```
CornerRadius
```

### UIGradient
```
Color
Enabled
Offset
Rotation
Transparency
```

### UIStroke
```
ApplyStrokeMode
Color
Enabled
LineJoinMode
Thickness
Transparency
```

### UIPadding
```
PaddingBottom
PaddingLeft
PaddingRight
PaddingTop
```

### UIListLayout
```
FillDirection
HorizontalAlignment
Padding
SortOrder
VerticalAlignment
```

### UIGridLayout
```
CellPadding
CellSize
FillDirection
FillDirectionMaxCells
HorizontalAlignment
SortOrder
StartCorner
VerticalAlignment
```

### UIAspectRatioConstraint
```
AspectRatio
AspectType
DominantAxis
```

### UISizeConstraint
```
MaxSize
MinSize
```

### UIScale
```
Scale
```

### Script / LocalScript / ModuleScript
```
Source
LinkedSource
Disabled
RunContext
```

### RemoteEvent / RemoteFunction / BindableEvent / BindableFunction
```
OnClientEvent
OnServerEvent
OnClientInvoke
OnServerInvoke
OnInvoke
Event
```

### Tool
```
CanBeDropped
Enabled
Grip
GripForward
GripPos
GripRight
GripUp
ManualActivationOnly
RequiresHandle
ToolTip
```

### Animation
```
AnimationId
Priority
Loop
```

### Keyframe / KeyframeSequence
```
Time
Priority
Loop
```

### Pose
```
CFrame
EasingDirection
EasingStyle
MaskWeight
Weight
```

### Attachment
```
Axis
CFrame
Orientation
Position
SecondaryAxis
WorldAxis
WorldCFrame
WorldOrientation
WorldPosition
WorldSecondaryAxis
```

### Beam
```
Attachment0
Attachment1
Brightness
Color
CurveSize0
CurveSize1
Enabled
FaceCamera
LightEmission
LightInfluence
Segments
Texture
TextureLength
TextureMode
TextureSpeed
Transparency
Width0
Width1
ZOffset
```

### Trail
```
Attachment0
Attachment1
Brightness
Color
Enabled
FaceCamera
Lifetime
LightEmission
LightInfluence
MaxLength
MinLength
Texture
TextureLength
TextureMode
Transparency
WidthScale
```

### ParticleEmitter
```
Acceleration
Brightness
Color
Drag
EmissionDirection
Enabled
FlipbookFramerate
FlipbookLayout
FlipbookMode
FlipbookStartRandom
Lifetime
LightEmission
LightInfluence
LockedToPart
Orientation
Rate
RotSpeed
Rotation
Size
Speed
SpreadAngle
Squash
Texture
TimeScale
Transparency
VelocityInheritance
WindAffectsDrag
ZOffset
```

### Fire / Smoke / Sparkles
```
Color
Enabled
Heat
SecondaryColor
Size
TimeScale
```

### Explosion
```
BlastPressure
BlastRadius
DestroyJointRadiusPercent
ExplosionType
Position
Visible
```

### ForceField
```
Visible
```

### Terrain
```
Decoration
GrassLength
MaxExtents
SmoothingGrid
WaterColor
WaterReflectance
WaterTransparency
WaterWaveSize
WaterWaveSpeed
```

### Camera
```
CameraSubject
CameraType
CoordinateFrame
DiagonalFieldOfView
FieldOfView
FieldOfViewMode
Focus
HeadLocked
HeadScale
MaxAxisFieldOfView
NearPlaneZ
ViewportSize
```

### Player
```
AccountAge
CameraMaxZoomDistance
CameraMinZoomDistance
CameraMode
CharacterAppearanceId
CharacterAppearanceLoaded
DevCameraOcclusionMode
DevComputerCameraMode
DevComputerMovementMode
DevEnableMouseLock
DevTouchCameraMode
DevTouchMovementMode
DisplayName
GameplayPaused
HasVerifiedBadge
HealthDisplayDistance
LocaleId
MembershipType
NameDisplayDistance
NeutralHipHeight
ReplicationFocus
RespawnLocation
Team
TeamColor
UserId
```

### Teams / Team
```
AutoAssignable
Color
Score
```

### PhysicalProperties (custom)
```
CustomPhysicalProperties
Density
Elasticity
ElasticityWeight
Friction
FrictionWeight
```

### Constraint (shared)
```
Active
Attachment0
Attachment1
Color
Enabled
Visible
```

### SpringConstraint
```
Coils
Damping
FreeLength
LimitsEnabled
MaxForce
MaxLength
MinLength
Radius
Stiffness
```

### HingeConstraint
```
ActuatorType
AngularResponsiveness
AngularSpeed
AngularVelocity
CurrentAngle
LimitsEnabled
LowerAngle
Motor
MotorMaxAcceleration
MotorMaxTorque
Restitution
ServoMaxTorque
TargetAngle
UpperAngle
```

### Motor6D
```
C0
C1
CurrentAngle
DesiredAngle
MaxVelocity
Part0
Part1
```

### WeldConstraint
```
Active
Part0
Part1
```

---

## 4. Enum Names & Values

### Material
```
Plastic
SmoothPlastic
Wood
WoodPlanks
Brick
Cobblestone
Concrete
CorrodedMetal
DiamondPlate
Fabric
Foil
Glacier
Glass
Granite
Grass
Ground
Ice
LeafyGrass
Limestone
Marble
Metal
Mud
Neon
Pebble
Rock
Salt
Sand
Sandstone
Slate
SmoothPlastic
Snow
Soil
Water
Wood
Cracked Lava
Asphalt
CrackedLava
```

### SurfaceType
```
Smooth
Glue
Weld
Studs
Inlet
Universal
Hinge
Motor
SteppingMotor
SmoothNoOutlines
```

### MeshType
```
Head
Torso
Wedge
Sphere
Cylinder
FileMesh
SpecialMesh
Brick
```

### RigType (Humanoid)
```
R6
R15
```

### Font
```
Legacy
Arial
ArialBold
SourceSans
SourceSansBold
SourceSansSemibold
SourceSansLight
SourceSansItalic
Bodoni
Highway
SciFi
Antique
Cartoon
Code
GothamBook
Gotham
GothamMedium
GothamBold
GothamBlack
Oswald
RobotoCondensed
Roboto
RobotoMono
BuilderSans
BuilderSansBold
BuilderSansMedium
Creepster
DenkOne
Fondamento
FredokaOne
GrenzeGotisch
IndieFlower
JosefinSans
Jura
Kalam
LuckiestGuy
Merriweather
Michroma
Nunito
Oswald
PatrickHand
PermanentMarker
Raleway
RedHatMono
RobotoCondensed
Sarpanch
SpecialElite
TitilliumWeb
Ubuntu
Unknown
```

### TextXAlignment
```
Left
Center
Right
```

### TextYAlignment
```
Top
Center
Bottom
```

### ZIndexBehavior
```
Sibling
Global
```

### ScaleType
```
Stretch
Slice
Tile
Fit
Crop
```

### SizeConstraint
```
RelativeXY
RelativeXX
RelativeYY
```

### ScrollingDirection
```
X
Y
XY
```

### FillDirection
```
Horizontal
Vertical
```

### SortOrder
```
LayoutOrder
Name
Custom
```

### HorizontalAlignment
```
Center
Left
Right
```

### VerticalAlignment
```
Bottom
Center
Top
```

### AlignmentMode (UIListLayout)
```
Automatic
```

### CameraType
```
Fixed
Attach
Watch
Track
Follow
Custom
Scriptable
Orbital
```

### CameraMode (Player)
```
Classic
LockFirstPerson
```

### PartType
```
Block
Ball
Cylinder
```

### RenderFidelity
```
Automatic
Precise
Performance
```

### CollisionFidelity
```
Default
Hull
Box
Precise
```

### LevelOfDetail
```
Automatic
Disabled
StreamingMesh
```

### Technology (Lighting)
```
Legacy
Compatibility
ShadowMap
Future
Voxel
```

### EasingStyle
```
Linear
Sine
Back
Bounce
Circular
Cubic
Elastic
Exponential
Quadratic
Quartic
Quintic
```

### EasingDirection
```
In
Out
InOut
```

### TweenStatus
```
Completed
Cancelled
```

### AnimationPriority
```
Core
Idle
Movement
Action
Action2
Action3
Action4
```

### KeyCode (UserInputService)
```
Unknown
Backspace
Tab
Clear
Return
Pause
Escape
Space
ExclamationMark
QuotedDouble
Hash
Dollar
Percent
Ampersand
Quote
LeftParenthesis
RightParenthesis
Asterisk
Plus
Comma
Minus
Period
Slash
Zero
One
Two
Three
Four
Five
Six
Seven
Eight
Nine
Colon
Semicolon
LessThan
Equals
GreaterThan
Question
At
LeftBracket
BackSlash
RightBracket
Caret
Underscore
Backquote
A B C D E F G H I J K L M N O P Q R S T U V W X Y Z
LeftCurly
Pipe
RightCurly
Tilde
Delete
F1 F2 F3 F4 F5 F6 F7 F8 F9 F10 F11 F12
Up Down Left Right
Insert Home End PageUp PageDown
NumLock CapsLock ScrollLock
LeftShift RightShift
LeftControl RightControl
LeftAlt RightAlt
LeftSuper RightSuper
LeftMeta RightMeta
Print Pause Menu
ButtonA ButtonB ButtonX ButtonY
ButtonL1 ButtonL2 ButtonL3
ButtonR1 ButtonR2 ButtonR3
ButtonStart ButtonSelect
DPadLeft DPadRight DPadUp DPadDown
Thumbstick1 Thumbstick2
```

### UserInputType
```
Keyboard
MouseButton1
MouseButton2
MouseButton3
MouseWheel
MouseMovement
Touch
Gamepad1 Gamepad2 Gamepad3 Gamepad4 Gamepad5 Gamepad6 Gamepad7 Gamepad8
Focus
Accelerometer
Gyro
TextInput
InputMethod
None
```

### UserInputState
```
Begin
Change
End
Cancel
None
```

### RollOffMode
```
Inverse
Linear
InverseTapered
LinearSquare
```

### SoundType
```
NoAudio
Alarm
Notification
Music
SoundEffect
```

### ExplosionType
```
NoCraters
Craters
```

### NormalId (Faces)
```
Top
Bottom
Front
Back
Left
Right
```

### AxisId
```
X
Y
Z
```

### BrickColor (selected)
```
White
Grey
Light yellow
Brick yellow
Light green (Mint)
Light reddish violet
Pastel Blue
Light orange brown
Nougat
Bright red
Med. reddish violet
Bright blue
Bright yellow
Earth orange
Black
Dark grey
Dark green
Medium green
Lig. Yellowich orange
Bright green
Dark orange
Light bluish violet
Transparent
Tr. Red
Tr. Lg blue
Tr. Blue
Tr. Yellow
Light blue
Tr. Flu. Reddish orange
Sand green
Sand heather
Sand blue violet
Sand yellow
Earth blue
Earth green
Tr. Flu. Green
Phosph. White
Light red
Medium red
Medium blue
Light grey
Bright violet
Br. Yellowish orange
Bright orange
Bright bluish green
Earth Yellow
Bright yellowish green
Bright reddish lilac
White
Medium stone grey
Dark stone grey
Light stone grey
Medium brown
Dark brown
```

### Font style (TextService)
```
Normal
Bold
Italic
```

### Platform
```
Windows
OSX
IOS
Android
XBoxOne
PS4
PS5
UWP
Studio
Unknown
```

### DeviceType
```
Desktop
Tablet
Phone
Console
Unknown
```

---

## 5. Service & Singleton Names

```
RobloxReplicatedStorage
JointsService
AdService
AppUpdateService
AssetDeliveryProxy
AvatarEditorService
BadgeService
BrowserService
CaptureService
Chat
ClickDetectorService
ClusterPacketCache
ContentProvider
ContextActionService
ControllerService
CookiesService
CoreGui
CorePackages
DataStoreService
Debris
EventIngestService
FriendService
GamePassService
GeometryService
GroupService
GuiService
HttpRbxApiService
HttpService
InsertService
KeyframeSequenceProvider
Lighting
LocalizationService
LogService
MarketplaceService
MaterialService
MemStorageService
MessageBusService
NetworkClient
NetworkServer
NotificationService
PathfindingService
PhysicsService
Players
PluginDebugService
PluginGuiService
PluginManager
PolicyService
ProximityPromptService
RbxAnalyticsService
ReplicatedFirst
ReplicatedStorage
RobloxPluginGuiService
RunService
ScriptContext
ScriptDebugger
Selection
ServerScriptService
ServerStorage
SessionService
SoundService
StarterGui
StarterPack
StarterPlayer
Teams
TeleportService
TestService
TextChatService
TextService
TweenService
UserInputService
UserService
VRService
VoiceChatService
Workspace
```

---

## 6. Lua / Luau Standard Globals & APIs

### Game services access
```
game
game:GetService
workspace
script
plugin
```

### Core globals
```
print
warn
error
assert
pcall
xpcall
require
tostring
tonumber
type
typeof
rawget
rawset
rawequal
rawlen
select
unpack
table.unpack
next
pairs
ipairs
setmetatable
getmetatable
newproxy
tick
time
elapsedTime
os.clock
os.date
os.time
os.difftime
math.huge
math.pi
math.abs
math.ceil
math.floor
math.max
math.min
math.pow
math.random
math.randomseed
math.sqrt
math.sin
math.cos
math.tan
math.asin
math.acos
math.atan
math.atan2
math.exp
math.log
math.log10
math.modf
math.fmod
string.byte
string.char
string.find
string.format
string.gmatch
string.gsub
string.len
string.lower
string.match
string.rep
string.reverse
string.sub
string.upper
string.split
table.insert
table.remove
table.sort
table.concat
table.move
table.create
table.find
table.clear
table.freeze
table.isfrozen
table.clone
```

### Roblox types
```
Vector2
Vector2int16
Vector3
Vector3int16
CFrame
Color3
BrickColor
UDim
UDim2
Rect
NumberRange
NumberSequence
NumberSequenceKeypoint
ColorSequence
ColorSequenceKeypoint
PhysicalProperties
Ray
Axes
Faces
Region3
Region3int16
TweenInfo
PathWaypoint
RbxScriptSignal
RbxScriptConnection
Random
Enum
EnumItem
Instance
DataModel
```

### Roblox global APIs
```
Instance.new
Instance.fromExisting
game:GetService
game:FindService
game:IsLoaded
game:WaitForChild
game.Loaded
game.DescendantAdded
game.DescendantRemoving
workspace.CurrentCamera
workspace.FallenPartsDestroyHeight
workspace.FilteringEnabled
workspace.Gravity
workspace.StreamingEnabled
```

### RunService
```
RunService.Heartbeat
RunService.Stepped
RunService.RenderStepped
RunService.PreSimulation
RunService.PostSimulation
RunService.PreRender
RunService.IsServer
RunService.IsClient
RunService.IsStudio
RunService.IsRunning
RunService.IsRunMode
RunService:BindToRenderStep
RunService:UnbindFromRenderStep
```

### TweenService
```
TweenService:Create
TweenService:GetValue
Tween:Play
Tween:Pause
Tween:Cancel
Tween.Completed
```

### DataStoreService
```
DataStoreService:GetDataStore
DataStoreService:GetGlobalDataStore
DataStoreService:GetOrderedDataStore
DataStore:GetAsync
DataStore:SetAsync
DataStore:UpdateAsync
DataStore:RemoveAsync
DataStore:IncrementAsync
DataStore:ListKeysAsync
GlobalDataStore:GetAsync
GlobalDataStore:SetAsync
GlobalDataStore:UpdateAsync
GlobalDataStore:RemoveAsync
GlobalDataStore:IncrementAsync
```

### Players
```
Players.LocalPlayer
Players.PlayerAdded
Players.PlayerRemoving
Players.CharacterAutoLoads
Players.MaxPlayers
Players:GetPlayers
Players:GetPlayerByUserId
Players:GetUserIdFromNameAsync
Players:GetNameFromUserIdAsync
Players:GetCharacterAppearanceInfoAsync
Players:GetHumanoidDescriptionFromUserId
Players:GetHumanoidDescriptionFromOutfitId
Player:LoadCharacter
Player:Kick
Player.CharacterAdded
Player.CharacterRemoving
Player.Chatted
Player.UserId
Player.Name
Player.DisplayName
Player.Character
Player.Team
Player.TeamColor
```

### UserInputService
```
UserInputService.InputBegan
UserInputService.InputChanged
UserInputService.InputEnded
UserInputService.TouchTap
UserInputService.TouchMove
UserInputService.TouchLongPress
UserInputService.TouchPan
UserInputService.TouchPinch
UserInputService.TouchRotate
UserInputService.TouchSwipe
UserInputService.GamepadConnected
UserInputService.GamepadDisconnected
UserInputService:GetMouseLocation
UserInputService:IsKeyDown
UserInputService:IsMouseButtonPressed
UserInputService:IsGamepadButtonDown
UserInputService:GetGamepadState
UserInputService:GetConnectedGamepads
UserInputService:SetNavigationGamepad
UserInputService.MouseEnabled
UserInputService.TouchEnabled
UserInputService.GamepadEnabled
UserInputService.KeyboardEnabled
UserInputService.MouseDeltaSensitivity
```

### ContextActionService
```
ContextActionService:BindAction
ContextActionService:BindActionAtPriority
ContextActionService:UnbindAction
ContextActionService:GetBoundActionInfo
ContextActionService:SetTitle
ContextActionService:SetDescription
ContextActionService:SetImage
ContextActionService:SetPosition
ActionResultType.Sink
ActionResultType.Pass
```

### HttpService
```
HttpService:GetAsync
HttpService:PostAsync
HttpService:RequestAsync
HttpService:JSONEncode
HttpService:JSONDecode
HttpService:GenerateGUID
HttpService.HttpEnabled
```

### MarketplaceService
```
MarketplaceService:PromptProductPurchase
MarketplaceService:PromptGamePassPurchase
MarketplaceService:PromptPremiumPurchase
MarketplaceService:UserOwnsGamePassAsync
MarketplaceService:PlayerOwnsAsset
MarketplaceService:GetProductInfo
MarketplaceService.ProcessReceipt
MarketplaceService.PromptProductPurchaseFinished
MarketplaceService.PromptGamePassPurchaseFinished
```

### TeleportService
```
TeleportService:Teleport
TeleportService:TeleportToPlaceInstance
TeleportService:TeleportToPrivateServer
TeleportService:TeleportPartyAsync
TeleportService:ReserveServer
TeleportService:GetLocalPlayerTeleportData
TeleportService.TeleportInitFailed
```

### CollectionService
```
CollectionService:AddTag
CollectionService:RemoveTag
CollectionService:HasTag
CollectionService:GetTags
CollectionService:GetTagged
CollectionService:GetInstanceAddedSignal
CollectionService:GetInstanceRemovedSignal
```

### PathfindingService
```
PathfindingService:CreatePath
Path:ComputeAsync
Path:GetWaypoints
Path:GetBlockedSignal
Path.Status
PathStatus.Success
PathStatus.NoPath
```

### PhysicsService
```
PhysicsService:CreateCollisionGroup
PhysicsService:DeleteCollisionGroup
PhysicsService:CollisionGroupSetCollidable
PhysicsService:CollisionGroupsAreCollidable
PhysicsService:GetCollisionGroupId
PhysicsService:GetCollisionGroupName
PhysicsService:SetPartCollisionGroup
```

---

## 7. Asset URL Patterns

```
rbxassetid://
rbxasset://
rbxhttp://
rbxgameasset://
rbxthumb://type=Asset&id=
rbxthumb://type=Avatar&id=
rbxthumb://type=AvatarHeadShot&id=
rbxthumb://type=BadgeIcon&id=
rbxthumb://type=BundleThumbnail&id=
rbxthumb://type=GameIcon&id=
rbxthumb://type=GamePass&id=
rbxthumb://type=GroupIcon&id=
rbxthumb://type=Outfit&id=
&w=420&h=420
&w=150&h=150
&w=100&h=100
https://assetdelivery.roblox.com/v1/asset/?id=
https://assetdelivery.roblox.com/v2/assetId/
https://apis.roblox.com/
https://www.roblox.com/asset/?id=
https://www.roblox.com/games/
https://www.roblox.com/users/
https://thumbnails.roblox.com/
https://catalog.roblox.com/
https://economy.roblox.com/
```

---

## 8. Animation Track Strings

```
HumanoidRootPart
UpperTorso
LowerTorso
Head
LeftUpperArm
LeftLowerArm
LeftHand
RightUpperArm
RightLowerArm
RightHand
LeftUpperLeg
LeftLowerLeg
LeftFoot
RightUpperLeg
RightLowerLeg
RightFoot
Idle
Walk
Run
Jump
Fall
Swim
SwimIdle
Climb
Sit
FreeFall
Land
ToolNone
ToolSlash
ToolOverhead
Cheer
Dance
Dance2
Dance3
Point
Wave
Laugh
```

---

## 9. Localization & Text Chat

```
TextChatService.ChatVersion
TextChatService.CreateDefaultCommands
TextChatService.CreateDefaultTextChannels
TextChatService.OnIncomingMessage
TextChatService.OnBubbleAdded
TextChatService.OnChatWindowAdded
TextChatService:DisplayBubble
TextChannel.ShouldDeliverCallback
TextChannel.OnIncomingMessage
TextChannel:SendAsync
TextChatMessage.Text
TextChatMessage.Status
TextChatMessage.Metadata
LocalizationService:GetTranslatorForPlayer
LocalizationService:GetTranslatorForLocaleAsync
LocalizationTable:GetTranslator
Translator:Translate
Translator.LocaleId
```

---

## 10. Studio / Plugin Binary Strings

```
Plugin:CreateToolbar
Plugin:CreateButton
Plugin:Activate
Plugin:Deactivate
Plugin:GetMouse
Plugin:CreateDockWidgetPluginGui
Plugin:CreatePluginMenu
Plugin:CreatePluginAction
Plugin:OpenWikiPage
Plugin:OpenScript
Plugin:ImportFbxAnimation
Plugin:ImportFbxRig
Plugin:Undo
Plugin:Redo
Plugin.Unloading
ChangeHistoryService:SetWaypoint
ChangeHistoryService:Undo
ChangeHistoryService:Redo
ChangeHistoryService.OnUndo
ChangeHistoryService.OnRedo
Selection:Set
Selection:Get
Selection.SelectionChanged
CoreGui.PromptBlockDialog
CoreGui.PromptUnblockDialog
```

---

## 11. Physics & Constraints — Detailed Strings

```
Constraint
BallSocketConstraint
HingeConstraint
RodConstraint
RopeConstraint
SliderConstraint
SpringConstraint
TorsionSpringConstraint
UniversalConstraint
WeldConstraint
NoCollisionConstraint
AlignOrientation
AlignPosition
AngularVelocity
LinearVelocity
LineForce
Plane
Torque
VectorForce
Attachment0
Attachment1
Enabled
Active
Visible
LimitsEnabled
ReactionTorqueEnabled
TwistLimitsEnabled
UpperAngle
LowerAngle
TwistUpperAngle
TwistLowerAngle
MaxTorque
MaxForce
FreeLength
Stiffness
Damping
Coils
Radius
ActuatorType
AngularSpeed
AngularVelocity
TargetAngle
ServoMaxTorque
MotorMaxTorque
MotorMaxAcceleration
Restitution
Motor
Servo
None
ApplyAtCenterOfMass
InverseSquareLaw
Magnitude
RelativeTo
World
Attachment0
Attachment1
RigidityEnabled
```

---

## 12. DataStore Key Patterns (common in Roblox games)

```
Player_
User_
Data_
Save_
Stats_
Coins
Cash
Gems
Experience
Level
Inventory
Settings
Achievements
Wins
Losses
PlayTime
LastSeen
Version
```

---

## 13. Internal Engine / Replication Strings

```
Workspace.StreamingEnabled
Workspace.StreamingMinRadius
Workspace.StreamingTargetRadius
Workspace.StreamingPauseMode
Workspace.RequestStreamAroundAsync
game:GetService("NetworkServer")
game:GetService("NetworkClient")
RemoteEvent:FireClient
RemoteEvent:FireServer
RemoteEvent:FireAllClients
RemoteFunction:InvokeClient
RemoteFunction:InvokeServer
ReplicatedStorage
ServerScriptService
ServerStorage
FilteringEnabled
LocalTransparencyModifier
NetworkOwnership
SetNetworkOwner
GetNetworkOwner
GetNetworkOwnershipAuto
```

---

## 14. Format Discriminants (binary chunk types, Roblox binary)

Binary RBXL chunk type identifiers (4-byte ASCII):
```
META
SSTR
INST
PROP
PRNT
END
SIGN
```

Data type tokens (internal enum, uint8 in binary prop chunks):
```
0x00 — Invalid/None
0x01 — String
0x02 — Bool
0x03 — Int32 (transformed)
0x04 — Float (transformed)
0x05 — Double
0x06 — UDim
0x07 — UDim2
0x08 — Ray
0x09 — Faces
0x0A — Axes
0x0B — BrickColor
0x0C — Color3
0x0D — Vector2
0x0E — Vector3
0x10 — CFrame
0x11 — CFrame (quaternion form)
0x12 — Token (Enum)
0x13 — Referent
0x14 — Vector3int16
0x15 — NumberSequence
0x16 — ColorSequence
0x17 — NumberRange
0x18 — Rect
0x19 — PhysicalProperties
0x1A — Color3uint8
0x1B — Int64 (transformed)
0x1C — SharedString
0x1D — Bytecode (Luau)
0x1E — OptionalCFrame
0x1F — UniqueId
```

CFrame special matrix IDs (byte, precoded orientations):
```
0x02 — identity
0x03 through 0x24 — 36 axis-aligned rotations
0x00 — arbitrary (followed by 9 floats)
```

---

## 15. Eustress Mapping Reference

| Roblox String | Eustress Equivalent |
|---|---|
| `StudsOffset` | `units_offset` (BillboardGui) |
| `StudsOffsetWorldSpace` | `units_offset_world_space` |
| `SmoothPlastic` | `Material::SmoothPlastic` |
| `SpecialMesh` / `FileMesh` | asset mesh path via `MeshId` |
| `rbxassetid://` | local asset path lookup |
| `BrickColor` | `Color3` (converted) |
| `FilteringEnabled` | always true in Eustress |
| `StreamingEnabled` | `SpaceStreamingMode` |
| `DataStoreService` | `eustress_common::datastore` |
| `RemoteEvent` / `RemoteFunction` | `EustressEvent` / `EustressFunction` |
| `Players.LocalPlayer` | `LocalPlayer` singleton |
| `Humanoid` | `Humanoid` + `HumanoidController` system |
| `AnimationTrack:Play` | `AnimationPlayer::play()` |
| `TweenService:Create` | `TweenBuilder` |
| `CollectionService` | `TagService` (ECS tag system) |
| `PhysicsService` | collision group registry |
| `RunService.Heartbeat` | `bevy::app::Update` schedule |
| `RunService.Stepped` | `PhysicsUpdate` schedule |
| `RunService.RenderStepped` | `bevy::app::PostUpdate` |
| `workspace.Gravity` | `bevy_rapier::RapierConfiguration.gravity` |
| `TextChatService` | `ChatService` |
| `BillboardGui` | `BillboardGui` + `BillboardGuiMarker` |
| `SurfaceGui` | `SurfaceGui` + `SurfaceGuiMarker` |
| `ScreenGui` | `ScreenGui` + Slint overlay |
| `Motor6D` | `Motor6D` + Bevy skeleton joint |
| `Animator` | `AnimationPlayer` |
| `Terrain` | voxel terrain system |
| `Lighting.Technology` | `RenderingMode` (Forward / Deferred) |
| `Atmosphere` | atmosphere plugin |
| `Sky` | `SkyPlugin` + HDR skybox |
| `Script` / `LocalScript` | `SoulScript` (Luau via mlua) |
| `ModuleScript` | `ModuleScript` cached via `load_module()` |
| `RemoteEvent:FireServer` | `EcsQueue::push_event()` |
| `DataStore:GetAsync` | `DatastoreGet` Luau binding |
| `HttpService:RequestAsync` | `HttpRequest` Luau binding |
| `MarketplaceService` | `ShopService` / Stripe integration |

---

## 16. Studio Tools — Strings, Modes, and Internal Mechanics

Studio exposes its tool system through a combination of string-keyed actions, mode names, mouse cursor
identifiers, drag handle names, and plugin API surface. Everything below is a string literal that appears
in the Studio binary, plugin APIs, or serialized state.

---

### 16.1 Built-in Tool Names (Toolbar / Ribbon)

These are the canonical string identifiers Studio uses internally for each built-in editing tool.

```
Select
Move
Scale
Rotate
Transform
Paint
Smooth
Add
Subtract
Grow
Erode
Flatten
Replace
SeaLevel
Undo
Redo
Cut
Copy
Paste
Duplicate
Delete
Group
Ungroup
Anchor
Lock
Play
Pause
Stop
PlayHere
Run
```

### 16.2 Mouse / Cursor State Strings

Studio sets the active cursor by string name. These appear in mouse state machines and plugin mouse APIs.

```
Default
OpenedHand
ClosedHand
PointingHand
SizeNS
SizeEW
SizeNESW
SizeNWSE
SizeAll
Crosshair
IBeam
Wait
Forbidden
Eraser
Pencil
Pipette
Bucket
Zoom
ZoomIn
ZoomOut
RotateArcball
RotateAxis
DragArrow
DragPlane
ResizeHandle
```

### 16.3 Studio Action Names (ChangeHistoryService / keyboard shortcuts)

These strings appear as waypoint names, action IDs in keybind tables, and undo/redo stack labels.

```
Select Tool
Move Tool
Scale Tool
Rotate Tool
Play
Pause
Stop
Play Here
Run
Test
Reset
Undo
Redo
Cut
Copy
Paste
Duplicate
Delete
Select All
Deselect All
Group
Ungroup
Lock
Unlock
Anchor
UnAnchor
Insert Object
Insert Model
Find
Find Next
Find Previous
Replace
Go to Script Error
Toggle Comment
Format Selection
Indent
Outdent
Zoom In
Zoom Out
Zoom To Extents
Zoom To Selection
Reset Camera
Toggle Full Screen
Screenshot
Record Video
Open Script
Close Script
Save
Save As
Publish
Publish As
Import
Export
Export Selection
New Place
Open Place
Close Place
```

### 16.4 Plugin Mouse API Strings

Returned by `Plugin:GetMouse()` and set by plugin authors.

```
Mouse.Button1Down
Mouse.Button1Up
Mouse.Button2Down
Mouse.Button2Up
Mouse.Move
Mouse.WheelForward
Mouse.WheelBackward
Mouse.Idle
Mouse.KeyDown
Mouse.KeyUp
Mouse.Hit
Mouse.Target
Mouse.UnitRay
Mouse.Origin
Mouse.ViewSizeX
Mouse.ViewSizeY
Mouse.X
Mouse.Y
Mouse.Delta
Mouse.Icon
```

### 16.5 DragDetector Strings (new in 2023 API)

```
DragDetector
DragStyle
ResponseStyle
MaxActivationDistance
Orientation
AxisList
VelocityConstraint
BoundingBox
DragStart
DragContinue
DragEnd
DragFrame
PermissionPolicy
DragStyle.TranslateLine
DragStyle.TranslatePlane
DragStyle.TranslatePlaneOrLine
DragStyle.TranslateViewPlane
DragStyle.RotateAxis
DragStyle.RotateTrackball
DragStyle.BestForDevice
DragStyle.Scriptable
ResponseStyle.Geometric
ResponseStyle.Physical
ResponseStyle.Custom
```

### 16.6 Gizmo / Handle API Strings

Handles and ArcHandles expose these strings for face/axis identification.

```
Handles
ArcHandles
Faces
Axes
NormalId.Top
NormalId.Bottom
NormalId.Front
NormalId.Back
NormalId.Left
NormalId.Right
Axis.X
Axis.Y
Axis.Z
MouseButton1Down
MouseButton1Up
MouseEnter
MouseLeave
MouseDrag
MouseHover
```

### 16.7 SelectionService Strings

```
Selection:Set
Selection:Get
Selection:Add
Selection.SelectionChanged
Studio.SelectionChanged
ActiveTool
SelectedObjects
```

### 16.8 Studio Property Widget Strings

Property panel metadata strings used to drive the Properties widget.

```
Studio.GetPropertyChangedSignal
Studio.Theme
Studio.Theme.GetColor
Studio.Theme.GetIcon
Studio.Theme.GetImage
StyleColor.MainBackground
StyleColor.CategoryItem
StyleColor.Item
StyleColor.Hover
StyleColor.Selection
StyleColor.SelectionBorder
StyleColor.Mid
StyleColor.Bright
StyleColor.Dark
StyleColor.DialogMainButton
StyleColor.DialogButton
StyleColor.RibbonTab
StyleColor.RibbonTabTopBar
StyleColor.Button
StyleColor.ButtonBorder
StyleColor.ButtonText
StyleColor.MainText
StyleColor.SubText
StyleColor.TitlebarText
StyleColor.Titlebar
StyleColor.Toolbar
StyleColor.BrightText
StyleColor.DimmedText
StyleColor.Shadow
StyleColor.Light
StyleColor.Mid
StyleColor.Dark
StyleColor.Highlight
StyleColor.Border
StyleColor.Error
StyleColor.Warning
StyleColor.MainButton
StyleColor.InputFieldBorder
StyleColor.InputFieldBackground
StyleFont.SplitterLabel
StyleFont.BoldTitle
StyleFont.Title
StyleFont.Header
StyleFont.SubHeader
StyleFont.Normal
StyleFont.NormalAnnotation
StyleFont.Category
StyleFont.SplitterLabel
Modifier.Default
Modifier.Disabled
Modifier.Hover
Modifier.Pressed
Modifier.Selected
```

### 16.9 Viewport / Camera Manipulation Strings

These appear in camera rig code, focus/zoom logic, and Studio's navigation state machine.

```
CameraType.Scriptable
CameraType.Custom
CameraType.Track
CameraType.Follow
CameraType.Fixed
CameraType.Orbital
CameraType.Attach
CameraType.Watch
CameraMode.Classic
CameraMode.LockFirstPerson
Enum.CameraPanMode.Classic
Enum.CameraPanMode.EdgeBump
Workspace.CurrentCamera
CurrentCamera.CFrame
CurrentCamera.Focus
CurrentCamera.ViewportSize
CurrentCamera.FieldOfView
CurrentCamera.NearPlaneZ
CurrentCamera.CameraType
CurrentCamera.CameraSubject
```

### 16.10 Undo System Strings (ChangeHistoryService)

Waypoint names Studio bakes into the undo stack. Eustress uses these as `UndoStack` event kind labels.

```
Select
Move
Rotate
Scale
Delete
Insert
Paste
Duplicate
Group
Ungroup
Anchor
Anchor Change
Lock
Terrain Paint
Terrain Smooth
Terrain Add
Terrain Subtract
Property Change
Name Change
Parent Change
Color Change
Material Change
Surface Change
Resize
Transform
Model Pivot Change
CFrame Change
Joint Create
Joint Delete
Constraint Create
Constraint Delete
Attachment Create
Attachment Delete
Script Edit
Publish
Reset
Undo
Redo
Waypoint
```

### 16.11 Terrain Tool Strings

Terrain brush tool identifies its modes with these strings internally.

```
Add
Subtract
Grow
Erode
Smooth
Flatten
Paint
SeaLevel
Replace
Fill
Generate
Clear
ImportHeightmap
ExportHeightmap
BaseMaterial
IgnoreWater
FlattenMode
FixedHeight
Height
Strength
BaseSize
PivotType
PlaneLock
EditTarget
Workspace
Localize
MaterialMask
```

### 16.12 Model Import / FBX Strings

Strings that appear in FBX import dialogs, model import pipelines, and animation retargeting.

```
ImportFbxAnimation
ImportFbxRig
ImportOptions
ImportAsRiggedMesh
ImportLODs
ImportSingleMesh
ScaleFactor
MeshType
RigType
AnimationNaming
InplaceAnimation
AverageFrameRate
FrameRate
24
30
60
120
RootMotion
Skin
Skinned
Static
MeshPart
SpecialMesh
FileMesh
Triangulate
Weld
ZipContent
FbxVersion
LevelOfDetail
LOD0
LOD1
LOD2
LOD3
LOD4
```

### 16.13 Scripting / Script Editor Strings

Script editor action and autocomplete metadata strings.

```
ScriptEditor
ScriptEditorService
ScriptEditorService:GetEditorSource
ScriptEditorService:UpdateSourceAsync
ScriptEditorService.TextDocumentDidOpen
ScriptEditorService.TextDocumentDidClose
ScriptEditorService.TextDocumentDidChange
ScriptEditorService:RegisterAutocompleteCallback
ScriptEditorService:DeregisterAutocompleteCallback
ScriptEditorService:RegisterScriptAnalysisCallback
ScriptEditorService:DeregisterScriptAnalysisCallback
AutocompleteRequest
AutocompleteResponse
AutocompleteItem
AutocompleteItemKind
AutocompleteItemKind.Variable
AutocompleteItemKind.Function
AutocompleteItemKind.Keyword
AutocompleteItemKind.Property
AutocompleteItemKind.Class
AutocompleteItemKind.Enum
AutocompleteItemKind.EnumItem
AutocompleteItemKind.Module
AutocompleteItemKind.Color
AutocompleteItemKind.Instance
ScriptContext.Error
ScriptContext.ErrorDetailed
LintSeverity.Error
LintSeverity.Warning
LintSeverity.Information
LintSeverity.Hint
FindPlugin
ReplacePlugin
CommandBar
Output
Diagnostics
```

### 16.14 Studio Ribbon / UI Layout Strings

How Studio names its ribbon tabs, groups, and button IDs internally.

```
Home
Model
Avatar
Test
View
Plugins
RibbonTab
RibbonGroup
ToolbarButton
ToggleButton
ComboBox
Separator
Tools
Edit
Insert
Terrain
Material
Surface
Constraint
Camera
Lighting
Effects
Audio
Animation
Rigging
Skinning
IK
Script
Testing
Simulation
Network
Performance
Diagnostics
```

### 16.15 Studio Output / Diagnostics Strings

These appear in Output window, diagnostics panel, and error reporting.

```
Output
Error
Warning
Information
Print
Clear
Filter
Search
Copy
MessageType.Output
MessageType.Info
MessageType.Warning
MessageType.Error
LogService.MessageOut
LogService:GetLogHistory
DebuggerManager
DebuggerWatchExpression
DebuggerBreakpoint
DebuggerCallstack
DebuggerLocals
LineNumber
Source
Column
Status
Running
Paused
Stopped
```

### 16.16 ProximityPrompt Strings

```
ProximityPrompt
ProximityPromptService
ActionText
ObjectText
HoldDuration
MaxActivationDistance
RequiresLineOfSight
Exclusivity
ClickablePrompt
ProximityPromptStyle
UIOffset
Triggered
PromptButtonHoldBegan
PromptButtonHoldEnded
PromptShown
PromptHidden
TriggerEnded
Exclusivity.OneGlobally
Exclusivity.OnePerButton
Exclusivity.AlwaysShow
```

### 16.17 Eustress Studio Tool Mapping

| Roblox Studio String | Eustress Equivalent |
|---|---|
| `Select` tool | `SelectTool` / `SelectionPlugin` |
| `Move` tool | `MoveTool` / `TranslateGizmo` |
| `Scale` tool | `ScaleTool` / `ScaleGizmo` |
| `Rotate` tool | `RotateTool` / `RotateGizmo` |
| `Transform` tool | combined gizmo (translate + rotate + scale) |
| `ChangeHistoryService:SetWaypoint` | `UndoStack::push()` |
| `ChangeHistoryService:Undo` | `UndoStack::undo()` |
| `ChangeHistoryService:Redo` | `UndoStack::redo()` |
| `Selection:Set` / `.SelectionChanged` | `SelectionState` resource + `SelectionChanged` event |
| `Plugin:GetMouse()` | `ToolMouse` Bevy resource |
| `Handles` / `ArcHandles` | `GizmoPlugin` + `HandleAdornment` system |
| `DragDetector` | `DragDetectorPlugin` (physics drag) |
| `SelectionBox` | `SelectionBoxPlugin` + outline shader |
| `Studio.Theme.GetColor` | `EustressTheme` resource |
| `StyleColor.*` | `ThemeColor` enum |
| `ScriptEditorService` | LSP bridge (Rune LSP / mlua) |
| `ProximityPrompt` | `ProximityPromptPlugin` |
| `Terrain` tools | `TerrainBrush` system |
| `ImportFbxRig` | `import_fbx_rig()` in eustress-cad |
| `RibbonTab` / `ToolbarButton` | Slint `ribbon.slint` components |
| `LogService.MessageOut` | `OutputPanel` stream topic |
| `DebuggerBreakpoint` | `SoulScript` Luau breakpoint system |
| `DragStyle.TranslateLine` | `GizmoAxis::Single` constraint mode |
| `DragStyle.TranslatePlane` | `GizmoAxis::Plane` constraint mode |
| `DragStyle.RotateTrackball` | `RotateGizmo::Trackball` mode |
| `NormalId.Top/Bottom/etc` | `NormalId` enum in `eustress_common::classes` |
| `MouseButton1Down` on Handles | `GizmoPickEvent::GrabStart` |
| `MouseDrag` on Handles | `GizmoPickEvent::Drag` |
| `Plugin:CreateToolbar` | `ToolbarPlugin::register()` |
| `Plugin:CreateButton` | `ToolbarButton` Slint component |
| `PluginGui` / `DockWidgetPluginGui` | `DockPanel` in Slint layout |
