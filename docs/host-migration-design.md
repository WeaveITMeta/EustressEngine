# Host migration

Status: **design only.** This branch does not move a running game from one
machine to another. It records the spec. The two cheap hooks (a job id that
is not the host, and a snapshot/session shape) are named below and are not
in the tree. No Worker deploy, and no port, DNS, or firewall change.

World shards (many hosts, one universe, one city space per host) are a
different document: `docs/world-shards-design.md`. Migration moves **one**
job. Shards are **many** jobs, one per city. Paris is the shard target, not
the migration target. A city is not cut into slices unless one host cannot
carry it.

## Spec

When a self-hoster closes their server, the job migrates to a new host
without losing the game.

Forge Orchestration 0.6.0 is the foundation for routing and handoff. In this
repo that crate is the one `eustress/crates/forge` depends on (0.6.0 from
crates.io). `Forge::submit_job` forwards a job to Nomad only when a Nomad
client was configured. `POST /api/v1/jobs` on the control plane stores the
job. Neither call moves a live process. A later branch uses that job record
as the thing that is handed off: the job id stays, the machine changes.

Game state moves as incremental snapshots. A handoff streams the deltas
since the last snapshot, so the payload is kilobytes, not the whole Space.

Target: the new host is simulating again in **under one second**. The only
cost a player can see is the network switch: the new host opens its port and
the listing shows it.

## Networking notes

These are constraints on any implementation, not a design that is built.

(a) Under one second needs a **warm standby** that is already receiving the
snapshot stream. A cold start (download the world, open the database, bind
the port, mint a certificate) cannot finish inside that second. The standby
is another process of the same job id, not a second listing.

(b) The orchestration handoff is a network round trip. That is milliseconds,
not microseconds. The budget is the player's reconnect plus that round trip,
not a shared-memory copy.

(c) Clients need a reconnect path to the **new address** that keeps the same
session key and the same certificate pin, and does not go back through the
website. Today a join link is `eustress-player://join/<host>:<port>?key=…&pin=…`
and the pin is the SHA-256 of that process's certificate
(`eustress/crates/common/eustress-networking/src/native.rs`). A migrated host
that mints a new certificate breaks the pin. The later branch has to keep
the key and the pin stable across the move, and teach the Player to dial the
new address with those same values. That dial is not implemented.

(d) A **graceful** close can hand off: the host finishes a snapshot, then
stops. A **crash** loses whatever changed since the last snapshot. There is
no promise of zero loss on a kill.

## Hooks this design wants

The local launcher mints a UUID job id and passes it to the process as
`EUSTRESS_SERVER_ID`. The heartbeat sends that string as `server_id`.
There is still no `eustress/crates/forge/src/handoff.rs`, and no
`JobId` / `SessionHandoff` / `SnapshotDelta` record. Those do not start a
migration.

**Job identity is not the host.** A Forge job id is its own string. It is
not a process id, not a UDP port, and not an IP address. `eustress-forge`
mints that id when the job is created. Replacing the process would not
rename the job. Nothing yet replaces the process.

**Snapshot and session shape.** A later file would name three records
and would not fill them from a running game:

- `JobId` — the stable id.
- `SessionHandoff` — job id, session key, certificate pin, and the current
  address. The address is the field that changes. The key and the pin are
  the fields that must not.
- `SnapshotDelta` — job id, the sequence the delta starts after, the
  sequence it ends on, and the bytes. Empty bytes are a legal delta. Nothing
  writes or streams one.

## What is explicitly not built

- No standby process, no snapshot stream, no address flip.
- No Player reconnect except the join path that already exists.
- No change to certificate lifetime (still a minute under 14 days, minted
  per process today).
- No deploy.
