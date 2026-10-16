# Physics constraints from scripts

During Play, a Luau or Rune script can build a machine out of parts: put attachments on
parts, join them with constraints, and tune those constraints every frame. The engine
simulates what the script built. Hinges turn, sliders slide, springs push back, motors
drive and servos steer, with the same classes and property names as Roblox, so a
script written for a Roblox constraint rig reads the same here.

A car, a door on a hinge, an elevator on a slider, a crane, a suspension bridge and a
ragdoll are all scripts on top of these classes. The engine carries no game's logic; it
carries the classes.

## What a script does

```rune
use eustress::dm;
use eustress::Vector3;

// A wheel that spins on a motor about the kart's left axis (the kart faces -Z).
pub fn on_update(dt) {
    let kart = dm::find_path("Workspace.Kart");
    let axle = dm::find_child(kart, "RearAxle");
    if axle == 0 {
        let body = dm::find_child(kart, "Body");
        let wheel = dm::find_child(kart, "RearWheel");
        let left = Vector3::new(-1.0, 0.0, 0.0);
        let a0 = dm::create("Attachment");
        dm::set_parent(a0, body);
        dm::set_vector3(a0, "WorldPosition", dm::get_position(wheel));
        dm::set_vector3(a0, "WorldAxis", left);
        let a1 = dm::create("Attachment");
        dm::set_parent(a1, wheel);
        dm::set_vector3(a1, "WorldPosition", dm::get_position(wheel));
        dm::set_vector3(a1, "WorldAxis", left);
        axle = dm::create("HingeConstraint");
        dm::set_string(axle, "Name", "RearAxle");
        dm::set_instance(axle, "Attachment0", a0);
        dm::set_instance(axle, "Attachment1", a1);
        dm::set_string(axle, "ActuatorType", "Motor");
        dm::set_parent(axle, kart);
    }
    dm::set_number(axle, "AngularVelocity", 20.0);  // rad/s
    dm::set_number(axle, "MotorMaxTorque", 400.0);  // N·m
}
```

Luau writes the same thing with `Instance.new("HingeConstraint")` and
`hinge.Attachment0 = a0`.

## Classes

Lengths are meters, forces newtons, torques newton-meters. Angles that Roblox gives in
degrees stay in degrees (`TargetAngle`, `LowerAngle`, `UpperAngle`, `Orientation`);
angular speeds are radians per second.

### Attachment

A frame on a part. A constraint joins two attachments, and its axes come from their
frames.

| Property | Meaning |
|---|---|
| `CFrame`, `Position`, `Orientation` | The frame relative to the parent part's pose |
| `Axis`, `SecondaryAxis` | The frame's X and Y axes, relative to the part |
| `WorldCFrame`, `WorldPosition`, `WorldAxis`, `WorldSecondaryAxis` | The same, in the world; writable |

The part's pose is its position and rotation, never its size: an attachment 0.5 m above
a part's centre stays 0.5 m above it whatever the part's size.

### Constraints

Every constraint has `Enabled` (a disabled constraint does nothing), `Visible`, and the
two ends it joins, `Attachment0` and `Attachment1`. A constraint whose ends are not both
on parts, or whose two parts belong to one rigid assembly, does nothing until that
changes.

| Class | What it does | Properties |
|---|---|---|
| `WeldConstraint` | Holds `Part0` and `Part1` rigidly where they are | `Part0`, `Part1` |
| `Weld`, `ManualWeld` | Holds `Part1` at `Part0 * C0 * C1:Inverse()` | `Part0`, `Part1`, `C0`, `C1` |
| `HingeConstraint` | Rotation about `Attachment0`'s X axis only | `ActuatorType` (`None`, `Motor`, `Servo`); Motor: `AngularVelocity`, `MotorMaxTorque`, `MotorMaxAcceleration`; Servo: `TargetAngle`, `AngularSpeed`, `ServoMaxTorque`; `LimitsEnabled`, `LowerAngle`, `UpperAngle`; read-only `CurrentAngle` |
| `PrismaticConstraint` | Sliding along `Attachment0`'s X axis only, no rotation | `ActuatorType`; Motor: `Velocity`, `MotorMaxForce`; Servo: `TargetPosition`, `Speed`, `ServoMaxForce`; `LimitsEnabled`, `LowerLimit`, `UpperLimit`; read-only `CurrentPosition` |
| `CylindricalConstraint` | Sliding along and rotating about `Attachment0`'s X axis | the prismatic's linear properties, plus `AngularActuatorType`, `AngularVelocity`, `MotorMaxTorque`, `TargetAngle`, `AngularSpeed`, `ServoMaxTorque`, `AngularLimitsEnabled`, `LowerAngle`, `UpperAngle` |
| `SpringConstraint` | A spring and damper between the two attachment points | `FreeLength`, `Stiffness` (N/m), `Damping` (N·s/m), `LimitsEnabled`, `MinLength`, `MaxLength`; read-only `CurrentLength` |
| `NoCollisionConstraint` | `Part0` and `Part1` pass through each other | `Part0`, `Part1` |
| `VectorForce` | A steady force on `Attachment0`'s part | `Force`, `RelativeTo` (`World`, `Attachment0`, `Attachment1`), `ApplyAtCenterOfMass` |

A Motor turns (or pushes) toward its target speed with at most its maximum torque (or
force): with a large gap it applies the maximum, and it holds the target once there. A
maximum of 0 applies nothing. A Servo moves toward its target angle (or position) at no
more than its speed, with at most its maximum torque (or force). Limits stop motion past
them.

### Every write travels

A property write from a script is an ordinary DataModel write: it reaches the engine the
same frame and, in a multiplayer session, every player's machine as a reliable update.
Writing the value a property already holds costs nothing (the DataModel skips it), so a
script that sets a motor speed or a servo angle every frame should round the value to
the step it needs; the rounded value then changes a few times a second instead of every
frame.

### Scripts reading and writing references

`Attachment0`, `Attachment1`, `Part0` and `Part1` hold instances. Luau assigns them
directly. Rune uses:

- `dm::set_instance(i, prop, other)` sets an instance-valued property (`0` clears it);
- `dm::get_instance(i, prop)` reads one (`0` when empty). It also reads `PrimaryPart`,
  `Occupant`, `Adornee` and `Value` on an `ObjectValue`.

## How the engine simulates them

Each constraint becomes a joint between the rigid bodies its two parts belong to. The
joint's frames are the attachments' frames expressed in those bodies. It is rebuilt when
either attachment moves or changes parent, when a reference changes, or when welding
changes which body a part belongs to. Every property write reaches the joint the same
frame. Destroying a constraint, or setting `Enabled = false`, removes its joint.

On Avian 0.7:

| Class | Joint |
|---|---|
| `WeldConstraint` | the weld group as one body, holding the pose the two parts are in when it binds; a `FixedJoint` in a group with an anchored part (see Weld groups) |
| `Weld`, `ManualWeld` | the same, holding `Part0 * C0 == Part1 * C1` (see Weld groups) |
| `HingeConstraint` | `RevoluteJoint`, hinge axis the attachment frame's X |
| `PrismaticConstraint` | `PrismaticJoint`, slider axis the attachment frame's X |
| `CylindricalConstraint` | a `PrismaticJoint` to a physics-only body with no instance, and a `RevoluteJoint` from it (Avian has no cylindrical joint) |
| `SpringConstraint` | its own XPBD constraint, solved every substep with compliance `1 / Stiffness` and the damping term, registered through `solve_xpbd_joint` in the `SubstepSchedule` |
| `NoCollisionConstraint` | the pair's contacts filtered out (a collision hook, or a joint marked `JointCollisionDisabled` between the two bodies) |
| `VectorForce` | a force applied to the body every step, at its centre of mass or at the attachment point, in world or attachment axes |

Actuators map to the joint's motor:

- **Motor**: an `AngularMotor` (or `LinearMotor`) with `MotorModel::AccelerationBased {
  stiffness: 0, damping: 1 }`, `target_velocity` from the constraint and `max_torque` (or
  `max_force`) from its maximum. With a damping of 1, Avian's motor reaches the target
  speed within one substep when the cap allows, so it neither overshoots nor chatters.
- **Servo**: the motor's `target_position` moves toward the target at the constraint's
  speed, one step at a time, with the cap from its maximum.

Things about Avian 0.7 that the mapping has to respect:

- A motor's `max_torque` or `max_force` of 0 means no cap at all. A constraint maximum of
  0 disables the motor instead.
- `MotorModel::ForceBased` and the damping term of `AccelerationBased` leave out a factor
  of the substep time; `SpringDamper` is the one physically dimensioned model.
- The prismatic joint's motor damps the difference in centre-of-mass velocity only, so it
  cannot stand in for a spring: rotation of the bodies goes undamped.
- A spring applied as a force once per frame is unstable on light bodies; it has to be
  solved every substep, as above.
- A rigid body nested under a part that is not a body gets its Transform written relative
  to that part's pose before the step; the engine rewrites such Transforms from the
  nearest ancestor body after `PositionToTransform`.
- Avian skips contacts between two bodies joined by any joint that carries
  `JointCollisionDisabled`. Welds carry it, so the two parts a weld joins never touch each
  other, as in Roblox. The other constraint joints leave it off, since two parts
  joined by a hinge still touch; a `NoCollisionConstraint` is what turns a pair's contacts
  off. The physics-only body of a `CylindricalConstraint` has no collider.
- Stock 0.7.0 estimates a contact's separation within a step by turning each contact point
  with its body, so a fast-spinning round body (a wheel at speed) reads as lifted off the
  ground after its first substep and loses its normal force, and with it its grip, for the
  rest of the step: a car topped out at 45 m/s with its driven wheels slipping 23 percent,
  and its wheels sank into the road. The workspace's copy (`.patches/avian3d-0.7.0`) counts
  a contact point's turn only where it takes the contact deeper.
- Stock 0.7.0's gyroscopic step keeps a spin's angular momentum but not its energy, so a
  free wheel spun about its axle tumbles off it with more energy than it had. The
  workspace's copy takes the implicit midpoint rule, which keeps both.
- Parry meets a cuboid and a cylinder, capsule or convex hull (a wedge) with GJK, whose
  normals are noise when the cuboid is kilometres across and the gap is sub-millimetre: a
  wheel resting on a 4 km baseplate was thrown into the air, and a wedge slid across it
  veered aside. The workspace's copy meets a large cuboid's face as a half-space. Its
  `PATCH.md` has the details of all three.
- A joint between two bodies neither of which simulates (two anchored parts) panics
  linking it into the solver's islands; see Constraints between anchored parts.

### Weld groups

Bodies welded together form a group, and a group of dynamic bodies is ONE rigid body, as a
Roblox assembly is. The heaviest body of the group is its hub, judged by each part's shape
and authored density rather than the solver's current mass (a part made dynamic a moment
ago can read light). The hub keeps its body; every other part keeps its own entity and
collider, attached to the hub's body at the part's place in the hub's frame, and its own
body is switched off (`play-runtime/src/assemblies.rs`). So the hub weighs, turns and
collides as the whole group, its parts never collide with each other, and nothing inside
it can bend or drift, whatever shape the script's welds had: a script that welds every part
to a 30 kg seat gets the same body as one that welds them to the chassis. A touch or a
raycast on a part still names that part, and every part reads the group's `AssemblyMass`.

A merged part follows its hub: after every physics step it stands where the hub holds it
and moves as the hub does, and it is drawn where the hub is drawn (eased between steps). A
script's `CFrame` write to any part moves the whole group, as in Roblox; its
`AssemblyLinearVelocity`, `AssemblyAngularVelocity` and `ApplyImpulse` act on the group's
body, and anything else that pushes a part (a mover, a `VectorForce`, buoyancy) is moved
onto the hub at the part's centre of mass each step. A hinge, prismatic, cylindrical or
spring constraint whose attachment is on a part acts on the hub at the attachment's place in
the world. A constraint between two parts of one group builds no joint, since they cannot
move against each other. A `NoCollisionConstraint` on a merged part keeps the whole group
from colliding with the other part.

The Space's own welds join the same groups: a loaded weld between two parts of a merged
group is switched off while the group stands, and any other loaded joint on a merged part
(a hinge, a `Motor6D`) is carried by the hub at the same place. All of it is undone when a
part leaves its group and when the session ends, so Edit finds every part its own body. A
part that fractures leaves its group, and the rest re-form around the heaviest part left.

A group with a static, kinematic or character body in it (an anchored part, an avatar)
keeps one `FixedJoint` per weld, since a static anchor is well conditioned. Groups are made
again when a weld changes or one of their bodies is anchored or released, and the
constraints that hang from their parts are built again with them. A plan that comes out the
same (a rider's `SeatWeld` coming or going, a part spawned elsewhere) changes nothing. A
`WeldConstraint` holds the offset it was made with, taken when both its parts were first
drawn: its `Part0`, `Part1` or `Enabled` changing takes a new one, as in Roblox, so a group
that re-forms never welds its parts where they have drifted to. A part that joins a group
goes to the place its welds give it at once, as a Roblox weld snaps.

Avian turns every joint correction into velocity, so joints between bodies of very
different mass do not stay rigid. A 30 kg seat welded to 700 kg of chassis, with the springs
on the seat, drifted 18 cm at rest and threw a bumper off at 89 m/s. A star of joints from
the heaviest body, each at its part's centre of mass, held to millimetres while the hub
outweighed the rest: a 390 kg deck carrying 840 kg of parts drifted 33 mm into a wall. One
body drifts none, and the solver has one body instead of a body and a joint per part.

Constraints loaded from the Space work at their attachments too: each end's frame is the
Attachment's `CFrame` on its part's pose, expressed in the body the part moves with, so an
imported door's hinge turns about the hinge's own axis at the hinge's own point. An
attachment loaded without its frame (an older loader) stands in with its own pose.

Welds loaded from the Space bind the same way. Roblox's `ManualWeld`, `Snap` and `Glue`
import as `Weld`, with their `C0` and `C1`, so a legacy-welded assembly holds together in
Play. Where the loaded parts sit apart from what `C0` and `C1` say, the weld pulls them
together when physics starts, as Roblox's does; the first eight such welds are named in a
warning, since that usually means a frame came in wrong.

### Constraints between anchored parts

A constraint none of whose parts simulates (two anchored parts, a welded model that is
still anchored) builds no joint until one of them is released, and then builds from the
same attachments and offsets. Avian 0.7 links every joint into the simulation islands of
its two bodies, only a dynamic or kinematic body has one, and a joint between two bodies
that have none panics the solver. A Roblox constraint between anchored parts does nothing
until a part moves either, so a script can weld, hinge and spring a whole vehicle while it
is anchored and release it at once: it starts assembled where it was built, with nothing
falling while the joints are made. Constraints loaded from the Space wait the same way
(`AwaitingSimulatedBody` in `joint_resolver.rs`).

## Seats

A vehicle also needs seats to behave as they do in Roblox:

- `Seat` and `VehicleSeat` load their settings from their files (`[seat]`, `[vehicle]`).
- A character sits by touching an enabled seat, or when a script calls
  `seat:Sit(humanoid)`. `Humanoid.Sit`, `Humanoid.SeatPart`, `Seat.Occupant` and the
  `Seated` event report it. The seated character stays with the seat as it moves.
  Jumping leaves the seat, and so does a script setting `Humanoid.Sit = false`; a script
  may turn jumping off (`Humanoid.JumpEnabled = false`) to keep a driver in.
- The `SeatWeld` a seat makes is a record for scripts. No weld between a seat and a
  character becomes a joint: the avatar is a kinematic character, and a joint would drag
  the vehicle by its controller. Riding carries the character. The character is known
  from the tree (a `HumanoidRootPart` in a Model holding a Humanoid) as well as from its
  avatar, so a Player that receives the weld before the character's avatar exists skips
  it too. A weld of anything else to a character, such as a hat, is an ordinary joint.
- The occupant's movement keys set the VehicleSeat's `Throttle` and `Steer` (-1, 0, 1),
  and `ThrottleFloat` and `SteerFloat` to the same values, for the local player and for
  remote players alike: `W` or `Up` and `S` or `Down` for `Throttle`, `D` or `Right` and
  `A` or `Left` for `Steer`, a held pair cancelling. There is no smoothing; scripts shape
  the response and turn it into motion. The seat finds its driver through the Player
  whose character's Humanoid has it as `SeatPart` and is its `Occupant`; a remote
  player's keys are the host's `DataModel::player_input` for that Player (Roblox KeyCode
  names, refreshed every frame), the host's own player's are local input.
- These are ordinary writes, so they replicate and a driver's own HUD reads the same
  `Throttle` the host's scripts do. They happen only when the keys change the value, and
  once as zero when the driver leaves, so a script's own write (an AI driver, a cutscene)
  holds until the driver's keys next change it. They run on the host, before the frame's
  scripts, so scripts read this frame's keys.
- A remote player riding a seat needs that player's character in the host's DataModel and
  a sit and leave handshake the host decides; that is the multiplayer server-authority
  work, and the seat simulation above is unchanged by it.

## What scripts see

Every moving part's `CFrame`, `Position`, `AssemblyLinearVelocity` and
`AssemblyAngularVelocity` are current each frame, including parts moved by their
assembly's body rather than their own. `AssemblyMass` is readable. Writing
`AssemblyLinearVelocity` or `AssemblyAngularVelocity` on any part of an assembly sets
the velocity of the assembly's body, as in Roblox. The engine's own pose updates are
engine-side writes, which never count as script writes: moving parts reach other players
over the motion lane, never as reliable property updates.

## Parity

Constraints and attachments that scripts create go through the same record and seed path
as instances loaded from the Space, so Studio, the Player and the headless host build the
same joints and keep the same tree. Stop removes everything a script made.

## Verifying

- A hinge Motor with `AngularVelocity = 10` spins a free wheel up to 10 rad/s and holds
  it; with `MotorMaxTorque` too small for the load it turns slower, never faster.
- A hinge Servo reaches `TargetAngle` at `AngularSpeed` and holds against a push up to
  `ServoMaxTorque`.
- A 100 kg block hanging from a `SpringConstraint` with `Stiffness = 9810` settles 0.1 m
  past `FreeLength`; with `Damping` at `2 * sqrt(Stiffness * 100)` it settles without
  bouncing.
- A `PrismaticConstraint` with limits stops the slider at each limit.
- Two parts joined by a `NoCollisionConstraint` pass through each other; each still
  collides with everything else.
- A car whose parts are all welded to a 30 kg seat, with the suspension hung on the seat,
  is one 1260 kg body on its spine, settles on its springs and drives to 15 m/s in five
  seconds.
- Two welded parts are one body weighing both; anchoring one splits them, and the weld
  becomes a joint to the anchored part. Destroying the heavier leaves the other standing.
- A loaded weld's pair is one body in Play and a joint again after Stop; a loaded hinge on
  a merged part turns at the same place, on the hub.
- Two anchored parts welded or hinged build no joint; releasing one builds it, and the
  part holds where it was welded, or swings on the hinge.
- A model welded while anchored falls as one rigid piece once released.
- A ball rolling at 30 m/s stays on the road (its centre never below its radius) and keeps
  its speed; a ball spun on the spot grips and turns its spin into speed without sinking.
- A cylinder wheel rolling at 30 m/s across a 4 km baseplate neither sinks nor hops, and a
  wedge slid across it stops dead ahead.
- A wheel spun about its axle in the air keeps spinning about the axle.
- Everything a script built shows in the Explorer during Play and is gone after Stop.

## Status

In the source, not yet in a build: the DataModel classes above with their Roblox
defaults, the Attachment frame properties, and Rune's `get_instance` and `set_instance`.

Written and type-checked, landing in the next engine window: joint building for the
constraints scripts make (`play-runtime/src/joints.rs`, which reads the tree and the
parts' poses itself), loaded `Weld` binding and the avatar-weld rule
(`engine/src/physics/joint_resolver.rs`), the `Seat` and `VehicleSeat` defaults, and
`VehicleSeat` input (`engine/src/play_datamodel/vehicle_seat_input.rs`, after the seat
system in `docs/networking/SEATS.md`). None of it has run yet.

Not written yet: poses for parts nested under another part's body (the pull reads parts
whose own Transform changed, and a nested part's does not as its body moves).
