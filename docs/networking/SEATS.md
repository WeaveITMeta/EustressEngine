# Seats

How a character sits in a `Seat` or `VehicleSeat` and gets out, on the host
and on every Player, for the host's own character and every joined player's
alike. Part of server authority phase 2a
([SERVER_AUTHORITY.md](SERVER_AUTHORITY.md), "Characters and seats").

Status: written 2026-09-24 and type-checked on every side, not yet built or
played: the host's `engine/src/play_datamodel/seats.rs`, the avatar's
`common/src/avatar/seat.rs`, the Player's `client/src/systems/net_replica.rs`,
`Seat:Sit` and `Humanoid.Seated` in Luau, and joint support's skip.

## Who does what

- **The host decides.** Who sits, in which seat, and when they leave are the
  host's writes to its own tree. They replicate like any other write, so
  scripts on every machine see the same `Occupant`, `Sit`, `SeatPart` and
  `SeatWeld`.
- **The machine that moves an avatar makes it ride.** The host moves its own
  avatar and the replicas it keeps of joined players; each Player moves its
  own avatar and its replicas of everyone else. Each one attaches the avatar
  to the seat as it draws the seat.
- **The seat's simulation reads its occupant's input.** A `VehicleSeat` takes
  `Throttle` and `Steer` from the occupant's keys, on the host.

A player never seats itself. The host acts on the touch it sees.

## Sitting (the host)

A host-only system in `engine/src/play_datamodel/seats.rs` runs in the Pull
set after the collisions are pulled, and reads the frame's
`Touched { part, other }` events.

A seat takes a character when all of these hold:

- `part` is a `Seat` or `VehicleSeat` with `Disabled` false and no `Occupant`;
- `other` is a `HumanoidRootPart` whose Model holds a `Humanoid` (a player's
  character or an NPC);
- that Humanoid is alive, has `Sit` false, and did not leave a seat in the
  last second.

`Seat:Sit(humanoid)` from a script asks the same thing without the touch.

Sitting is one frame of ordinary writes, so they replicate together:

| Write | Value |
|---|---|
| `seat.Occupant` | the Humanoid |
| `humanoid.Sit` | `true` |
| `humanoid.SeatPart` | the seat |
| a `Weld` named `SeatWeld`, child of the seat | `Part0` the seat, `Part1` the root, `C0` the seat offset below |

and `humanoid.Seated` fires with `(true, seat)`.

**The seat offset** puts the root on the seat's top face, facing the seat's
front: `C0 = CFrame(0, seat.Size.Y / 2 + seated_root_height, 0)` in the
seat's frame, where `seated_root_height` comes from the avatar's metrics (the
root's height above the surface it sits on). The same number places the
avatar when it rides, so the weld and the drawn avatar agree.

**The `SeatWeld` is a record, not a physics joint.** An avatar is a kinematic
character, so a rigid joint between the seat's body and the avatar would drag
the vehicle by the character controller. Joint support skips a weld between
a seat and a rider: a `HumanoidRootPart` in a Model holding a `Humanoid`,
known from the tree, or a part bound to an avatar. It stays skipped on every
retry, so a Player that binds the root after the weld arrives never turns it
into a joint. Anything else welded to a character (a hat, a backpack) is an
ordinary joint. Riding (below) carries the character.

## Leaving (the host)

The occupant leaves when:

- the Humanoid jumps: its jump is pressed (the host's own jump input for the
  host's character, `Space` newly held in a joined player's input frames)
  while the host's copy of the Humanoid has `JumpEnabled` true and a jump
  above zero (`JumpPower` when `UseJumpPower` is set, `JumpHeight`
  otherwise). A game that turns jumping off keeps its driver in, and can use
  the jump key for something else, as Vehicle Simulator's handbrake does;
- a script sets `humanoid.Sit = false` or `humanoid.Jump = true` (the jump
  with the same conditions as above), destroys the `SeatWeld`, disables or
  destroys the seat, or moves the character away;
- the Humanoid dies or the character is removed.

The seat system keeps its own record of every occupant and compares it with
the tree each frame, so a script's write counts however it was made, with no
event to miss.

`Seat:Sit(humanoid)` arrives as a `SitRequest { seat, humanoid }` in the
DataModel's `sit_requests` queue, which only the seat system drains. On a
Player the method does nothing, as a client's `Seat:Sit` does in Roblox.

Leaving clears `seat.Occupant`, `humanoid.Sit` and `humanoid.SeatPart`,
destroys the `SeatWeld`, fires `humanoid.Seated` with `(false, nil)`, starts
the one-second cooldown, and lifts the root clear of the seat's top face.

## Riding (every machine)

The avatar runtime has a component on the avatar entity
(`common/src/avatar/seat.rs`):

```text
AvatarSeated { seat: Entity, offset: Transform }
```

It owns the body, as a climb does. While it is present:

- locomotion does not run (no gravity, no collide and slide, no stepping, no
  jump), inserting it ends any climb, and climbing skips the avatar;
- foot IK and balance fade out, so the legs come from the sit clip rather
  than planting on the vehicle;
- the avatar's collider is a `Sensor`: it pushes nothing, the vehicle
  included, while raycasts and touches still find it;
- the jump intent still flows to the input lane, so the host sees the jump
  and releases the seat;
- the locomotion sample reports `seated`, so the Humanoid's state is
  `Seated` and its Animate plays the sit animation.

Removing it restores the collider and leaves the avatar where it is, with
the jump it left on.

**When it rides.** In `AvatarSystems::Ride`, in PostUpdate, before
animation (and so before the foot IK that raycasts from the root), before
the camera follows, and before transform propagation. PostUpdate comes after
every writer of a seat's pose on both machines: the host's physics writeback
(the fixed schedule, before Update) and a script's `CFrame` write (Play's
apply step, in Update); on a Player, the motion lane and the tree's apply
step (both in Update).

**The seat's pose this frame**, composed from its `Transform` up its parent
chain, never its `GlobalTransform`, which propagation has not updated yet
and so is the last frame's: a rider would trail a fast vehicle by a frame
(half a metre at 30 m/s). On a Player a drawn part has no parent, so it is
the seat's `Transform` itself. The avatar's `Transform` becomes that pose
times `offset`.

`seated_root_height` lives in the avatar metrics: the root's height above
the surface it sits on, with the sit clip's hips retargeted to match, so the
drawn pelvis rests where the `SeatWeld`'s `C0` says.

Who inserts and removes it:

| Machine | Avatar | From |
|---|---|---|
| The host | its own, and each joined player's replica | the seat system, when it seats or releases that Humanoid |
| A Player | its own, and each other player's replica | the Player's replication glue, when a character's `Humanoid.SeatPart` names a seat it draws |

On a Player the seat is the part it draws, moved by the motion lane, so an
occupant rides the vehicle exactly where that Player draws the vehicle.

While seated, the avatar lane keeps flowing, and a replica's correction
leaves a seated replica alone: its seat carries it.

## Input

- `Throttle` and `Steer`: the `VehicleSeat` simulation reads the occupant's
  keys. For the host's character they are the host's own input; for a joined
  player, `DataModel::player_input` for that player's `Player`, found through
  `Occupant` → Humanoid → character → `Players:GetPlayerFromCharacter`.
- Jump to leave: the seat system reads the same keys.

## Pieces and owners

| Piece | Where | Owner |
|---|---|---|
| Sitting and leaving, the cooldown, `Seat:Sit`'s request | `engine/src/play_datamodel/seats.rs` (new, host-only) | server authority |
| `AvatarSeated` and riding in locomotion; `seated_root_height` | `common/src/avatar/` | Client |
| Inserting `AvatarSeated` on a Player; `humanoid.Seated` there from the replicated `SeatPart` | `client/src/systems/net_replica.rs` | server authority |
| A seated replica left to its seat | `eustress-networking/src/session.rs` `correct_replicas` | server authority |
| `SeatWeld` never a physics joint | the joint support (scripted constraints) | Eustress Vehicle Simulator |
| `Throttle` and `Steer` from the occupant | the `VehicleSeat` simulation | Eustress Vehicle Simulator |
| `Seat:Sit` and `Humanoid.Seated` in Luau | `common/src/luau/play/` | mlua |

## Tests

- Host: a character's root touching a free seat is seated, with the four
  writes in one frame and `Seated` fired; a second character touching the
  occupied seat is not; jump releases it and the cooldown holds for a second.
- Replication: a joined player seated on the host has `SeatPart` and the
  `SeatWeld` on its own Player and on every other one.
- Riding: an avatar with `AvatarSeated` on a moving seat stays at the offset
  every frame and runs no locomotion.
