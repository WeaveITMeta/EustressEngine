# Server Authority

How a hosted Eustress session keeps every player in the same world, for any
simulation: whatever instances it has, whatever its scripts change, whatever
input it reads. The host is the one source of truth. Players send what they
do, and the host decides what happens.

This page is the design and the contract. The status of each part is in
[Phases](#phases), and [LOCAL_SERVER.md](LOCAL_SERVER.md) covers running a
session today.

## Roblox's model, and where it stops

Roblox shipped Server Authority to every creator in July 2026. A place turns
it on with `Workspace.AuthorityMode = Server`, which forces five more
settings: `NextGenerationReplication`, `PlayerScriptsUseInputActionSystem`,
`SignalBehavior = Deferred`, `UseFixedSimulation` and `StreamingEnabled`.

- Clients send only input, through `InputAction`s. The server simulates.
- Game logic moves into functions bound with `RunService:BindToSimulation`,
  in ModuleScripts that run on both the client and the server. Only
  properties marked Simulation Access, and attributes, take part.
- The client runs ahead of the server by its latency (6 frames at 100 ms and
  60 Hz). When the server's state for a frame differs from what the client
  predicted, the client rolls back every predicted instance to the server's
  state and simulates forward again with its own inputs.
- `RunService:SetPredictionMode` sets `Off`, `Automatic` (parts inside the
  player's simulation radius) or `On` per instance.
- An instance made inside a simulation callback gets a deterministic id from
  its type, its source, the frame and a per-script counter, so the server's
  copy merges with the client's ("instance stitching").
- Other players' characters render slightly in the past.

Roblox's own documentation and engineering posts name the gaps:

- **Everything predicted rolls back together.** Their staff call the missing
  piece "prediction islands".
- **Resimulation is expensive**: at 100 ms of latency, a misprediction costs
  6.25 times the normal simulation load.
- **Floating point drifts** between Windows clients and Linux servers. A
  0.1-stud threshold filters the noise, and random mispredictions still occur
  on an empty baseplate.
- **RemoteEvents are not ordered** with property replication. An event can
  arrive before the change it refers to.
- **No server-side rewind** for latency-compensated hit detection.
- **Smoothing corrections is manual**: an invisible simulated part, a
  rendered clone, and `TweenService:SmoothDamp` in `RenderStepped`.
- **Synchronized custom state is capped**: 64 attributes per instance, names
  and strings under 50 characters, 1 KB in total.
- **It is opt-in and needs a rewrite** into `BindToSimulation`.
- **Late inputs are dropped**: the server ignores input that arrives too
  early or too late.

## What Eustress does instead

| | Roblox Server Authority | Eustress |
|---|---|---|
| Turning it on | Opt-in: six settings, and the game logic rewritten | Every hosted session. Existing scripts run on the host unchanged |
| What rolls back | Every predicted instance at once | Only the island that mispredicted |
| Float drift | Client and server disagree below 0.1 stud | Both sides quantize state at every tick before storing or comparing it, and run one Rust simulation |
| Smoothing corrections | Manual, per instance | Built in for every predicted or interpolated body |
| RemoteEvent order | Independent of property replication | One tick-ordered stream: an event fired after a write arrives after it |
| Hit detection | No rewind | The host keeps 1 s of pose history; a hit query rewinds to what the shooter saw |
| Synced custom state | 64 attributes, 50-character strings, 1 KB | Any property or attribute, within a per-player bandwidth budget |
| Late input | Dropped | Sent redundantly; the host buffers to measured jitter |
| Remote arguments | Anything a client sends | Typed and rate-limited before a script sees them |
| Joining | The server streams the place | The world is content-addressed `.echk`, cached between sessions; the host sends only what changed since it began |
| Authority granularity | One setting for the place | Per instance: host, predicted, or validated owner |

The table is the design. [Phases](#phases) says which parts exist.

## The model

### One tick, stamped everywhere

Studio and the Player both step physics at a fixed 60 Hz (6 substeps), so the
host's physics step is the session tick. Every message carries the host tick
it describes. A player estimates the host's tick from the Welcome and its
round-trip time, runs its own prediction ahead of that estimate by half the
round trip plus its input buffer, and renders everyone else behind it by the
interpolation delay.

### Identity: `NetId`

Every replicated instance has a 64-bit `NetId`, the same number on the host
and on every player.

- **Scene instances**, the ones both sides loaded from the same world, take
  a hash of their record key (the Space-relative path of the file that
  defines them, such as `Workspace/Car/Body/_instance.toml`). Each side
  computes it from its own load, so no table crosses the wire. The top bit is
  clear. At session start the host checks for two keys that hash alike and
  sends any such instance as a spawn instead.
- **Runtime instances**, made by `Instance.new`, `Clone`, or the engine
  (players, characters), get a host-minted id with the top bit set.
- **Predicted spawns** (phase 4) get an id built from the player, the input
  tick and a per-tick counter, so the host mints the same id when it runs the
  same input, and its copy lands on the player's.

The hash is FNV-1a 64 followed by the SplitMix64 finalizer. It is written out
in the code, never taken from `std`'s hasher, whose output may change between
Rust releases.

### Three lanes

| Lane | Transport | Carries | Loss |
|---|---|---|---|
| World | the reliable, ordered stream | spawns, destroys, reparents, property and attribute writes, tags, RemoteEvents to players, sounds and particle bursts, animation track changes, as one `WorldFrame` per tick | never lost, never reordered |
| Motion | datagrams | poses and velocities of bodies physics moved | the newest wins; a lost one is replaced by the next |
| Input | datagrams | each player's input for recent ticks, redundantly | covered by the redundancy |

A transport without datagrams (a WebSocket fallback) carries the motion and
input lanes on the stream instead.

Putting RemoteEvents in the world lane is what orders them: a script that
sets `door.Open = true` and then fires `DoorOpened` produces one frame with
the write first and the event second, and every player applies them in that
order.

### What replicates

- **Containers players see**: Workspace, ReplicatedStorage, ReplicatedFirst,
  Lighting, Players, Teams, StarterGui, StarterPack, StarterPlayer,
  SoundService, Chat.
- **Never**: ServerScriptService, ServerStorage, and every `Script`'s
  `Source`. The published world already leaves them out
  (`echk_export`, `Audience::Players`); replication applies the same rule to
  what scripts do at run time.
- **Not the host's view**: the host's `Camera`. Each player has its own.
- **One player only**: a player's `PlayerGui`, `Backpack` and
  `PlayerScripts` go to that player. The host fills each joined player's
  `PlayerGui` from StarterGui as Roblox does: everything when they join, then
  the `ResetOnSpawn` GUIs again on each respawn. That player's machine draws
  it and runs its LocalScripts; the host does neither.

An instance moved from a container players see into one they do not (a part
parented to ServerStorage, or to `nil`) is removed on every player, and sent
again as a spawn if it comes back.

### Joining late

A player loads the world the host exported when the session began, from the
CDN or its own cache. The host then sends one `WorldFrame` with everything
since: scene instances destroyed or moved, the properties scripts changed on
the rest (only those properties), and every runtime instance in scope,
parents before children. Live frames follow on the same stream, so nothing
falls between the two.

The world itself never crosses the host's upload. A 300,000-instance world
costs the host only its live changes.

### Input, for any simulation

Every tick a player samples its devices into an `InputFrame`: the keys held
(a bit set over the key table), mouse and gamepad buttons, six gamepad axes,
the camera position, and the look and aim directions (octahedral, 16 bits per
axis). Each datagram carries the newest four ticks, so a lost datagram costs
nothing.

The host keeps each player's input by tick. What reads it is the same thing
that reads the host's own input: the character controller for that player's
character, a `VehicleSeat` for the player sitting in it, and server scripts
through the player (`Player:IsKeyDown`, `Player:GetActionValue`). A
simulation needs no input code to become multiplayer: whatever its
characters and seats do with the host's keyboard, they do with each player's.

Values are clamped on arrival (axes to ±1, directions renormalized), and at
most one frame per tick is accepted.

### Characters and seats

A player's character is an instance tree the host owns: a Model named after
the player in Workspace, holding a `HumanoidRootPart`, a `Head` and a
`Humanoid`, with `Player.Character` pointing at it. The host makes one for
every player whose avatar is in the world, its own and each joined player's,
so `CharacterAdded`, `Touched` and `Players:GetPlayerFromCharacter` behave the
same for all of them.

Until characters move by input on the host (SA-2b), each player's machine
moves its own avatar and sends its pose on the avatar lane, and the host and
every other player place that avatar from it. The character's
`HumanoidRootPart` is bound on each machine to the avatar body that stands
for it there:

| Machine | The root is bound to |
|---|---|
| The host | its own avatar, or the replica of a joined player's |
| That player's machine | its own avatar |
| Every other player | the replica of that player's avatar |

So the root's `CFrame` is current everywhere and is never sent twice. Writes
to character instances replicate like any other: a `Humanoid`'s `WalkSpeed`,
a tool welded to a hand, a `BillboardGui` a server script parents to the
head.

Seats are the host's decision. When a character's root touches a free `Seat`
or `VehicleSeat`, the host seats it: `Occupant`, `Humanoid.Sit`,
`Humanoid.SeatPart` and the `SeatWeld` arrive on every player in one frame.
The seated player's avatar then rides the seat's drawn pose on its own
machine and stops walking. Its input still flows, so the host's `VehicleSeat`
takes `Throttle` and `Steer` from the occupant's keys, the same keys
`Player:IsKeyDown` reads. Jump, from the occupant's input, unseats it on the
host, and the release replicates the same way. A player never seats itself;
the host acts on the touch it sees.

### Animation tracks

A track's time and weight follow from its control state and the clock, so
tracks replicate as control changes, never as poses. The machine whose script
played a track sends its changes:

- A player sends the changes its scripts make on its own character's
  Animator. The host checks each one (the Animator is inside that player's
  current character; the clip is in the published world: `space://`,
  `bundled://`, `rig://` or a Roblox asset id; speed, weights and fades are
  within bounds; at most 30 changes a second, in bursts of 60), plays it on
  its own tree, and relays it on the world lane to everyone else.
- The host's own scripts' tracks, an NPC's or a server-played emote, go to
  every player that can see the Animator, the player it plays on included.
- A track a player plays on anything but its own character stays on that
  machine, as in Roblox.

Times travel in session ticks and each machine converts them to its own
clock, so every machine shows a track at the same phase. A late joiner gets
every loaded track in its catch-up.

### Remotes: typed, ordered, rate-limited

`RemoteEvent`, `UnreliableRemoteEvent` and `RemoteFunction` replicate as
ordinary instances.

- `FireServer` and `InvokeServer` send `ToHost::Remote` on the stream.
  `UnreliableRemoteEvent:FireServer` sends a datagram.
- The host checks each call before any script sees it: the remote exists
  and the player can see it; the arguments decode (depth at most 16, 4,096
  values, 64 KiB); every instance an argument names is one that player can
  see; and the player is within the remote's rate (60 calls a second, bursts
  of 120). A remote with declared argument types
  (`RemoteEvent:SetArgumentTypes({"Vector3", "number"})`) also has each
  argument's type checked and its numbers checked for NaN.
- `FireClient` and `FireAllClients` become a `Remote` op in the world lane.
- `InvokeClient` is not offered: a player that never answers would hold a
  server script forever.

Arguments are `WireValue`s: the property value types, plus arrays and string
keyed maps. Instance references travel as `NetId`s.

### Prediction islands and reconciliation

A player predicts, by default, its own character and anything that character
drives (the assembly of a `VehicleSeat` it sits in). A `PredictionMode`
attribute (`Off`, `Automatic`, `On`) changes that per instance.

Predicted bodies are grouped into islands: bodies touching, jointed, or
touched by the same predicted body within the history window. For each
island the player keeps one second of (tick, input, quantized state).

When the host's state for tick T arrives:

1. Quantize it the way the prediction was quantized, and compare it with the
   prediction for T, per island.
2. An island that matches drops its history up to T. Nothing else happens.
3. An island that differs is set to the host's state at T and replayed to the
   present with the player's inputs. Only that island replays. A character
   replays by running its controller again, which is cheap. A vehicle whose
   error is small takes the error as an offset instead of a replay; a large
   error replays.

Both sides quantize state at the end of every tick (positions to 1/1024 m,
rotations to 16 bits per component, velocities to 1/256 m/s) before storing
it or stepping from it. A difference below one quantum never reads as a
misprediction, and the same inputs produce the same quantized state on either
side. The determinism test runs one input sequence through two worlds and
compares state hashes tick by tick; enabling Avian's `enhanced-determinism`
(its libm path) is the step that extends this across operating systems.

### Interpolation and smoothing

Everything the player does not predict renders at `now - delay`. The delay is
two snapshot intervals plus measured jitter (about 67 ms at 30 snapshots a
second), and a body is Hermite-interpolated using both its poses and its
velocities. Rotation follows the angular velocity too (up to 327 rad/s), so a
wheel turning more than half a turn between snapshots still turns forward.
When snapshots stop, a body extrapolates for up to 250 ms, then holds.

A correction to a predicted body never teleports what is drawn. The
simulated transform jumps; the rendered one carries the difference as an
offset that decays over about 100 ms. This is built into the rendering of
every predicted and interpolated body, so no simulation has to build it.

### Lag-compensated hit queries

The host keeps one second of pose history for every replicated part and
character. `workspace:RaycastAsSeenBy(player, origin, direction, params)`
casts against the world as that player saw it: at the tick its input frame
says it was rendering, capped at 250 ms in the past so a player with a slow
connection cannot shoot around corners long after the fact.

### Bandwidth: relevance and priority

Each player has a budget (256 KB/s by default, adapting to loss). World-lane
ops always go. Motion entries compete: each tick, every body a player can
see gains priority by relevance (near the player's camera, fast, recently
changed) times the time since it was last sent; the highest go first until
the tick's share of the budget is spent, and each one sent starts again from
zero. Distant, slow bodies update less often but never starve.

### Security

- A player never states where anything is, its own character included, once
  phase 2 lands. Until then the host checks every avatar frame for speed and
  reach, as it does today.
- What a player may send: input, remote calls, chat, appearance, purchase
  answers and receipts. Each has a size cap and a rate.
- Server containers and server code never leave the host.
- A player learns only what it can see: other players' private containers,
  and parts outside its relevance set (phase 3), stay on the host.

## Phases

| Phase | Delivers | Status |
|---|---|---|
| SA-1 | NetIds; the world lane (every instance, property, attribute and tag scripts change); late join; the motion lane for bodies physics moves, interpolated on players; input frames carried to the host; `Player:IsKeyDown` for joined players; typed, rate-limited remotes | Built and run in a two-window session on one machine (2026-09-24): the world download, the catch-up and avatars worked. Build 5 (2026-09-25) carries all of phase 1 and phase 2a so far, and the networking tests pass (56 of 56). Protocol v3 and its core: `eustress-networking` (`repl`, `session`). The host: `engine/src/net_replicate.rs`. The Player: `client/src/systems/net_replica.rs`, applying the host's frames to the tree it reads from the downloaded world (`common/src/tree_read.rs`, drawn by `tree_apply.rs`) |
| SA-2a | Characters as instances on every machine: the host makes one for every player and each machine binds it to the avatar body it has; seats the host decides, the occupant's avatar riding its seat, `VehicleSeat` input from the occupant | Built in build 5 (2026-09-25), with its networking tests passing; the two-window test (`PLAYTEST_SA2A.md`) is next. The host makes every joined player's character and fills their `PlayerGui` (`engine/src/play_datamodel/remote_players.rs`); `Player.Character` and its events reach every player; each Player binds characters to the avatars it draws; animation tracks replicate; seats are decided by the host and ridden on every machine ([SEATS.md](SEATS.md)) |
| SA-2b | Characters driven by input on the host, predicted and reconciled on their player; the Player runs LocalScripts; terrain edits scripts make | Designed |
| SA-3 | Prediction islands for vehicles and anything a character pushes; lag-compensated hit queries; relevance and priority; per-player motion slots in place of 8-byte ids | Designed |
| SA-4 | `RunService:BindToSimulation` for scripts, with rollback of properties and attributes; predicted spawns | Designed |

## The contract with the Luau runtime

Replication moves DataModel changes between two DataModels. On the host that
is Studio's Play tree. On a player it is the Player's own tree, which the
Luau runtime brings to the Player. Both sides agree on this:

1. **The Player builds its tree from the world it loaded**, with the whole
   hierarchy (Models, Folders, values, scripts, GUI, not only Parts), and
   calls `Replica::bind_scene(record_key, instance_id)` for each scene
   instance. The host calls `HostReplicator::bind_scene` the same way at
   Play start. `record_key` is the Space-relative path of the file that
   defined the instance.
2. **Replicated changes go through the tree's normal writes** (`set_prop`,
   `set_parent`, `destroy`, `create`), so they mark themselves dirty and the
   Player's apply step renders them, and `Changed`, `ChildAdded` and
   `Destroying` fire for LocalScripts.
3. **Remote calls leave and enter through queues on the DataModel** while
   `networked` is set (a host sets it while it serves; with no session,
   remotes loop back inside the VM as they always have):
   - `remote_out`: calls scripts made. `FireServer` and `InvokeServer` on a
     player; `FireClient` and `FireAllClients` on the host.
   - `remote_in`: calls that arrived, for the VM to fire `OnServerEvent`
     and `OnClientEvent`.
   - `reply_out` and `reply_in`: answers to `InvokeServer`.

   Arguments are `RemoteValue`s: a `DmValue`, an array, or a string-keyed
   map of them. A remote's optional `ArgumentTypes` property (comma
   separated `typeof` names, `?` for optional) is checked on the host before
   a script sees the call.
4. **The host exposes each player's input** to the character controller, to
   seats, and to scripts through the `Player` instance. `Player:IsKeyDown`
   and `Player:IsMouseButtonPressed` read that player's newest input
   (`DataModel::player_input`), which the host fills every frame.

## Verification

Each of these can fail, which is what makes it a test:

- Identity: the same record key hashes to the same `NetId` in both processes,
  and a key never collides with a runtime id (top bit).
- The world lane: a host tree changed by a script sequence, replicated into
  an empty player tree loaded from the same records, ends with the same
  instances, names, parents, properties, attributes and tags. The test also
  applies the frames to a tree with one frame dropped and expects a mismatch.
- Order: a write followed by a fire in one frame arrives in that order.
- Late join: a player joining after changes ends identical to one that
  joined before them.
- Scope: nothing under ServerScriptService or ServerStorage, and no `Source`
  of a `Script`, is ever encoded.
- Remotes: a call over the rate, over the size, with an argument of the wrong
  type, or naming an instance the player cannot see, never reaches a script.
- Determinism (SA-3): two worlds, one input sequence, identical quantized
  state hashes at every tick.
