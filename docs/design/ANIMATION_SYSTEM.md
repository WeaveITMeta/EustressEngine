# Animation System

Animation in Eustress is built from instances. An `Animation` names a clip, an
`Animator` plays clips on a rig, and `Animator:LoadAnimation` returns an
`AnimationTrack` whose properties and methods control one playing clip. Clips
are data: glTF files, or `KeyframeSequence` records that a person, a script or
an agent can write. One runtime plays them on every kind of rig: the Eustress
avatar's skeleton, an R15 model made of parts, a machine joined by `Motor6D`s.
Every player is their Eustress avatar, in imported games too, where Roblox's
R15 clips play on the avatar's skeleton. The avatar walks through an ordinary
`Animate` script, and a hosted session replicates tracks as control changes
stamped on the session tick.

This page is the design and the contract. The status of each part is in
[Phases](#phases).

## What exists

Read from the repository on 2026-09-24.

| Area | Today | Where |
|---|---|---|
| The avatar's motion | One Bevy `AnimationGraph` per avatar: a `ground` blend (idle, walk, run) and an `air` blend (jump). Every clip plays once and only weights move. The weights are written into the graph asset every frame. Walk and run play at ground speed over authored speeds of 1.45 and 3.9 m/s, clamped to 0.6 to 1.6. `lock_root_motion` pins the hips. A liveness probe reports LIVE or FROZEN. | `common/src/avatar/anim.rs` |
| Clip sources | Four slots per rig (idle, walk, run, jump) in `RigDefinition.animations`, replaced per body option by `StarterPlayer/Characters/*.rig.toml`. glTF clips are retargeted onto canonical bone ids. | `avatar/space_character.rs`, `avatar/retarget.rs` |
| Procedural layer | Breathing, exertion, idle breaks, landing flex and look-at, composed onto the animated pose in `AvatarSystems::PostAnim`. | `avatar/procedural.rs` |
| Hard links | `procedural.rs` orders itself after `anim::lock_root_motion`, and `spawn.rs` waits for `anim::AvatarMotionGraph`, both behind `cfg(feature = "model-import")` because `anim` compiles only with `all(physics, model-import)`. The Player's agent loop reads `AvatarMotionGraph` for its observation. | `avatar/mod.rs:51`, `procedural.rs:97`, `spawn.rs:140`, `client/src/systems/agent_control.rs:591` |
| Classes | `Animator`, `Animation`, `AnimationController`, `KeyframeSequence`, `Pose`, `NumberPose`, `KeyframeMarker`, `CurveAnimation`, `Motor6D`, `Bone`, `IKControl`, `FaceControls` and `AnimationConstraint` are declared, with components and spawners. `Keyframe` is a struct inside `KeyframeSequence` that holds one `Transform` per keyframe. No system reads the `Animator` or `KeyframeSequence` components. | `common/src/classes.rs`, `engine/src/spawners/animation/` |
| Class templates | `KeyframeSequence` defaults to priority 0 (Idle) and looped false; Roblox's defaults are Action and true. `Motor6D` is modelled as a velocity motor. `Animation` and `AnimationController` have no template. | `common/assets/class_schema/` |
| Scripts | `GetPlayingAnimationTracks` returns `{}`. `Humanoid:LoadAnimation` and `Animator:LoadAnimation` return a stub whose `Play` sets a flag. `AnimationController:LoadAnimation` is not routed. | `common/src/luau/play/instance.rs:627`, `prelude.luau:917` |
| The Play character | A Model holding `HumanoidRootPart` (bound to the avatar body), `Head` and `Humanoid`, with no `Animator`. The Humanoid reports `MoveDirection`; `GetState` answers Dead or Running. | `engine/src/play_datamodel/pull.rs:372` |
| StarterCharacterScripts | Nothing copies them into a character, and the launch lists skip the templates, so a Space's character scripts never run. Studio launches scripts once, when Play starts and its player joins. | `play_datamodel/seed.rs:281`, `common/src/tree_scripts.rs:228` |
| Motor6D | A `Motor6D` loaded from a Space becomes `RevoluteJoint::new(part0, part1)` with no anchors: a free hinge that pins both parts' origins together and ignores `C0` and `C1`. `engine/src/motor6d.rs` is an empty plugin. Nothing writes `Motor6D.Transform`. | `engine/src/physics/joint_resolver.rs:573` |
| Legacy stacks | `SharedAnimationPlugin` (1,022 lines) and `services/animation.rs` (964 lines, with its own `HumanoidBone`, state machine and blend trees) are still added by Studio (`play_mode.rs:1721`) and by the Player (`CharacterAnimationPlugin`, `client/src/main.rs:285`). They act on components that only the unused `skinned_character` path inserts. | `common/src/plugins/animation_plugin.rs` |
| Property animation | The Timeline's keyframed and procedural property tracks, a separate system whose component is also named `AnimationTrack`. | `engine/src/timeline_animation.rs` |
| Multiplayer | A remote avatar walks with its sender's replicated intent through the avatar runtime, so the motion graph animates it locally. No animation state crosses the wire. | `eustress-networking/src/session.rs` |

**Imported games.** Vehicle Simulator, as re-imported on 2026-09-24:

- 7 `Animation`, 16 `Animator`, 36 `Humanoid` and 304 `Motor6D` instances; 29
  script files make 207 animation calls (`LoadAnimation`, `AdjustSpeed`,
  `GetMarkerReachedSignal` and the rest).
- No `KeyframeSequence` reached disk. `Keyframe` is unmapped (156 dropped with
  their subtrees).
- Every `AnimationId` sits in `[properties.extras]`, which no reader reads, as
  a raw `rbxassetid://` that could not be fetched without a credential.
- The importer folds value objects into attributes, and the fold dropped their
  children. `ServerStorage/RBX_ANIMSAVES`, where Roblox's Animation Editor keeps
  its saves, is 39 empty attributes; the KeyframeSequences under them are gone.
  Each pedestrian's `Animate` script lost its `idle`, `walk` and `run` values
  the same way, so `LocalNPCManagement`'s
  `NPCToMove.Animate.walk.WalkAnim` finds nothing.
- The pedestrians are R15 rigs running Roblox's standard `Animate` (the R15
  "moods" version), a LocalScript whose defaults are Roblox asset ids written in
  its source (`507766666` for idle and so on). `CharacterScript` makes
  Animations at run time with ids such as
  `http://www.roblox.com/Asset?ID=132193066130399`.
- The importer's fetch cache holds 4 animation assets among 642: all rbxm or
  rbxmx files whose root is a `KeyframeSequence`. `507770239` is R15's default
  wave; `54584713` is an R6 clip in the legacy XML form, with poses running
  `Torso` > `Right Arm` > `Handle`.

**Bevy 0.19**, read from `bevy_animation-0.19.0`:

- A blend node normalizes weights over the children that animate a given
  target. A child with no curve for a bone contributes nothing to it.
- The blended value overwrites the property; nothing blends with the value
  already there. A partial weight toward a rest pose therefore needs a rest-pose
  contributor.
- Two contributors of weight 0 in one blend compute 0/0 and write NaN (`combine`
  in `animation_curves.rs`). A clip node whose `ActiveAnimation` weight is exactly
  0 is skipped before it contributes.
- A paused `ActiveAnimation` never advances, and `set_seek_time` moves it
  without firing events.
- Masks are per node, 64 groups. A node reached through two parents takes one
  computed mask, so a graph that uses masks must be a tree.
- `animated_field!` animates any field of a reflected component. glTF clips use
  `animated_field!(Transform::rotation)` and its siblings.
- Changing the graph asset rebuilds its threaded evaluation order. Today's motion
  graph does that every frame.

## Principles

1. Every animation is an instance, and every behaviour is a property or a
   script. The engine's defaults are written the way a Space would write them.
2. One runtime for every rig: a skeleton's bones, parts joined by `Motor6D`,
   and `AnimationConstraint` joints.
3. Roblox's semantics by default. Each deliberate difference is listed in
   [Where Eustress differs](#where-eustress-differs).
4. A track is a pure function of its control changes and the session clock.
   Two machines holding the same changes show the same pose at the same tick,
   which is what replication, late join, replay and hit rewind need.
5. The tree is the source of truth on both shells: Studio's Play tree and the
   Player's own tree. The ECS draws.
6. Contracts compile unconditionally. System sets and markers live in an
   ungated module, and a feature only adds systems to them.

## 1. Classes and properties

### Animation

| Member | Behaviour |
|---|---|
| `AnimationId` | The clip, as a content id (below). Stored in `[properties] animation_id`, as written: a Roblox id resolves through the Space's id map. Changing it after `LoadAnimation` leaves loaded tracks on the clip they loaded, as in Roblox. |

### Where clips come from

| AnimationId | Resolves to |
|---|---|
| `space://Characters/Wave.glb#Animation1` | A glTF clip in the Space. With no fragment, `Animation0`. |
| `bundled://characters/animations/male_walking.glb` | A clip shipped with Eustress. |
| `space://ReplicatedStorage/Animations/Wave` | The `KeyframeSequence` at that path: the live instance in the tree when there is one, its record otherwise. |
| `space://assets/animations/rbx-507770239.anim.toml` | A clip file (a `KeyframeSequence` record kept as an asset). |
| `rig://walk` | The clip the Animator's rig names `walk`: the body option's own clip, or the Space's `*.rig.toml` replacement. |
| `rbxassetid://507770239`, `http://www.roblox.com/asset/?id=507770239` | The file the Space's Roblox id map (`assets/roblox_ids.toml`) names for that id. |
| `active://3` | A sequence registered this session by `KeyframeSequenceProvider:RegisterKeyframeSequence`. |

`space://` paths stay inside the Space, with the same character rules as
`*.rig.toml`. An id that resolves to nothing leaves `Length` at 0 and prints
once to Output with the remedy: re-import with a credential, or add the id to
`assets/roblox_ids.toml`.

### Animator

Child of a `Humanoid` or an `AnimationController`. Its rig is that parent's
Model. One Animator drives a rig; a second is ignored with a warning.

| Member | Behaviour |
|---|---|
| `LoadAnimation(animation)` | Returns a new `AnimationTrack` named after the Animation. At most 256 tracks per Animator, Roblox's limit. At the limit, loading releases the oldest track that has finished and faded out; with none to release, the call warns once and returns a track that plays nothing. |
| `GetPlayingAnimationTracks()` | Every track that is playing or still fading out. |
| `AnimationPlayed(track)` | Fires when a track starts playing. |
| `PreferLodEnabled` | Default true. With it on, a distant rig evaluates less often (phase 5). |
| `EvaluationThrottled` | Read-only; true while a rig is evaluated at the reduced rate. |
| `RootMotion`, `RootMotionWeight` | Read-only; identity and 0 until root motion extraction exists. |
| `ApplyJointVelocities(motors)` | Accepted. It sets part velocities once physically simulated joints exist (phase 5). |
| `StepAnimations(dt)` | Advances a rig outside Play, for Studio's preview (phase 5). |
| `RootMotionMode` | Eustress. `Pin`, the default on avatars: the character controller owns translation, and the clip's hips translation is pinned to the bind pose, as `lock_root_motion` does today. `Keep`, the default elsewhere: the clip's hips translation shows. |
| `Enabled` | Eustress, from today's template. False rests the rig. |

`Humanoid:LoadAnimation`, `Humanoid:GetPlayingAnimationTracks`,
`AnimationController:LoadAnimation` and
`AnimationController:GetPlayingAnimationTracks` are Roblox's deprecated proxies:
each finds the `Animator` child, making one if there is none, and forwards.

### AnimationController

Holds the `Animator` for a rig with no Humanoid: a door, a crane, a creature.
It has no properties of its own.

### AnimationTrack

Made only by `LoadAnimation`, never by `Instance.new`. Its `Parent` is nil, as
in Roblox; its Animator holds it.

| Member | Behaviour |
|---|---|
| `Animation` | Read-only; the Animation it was loaded from. |
| `IsPlaying` | Read-only; true from `Play` until `Stop` or the end of a non-looped clip. |
| `Length` | Read-only; seconds. 0 until the clip is loaded. |
| `Looped` | Starts as the clip's `Loop`. A change while playing takes effect when the current cycle ends. |
| `Priority` | Starts as the clip's `Priority`. `Core` (lowest), `Idle`, `Movement`, `Action`, `Action2`, `Action3`, `Action4` (highest). |
| `Speed` | Read-only; set by `Play` and `AdjustSpeed`. Negative plays backward; 0 pauses. |
| `TimePosition` | Seconds into the clip. Writing it seeks, without firing the keyframes and markers it skips. |
| `WeightCurrent`, `WeightTarget` | Read-only. `WeightCurrent` moves linearly to `WeightTarget` over the fade time of the call that set it. |
| `Play(fadeTime = 0.1, weight = 1, speed = 1)` | Starts at time 0 and fades from the present weight to `weight`. On a track already playing, it restarts from 0 and fades from wherever the weight is, so nothing pops. `IsPlaying` reads true at once. |
| `Stop(fadeTime = 0.1)` | `IsPlaying` reads false at once, `Stopped` fires, the weight fades to 0, and `Ended` fires when it arrives. |
| `AdjustSpeed(speed = 1)` | Changes `Speed` now. |
| `AdjustWeight(weight = 1, fadeTime = 0.1)` | Sets `WeightTarget`; `WeightCurrent` follows over `fadeTime`. |
| `GetMarkerReachedSignal(name)` | A signal that fires with the marker's `Value` each time playback crosses a `KeyframeMarker` of that name. |
| `GetTimeOfKeyframe(name)` | The time of the first keyframe with that name. |
| `Stopped` | Fires when the track begins to wind down: on `Stop`, or when a non-looped clip reaches its end (its start, playing backward). |
| `Ended` | Fires when the track no longer moves anything: the fade-out is done. |
| `DidLoop` | Fires on the update after a looped track wraps. |
| `KeyframeReached(name)` | Fires for each keyframe crossed whose name is not `Keyframe`. |
| `Destroy()` | Unloads the track. |

A non-looped track that reaches its end holds its last frame and fades out over
its last `Play` fade time. Keyframes and markers crossed in one update fire in
playback order, across a wrap as well. A track stays loaded until it is
destroyed or its Animator goes, and at the limit the oldest finished track is
released (above).

### KeyframeSequence, Keyframe, Pose, NumberPose, KeyframeMarker

A clip written as data. On disk a sequence is one record; in the tree it has
Roblox's shape, `Keyframe` children holding `Pose` children nested the way the
joints nest, so scripts read and edit it with Roblox's API.

| Class | Members |
|---|---|
| `KeyframeSequence` | `Loop` (default true), `Priority` (default `Action`), `AuthoredHipHeight` (default 2); `AddKeyframe`, `GetKeyframes`, `RemoveKeyframe`. The last keyframe's time is the clip's length. |
| `Keyframe` | `Time`; its `Name` drives `KeyframeReached`; `AddPose`, `GetPoses`, `RemovePose`, `AddMarker`, `GetMarkers`, `RemoveMarker`. |
| `Pose` | `CFrame`: the joint's `Transform` at this keyframe. `EasingStyle` (default `Linear`) and `EasingDirection` (default `In`) shape the segment from this keyframe to the next keyframe that poses the same joint. `Weight` 0 marks a pose that only holds the hierarchy together, as Roblox's Animation Editor saves every unkeyed parent: it keys nothing, so an arm-only clip leaves the torso to the tracks below it. Any other weight keys the joint fully. `AddSubPose`, `GetSubPoses`, `RemoveSubPose`. |
| `NumberPose` | `Value`: a number channel, such as a face control. |
| `KeyframeMarker` | `Name` and `Value`, reported by `GetMarkerReachedSignal`. |

**Which joint a Pose drives.** On parts, the `Motor6D` whose `Part1` has the
Pose's name and whose `Part0` has its parent Pose's name. That is Roblox's rule:
a Pose named `LowerTorso` under `HumanoidRootPart` drives the `Root` joint. On a
skeleton, the bone of that name, matched through the canonical bone keys, so
`LeftUpLeg`, `mixamorig:LeftUpLeg_056` and `leftupleg` are one bone.

**Easing.** `PoseEasingStyle` is `Linear`, `Constant`, `Elastic`, `Cubic`,
`Bounce` or `CubicV2`. `PoseEasingDirection` keeps Roblox's legacy meaning, in
which `In` and `Out` are the reverse of TweenService's. `Cubic` plays as Roblox's
runtime plays it; `CubicV2` is the corrected curve.

`KeyframeSequenceProvider` answers `RegisterKeyframeSequence(sequence)` with an
`active://` id that plays the live sequence for the session (the same sequence
keeps its id), and `GetKeyframeSequenceAsync(id)` with a new, unparented copy of
the sequence any AnimationId names: a registered or live sequence, or one read
from its record, a Roblox id through the id map. A glTF clip has no sequence.
Neither touches the network.

### The clip record

A `KeyframeSequence` record is TOML, one file per clip: `_instance.toml` in the
sequence's folder, or a `.anim.toml` file under `assets/animations/`. Positions
are metres; rotations are either a quaternion (`x, y, z, w`) or three angles in
degrees applied in the order Roblox's `Orientation` uses.

```toml
[metadata]
class_name = "KeyframeSequence"
name = "Wave"

[keyframe_sequence]
loop = false
priority = "Action"
rig = "R15"                     # optional; checked when the clip binds
source = "rbxassetid://507770239"   # provenance, written by the importer

[[keyframes]]
time = 0.0

[keyframes.poses.RightUpperArm]
parent = "UpperTorso"
position = [0.0, 0.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]

[[keyframes]]
time = 0.3
name = "Raised"

[keyframes.poses.RightUpperArm]
parent = "UpperTorso"
rotation = [0.0, 0.0, 150.0]
easing = "CubicV2"
direction = "Out"

[[keyframes.markers]]
name = "Wave"
value = "start"
```

`parent` names the pose above; the rig needs it only where two joints share a
name. A pose with `weight = 0` is structural and keys nothing (see `Pose`).
Number channels sit under `[keyframes.numbers]`. The loader caps a record at
8 MB, 10,000 keyframes and 512 poses per keyframe, and refuses non-finite
numbers, because published worlds carry clips from other people.

People write these by hand; scripts build them with `Instance.new`; an agent
writes one over MCP with `write_file` and plays it with `execute_luau`; the
importer writes them from Roblox assets. An Animator builds a clip the first
time one of its tracks names it, so an edit made to a sequence during Play shows
on Animators that load it afterwards.

### Joints that animate

| Class | Animated value | Notes |
|---|---|---|
| `Motor6D` | `Transform`, with `Part1.CFrame = Part0.CFrame * C0 * Transform * C1:Inverse()` | `Part0`, `Part1`, `C0`, `C1` and `Enabled` as in Roblox. Old R6 scripts drive joints through the legacy `DesiredAngle` and `MaxVelocity`: `CurrentAngle` steps toward `DesiredAngle` by `MaxVelocity` every 1/60 s and turns the joint about `C0`'s Z axis, as Roblox's `Motor` does. |
| `Bone` | `Transform`, over the rest `CFrame` | For Roblox skinned MeshParts, once the mesh importer reads skinning. |
| `AnimationConstraint` | `Transform` | Roblox's newer avatar joint. `IsKinematic = true` follows like a `Motor6D`; force-limited joints come with physically simulated avatars (phase 5). |

A script's write to `Transform` replaces the animated value for the frame it is
made in, and stays on a joint that no playing track animates.

### Humanoid state

The Animate script and imported scripts read locomotion from the Humanoid:

| Member | Behaviour |
|---|---|
| `GetState()` | `Running`, `Jumping`, `Freefall`, `Landed`, `Climbing`, `Seated`, `Dead`, `Swimming`. |
| `Running(speed)` | Horizontal speed in metres per second; fires when it changes by more than 0.05 m/s and when it reaches 0. |
| `Jumping(active)`, `FreeFalling(active)`, `Climbing(speed)` | As in Roblox. |
| `StateChanged(old, new)` | Every state change. |
| `WalkSpeed` | The avatar's walking pace: the body's own unless a Space or a script sets it. |
| `RunSpeed` attribute | Eustress: the avatar's running pace, capped at a human sprint (12.42 m/s) and never below `WalkSpeed`. |
| `StrideScale` attribute | Eustress: the body's stride over the stride its clips were authored for, so playback can match ground speed. |

Every character reports from its avatar's locomotion and climb state each frame
(`animation::humanoid::report_state`); the tree keeps each Humanoid's state from
frame to frame, so a character builder stores nothing. Every attached climb
phase is `Climbing`, and a non-finite speed reads as 0. `WalkSpeed` comes from
the body. `furnish_character` seeds `RunSpeed` and `StrideScale` when the
character is built, and each frame's update keeps `RunSpeed` current, so a
script reads the running pace in force. The default `Animate` picks its gait
from the ground speed against each clip's `AuthoredSpeed` times `StrideScale`,
the way legs choose a gait, so a Space's fast `WalkSpeed` plays the run rather
than a walk dragged across the ground. NPC humanoids report from their bodies in `npc.rs`:
horizontal speed, and vertical speed with ground contact.

### Blending

Roblox's rule, per joint: tracks are ranked from the highest priority to the
lowest; a priority takes what is left of a total weight of 1; tracks of one
priority share their part in proportion to their weights; the joint's rest pose
takes whatever remains. For each joint, with `w` each track's `WeightCurrent`:

```rust
let mut remaining = 1.0;
for level in [Action4, Action3, Action2, Action, Movement, Idle, Core] {
    let tracks = tracks_animating_this_joint_at(level);
    let total: f32 = tracks.iter().map(|t| t.w).sum();
    if total <= 0.0 || remaining <= 0.0 {
        continue;
    }
    let take = total.min(remaining);
    for t in tracks {
        t.effective = t.w / total * take;
    }
    remaining -= take;
}
rest_pose.effective = remaining;
```

Four tracks at 0.5 in one priority get 0.25 each. An arm wave at `Action` and
weight 1 owns the arm, while the legs keep walking at `Movement`. An `Action`
track at 0.3 over a walk at 1 shows 30% and 70%. A lone track at 0.5 lies halfway
to the rest pose.

### Where Eustress differs

| Roblox | Eustress | Why |
|---|---|---|
| A clip is an uploaded asset id; a `KeyframeSequence` plays after publishing or `RegisterKeyframeSequence`. | A content id names the clip where it lives in the Space; Roblox ids resolve through the Space's map. | A Space is a self-contained folder under git. |
| Clips come from the Animation Editor. | glTF clips play directly, and humanoid clips retarget by bone name. | Most motion data is glTF. |
| No `rig://`. | `rig://walk` names the rig's own clip. | One Animate script serves every body option and every `*.rig.toml` body. |
| A replicated track's time is synced. | A track is a function of tick-stamped control changes. | Every machine computes the same time at the same tick; a late joiner lands mid-cycle exactly. |
| An Animator must be created on the server for its tracks to replicate. | Ownership decides: the character's player, or the host. | Where an Animator was made says nothing about who controls the rig. |
| A `KeyframeSequence` is an instance tree. | One record per sequence; the tree still shows `Keyframe` and `Pose` instances. | A clip is one file in git, not hundreds. |
| `Motor6D` moves `Part1` inside the rigid assembly. | `Part1` follows `Part0` as a kinematic body each frame; welds hanging off a rig follow the same way. | The same pose, with no joint solve. |
| The Animator writes `Transform` between `PreAnimation` and `PreSimulation`, so an override has to run in `PreSimulation`. | A `Transform` write wins for the frame it is made in, whatever the phase. | Scripts need no RunService phase to override a joint. |
| Studs. | Metres. Imported poses convert with their rig. | Eustress is metre-native. |
| A game's `StarterCharacter` replaces the player's body. | Every player is their Eustress avatar in every Space; a game's R15 clips retarget onto the avatar's skeleton, and R15 part names answer through stand-ins. | A player's avatar belongs to their account. |

## 2. The runtime

### Two halves

- **The track model**, `common/src/datamodel/animation.rs`: plain Rust inside
  the DataModel, beside `commerce`. It holds each Animator's tracks, their
  control state, the clock, fades, events, the blending rule and a log of
  control changes. Both script languages call it, replication reads and writes
  its changes, and it is tested without Bevy.
- **The pose evaluator**, `common/src/animation/`: a Bevy plugin,
  `AnimatorPlugin`, that both shells add. It binds each Animator to its rig,
  loads clips, builds the rig's `AnimationGraph` and drives its
  `AnimationPlayer`. It compiles without `physics` and without `model-import`;
  glTF retargeting joins when `model-import` is on.

Both shells insert one resource, `LiveTree(SharedDataModel)`: Studio at Play
start beside `PlayDataModel`, the Player beside `PlayerTree`. The evaluator
reads the tree through it on either shell.

### The frame

```text
Studio                              Player
Update                              Update
  Pull (the clock advances)           TreeClock (the clock advances)
  AnimatorSet::Step                   replication writes the tree
  Scripts (Luau, Rune)                AnimatorSet::Step
  Apply                               RunTreeScripts (LocalScripts)
  AnimatorSet::Drive                  AnimatorSet::Drive
  End                                 TreeApply
PostUpdate (both)
  AnimationSystems        Bevy samples every rig's graph
  AnimatorSet::RootMotion RootMotionMode Pin
  AnimatorSet::Procedural breathing, exertion, idle breaks, landing flex, look-at
  AnimatorSet::Ik         IKControl, foot placement, climb grips
  AnimatorSet::Facing     the avatar's facing
  AnimatorSet::Overrides  script writes to Transform
  AnimatorSet::Joints     the Motor6D forward pass
  TransformSystems::Propagate
```

- `step` advances every track to the session clock, fires events and writes the
  read-only properties. It runs before scripts because the Player trims every
  event at the end of `TreeApply`; an event written after its scripts ran would
  be lost there.
- `drive` runs after scripts, so a `Play` shows in the frame that made it. It
  hands each rig its tracks' times and effective weights. In Studio it follows
  Apply, so the frame's new parts already have entities.
- The avatar's `AvatarSystems::PostAnim` sits inside the `AnimatorSet` window:
  after `RootMotion`, whose hips pin the life layer's hips writes build on, and
  before `Overrides` and `Joints`.

### Binding a rig

| Rig | Found by | Joints | Animated value | Rest pose |
|---|---|---|---|---|
| Skeleton (the built-in avatar, `*.rig.toml` bodies) | The Animator's model has a root part bound to an avatar entity with an `AvatarRig` | Every bone in `AvatarRig.by_key` | The bone's `Transform` | The bind pose |
| Parts (`Motor6D`) | `Motor6D`s under the model | Each `Motor6D`, keyed by its `Part0` and `Part1` names | `JointPose { rotation, translation }` on a joint entity the runtime owns | Identity |
| Skinned MeshPart with `Bone`s | Later, with skinning in the mesh importer | | | |

- Skeleton targets keep today's id space,
  `AnimationTargetId::from_iter(["eustress_rig", key])`. Part joints use
  `["eustress_joint", part0_name, part1_name]`.
- Every Animator has a runtime entity holding its `AnimationPlayer` and graph
  handle, and each joint it animates carries `AnimatedBy` that entity.
- A skeleton binds at once, and the Animator takes its bones over once one of
  its tracks plays a ready clip, marking the avatar `AnimatorDriven`. Until
  then the avatar's own motion graph (`avatar/anim.rs`) keeps animating it, so
  an avatar never stands in its rest pose waiting for tracks or clips; after,
  the graph stops its player and never touches those bones again.
- A character model finds its rig through its root part's bound entity. Studio's
  `pull_character` already binds `HumanoidRootPart` to the local avatar; the
  Player binds each remote character's root part to its replica the same way.
- The rig is read from the tree on both shells (`Part0`, `Part1`, `C0`, `C1`,
  `Enabled` on each joint), and parts are reached with `entity_of`.

### Clips

- **glTF**: loaded by the asset server. On a skeleton, retargeted to canonical
  ids by `avatar/retarget.rs`, into a private copy per rig signature, with the
  clip frame's root correction applied to the skeleton root once at bind, as
  today.
- **KeyframeSequence** (record, clip file, or live instance): built into an
  `AnimationClip`. Each posed joint gets a rotation curve and a translation
  curve, each an `AnimatableCurve` over a curve type that evaluates Roblox's
  easing exactly per segment, with nothing baked. Skeleton clips use
  `animated_field!(Transform::rotation)` and its siblings, the fields glTF clips
  use, so the two kinds blend on one bone.
- Every clip covers rotation, translation and, on skeletons, scale for every
  joint it poses. A channel the source lacks is filled with the joint's rest
  value, so no property of a posed joint is left without a contributor.
- An Animator builds each clip once, the first time one of its tracks names it.
  A glTF clip starts looped at `Core`, since it carries neither.
- Once a clip is ready, its length, keyframe names and times, markers and joint
  coverage go to the track model. Until then `Length` reads 0.

### The graph: coverage classes

A joint's coverage is the set of playing tracks that animate it. Joints with the
same coverage form a class, and each class gets its own branch:

```text
root (Blend)
├── class 0 (Blend; masked to class 0's joints: the right arm)
│   ├── rest pose
│   ├── walk
│   └── wave
└── class 1 (Blend; masked to class 1's joints: everything else)
    ├── rest pose
    └── walk
```

- Each class has its own clip node per track, so every node has one parent and
  one mask.
- Every frame, for each class, the runtime applies the blending rule to the
  tracks in that class and writes each result into that node's
  `ActiveAnimation` weight. The rest pose takes what is left. Node weights stay
  1, so the graph asset is untouched between membership changes.
- Weights under 1e-6 are written as exactly 0. Bevy skips a zero-weight clip
  before it contributes, so the 0/0 case never arises, and the rest pose keeps
  every class non-empty.
- Every clip node is paused, and its time is set each frame from the track
  model with `set_seek_time`. Bevy neither advances time nor fires events; the
  track model is the one clock.
- The graph is rebuilt when a track starts or ends, or a clip finishes loading.
  Priority, weight, speed and time are per-frame values.
- A rig has at most 64 classes, Bevy's mask width; past that the smallest merge,
  and Output names the rig whose blend is then approximate. Rigs have 1 to 4 in
  practice.
- The rest pose writes every joint every frame, so a joint no track animates
  returns to rest, as Roblox's neutral pose, and a composed procedural delta
  can never accumulate on a bone no clip keys.

A chain of priority layers cannot do this. A chain gives each layer one weight
over the layers below, the same on every joint, so two same-priority tracks
animating different joints would be weighed by their sum everywhere, and a lower
layer fading out would pull the rest pose into the blend.

### Motor6D rigs

- **The root** is the Humanoid's `RootPart`, or the part of an
  AnimationController's model that is no joint's `Part1`. Physics owns it: the
  player's kinematic capsule, an NPC's dynamic body.
- **The forward pass** runs after the pose is evaluated: from the root outward,
  each `Part1` takes `Part0.CFrame * C0 * Transform * C1:Inverse()`.
- **Followers.** The `Part1` of an enabled `Motor6D` becomes a kinematic body
  that follows. Rigid joints hanging off a rig part (`Weld`, `WeldConstraint`, a
  tool's `RightGrip`, an accessory's weld) join the same pass, so a held tool
  and a hat follow the hand and the head exactly. This replaces the free hinge.
  Welds outside rigs keep today's `FixedJoint`.
- **Motion lane.** Parts moved by the pass are left out of it; every machine
  computes them from the root's pose and the tracks.

### The procedural layer

It sits in `AnimatorSet::Procedural` and `AnimatorSet::Ik`: after Bevy samples
the graph and the root motion policy runs, before script overrides and the
forward pass. The rule stays: procedural writes compose onto the animated value
and never assign it.

Its switches become instances in phase 5: `IKControl`, Roblox's class, served by
the solver in `avatar/ik.rs`; and two Eustress classes a character carries by
default, `FootPlacement` (planting and pelvis levelling) and `ProceduralLife`
(breathing, exertion, idle breaks, landing flex, look-at). A Space or a script
tunes or removes them like anything else:
`character.Humanoid.Animator.FootPlacement.Enabled = false`. Until then the
avatar runtime adds the layer for avatars as it does now.

### Removing the hard links

- `AnimatorSet` and the markers `PoseReady` and `AnimatorDriven` live in an
  ungated module. `PoseReady` goes on a rig once its Animator plays it, or once
  a bound rig has played nothing for 300 frames; any rig gets it after 600.
- `avatar/mod.rs` places `PostAnim` against `AnimatorSet`, and `spawn.rs`
  calibrates the feet once `PoseReady` is present or the motion graph is live.
  No ungated module names a gated one, so every feature combination compiles.
- The Player's agent loop reads the Animator's tracks from the tree (names,
  weights, times, speeds) once an Animator drives the avatar, and the motion
  graph before that.

### Cost

Bevy visits every graph node for every bone, so a frame costs bones times
nodes. A 65-bone avatar with 5 tracks in 2 classes has about 14 nodes: about
900 visits a frame, most of them one lookup that skips a masked or silent clip.
The phase 5 check measures 200 animated NPCs against the frame budget;
`PreferLodEnabled` rigs beyond a distance then evaluate at a lower rate and hold
their pose between.

## 3. The built-in avatar on the Animator

### The character in the tree

A character is a Model named after its player, holding `HumanoidRootPart`
(bound to the avatar body), `Head` and `Humanoid`. Studio's Play builds one for
the local player and one for every joined player (`play_datamodel`'s
`build_character`); a joined Player receives them from the host. The Player's
own binder, `animation::character::sync_local_character`, builds the local one
only when the Player plays a Space on its own. In a host's session it builds
nothing: it waits for the character the host replicates in and follows it for
its Humanoid's state only. A character the binder built gives way to any other
model that becomes the player's `Character`, so one avatar never has two
characters or two Animators. An Animator drives a skeleton only while its
character's `HumanoidRootPart` is bound to that avatar. Whoever builds it calls
`animation::character::furnish_character` before `CharacterAdded`:

1. The `Humanoid` gets an `Animator` child.
2. Everything in `StarterPlayer/StarterCharacterScripts` is cloned into the
   character; if nothing there is named `Animate`, the engine's default `Animate`
   is cloned in too. This is Roblox's rule.
3. `RunSpeed` and `StrideScale` are seeded on the Humanoid.

Each frame the builder reports the Humanoid's state from the avatar's movement
(see Humanoid state).

Scripts that arrive with a character start with it: on the host every `Script`
in any player's character, and on each machine the `LocalScript`s in its own
player's character only. A character is the nearest Model above a script that is
some player's `Character` or holds a `Humanoid`, so an NPC's LocalScripts run
nowhere. Both launch lists are rebuilt when the tree's shape changes, and a
destroyed script stops.

A Player that opens a Space locally draws it from its files and runs no tree
yet; the shared Play runtime reads it into one. Until then, and wherever no
Animator binds an avatar, the avatar's own motion graph animates it.

### The default Animate

A bundled template, `common/assets/characters/Animate/`: a LocalScript and four
Animation instances.

| Animation | AnimationId | `AuthoredSpeed` attribute |
|---|---|---|
| `idle` | `rig://idle` | |
| `walk` | `rig://walk` | 1.45 |
| `run` | `rig://run` | 3.9 |
| `jump` | `rig://jump` | |

`rig://` resolves through the avatar's rig, so the same script plays each body
option's clips and any Space's `*.rig.toml` replacements. A Space that wants
other behaviour copies `Animate` into `StarterCharacterScripts` and edits it.

```lua
-- Animate: the engine's default character animation. A Space replaces it by
-- putting its own script named Animate in StarterPlayer.StarterCharacterScripts.
local RunService = game:GetService("RunService")

local character = script.Parent
local humanoid = character:WaitForChild("Humanoid")
local animator = humanoid:WaitForChild("Animator")

local function load(name)
	local animation = script:WaitForChild(name)
	local track = animator:LoadAnimation(animation)
	track.Priority = Enum.AnimationPriority.Core
	track.Looped = true
	return track, animation:GetAttribute("AuthoredSpeed")
end

local idle = load("idle")
local walk, walkSpeed = load("walk")
local run, runSpeed = load("run")
local jump = load("jump")

-- Every track plays for the character's whole life and only its weight moves,
-- so a crossfade never starts from a track that is not playing.
idle:Play(0, 1)
walk:Play(0, 0)
run:Play(0, 0)
jump:Play(0, 0)

local speed, airborne, climbing = 0, false, false
humanoid.Running:Connect(function(s)
	speed = s
end)
humanoid.StateChanged:Connect(function(_, state)
	airborne = state == Enum.HumanoidStateType.Jumping or state == Enum.HumanoidStateType.Freefall
	climbing = state == Enum.HumanoidStateType.Climbing
end)

-- Changes are rounded to 1/32 so a steady gait sends nothing over the network.
local sent = {}
local function send(track, key, value, apply)
	value = math.floor(value * 32 + 0.5) / 32
	if sent[key] ~= value then
		sent[key] = value
		apply(track, value)
	end
end
local function weight(track, key, w)
	send(track, key, w, function(t, v) t:AdjustWeight(v, 0.1) end)
end
local function rate(track, key, r)
	send(track, key, r, function(t, v) t:AdjustSpeed(v) end)
end

RunService.Heartbeat:Connect(function()
	-- The gait follows the ground speed against the speed each clip was
	-- authored at, scaled by the body's stride, the way legs choose a gait: a
	-- Space's fast WalkSpeed plays the run, never a walk dragged across the
	-- ground. A clip without an authored speed falls back to the Humanoid's
	-- WalkSpeed and RunSpeed. Playback rate follows the same speeds.
	local stride = math.max(humanoid:GetAttribute("StrideScale") or 1, 0.25)
	local walkAt = math.max(if walkSpeed then walkSpeed * stride else humanoid.WalkSpeed, 0.05)
	local runAt = math.max(
		if runSpeed then runSpeed * stride else (humanoid:GetAttribute("RunSpeed") or walkAt * 2),
		walkAt + 0.05
	)
	local s = if climbing then 0 else speed

	local wi, ww, wr = 1, 0, 0
	if s >= 0.08 and s <= walkAt then
		ww = s / walkAt
		wi = 1 - ww
	elseif s > walkAt then
		wr = math.clamp((s - walkAt) / (runAt - walkAt), 0, 1)
		wi, ww = 0, 1 - wr
	end
	-- Climbing holds idle under the limb solve; the air takes the jump clip.
	local ground = if airborne and not climbing then 0 else 1

	weight(idle, "idle", wi * ground)
	weight(walk, "walk", ww * ground)
	weight(run, "run", wr * ground)
	weight(jump, "jump", 1 - ground)
	rate(walk, "walkRate", math.clamp(s / walkAt, 0.6, 1.6))
	rate(run, "runRate", math.clamp(s / runAt, 0.6, 2.5))
end)
```

It keeps what the motion graph got right: tracks that never stop, so a crossfade
cannot start from nothing; one priority for all four, so the blending rule
normalizes them the way the `ground` blend did; the gait blended by the
character's walk and run speeds; ground and air split by state; climbing held on
idle under the limb solve; playback matched to ground speed and stride.

### The avatar's own motion graph

`avatar/anim.rs` animates every avatar no Animator binds: an avatar with no
tree to animate from, a joined avatar until its owner's tracks arrive, and every
avatar under `EUSTRESS_LEGACY_MOTION=1`, the reference the Animator is compared
against. The Animator's runtime carries the hips pin (`AnimatorSet::RootMotion`)
and the liveness probe, for any rig.

The agent loop records the old graph's clip weights at 0, 1, 2.5 and 4 m/s,
airborne and climbing; the Animator must match each within 0.02, and its
liveness probe must report LIVE. Once every avatar animates from a tree,
`avatar/anim.rs`, `plugins/animation_plugin.rs`, `services/animation.rs` and
whatever only they reach go, once a grep for readers comes back empty, with
`engine/src/motor6d.rs`.

## 4. Scripting

### Luau

Everything in [Classes and properties](#1-classes-and-properties) is reachable
from Luau with Roblox's names.

- The methods live in `luau/play/instance.rs` and call the track model under the
  DataModel lock. `method_applies` and `is_event` know the animation classes.
- One event, `DmEvent::Signal { id, name, args }`, carries what happened: a
  track's `Stopped`, `Ended`, `DidLoop`, `KeyframeReached` and `Marker:<name>`;
  an Animator's `AnimationPlayed` (echoed on its Humanoid or
  AnimationController); a Humanoid's `StateChanged`, `Running`, `Jumping`,
  `FreeFalling` and `Climbing`. `dispatch_events` fires each as the signal of
  that name, for listeners only. `GetMarkerReachedSignal(name)` is the track's
  signal named `Marker:<name>`.
- Writes to `Priority`, `Looped` and `TimePosition` go to the track model.
  Read-only properties refuse writes with Roblox's message.

```lua
local player = game:GetService("Players").LocalPlayer
local character = player.Character or player.CharacterAdded:Wait()
local animator = character:WaitForChild("Humanoid"):WaitForChild("Animator")

local wave = Instance.new("Animation")
wave.AnimationId = "space://ReplicatedStorage/Animations/Wave"

local track = animator:LoadAnimation(wave)
track.Priority = Enum.AnimationPriority.Action
track:GetMarkerReachedSignal("Wave"):Connect(function(value)
	print("hand up", value)
end)
track:Play(0.2)
track.Ended:Wait()
```

### Rune

`eustress::dm`, with instances as `i64` ids, as the module already works:

| Function | Does |
|---|---|
| `load_animation(animator, animation) -> i64` | `Animator:LoadAnimation`. |
| `load_animation_id(animator, content_id) -> i64` | The same, from a content id, with no Animation instance. |
| `play(track, fade, weight, speed)`, `stop(track, fade)` | `Play`, `Stop`. |
| `adjust_speed(track, speed)`, `adjust_weight(track, weight, fade)` | `AdjustSpeed`, `AdjustWeight`. |
| `track_time(track)`, `set_track_time(track, t)`, `track_length(track)`, `is_playing(track)` | `TimePosition`, `Length`, `IsPlaying`. |
| `playing_tracks(animator) -> Vec<i64>` | `GetPlayingAnimationTracks`. |
| `animation_events() -> Vec<(i64, String, String, String)>` | This frame's track events, in the order they fired: the track, the kind (`stopped`, `ended`, `looped`, `keyframe`, `marker`), the keyframe's or marker's name, and the marker's value. |

`Priority` and `Looped` go through the existing `set_string` and `set_bool`.
Rune polls, because each `on_update` runs in a fresh VM, and destroys the
tracks it loads, which it names by id. A call that fails returns false or 0 and
says why in Output.

```rune
use eustress::dm;

// Plays the vault door's opening once, when its Open attribute turns on.
pub fn on_update(dt) {
    let door = dm::find_path("Workspace.Vault.Door");
    if dm::get_attribute_bool(door, "Open") && !dm::get_attribute_bool(door, "Opening") {
        let animator = dm::find_path("Workspace.Vault.Door.AnimationController.Animator");
        let track = dm::load_animation_id(animator, "space://ReplicatedStorage/Animations/DoorOpen");
        dm::play(track, 0.1, 1.0, 1.0);
        dm::set_attribute_bool(door, "Opening", true);
    }
}
```

### The Player

The Player's VM is `common::luau::play` running on `PlayerTree`, so every call
above works there unchanged. The Player adds `AnimatorPlugin` beside
`TreeApplyPlugin` and reads clips from the downloaded world's folder and its
bundled assets. Rune's functions work wherever Rune runs.

## 5. Multiplayer

Animation rides the world lane of [SERVER_AUTHORITY.md](../networking/SERVER_AUTHORITY.md).
Nothing about it uses the motion lane or the input lane.

### What crosses the wire

```rust
/// In `ReplOp`, ordered with the writes, sounds and remote events around it.
enum TrackOp {
    /// `Animator:LoadAnimation` made `track` on `animator`.
    Load { animator: NetId, track: NetId, animation: Option<NetId>, content: String },
    /// The whole control state after any change. Idempotent: the newest wins.
    Control { track: NetId, state: TrackControl },
    /// `Destroy()`, or the Animator went away.
    Unload { track: NetId },
}

struct TrackControl {
    playing: bool,
    looped: bool,
    priority: u8,
    /// Time `anchor_time` at tick `anchor_tick`, then `speed` seconds per second.
    anchor_tick: u64,
    anchor_time: f32,
    speed: f32,
    /// `WeightCurrent` goes from `fade_from` to `fade_to` over `fade_secs`.
    fade_start_tick: u64,
    fade_from: f32,
    fade_to: f32,
    fade_secs: f32,
}
```

- Every `Play`, `Stop`, `AdjustSpeed`, `AdjustWeight`, `TimePosition`, `Looped`
  and `Priority` change produces one `Control`, anchored at the tick of the frame
  that made it. A world frame keeps only the newest per track.
- `content` is the resolved content id, because a script can make an Animation
  locally and change its `AnimationId` before loading.
- Joint poses are never sent. Every machine evaluates each rig from the tracks.
- A track the session loads for another machine's script never logs a change,
  so nothing echoes back; a local script that stops it affects only this
  machine, as in Roblox, until that track's next change arrives.
- `Keyframe` and `Pose` instances made from a record take scene ids from the
  record's key plus their place in it, so a host's edit to a Pose reaches players
  like any other property write.

### Who may play what

- **The host** plays tracks on any Animator, and every player sees them.
- **A player** plays tracks on its own character's Animator, and they reach
  everyone through the host. The host checks each op first: the Animator is
  inside that player's current character; the content id resolves inside the
  published world (`space://`, `bundled://`, `rig://`, or a mapped Roblox id);
  speed is finite and within 100 either way, weight within 0 to 10, fades within
  0 to 60 s; at most 256 tracks per Animator; at most 30 ops a second per player,
  in bursts of 60, the remotes' token buckets.
- **A player on any other Animator** plays on its own machine only, as in
  Roblox. So does an Animator a LocalScript made.

A player mints the ids of tracks it loads, with no round trip, in a range no one
else mints: the predicted-spawn scheme of phase SA-4 (the player, the tick, a
counter).

### On every machine

- **Late join**: the host's snapshot carries `Load` and `Control` for every live
  track in scope. The joiner evaluates the same functions and lands at the same
  phase.
- **Interpolation**: a character drawn in the past is also evaluated in the
  past, at the interpolation clock, so its pose matches its position.
- **Events**: each machine fires `Stopped`, `Ended`, `DidLoop`,
  `KeyframeReached` and markers itself as its own clock crosses them, so a
  footstep sound tied to a marker plays in step with what that machine draws.
- **The host's reads**: a host script reading a swinging arm's pose sees the
  pose the host evaluated. Rewinding a hit (SA-3) evaluates the rig at a past
  tick from the ops it already holds, so animated joints need no pose history.

### Bandwidth

A `Control` op is about 50 bytes. The default Animate rounds its weights and
rates to 1/32, so a character moving at a steady speed sends nothing and a start
or stop sends a handful.

### LocalScripts inside characters

Roblox runs a LocalScript inside a character only on the machine of the player
who owns it. Eustress runs every LocalScript in `Workspace` on every machine
(Studio's `seed.rs`, the Player's `tree_scripts.rs`), which would run every
character's `Animate` everywhere and drive each rig twice. For scripts inside a
Model that holds a Humanoid, both launch lists take Roblox's rule: they run for
the owning player only, and not at all in an NPC. Other LocalScripts in
`Workspace` keep today's rule.

### What it needs first

Host-played tracks need phase SA-1's world lane, which is built and not yet
tested. Player-played tracks need phase SA-2's characters on the host, because
the host checks each op against the player's character there.

## 6. Imported games

### How Roblox stores animation

- An `Animation` holds an `AnimationId`. The asset behind it is a model file,
  rbxm or rbxmx, whose root is a `KeyframeSequence`, or a `CurveAnimation` for
  newer uploads.
- Scripts name further ids in their source: the R15 `Animate` lists its
  defaults, and games make Animations at run time.
- `KeyframeSequence`s also live inside places: `ServerStorage/RBX_ANIMSAVES`,
  or wherever a game keeps them.

### What the importer writes

1. **Fetch every animation id**: from `AnimationId` properties, and from asset
   ids in script sources (`rbxassetid://N`; `roblox.com/asset/?id=N` and
   `Asset?ID=N` in any case), with the credential the importer already uses for
   meshes. Content sniffing tells animations from sounds and images.
2. **Convert each clip** to `assets/animations/rbx-<id>.anim.toml`: each
   `CFrame` to a position and a quaternion; positions scaled by the stud factor
   the rig's parts and joints use, so a clip and its rig agree; names, easing,
   markers and number poses kept. A `CurveAnimation` is resampled at 60 Hz into
   linear keyframes and reported as an approximation. Top-level poses
   (`HumanoidRootPart` in R15 clips, `Torso` in many R6 ones) are left out: in
   Roblox a pose drives the joint between its parent pose's part and its own,
   so a top-level pose drives nothing, while a record pose without `parent`
   binds by name alone. Their children still name them as `parent`.
3. **Write the id map**, `assets/roblox_ids.toml`, naming the file for every
   fetched id. The Animator resolves any Roblox id a script names at run time
   through it. Sounds and images can use the same map.
4. **Write the Animation's key**: `[properties] animation_id`, the id exactly as
   Roblox has it. Both readers carry it into the tree through one function,
   `record_animation_id`, and a Roblox id resolves through the map.
5. **Import the place's own sequences** as `KeyframeSequence` records with their
   keyframes inline.
6. **Stop folding a value object that has children.** A folded object's children
   are lost; that is how `RBX_ANIMSAVES` and the pedestrians' `Animate` configs
   emptied.
7. **Carry joints into the tree**: `Motor6D` and `Weld` records hold `Part0` and
   `Part1` by UUID, `C0`, `C1` and `Enabled`, and both Studio's seed and the
   Player's reader (`datamodel/record.rs`) put them in the tree.
8. **Report** animations fetched, converted and approximated, and ids left
   unresolved.

With a credential, Vehicle Simulator's pedestrians walk and sit with their own
clips: `PedestrianServer` plays walk on the host, `LocalNPCManagement` finds
`Animate.walk.WalkAnim`, and the taxi's driver and passenger sit.

### The player's own body

A player in an imported game is their Eustress avatar. The game's scripts expect
an R15 character, so two things bridge the gap:

- **R15 clips retarget onto the avatar.** A table maps each R15 joint to an
  avatar bone: `Root` to the hips, `Waist` spread over the three spine bones,
  `Neck` to the neck, each shoulder, elbow and wrist to the arm, forearm and
  hand, each hip, knee and ankle to the up-leg, leg and foot (R6 clips map
  `Torso`, the four limbs and the head the same way). A joint's rotation is
  expressed in the character's frame and applied to its bone over the rotation
  between R15's rest pose (arms down) and the avatar's bind pose. The `Root`
  translation scales by the ratio of hip heights. The clip builder does this
  once per clip and rig, so a retargeted clip blends like any other.
- **R15 part names answer through stand-ins.** The character model holds
  invisible, massless, non-colliding parts named as R15 names them
  (`LowerTorso`, `UpperTorso`, `LeftUpperArm`, `RightHand`, `LeftFoot` and the
  rest), each following its avatar bone, with R15's attachments
  (`RightGripAttachment`, `HatAttachment` and the others). A tool's grip weld
  holds it in the avatar's hand and an accessory sits on the avatar's head. A
  ragdoll script finds no `Motor6D`s to replace, and the avatar keeps its own
  physics.

## Phases

| Phase | Delivers | Playable at the end | Status |
|---|---|---|---|
| 1. Tracks on the avatar | The track model with its tests; `AnimatorPlugin` for skeletons (bind, glTF clips, coverage-class graph, rest pose); the ungated `AnimatorSet` and `PoseReady`; Luau and Rune APIs for `Animator`, `AnimationTrack`, and the `Humanoid` and `AnimationController` proxies; Humanoid state for the player's avatar; `Humanoid` > `Animator` in every character; StarterCharacterScripts copied into characters, and scripts that arrive after start run; the default `Animate` with `rig://` ids; the equivalence check against the old motion graph, which stays as the fallback until every avatar animates from a tree | The avatar walks, runs, jumps and climbs through `Animate`, matching today's weights. A script plays any glTF clip on it: a wave over walking that fades out and fires `Stopped` and `Ended`. A Space edits its own `Animate`. | Built and type-checked lock-free; not yet built or run. The equivalence check is next. |
| 2. Authored clips and part rigs | The clip record and its loader; the Roblox easing curve; `Keyframe`, `Pose`, `NumberPose` and `KeyframeMarker` in the tree with their API; `KeyframeSequenceProvider`; markers and `KeyframeReached`; `Motor6D` rigs (forward pass, followers, welds hanging off rigs, `Transform` overrides, the legacy angle); `AnimationController`; `rig://` names beyond four slots in `*.rig.toml`; class templates for `Animation` and `AnimationController`, and `KeyframeSequence` and `Motor6D` corrected; joints in the tree | A clip written by hand, by a script or by an agent plays on the avatar or on any `Motor6D` model: a door swings, a robot arm reaches, an R15 NPC stands in its pose where it collapses today. Markers time footstep sounds. | Partly built, type-checked, not run: the record and its loader, the easing curve, the tree API, `KeyframeSequenceProvider`, markers, the `Motor6D` forward pass, `AnimationController`, the class knowledge. Not built: followers, welds off rigs, `Transform` overrides, the legacy angle, `*.rig.toml` names, class templates. |
| 3. Imported games | The importer's eight changes; NPC Humanoid state from `npc.rs`; the launch rule for scripts inside characters; the R15 `Animate`'s globals answer (`UserSettings()` and the rest); R15 clips retargeted onto the avatar and R15 stand-in parts | Vehicle Simulator's pedestrians walk and sit with their Roblox clips after a credentialed re-import, and the player's avatar plays the game's own animations. `RBX_ANIMSAVES` opens as KeyframeSequences. | Partly built: the launch rule for scripts inside characters. With Roblox Place Import and the WASM session: the importer changes and `record_animation_id`. Not built: NPC state, the R15 globals, R15 retargeting and stand-ins. |
| 4. Multiplayer | `TrackOp` in the world lane; player ops to the host with their checks and rates; player-minted track ids; late join; followers off the motion lane; `AnimatorPlugin` on the Player; remote characters' root parts bound to their replicas | Two windows: each sees the other's avatar walk, run and jump in phase; a player's emote reaches everyone; NPCs move alike on every machine; a late joiner sees tracks mid-play at the right time. | Specified to the Multiplayer session (`TrackOp`, clocks, late join, checks). `AnimatorPlugin` is on the Player. |
| 5. Procedural classes, preview and scale | `IKControl`, `FootPlacement` and `ProceduralLife`; `Animator:StepAnimations` and a preview in Studio without Play; evaluation LOD; number poses to `FaceControls`; kinematic `AnimationConstraint` | A script turns foot planting off or points a head with an `IKControl`. Studio previews a clip on a rig without Play. 200 animated NPCs fit the frame budget. | Designed |

After these: an animation editor on the Timeline panel, showing a sequence's
keyframes as its tracks, and property channels in the clip record, so one clip
format serves both joints and the Timeline's property tracks.

## Verification

Each of these can fail, which is what makes it a test:

- **The blending rule**: four same-priority tracks at 0.5 give 0.25 each; an
  arm-only `Action` track at 1 leaves the legs on the walk; `Action` at 0.3 over
  `Movement` at 1 gives 0.3 and 0.7; no tracks gives the rest pose.
- **No NaN**: a rig whose tracks fade to exactly 0 in the same frame keeps
  finite transforms on every bone.
- **The clock**: `Play`, `Stop`, fades, loops, the end of a non-looped clip,
  negative speed and seeks produce Roblox's event order. Two DataModels given the
  same ops hold equal `TimePosition` and `WeightCurrent` at every tick, and fail
  with one op dropped.
- **Clips**: the four cached Roblox assets convert, with their keyframe times,
  pose counts and names; the R6 clip keeps its `Handle` pose; easing reproduces
  Roblox's `In` and `Out` reversal.
- **Checked against Roblox**, from a recording made in Roblox Studio: the fade
  at the natural end of a non-looped clip, `Play` on a playing track, `Cubic`
  against `CubicV2`, how `Constant` treats `EasingDirection`, how `CurrentAngle`
  composes with `Transform`, and whether a written `Transform` stays on a joint
  no track animates.
- **The avatar**: the liveness probe reports LIVE; the shipped clips still map
  onto the humanoid rig; clip weights match the old graph within 0.02 at the
  recorded speeds and states.
- **Part rigs**: an R15 NPC in Play holds its authored pose (every limb within
  1 mm of `Part0 * C0 * C1:Inverse()`) and plays a converted wave; a tool welded
  to its hand follows.
- **Multiplayer**: a late joiner's tracks equal an early joiner's; an op on
  another player's Animator, over the rate, or naming a clip outside the world
  never reaches a tree.
- **The importer**: after a Vehicle Simulator re-import, 7 Animations resolve,
  `RBX_ANIMSAVES` holds KeyframeSequences, `Animate.walk.WalkAnim` exists on every
  pedestrian, and every script literal id is in the map or in the report.

## Files and owners

Several sessions own files this touches; the orchestrator schedules each window.

| Area | Files | Owner today |
|---|---|---|
| Track model | `common/src/datamodel/animation.rs` (new), `datamodel/mod.rs` | This work |
| Evaluator | `common/src/animation/` (new); `avatar/{anim,mod,spawn}.rs` for the handover; `avatar/anim.rs`, `plugins/animation_plugin.rs` and `services/animation.rs` go once every avatar animates from a tree | This work; the avatar files with the Client session |
| Default Animate | `common/assets/characters/Animate/` (new), class templates in `common/assets/class_schema/` | This work |
| Luau | `luau/play/instance.rs`, `luau/play/mod.rs`, `prelude.luau` | The Luau runtime session (`prelude.luau`, `luau/play/mod.rs`) |
| Player VM and drawing | `tree_scripts.rs`, `tree_apply.rs` | The Luau runtime session; the Client session |
| Reader parity and launch lists | `datamodel/record.rs`, `tree_read.rs`, Studio's `play_datamodel/seed.rs` | The Eustress WASM session (`record.rs`, `tree_read.rs`, `seed.rs`'s parity module); Roblox Place Import (the rest of `seed.rs`) |
| Play glue | `play_datamodel/{pull,npc}.rs`, `soul/rune_datamodel.rs` | This work |
| Joints | `physics/joint_resolver.rs`, `spawners/constraints/motor6d.rs`; delete `motor6d.rs` | This work |
| Replication | `eustress-networking/src/repl/`, `wire.rs`, `engine/src/net_replicate.rs`, `client/src/systems/net_replica.rs` | The Multiplayer session |
| Importer | `roblox-import/` (fetch, convert, map, fold, joints) | Roblox Place Import |
| Agent observation | `client/src/systems/agent_control.rs` | The Client session |
