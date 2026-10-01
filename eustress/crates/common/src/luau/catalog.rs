//! What a Luau script can reach, as data.
//!
//! The Play VM resolves an instance's methods and signals through
//! [`method_applies`] and [`is_event`], which read [`MEMBERS`]. The script
//! editor completes, signs and hovers from the same entries, so it offers
//! exactly what a running script can reach. Each entry names the classes it
//! belongs to, its parameters, what it returns and what it does.
//!
//! Properties come from the tree: [`properties_of`] reads the class defaults
//! and the properties the tree computes. Services come from the tree's
//! service list. The objects that are not instances (signals, the mouse,
//! input objects, Vector3 and the other values), the libraries, the globals
//! and the enums a script reads are listed here as well.
//!
//! The tests run every entry against a live Play VM, so an entry is here
//! only if a script can reach it.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::datamodel::{class_is_a, default_properties, is_base_part, is_service_class, DmValue, SERVICE_CLASSES};

// ── Instance members ─────────────────────────────────────────────────────────

/// What a member is, and how the VM reaches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `inst:Name(...)`, from the VM's method table.
    Method,
    /// `inst.Name:Connect(...)`, a signal every matching instance has.
    Event,
    /// `inst.Name = function(...) end`: a function the engine calls.
    Callback,
    /// A service method the prelude implements (`TweenService:Create`).
    ServiceMethod,
    /// A service signal the prelude implements (`RunService.Heartbeat`).
    ServiceEvent,
}

/// The classes a member belongs to: these exactly, or anything that `IsA`
/// one of `is_a`.
#[derive(Clone, Copy, Debug)]
pub struct On {
    pub exact: &'static [&'static str],
    pub is_a: &'static [&'static str],
}

impl On {
    pub fn contains(&self, class: &str) -> bool {
        self.exact.contains(&class) || self.is_a.iter().any(|base| class_is_a(class, base))
    }
}

const ANY: On = On { exact: &[], is_a: &["Instance"] };

const fn only(classes: &'static [&'static str]) -> On {
    On { exact: classes, is_a: &[] }
}

const fn kind_of(bases: &'static [&'static str]) -> On {
    On { exact: &[], is_a: bases }
}

#[derive(Clone, Copy, Debug)]
pub struct Member {
    pub name: &'static str,
    pub kind: Kind,
    pub on: On,
    /// Parameters as Luau writes them: `name: string, recursive: boolean?`.
    pub params: &'static str,
    /// What a method returns, or what an event hands its handlers.
    /// `Self` is the receiver's own class, `ArgClass` the class named by the
    /// first argument.
    pub returns: &'static str,
    pub doc: &'static str,
    /// Still accepted, never offered.
    pub deprecated: bool,
}

const fn method(name: &'static str, on: On, params: &'static str, returns: &'static str, doc: &'static str) -> Member {
    Member { name, kind: Kind::Method, on, params, returns, doc, deprecated: false }
}

const fn old(name: &'static str, on: On, params: &'static str, returns: &'static str, doc: &'static str) -> Member {
    Member { name, kind: Kind::Method, on, params, returns, doc, deprecated: true }
}

const fn event(name: &'static str, on: On, params: &'static str, doc: &'static str) -> Member {
    Member { name, kind: Kind::Event, on, params, returns: "", doc, deprecated: false }
}

const fn callback(name: &'static str, on: On, params: &'static str, returns: &'static str, doc: &'static str) -> Member {
    Member { name, kind: Kind::Callback, on, params, returns, doc, deprecated: false }
}

const fn service(name: &'static str, on: On, params: &'static str, returns: &'static str, doc: &'static str) -> Member {
    Member { name, kind: Kind::ServiceMethod, on, params, returns, doc, deprecated: false }
}

const fn service_event(name: &'static str, on: On, params: &'static str, doc: &'static str) -> Member {
    Member { name, kind: Kind::ServiceEvent, on, params, returns: "", doc, deprecated: false }
}

const PART: On = kind_of(&["BasePart"]);
const MODEL: On = kind_of(&["Model"]);
const PVINSTANCE: On = kind_of(&["BasePart", "Model"]);
const HUMANOID: On = only(&["Humanoid"]);
const ANIMATES: On = only(&["Humanoid", "Animator", "AnimationController"]);
const TRACK: On = only(&["AnimationTrack"]);
const GUI: On = kind_of(&["GuiButton", "GuiObject"]);
const INPUT: On = On { exact: &["UserInputService"], is_a: &["GuiObject"] };
/// Both kinds of remote event; the session sends an UnreliableRemoteEvent's
/// calls on its unreliable lane.
const REMOTE_EVENT: On = only(&["RemoteEvent", "UnreliableRemoteEvent"]);

pub static MEMBERS: &[Member] = &[
    // Every instance.
    method("Destroy", ANY, "", "", "Removes the instance and its descendants for good."),
    method("Clone", ANY, "", "Self", "A copy of the instance and its descendants, not yet parented."),
    method("ClearAllChildren", ANY, "", "", "Destroys every child."),
    method("FindFirstChild", ANY, "name: string, recursive: boolean?", "Instance?", "The first child with this name, or nil."),
    method("FindFirstChildOfClass", ANY, "className: string", "ArgClass?", "The first child of exactly this class, or nil."),
    method("FindFirstChildWhichIsA", ANY, "className: string, recursive: boolean?", "ArgClass?", "The first child that IsA this class, or nil."),
    method("FindFirstAncestor", ANY, "name: string", "Instance?", "The nearest ancestor with this name, or nil."),
    method("FindFirstAncestorOfClass", ANY, "className: string", "ArgClass?", "The nearest ancestor of exactly this class, or nil."),
    method("FindFirstAncestorWhichIsA", ANY, "className: string", "ArgClass?", "The nearest ancestor that IsA this class, or nil."),
    method("FindFirstDescendant", ANY, "name: string", "Instance?", "The first descendant with this name, or nil."),
    method("GetChildren", ANY, "", "{Instance}", "The children, in order."),
    method("GetDescendants", ANY, "", "{Instance}", "Every descendant, depth first."),
    method("IsA", ANY, "className: string", "boolean", "Whether the instance is this class or inherits from it."),
    method("IsDescendantOf", ANY, "ancestor: Instance", "boolean", "Whether the instance sits under `ancestor`."),
    method("IsAncestorOf", ANY, "descendant: Instance", "boolean", "Whether `descendant` sits under the instance."),
    method("GetFullName", ANY, "", "string", "The dotted path from the DataModel, as `Workspace.Base.Door`."),
    method("WaitForChild", ANY, "name: string, timeout: number?", "Instance", "The child with this name, yielding until it exists."),
    method("GetAttribute", ANY, "name: string", "any", "The attribute's value, or nil."),
    method("SetAttribute", ANY, "name: string, value: any", "", "Sets an attribute; nil removes it."),
    method("GetAttributes", ANY, "", "{[string]: any}", "Every attribute by name."),
    method("GetAttributeChangedSignal", ANY, "name: string", "RBXScriptSignal", "Fires when this attribute changes."),
    method("GetPropertyChangedSignal", ANY, "property: string", "RBXScriptSignal", "Fires when this property changes."),
    method("AddTag", ANY, "tag: string", "", "Adds a CollectionService tag."),
    method("RemoveTag", ANY, "tag: string", "", "Removes a CollectionService tag."),
    method("HasTag", ANY, "tag: string", "boolean", "Whether the instance has this tag."),
    method("GetTags", ANY, "", "{string}", "The instance's tags."),
    method("GetDebugId", ANY, "", "string", "A stable id for this instance in the session."),
    old("isA", ANY, "className: string", "boolean", "Old spelling of IsA."),
    old("findFirstChild", ANY, "name: string, recursive: boolean?", "Instance?", "Old spelling of FindFirstChild."),
    old("children", ANY, "", "{Instance}", "Old spelling of GetChildren."),
    old("remove", ANY, "", "", "Old form of Destroy."),
    old("Remove", ANY, "", "", "Old form of Destroy."),
    // Parts and models.
    method("GetPivot", PVINSTANCE, "", "CFrame", "The pivot the instance moves by."),
    method("PivotTo", PVINSTANCE, "targetCFrame: CFrame", "", "Moves the instance so its pivot lands on `targetCFrame`."),
    method("ApplyImpulse", PART, "impulse: Vector3", "", "Pushes the assembly at its center of mass."),
    method("ApplyAngularImpulse", PART, "impulse: Vector3", "", "Spins the assembly."),
    method("ApplyImpulseAtPosition", PART, "impulse: Vector3, position: Vector3", "", "Pushes the assembly at a world point."),
    method("GetMass", PART, "", "number", "The part's mass in kg: its density times its volume."),
    method("SetNetworkOwner", PART, "player: Player?", "", "Hands the part's physics to a player, or to the server with nil."),
    method("GetNetworkOwner", PART, "", "Player?", "The player simulating the part, or nil for the server."),
    method("SetNetworkOwnershipAuto", PART, "", "", "Lets the engine pick who simulates the part."),
    method("BreakJoints", PART, "", "", "Removes the part's joints."),
    method("GetTouchingParts", PART, "", "{BasePart}", "Parts overlapping this one."),
    method("GetConnectedParts", PART, "recursive: boolean?", "{BasePart}", "Parts joined to this one."),
    method("GetRootPart", PART, "", "BasePart", "The root part of the assembly."),
    method("CanCollideWith", PART, "part: BasePart", "boolean", "Whether the two parts collide."),
    method("GetBoundingBox", MODEL, "", "(CFrame, Vector3)", "The model's bounding box: its orientation and size."),
    method("GetExtentsSize", MODEL, "", "Vector3", "The size of the model's bounding box."),
    method("MoveTo", MODEL, "position: Vector3", "", "Moves the model so its pivot sits at `position`."),
    method("MoveTo", HUMANOID, "location: Vector3, part: BasePart?", "", "Walks the character to `location`."),
    method("SetPrimaryPartCFrame", MODEL, "cframe: CFrame", "", "Moves the model by its PrimaryPart."),
    method("GetPrimaryPartCFrame", MODEL, "", "CFrame", "The PrimaryPart's CFrame."),
    method("TranslateBy", MODEL, "delta: Vector3", "", "Moves the model by `delta`."),
    method("GetModelCFrame", MODEL, "", "CFrame", "The center of the model's bounding box."),
    // Humanoids.
    method("TakeDamage", HUMANOID, "amount: number", "", "Lowers Health by `amount`."),
    method("Move", HUMANOID, "direction: Vector3, relativeToCamera: boolean?", "", "Walks in a direction until told otherwise."),
    method("ChangeState", HUMANOID, "state: Enum.HumanoidStateType", "", "Puts the humanoid in a state."),
    method("GetState", HUMANOID, "", "Enum.HumanoidStateType", "The humanoid's current state."),
    method("UnequipTools", HUMANOID, "", "", "Puts held tools back in the Backpack."),
    method("EquipTool", HUMANOID, "tool: Tool", "", "Holds a tool."),
    method("SetStateEnabled", HUMANOID, "state: Enum.HumanoidStateType, enabled: boolean", "", "Allows or forbids a state."),
    method("GetStateEnabled", HUMANOID, "state: Enum.HumanoidStateType", "boolean", "Whether a state is allowed."),
    method("LoadAnimation", ANIMATES, "animation: Animation", "AnimationTrack", "A track that plays the animation on this rig."),
    method("GetPlayingAnimationTracks", ANIMATES, "", "{AnimationTrack}", "The tracks playing now."),
    // Seats and text boxes.
    method("Sit", only(&["Seat", "VehicleSeat"]), "humanoid: Humanoid", "", "Seats a humanoid."),
    method("CaptureFocus", only(&["TextBox"]), "", "", "Gives the text box the keyboard."),
    method("ReleaseFocus", only(&["TextBox"]), "submitted: boolean?", "", "Takes the keyboard back."),
    method("IsFocused", only(&["TextBox"]), "", "boolean", "Whether the text box has the keyboard."),
    // Sound and animation.
    method("Play", only(&["Sound"]), "", "", "Plays the sound from the start."),
    method("Stop", only(&["Sound"]), "", "", "Stops the sound and rewinds it."),
    method("Pause", only(&["Sound"]), "", "", "Pauses the sound where it is."),
    method("Resume", only(&["Sound"]), "", "", "Carries on from where the sound paused."),
    method("Play", TRACK, "fadeTime: number?, weight: number?, speed: number?", "", "Plays the animation."),
    method("Stop", TRACK, "fadeTime: number?", "", "Fades the animation out."),
    method("AdjustSpeed", TRACK, "speed: number?", "", "Changes playback speed."),
    method("AdjustWeight", TRACK, "weight: number?, fadeTime: number?", "", "Changes how strongly the animation applies."),
    method("GetMarkerReachedSignal", TRACK, "name: string", "RBXScriptSignal", "Fires at each marker with this name."),
    method("GetTimeOfKeyframe", TRACK, "keyframeName: string", "number", "The time of the named keyframe."),
    method("GetKeyframes", only(&["KeyframeSequence"]), "", "{Keyframe}", "The keyframes, in time order."),
    method("AddKeyframe", only(&["KeyframeSequence"]), "keyframe: Keyframe", "", "Adds a keyframe."),
    method("RemoveKeyframe", only(&["KeyframeSequence"]), "keyframe: Keyframe", "", "Removes a keyframe."),
    method("GetPoses", only(&["Keyframe"]), "", "{Pose}", "The keyframe's top-level poses."),
    method("AddPose", only(&["Keyframe"]), "pose: Pose", "", "Adds a pose."),
    method("RemovePose", only(&["Keyframe"]), "pose: Pose", "", "Removes a pose."),
    method("GetMarkers", only(&["Keyframe"]), "", "{KeyframeMarker}", "The keyframe's markers."),
    method("AddMarker", only(&["Keyframe"]), "marker: KeyframeMarker", "", "Adds a marker."),
    method("RemoveMarker", only(&["Keyframe"]), "marker: KeyframeMarker", "", "Removes a marker."),
    method("GetSubPoses", only(&["Pose"]), "", "{Pose}", "The poses under this one."),
    method("AddSubPose", only(&["Pose"]), "pose: Pose", "", "Adds a pose under this one."),
    method("RemoveSubPose", only(&["Pose"]), "pose: Pose", "", "Removes a pose under this one."),
    method("RegisterKeyframeSequence", only(&["KeyframeSequenceProvider"]), "keyframeSequence: KeyframeSequence", "string", "An id an Animation can play the sequence by."),
    method("RegisterActiveKeyframeSequence", only(&["KeyframeSequenceProvider"]), "keyframeSequence: KeyframeSequence", "string", "An id that follows later edits to the sequence."),
    method("GetKeyframeSequenceAsync", only(&["KeyframeSequenceProvider"]), "assetId: string", "KeyframeSequence", "The sequence behind an animation id."),
    // Effects and terrain.
    method("Emit", only(&["ParticleEmitter"]), "particleCount: number?", "", "Emits particles now."),
    method("Clear", only(&["ParticleEmitter", "Terrain"]), "", "", "Removes every particle, or all terrain."),
    method("FillBall", only(&["Terrain"]), "center: Vector3, radius: number, material: Enum.Material", "", "Fills a sphere with a material."),
    method("FillBlock", only(&["Terrain"]), "cframe: CFrame, size: Vector3, material: Enum.Material", "", "Fills a box with a material."),
    method("FillCylinder", only(&["Terrain"]), "cframe: CFrame, height: number, radius: number, material: Enum.Material", "", "Fills a cylinder with a material."),
    method("FillRegion", only(&["Terrain"]), "region: Region3, resolution: number, material: Enum.Material", "", "Fills a region with a material."),
    method("ReplaceMaterial", only(&["Terrain"]), "region: Region3, resolution: number, sourceMaterial: Enum.Material, targetMaterial: Enum.Material", "", "Swaps one material for another in a region."),
    method("ReadVoxels", only(&["Terrain"]), "region: Region3, resolution: number", "({any}, {any})", "The materials and occupancies in a region."),
    method("WriteVoxels", only(&["Terrain"]), "region: Region3, resolution: number, materials: {any}, occupancy: {any}", "", "Writes materials and occupancies into a region."),
    method("CellCenterToWorld", only(&["Terrain"]), "x: number, y: number, z: number", "Vector3", "The world position of a cell's center."),
    method("CellCornerToWorld", only(&["Terrain"]), "x: number, y: number, z: number", "Vector3", "The world position of a cell's corner."),
    method("WorldToCell", only(&["Terrain"]), "position: Vector3", "Vector3", "The cell holding a world position."),
    method("WorldToCellPreferSolid", only(&["Terrain"]), "position: Vector3", "Vector3", "The cell holding a position, leaning to a solid neighbour."),
    method("WorldToCellPreferEmpty", only(&["Terrain"]), "position: Vector3", "Vector3", "The cell holding a position, leaning to an empty neighbour."),
    method("GetMaterialColor", only(&["Terrain"]), "material: Enum.Material", "Color3", "A terrain material's color."),
    method("SetMaterialColor", only(&["Terrain"]), "material: Enum.Material, value: Color3", "", "Sets a terrain material's color."),
    // Events and functions between scripts.
    method("Fire", only(&["BindableEvent"]), "...: any", "", "Fires Event on the same machine."),
    method("Invoke", only(&["BindableFunction"]), "...: any", "...any", "Calls OnInvoke and returns what it returns."),
    method("FireServer", REMOTE_EVENT, "...: any", "", "From a client: fires OnServerEvent on the server."),
    method("FireClient", REMOTE_EVENT, "player: Player, ...: any", "", "From the server: fires OnClientEvent on one client."),
    method("FireAllClients", REMOTE_EVENT, "...: any", "", "From the server: fires OnClientEvent on every client."),
    method("InvokeServer", only(&["RemoteFunction"]), "...: any", "...any", "From a client: calls OnServerInvoke and waits."),
    method("InvokeClient", only(&["RemoteFunction"]), "player: Player, ...: any", "...any", "From the server: calls a client's OnClientInvoke and waits."),
    method("SetArgumentTypes", only(&["RemoteEvent", "UnreliableRemoteEvent", "RemoteFunction"]), "types: {string}", "", "Declares the argument types each call must carry; the host refuses a call that does not match."),
    // Players.
    method("GetPlayers", only(&["Players"]), "", "{Player}", "Every player in the session."),
    method("GetPlayerFromCharacter", only(&["Players"]), "character: Model", "Player?", "The player whose character this is."),
    method("GetPlayerByUserId", only(&["Players"]), "userId: number", "Player?", "The player with this UserId."),
    method("GetMouse", only(&["Player"]), "", "PlayerMouse", "This player's mouse."),
    method("LoadCharacter", only(&["Player"]), "", "", "Spawns a new character for the player."),
    method("Kick", only(&["Player"]), "message: string?", "", "Removes the player from the session."),
    method("GetRankInGroup", only(&["Player"]), "groupId: number", "number", "The player's rank in a group."),
    method("IsInGroup", only(&["Player"]), "groupId: number", "boolean", "Whether the player is in a group."),
    method("DistanceFromCharacter", only(&["Player"]), "point: Vector3", "number", "Distance from the character's head to a point."),
    // Cameras and the workspace.
    method("ViewportPointToRay", only(&["Camera"]), "x: number, y: number, depth: number?", "Ray", "A unit ray through a viewport pixel."),
    method("ScreenPointToRay", only(&["Camera"]), "x: number, y: number, depth: number?", "Ray", "A unit ray through a screen pixel."),
    method("WorldToViewportPoint", only(&["Camera"]), "worldPoint: Vector3", "(Vector3, boolean)", "Where a world point lands in the viewport, and whether it is in view."),
    method("WorldToScreenPoint", only(&["Camera"]), "worldPoint: Vector3", "(Vector3, boolean)", "Where a world point lands on screen, and whether it is in view."),
    method("Raycast", only(&["Workspace"]), "origin: Vector3, direction: Vector3, params: RaycastParams?", "RaycastResult?", "The first hit along a ray, or nil."),
    method("GetServerTimeNow", only(&["Workspace"]), "", "number", "Seconds on the session's shared clock."),
    method("GetPartBoundsInRadius", only(&["Workspace"]), "position: Vector3, radius: number, params: OverlapParams?", "{BasePart}", "Parts whose bounds reach into a sphere."),
    method("GetPartBoundsInBox", only(&["Workspace"]), "cframe: CFrame, size: Vector3, params: OverlapParams?", "{BasePart}", "Parts whose bounds reach into a box."),
    old("FindPartOnRay", only(&["Workspace"]), "ray: Ray, ignore: Instance?", "(BasePart?, Vector3, Vector3)", "Old ray test; use Raycast."),
    old("FindPartOnRayWithIgnoreList", only(&["Workspace"]), "ray: Ray, ignore: {Instance}", "(BasePart?, Vector3, Vector3)", "Old ray test; use Raycast."),
    method("GetRealPhysicsFPS", only(&["Workspace"]), "", "number", "Physics steps per second."),
    method("GetService", only(&["DataModel"]), "className: string", "ArgClass", "The service of this class, made if it does not exist yet."),
    method("FindService", only(&["DataModel"]), "className: string", "ArgClass?", "The service of this class, or nil."),
    method("IsLoaded", only(&["DataModel"]), "", "boolean", "Whether the Space has finished loading."),
    method("BindToClose", only(&["DataModel"]), "callback: () -> ()", "", "Runs a function when the session ends."),
    // Input.
    method("IsKeyDown", only(&["UserInputService", "Player"]), "keyCode: Enum.KeyCode", "boolean", "Whether a key is held (on a Player: that player's keys)."),
    method("IsMouseButtonPressed", only(&["UserInputService", "Player"]), "mouseButton: Enum.UserInputType", "boolean", "Whether a mouse button is held."),
    method("GetMouseLocation", only(&["UserInputService"]), "", "Vector2", "The cursor in viewport pixels."),
    method("GetMouseDelta", only(&["UserInputService"]), "", "Vector2", "How far the mouse moved this frame."),
    method("GetKeysPressed", only(&["UserInputService"]), "", "{InputObject}", "The keys held now."),
    method("GetMouseButtonsPressed", only(&["UserInputService"]), "", "{InputObject}", "The mouse buttons held now."),
    method("GetLastInputType", only(&["UserInputService"]), "", "Enum.UserInputType", "The kind of input used last."),
    method("GetFocusedTextBox", only(&["UserInputService"]), "", "TextBox?", "The text box holding the keyboard, or nil."),
    method("IsGamepadButtonDown", only(&["UserInputService"]), "gamepad: Enum.UserInputType, button: Enum.KeyCode", "boolean", "Whether a gamepad button is held."),
    method("GetConnectedGamepads", only(&["UserInputService"]), "", "{Enum.UserInputType}", "The gamepads plugged in."),
    // Run state.
    method("IsClient", only(&["RunService"]), "", "boolean", "Whether this code runs on a client."),
    method("IsServer", only(&["RunService"]), "", "boolean", "Whether this code runs on the server."),
    method("IsStudio", only(&["RunService"]), "", "boolean", "Whether this runs inside Studio."),
    method("IsRunning", only(&["RunService"]), "", "boolean", "Whether the simulation is running."),
    method("IsRunMode", only(&["RunService"]), "", "boolean", "Whether Studio is in Run mode."),
    method("IsEdit", only(&["RunService"]), "", "boolean", "Whether Studio is editing rather than playing."),
    method("BindToRenderStep", only(&["RunService"]), "name: string, priority: number, fn: (deltaTime: number) -> ()", "", "Runs a function every frame before drawing, in priority order."),
    method("UnbindFromRenderStep", only(&["RunService"]), "name: string", "", "Stops a function bound with BindToRenderStep."),
    // Other services.
    method("GetTagged", only(&["CollectionService"]), "tag: string", "{Instance}", "Every instance with this tag."),
    method("GetInstanceAddedSignal", only(&["CollectionService"]), "tag: string", "RBXScriptSignal", "Fires when an instance gains this tag."),
    method("GetInstanceRemovedSignal", only(&["CollectionService"]), "tag: string", "RBXScriptSignal", "Fires when an instance loses this tag."),
    method("GetAllTags", only(&["CollectionService"]), "", "{string}", "Every tag in use."),
    method("JSONEncode", only(&["HttpService"]), "input: any", "string", "A value as JSON text."),
    method("JSONDecode", only(&["HttpService"]), "input: string", "any", "JSON text as a value."),
    method("GenerateGUID", only(&["HttpService"]), "wrapInCurlyBraces: boolean?", "string", "A new random GUID."),
    method("UrlEncode", only(&["HttpService"]), "input: string", "string", "Text escaped for a URL."),
    method("Mine", only(&["DataService"]), "request: {[string]: any}", "any", "Asks the Data Platform's front door a question."),
    method("Describe", only(&["DataService"]), "request: {[string]: any}", "any", "Describes a dataset."),
    method("Query", only(&["DataService"]), "request: {[string]: any}", "any", "Rows from a Connector."),
    method("Render", only(&["DataService"]), "reply: any", "string", "A reply as readable text."),
    method("PlayLocalSound", only(&["SoundService"]), "sound: Sound", "", "Plays a sound for this player only."),
    // Signals every matching instance has.
    event("Changed", ANY, "property: string", "Fires when a property changes."),
    event("ChildAdded", ANY, "child: Instance", "Fires when a child is added."),
    event("ChildRemoved", ANY, "child: Instance", "Fires when a child is removed."),
    event("DescendantAdded", ANY, "descendant: Instance", "Fires when anything is added below."),
    event("DescendantRemoving", ANY, "descendant: Instance", "Fires before anything below is removed."),
    event("AncestryChanged", ANY, "child: Instance, parent: Instance?", "Fires when the instance or an ancestor moves."),
    event("Destroying", ANY, "", "Fires just before the instance is destroyed."),
    event("AttributeChanged", ANY, "attribute: string", "Fires when an attribute changes."),
    event("Touched", PART, "otherPart: BasePart", "Fires when another part starts touching this one."),
    event("TouchEnded", PART, "otherPart: BasePart", "Fires when another part stops touching this one."),
    event("Died", HUMANOID, "", "Fires when Health reaches zero."),
    event("HealthChanged", HUMANOID, "health: number", "Fires when Health changes."),
    event("MoveToFinished", HUMANOID, "reached: boolean", "Fires when a MoveTo ends."),
    event("Running", HUMANOID, "speed: number", "Fires when the running speed changes."),
    event("Jumping", HUMANOID, "active: boolean", "Fires when the humanoid jumps."),
    event("StateChanged", HUMANOID, "old: Enum.HumanoidStateType, new: Enum.HumanoidStateType", "Fires when the state changes."),
    event("FreeFalling", HUMANOID, "active: boolean", "Fires when the humanoid starts or stops falling."),
    event("Climbing", HUMANOID, "speed: number", "Fires while the humanoid climbs."),
    event("Seated", HUMANOID, "active: boolean, currentSeatPart: BasePart?", "Fires when the humanoid sits or stands."),
    event("AnimationPlayed", ANIMATES, "animationTrack: AnimationTrack", "Fires when a track starts playing."),
    event("Stopped", TRACK, "", "Fires when the track stops."),
    event("DidLoop", TRACK, "", "Fires each time a looped track wraps."),
    event("KeyframeReached", TRACK, "keyframeName: string", "Fires at each named keyframe."),
    event("Ended", TRACK, "", "Fires when the track finishes."),
    event("Ended", only(&["Sound"]), "soundId: string", "Fires when the sound plays to its end."),
    event("PlayerAdded", only(&["Players"]), "player: Player", "Fires when a player joins."),
    event("PlayerRemoving", only(&["Players"]), "player: Player", "Fires when a player is leaving."),
    event("CharacterAdded", only(&["Player"]), "character: Model", "Fires when the player's character spawns."),
    event("CharacterRemoving", only(&["Player"]), "character: Model", "Fires before the character is removed."),
    event("CharacterAppearanceLoaded", only(&["Player"]), "character: Model", "Fires when the character's appearance has loaded."),
    event("Chatted", only(&["Player"]), "message: string", "Fires when the player chats."),
    event("Idled", only(&["Player"]), "time: number", "Fires when the player has been idle a while."),
    event("MouseButton1Click", GUI, "", "Fires on a left click."),
    event("MouseButton1Down", GUI, "x: number, y: number", "Fires when the left button goes down over it."),
    event("MouseButton1Up", GUI, "x: number, y: number", "Fires when the left button comes up over it."),
    event("MouseButton2Click", GUI, "", "Fires on a right click."),
    event("Activated", GUI, "inputObject: InputObject, clickCount: number", "Fires when the button is pressed."),
    event("MouseEnter", GUI, "x: number, y: number", "Fires when the cursor comes over it."),
    event("MouseLeave", GUI, "x: number, y: number", "Fires when the cursor leaves it."),
    event("Focused", only(&["TextBox"]), "", "Fires when the text box takes the keyboard."),
    event("FocusLost", only(&["TextBox"]), "enterPressed: boolean, inputThatCausedFocusLoss: InputObject?", "Fires when the text box gives the keyboard back."),
    event("Event", only(&["BindableEvent"]), "...: any", "Fires on Fire."),
    event("OnServerEvent", REMOTE_EVENT, "player: Player, ...: any", "On the server: fires when a client calls FireServer."),
    event("OnClientEvent", REMOTE_EVENT, "...: any", "On a client: fires when the server calls FireClient or FireAllClients."),
    event("InputBegan", INPUT, "input: InputObject, gameProcessedEvent: boolean", "Fires when a key, button or touch starts."),
    event("InputEnded", INPUT, "input: InputObject, gameProcessedEvent: boolean", "Fires when a key, button or touch ends."),
    event("InputChanged", INPUT, "input: InputObject, gameProcessedEvent: boolean", "Fires when the mouse moves or the wheel turns."),
    event("JumpRequest", INPUT, "", "Fires when the player asks to jump."),
    event("WindowFocused", INPUT, "", "Fires when the window gains focus."),
    event("WindowFocusReleased", INPUT, "", "Fires when the window loses focus."),
    event("MouseClick", only(&["ClickDetector"]), "playerWhoClicked: Player", "Fires when a player clicks the part."),
    event("RightMouseClick", only(&["ClickDetector"]), "playerWhoClicked: Player", "Fires when a player right-clicks the part."),
    event("MouseHoverEnter", only(&["ClickDetector"]), "playerWhoHovered: Player", "Fires when a player's cursor comes over the part."),
    event("MouseHoverLeave", only(&["ClickDetector"]), "playerWhoHovered: Player", "Fires when a player's cursor leaves the part."),
    event("Triggered", only(&["ProximityPrompt"]), "playerWhoTriggered: Player", "Fires when a player uses the prompt."),
    event("TriggerEnded", only(&["ProximityPrompt"]), "playerWhoTriggered: Player", "Fires when a player lets go of the prompt."),
    event("PromptShown", only(&["ProximityPrompt"]), "inputType: Enum.ProximityPromptInputType", "Fires when the prompt appears."),
    event("PromptHidden", only(&["ProximityPrompt"]), "", "Fires when the prompt disappears."),
    event("Played", only(&["Sound"]), "soundId: string", "Fires when the sound starts."),
    event("Loaded", only(&["Sound"]), "soundId: string", "Fires when the sound has loaded."),
    event("Paused", only(&["Sound"]), "soundId: string", "Fires when Pause stops the sound where it is."),
    event("Resumed", only(&["Sound"]), "soundId: string", "Fires when Resume carries on from where it paused."),
    event("Stopped", only(&["Sound"]), "soundId: string", "Fires when Stop stops the sound and rewinds it."),
    event("DidLoop", only(&["Sound"]), "soundId: string, numOfTimesLooped: number", "Fires each time a looped sound wraps."),
    event("Close", only(&["DataModel"]), "", "Fires when the session ends."),
    // Functions the engine calls.
    callback("OnInvoke", only(&["BindableFunction"]), "...: any", "...any", "Answers Invoke."),
    callback("OnServerInvoke", only(&["RemoteFunction"]), "player: Player, ...: any", "...any", "On the server: answers InvokeServer."),
    callback("OnClientInvoke", only(&["RemoteFunction"]), "...: any", "...any", "On a client: answers InvokeClient."),
    callback("ProcessReceipt", only(&["MarketplaceService"]), "receiptInfo: {[string]: any}", "Enum.ProductPurchaseDecision", "Grants a purchase and says whether it was granted."),
    // Service members the prelude implements.
    service("Create", only(&["TweenService"]), "instance: Instance, tweenInfo: TweenInfo, propertyTable: {[string]: any}", "Tween", "A tween that moves properties to their goals."),
    service("GetValue", only(&["TweenService"]), "alpha: number, easingStyle: Enum.EasingStyle, easingDirection: Enum.EasingDirection", "number", "An eased value for `alpha` between 0 and 1."),
    service("AddItem", only(&["Debris"]), "item: Instance, lifetime: number?", "", "Destroys `item` after `lifetime` seconds (10 by default)."),
    service("BindAction", only(&["ContextActionService"]), "actionName: string, functionToBind: (string, Enum.UserInputState, InputObject) -> any, createTouchButton: boolean, ...: Enum.KeyCode | Enum.UserInputType", "", "Calls a function for the given inputs."),
    service("BindActionAtPriority", only(&["ContextActionService"]), "actionName: string, functionToBind: (string, Enum.UserInputState, InputObject) -> any, createTouchButton: boolean, priorityLevel: number, ...: Enum.KeyCode | Enum.UserInputType", "", "BindAction with a priority."),
    service("UnbindAction", only(&["ContextActionService"]), "actionName: string", "", "Removes a bound action."),
    service("UnbindAllActions", only(&["ContextActionService"]), "", "", "Removes every bound action."),
    service("GetAllBoundActionInfo", only(&["ContextActionService"]), "", "{[string]: any}", "Every bound action."),
    service("SetTitle", only(&["ContextActionService"]), "actionName: string, title: string", "", "A touch button's title."),
    service("SetImage", only(&["ContextActionService"]), "actionName: string, image: string", "", "A touch button's image."),
    service("SetPosition", only(&["ContextActionService"]), "actionName: string, position: UDim2", "", "A touch button's position."),
    service("PromptProductPurchase", only(&["MarketplaceService"]), "player: Player, productId: number", "", "Asks a player to buy a developer product."),
    service("PromptGamePassPurchase", only(&["MarketplaceService"]), "player: Player, gamePassId: number", "", "Asks a player to buy a pass."),
    service("PromptPurchase", only(&["MarketplaceService"]), "player: Player, assetId: number", "", "Asks a player to buy an item."),
    service("GetProductInfo", only(&["MarketplaceService"]), "assetId: number, infoType: Enum.InfoType?", "{[string]: any}", "A product's name, price and kind."),
    service("UserOwnsGamePassAsync", only(&["MarketplaceService"]), "userId: number, gamePassId: number", "boolean", "Whether a user owns a pass."),
    service("PlayerOwnsAsset", only(&["MarketplaceService"]), "player: Player, assetId: number", "boolean", "Whether a player owns an item."),
    service_event("PromptProductPurchaseFinished", only(&["MarketplaceService"]), "userId: number, productId: number, isPurchased: boolean", "Fires when a product prompt closes."),
    service_event("PromptGamePassPurchaseFinished", only(&["MarketplaceService"]), "player: Player, gamePassId: number, wasPurchased: boolean", "Fires when a pass prompt closes."),
    service_event("PromptPurchaseFinished", only(&["MarketplaceService"]), "player: Player, assetId: number, isPurchased: boolean", "Fires when an item prompt closes."),
    service_event("Stepped", only(&["RunService"]), "time: number, deltaTime: number", "Fires every frame before physics."),
    service_event("Heartbeat", only(&["RunService"]), "deltaTime: number", "Fires every frame after physics."),
    service_event("RenderStepped", only(&["RunService"]), "deltaTime: number", "Fires every frame before drawing (client only)."),
    service_event("PreSimulation", only(&["RunService"]), "deltaTime: number", "Fires every frame before physics."),
    service_event("PostSimulation", only(&["RunService"]), "deltaTime: number", "Fires every frame after physics."),
    service_event("PreRender", only(&["RunService"]), "deltaTime: number", "Fires every frame before drawing."),
];

fn index() -> &'static HashMap<&'static str, Vec<&'static Member>> {
    static INDEX: OnceLock<HashMap<&'static str, Vec<&'static Member>>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map: HashMap<&'static str, Vec<&'static Member>> = HashMap::new();
        for m in MEMBERS {
            map.entry(m.name).or_default().push(m);
        }
        map
    })
}

fn any_of(class: &str, key: &str, kind: Kind) -> bool {
    index().get(key).is_some_and(|ms| ms.iter().any(|m| m.kind == kind && m.on.contains(class)))
}

/// Whether `key` names a method on `class`, so a child called "Play" is not
/// shadowed on a Part.
pub fn method_applies(class: &str, key: &str) -> bool {
    any_of(class, key, Kind::Method)
}

/// Whether `key` names a signal on `class`.
pub fn is_event(class: &str, key: &str) -> bool {
    any_of(class, key, Kind::Event)
}

/// Every member a `class` instance offers, deprecated ones left out.
pub fn members_of(class: &str) -> impl Iterator<Item = &'static Member> + '_ {
    MEMBERS.iter().filter(move |m| !m.deprecated && m.on.contains(class))
}

/// The members named `name` on `class` (a name such as `Play` can mean
/// different things on different classes).
pub fn member(class: &str, name: &str) -> Option<&'static Member> {
    index().get(name)?.iter().copied().find(|m| m.on.contains(class))
}

// ── Properties ───────────────────────────────────────────────────────────────

/// A property a script reads with `inst.Name`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Property {
    pub name: &'static str,
    /// The value's type as Luau writes it: `number`, `Vector3`,
    /// `Enum.Material`, `BasePart?`.
    pub ty: String,
}

/// What a property that starts empty holds once set.
fn nil_property_type(class: &str, name: &str) -> &'static str {
    match name {
        "PrimaryPart" => "BasePart?",
        "Character" => "Model?",
        "Team" => "Team?",
        "Occupant" => "Humanoid?",
        "Adornee" | "CameraSubject" => "Instance?",
        "Attachment0" | "Attachment1" => "Attachment?",
        "Part0" | "Part1" => "BasePart?",
        "Value" if class == "ObjectValue" => "Instance?",
        _ => "any",
    }
}

fn type_of(class: &str, name: &str, v: &DmValue) -> String {
    match v {
        DmValue::Nil => nil_property_type(class, name).into(),
        DmValue::Bool(_) => "boolean".into(),
        DmValue::Number(_) => "number".into(),
        DmValue::String(_) => "string".into(),
        DmValue::Vector2(_) => "Vector2".into(),
        DmValue::Vector3(_) => "Vector3".into(),
        DmValue::CFrame(_) => "CFrame".into(),
        DmValue::Color3(_) => "Color3".into(),
        DmValue::UDim(_) => "UDim".into(),
        DmValue::UDim2(_) => "UDim2".into(),
        DmValue::NumberRange(_) => "NumberRange".into(),
        DmValue::Enum(e) => format!("Enum.{}", e.enum_type),
        DmValue::Instance(_) => "Instance".into(),
        DmValue::NumberSequence(_) => "NumberSequence".into(),
        DmValue::ColorSequence(_) => "ColorSequence".into(),
    }
}

/// Properties the running session sets beyond the class defaults: on
/// services, and a Sound's playback state (read-only, from its player).
const SESSION_PROPERTIES: &[(&str, &str, &str)] = &[
    ("Workspace", "CurrentCamera", "Camera"),
    ("Workspace", "Gravity", "number"),
    ("Players", "LocalPlayer", "Player"),
    ("Players", "RespawnTime", "number"),
    ("Players", "CharacterAutoLoads", "boolean"),
    ("Sound", "IsPlaying", "boolean"),
    ("Sound", "IsPaused", "boolean"),
    ("Sound", "IsLoaded", "boolean"),
    ("Sound", "TimeLength", "number"),
];

/// Children the session puts under an instance, by name.
pub const SESSION_CHILDREN: &[(&str, &str, &str)] = &[
    ("Player", "PlayerGui", "PlayerGui"),
    ("Player", "Backpack", "Backpack"),
    ("Player", "PlayerScripts", "PlayerScripts"),
];

/// Every property a `class` instance answers: the ones every instance has,
/// the ones the tree computes from a pose, the class defaults, and what the
/// session sets on services.
pub fn properties_of(class: &str) -> Vec<Property> {
    let mut out: Vec<Property> = Vec::new();
    let mut add = |name: &'static str, ty: String| {
        if !out.iter().any(|p| p.name == name) {
            out.push(Property { name, ty });
        }
    };
    for (name, ty) in [("Name", "string"), ("ClassName", "string"), ("Parent", "Instance?"), ("Archivable", "boolean")] {
        add(name, ty.into());
    }
    let part = is_base_part(class);
    if part || matches!(class, "Camera" | "Attachment") {
        add("Position", "Vector3".into());
        add("Orientation", "Vector3".into());
        add("WorldPosition", "Vector3".into());
    }
    if part {
        add("Rotation", "Vector3".into());
        add("Velocity", "Vector3".into());
        add("RotVelocity", "Vector3".into());
        add("Mass", "number".into());
        add("AssemblyMass", "number".into());
    }
    if class == "Attachment" {
        for name in ["Axis", "SecondaryAxis", "WorldAxis", "WorldSecondaryAxis", "WorldOrientation"] {
            add(name, "Vector3".into());
        }
        add("WorldCFrame", "CFrame".into());
    }
    for (name, v) in default_properties(class) {
        add(name, type_of(class, name, &v));
    }
    for (c, name, ty) in SESSION_PROPERTIES {
        if *c == class {
            add(name, (*ty).into());
        }
    }
    out
}

// ── Services and classes ─────────────────────────────────────────────────────

/// Every name `game:GetService` answers, in the order `game` lists them.
pub fn services() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = SERVICE_CLASSES.to_vec();
    for extra in ["StarterPlayerScripts", "StarterCharacterScripts", "DataService", "KeyframeSequenceProvider"] {
        if is_service_class(extra) && !out.contains(&extra) {
            out.push(extra);
        }
    }
    out
}

/// Classes `Instance.new` makes that a script commonly creates.
pub const CREATABLE: &[&str] = &[
    "Part", "WedgePart", "CornerWedgePart", "TrussPart", "MeshPart", "SpawnLocation", "Seat", "VehicleSeat", "Model",
    "Folder", "Configuration", "Tool", "Accessory", "Camera", "Humanoid", "Animator", "Animation",
    "AnimationController", "KeyframeSequence", "Keyframe", "Pose", "NumberPose", "KeyframeMarker", "Script",
    "LocalScript", "ModuleScript", "ScreenGui", "BillboardGui", "SurfaceGui", "Frame", "ScrollingFrame", "TextLabel",
    "ImageLabel", "TextButton", "ImageButton", "TextBox", "ViewportFrame", "UIListLayout", "UIGridLayout", "UIPadding",
    "UICorner", "UIStroke", "UIScale", "UIGradient", "UIAspectRatioConstraint", "UISizeConstraint",
    "UITextSizeConstraint", "PointLight", "SpotLight", "SurfaceLight", "Sound", "SoundGroup", "ParticleEmitter", "Beam",
    "Trail", "Fire", "Smoke", "Sparkles", "Explosion", "Attachment", "Decal", "Texture", "SpecialMesh", "WeldConstraint",
    "Weld", "Motor6D", "BindableEvent", "BindableFunction", "RemoteEvent", "UnreliableRemoteEvent", "RemoteFunction",
    "StringValue",
    "NumberValue", "IntValue", "BoolValue", "ObjectValue", "Vector3Value", "CFrameValue", "Color3Value", "Team",
    "ClickDetector", "ProximityPrompt", "HingeConstraint", "PrismaticConstraint", "CylindricalConstraint",
    "SpringConstraint", "BallSocketConstraint", "RopeConstraint", "RodConstraint", "VectorForce",
    "NoCollisionConstraint",
];

// ── Objects that are not instances ───────────────────────────────────────────

/// A field or method on a value that is not an instance.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub name: &'static str,
    pub kind: Kind,
    pub params: &'static str,
    /// A field's type, a method's return, or what an event hands handlers.
    pub ty: &'static str,
    pub doc: &'static str,
}

const fn field(name: &'static str, ty: &'static str, doc: &'static str) -> Field {
    Field { name, kind: Kind::Callback, params: "", ty, doc }
}

const fn func(name: &'static str, params: &'static str, ty: &'static str, doc: &'static str) -> Field {
    Field { name, kind: Kind::Method, params, ty, doc }
}

const fn signal(name: &'static str, params: &'static str, doc: &'static str) -> Field {
    Field { name, kind: Kind::Event, params, ty: "", doc }
}

/// Values a script meets that are not instances: what `typeof` calls them,
/// and their fields (`kind` Callback), methods and signals.
pub static OBJECTS: &[(&str, &[Field])] = &[
    ("RBXScriptSignal", &[
        func("Connect", "fn: (...any) -> ()", "RBXScriptConnection", "Calls `fn` each time the signal fires."),
        func("Once", "fn: (...any) -> ()", "RBXScriptConnection", "Calls `fn` the next time only."),
        func("Wait", "", "...any", "Yields until the signal fires, and returns what it fired with."),
    ]),
    ("RBXScriptConnection", &[
        func("Disconnect", "", "", "Stops the handler."),
        field("Connected", "boolean", "Whether the handler is still connected."),
    ]),
    ("PlayerMouse", &[
        field("Hit", "CFrame", "Where the cursor's ray meets the world."),
        field("Origin", "CFrame", "The camera, facing along the cursor's ray."),
        field("Target", "BasePart?", "The part under the cursor."),
        field("TargetFilter", "Instance?", "An instance (and its descendants) the cursor looks through."),
        field("TargetSurface", "Enum.NormalId", "The face of Target under the cursor."),
        field("UnitRay", "Ray", "The cursor's ray, one unit long."),
        field("X", "number", "The cursor's x in viewport pixels."),
        field("Y", "number", "The cursor's y in viewport pixels."),
        field("ViewSizeX", "number", "The viewport's width."),
        field("ViewSizeY", "number", "The viewport's height."),
        field("Icon", "string", "The cursor image."),
        signal("Button1Down", "", "Fires when the left button goes down."),
        signal("Button1Up", "", "Fires when the left button comes up."),
        signal("Button2Down", "", "Fires when the right button goes down."),
        signal("Button2Up", "", "Fires when the right button comes up."),
        signal("Move", "", "Fires when the cursor moves."),
        signal("WheelForward", "", "Fires when the wheel turns forward."),
        signal("WheelBackward", "", "Fires when the wheel turns back."),
        signal("Idle", "", "Fires every frame the mouse is still."),
    ]),
    ("InputObject", &[
        field("KeyCode", "Enum.KeyCode", "The key, for keyboard input."),
        field("UserInputType", "Enum.UserInputType", "What kind of input this is."),
        field("UserInputState", "Enum.UserInputState", "Begin, Change or End."),
        field("Position", "Vector3", "The cursor (x, y) and the wheel (z)."),
        field("Delta", "Vector3", "The movement since the last event."),
    ]),
    ("RaycastResult", &[
        field("Instance", "BasePart", "The part the ray hit."),
        field("Position", "Vector3", "Where it hit."),
        field("Normal", "Vector3", "The surface normal there."),
        field("Distance", "number", "How far along the ray."),
        field("Material", "Enum.Material", "The material there."),
    ]),
    ("Tween", &[
        func("Play", "", "", "Starts or resumes the tween."),
        func("Pause", "", "", "Pauses the tween."),
        func("Cancel", "", "", "Stops the tween where it is."),
        func("Destroy", "", "", "Stops the tween and lets it go."),
        field("PlaybackState", "Enum.PlaybackState", "Playing, Paused, Completed or Cancelled."),
        signal("Completed", "playbackState: Enum.PlaybackState", "Fires when the tween ends."),
    ]),
    ("Vector3", &[
        field("X", "number", "The x component."),
        field("Y", "number", "The y component."),
        field("Z", "number", "The z component."),
        field("Magnitude", "number", "The length."),
        field("Unit", "Vector3", "The same direction, length 1."),
        func("Dot", "other: Vector3", "number", "The dot product."),
        func("Cross", "other: Vector3", "Vector3", "The cross product."),
        func("Lerp", "goal: Vector3, alpha: number", "Vector3", "The point `alpha` of the way to `goal`."),
        func("FuzzyEq", "other: Vector3, epsilon: number?", "boolean", "Whether the two are within `epsilon`."),
    ]),
    ("Vector2", &[
        field("X", "number", "The x component."),
        field("Y", "number", "The y component."),
        field("Magnitude", "number", "The length."),
        field("Unit", "Vector2", "The same direction, length 1."),
        func("Dot", "other: Vector2", "number", "The dot product."),
        func("Cross", "other: Vector2", "number", "The 2D cross product."),
        func("Lerp", "goal: Vector2, alpha: number", "Vector2", "The point `alpha` of the way to `goal`."),
        func("Max", "other: Vector2", "Vector2", "The larger of each component."),
        func("Min", "other: Vector2", "Vector2", "The smaller of each component."),
        func("Abs", "", "Vector2", "Each component made positive."),
        func("Floor", "", "Vector2", "Each component rounded down."),
        func("Ceil", "", "Vector2", "Each component rounded up."),
    ]),
    ("CFrame", &[
        field("Position", "Vector3", "The position."),
        field("Rotation", "CFrame", "The rotation alone."),
        field("X", "number", "The position's x."),
        field("Y", "number", "The position's y."),
        field("Z", "number", "The position's z."),
        field("LookVector", "Vector3", "The forward direction."),
        field("RightVector", "Vector3", "The right direction."),
        field("UpVector", "Vector3", "The up direction."),
        field("XVector", "Vector3", "The first column."),
        field("YVector", "Vector3", "The second column."),
        field("ZVector", "Vector3", "The third column."),
        func("Inverse", "", "CFrame", "The inverse transform."),
        func("Lerp", "goal: CFrame, alpha: number", "CFrame", "The frame `alpha` of the way to `goal`."),
        func("ToWorldSpace", "cf: CFrame", "CFrame", "`cf` from this frame's space into the world."),
        func("ToObjectSpace", "cf: CFrame", "CFrame", "`cf` from the world into this frame's space."),
        func("PointToWorldSpace", "v: Vector3", "Vector3", "A local point in world space."),
        func("PointToObjectSpace", "v: Vector3", "Vector3", "A world point in local space."),
        func("VectorToWorldSpace", "v: Vector3", "Vector3", "A local direction in world space."),
        func("VectorToObjectSpace", "v: Vector3", "Vector3", "A world direction in local space."),
        func("GetComponents", "", "...number", "Position then the rotation matrix, row by row."),
        func("ToEulerAnglesXYZ", "", "(number, number, number)", "The rotation as X, Y, Z angles in radians."),
        func("ToEulerAnglesYXZ", "", "(number, number, number)", "The rotation as Y, X, Z angles in radians."),
        func("ToOrientation", "", "(number, number, number)", "The rotation as Orientation angles in radians."),
        func("ToAxisAngle", "", "(Vector3, number)", "The rotation as an axis and an angle."),
    ]),
    ("Color3", &[
        field("R", "number", "Red, 0 to 1."),
        field("G", "number", "Green, 0 to 1."),
        field("B", "number", "Blue, 0 to 1."),
        func("Lerp", "goal: Color3, alpha: number", "Color3", "The color `alpha` of the way to `goal`."),
        func("ToHSV", "", "(number, number, number)", "Hue, saturation and value, 0 to 1."),
        func("ToHex", "", "string", "The color as `RRGGBB`."),
    ]),
    ("UDim2", &[
        field("X", "UDim", "The horizontal scale and offset."),
        field("Y", "UDim", "The vertical scale and offset."),
        field("Width", "UDim", "Same as X."),
        field("Height", "UDim", "Same as Y."),
        func("Lerp", "goal: UDim2, alpha: number", "UDim2", "The value `alpha` of the way to `goal`."),
    ]),
    ("Ray", &[
        field("Origin", "Vector3", "Where the ray starts."),
        field("Direction", "Vector3", "Where it points, and how far."),
        field("Unit", "Ray", "The same ray, one unit long."),
        func("ClosestPoint", "point: Vector3", "Vector3", "The point on the ray nearest `point`."),
        func("Distance", "point: Vector3", "number", "How far `point` is from the ray."),
    ]),
];

/// The fields, methods and signals of a non-instance type.
pub fn object_fields(ty: &str) -> &'static [Field] {
    OBJECTS.iter().find(|(name, _)| *name == ty).map_or(&[], |(_, fields)| *fields)
}

// ── Libraries and globals ────────────────────────────────────────────────────

/// Global tables and what they hold. Functions carry parameters; constants
/// (`math.pi`, `Vector3.zero`) have none and `kind` Callback.
pub static LIBRARIES: &[(&str, &[Field])] = &[
    ("task", &[
        func("wait", "duration: number?", "number", "Yields for `duration` seconds (a frame when omitted); returns the time waited."),
        func("spawn", "fn: (...any) -> ...any, ...: any", "thread", "Runs `fn` now, as its own thread."),
        func("defer", "fn: (...any) -> ...any, ...: any", "thread", "Runs `fn` later this frame."),
        func("delay", "duration: number, fn: (...any) -> ...any, ...: any", "thread", "Runs `fn` after `duration` seconds."),
        func("cancel", "thread: thread", "", "Stops a thread from resuming."),
    ]),
    ("math", &[
        func("abs", "x: number", "number", "The absolute value."),
        func("floor", "x: number", "number", "Rounded down."),
        func("ceil", "x: number", "number", "Rounded up."),
        func("round", "x: number", "number", "Rounded to the nearest whole number."),
        func("max", "x: number, ...: number", "number", "The largest argument."),
        func("min", "x: number, ...: number", "number", "The smallest argument."),
        func("clamp", "x: number, min: number, max: number", "number", "`x` kept between `min` and `max`."),
        func("sqrt", "x: number", "number", "The square root."),
        func("sign", "x: number", "number", "-1, 0 or 1."),
        func("random", "m: number?, n: number?", "number", "A random number: in [0, 1), or a whole number in [m, n]."),
        func("randomseed", "seed: number", "", "Seeds math.random."),
        func("sin", "x: number", "number", "Sine, radians."),
        func("cos", "x: number", "number", "Cosine, radians."),
        func("tan", "x: number", "number", "Tangent, radians."),
        func("asin", "x: number", "number", "Arc sine."),
        func("acos", "x: number", "number", "Arc cosine."),
        func("atan", "x: number", "number", "Arc tangent."),
        func("atan2", "y: number, x: number", "number", "The angle of (x, y)."),
        func("rad", "degrees: number", "number", "Degrees to radians."),
        func("deg", "radians: number", "number", "Radians to degrees."),
        func("exp", "x: number", "number", "e to the power x."),
        func("log", "x: number, base: number?", "number", "The logarithm (natural by default)."),
        func("pow", "x: number, y: number", "number", "x to the power y."),
        func("fmod", "x: number, y: number", "number", "The remainder of x / y."),
        func("modf", "x: number", "(number, number)", "The whole and fractional parts."),
        func("noise", "x: number, y: number?, z: number?", "number", "Perlin noise."),
        func("lerp", "a: number, b: number, t: number", "number", "The number `t` of the way from `a` to `b`."),
        field("pi", "number", "The ratio of a circle's circumference to its diameter."),
        field("huge", "number", "Positive infinity."),
    ]),
    ("string", &[
        func("format", "format: string, ...: any", "string", "Fills `%d`, `%s`, `%.2f` and the like."),
        func("sub", "s: string, i: number, j: number?", "string", "The characters from i to j."),
        func("len", "s: string", "number", "The length in bytes."),
        func("lower", "s: string", "string", "Lowercase."),
        func("upper", "s: string", "string", "Uppercase."),
        func("rep", "s: string, n: number, sep: string?", "string", "`s` repeated n times."),
        func("reverse", "s: string", "string", "The string backwards."),
        func("split", "s: string, separator: string?", "{string}", "The pieces between each separator (comma by default)."),
        func("find", "s: string, pattern: string, init: number?, plain: boolean?", "(number?, number?)", "Where a pattern first matches."),
        func("match", "s: string, pattern: string, init: number?", "...string", "The first match's captures."),
        func("gmatch", "s: string, pattern: string", "() -> ...string", "An iterator over every match."),
        func("gsub", "s: string, pattern: string, repl: string | {[string]: string} | (...string) -> string, n: number?", "(string, number)", "Every match replaced, and how many."),
        func("byte", "s: string, i: number?, j: number?", "...number", "The byte values."),
        func("char", "...: number", "string", "A string from byte values."),
    ]),
    ("table", &[
        func("insert", "t: {any}, pos: number | any, value: any?", "", "Adds a value at the end, or at `pos`."),
        func("remove", "t: {any}, pos: number?", "any", "Removes and returns the value at `pos` (the last by default)."),
        func("find", "t: {any}, value: any, init: number?", "number?", "The first index holding `value`."),
        func("sort", "t: {any}, comp: ((a: any, b: any) -> boolean)?", "", "Sorts in place."),
        func("concat", "t: {any}, sep: string?, i: number?, j: number?", "string", "The values joined into a string."),
        func("clear", "t: {any}", "", "Empties the table in place."),
        func("clone", "t: {any}", "{any}", "A shallow copy."),
        func("freeze", "t: {any}", "{any}", "Makes the table read-only."),
        func("isfrozen", "t: {any}", "boolean", "Whether the table is read-only."),
        func("create", "n: number, value: any?", "{any}", "A list of `n` copies of `value`."),
        func("unpack", "t: {any}, i: number?, j: number?", "...any", "The values as separate results."),
        func("pack", "...: any", "{any}", "The arguments in a table, with `n`."),
    ]),
    ("coroutine", &[
        func("create", "fn: (...any) -> ...any", "thread", "A new coroutine."),
        func("resume", "co: thread, ...: any", "(boolean, ...any)", "Runs a coroutine until it yields."),
        func("yield", "...: any", "...any", "Pauses the running coroutine."),
        func("wrap", "fn: (...any) -> ...any", "(...any) -> ...any", "A function that resumes a new coroutine."),
        func("status", "co: thread", "string", "running, suspended, normal or dead."),
        func("running", "", "thread", "The running coroutine."),
        func("isyieldable", "", "boolean", "Whether the running code may yield."),
        func("close", "co: thread", "(boolean, any?)", "Stops a suspended coroutine."),
    ]),
    ("os", &[
        func("time", "t: {[string]: any}?", "number", "Seconds since 1970."),
        func("clock", "", "number", "CPU seconds, for timing code."),
        func("date", "format: string?, time: number?", "string | {[string]: any}", "A formatted date, or its parts."),
        func("difftime", "t2: number, t1: number", "number", "t2 - t1 in seconds."),
    ]),
    ("Vector3", &[
        func("new", "x: number?, y: number?, z: number?", "Vector3", "A vector from its components."),
        func("FromNormalId", "normal: Enum.NormalId", "Vector3", "The unit vector out of a face."),
        field("zero", "Vector3", "(0, 0, 0)."),
        field("one", "Vector3", "(1, 1, 1)."),
        field("xAxis", "Vector3", "(1, 0, 0)."),
        field("yAxis", "Vector3", "(0, 1, 0)."),
        field("zAxis", "Vector3", "(0, 0, 1)."),
    ]),
    ("Vector2", &[
        func("new", "x: number?, y: number?", "Vector2", "A 2D vector."),
        field("zero", "Vector2", "(0, 0)."),
        field("one", "Vector2", "(1, 1)."),
    ]),
    ("CFrame", &[
        func("new", "x: number?, y: number?, z: number?", "CFrame", "A frame at a position, or from a position and a point to look at, or 12 components."),
        func("Angles", "rx: number, ry: number, rz: number", "CFrame", "A rotation from X, Y, Z angles in radians."),
        func("fromEulerAnglesXYZ", "rx: number, ry: number, rz: number", "CFrame", "Same as Angles."),
        func("fromEulerAnglesYXZ", "rx: number, ry: number, rz: number", "CFrame", "A rotation applied Y, then X, then Z."),
        func("fromOrientation", "rx: number, ry: number, rz: number", "CFrame", "A rotation from Orientation angles in radians."),
        func("fromAxisAngle", "axis: Vector3, angle: number", "CFrame", "A rotation about an axis."),
        func("lookAt", "at: Vector3, lookAt: Vector3, up: Vector3?", "CFrame", "A frame at `at` facing `lookAt`."),
        func("lookAlong", "at: Vector3, direction: Vector3, up: Vector3?", "CFrame", "A frame at `at` facing along `direction`."),
        func("fromMatrix", "pos: Vector3, vX: Vector3, vY: Vector3, vZ: Vector3?", "CFrame", "A frame from a position and axes."),
        field("identity", "CFrame", "No move, no turn."),
    ]),
    ("Color3", &[
        func("new", "r: number?, g: number?, b: number?", "Color3", "A color from components 0 to 1."),
        func("fromRGB", "r: number?, g: number?, b: number?", "Color3", "A color from components 0 to 255."),
        func("fromHSV", "h: number, s: number, v: number", "Color3", "A color from hue, saturation and value, 0 to 1."),
        func("fromHex", "hex: string", "Color3", "A color from `#RRGGBB`."),
    ]),
    ("UDim2", &[
        func("new", "xScale: number?, xOffset: number?, yScale: number?, yOffset: number?", "UDim2", "A size or position: scale and pixel offset per axis."),
        func("fromScale", "xScale: number, yScale: number", "UDim2", "Scale only."),
        func("fromOffset", "xOffset: number, yOffset: number", "UDim2", "Pixels only."),
    ]),
    ("UDim", &[func("new", "scale: number?, offset: number?", "UDim", "A scale and a pixel offset.")]),
    ("Instance", &[func("new", "className: string, parent: Instance?", "ArgClass", "A new instance of the class.")]),
    ("TweenInfo", &[func("new", "time: number?, easingStyle: Enum.EasingStyle?, easingDirection: Enum.EasingDirection?, repeatCount: number?, reverses: boolean?, delayTime: number?", "TweenInfo", "How a tween moves.")]),
    ("RaycastParams", &[func("new", "", "RaycastParams", "Filters for workspace:Raycast.")]),
    ("Ray", &[func("new", "origin: Vector3, direction: Vector3", "Ray", "A ray from a point along a direction.")]),
    ("Random", &[func("new", "seed: number?", "Random", "A seeded random number generator.")]),
    ("NumberRange", &[func("new", "min: number, max: number?", "NumberRange", "A range of numbers.")]),
    ("NumberSequence", &[func("new", "n: number | {NumberSequenceKeypoint}", "NumberSequence", "Numbers over a lifetime.")]),
    ("ColorSequence", &[func("new", "c: Color3 | {ColorSequenceKeypoint}", "ColorSequence", "Colors over a lifetime.")]),
    ("BrickColor", &[func("new", "name: string | number", "BrickColor", "A named palette color.")]),
    ("Region3", &[func("new", "min: Vector3, max: Vector3", "Region3", "An axis-aligned box.")]),
];

/// Global functions and values.
pub static GLOBALS: &[Field] = &[
    field("game", "DataModel", "The DataModel: the root of every service."),
    field("workspace", "Workspace", "The Workspace service."),
    field("script", "LuaSourceContainer", "The script this code belongs to."),
    field("Enum", "Enums", "Every enum type."),
    field("shared", "{[string]: any}", "A table every script shares."),
    field("_G", "{[string]: any}", "A table every script shares."),
    func("print", "...: any", "", "Writes to Output."),
    func("warn", "...: any", "", "Writes a warning to Output."),
    func("error", "message: any, level: number?", "never", "Stops with an error."),
    func("assert", "value: any, message: string?", "any", "Stops with an error when `value` is false or nil."),
    func("pcall", "fn: (...any) -> ...any, ...: any", "(boolean, ...any)", "Calls `fn`; an error comes back as false and the message."),
    func("xpcall", "fn: (...any) -> ...any, handler: (err: any) -> any, ...: any", "(boolean, ...any)", "pcall with an error handler."),
    func("type", "value: any", "string", "The Luau type name."),
    func("typeof", "value: any", "string", "The type name, including engine types such as Vector3 and Instance."),
    func("tostring", "value: any", "string", "The value as text."),
    func("tonumber", "value: any, base: number?", "number?", "The value as a number, or nil."),
    func("pairs", "t: {[any]: any}", "iterator", "Every key and value."),
    func("ipairs", "t: {any}", "iterator", "Each index and value from 1 until nil."),
    func("next", "t: {[any]: any}, key: any?", "(any, any)", "The key and value after `key`."),
    func("select", "index: number | string, ...: any", "...any", "The arguments from `index` on, or their count with \"#\"."),
    func("unpack", "t: {any}, i: number?, j: number?", "...any", "The values as separate results."),
    func("require", "module: ModuleScript", "any", "What a ModuleScript returns."),
    func("setmetatable", "t: {[any]: any}, mt: {[any]: any}?", "{[any]: any}", "Sets a table's metatable."),
    func("getmetatable", "t: any", "{[any]: any}?", "A table's metatable."),
    func("rawget", "t: {[any]: any}, key: any", "any", "A field, ignoring metamethods."),
    func("rawset", "t: {[any]: any}, key: any, value: any", "{[any]: any}", "Sets a field, ignoring metamethods."),
    func("rawequal", "a: any, b: any", "boolean", "Equality, ignoring metamethods."),
    func("rawlen", "t: {any} | string", "number", "The length, ignoring metamethods."),
    func("tick", "", "number", "Seconds since 1970, local time."),
    func("time", "", "number", "Seconds since the session started."),
    func("elapsedTime", "", "number", "Seconds since the session started."),
    func("wait", "seconds: number?", "(number, number)", "Old form of task.wait."),
    func("delay", "seconds: number, fn: () -> ()", "", "Old form of task.delay."),
    func("spawn", "fn: () -> ()", "", "Old form of task.defer."),
];

/// The functions and constants of a global table (`math`, `Vector3`, ...).
pub fn library(name: &str) -> &'static [Field] {
    LIBRARIES.iter().find(|(n, _)| *n == name).map_or(&[], |(_, fields)| *fields)
}

// ── Enums ────────────────────────────────────────────────────────────────────

/// The enums a script uses, with their items. `Enum.<Type>.<Item>` resolves
/// for any name; these are the ones the engine reads.
pub static ENUMS: &[(&str, &[&str])] = &[
    ("KeyCode", &[
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V",
        "W", "X", "Y", "Z", "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Space",
        "Return", "Escape", "Tab", "Backspace", "Delete", "Up", "Down", "Left", "Right", "LeftShift", "RightShift",
        "LeftControl", "RightControl", "LeftAlt", "RightAlt", "LeftSuper", "RightSuper", "F1", "F2", "F3", "F4", "F5",
        "F6", "F7", "F8", "F9", "F10", "F11", "F12", "Minus", "Equals", "LeftBracket", "RightBracket", "BackSlash",
        "Slash", "Semicolon", "Quote", "Comma", "Period", "Backquote", "CapsLock", "Unknown",
    ]),
    ("UserInputType", &["MouseButton1", "MouseButton2", "MouseButton3", "MouseMovement", "MouseWheel", "Keyboard", "Touch", "Gamepad1", "None"]),
    ("UserInputState", &["Begin", "Change", "End", "Cancel", "None"]),
    ("HumanoidStateType", &[
        "FallingDown", "Ragdoll", "GettingUp", "Jumping", "Swimming", "Freefall", "Flying", "Landed", "Running",
        "RunningNoPhysics", "StrafingNoPhysics", "Climbing", "Seated", "PlatformStanding", "Dead", "Physics", "None",
    ]),
    ("EasingStyle", &["Linear", "Sine", "Back", "Quad", "Quart", "Quint", "Bounce", "Elastic", "Exponential", "Circular", "Cubic"]),
    ("EasingDirection", &["In", "Out", "InOut"]),
    ("PlaybackState", &["Begin", "Delayed", "Playing", "Paused", "Completed", "Cancelled"]),
    ("RaycastFilterType", &["Exclude", "Include"]),
    ("CameraType", &["Fixed", "Attach", "Watch", "Track", "Follow", "Custom", "Scriptable", "Orbital"]),
    ("CameraProjection", &["Perspective", "Orthographic"]),
    ("Material", &[
        "Plastic", "SmoothPlastic", "Neon", "Wood", "WoodPlanks", "Marble", "Slate", "Concrete", "Granite", "Brick",
        "Pebble", "Cobblestone", "Metal", "DiamondPlate", "CorrodedMetal", "Foil", "Grass", "Ice", "Glass", "Fabric",
        "Sand", "ForceField", "Ground", "Snow", "Mud", "Rock", "Basalt", "Asphalt", "Salt", "Limestone", "Pavement",
        "LeafyGrass", "Sandstone", "CrackedLava", "Glacier", "Air", "Water",
    ]),
    ("PartType", &["Block", "Ball", "Cylinder", "Wedge", "CornerWedge"]),
    ("NormalId", &["Front", "Back", "Left", "Right", "Top", "Bottom"]),
    ("Font", &["SourceSans", "SourceSansBold", "SourceSansItalic", "SourceSansLight", "Gotham", "GothamBold", "GothamMedium", "Arial", "ArialBold", "Code", "Legacy", "Fantasy", "Cartoon", "SciFi", "Highway", "Arcade", "Roboto", "RobotoMono"]),
    ("TextXAlignment", &["Left", "Center", "Right"]),
    ("TextYAlignment", &["Top", "Center", "Bottom"]),
    ("FillDirection", &["Horizontal", "Vertical"]),
    ("HorizontalAlignment", &["Left", "Center", "Right"]),
    ("VerticalAlignment", &["Top", "Center", "Bottom"]),
    ("SortOrder", &["LayoutOrder", "Name"]),
    ("SizeConstraint", &["RelativeXY", "RelativeXX", "RelativeYY"]),
    ("ScaleType", &["Stretch", "Slice", "Tile", "Fit", "Crop"]),
    ("ActuatorType", &["None", "Motor", "Servo"]),
    ("ActuatorRelativeTo", &["Attachment0", "Attachment1", "World"]),
    ("AnimationPriority", &["Idle", "Movement", "Action", "Action2", "Action3", "Action4", "Core"]),
    ("PoseEasingStyle", &["Linear", "Constant", "Elastic", "Cubic", "Bounce", "CubicV2"]),
    ("PoseEasingDirection", &["In", "Out", "InOut"]),
    ("ProductPurchaseDecision", &["NotProcessedYet", "PurchaseGranted"]),
    ("InfoType", &["Asset", "Product", "GamePass", "Subscription", "Bundle"]),
    ("ProximityPromptInputType", &["Keyboard", "Gamepad", "Touch"]),
];

/// The items of an enum type, or none for a type the engine does not read.
pub fn enum_items(ty: &str) -> &'static [&'static str] {
    ENUMS.iter().find(|(name, _)| *name == ty).map_or(&[], |(_, items)| *items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datamodel::{class_ancestry, new_shared, InstanceId, OutputLevel, SharedDataModel};
    use crate::luau::play::{PlayLuau, RayHit, RayQuery, ScriptLaunch, TerrainReadFn};

    /// The VM's two lookups before they read the table, kept to prove the
    /// table answers exactly as they did.
    fn legacy_method_applies(class: &str, key: &str) -> bool {
        match key {
            "Destroy" | "Clone" | "ClearAllChildren" | "FindFirstChild" | "FindFirstChildOfClass"
            | "FindFirstChildWhichIsA" | "FindFirstAncestor" | "FindFirstAncestorOfClass" | "FindFirstAncestorWhichIsA"
            | "FindFirstDescendant" | "GetChildren" | "GetDescendants" | "IsA" | "IsDescendantOf" | "IsAncestorOf"
            | "GetFullName" | "WaitForChild" | "GetAttribute" | "SetAttribute" | "GetAttributes"
            | "GetAttributeChangedSignal" | "GetPropertyChangedSignal" | "AddTag" | "RemoveTag" | "HasTag" | "GetTags"
            | "GetDebugId" | "isA" | "findFirstChild" | "children" | "remove" | "Remove" => true,
            "GetPivot" | "PivotTo" => is_base_part(class) || class_is_a(class, "Model"),
            "ApplyImpulse" | "ApplyAngularImpulse" | "ApplyImpulseAtPosition" | "GetMass" | "SetNetworkOwner"
            | "GetNetworkOwner" | "SetNetworkOwnershipAuto" | "BreakJoints" | "GetTouchingParts" | "GetConnectedParts"
            | "GetRootPart" | "CanCollideWith" => is_base_part(class),
            "GetBoundingBox" | "GetExtentsSize" | "MoveTo" | "SetPrimaryPartCFrame" | "GetPrimaryPartCFrame"
            | "TranslateBy" | "GetModelCFrame" => class_is_a(class, "Model") || (key == "MoveTo" && class == "Humanoid"),
            "TakeDamage" | "Move" | "ChangeState" | "GetState" | "UnequipTools" | "EquipTool" | "SetStateEnabled"
            | "GetStateEnabled" => class == "Humanoid",
            "CaptureFocus" | "ReleaseFocus" | "IsFocused" => class == "TextBox",
            "Sit" => matches!(class, "Seat" | "VehicleSeat"),
            "LoadAnimation" | "GetPlayingAnimationTracks" => matches!(class, "Humanoid" | "Animator" | "AnimationController"),
            "Play" | "Stop" => class == "Sound" || class == "AnimationTrack",
            "Pause" | "Resume" => class == "Sound",
            "AdjustSpeed" | "AdjustWeight" | "GetMarkerReachedSignal" | "GetTimeOfKeyframe" => class == "AnimationTrack",
            "GetKeyframes" | "AddKeyframe" | "RemoveKeyframe" => class == "KeyframeSequence",
            "GetPoses" | "AddPose" | "RemovePose" | "GetMarkers" | "AddMarker" | "RemoveMarker" => class == "Keyframe",
            "GetSubPoses" | "AddSubPose" | "RemoveSubPose" => class == "Pose",
            "RegisterKeyframeSequence" | "RegisterActiveKeyframeSequence" | "GetKeyframeSequenceAsync" => {
                class == "KeyframeSequenceProvider"
            }
            "Emit" => class == "ParticleEmitter",
            "Clear" => class == "ParticleEmitter" || class == "Terrain",
            "FillBall" | "FillBlock" | "FillCylinder" | "FillRegion" | "ReplaceMaterial" | "ReadVoxels" | "WriteVoxels"
            | "CellCenterToWorld" | "CellCornerToWorld" | "WorldToCell" | "WorldToCellPreferSolid"
            | "WorldToCellPreferEmpty" | "GetMaterialColor" | "SetMaterialColor" => class == "Terrain",
            "Fire" => class == "BindableEvent",
            "Invoke" => class == "BindableFunction",
            "FireServer" | "FireClient" | "FireAllClients" => class == "RemoteEvent",
            "InvokeServer" | "InvokeClient" => class == "RemoteFunction",
            "GetPlayers" | "GetPlayerFromCharacter" | "GetPlayerByUserId" => class == "Players",
            "GetMouse" | "LoadCharacter" | "Kick" | "GetRankInGroup" | "IsInGroup" | "DistanceFromCharacter" => class == "Player",
            "ViewportPointToRay" | "ScreenPointToRay" | "WorldToViewportPoint" | "WorldToScreenPoint" => class == "Camera",
            "Raycast" | "GetServerTimeNow" | "GetPartBoundsInRadius" | "GetPartBoundsInBox" | "FindPartOnRay"
            | "FindPartOnRayWithIgnoreList" | "Spherecast" | "Blockcast" | "GetRealPhysicsFPS" => class == "Workspace",
            "GetService" | "FindService" | "IsLoaded" | "BindToClose" => class == "DataModel",
            "IsKeyDown" | "IsMouseButtonPressed" => class == "UserInputService" || class == "Player",
            "GetMouseLocation" | "GetMouseDelta" | "GetKeysPressed" | "GetMouseButtonsPressed" | "GetLastInputType"
            | "GetFocusedTextBox" | "IsGamepadButtonDown" | "GetConnectedGamepads" => class == "UserInputService",
            "IsClient" | "IsServer" | "IsStudio" | "IsRunning" | "IsRunMode" | "IsEdit" | "BindToRenderStep"
            | "UnbindFromRenderStep" => class == "RunService",
            "GetTagged" | "GetInstanceAddedSignal" | "GetInstanceRemovedSignal" | "GetAllTags" => class == "CollectionService",
            "JSONEncode" | "JSONDecode" | "GenerateGUID" | "UrlEncode" => class == "HttpService",
            "Mine" | "Describe" | "Query" | "Render" => class == "DataService",
            "PlayLocalSound" => class == "SoundService",
            _ => false,
        }
    }

    fn legacy_is_event(class: &str, key: &str) -> bool {
        match key {
            "Changed" | "ChildAdded" | "ChildRemoved" | "DescendantAdded" | "DescendantRemoving" | "AncestryChanged"
            | "Destroying" | "AttributeChanged" => true,
            "Touched" | "TouchEnded" => is_base_part(class),
            "Died" | "HealthChanged" | "MoveToFinished" | "Running" | "Jumping" | "StateChanged" | "FreeFalling"
            | "Climbing" | "Seated" => class == "Humanoid",
            "AnimationPlayed" => matches!(class, "Animator" | "Humanoid" | "AnimationController"),
            "Stopped" | "DidLoop" | "KeyframeReached" => class == "AnimationTrack",
            "PlayerAdded" | "PlayerRemoving" => class == "Players",
            "CharacterAdded" | "CharacterRemoving" | "CharacterAppearanceLoaded" | "Chatted" | "Idled" => class == "Player",
            "MouseButton1Click" | "MouseButton1Down" | "MouseButton1Up" | "MouseButton2Click" | "Activated"
            | "MouseEnter" | "MouseLeave" => class_is_a(class, "GuiButton") || class_is_a(class, "GuiObject"),
            "Focused" | "FocusLost" => class == "TextBox",
            "Event" => class == "BindableEvent",
            "OnServerEvent" | "OnClientEvent" => class == "RemoteEvent",
            "InputBegan" | "InputEnded" | "InputChanged" | "JumpRequest" | "WindowFocused" | "WindowFocusReleased" => {
                class == "UserInputService" || class_is_a(class, "GuiObject")
            }
            "MouseClick" | "RightMouseClick" | "MouseHoverEnter" | "MouseHoverLeave" => class == "ClickDetector",
            "Triggered" | "TriggerEnded" | "PromptShown" | "PromptHidden" => class == "ProximityPrompt",
            "Ended" => class == "Sound" || class == "AnimationTrack",
            "Played" | "Loaded" => class == "Sound",
            "Close" | "Loaded_" => class == "DataModel",
            _ => false,
        }
    }

    /// Every class the tree knows, plus the services.
    fn classes() -> Vec<&'static str> {
        let mut out: Vec<&'static str> = CREATABLE.to_vec();
        out.extend(services());
        out.extend(["DataModel", "Player", "Terrain", "AnimationTrack", "KeyframeSequenceProvider", "GuiObject"]);
        for c in out.clone() {
            out.extend(class_ancestry(c).iter().copied());
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Where the table and the old lookups disagree, as `(class, name)`.
    fn disagreements(table: fn(&str, &str) -> bool, legacy: fn(&str, &str) -> bool, extra: &[&str]) -> Vec<(String, String)> {
        let mut names: Vec<&str> = MEMBERS.iter().map(|m| m.name).chain(extra.iter().copied()).collect();
        names.sort_unstable();
        names.dedup();
        let mut out = Vec::new();
        for class in classes() {
            for name in &names {
                if table(class, name) != legacy(class, name) {
                    out.push((class.to_string(), name.to_string()));
                }
            }
        }
        out
    }

    #[test]
    fn the_table_resolves_methods_exactly_as_the_vm_did() {
        // Two differences, both on purpose. Spherecast and Blockcast were
        // listed but never implemented, so the table leaves them out and a
        // child of that name is no longer shadowed. SetArgumentTypes was
        // implemented in the prelude but never listed, so no script could
        // reach it; the table lists it. Nothing else may differ.
        // UnreliableRemoteEvent, which the session already sends on its
        // unreliable lane, gets RemoteEvent's methods; a script could not
        // call them on it before.
        let expected = ["Spherecast", "Blockcast", "SetArgumentTypes"];
        let unreliable = ["FireServer", "FireClient", "FireAllClients"];
        let diff = disagreements(method_applies, legacy_method_applies, &expected);
        let unexpected: Vec<_> = diff
            .iter()
            .filter(|(c, n)| !expected.contains(&n.as_str()) && !(c == "UnreliableRemoteEvent" && unreliable.contains(&n.as_str())))
            .collect();
        assert!(unexpected.is_empty(), "{unexpected:?}");
        for class in ["RemoteEvent", "UnreliableRemoteEvent", "RemoteFunction"] {
            assert!(method_applies(class, "SetArgumentTypes"), "{class}");
        }
        assert!(unreliable.iter().all(|m| method_applies("UnreliableRemoteEvent", m)));
        assert!(!method_applies("Workspace", "Spherecast"));
    }

    #[test]
    fn the_table_resolves_signals_exactly_as_the_vm_did() {
        // `Loaded_` on the DataModel was a placeholder name nothing fired,
        // an UnreliableRemoteEvent gets RemoteEvent's two signals, and a
        // Sound gained Paused, Resumed, Stopped and DidLoop when its player
        // came to report them.
        let diff = disagreements(is_event, legacy_is_event, &["Loaded_"]);
        let unexpected: Vec<_> = diff
            .iter()
            .filter(|(c, n)| n != "Loaded_" && !(c == "UnreliableRemoteEvent" && matches!(n.as_str(), "OnServerEvent" | "OnClientEvent")))
            .filter(|(c, n)| !(c == "Sound" && matches!(n.as_str(), "Paused" | "Resumed" | "Stopped" | "DidLoop")))
            .collect();
        assert!(unexpected.is_empty(), "{unexpected:?}");
        assert!(is_event("UnreliableRemoteEvent", "OnServerEvent") && is_event("UnreliableRemoteEvent", "OnClientEvent"));
    }

    #[test]
    fn member_names_are_unique_per_class() {
        for class in classes() {
            let mut seen = std::collections::HashSet::new();
            for m in MEMBERS.iter().filter(|m| m.on.contains(class)) {
                assert!(seen.insert(m.name), "{class}.{} is listed twice", m.name);
            }
        }
    }

    // ── Against a live VM ──

    fn no_rays(_: &RayQuery) -> Option<RayHit> {
        None
    }

    /// Runs `source` as a Script and returns what it printed.
    fn run(source: &str) -> (Vec<String>, SharedDataModel) {
        let dm = new_shared();
        let script: InstanceId = {
            let mut g = dm.lock();
            let players = g.get_service("Players").expect("Players");
            let player = g.create_virtual("Player", "Tester", Some(players));
            g.local_player = Some(player);
            // Seeded as a session is: Workspace holds its one Terrain.
            let workspace = g.get_service("Workspace").expect("Workspace");
            crate::luau::play::terrain::ensure_terrain_instance(&mut g, workspace);
            let sss = g.get_service("ServerScriptService").expect("ServerScriptService");
            g.create_virtual("Script", "Catalog", Some(sss))
        };
        let mut vm = PlayLuau::new(dm.clone()).expect("the prelude loads");
        let no_terrain: &TerrainReadFn<'_> = &|_| {};
        let launch = ScriptLaunch { instance: script, source: source.to_string(), chunk_name: "Catalog".into() };
        vm.run_scripts(vec![launch], &no_rays, no_terrain);
        let lines = dm.lock().output.iter().filter(|l| !l.lifecycle).map(|l| l.text.clone()).collect();
        (lines, dm)
    }

    fn quote(s: &str) -> String {
        format!("{s:?}")
    }

    /// A Luau expression for a live instance of `class`.
    fn live(class: &str) -> String {
        match class {
            "DataModel" => "game".into(),
            "Player" => "game:GetService(\"Players\"):GetPlayers()[1]".into(),
            // The session gives Workspace its one Terrain (see `run`), and
            // `Instance.new` refuses another, as Roblox does.
            "Terrain" => "workspace.Terrain".into(),
            "AnimationTrack" => {
                "(function() local h = Instance.new(\"Humanoid\"); local a = Instance.new(\"Animation\"); \
                 return h:LoadAnimation(a) end)()"
                    .into()
            }
            c if services().contains(&c) => format!("game:GetService({})", quote(c)),
            c => format!("Instance.new({})", quote(c)),
        }
    }

    #[test]
    fn every_method_and_signal_resolves_on_a_live_instance() {
        // One probe per (class, member). Signals are prelude tables, which
        // `typeof` calls "table", so a table with a Connect method counts as
        // a signal.
        let mut probes = String::from("local function probe(label, make, name, want)\n\
            local ok, inst = pcall(make)\n\
            if not ok or inst == nil then print(\"cannot make \" .. label .. \": \" .. tostring(inst)) return end\n\
            local ok2, v = pcall(function() return inst[name] end)\n\
            local got = ok2 and typeof(v) or \"error\"\n\
            if got == \"table\" and type(v.Connect) == \"function\" then got = \"RBXScriptSignal\" end\n\
            if got ~= want then print(label .. \".\" .. name .. \" is \" .. got .. \", wanted \" .. want) end\n\
            end\n");
        let mut classes_with_members: Vec<&str> = Vec::new();
        for m in MEMBERS {
            for c in m.on.exact {
                classes_with_members.push(c);
            }
        }
        classes_with_members.extend(["Part", "Model", "Frame", "TextButton"]);
        classes_with_members.sort_unstable();
        classes_with_members.dedup();
        for class in classes_with_members {
            for m in MEMBERS.iter().filter(|m| m.on.contains(class)) {
                let want = match m.kind {
                    Kind::Method | Kind::ServiceMethod => "function",
                    Kind::Event | Kind::ServiceEvent => "RBXScriptSignal",
                    Kind::Callback => continue,
                };
                probes.push_str(&format!(
                    "probe({}, function() return {} end, {}, {})\n",
                    quote(class),
                    live(class),
                    quote(m.name),
                    quote(want)
                ));
            }
        }
        let (out, dm) = run(&probes);
        let errors: Vec<_> = dm.lock().output.iter().filter(|l| l.level == OutputLevel::Error).map(|l| l.text.clone()).collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(out.is_empty(), "members the VM does not resolve:\n{}", out.join("\n"));
    }

    #[test]
    fn every_library_member_and_global_exists() {
        let mut src = String::new();
        for (lib, fields) in LIBRARIES {
            for f in *fields {
                src.push_str(&format!(
                    "if {lib} == nil or {lib}[{}] == nil then print(\"missing {lib}.{}\") end\n",
                    quote(f.name),
                    f.name
                ));
            }
        }
        for g in GLOBALS {
            src.push_str(&format!("if {} == nil then print(\"missing global {}\") end\n", g.name, g.name));
        }
        let (out, _) = run(&src);
        assert!(out.is_empty(), "{}", out.join("\n"));
    }

    #[test]
    fn every_object_field_exists_on_its_value() {
        let values = [
            ("Vector3", "Vector3.new(1, 2, 3)"),
            ("Vector2", "Vector2.new(1, 2)"),
            ("CFrame", "CFrame.new(1, 2, 3)"),
            ("Color3", "Color3.new(1, 0, 0)"),
            ("UDim2", "UDim2.new(0, 1, 0, 2)"),
            ("Ray", "Ray.new(Vector3.new(), Vector3.new(0, 0, 1))"),
            ("PlayerMouse", "game:GetService(\"Players\"):GetPlayers()[1]:GetMouse()"),
            ("RBXScriptSignal", "Instance.new(\"Part\").Changed"),
            ("RBXScriptConnection", "Instance.new(\"Part\").Changed:Connect(function() end)"),
            (
                "Tween",
                "game:GetService(\"TweenService\"):Create(Instance.new(\"Part\"), TweenInfo.new(1), {Transparency = 1})",
            ),
        ];
        let mut src = String::new();
        for (ty, make) in values {
            for f in object_fields(ty) {
                // A field that starts nil (Mouse.Target with nothing under
                // the cursor) is still a field: only a read that errors counts.
                src.push_str(&format!(
                    "do local ok, v = pcall(function() local o = {make}; return o[{}] end)\n\
                     if not ok then print(\"{ty}.{} errors: \" .. tostring(v)) \
                     elseif v == nil and {} then print(\"{ty}.{} is nil\") end end\n",
                    quote(f.name),
                    f.name,
                    if matches!(f.kind, Kind::Method | Kind::Event) { "true" } else { "false" },
                    f.name
                ));
            }
        }
        let (out, _) = run(&src);
        assert!(out.is_empty(), "{}", out.join("\n"));
    }

    #[test]
    fn properties_come_from_the_tree() {
        let mut g = crate::datamodel::DataModel::new();
        for class in CREATABLE {
            let id = g.create(class);
            for p in properties_of(class) {
                assert!(g.get_prop(id, p.name).is_some(), "{class}.{} is listed but the tree has none", p.name);
            }
        }
        let part = properties_of("Part");
        assert!(part.iter().any(|p| p.name == "Anchored" && p.ty == "boolean"));
        assert!(part.iter().any(|p| p.name == "Material" && p.ty == "Enum.Material"));
        assert!(part.iter().any(|p| p.name == "Position" && p.ty == "Vector3"));
        assert!(properties_of("Model").iter().any(|p| p.name == "PrimaryPart" && p.ty == "BasePart?"));
    }

    #[test]
    fn a_player_offers_its_own_members_and_not_a_remotes() {
        let names: Vec<&str> = members_of("Player").map(|m| m.name).collect();
        for want in ["GetMouse", "LoadCharacter", "Kick", "CharacterAdded", "IsKeyDown", "Destroy", "FindFirstChild"] {
            assert!(names.contains(&want), "{want} missing from {names:?}");
        }
        for never in ["FireServer", "TakeDamage", "Raycast", "isA", "Remove"] {
            assert!(!names.contains(&never), "{never} offered on a Player");
        }
    }
}
