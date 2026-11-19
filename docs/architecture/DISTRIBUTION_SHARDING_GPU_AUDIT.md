# Distribution, sharding and GPU audit

Audit date: 2026-11-19, finished about 8:00 AM Arizona time. Auditor: a Fable 5.2 worker for the
orchestrator, at McKale's request ("audit the multiplayer distribution sharding with GPUs and ensure
highest quality").

Scope: how Eustress distributes a multiplayer simulation across machines and GPUs, from the Play click
to the server process, and from one host to many (shards).

## 1. Verdict

Hosted multiplayer on Eustress machines is close on paper and not yet real: the placement policy, the
host agent and the Worker control plane are written and the Worker half is tested, but the Rust half has
never been compiled, nothing is deployed, and this audit found three defects that would have stopped the
first two-session machine from working (all three are fixed here, with tests). Sharding (one host per
city, handoff between hosts) is design text only: no handoff message, no shard port, no stable ids across
hosts and no cross-machine determinism exist in code, and its first hard blocker is not networking but
cost blindness, because the fleet budgets every session as a 60 MB, 0.8 thread Pong. GPUs are handled
correctly by avoidance: `--server` creates no device and every authoritative simulation path runs on the
CPU, so the rule to keep is "a hosted or sharded session never needs a GPU", and the two GPU surfaces
that do exist (the renderer's device-loss recovery, and Forge's co-placement scaffold) are client-side
or unbuilt.

## 2. Method and limits

- Read in full by the auditor: `forge/src/fleet_host.rs`, `local_launch.rs`, `studio_compat.rs`,
  `sim/{mod,cell,driver}.rs`, the header and constants of `fleet.rs`, `engine/src/server_mode.rs`, the
  server parts of `dedicated.rs` and `multiplayer.rs`, the Worker's `fleet.mjs`, `fleet_policy.mjs`,
  `fleet_credentials.mjs`, the `/live` routes and the `LiveHost` object in `live.mjs`, the source and
  serving gates in `world.mjs`, and forge-orchestration 0.6.0's `scheduler/{sim,gang,mod}.rs` from the
  cargo registry.
- Read by two delegated workers and spot checked by the auditor: the networking crate
  (`session.rs`, `native.rs`, `wire.rs`, `repl/*`) and the GPU map (`app_core.rs`, `realism/gpu`,
  `particle-sim`, `gpu_recovery.rs`). Claims the auditor did not re-read line by line are marked
  **(delegated read)**.
- The prior review's journal holds 448 finding records, 203 distinct ids, all against the five design
  docs. They were used as leads. Section 9 says which ones today's code confirms, which are already
  fixed, and which are decisions rather than defects.
- **Nothing was compiled.** The orchestrator owns cargo. Every Rust change here parses (`rustfmt
  --check` reads each edited file without a syntax error) and was reviewed by hand for types; that is
  not a build. Worker changes were run: see section 8.
- Labels follow the repo's own: MEASURED, TARGET, CONFIG DEFAULT.

## 3. What exists

| Piece | Where | State |
|---|---|---|
| Headless server: no window, no GPU device, own frame pacer (60 to 120 Hz, 5 Hz idle, optional pause while empty, empty exit, host-failed exit) | `eustress/crates/engine/src/server_mode.rs:208` (runner), `:318` (plugin set), `:111` (host-failed grace) | Written; last compiled state unknown to this audit; the 11-14 to 11-16 changes are booked as unbuilt |
| Session transport: WebTransport over QUIC, one self-signed certificate and one join key per process, pin in the join link | `eustress/crates/common/eustress-networking/src/native.rs:148-164`, `:301` | Built and in use for creator hosting |
| Admission per joiner through the Worker, single-use tickets bound to the certificate pin | `session.rs` (`host_admissions`), `infrastructure/cloudflare/api/src/live.mjs` (`admit`, `admissionPins`) | Built; per-server admission written 11-17, tested, not deployed |
| Placement policy: 80% committed line, 90% activation line, 80% memory line, warm pool held to 80% occupancy, machine advice, GPU placement for a listing that requires one (`docs/architecture/GPU_HOSTING_RULE.md`) | `eustress/crates/forge/src/fleet.rs:1-112`, port in `infrastructure/cloudflare/api/src/fleet_policy.mjs` | Rust written, not compiled; JavaScript port tested with a shared fixture |
| Host agent: one per machine, heartbeat with `ehost_`, start and stop commands, per-session `eshs_` | `eustress/crates/forge/src/fleet_host.rs:1217` (`run_heartbeat`) | Written, not compiled |
| Control plane: one `Fleet` Durable Object, allocate route, heartbeat route, admin routes | `infrastructure/cloudflare/api/src/fleet.mjs:189` (`loadView`), `:271` onward | Written, 26 tests pass, not deployed |
| Local launcher: one process per server, own status directory, port from 7777 | `eustress/crates/forge/src/local_launch.rs:283-340` | Built for Studio's Forge path |
| Studio's loopback Forge compatibility server | `eustress/crates/forge/src/studio_compat.rs` | Built |
| Renderer device-loss recovery (Studio and Player) | `eustress/crates/play-runtime/src/gpu_recovery.rs` | Written 11-19, not compiled, another session is editing it now |
| Agents-in-simulation scheduling scaffold (256 m cells, gang placement, GPU co-placement) | `eustress/crates/forge/src/sim/`, `eustress/crates/engine/src/space/sim_orchestration.rs` | Scaffold behind the `sim-orchestration` feature, off by default |

forge-orchestration 0.6 usage, checked against the 0.6.0 source: the fleet uses `NodeResources`,
`ResourceRequirements`, `PlacementConstraint::Region`, `AutoscalerConfig` and `ScalingDecision` only
(`fleet.rs:116-118`). `GangScheduler`, `CoPlacement` and `RaftStateStore` appear only in the `sim`
scaffold and its demo binary. `TickDeadlineScheduler` is not used anywhere in `eustress-forge`.

## 4. Designed only (no code)

- World shards: the universe, space and job hierarchy, the city handoff, the second QUIC port, the
  tile fallback, the Paris pilot (`docs/world-shards-design.md`; its own lines 229, 275-276 and 333 say
  "not implemented" and "Not built").
- Host migration, warm standby, snapshots (`docs/host-migration-design.md`).
- Idle host preferences and the uptime badge (`docs/idle-host-preferences-design.md`).
- Hosting payouts (`docs/host-bliss-payouts-design.md`).
- Guests on fleet sessions (`fleet.mjs:447`, `:463`: a guest is never placed on one).
- Any reconnect, redirect or handoff message in the protocol **(delegated read: searched `shard`,
  `handoff`, `redirect`, `reconnect`, `migrat` across networking, engine and forge)**.

## 5. Defects, ranked

Status: **FIXED** means changed in the live tree by this audit with a test that fails without the fix
(section 8). **PROPOSED** means too large, not contained, or a decision.

### Blockers

**B1. Every server start deleted the listing's shared directory. FIXED.**
`download_for_host` clears its target before writing (`engine/src/space/gallery_edit.rs:439`), the target
was `~/.eustress_engine/dedicated/<listing>` for every server of the listing (`dedicated.rs:192`), and
the launcher puts each server's `status.json` and `dedicated.stop` in
`~/.eustress_engine/dedicated/<listing>/<server>/` (`local_launch.rs:292`, `:324`), inside that same
directory. Failing scenario: a fleet machine runs a Pong session with two players; the warm pool starts a
second Pong server; its start removes the first server's world files, status file and stop file. On
Linux the first session loses the files under it and can no longer be stopped by file; on Windows the
removal fails on open files and the second server exits with code 1, so a second session of a listing
never starts. This is the exact layout the fleet creates on its first busy machine, and it is also the
"one Forge job per city" layout.
Fix: `dedicated::server_download_dir` gives each server `world/` beside its own status file, or
`<listing>/<server id>/world` when it has no named status directory. The agent removes a finished
server's `world/` (`fleet_host::world_dir_of`, `remove_world`), since each session now has its own copy.

**B2. A server's stop named no server, so it ended every server of the listing. FIXED.**
The three `DELETE /live` calls sent no `server_id` (`multiplayer.rs`, now through `live_stop_url` at
`:721`). The Worker reads a stop with no server as "the whole session" (`live.mjs:1316`, `stop`), and it
refuses a hosted session's `eshs_` a stop that names none (400 `bad_server_id`). Failing scenario: a
creator runs three dedicated servers of one listing; one leaves; the gallery drops all three and clears
every admitted ticket until the survivors heartbeat again (up to 30 s). A fleet session's own stop always
failed, so its listing row waited for the host agent's next heartbeat.
Fix: every stop carries `?server_id=<its own id>`, the id its heartbeats already carry.

**B3. One frame from an unadmitted connection could end the host process. FIXED (decode depth), with a
second line of defence (frame cap before admission).**
`WireValue::Array(Vec<WireValue>)` decoded with no depth bound; the depth check ran afterwards
(`repl/value.rs`, `check_at`). The host decodes every frame from any open connection before it looks at
the peer's stage (`session.rs`, `host_pump`). Failing scenario: anyone holding a join link (every
signed-in account for a listed session) opens a connection and sends one 4 MiB `ToHost::Remote` frame of
nested array headers, twelve bytes a level; the decoder recurses about 300,000 levels and the process
dies on its stack, before `Hello` and before admission. Every session on a packed machine is one process
each, so this is one session per frame, repeatable.
Fix: decoding counts nesting and refuses past `MAX_DECODE_DEPTH` (128; scripts may send 16), wire
format unchanged (`repl/value.rs:23`). Separately, a connection the session has not let in yet may send
at most 16 KiB per frame (`session.rs:2211`, `MAX_UNADMITTED_FRAME_BYTES`).

**B4. A hosted session runs no server scripts unless the listing shares its source with everyone.
DECIDED 2026-11-19 (decision 1, (b) and (c)); Worker change tested, Rust written and not compiled.**
The server downloads `/world/source` and falls back to the player world, which has no
`ServerScriptService` (`server_mode.rs:247-270`, `gallery_edit.rs:418-433`). The Worker stores a source
world only while Share Source is on (`world.mjs:463`, `not_open_source`) and reads it out to anyone
(`world.mjs:633`). Failing scenario: Eustress hosts a listing whose author did not turn Share Source on;
the session starts, lists, admits players, and runs with zero server scripts; the only sign is
`script_note` in a status file nobody shows a player. For Pong that is a table with no ball.
Smallest fix that keeps authors' code private: a source world an author can commit without Share
Source, readable by the author and by an `eshs_` minted for that listing; and the fleet refuses to start
a session for a listing with no source world. This is a credential scope change, so Security reviews it.
Done that way: Studio commits the source on every publish, Share Source decides only whether it is public, an
`eshs_` reads its own listing's world, the fleet refuses a listing with no source world (`409 no_source_world`), and
a session never falls back to the player world. The review, with what each party can read, the token's scope and
expiry, the deleted and locked paths and the cache headers, is
`docs/architecture/PRIVATE_SOURCE_SECURITY_REVIEW.md`. Its F1 corrects this entry's premise: since 2026-11-12 the
player world carries the whole Space, server code included (`echk_export.rs`, `Audience::Players`), so the failing
scenario holds for listings published before that date or without the server services, and a private source keeps
code from players only once the player world stops carrying it.

**B5 (sharding). The fleet prices every session as Pong.** PROPOSED.
`OCCUPIED_MILLIS = 800`, `EMPTY_MILLIS = 63`, `SESSION_MEMORY_MB = 60` are constants for all listings
(`fleet.rs:121-125`, `fleet_policy.mjs:25-29`), MEASURED on a two-player Pong whose server script was not
running. `docs/world-shards-design.md:109` reports Paris at about 15 GB of RAM in Studio. Failing
scenario: a Paris city job is placed by the same policy; the memory line admits 100 of them on an 8 GB
machine (6,553 MB / 60), and the CPU line admits four at "0.8 thread" each when one may need several
whole cores. The 60 Hz floor and the machine both fail on the first placement.
Smallest fix: a per-listing cost record (threads at 60 Hz, resident memory, world bytes), measured by
the host agent from its own servers and kept by the control plane; the policy reads it and falls back to
the Pong constants only for a listing never measured. No shard is placed before this exists.

### High

**H1. A silent connection held a seat for ever. FIXED.** A connection counts against `max_players` from
the moment it opens (`session.rs`, `has_room`), and nothing closed one that never said `Hello`. Failing
scenario: a signed-in account reads the join link from `/live`, opens `max_players` connections and
sends nothing; every later joiner is told the session is full, for as long as the keep-alive runs.
Fix: `HELLO_TIMEOUT` (10 s, `session.rs:454`), enforced in `host_admissions`. `server_mode` now counts
every open connection as someone to serve, so the wait also ends on a server that pauses simulated time
while empty.

**H2. A player's seat was counted twice for up to 60 s. FIXED.** The control plane held each
allocation's reservation for 60 s by the clock alone (`fleet.mjs`, `loadView`); the host's `players`
rose when the player arrived; both counted. Failing scenario: Alice clicks Play on two-seat Pong and
joins; her friend Bob clicks 20 s later; the fleet sees 1 player + 1 reserved = full and starts Bob a
session of his own. Two friends can only meet if they click within the same few seconds.
Fix, three parts: the listing tells the fleet when it admits a fleet session's player
(`live.mjs:1308`, `fleet.mjs:513` `arrived`); the reservation stops counting once that session's machine
has heartbeated 3 s after the admission; and the engine reports `seated` (everyone past `Hello`,
`session.rs:1113`) so a player still downloading the world keeps a seat the host itself counts
(`dedicated.rs` `DedicatedStatus::seated`, `fleet_host.rs` `ServerStatus::seats`). Before this, a
download longer than 60 s gave the seat away.

**H3. The 60 Hz floor is a setting, not a measurement.** PROPOSED. `server_runner` paces frames and
starts a fresh cadence when a frame runs long (`server_mode.rs:192-206`); nothing records that it ran
long. The status file has no frame time, the agent's heartbeat has no load field, and the policy has no
input for it. The activation line allows 90% of a machine's threads (`ACTIVATION_PERMILLE = 900`), so 4
sessions on 4 threads at the measured upper bound of 0.85 thread need 85% and fit; that bound is one Pong
measurement on an i9. Failing scenario: a Space whose occupied frame costs 0.95 thread is placed at the
0.8 budget; 4 of them on 4 threads need 95%; every session drops below 60 Hz and no part of the system
knows. Smallest fix: the server writes its achieved rate and worst frame of the last second to the
status file; the agent reports a per-machine `overrun` flag; the policy stops placing on a machine that
reports one, and decision 2 sets the headroom.

**H4. A restarted host agent forgets its servers.** PROPOSED. `run_heartbeat` keeps its servers in
memory only (`fleet_host.rs:1217` onward). After a restart the agent reports no sessions; the control
plane then revokes the credentials of every session that machine was running (`fleet.mjs`, the
`boot_id` branch and the "no longer reports" loop) and delists them. Failing scenario: the agent is
restarted for an update while 4 sessions have players; the players stay connected to servers that can no
longer admit anyone or heartbeat; the agent believes the machine is empty and accepts 4 more starts; the
machine runs 8 sessions on 4 threads, and the new ones are handed ports 7777 onward, which the old ones
still hold, so they fail to bind. Smallest fix: on boot, read each `status.json` under the dedicated
directory; a live `pid` is adopted (id, listing, port, status file), and its `eshs_` keeps working
because the heartbeat reports the session.

**H5. Nothing ever gives up on a stuck server.** PROPOSED. The agent never kills a process
(`fleet_host.rs:9`), a server whose stop file was written but which never exits stays `stopping` for
ever, and a server that never reaches `hosting` stays `starting` for ever (the engine's own host-failed
exit covers only "Play began and no session is on the way", `server_mode.rs:111`). The policy sends every
new request for that listing to the session that is already starting (`fleet_policy.mjs:337-338`).
Failing scenario: one Paris server hangs in its world load; every Play click for Paris waits on it,
polling `starting` every 5 s, for as long as the process lives. Smallest fix: a start deadline and a stop
deadline in the agent; past either, the slot is reported `dead` and that one process is ended by its own
PID.

**H6. Any web page could start servers through Studio's loopback Forge port. FIXED (the launch path).**
`POST http://127.0.0.1:8090/api/deployments` was accepted with no `Authorization` header
(`studio_compat.rs`, the old module comment said so). A page in the user's browser can send that
request with any body. It started up to 32 internet-facing engine processes for any listing id, against
any `https://` API the body named, so a hostile API could serve its own world with its own server
scripts; and the listing id was used as a directory name unchecked. Fix: a deployment needs the bearer
`/api/auth` hands out (a page cannot add that header without a preflight this server never answers), and
the listing id must be 1 to 64 letters, digits, `_` or `-`. Still open: the body may name any `https://`
API; `stop_children` kills each child before it writes its stop file (`studio_compat.rs:107`), so a
stopped server never tells the gallery it left; the compatibility server never notices a child that died
(`refresh_from_status` reads players only).

**H7. A joiner can make the host buffer the world without limit. PROPOSED (delegated read, the queue
line re-read by the auditor).** `RequestChunks` is accepted repeatedly and appended to the peer's outbox
(`session.rs:1838`); the host pushes 1 MiB a frame per peer (`:84`) into unbounded channels. Failing
scenario: an admitted player asks for every chunk in a loop and stops reading; host memory grows by tens
of megabytes a second on a machine budgeted at 60 MB a session. Smallest fix: one request per join,
`Downloading` stage only, and a bounded per-connection send queue.

**H8. A server's certificate ends 12 days 23 hours 59 minutes after start and nothing rotates it.**
PROPOSED. `native.rs:148-164`. The session keeps heartbeating, the gallery keeps listing it, and every
new join fails at TLS with a generic "could not reach the host". A fleet session with steady traffic
never empties, so it never exits by itself. Smallest fix: the status file carries the certificate's end;
the agent stops routing new players to a session inside its last day and stops it once empty; the server
exits with a named phase at expiry.

**H9. One `esrv_` per listing: a second creator server revokes the first's credential.** PROPOSED
(prior lead DS-1, credential half, confirmed). `authorFromServerCredential` accepts only the hash the
listing names now (`live.mjs:137-152`), and each `--server` child that receives the author's session
mints its own (`dedicated.rs`, `ensure_server_token`). Failing scenario: Studio allocates 3 servers
through Forge; each mints; two of the three heartbeat with a replaced credential and are refused.
Smallest fix: the launcher mints once and gives every child the same `esrv_`. The fleet is not affected
(one `eshs_` per session).

### Medium

- **M1. Cross-machine determinism does not exist.** Avian is built with `parallel` and without
  `enhanced-determinism` (`eustress/Cargo.toml:91`); physics steps at a fixed 60 Hz with 6 substeps
  (`app_core.rs:269-272`). `docs/networking/SERVER_AUTHORITY.md:268-274` claims same-build agreement
  after quantisation and names `enhanced-determinism` as the step for other operating systems. The shard
  doc's overlap rule ("two tiles that both predict the same car must get the same step and the same
  random draws", `world-shards-design.md:248-250`) therefore cannot hold between two hosts with
  different CPUs or thread counts. Handoff must move state; it cannot rely on two hosts computing the
  same answer (decision 5).
- **M2. Ids are host-local (delegated read).** Runtime replication ids are a per-host counter from 1,
  scene ids hash the Space-relative path without the Space name, peer ids are a per-host counter, and
  the tick base is each process's own elapsed time. A car that crosses from Paris to Lyon has no id both
  hosts agree on.
- **M3. No interest management in the live path (delegated read).** Up to 256 bodies per motion sample
  go to every ready player, avatars relay in O(players squared), and a join sends the whole Universe,
  which the host holds in memory. This is the real limit on "one host per city", ahead of sharding.
- **M4. Ports are assigned from a list, never probed.** `take_port` (`local_launch.rs:283`) avoids only
  ports the agent still tracks. A process that was freed but has not exited, an adopted orphan, or any
  other program on 7777 makes the next start fail to bind, exit `host-failed` after 10 s, free the same
  port, and fail again. Smallest fix: skip a port a UDP bind probe cannot take.
- **M5. A request answered `join` keeps that answer for 10 minutes while the session lives**
  (`fleet.mjs`, the `rec.status === 'join'` branch), even when the host then refused the player as full.
  The player's retries return the same link. Smallest fix: a retry that reports the refusal is placed
  again.
- **M6. `draining` can never be true.** The agent sends `draining: false` always
  (`fleet_host.rs:533`), so the plan's "drain it first" advice has no switch behind it. Smallest fix:
  `EUSTRESS_FLEET_DRAINING=1`, or a drain file the agent reads each loop.
- **M7. Studio closes on a render error that is not a lost device.** `gpu_recovery.rs:371` ignores
  `keep_app`, and the handler then writes `AppExit::error()` (`:553-561`). Out of memory is the error a
  shared GPU produces first. The fix is one line (`if situation.hosting || situation.keep_app`) and a
  test, `studio_stays_open_on_a_render_error_that_is_not_a_loss`. It was applied by this audit and then
  the file was replaced by another writer a few minutes later, so it is PROPOSED to that file's owner;
  the edit is kept in the audit's scratchpad as `edits/gpu_recovery.js`.
- **M8. An author's hidden listing can be allocated and can never start. FIXED with decision 1.**
  `handleAllocate` lets the author allocate an unapproved listing; the session's `eshs_` was not a
  credential `world.mjs` knew (`servableListing` read `esrv_` only), so the download was refused and the
  server exited. An `eshs_` now stands for the author on reads of its own listing's world
  (`world.mjs` `credentialAuthor`); the session still admits its author only.
- **M9. Remote-call rate limits are keyed by a peer-chosen id (delegated read).** `repl/remote.rs:115`.
  A ready peer that names a fresh id each call is never limited and grows the map.
- **M10. The start credential lasts 600 s** (`fleet.mjs`, `SESSION_CREDENTIAL_START_MS`) and
  `start_lead_secs` is a 60 s TARGET. A world that takes longer than ten minutes to download and load
  starts with a dead credential. Both need the measured start time of the listing (B5's record).

### Low

- L1. `OCCUPIED_MILLIS` is documented as "rounded up" from 0.75 to 0.85; 800 is the middle, not above
  the top (`fleet.rs:121`).
- L2. A player's input tick near `u64::MAX` overflowed an addition, a panic in a build that checks
  arithmetic. **FIXED** (`repl/input.rs:202`).
- L3. The join key `open` is accepted from loopback (`native.rs`, delegated read); harmless on a
  creator's PC, wrong on a shared host. Turn it off under `--server`.
- L4. The transport binds IPv4 only (`native.rs:301`).
- L5. `eustress-forge` describes itself as "Game server orchestration" (`forge/Cargo.toml:6`,
  `src/lib.rs:3`, `jobs.rs:3`), against the rule on how Eustress is named.
- L6. A connection squatter can now hold a warm server at its full rate (each connection counts as
  someone to serve for up to 10 s). A per-address connection rate limit closes it.

## 6. GPUs

**Where a GPU is required, optional or harmful.**

| Role | GPU | Evidence |
|---|---|---|
| `--server` (creator's dedicated server, fleet session, future city shard) | None. Not created, not probed | `server_mode.rs:318-333`: `MinimalPlugins` with asset containers only; no window, no render plugin |
| Authoritative physics | CPU (Avian, multi-threaded) | `app_core.rs:215`, `:269-272` |
| Particle simulation that feeds physics and script values | CPU (rayon), with a test that 1 and 4 threads agree **(delegated read)** | `crates/particle-sim`, `common/src/realism/particle_sim/plugin.rs` |
| GPU fluid (SPH), GPU terrain, GPU deformation | Not live: behind `realism-gpu` (no crate enables it), a stub, or WGSL text with no reader **(delegated read)** | `common/Cargo.toml:175,184`, `common/src/terrain/compute.rs:592` |
| Ray tracing, DLSS | Rendering only, Player and Studio | `crates/raytracing` |
| Studio or Player as host | Has a GPU, and is exposed to it | today's crash; `gpu_recovery.rs` |
| Forge agent co-placement | Scaffold: a fabricated 24 GB node | `engine/src/space/sim_orchestration.rs:238` |

So the answer to "could the authoritative result differ between a host with a GPU and one without" is
no, today: there is no GPU path that writes authoritative state. That property is worth more than any
GPU speed-up at a shard boundary, and it is not written down as a rule anywhere. It should be
(decision 4): the moment `realism-gpu` or a compute-shader terrain path becomes authoritative, a shard on
a GPU host and its neighbour on a CPU host stop agreeing, and no test would notice.

**Does placement account for GPU capacity and VRAM?** Only for a listing that asks for a GPU
(`docs/architecture/GPU_HOSTING_RULE.md`). Every host reads its GPU from the operating system without
creating a device (`eustress/crates/forge/src/gpu_probe.rs`, compiled into `dedicated.rs` too): the
dedicated server's `hardware` carries `gpu`, `gpu_vram_mb` and `gpu_available`, and so does the fleet
machine record (`fleet_host.rs` `BeatMachine`, `fleet.mjs` `checkHeartbeat`). A listing's `gpu` is
`none` (default), `optional` or `required` with `gpu_min_vram_mb`. `none` and `optional` place as the
CPU policy always has. `required` places only where `gpu_vram_mb` minus the VRAM held by the machine's
other GPU sessions covers the minimum, and is otherwise refused with `no_gpu_capacity`. The launcher
used by Studio's loopback Forge (`studio_compat.rs`, `local_hardware("")`) still passes no GPU name.

**Forge's GPU model, if it is ever used for sessions.** forge-orchestration 0.6 allocates whole
devices: `NodeResources::allocate` pushes a device id into `gpus_allocated`, and `can_fit` skips
allocated devices (`scheduler/mod.rs:173-220`); the gang's summed `gpu_memory_mb` is set to 0
(`gang.rs:241`). Two agents that each need 1 GB cannot share one 24 GB card, and VRAM in use is never
updated. On a one-GPU machine, one GPU member places and the rest of the fleet's cells wait. The
scaffold hides this by registering as many fake devices as it has agents. VRAM-aware sharing does not
exist upstream and would have to be written.

**Several sessions sharing one GPU, and today's crash.** A Player and Studio on one RTX 2080 Ti lost
the device and took the hosted session with them. Three separate things follow:

1. A session hosted from a windowed app is tied to that app's renderer. `gpu_recovery.rs` keeps the app
   open and rebuilds, but the rebuild blocks the main thread, which is the thread the session runs on
   **(delegated read: `block_on` in Bevy's renderer creation, and Studio compiles pipelines
   synchronously)**. Whether the transport's 15 s idle timeout survives a rebuild has not been measured.
2. `--server` has no exposure at all. For anything meant to stay up, host with `--server`, including on
   a creator's own PC: Studio's Start Server could launch the `--server` child instead of hosting in
   process. That is the durable fix for the crash; recovery is the fallback.
3. `gpu_recovery.rs` itself has M7 (Studio exits on out-of-memory), a per-set gate that leaves sibling
   systems in the losing set running, and a stop that never clears **(delegated read for the last
   two)**. No test exercises the handler against a real device.

Paris in Studio uses 9.7 of 11.3 GB of VRAM (`world-shards-design.md:109`). A Player joining from the
same PC cannot fit beside it. The same-machine load shedding in `gpu_recovery.rs` lowers the load; it
cannot make 9.7 + N fit in 11.3. A city host that is also someone's Studio is the configuration to rule
out, not tune.

## 7. Capacity and cost, checked against the code

The activation line is 90% of a machine's threads (decision 2, `ACTIVATION_PERMILLE = 900` in both
`fleet.rs` and `fleet_policy.mjs`, pinned to each other by `the_worker_port_holds_the_same_numbers` on
each side). The code's two CPU lines for a machine of `T` threads, with 800 millithreads per occupied
session and 63 per warm one:

```text
occupied sessions    <= T * 800 / 800          = T                    (committed line, 80%)
sessions of any kind <= floor(T * 900 / 800)   = floor(1.125 * T)     (activation line, 90%)
```

| Threads | Occupied (80% line) | Any kind (90% line) | Every session at the 0.85 upper bound |
|---|---|---|---|
| 4 (VPS-2) | 3200 / 800 = 4 | floor(3600 / 800) = floor(4.5) = 4 | 4 x 0.85 = 3.4 threads, 85% |
| 8 | 6400 / 800 = 8 | floor(7200 / 800) = 9 | 9 x 0.85 = 7.65 threads, 96% |
| 16 (AX42) | 12800 / 800 = 16 | floor(14400 / 800) = 18 | 18 x 0.85 = 15.3 threads, 96% |

On every size the line keeps all sessions at the 0.80 budget within 90% of the threads. On 4 threads it
also keeps them within 90% at the measured 0.85 upper bound; on 8 and 16 threads that bound reaches 96%,
under 100% (the 100% line allowed 106%). `the_activation_line_is_90_percent_and_leaves_room_for_the_measured_spread`
(in `fleet.rs` and in `tests/fleet_policy.test.mjs`) checks the counts and that bound for 1, 2, 4, 8, 16 and 32 threads.

Cost on the VPS-2, $8.50 / 730 h = $0.011644 a machine-hour:

- Packed full of players: 4 occupied sessions, 8 players. $0.011644 / 8 = $0.00146 a player-hour.
- At 80% occupancy: the machine runs at most 4 sessions, 4 x 0.8 = 3.2 occupied, 6.4 players.
  $0.011644 / 6.4 = $0.00182 a player-hour.
- At 10% occupancy: 4 x 0.1 x 2 = 0.8 players. $0.011644 / 0.8 = $0.0146 a player-hour.

Those three are the runbook's $0.0015, $0.0018 and $0.015 (`docs/launch/EUSTRESS_HOSTED_SESSIONS.md`,
"Labelled cost table"): on a 4-thread machine the 80% line, not the activation line, binds the occupied
sessions, so the per-machine numbers hold. What the 90% line changes is how many warm servers fit beside
them. A 4-thread machine holds at most 4 sessions of any kind (it held 5 under a 100% line), so a fleet
at 80% occupancy needs more machines:

```text
100 occupied sessions, warm pool W <= floor(100 / 4) = 25, 125 sessions
committed:  100 x 0.8 + 25 x 0.063 = 81.6 threads; 81.6 / 3.2 = 25.5  -> 26 machines
activation: 125 sessions / 4 per machine = 31.25                        -> 32 machines (binds)
32 x $8.50 = $272 a month; 200 players; 32 x $0.011644 / 200 = $0.00186 a player-hour
```

Under a 100% line the same fleet was 26 machines ($221 a month, $0.0015 a player-hour): the 90% line
costs 6 more VPS-2 (+23%) at 80% occupancy and buys room for every session on the machine to run at the
measured 0.85 thread. A player joining a warm server is not a placement ("that move is not a
placement", `fleet.rs`), so committed CPU may pass the 80% line by that move, never the 90% line.
- Warm pool: `W = min(desired, max(floor(O / 4), min(floors, 2)))`. Occupancy is at least 80% once 8
  sessions are occupied; below that the floor of up to 2 warm servers dominates. With 1 occupied session
  and 2 warm, occupancy is 33%. The 80% target is a property of a busy fleet, not of launch week.

What the arithmetic rests on, none of it measured on a rented machine:

1. 0.8 thread is one i9-10980XE thread, with the server script not running, on a machine with spare
   cores. Each engine process starts task pools sized to the whole machine, so 4 processes on 4 vCores
   contend in a way the measurement never saw.
2. The empty frame costs about 13 ms of a 16.7 ms period. A Space heavier than Pong has 3.7 ms before
   it misses 60 Hz on its own, with the machine otherwise idle. The floor is a per-listing fact.
3. 63 millithreads for an idle server is 0.76 x 5 / 60, a TARGET. The fleet sets
   `EUSTRESS_SERVER_IDLE_PAUSE=1` (`fleet_host.rs`, `server_env`), which is what makes it plausible.
4. Memory is 60 MB a session for Pong; the host keeps every chunk of the Universe in memory.

Before any price is quoted: run 4 Pong sessions with the server script live on one rented 4-vCore
machine and record frame times (the profiler is registered under `--server`), then one heavier Space.

## 8. Fixes made by this audit

No commit, no deploy, no cargo. Line endings preserved per file (checked with node before and after).

| Fix | Files | Tests (filter) |
|---|---|---|
| B1 own download directory per server; agent removes a finished server's copy | `engine/src/dedicated.rs`, `engine/src/server_mode.rs`, `forge/src/fleet_host.rs` | `each_server_downloads_into_its_own_directory` (engine), `a_servers_world_directory_is_its_own` (forge) |
| B2 a stop names its server | `engine/src/multiplayer.rs` | `a_stop_names_its_own_server` (engine) |
| B3 bounded decode depth; frame cap before admission | `eustress-networking/src/repl/value.rs`, `session.rs` | `nesting_is_bounded_while_decoding`, `a_large_frame_before_joining_is_refused_unread` |
| H1 `Hello` deadline; every open connection keeps the server's clock running | `eustress-networking/src/session.rs`, `engine/src/server_mode.rs` | `a_connection_that_never_says_hello_gives_its_seat_back` |
| H2 a seat counted once | Worker `src/fleet.mjs`, `src/live.mjs`; `session.rs` (`seated_count`), `dedicated.rs` (`seated`), `server_mode.rs`, `fleet_host.rs` (`seats`) | Worker: "a reserved seat is counted once" in `tests/fleet.test.mjs`; Rust: `seated_counts_everyone_past_hello`, `a_joining_player_holds_a_seat_before_it_enters_the_world`, `a_status_without_seated_reads_as_zero` |
| H6 a deployment needs the key; listing id checked | `forge/src/studio_compat.rs` | `a_deployment_without_the_key_starts_nothing`, `a_listing_id_that_is_not_a_token_is_refused` |
| L2 input tick overflow | `eustress-networking/src/repl/input.rs` | `a_sample_with_the_largest_tick_does_not_overflow` |

Commands for the orchestrator:

```text
cargo test -p eustress-networking --lib -- nesting_is_bounded a_large_frame_before_joining a_connection_that_never_says_hello seated_counts a_sample_with_the_largest_tick
cargo test -p eustress-forge --lib -- a_servers_world_directory a_joining_player_holds a_deployment_without_the_key a_listing_id_that_is_not_a_token
cargo test -p eustress-engine --lib -- each_server_downloads a_status_without_seated a_stop_names_its_own_server
```

The package names are the ones in each crate's manifest (`eustress-networking`, `eustress-forge`,
`eustress-engine`). Worker, run by the auditor in `infrastructure/cloudflare/api`: `node --test
tests/fleet.test.mjs`, 26 tests, 26 pass; mutation check: with the new `arrived` skip in `loadView`
switched off, the new test fails and the other 25 pass. Full `npm test` on 2026-11-19 at about 7:55 AM
Arizona time: 2,530 tests, 2,529 pass, 0 fail, 1 older todo (G2B-2).

Behaviour changes someone could trip over:

- Studio's Forge client must authenticate before it deploys. Its local path already does (key
  `local-dev`). A connection made with an empty key now gets 401 on deploy.
- A dedicated server's world is now in `.../dedicated/<listing>/<server id>/world`. Whatever was in
  `.../dedicated/<listing>/` from older runs is left where it is.
- The status file has a new field, `seated`. Older readers ignore it; older engines write none and read
  as 0.

## 9. The prior review's leads

All 203 distinct findings are about the five design docs. Grouped by what today's code says:

- **Confirmed and fixed here:** DS-1 (the directory half: B1), DS-2 and HM-1 (a stop takes every server
  offline: B2), DS-SEC-1, IHP-3, IHP-SEC-4, S4, HM-SEC-7, HBP-SEC-5, HBP-SEC-6 (the loopback launch
  route: H6).
- **Confirmed, open:** DS-1 (the credential half: H9), DS-4 and DS-SEC-7 (kill before stop file: H6),
  DS-5 (a dead child stays listed: H6), DS-6, HM-7, HM-OPS-6, WS-OPS-5 (certificate lifetime: H8), DS-8
  (no capacity budget for creator servers), DS-PRIV-5 and DS-LEGAL-4 (server scripts need Share Source:
  B4), WS-F2 (two city jobs on one PC: B1 fixes the directory; a job still cannot be told which city to
  run), WS-F5 and WS-OPS-4 (a host switch is a cold join), WS-F7 (the shard doc ignores the 256 m cell
  scheduler in `forge/src/sim`), WS-OPS-7 and WS-7 (no interest management: M3), WS-8 and its many
  duplicates (who may host: decision 3).
- **Already fixed before this audit:** DS-SEC-5, DS-PRIV-3 (pin half), WS-F3, WS-OPS-3 (admission only
  for the Quick Join server: `admissionPins` now checks each server's own pin, tests in
  `tests/fleet.test.mjs` and `tests/live_admit*.test.mjs`).
- **Not true today:** WS-F1 ("the working tree has removed the `--server` dispatch"):
  `engine/src/main.rs:238-239` dispatches it.
- **Decisions, not defects (the bulk):** every finding about migration, idle hosts, payouts and shards
  that reduces to "the design never says who may run someone else's listing, with what credential, under
  what terms, seeing whose data". They are right, they repeat one gap about forty times, and none can be
  fixed in code before decision 3. The money findings (uptime is the host's own claim; a warm standby is
  paid idle time; guest-only sessions would earn) are sound and all depend on the same decision.
- **Not checked by this audit:** the privacy and legal findings (data location, IP logging, licences for
  the Paris data, Stripe terms). They need counsel, not a code read.

## 10. Order of work

Hosted multiplayer first.

1. Build. `eustress-forge` and the networking crate first (minutes, and they hold most of this audit's
   Rust), the engine when the slot is free. Run the filters in section 8, then each crate's whole lib
   suite once.
2. Decision 1 (made 2026-11-19; Worker change written and tested, Rust written), then Security's review of
   `PRIVATE_SOURCE_SECURITY_REVIEW.md` with the whole fleet control plane (already owed) in one pass.
3. H4 and H5 in the host agent (adopt on boot, start and stop deadlines). Without them the first agent
   update or the first hung server is an outage.
4. H3: frame time in the status file and the heartbeat. Then the density measurement on one rented
   machine (section 7). Set decision 2 from the numbers.
5. H7 and H8 (bounded chunk queue, certificate end in the status file).
6. Staging: one machine, Pong with its server script, two accounts from two networks. Pass means both
   land in one session, the ball moves, a third click starts a second session, both sessions keep 60 Hz
   by the recorded frame times, and stopping one leaves the other listed.

Sharding second, and only after one host carries one city.

7. B5: the per-listing cost record. It is needed for step 4 anyway.
8. Interest management and world streaming on the host (M3). This decides whether a city needs a split
   at all, and it is most of the Paris pilot's real work.
9. Decision 3 (made: the ranked hosts) and decision 5 (made: handoff by state transfer). Then stable ids across hosts
   (M2) and a handoff that goes back through Worker admission. The doc's "reconnect without a round trip
   through the website" contradicts per-process pins and single-use tickets; keep the round trip and make
   it fast with a pre-issued ticket for the neighbour.
10. Only then the second city.

## 11. Decisions for McKale

Decisions 1 to 6 were made by McKale on 2026-11-19. Decision 7 is open.

1. **Server scripts on Eustress machines. DECIDED 2026-11-19: (b) and (c).** (b) A private source: the
   author commits the source without sharing it, and only the author and a session server credential
   (`eshs_`) minted for that listing read it; Share Source keeps working as before, a public source anyone
   can read. (c) The fleet refuses to start a hosted session for a listing that has no source world, so a
   session never runs half a simulation. Implemented in the working tree; the review is
   `docs/architecture/PRIVATE_SOURCE_SECURITY_REVIEW.md` (B4, M8). The options were: (a) keep the old rule,
   where a listing had to turn Share Source on, publishing its whole source to everyone, or its hosted
   sessions ran with no server scripts; (b); (c).
2. **Headroom for the 60 Hz floor. DECIDED 2026-11-19: (b), 90%.** The activation line is 90% of a
   machine's threads: one fewer session on some machine sizes, room for the measured spread. Revisit with
   the measurement in section 10 step 4. Implemented by another worker (`forge/src/fleet.rs`,
   `fleet_policy.mjs`). The options were: (a) keep 100%, at the measured upper bound a fully occupied
   machine needs 106%; (b) 90%; (c) raise the per-session budget from 0.80 to 0.85 thread and keep 100%.
3. **Who may host a listing besides its author. DECIDED 2026-11-19, in McKale's words: "I already had it
   so most qualified (ping and power) ranked hosts can host. Do not go against what is already there."**
   The rank decides: a session's `/live` heartbeat carries `latency_ms` and `hardware` (`gpu`, `cpu`,
   `tier`), and `rankServers` in `live.mjs` orders a listing's servers by full last, then
   `rankScore` (0.5 fullness, 0.3 hardware tier, 0.2 ping), then players. Players go to the best ranked
   host with a seat, whoever runs it; nothing restricts hosting to machines Eustress owns. A machine
   hosts fleet sessions when an administrator mints it an `ehost_`. Private source still reaches only a
   session credential minted for that listing, whoever's host runs it; what a third-party operator can
   do with it is in the review's section 4. Every change of this audit was checked against this: none
   narrows who may host or bypasses the rank (review section 6). One older line did bypass it
   (allocation passed over an empty fleet session that outranked a creator's host) and is fixed, with a
   test. The options were: (a) Eustress-run machines only; (b) vetted third parties under host terms.
4. **GPU rule for hosts. DECIDED 2026-11-19: CPU first, with a GPU option.** Hosted and sharded sessions
   run every authoritative simulation path on the CPU, which is how they run today; GPU simulation on a
   host is an option to build, which needs GPU and VRAM in the heartbeat, VRAM-aware placement that
   forge-orchestration does not have, and proof that GPU and CPU hosts agree. Implemented by another
   worker (`dedicated.rs` heartbeat GPU fields, `forge/src/fleet.rs`, `fleet_policy.mjs`). The rule:
   `docs/architecture/GPU_HOSTING_RULE.md`.
5. **How a car crosses between hosts. DECIDED 2026-11-19: (a), state transfer.** One owner at a time: the
   source host stops simulating the car when the destination confirms it. Implemented by another worker
   (`eustress/crates/networking`). The rejected option was (b): both hosts predict across an overlap,
   which needs bit-identical physics across machines.
6. **Studio's Forge key. DECIDED 2026-11-19: it stays as coded.** A deployment through the loopback port
   needs the key, which closes a web page starting servers on a creator's PC (H6).
7. **`gpu_recovery.rs`.** Open. Its owner applies the one-line `keep_app` fix (M7), or says why Studio
   should close on an out-of-memory render error.
