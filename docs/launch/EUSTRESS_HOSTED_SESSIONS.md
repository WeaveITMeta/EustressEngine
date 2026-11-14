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
| Fix for that race | Written in `eustress/crates/engine/src/server_mode.rs` (`enter_play` waits for the deferred service queue, at most 1,800 frames). **Not built** | Needs the orchestrator's build slot, then the local proof below again |
| `EUSTRESS_SERVER_HZ` (server frame rate, 10 to 120, default 60) | Written in `server_mode.rs`. **Not built** | Same |
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
2. The log says `Luau Play VM ready: 1 server and ... script(s) queued`
   (it says 0 in the installed build: that is the bug above).
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
export EUSTRESS_SERVER_HZ=30            # see "Tick rate" below
export EUSTRESS_SERVER_TOKEN=<esrv_ credential, from the environment, never an argument>
./eustress-engine --server --sim <staging listing id> --env staging \
    --port 7777 --status-file /var/lib/eustress/7777/status.json
```

`--sim` downloads the listing's **source** world (server scripts included)
when the listing has Share Source on; otherwise it hosts the player world and
says server scripts will not run (`space/gallery_edit.rs download_for_host`).
More sessions on one machine: one process each, ports 7778, 7779 and up, one
status file each, as Forge's local launch does.

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
cut: a lower `EUSTRESS_SERVER_HZ` (built, then measured), and a profile of
what an empty headless frame spends about 13 ms on.

## Cost

Scaled from this PC's per-session numbers by thread count only. A VPS
vCore or a Ryzen thread is not the same speed as a thread of this i9; that
difference was not measured. Budget: 70% of the threads, 0.8 thread per
2-player Pong session (the measured 77 to 85%, rounded).

| Machine | Price/month | Threads | Sessions (CPU) | Sessions (memory, 60 MB each, 80% of RAM) | Players | Per session | Per concurrent player |
|---|---|---|---|---|---|---|---|
| OVHcloud US VPS-2 | about $8.50 | 4 vCores | 3 | about 109 | 6 | about $2.83 | about $1.42 |
| Hetzner AX42 | $117.10 | 16 (8 cores) | 14 | about 870 | 28 | about $8.36 | about $4.18 |

CPU binds long before memory or bandwidth (6 players at 32 kbps is under
0.2 Mbps). At these numbers the VPS is the cheaper unit, and the advisor's
assumed density of about 100 players per $8.50 box is about 16 times too
high for today's server. 100 players per box needs roughly a 16-fold cut in
the empty-frame cost, or several sessions in one process.

If CPU per session falls in step with the frame rate (assumed, not
measured), 20 Hz would make it about 0.27 thread, so about 10 Pong sessions
(20 players) per VPS-2, about $0.43 per concurrent player a month. Measure
before quoting it.
