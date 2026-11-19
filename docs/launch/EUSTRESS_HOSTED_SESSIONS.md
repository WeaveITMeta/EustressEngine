# Eustress-hosted sessions

How Eustress, not a creator's Studio, hosts a multiplayer session: one
headless `eustress-engine --server` process per session, on a machine
Eustress rents. Worked example: **Pong** (2 players). Example world for the
first public session: **{EXAMPLE_WORLD: McKale names it}**.

Nothing here is deployed. Renting a machine, a production deploy, a
production republish and committing all need McKale's word.

## What works now, and what does not

| Piece | State | Evidence |
|---|---|---|
| `eustress-engine --server` in the installed 0.3.8 Studio | Starts with no window or GPU, hosts on UDP, writes its status file, stops on `dedicated.stop` | Local runs, this document's "Measured" section |
| Server scripts (`ServerScriptService`) on `--server` | **Broken in the installed build.** Play starts before the Space's services finish loading, so the Luau VM queues 0 scripts and the world export players download lacks `StarterPlayer` scripts | Two runs on a Pong copy: `Luau Play VM ready: 0 server and 0 client script(s) queued`, logged before `ServerScriptService` and `StarterPlayer` were spawned |
| Fix for that race | In `eustress/crates/engine/src/server_mode.rs` (`enter_play` waits for the deferred service queue, at most 1,800 frames). **Built and measured 2026-11-16** | `Luau Play VM ready: 1 server and 0 client script(s) queued` on a Pong copy; the status file reports 0 players until someone joins |
| `EUSTRESS_SERVER_HZ` (frame rate with players in the session, 60 to 120, default 60; 60 is the floor) | Built 2026-11-16. The first build's runner slept a little long each frame and measured 50 Hz; frames are now due at fixed deadlines (`next_frame_due`). **That fix is in the rebuild; re-measure** | Frame profile below |
| `EUSTRESS_SERVER_IDLE_HZ` (frame rate with nobody in the session, 1 to the full rate, default 5) | **Built and measured 2026-11-16** | 63% of one core at 60 Hz, 21% at 5 Hz, reached 39 s after start |
| `EUSTRESS_SERVER_IDLE_PAUSE=1` (simulated time is paused while the server idles with nobody in it; off by default; the fleet host sets it) | Written in `server_mode.rs`. **In the rebuild; not measured** | Same |
| `EUSTRESS_SERVER_EMPTY_EXIT_SECS` (leave after this long empty; unset means never; at least 60) | Written in `server_mode.rs` and `dedicated.rs`. **Not built** | Same |
| A server that cannot host leaves (`"phase": "host-failed"`, non-zero exit code) | Written in `server_mode.rs` and `dedicated.rs`. **Not built** | Same |
| Frame profile under `--server` (`EUSTRESS_PROFILE=1`, `EUSTRESS_FRAMETIME_CSV=<path>`) | Written in `server_mode.rs` and `profiler.rs`. **Not built** | Same |
| Placement and the host agent on Forge (80% of a machine's threads, a small warm pool, 60 Hz floor, each server in its own status directory) | Written in `eustress/crates/forge/src/fleet.rs`, `fleet_host.rs` and `local_launch.rs`. **Not built** | `cargo test -p eustress-forge --lib fleet` after the build |
| Play click starts or finds a session (the Worker's allocate and host-heartbeat routes, the `ehost_` and `eshs_` credentials, the placement policy in JavaScript) | Written in `infrastructure/cloudflare/api/src/fleet.mjs`, `fleet_policy.mjs`, `fleet_credentials.mjs` and `live.mjs`. **Not deployed**; Security reviews first. `wrangler.toml` carries a new Durable Object (`FLEET`, migration `v10-fleet`) | `npm test`: 2,523 tests, 2,522 pass, 0 fail, 1 todo that predates this work; 37 are new |
| The host agent's heartbeat to the Worker, and admission for more than one server of a listing | **Not written** | The next two pieces; see docs/architecture/HOSTED_FLEET.md |
| World play token (cheaper chunk downloads) | Worker code and 11 tests, full suite green. **Not deployed.** Player side **not built** | docs/architecture/WORLD_PLAY_TOKEN.md |
| Heartbeat with the `esrv_` server credential | Worker code exists in the tree; whether staging runs it was not checked here | docs/networking/DEDICATED_SERVER.md |
| A join from another network through a UDP forward | Not done | Needs a rented machine or McKale's router |

Known side effect, reported, not fixed: a `--server` started with `--space`
writes its Engine Bridge port file into a Universe other than the hosted
Space's (on this PC, `Documents\Eustress\Astra\.eustress\engine.port`). The
MCP server reads that file to find an engine, so a running `--server` can
take over the MCP's view of that Universe. Studio's own port file was not
touched.

## Run the host on this PC (loopback only)

Host from a **copy** of the Space, never from a folder Studio has open.

```powershell
$copy = "$env:TEMP\eustress-host\Games"
Copy-Item -Recurse "$HOME\Documents\Eustress\Games\.eustress" "$copy\"
Copy-Item -Recurse "$HOME\Documents\Eustress\Games\Spaces\Pong" "$copy\Spaces\"
$env:EUSTRESS_HOST_LAN = "0"          # loopback only; no heartbeat without a public host
$env:EUSTRESS_SERVER_HZ = "60"        # after the fix is built
& "$env:LOCALAPPDATA\Programs\Eustress Engine\eustress-engine.exe" `
    --server --space "$copy\Spaces\Pong" --port 7777 `
    --status-file "$env:TEMP\eustress-host\status.json"
```

The log prints `join with eustress-player://join/127.0.0.1:7777?key=...&pin=...`.
Open that link with the Player, or run bots:

```powershell
eustress-stress --connect "<that link>" --clients 2 --duration 60
```

Stop it by creating `dedicated.stop` beside the status file.

Accept the host when:

1. `status.json` says `"phase": "hosting"` and `"listening": true`.
2. The log says `Luau Play VM ready: 1 server and 0 client script(s) queued`
   (it says 0 server in the installed build: that is the bug above). The
   server has no Player of its own and runs no LocalScript, so `Players`
   holds only the players who join.
3. With nobody joined, Pong's paddles play themselves, so the Ball moves:
   read it twice over the Engine Bridge (`properties.get` on the Ball's
   uuid) and the Position differs.

## Run the host on a Linux x64 VPS

Assumes the Linux build of 0.3.8 or later with the fix above.

```bash
sudo ufw allow 7777/udp
export EUSTRESS_HOST_LAN=1
export EUSTRESS_HOST_PUBLIC=<the VPS public IPv4>
export EUSTRESS_HOST_REGION=us-east-1
export EUSTRESS_SERVER_HZ=60            # the default, and the floor for a session with players
export EUSTRESS_SERVER_IDLE_HZ=5        # the default; shown for clarity
export EUSTRESS_SERVER_EMPTY_EXIT_SECS=600   # optional: a fleet that scales to zero
export EUSTRESS_SERVER_TOKEN=<esrv_ credential, from the environment, never an argument>
./eustress-engine --server --sim <staging listing id> --env staging \
    --port 7777 --status-file /var/lib/eustress/7777/status.json
```

`--sim` downloads the listing's **source** world (server scripts included)
when the listing has Share Source on; otherwise it hosts the player world and
says server scripts will not run (`space/gallery_edit.rs download_for_host`).
More sessions on one machine: one process each, ports 7778, 7779 and up, one
status file each, as Forge's local launch does.

## Idle rate and empty exit

An empty server frame costs about as much as a full one (see "Measured"),
so the server runs fewer frames while nobody is there.

- **Full rate**, `EUSTRESS_SERVER_HZ` (CONFIG DEFAULT 60; 60 is the floor and
  120 the ceiling): while the Space
  loads and Play starts, while a player is in the world or downloading it,
  and for 30 s after any session notice. A `Hello` that waits for the
  gallery's admission raises one, so a whole join runs at the full rate.
- **Idle rate**, `EUSTRESS_SERVER_IDLE_HZ` (CONFIG DEFAULT 5): after 30 s
  with none of the above. A joining player's `Hello` waits at most one idle
  frame (200 ms at 5 Hz) before the host reads it; the frame after that is
  a full-rate one. The transport's keep-alive (3 s) and idle timeout (15 s)
  run on their own thread, and the session waits 15 s for an admission and
  20 s for a Welcome, so the idle rate stays well inside every timeout. A
  value at or above the full rate turns the idle rate off.
- Simulated time stays real time at either rate: a frame may carry two
  periods of virtual time (0.4 s at 5 Hz), and physics keeps its fixed
  60 Hz step, catching up inside each frame.
- **Empty exit**, `EUSTRESS_SERVER_EMPTY_EXIT_SECS` (CONFIG DEFAULT unset,
  meaning never; at least 60): after that many seconds with nobody, the
  server writes `"phase": "exited-empty"` to its status file and leaves the
  way a stop does, so the gallery hears the session ended. A fleet starts a
  fresh process for the next player.
- **Failed host**: a server that entered Play and has no session hosting,
  and none on the way, for 10 s (the world bake, the bind or the listener
  failed) writes `"phase": "host-failed"` and ends with a non-zero exit
  code. Whatever started it frees the slot and starts a fresh one.

The log states all three at startup and each change of rate.

## Staging steps that need McKale

1. Publish the example world to **staging**, signed in to staging, with
   **Share Source** on and `Players.MaxPlayers` set for the game (Pong: 2).
   The Pong Staging copy on this PC has `max_players = 12` and no publish
   record yet.
2. Mint the listing's `esrv_` server credential with his staging sign-in
   (`infrastructure/cloudflare/api/src/live.mjs`, author only).
3. Off-network join: a player on another network joins through a forwarded
   **UDP 7777** (router or VPS firewall). STUN only shows the outbound
   mapping; a real outside player is the proof.

## Gates that need McKale's word

- Renting any machine (OVHcloud, Hetzner, anything).
- Any production deploy (the Worker's play token included; Security reviews
  WORLD_PLAY_TOKEN.md first).
- Republishing a production listing.
- Committing this work.

## Measured on this PC

2026-11-14, installed `eustress-engine.exe` (Studio 0.3.8 install, built
2026-11-14 6:28 AM) with `--server`, one Pong copy per instance,
loopback only (`EUSTRESS_HOST_LAN=0`), 60 Hz (the installed build has no
`EUSTRESS_SERVER_HZ`). Machine: Intel Core i9-10980XE (18 cores, 36
threads, 3.0 GHz base), 256 GB, with Studio and other builds running beside
it (25 to 67% machine load before the bots). Bots: `eustress-stress` built
from this tree, 2 per server, 60 s, avatar at 20 Hz, movement on. CPU is
each server process's own CPU time over 30 s (100% is one logical core);
memory is its private bytes, since the 400 MB executable image is shared
between instances (working set about 353 MB each).

**Caveat:** PongServer did not run in these sessions (the race above), so
the ball and paddles were not moving and their replication is not in these
numbers. Treat CPU and bandwidth as a floor for Pong.

| Servers | Idle CPU each | With 2 players, CPU each | Private memory each | Players joined | Bot exit codes |
|---|---|---|---|---|---|
| 1 | 76% | 79% | 59 MB | 2 of 2 | 0 |
| 5 | 73 to 77% | 75 to 79% (389% total) | 57 to 59 MB | 10 of 10 | all 0 |
| 10 | 73 to 78% | 77 to 85% (808% total) | 57 to 60 MB | 20 of 20 | all 0 |

Per player, after the 74 KB world download: about 32 kbps (4 KB/s) host to
player and 49 kbps (6 KB/s) player to host. Join time p95 100 to 118 ms,
RTT 3.5 to 5.7 ms, relay loss 1.7 to 1.8%, no kicks or disconnects.

The installed Player also joined one of these servers on loopback: welcomed
as peer 3, downloaded the 74 KB world, opened Pong, and the server's status
file counted 1 player; it joined as a guest because the Player was not
signed in.

What this says: an **empty** server spends about three quarters of a core
at 60 Hz, and two players add only a few percent. The engine's per-frame
cost, not the game or the players, sets the density. That is the thing to
cut, and the tools to find it run under `--server`: `EUSTRESS_PROFILE=1`
logs where each frame's time goes, and `EUSTRESS_FRAMETIME_CSV=<path>`
writes the per-frame series. A session with players stays at 60 Hz or
above; only an empty one slows down.

## Measured on the 2026-11-16 build

Release build of this tree (linked 6:35 PM), `eustress-engine.exe --server`
on a temp copy of Pong, loopback only (`EUSTRESS_HOST_LAN=0`), nobody
joined, PongServer running. Same machine as above. CPU is the process's own
CPU time (100% is one logical core).

| What | Value | Label |
|---|---|---|
| Time from start to `"phase": "hosting"` | 9 s | MEASURED |
| Server scripts | `1 server and 0 client script(s) queued`; `players` 0 | MEASURED |
| CPU at the full rate, nobody in it | 63% of one core | MEASURED |
| CPU at the 5 Hz idle rate | 21% of one core (two 20 s windows: 20.6, 20.7) | MEASURED |
| Idle rate reached | 39 s after start (30 s after hosting began) | MEASURED |
| Private memory | 64 MB | MEASURED |
| Mean frame at "60 Hz" | 19.9 ms, which is 50.4 Hz | MEASURED |
| Main-world time per frame | 8.0 ms (Update 4.9, PreUpdate 2.4, PostUpdate 0.5) | MEASURED |
| Main-thread CPU per frame | 3.0 ms; the rest runs on worker threads | MEASURED |
| Script apply stage | mean 2.94 ms over 600 frames, with one frame of 1,378.75 ms | MEASURED |

What this says:

- The server-script race is fixed, and the server is no longer one of its
  own players.
- The idle rate cuts an empty server's CPU about 3 times, not 12. Physics
  keeps its 60 steps a second at 5 Hz, so only the per-frame cost drops:
  from the two measurements, a frame costs about 7.7 ms of CPU and a fixed
  step about 2.8 ms. `EUSTRESS_SERVER_IDLE_PAUSE=1` stops those steps on an
  empty server; with it the idle cost should be the five frames alone,
  about 4% of a core (TARGET until measured).
- 60 Hz ran at 50 Hz. The runner slept for what was left of each period
  and every sleep ran about 3 ms long. Frames are now due at fixed
  deadlines, so a long sleep is paid back by the next one.
- The placement budget of 0.063 thread for an empty server is a TARGET
  that the first measurement did not reach (0.21). The fleet's rule that
  every session on a machine can run at 60 Hz at once bounds the machine
  either way; the empty budget is set again from the next measurement.

## Cost

Scaled from this PC's per-session numbers by thread count only. A VPS
vCore or a Ryzen thread is not the same speed as a thread of this i9; that
difference was not measured. Budget: 0.8 thread per 2-player Pong session
(the measured 77 to 85%, rounded). Two CPU lines place sessions: occupied
sessions use at most 80% of the threads (the committed line), and every
session on the machine, warm ones included, could run at 60 Hz at once
within 90% of the threads (the activation line). The last 10% is room for
the measured spread: 4 sessions on 4 threads at the 0.85 upper bound use
85%.

```text
occupied sessions    <= threads x 0.8 / 0.8            = threads
sessions of any kind <= floor(threads x 0.9 / 0.8)
```

| Machine | Price/month | Threads | Occupied sessions (80% line) | Sessions of any kind (90% line) | Sessions (memory, 60 MB each, 80% of RAM) | Players | Per session | Per concurrent player |
|---|---|---|---|---|---|---|---|---|
| OVHcloud US VPS-2 | about $8.50 | 4 vCores | 4 | floor(3.6 / 0.8) = 4 | about 109 | 8 | about $2.13 | about $1.06 |
| Hetzner AX42 | $117.10 | 16 (8 cores) | 16 | floor(14.4 / 0.8) = 18 | about 870 | 32 | about $7.32 | about $3.66 |

CPU binds long before memory or bandwidth (8 players at 32 kbps is under
0.3 Mbps). At these numbers the VPS is the cheaper unit. Reaching about
100 players on an $8.50 box takes roughly a 12-fold cut in the empty-frame
cost, or several sessions in one process; the frame profile above is how
the first is found.

### Labelled cost table (OVHcloud US VPS-2)

Every number carries a label:

- **MEASURED**: the 2026-11-14 run above (installed 0.3.8, loopback, Pong,
  60 Hz, i9-10980XE, PongServer not running).
- **CONFIG DEFAULT**: what the server does when the variable is unset.
- **TARGET**: arithmetic from MEASURED numbers under a stated assumption.
  Not a measurement; quote none of them until a build is measured.

Common inputs: VPS-2 at about $8.50 a month, 730 hours a month, so about
$0.0116 a machine-hour; 4 vCores, 80% committed line, so 3.2 threads for
occupied sessions, and 90% activation line, so 3.6 threads for every
session at 60 Hz at once (both assumptions, see above); 2 players per Pong
session. Occupancy is the
share of running sessions that have players in them; the target is 80%.
The empty-server row assumes CPU scales with the frame rate. Physics keeps
its fixed 60 Hz step at every frame rate, so the real saving is smaller by
whatever share of the 13 ms frame physics takes; the frame profile
measures that share.

| Case | CPU per session | Sessions per VPS-2 | Players | Cost per player-hour | Label |
|---|---|---|---|---|---|
| Idle server, 60 Hz | 0.73 to 0.78 thread | | | | MEASURED |
| 2 players, 60 Hz | 0.75 to 0.85 thread (budget 0.8) | 4 (3.2 / 0.8) | 8 | about $0.0015 ($0.011644 / 8 = $0.00146) | MEASURED CPU; sessions and cost TARGET |
| Private memory | 57 to 60 MB | | | | MEASURED |
| Host to player | about 32 kbps | | | | MEASURED |
| Empty server, idle rate 5 Hz | about 0.063 thread (0.76 x 5/60) | | | | CONFIG DEFAULT rate; CPU TARGET |
| Fleet at 80% occupancy: 60 Hz with players, 5 Hz empty, empty exit on | 0.65 thread average (0.8 x 0.8 + 0.2 x 0.063) | 4 of any kind (floor(3.6 / 0.8)), 3.2 occupied | 6.4 on average | about $0.0018 ($0.011644 / 6.4 = $0.00182) | TARGET |
| The same fleet at 100 occupied sessions | warm pool at most floor(100 / 4) = 25, so 125 sessions | 125 / 4 = 31.25, so 32 VPS-2 ($272 a month; the committed line alone needs 81.6 / 3.2 = 25.5, so 26) | 200 | about $0.0019 (32 x $0.011644 / 200 = $0.00186) | TARGET |
| For comparison, sessions left running and occupied 10% of the time, 60 Hz always | 0.8 thread average | 4 | 0.8 on average | about $0.015 ($0.011644 / 0.8 = $0.0146) | TARGET |
| A session that has ended (empty exit) | 0 | | | a world download and a Space load at the next start | TARGET |

The fleet rows hold while placement keeps each machine's committed CPU at
or under 80% of its threads, keeps every session's 60 Hz cost at or under
90% of them, and keeps empty servers to a small warm pool. The 90% line is
what binds the fleet row: a VPS-2 runs 4 sessions of any kind, warm ones
included, so the warm pool takes room the occupied sessions could use.
`eustress-forge` does both: `eustress/crates/forge/src/fleet.rs` is the
policy, and `fleet_host.rs` is the agent on each machine, started as
`eustress-forge host`. The contract with the control plane is
[HOSTED_FLEET](../architecture/HOSTED_FLEET.md). Written, not built.
