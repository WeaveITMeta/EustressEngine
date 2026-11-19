# Hosted fleet: placement, host agent, control-plane contract

How Eustress packs hosted sessions onto rented machines at 80% of their
threads, keeps at least 80% of running sessions occupied, and starts a
session only when a player asks. One session is one `eustress-engine
--server` process (a simulation server). The founder's targets
(2026-11-16): 60 Hz minimum for a session with players, 5 Hz allowed while
empty, room left for an empty server to return to 60 Hz, occupancy 80%.

Nothing here is deployed and nothing is rented. The Worker routes below are
written and tested (see "Implemented in the Worker"), and the host agent
speaks Route 1. Renting a machine, any production deploy, and minting any
credential need McKale's word.

Every number is labelled: **MEASURED** (the 2026-11-14 run in
docs/launch/EUSTRESS_HOSTED_SESSIONS.md: installed 0.3.8, i9-10980XE,
loopback, 2-player Pong, 60 Hz), **CONFIG DEFAULT** (what the code does when
nothing is set), or **TARGET** (arithmetic under a stated assumption, not a
measurement).

## Pieces

| Piece | Where | State |
|---|---|---|
| Placement policy (pure functions) | `eustress/crates/forge/src/fleet.rs` | Written, not compiled |
| Host agent, `eustress-forge host` | `eustress/crates/forge/src/fleet_host.rs`, `src/bin/eustress_forge.rs` | Written, not compiled; heartbeats Route 1 with the machine's `ehost_` (`EUSTRESS_FLEET_HOST_TOKEN`), and without it reads join requests from a local inbox for local testing |
| One status directory per server (its own `dedicated.stop`) | `local_launch::plan` | Written, not compiled |
| Host heartbeat and allocate routes | `infrastructure/cloudflare/api/src` | Written and tested, not deployed |

## The policy in one page

CPU is counted in millithreads (1000 = one thread).

| Input | Value | Label |
|---|---|---|
| CPU of a session with players at 60 Hz | 800 (0.75 to 0.85 thread, rounded up) | MEASURED |
| CPU of an empty server at 5 Hz | 63 (0.76 x 5 / 60) | TARGET |
| Memory per session | 60 MB (57 to 60) | MEASURED |
| Committed-CPU line | 80% of the threads | CONFIG DEFAULT |
| Activation line | 90% of the threads | CONFIG DEFAULT (decision 2, 2026-11-19) |
| Memory line | 80% of the RAM | CONFIG DEFAULT |
| Occupancy target | 80% | CONFIG DEFAULT |
| Frame rate with players / empty | 60 Hz / 5 Hz; under 60 is refused | CONFIG DEFAULT |
| Hold before an empty server is stopped | 120 s | CONFIG DEFAULT |
| A join request counts as recent | 600 s | CONFIG DEFAULT |
| Start to hosting | 60 s | TARGET (not measured) |
| Warm servers the floor may keep while few sessions run | 2 | CONFIG DEFAULT |
| Empty exit set on every server | 900 s | CONFIG DEFAULT |

Per machine of `T` threads, a session is placed only if

```text
committed + cost             <= T * 800      (cost: 800 for a player, 63 for a warm server)
(sessions + 1) * 800         <= T * 900      (every warm server at 60 Hz at once still fits in 90%)
(sessions + 1) * 60 MB       <= RAM * 0.8
```

A 4-thread machine holds 4 occupied sessions (3200 / 800) and at most 4
sessions of any kind (3600 / 800 = 4.5, rounded down). A player joining a
warm server is not a placement: it may lift committed CPU past the 80% line,
never past 90%.

A listing whose `gpu` is `required` places only on a machine that reports
`gpu_available` and has its `gpu_min_vram_mb` free, and is otherwise refused
with `no_gpu_capacity`; `none` and `optional` place as above
([GPU_HOSTING_RULE](GPU_HOSTING_RULE.md)).

Warm pool, with `O` occupied sessions fleet-wide and `t` = 0.80:

```text
floor_L   = 1 if listing L had a click in the last 600 s and no running session of L with players has a free seat
wanted_L  = ceil(cold clicks of L in 600 s * 60 / 600)
W         = min(sum max(floor_L, wanted_L), max(floor(O * (1 - t) / t), min(sum floor_L, 2)))
```

`W <= floor(O / 4)` once `O >= 8`, so occupancy `O / (O + W)` is at least
0.80. Empty servers beyond a listing's share stop after 120 s empty, from
the emptiest machine first. A click goes, in order, to a running session
with players and a free seat (the Quick Join rank), a warm server, a
session already starting, a new session on the fullest machine that fits,
or a refusal with a recommendation to add a machine. The plan recommends
machines through forge-orchestration's `AutoscalerConfig` (add at 80% of the
committed lines, release a machine with no players when the rest stays at
or under 30%, 300 s between machine changes); it never rents.

## Authentication

Two new credentials, both on the pattern of the `esrv_` server credential
in `live.mjs` (32 random bytes, hex, only the SHA-256 stored in the SOCIAL
namespace, `Authorization: Bearer`, checked before `verifyAuth`):

- **Host credential** `ehost_<64 hex>`, one per rented machine. Minted by an
  admin route (`requireAdmin`), stored as `hostcred:<sha256>` with
  `{ v: 1, machine_id, region, created_at }`, and `hostcred-machine:<machine_id>`
  naming the current hash, so minting again revokes the last. It
  authenticates the heartbeat only. It is not accepted by `/live`.
- **Hosted-session credential** `eshs_<64 hex>`, one per started session.
  The Worker mints it when it tells a host to start a session, stores
  `hostsess:<sha256>` with `{ sim_id, machine_id, server_id, expires_at }`,
  and accepts it on `POST`/`DELETE /api/simulations/{sim_id}/live` and
  `POST .../live/admit` for that `sim_id` and that `server_id` only, exactly
  where `esrv_` is accepted today. It is deleted when the session's stop is
  acknowledged or its `/live` entry expires. Why not `esrv_`: an `esrv_`
  stands for the listing's author and one listing; a fleet machine hosts
  many listings for Eustress, so each session the host agent starts carries
  its own `eshs_`. The agent's inbox mode, which has no control plane,
  carries one `EUSTRESS_SERVER_TOKEN` and so serves one listing.

A secret appears only in a TLS response body and in the child's
environment (`EUSTRESS_SERVER_TOKEN`). Never in argv (`local_launch::leaks_secret`
tests this), a log line, a status file, or any file in the repository.

## Route 1: host heartbeat

`POST /api/fleet/hosts/{machine_id}/heartbeat`, `Authorization: Bearer ehost_...`.
The host agent reads the credential from `EUSTRESS_FLEET_HOST_TOKEN` in its
environment and nowhere else.
Outbound only: the agent polls every 5 s (CONFIG DEFAULT) and the machine
opens no inbound port besides the sessions' UDP ports.

Request:

```json
{
  "boot_id": "uuid of this agent start",
  "seq": 1842,
  "machine": { "threads": 4, "memory_mb": 8192, "region": "us-east-1", "draining": false,
               "gpu": "NVIDIA GeForce RTX 2080 Ti", "gpu_vram_mb": 11264, "gpu_available": true },
  "capacity": { "committed_cap_millis": 3200, "committed_millis": 1663, "activation_cap_millis": 3600,
                "activation_millis": 2400, "occupied_room": 1, "warm_room": 1 },
  "sessions": [
    { "id": "8f0c...", "listing_id": "07d0...", "phase": "hosting", "players": 2, "reserved": 0,
      "max_players": 2, "started_at": 1763300000, "empty_since": null, "port": 7777 }
  ],
  "freed": [ { "id": "11aa...", "why": "exited_empty" } ],
  "acked": [ "cmd_01J...", "cmd_01K..." ],
  "rejected": [ { "command_id": "cmd_01L...", "why": "would pass the activation line" } ],
  "agent_version": "0.1.0"
}
```

`sessions` are `fleet::Session` values; `capacity` is `fleet::capacity`.
`machine.gpu`, `machine.gpu_vram_mb` and `machine.gpu_available` are the
machine's GPU as `gpu_probe` reads it from the operating system (empty, 0 and
false without one); an agent that sends none of them reports no GPU.
`why` is `exited_empty`, `host_failed`, `stopped` or `dead` (`fleet_host::Freed`).
No secret, pin or join link is in the heartbeat; each session publishes its
own link through its own `/live` heartbeat as today.

Answer `200`:

```json
{
  "seq": 1842,
  "commands": [
    { "command_id": "cmd_01M...", "do": "start", "listing_id": "07d0...", "max_players": 2,
      "server_id": "8f0d...", "for_request": "req_...", "session_credential": "eshs_..." },
    { "command_id": "cmd_01N...", "do": "stop", "server_id": "11ab..." }
  ],
  "next_heartbeat_secs": 5
}
```

- **Idempotency.** Every command has a `command_id`. The agent records the
  ids it executed for the life of `boot_id` and lists each in `acked` once.
  The Worker resends a command until it is acked, rejected, or 60 s old;
  the agent ignores an id it already executed. A heartbeat with a `seq` at
  or below the last seen for this `boot_id` is answered with no commands. A
  new `boot_id` means the agent restarted: the Worker forgets that machine's
  unacked commands and rebuilds its view from `sessions`.
- **Who decides.** The Worker holds the whole fleet (one Durable Object,
  `fleet`, so every decision sees one consistent state) and runs the same
  policy as `fleet::plan`. Either port it to JavaScript with the Rust tests'
  inputs and outputs as JSON fixtures (every type in `fleet.rs` is serde),
  or build `fleet.rs` to wasm; the second task chooses. The agent still
  checks each start with `fleet::can_place` against its own machine and
  rejects one that would cross a line, so a Worker bug cannot overload a
  machine.
- **Server id.** The Worker names `server_id`; the agent uses it as the
  process's `EUSTRESS_SERVER_ID`, so the `/live` entry and the fleet entry
  are the same id (1 to 64 of `A-Z a-z 0-9 _ -`, the `/live` rule).

Failures:

| Case | Answer | Agent does |
|---|---|---|
| Bad or revoked `ehost_` | 401 `bad_host_credential` | Starts nothing new; running sessions continue and leave by empty exit |
| `machine_id` does not match the credential | 403 `wrong_machine` | Same as 401 |
| Body not JSON, a field out of range | 400 with the field | Fixes nothing automatically; logs |
| Worker unreachable, 5xx, or 429 | none | Retries with backoff (5 s, doubling to 60 s); starts nothing new; still writes stop files for its own sessions past the hold |
| No heartbeat from a machine for 30 s | (Worker side) | The Worker places nothing there; its sessions stay listed through their own `/live` heartbeats until those expire (90 s) |
| A start the agent rejects | listed in `rejected` | The Worker re-plans that request elsewhere |

## Route 2: allocate

`POST /api/simulations/{id}/allocate`, from the listing page or the Player
when a player clicks Play. Authentication and gates are those of
`GET /api/simulations/{id}/live`: a signed-in account, or a guest with a
live lease on this listing for a guest-only session (GUEST_PLAY.md), the
listing listable or the viewer its author or an admin, and the adult
content gate. Allocation never replaces admission: the player still presents
a ticket at Hello (`POST .../live/admit`).

Request: `{ "request_id": "uuid from the client", "region": "us-east-1" }`
(`region` optional).

Answers:

| Status | Body | Meaning |
|---|---|---|
| 200 | `{ "status": "join", "link": "eustress-player://join/...", "cert_sha256": "...", "server_id": "...", "warm": false }` | A running session with a free seat, or a warm server just assigned (`warm: true`) |
| 202 | `{ "status": "starting", "server_id": "...", "retry_after_secs": 5 }` | A session is starting for this player, or one already starting has a seat; ask again with the same `request_id` |
| 503 | `{ "status": "full", "code": "no_capacity", "retry_after_secs": 30 }` | No machine has room for another 60 Hz session; the plan recommended adding one |
| 503 | `{ "status": "full", "code": "no_gpu_capacity", "gpu_min_vram_mb": 8192, "retry_after_secs": 30, "error": "..." }` | The listing requires a GPU and no machine with that much VRAM free has room; it is never started on a machine without one, and the plan recommended a machine with that GPU ([GPU_HOSTING_RULE](GPU_HOSTING_RULE.md)) |
| 409 | `{ "status": "unavailable", "code": "no_source_world" }` | No live server has a seat, and the listing has no source world for the world it plays, so the fleet starts nothing: a session would run with no server scripts. Its author publishes again from Studio, which commits the source world (PRIVATE_SOURCE_SECURITY_REVIEW.md) |
| 409 | `{ "status": "unavailable", "code": "not_hostable" }` | The listing is deleted or under a legal lock; the answer does not say which |
| 400 | `{ "code": "bad_request_id" }` | `request_id` missing or not 1 to 64 of `A-Z a-z 0-9 _ -` |
| 401 / 403 / 404 | as `GET /live` | Not signed in, gated, or no such listing |
| 429 | `{ "code": "rate_limited" }` | More than 10 allocations a minute from one account or lease (CONFIG DEFAULT) |

- **Idempotency.** The `fleet` object keeps each `request_id` for 10
  minutes with its answer and the seat it reserved. The same id again
  returns the same session; a 202 becomes a 200 once that server's `/live`
  entry carries a link. A reservation not used within 60 s (CONFIG
  DEFAULT, `fleet_host::RESERVATION_SECS`) frees the seat.
- **Order.** First the listing's live servers exactly as Quick Join ranks
  them (`rankServers` in `live.mjs`, mirrored by
  `local_launch::rank_servers`): the first with a seat gets the player,
  whoever hosts it. A creator's own host, a fleet session with players and
  an empty (warm) fleet session that already answers at a link are all in
  that one order. Then the placement policy: a fleet session not yet
  listed, then a start. Who
  owns a machine never enters the order (decision 3).
- **Failures.** The fleet object unreachable: 503 `fleet_unavailable` with
  `retry_after_secs`; the page falls back to today's Quick Join link when
  `GET /live` has one. A server that was assigned but never hosts
  (`host-failed`): the next retry with the same `request_id` is placed
  again and answered 202.

## Cost at 80% occupancy and 80% utilisation

Common inputs: OVHcloud US VPS-2, about $8.50 a month, 730 hours, so about
$0.0116 a machine-hour; 4 vCores; 2 players per Pong session. A VPS vCore is
not the same speed as a thread of the i9 the numbers were measured on; that
difference is not measured. Consistent with the runbook's cost section.

| Case | Arithmetic | Result | Label |
|---|---|---|---|
| CPU per session with players, 60 Hz | 0.75 to 0.85 thread, budget 0.8 | 0.8 thread | MEASURED (budget rounded up) |
| CPU per empty server, 5 Hz | 0.76 x 5 / 60 | about 0.063 thread | TARGET |
| Committed line on 4 vCores | 4 x 0.8 | 3.2 threads | CONFIG DEFAULT |
| Occupied sessions per VPS-2 | 3.2 / 0.8 | 4 | TARGET |
| Sessions of any kind per VPS-2 | floor(4 x 0.9 / 0.8) = floor(4.5) | 4 | TARGET |
| VPS-2 packed full of players | 4 sessions, 8 players, $0.0116 / 8 | about $0.0015 per player-hour | TARGET |
| VPS-2 at 80% occupancy | 4 sessions, 3.2 occupied, 6.4 players, $0.0116 / 6.4 | about $0.0018 per player-hour | TARGET |
| Average CPU per session at 80% occupancy | 0.8 x 0.8 + 0.2 x 0.063 | about 0.65 thread | TARGET |
| Fleet with 100 occupied sessions, committed check | warm pool at most 25; 100 x 0.8 + 25 x 0.063 = 81.6 threads; 81.6 / 3.2 = 25.5 | 26 VPS-2 would do for the 80% line | TARGET |
| Same fleet, activation check | 125 sessions / 4 per VPS-2 = 31.25; 125 x 0.8 = 100 threads <= 32 x 3.6 = 115.2 | 32 VPS-2 (this binds), 200 players, about $272 a month, 32 x $0.0116 / 200 = about $0.0019 per player-hour | TARGET |
| Host to player bandwidth | about 32 kbps per player; 8 players | under 0.3 Mbps per VPS-2 | MEASURED per player; total TARGET |
| For comparison, sessions left running and occupied 10% of the time at 60 Hz | 4 sessions, 0.8 players on average | about $0.015 per player-hour | TARGET |

CPU binds long before memory (4 x 60 MB of 8 GB) or bandwidth.

## What the second task builds

1. The `fleet` Durable Object: hosts, sessions, recent clicks, request ids.
2. The admin route that mints `ehost_` and the `eshs_` check in `handleLive`.
3. Both routes above, with tests, and the policy port or wasm build with
   the `fleet.rs` cases as fixtures.
4. In `fleet_host`: the heartbeat client in place of the inbox, the
   `can_place` guard on starts, and `EUSTRESS_SERVER_ID` from the command.

Gates that need McKale: renting any machine, deploying either route, and
minting any `ehost_` credential.

## Implemented in the Worker (2026-11-16)

Items 1 to 3 of the list above are written and tested; item 4 is in "The host agent's heartbeat client" below. Nothing is deployed and no credential is minted.

- `src/fleet_policy.mjs`: the policy ported to JavaScript, pure. `tests/fleet_policy.test.mjs` ports every Rust test in `fleet.rs` by name; `tests/fixtures/fleet_policy_cases.json` holds exact input and output pairs (serde field names) that a Rust test can read later.
- `src/fleet.mjs`: the `Fleet` Durable Object (one instance, `fleet`), Route 1, Route 2, `POST`/`DELETE /api/admin/fleet/hosts/{machine_id}/credential` (mints or revokes `ehost_`, body `{ "region": "us-east-1" }`), and `GET /api/admin/fleet` (machines, room, occupancy, the latest machine recommendation; no secrets).
- `src/fleet_credentials.mjs`: `ehost_` in SOCIAL as specified; the `eshs_` check, which `handleLive` in `src/live.mjs` calls before `verifyAuth`.

Readings where this document was silent:

- `eshs_` records live in the `Fleet` object, not in SOCIAL, so a stop or a freed session revokes at once and the 90 s expiry can move on every `/live` heartbeat without a KV write each time. A start command that is resent carries a newly minted `eshs_` each time; every one stands for the same listing and server and all are revoked together.
- A session whose credentials are revoked leaves its listing in the same heartbeat: an acknowledged stop, a session reported in `freed`, or one the machine no longer reports. Without its credential a server can neither renew nor remove its own `/live` entry, so the heartbeat route takes the entry off (`delistServer` in `src/live.mjs`) and the page never hands out a link nobody answers. The listing is read from the `hostsess:` record the Worker minted, never from the heartbeat, so a host can take off only a session the fleet started on that machine. If taking it off fails, the heartbeat still succeeds and the entry lapses by itself within 90 s.
- What the heartbeat route enforces beyond the request shown above: a session's `listing_id` is 1 to 64 of `a-f 0-9 -`; `machine.region` is empty or equal to the credential's region (the agent sends `null` when no region is set); each list holds at most 256 items and a `rejected[].why` at most 200 characters; a 5xx answer carries `{ "code": "fleet_unavailable", "retry_after_secs": 30 }`, which the agent does not read (it keeps its own 5 to 60 s backoff); and a 401 is also returned when the credential record cannot be read, which is why the agent keeps trying after one.
- An `eshs_` request must name its server: `server_id` in the body of `POST /live`, `?server_id=` on `DELETE /live`. On `POST /live/admit` the ticket's audience must be the pin of that server's own `/live` row. A fleet session never runs guest-only and never admits a guest ticket.
- `request_id` is kept per caller (account, or a hash of the guest lease), so one caller's id never returns another caller's answer. Asking again with a known id is not counted against the 10 a minute.
- A guest allocation is answered only from the listing's guest-only sessions run by hosts; the fleet starts no guest-only session.

What the deploy needs, in order: (1) `wrangler.toml` already has the `FLEET` binding (class `Fleet`) and migration `v10-fleet` in production and staging; deploy staging first. (2) No new secret. (3) An administrator mints each machine's `ehost_` once the machine is rented (McKale's word, per the gates above). (4) On each machine: `EUSTRESS_FLEET_HOST_TOKEN` (its `ehost_`), `EUSTRESS_FLEET_MACHINE_ID` (the id the credential was minted for), `EUSTRESS_API_URL`, and `EUSTRESS_FLEET_MEMORY_MB` where `/proc/meminfo` is absent; then `eustress-forge host`.

Admission is per server. `LiveHost.admit` accepts a ticket whose audience is the certificate pin of a server the listing lists now (a row that has beaten within 90 s), so every session of a listing admits the players sent to it. An `eshs_` caller is held to its own server twice: the route checks the audience against that server's `/live` row, and names the `server_id` to the listing's object, which admits only at that server's pin. The author's own call reaches any of the listing's servers. The rule that one pin is one process (an account session and a guest-only session never share a pin) is tested against every server of the other session (`livePins`), on the heartbeat, on admission, and when a guest ticket is minted. Tests: `tests/live_admit.test.mjs`, `tests/fleet.test.mjs`.

## The host agent's heartbeat client

`eustress-forge host` (`fleet_host.rs`) runs in heartbeat mode when `EUSTRESS_FLEET_HOST_TOKEN` holds an `ehost_` (`ehost_` and 64 lowercase hex digits; anything else stops the agent at start with an error that does not repeat the value). Without it the agent runs its inbox mode, for local testing.

- **Request.** Every `next_heartbeat_secs` (5 s when the answer names none, at most 60 s) it POSTs the Route 1 body to `<EUSTRESS_API_URL>/api/fleet/hosts/{machine_id}/heartbeat`, with a 10 s timeout. `EUSTRESS_API_URL` must be `https://`, or `http://` to `localhost`, `127.0.0.1` or `[::1]`. `machine.region` is `EUSTRESS_FLEET_REGION` (else `EUSTRESS_HOST_REGION`), which must be the credential's region; with neither set it is `null` and the Worker uses the credential's. `agent_version` is the crate version.
- **Idempotency.** A new UUID `boot_id` at every start; `seq` rises by one on every heartbeat sent, answered or not. Every `command_id` handled is remembered for the boot and a repeat is ignored. `acked`, `rejected` and `freed` entries stay in every heartbeat until one is answered `200`, and are never listed after that. A command whose id is not `cmd_` and 1 to 60 letters or digits is dropped, since the Worker would refuse it in either list. At most 256 entries of each list go in one heartbeat; the rest go in the next.
- **Start.** `fleet::can_place` against this machine, as occupied when the command has `for_request` and warm otherwise. A start that would cross a line is listed in `rejected` with the line (`would pass the committed-CPU line`, `would pass the activation line`, `would pass the memory line`). A malformed start, one whose process cannot be started, and a command the agent cannot read are rejected too. Otherwise the server starts with `EUSTRESS_SERVER_ID` = `server_id` (also its status directory), `EUSTRESS_SERVER_TOKEN` = `session_credential`, and the 60 Hz, 5 Hz idle and empty-exit environment; it is acked. A start for a `server_id` already running here is acked without a second process.
- **Stop.** The server's `dedicated.stop`, then acked; a stop for a server not running here, or already stopping, is acked. A stop file that cannot be written is rejected.
- **Failures.** 401 or 403: nothing starts and running servers leave by their empty exit; the heartbeat keeps trying on the backoff schedule, so a credential the Worker could not read for a moment recovers. 400: logged with the Worker's `field`; nothing in the body is changed to make it pass, and the next heartbeat goes at the usual interval. No answer, 5xx, 429, a `200` that is not a heartbeat answer or is for another `seq`, or any other status: wait 5 s, doubling to 60 s; nothing starts; each of the agent's own servers that is hosting, empty, and has been empty and running for the 120 s hold gets its stop file.
- **Secrets.** The `ehost_` is read from the environment only and is sent only in the `Authorization` header; a started server never inherits it (`local_launch::HOST_ONLY_ENV`). An `eshs_` goes only into its own server's environment. `EUSTRESS_SERVER_TOKEN` set on the agent is not given to any server in this mode. Debug output of every type that holds either credential prints `<redacted>`.
