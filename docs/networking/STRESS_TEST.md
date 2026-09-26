# Stress Test: synthetic players on the real protocol

Status: **design, not built.** Work starts after build 8's two-window join test
passes (the Gallery play goal comes first). Owner: Multiplayer. The dialog's
chrome (`engine/ui/slint/stress_test.slint`) is UI's.

## What it is for

Studio's Stress Test answers one question: how many players can this Space
hold on this machine, and what breaks first. It fills the local server with
headless synthetic players that join exactly as the Player joins (protocol v3
handshake, the world download, NetIds, the world, motion, avatar, input and
remote lanes), move and interact through those lanes, and reports what
players would feel (join time, latency, loss, disconnects) beside what the
host pays (frame time and the network's share of it).

## What is there today, and why it measures nothing

`engine/src/network_benchmark.rs` (357 lines) opens raw quinn connections with
a certificate verifier that accepts anything, then writes JSON strings like
`{"type":"position"}` on new unidirectional streams. The host speaks
WebTransport with length-prefixed bincode frames and ignores them, so no bot
ever joins. The latency it reports is the time to write a stream locally.
`simulate_interactions` is never read, and Start in the dialog shows the
"unbuilt feature" notice (`slint_ui.rs` ~7782). UI's slider write-back fix
lands in build 9.

## Design

### Where the bots run: inside Studio, on threads of their own

The bots run in Studio's process on a multi-threaded tokio runtime of their
own, never on the main thread, so a frame of Studio's is the host's work. The
bots still compete for the machine's cores, so the test reports their CPU
beside the host's numbers. It reads the runtime's per-worker busy time
(`RuntimeMetrics::worker_total_busy_duration`, stable on 64-bit targets) as
the cores the bots kept busy. The runtime has at most half the machine's
cores as workers, and at least one.

The bot core is Bevy-free (below), so the same code can later run from the
`eustress` CLI (`eustress stress <join link>`) on a second machine, where the
host's numbers are free of the bots' load entirely, and from CI and agents.

### The bot core: `eustress-networking/src/stress.rs`

Bevy-free: tokio plus the crate's own wire types, so the bots speak the
protocol the Player speaks, from the same definitions.

- **Transport.** A new `native::join_on(handle, &JoinLink) -> NetLink` runs the
  existing `join_main` as a task on a shared multi-threaded runtime, instead of
  one thread and runtime per join (`start_join` is unchanged). Studio passes
  its own join link, key and certificate pin included, so every bot verifies
  the host's certificate by its pin. The accept-anything verifier is deleted.
  The CLI refuses a link without a pin unless it is loopback, as
  `start_join` already does.
- **Join,** as `player_pump` does it: `Hello` (name `Bot 017`, engine version
  `eustress-stress <version>`), `Welcome` (protocol and manifest checked),
  `RequestChunks`, every `ChunkPiece` checked against its size and content
  hash, `WorldReady`, then the catch-up world frames. With "Download the
  world" off, a bot requests no chunks and sends `WorldReady` after `Welcome`,
  which measures steady play without the join's download (the host allows
  it, as it does for a Player whose cache holds the whole world). One bot builds the tree
  from the downloaded world through `tree_read` and keeps a `repl::Replica`, as
  the Player does; it shares what the others need (the NetIds of remotes and
  their `ArgumentTypes`). The others decode and count every world frame but
  keep no tree, since a tree per bot costs memory and the host never sees it.
- **Movement** (simulate-movement): each bot walks from the Space's spawn on a
  seeded wander at walking speed, within the host's plausibility limits
  (`MAX_AVATAR_SPEED`, `SPEED_SLACK_M`), and sends `AvatarFrame` datagrams at
  the Player's rate (`AVATAR_SEND_HZ`, 20 Hz). Input frames go at `TICK_HZ`
  (60 Hz) with W held, so host-side `Player:IsKeyDown` sees a moving player.
  The message-rate slider scales the avatar rate, with 20 as its default.
- **Interactions** (simulate-interactions):
  - a jump flag every few seconds;
  - chat inside the host's limit (`CHAT_BURST` lines per `CHAT_WINDOW`);
  - `FireServer` on each RemoteEvent the Space shows players, with arguments
    built from its declared `ArgumentTypes`;
  - `InvokeServer` on each declared RemoteFunction, timed to its reply.
  Remotes with no declared types are left alone: a bot does not guess
  arguments into a game's code.
- **Stop:** at Stop or the end of the duration, each bot sends `Goodbye` and
  closes; the runtime then shuts down with a 5 second limit.

### Driving bots

With "Drive" on, a share of the bots (a slider, default all) drive instead of
walking, so the host carries N driving cars. A driving bot uses only what a
joined Player uses. The host decides seating (`docs/networking/SEATS.md`) and
reads a driver's keys (`play_datamodel/vehicle_seat_input.rs`), so the host
cannot tell a driving bot from a person.

- **Finding a seat.** The bot that keeps the tree lists every `VehicleSeat`
  in Workspace with its NetId, its pose (from world frames, then from the
  motion lane once the car moves) and its `Occupant`. The run shares one seat
  plan: each driving bot is given its own free seat, so two bots never head
  for the same one. A seat whose `Occupant` becomes someone else, a real
  player or a script, is dropped from the plan and the bot is given another.
  A Space with fewer free seats than drivers says so in the report, and the
  bots left over walk.
- **Claiming it.** The bot walks its avatar to the seat at running speed,
  inside the host's movement limits, and stands where the seat's top meets
  its feet. The host seats a character whose root touches a free seat
  (`seats.rs`, `seat_characters`). The bot learns it is seated from the world
  lane: its `Humanoid.SeatPart` names the seat, and the seat's `Occupant`
  names its Humanoid. If that hasn't happened within 10 s of arriving, the
  claim fails, it is counted, and the bot tries the next free seat.
- **Riding.** A seated bot sends its avatar frames at the seat's pose plus
  the seat offset, as a seated Player's avatar reports (it rides the seat on
  its own machine). The host places seated avatars itself and ignores
  their positions.
- **Driving.** Input frames at `TICK_HZ` hold `W` (Throttle 1) and steer with
  `A` and `D` (Steer -1 and 1) toward a waypoint on a loop around the car's
  start, read against the seat's heading from the motion lane. A bot never
  writes `Throttle` or `Steer` itself; the host turns its keys into them, as
  it does a person's.
- **Leaving.** At Stop the bot presses `Space`; the host releases it when the
  Humanoid may jump. The bot then sends `Goodbye`.
- **Getting a car.** Many games give a player a car on request (a spawn pad,
  a menu, a RemoteEvent). The bots use the Space's free seats, and a Space's
  car request is theirs to make when it is a declared RemoteEvent the
  dialog names. The Vehicle Simulator session names Vehicle Simulator's.

What driving adds to the report:

| Metric | How |
|---|---|
| Cars driving, seat claims, claim time p95, failed claims (no free seat, timeout) | the seat plan |
| Physics step time per car | the engine's `PhysicsStepTimer` (Avian's, in `plugins/physics_plugin.rs`: the time from `PhysicsStepSystems::First` to `Last`, summed over the frame's fixed ticks). A baseline with no cars, then runs at two or three car counts; the cost per car is the slope, since the broad phase isn't guaranteed linear. The worker thread count stays the same across baseline and runs, because the timer is wall time and Avian runs in parallel |
| Where the time goes, per car | Avian's own per-step resources, live with no feature: `CollisionDiagnostics` (broad and narrow phase, `contact_count`) and `SolverDiagnostics` (`solve_constraints`, where a constraint-driven car's joints show, and `contact_constraint_count`), read after `PhysicsStepSystems::Last` |
| Motion lane: bodies moving, samples and bytes per second, per car | the host's motion counters, with `HostNetStats` |
| Key to seat: a bot's key change to the `Throttle` or `Steer` write it sees come back on the world lane | the bots' shared clock |
| Car speed, mean and p95 | the seat's pose on the motion lane, so the cars are shown to be moving |

### What it reports

The bots write their counts and latency histograms into shared atomics and a
stats snapshot. Studio reads them once a frame into the dialog, and builds
the end-of-run report from them. Latencies are p50, p95 and max.

| Metric | How |
|---|---|
| Joined, joining, failed (by reason), disconnected, kicked (by reason) | the join state machine |
| Join time: connect, Welcome, download done, first world frame | per bot, from its start |
| **Relay latency**: one bot's movement reaching the others through the host | every bot shares one clock, so a receiver matches `(peer, seq)` to its sender's send time |
| Relay loss | avatar frames each bot should have received, against those it did |
| RemoteFunction round trip | `InvokeServer` to `RemoteReply` |
| Transport round trip | QUIC's own RTT estimate, from a new `LinkEvent::PathStats` the connection pump sends each second (the Player can show it as ping too) |
| Down and up bytes per second per bot; world ops, motion samples and avatar frames per second | counted as decoded |
| The bots' CPU: cores kept busy, and the share of the machine | the bot runtime's worker busy time over wall time |

The host's side, measured in Studio:

| Metric | How |
|---|---|
| Frame time p50, p95 and max over the run | Studio's own frame times |
| Network work per frame: `host_pump`, `host_send_chunks`, `host_send_replication`, `host_avatar_frame` relays, and `net_replicate`'s observe and serialize | a `HostNetStats` resource the systems add their `Instant` spans to |
| Share of the 60 Hz frame budget (16.7 ms) the host's frame p95 uses | the first row |
| Frames, datagrams and bytes sent per second | counters in `NetLink::send` and `datagram` |
| Remote calls refused (type check, rate limit) and movement violations | the host's existing refusal points, counted |

### Studio's side

- `SlintAction::StartStressTest` starts the run. If Studio is not hosting, it
  hosts first, as F9 does, with `max_players` raised to the bot count plus 8
  (the default cap is 8, from `EUSTRESS_HOST_MAX_PLAYERS`). If it is already
  hosting, a new `HostSession::set_max_players` raises the cap for the run and
  restores it afterwards. The bots start once the host is listening.
- The client count is capped by `MachineCapabilities::recommended_max_clients`,
  and UI binds that cap to the slider.
- `network_benchmark.rs` keeps `MachineCapabilities`, `StressTestState` and the
  Slint sync; the raw-quinn client goes.

### Dialog outputs (UI's tiles, in build 9)

Root properties on StudioWindow, set with `ui.set_stress_*`:

- ints: `stress-joined`, `stress-join-failures`, `stress-disconnects`;
- ms: `stress-join-p95-ms`, `stress-relay-p50-ms`, `stress-relay-p95-ms`,
  `stress-invoke-p95-ms`, `stress-rtt-ms`, `stress-host-frame-p95-ms`,
  `stress-host-net-ms` (network work per frame);
- 0..1: `stress-relay-loss`, `stress-host-budget` (host frame p95 over 16.7 ms);
- kbps per bot: `stress-down-kbps`, `stress-up-kbps`;
- `stress-report`: the end-of-run summary, selectable and copyable;
- `stress-bot-cores`: the cores the bots kept busy (a float), in the HOST row
  beside the host's tiles;
- `stress-download-world` (in-out bool, default true): the "Download the
  world" switch. Studio reads it with `ui.get_stress_download_world()` when
  Start fires and passes it in `SlintAction::StartStressTest { .., download }`.

`stress-total-messages` ("Frames received"), `stress-avg-latency` ("Relay
latency, mean") and `stress-errors` (every failure) stay, with
`stress-running`, `stress-progress` and `stress-status`.
`stress-max-clients` is the Players slider's maximum. The rate slider reads
"Avatar updates per second", 1 to 60, default 20. Tiles turn red on any join
failure, disconnect or error, relay loss above 1%, host frame p95 above
16.7 ms, or a budget above 80%.

## Order of work

1. `LinkEvent::PathStats` and `native::join_on` in `eustress-networking`, plus
   unit tests.
2. `stress.rs`: the bot state machine against a loopback host in a networking
   test (`start_host` with a tiny world, 20 bots join, move, and leave; the
   host sees 20 `PeerJoined`, relays arrive, and no kicks).
3. Studio's side: the bot runtime and its CPU reading, the stats reader,
   `HostNetStats`, and `set_max_players`.
4. The dialog wiring with UI.
5. Proof on a build: Box Head hosted, 50 bots for 60 s, the report compared
   with the microprofiler's host frame (the microprofiler is the only frame
   time that counts; the dialog's number must agree with it).
6. Driving bots, once one real player drives in F9 (the Vehicle Simulator
   session leads that). The seat plan, claim, ride and drive states in
   `stress.rs`, with a loopback test where the host seats 4 bots and their
   seats' `Throttle` follows their keys. Then the driving tiles with UI, and
   proof in Vehicle Simulator: N bots driving, with the step timer checked
   once against the per-system profiler's Avian spans on a baseline run, and
   the report against the microprofiler.
