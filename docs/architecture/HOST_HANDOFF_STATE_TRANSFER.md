# Host handoff: state transfer with one owner at a time

Status: the protocol core, stable ids across hosts and the neighbour pick are
built and unit tested in `eustress-networking`. The host-to-host channel, the
engine glue, the rider notices on the player wire and the Worker's shard
fields are specified here and not built. Nothing is deployed.

Decision 5 of `DISTRIBUTION_SHARDING_GPU_AUDIT.md`, taken by McKale on
2026-11-19: an entity that crosses from one host's area into another's (a car
with its riders, or a player on foot) is handed over whole. The source keeps
simulating it until the destination confirms it holds it, then the source
stops. Exactly one host is authoritative at every instant. The entity is never
lost and never simulated twice.

Two hosts never compute the same physics step (audit M1: Avian runs with
`parallel` and without `enhanced-determinism`), so a handoff moves state. It
never relies on two machines predicting the same answer.

## Words

- **Source**: the host that owns the entity now. **Destination**: the host it
  is offered to.
- **Entity**, **root**: the instance a handoff is about (a car `Model`, or a
  character), with everything under it. Riders' characters travel in the same
  snapshot as extra subtrees.
- **Authority**: the right to change the entity's state, which is what
  simulating it means. One host at a time.
- **Custody**: the frozen copy a source keeps after it gave up authority,
  until the destination confirms. It is what makes the entity impossible to
  lose.
- **Handoff id**: unique across hosts; the source's origin in the high 24
  bits and its own count below.
- **Epoch**: per entity. A host that owns it at epoch `e` hands it over at
  `e`; the destination owns it at `e + 1`. A source that takes an entity back
  after it sent a Commit owns it at `e + 2`, past anything the destination
  could have held.
- **Origin**: a host's number in its universe (1 to 16,777,215). 0 is a host
  on its own, which never hands anything off.

## The protocol

```text
source                                          destination
  begin: Offer{id, epoch, snapshot} ──────────▶ checks it, stages it
  (owns it, simulates it)                       (not in its world yet)
                                    ◀────────── Accept, or Refuse{reason}
  Finalize: final snapshot, freeze,
  Commit{id, epoch, final snapshot} ──────────▶ spawns it under the same ids;
  (custody: frozen copy)                        owns it and simulates it from here
                                    ◀────────── Committed
  Release: removes its copy,
  riders reconnect to the destination
```

| Message | From | Meaning |
|---|---|---|
| `Offer` | source | Take this entity. The source still owns and simulates it. Carries the full snapshot. |
| `Accept` | destination | Staged. Send the Commit. |
| `Refuse` | destination | No, with a reason. Answers an Offer, or a Commit it cannot take. |
| `Commit` | source | It is yours as of this final snapshot. The source has frozen it and owns it no longer. |
| `Committed` | destination | It holds the entity and simulates it. |
| `Abort` | source | Forget the Offer. Never sent after a Commit. |

Rules, each with a test in `handoff.rs`:

1. **Authority passes on Commit and nowhere else.** The source gives it up
   when it sends Commit; the destination takes it when Commit arrives. An
   Accept moves nothing: the destination only staged a copy outside its world.
2. **The one trip in between.** While Commit is in flight no host steps the
   entity. Its state is the Commit's snapshot, taken the moment the source
   froze it, so the destination continues exactly from there. The cost is one
   one-way trip of stillness (a frame or two at 60 Hz between neighbouring
   machines). Two hosts stepping it at once would be a duplicate; this is not.
3. **Custody until confirmed.** The source keeps the frozen copy until
   `Committed`. It resends the Commit every 250 ms and never resumes on a
   timer. Only a `Refuse` of the Commit, or the control plane declaring the
   destination gone (`Handoffs::peer_lost`), brings the copy back to life.
4. **Idempotent by id and epoch.** Every message carries both. A receiver
   answers a duplicate from what it already decided and never decides again: a
   duplicate Offer gets the same Accept or Refuse, a duplicate Commit gets
   `Committed` and is never applied twice, a late Accept after an abort gets
   the Abort again, a duplicate `Committed` releases once. A message under
   another epoch, or from a host the handoff is not with, is ignored. A host
   may offer only under its own ids.
5. **Out of order is safe.** An Abort before its Offer is remembered, and that
   Offer is then refused. A Commit with no staged Offer is refused `NotHeld`,
   and the source takes its copy back. A destination that already owns the
   entity at `epoch + 1` answers a Commit `Committed` even after it forgot the
   handoff (the Commit names it, so only it can have taken it).
6. **Stale epochs are refused.** A host refuses an Offer below the newest
   epoch it knows for that entity. A host never reclaims an entity it handed
   away.
7. **Bounded.** A message body is at most 1 MiB (a quarter of the wire's frame,
   the world frame's budget), refused before it is decoded. A snapshot holds
   at most 4,096 instances, 512 bodies and 64 riders; ids unique, parents
   before children, every value finite and within the wire's value limits,
   every body a unit rotation and finite velocities, every rider an account id
   of printable ASCII up to 64 characters.

Timers (`HandoffConfig`, milliseconds of the caller's monotonic clock):

| Timer | Default | On expiry |
|---|---|---|
| `answer_ms` | 1,500 | A source with no answer, or no final snapshot, aborts and keeps owning it. |
| `stage_ms` | 5,000 | A destination drops a staged snapshot that got no Commit. A later Commit is refused `NotHeld`. |
| `resend_ms` | 250 | A source resends its Commit until answered. |
| `remember_ms` | 120,000 | Finished handoffs are forgotten. Longer than the Worker's 90 s heartbeat, so a source still resending is always answered from memory. |

### Refusals

`Full` (no room: the per-listing cost record decides, see below), `IdTaken`,
`Stale`, `AlreadyHeld`, `Busy` (another handoff of it is open there),
`Invalid`, `NotHeld`, `UnknownParent` (it hangs under something the
destination's world does not have), `Closed`. Their wire tags are pinned by
`handoff_tags_never_move`.

## What a snapshot carries

`HandoffSnapshot`:

- `instances`: the entity and each rider's character as `SpawnOp`s, the same
  record a player is sent, built by `HostReplicator::handoff_ops`: class,
  name, parent, every replicated property, attributes and tags. This is the
  script-visible state the replication layer already carries, in the
  replication layer's own `WireValue` encoding. Session peer ids are removed.
- `bodies`: per body, position, unit rotation, linear and angular velocity at
  Avian's `f32` precision (not the motion lane's quantized 40 bytes), the
  `RigidBody` kind, and whether Avian had it asleep. The car keeps moving
  instead of spawning at rest.
- `owner`: the account that owns the entity.
- `riders`: per rider, its account, its `Player` id on the source, its
  character and its seat.
- `tick`: the source's tick, for the source's own logs. Ticks are per process
  (audit M2) and are never compared across hosts.

Messages are framed by `wire::encode_frame` and read by `wire::decode_body`,
so values decode under `MAX_DECODE_DEPTH` (128). There is one codec.

## Ids that stay the same across hosts (audit M2)

How ids are minted (`repl/id.rs`):

- Scene instances hash their record key (the Space-relative path). Every host
  that loads the same world computes the same id, so no table is sent.
- Services hash their class, so `Workspace` is the same id on every host.
- Runtime instances (scripts' `Instance.new`, players, characters, and in
  practice every car a script spawns) get an id the host mints from a counter.
  That counter was the only host-local part.

The change: a runtime id carries the origin of the host that minted it.

```text
bit 63        RUNTIME_BIT
bits 39..62   origin (24 bits): the minting host
bits 0..38    that host's sequence number (2^39 per process)
```

`NetId::runtime_in(origin, n)`, `NetId::origin()`,
`HostReplicator::set_origin`. Origin 0 gives exactly the ids a lone host gave
before (`runtime_in(0, n) == runtime(n)`), so no player, no wire version and
no single-host session changes. Two hosts with different origins never mint
the same id.

An entity keeps its ids through a handoff:

- The destination makes the snapshot's instances through the same path a
  player applies a world frame (`handoff::materialize`, which drives a
  `Replica`) and binds each to the id it had on the source
  (`HostReplicator::adopt`) before the next `observe`. Its players are then
  sent the entity under those ids, and so is a late joiner.
- A host's own counter skips any id it adopted, so a car that comes back to
  the host that first minted it keeps its ids there too.
- A destination refuses (`IdTaken`) rather than bind one id to two instances.
  That covers two hosts left at origin 0, and a scene instance whose record
  key exists in both Spaces.
- Players are matched across hosts by account, never by session peer id. The
  rider's `Player` id travels in the snapshot so the destination can give the
  `Player` it makes on reconnect the same id (engine glue, below).

Still host-local, recorded here so nobody relies on them: scene ids do not
include the Space name, so a scene instance can cross only into a Space with
no record at the same path (refused `IdTaken` otherwise); ticks are each
process's own clock. Space-qualified scene ids need the player to hash with
the Space too, which is a wire change (Welcome carries the Space key, protocol
version bump); it waits until a scene-placed entity must cross Spaces.

Who assigns origins: the control plane, once per server process, from a
counter per universe that never reuses a value (an origin reused while the
entities it minted live elsewhere would mint over them). Until that exists a
shard host is configured with its origin; a collision is caught by `IdTaken`,
never applied.

## Failure table

| What fails | When | Result | Test |
|---|---|---|---|
| Destination crashes | before it answers the Offer | Source never stopped simulating; aborts at `answer_ms` and owns it. | `a_destination_crash_before_commit_leaves_the_source_owning_it` |
| Destination crashes | after Accept, before Commit arrives | Source is frozen in custody, resends Commit, never resumes on a timer; the control plane declares the destination gone; source resumes at epoch `e + 2`. | same |
| Destination crashes | after it took the Commit, before `Committed` arrived | As above: the source resumes from the Commit's state. The destination's few ticks die with it, the same limit as any host crash. Never two owners, since the destination is gone. | same |
| Destination crashes | after `Committed` arrived | The entity was the destination's and dies with that host, as everything on a crashed host does. | (host crash) |
| Source crashes | after the Commit arrived | Destination owns it and keeps it; nothing on the destination waits on the source. | `a_source_crash_after_commit_leaves_the_destination_owning_it` |
| Source crashes | before its Commit arrived | Destination never owned it; drops the staged copy at `stage_ms` or on `peer_lost`. The entity dies with the source, the host-crash limit of `host-migration-design.md`. | `a_source_crash_before_commit_drops_the_staged_copy` |
| Destination refuses | Offer or Commit | Source owns it (resumes it if it was frozen). | `a_refusal_leaves_the_source_owning_it`, `a_source_crash_before_commit_drops_the_staged_copy` |
| Final snapshot bad | at commit | Source aborts and owns it. | `a_bad_final_snapshot_aborts_and_the_source_keeps_it` |
| Commit duplicated | any time | Answered `Committed`; applied once; one Release. | `a_duplicate_commit_is_answered_and_never_applied_twice`, `a_commit_remembered_no_longer_is_still_answered_committed` |
| Messages late, duplicated, reordered, from the wrong host, under another epoch | any time | Answered from the decision already made, or ignored. | `duplicate_and_late_messages_are_idempotent`, `out_of_order_and_foreign_messages_change_nothing`, `a_stale_epoch_is_refused` |
| Control plane wrong (declares a live destination gone) | after Commit | Both would own it. Prevented by fencing, not by this protocol: a host the control plane declared gone has lost its credential and its listing row, and must stop simulating entities it took by handoff once its heartbeat is refused. A late `Committed` after a resume is reported as an `Anomaly`. | (fencing, not built) |

## Riders: back through Worker admission, with a ticket issued up front

Audit section 10 step 9: keep the round trip, make it fast. Per-process pins
and single-use tickets stay. Everything the Worker needs for signed-in riders
already exists:

- `POST /api/identity/ticket {audience}` mints a ticket for the caller's
  account bound to any certificate pin, valid 10 minutes, with a random id.
- `POST /api/simulations/{id}/live/admit` admits it only when its audience is
  the pin of a fresh server of that listing (`admissionPins`), and spends the
  id once.

The flow:

1. **Offer.** `begin` returns `PrepareRiders { to, riders }`. The source sends
   each rider a notice naming the destination's link and pin. The rider's
   Player calls `POST /api/identity/ticket` with that pin at once and holds the
   ticket, while the car is still on the source. It may also check the pin
   against the listing's own `GET /live` rows.
2. **Committed.** `Release { to, riders }`. The source tells each rider to go.
   The rider opens the destination's link with the pin and says `Hello` with
   the held ticket. The destination admits it through `/live/admit` exactly as
   a gallery join: the round trip is kept, and no ticket was minted on the
   critical path.
3. **On the destination.** The admission names the account. The engine glue
   gives the `Player` it makes for that account the carried `Player` id
   (`adopt`), sets its `Character` to the character that arrived in the
   snapshot, and seats it. A rider who does not arrive within the disconnect
   grace a session already gives is treated as one who left.

If the handoff aborts, the held tickets are never shown and expire in 10
minutes. A rider who is a guest has no identity ticket: guest join tickets are
minted only for the pin of the guest session (`livePin`), so a guest cannot
cross yet. Guests crossing is a Worker change (a guest join ticket for a
named server of the same listing, from the same lease) and waits until guests
play on shard hosts.

Cost of the cut a rider sees: the destination's world must already be on the
rider's machine. Same listing and Universe: the world is content-addressed and
cached, so `RequestChunks` asks for nothing. Another Space: the client
prefetches it while it approaches the boundary; that prefetch belongs to the
streaming work and is not built here.

### The Worker side, specified (not written)

Not written because `live.mjs` is under active edit by other work and these
fields have no producer until a shard job exists. Exact change:

1. `LIVE_OPS.beat` accepts two optional fields and stores them on the server
   row: `area` (1 to 64 of `[A-Za-z0-9_./-]`, the area this server owns, such
   as `france/lyon`) and nothing else from the host. A row's `origin` is
   assigned by the object, not the host: on a server id's first beat, the
   lowest value never handed out in this session object, kept in storage as
   `next_origin` and never reused; carried for the life of that server id.
2. `publicServer` passes `area` and `origin` through on rows that show their
   link. `rankServers` is unchanged and remains the only ranking.
3. Tests: a beat with a bad `area` stores none; two servers get different
   origins; a restarted server id (new pin) gets a new origin; `GET /live` rows
   carry both; ranking order is unchanged by the new fields.

## Choosing the neighbour

McKale: "most qualified (ping and power) ranked hosts can host. Do not go
against what is already there." Who may host and how servers are ranked do
not change. `handoff::pick_neighbour(rows, area, me)` takes the listing's rows
in the order `GET /live` returns them (`rankServers`: not full first, then
`0.5 fullness + 0.3 tier + 0.2 ping`) and returns the first row that serves
the target area, is not full, is not this server, and has an origin, a link
and a 64-hex pin. It never re-sorts. `the_neighbour_is_the_workers_first_choice_for_the_area`
holds it to that.

## The seam for streaming and interest management

The streaming, simulation LOD and job-graph work is designed elsewhere and is
not touched here (`common/src/streaming/**` unchanged). It asks for a
handoff through one type:

```rust
pub struct HandoffRequest { pub root: NetId, pub area: String }
```

meaning "`root` is leaving this host's area for `area`". The session glue
answers it: `pick_neighbour`, then `snapshot_of` plus bodies from Avian and
riders from seats, then `Handoffs::begin`. What streaming must provide: the
decision that an entity crossed (on a lane-graph edge, per
`world-shards-design.md`), early enough that the destination is warm, and
the destination's world prefetch for riders. What it gets back:
`Handoffs::holds` and `frozen` to skip an entity in physics and AI, and
`Ended` outcomes.

## What is built

All in `eustress/crates/common/eustress-networking`:

- `src/handoff.rs` (new): `Handoffs` (a Bevy resource; `claim`, `forget`,
  `begin`, `commit`, `on_message`, `tick`, `peer_lost`, `holds`, `frozen`,
  `authority`, `epoch_of`), `HandoffMsg`, `HandoffSnapshot`, `HandoffBody`,
  `Rider`, `Refusal`, `encode_message` / `decode_message`, `pick_neighbour`
  and `LiveRow`, `HandoffRequest`, `snapshot_of`, `fits`, `materialize`, and
  the tests below.
- `src/repl/id.rs`: origin-tagged runtime ids (`runtime_in`, `origin`,
  `ORIGIN_BITS`, `SEQ_BITS`, `MAX_ORIGIN`).
- `src/repl/host.rs`: `set_origin`, `origin`, a mint that skips adopted ids,
  `id_free`, `adopt`, `handoff_ops`.
- `src/repl/replica.rs`: `bind_net`, for the parents a snapshot hangs under.
- `src/lib.rs`: `pub mod handoff`.

Tests:

```text
cargo test -p eustress-networking --lib -- handoff::
cargo test -p eustress-networking --lib -- repl::id::tests::runtime_ids_carry_their_origin
cargo test -p eustress-networking --lib -- repl::
```

The last line is the whole replication suite, which the id change must leave
green (origin 0 keeps every id it minted before).

## What waits

- **Interest management (M3).** Which host's area an entity is in, the
  overlap and lane-graph edge where it crosses, keeping the destination warm,
  and sending a destination's players only what is near them. Until then
  nothing produces a `HandoffRequest`.
- **The per-listing cost record (B5).** `Refusal::Full` is the destination's
  answer from its own room, which needs measured threads and memory per
  session. Placing a shard's neighbour at all waits on B5 as well (no shard is
  placed before it exists).
- **The host-to-host channel.** A separate QUIC port for shard peers with its
  own pin (`world-shards-design.md`), never 7777 and never the gallery join.
  `HandoffMsg` frames ride its reliable stream.
- **Engine glue.** Avian state in and out of `HandoffBody`, freezing a
  `frozen` entity in physics and scripts, seats, the rider notices
  (`ToPlayer` variants for prepare and go, with a `PROTOCOL_VERSION` bump),
  adopting a reconnecting rider's `Player` id, and calling
  `HostReplicator::set_origin` from the configured origin.
- **The Worker fields** above, and fencing: a host whose heartbeat is refused
  stops simulating what it took by handoff.
- **Space-qualified scene ids**, when a scene-placed entity must cross Spaces.
- **Deltas.** The Commit carries the full final snapshot. A delta against the
  Offer is the size optimisation the shard design describes; a car with its
  riders is kilobytes, well inside the 1 MiB bound.
