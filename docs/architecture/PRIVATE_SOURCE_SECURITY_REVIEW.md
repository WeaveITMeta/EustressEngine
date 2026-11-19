# Private source world: Security Review

**Author:** the implementer of decision 1 (Opus), for the orchestrator, 2026-11-19. This is the builder's own review of a
credential scope change; it is written for Security to check, not in place of Security's ruling.
**Decision it implements:** McKale, 2026-11-19, decision 1 = (b) and (c) of
`docs/architecture/DISTRIBUTION_SHARDING_GPU_AUDIT.md` section 11, and decision 3 as McKale worded it.
**State:** in the working tree on `staging/integration`. Nothing is deployed. The Worker half is tested; the Rust half is
written and not compiled (the orchestrator owns cargo).

## 1. What changed

A listing has two worlds: the player world (the manifest every player downloads) and the source world (the player world
plus the server-only services and every server Script's code). Studio commits the source world on every publish. Share
Source no longer decides whether a source exists; it decides who reads it.

| Share Source | Who reads the source world and its server-only chunks |
|---|---|
| On (public source) | Anyone the listing's own gate lets through, as before: the listing listable (approved, public, not deleted or locked), the adult content gate passed, and the source the one its review bound |
| Off (private source) | The author: their signed-in session, their `esrv_` server credential for this listing, or an `eshs_` session credential the fleet minted for this listing. An admin. Nobody else |

And, in both cases: a listing that is deleted (its R2 deletion mark) or under a legal lock (either kind) gives its source
to an admin only.

The fleet starts no hosted session for a listing that has no source world for the world it plays now. A session never runs
with only the player world.

Files:

- `infrastructure/cloudflare/api/src/world.mjs`: `hasSourceWorld`, `credentialAuthor`, `closedListing`, `sourceReader`
  (replaces `mayReadSource`); `servableListing` accepts `eshs_`; the source commit no longer needs Share Source; a source
  is kept across a re-commit of the same world whatever Share Source says; private source chunks are `no-store`.
- `infrastructure/cloudflare/api/src/fleet.mjs`: `handleAllocate` passes `hostable` (a source world, and the listing
  neither deleted nor legally locked); the `Fleet` object answers `no_source` instead of starting anything, records no
  warm pool demand for such a listing, and re-reads the listing before it sends any start command (`listingHostable`).
  Allocation follows the Quick Join rank for every live server (section 6).
- `infrastructure/cloudflare/api/src/moderation.mjs`: `serverCodeInPackage`; the review's package facts say the package
  holds server code whenever a source world was committed.
- `eustress/crates/engine/src/space/echk_publish.rs`: the server export always runs and the source always commits; a
  webhook URL in a private source is a note to the author, not a refusal (it stays a refusal in anything public).
- `eustress/crates/engine/src/space/gallery_edit.rs`: `player_world_fallback_allowed`, `NO_SOURCE_FOR_SESSION`; a
  server holding an `eshs_` never falls back to the player world.
- `eustress/crates/engine/src/server_mode.rs`: a world that cannot be downloaded writes `host-failed` with the reason to
  the status file before the process exits.

## 2. Who can read a private source

Every path that serves source bytes is `GET /api/simulations/{id}/world/source` (the manifest) and
`GET /api/simulations/{id}/world/chunks/{hash}` for a hash only the source manifest names. Both run the listing's gate
(`servableListing`) first, then `sourceReader`:

1. Deleted or legally locked: an admin, nobody else. The deletion mark arrives on the listing through `simForServing`
   (`withDeletionMark`); the legal lock is read fresh from R2 on each request (`legalLockOf`), so a hold applied seconds
   ago is seen.
2. The viewer is the author: their session (through `verifyAuth`), or a credential that stands for them on this listing
   (`credentialAuthor`). A credential (`esrv_`, `eshs_`) is never passed to `verifyAuth`.
3. Share Source on: anyone who passed the gate.
4. An admin.
5. Anyone else: 404, the same answer as a listing with no source, so a private source cannot be told from none.

A source-only chunk additionally needs, for a public reader, a source the review bound (`sourceApproved`). The author's
side reads the source as committed: the author owns it, and their own server and their hosted sessions run it.

The play-token path (`chunkByPlayToken`) serves only chunks the player manifest names, so it never serves a source-only
chunk.

**Tested** (`tests/private_source.test.mjs`): the author reads; an anonymous request and another account are refused;
an `eshs_` for listing A reads A; an `eshs_` for listing B is refused on A; an expired, never minted or malformed `eshs_`
is refused, as is any `eshs_` when no fleet binding exists; a deleted listing and both kinds of legal lock refuse the
session and the author and serve an admin; Share Source on still reads publicly. `tests/world.test.mjs` covers the commit
with Share Source off and the source kept as private when Share Source goes off.

## 3. The session credential: scope, expiry, binding

`eshs_<64 hex>` is minted by the `Fleet` Durable Object when it sends a start command to a host agent
(`fleet.mjs`, `heartbeat`). Only its SHA-256 is stored, in the object, as
`hostsess:<hash> = { v: 1, sim_id, machine_id, server_id, expires_at }`.

- **Listing binding.** `sim_id` is the listing the Worker itself planned the start for (`planStart`, from the allocation
  or the warm plan), never a value the host sent. Every check goes back to the object (`session_credential`), which
  compares the stored `sim_id` with the listing in the request URL. A world read names no server, so it is checked with
  `server_id: null` (`sessionServerOf`): any server of that listing. The `/live` routes still bind the server id too.
- **Expiry.** 600 s from the mint until the session's first accepted `/live` heartbeat (`SESSION_CREDENTIAL_START_MS`),
  then 90 s after each accepted heartbeat. Revoked at once when the host acknowledges a stop, reports the session freed,
  or stops reporting it. A start that is resent carries a new `eshs_`; all of a server's credentials are revoked together.
  A world read does not extend the credential.
- **What it can do.** Heartbeat and stop its own server's `/live` entry; admit a player to its own server's pin; and now,
  read its listing's world: the player manifest, the source manifest, and every chunk either names. It cannot publish
  (the author routes use `verifyAuth`, which an `eshs_` never satisfies), mint any credential, read another listing,
  read a deleted or legally locked listing's world at all (`credentialAuthor` returns null for those), or act after its
  session ends.
- **What it stands for.** The author, on reads of this one listing's world, exactly as the author's own `esrv_` does. That
  is what lets a hosted session download a listing that is not yet approved (audit M8); such a session admits its author
  only (`handleLive`, the `not_available` rule), so nobody else plays an unreviewed world.

## 4. Who may host, and how a ranked host gets the source

Decision 3 (McKale, 2026-11-19): "I already had it so most qualified (ping and power) ranked hosts can host. Do not go
against what is already there." Nothing in this change, or in the 11-19 audit's changes, limits hosting to machines
Eustress owns. A machine hosts fleet sessions when an administrator mints it an `ehost_`; the fleet never asks who owns
it. Players are sent by the Quick Join rank (`rankServers` in `live.mjs`: not full first, then 0.5 fullness, 0.3
hardware tier, 0.2 ping, then players), and `handleAllocate` follows that rank exactly, for a creator's own host and a
fleet session alike (section 6).

A ranked third-party host gets a private source this way, and no other:

1. Its agent heartbeats with its `ehost_` and receives a start command carrying a fresh `eshs_` for one listing and one
   server id.
2. The agent gives that `eshs_` to the one engine process it starts (`EUSTRESS_SERVER_TOKEN`, that child's environment
   only; never to another server, never logged).
3. The engine downloads `/world/source` and every chunk with `Authorization: Bearer eshs_...`, checks each chunk against
   its BLAKE3 name, and writes the Universe to that server's own `world/` directory.
4. When the session ends, the credential is revoked and the agent deletes that `world/` directory.

What the host can do with it: the files are plain records on the operator's disk while the session runs, and the
`eshs_` is in a process environment the operator can read. An operator can therefore read, copy and keep the listing's
server code, and can use the `eshs_` for its short life to download the same source again. The credential bounds the
**time** and the **listing**; it does not stop the operator of a machine from reading what runs on that machine. No
technical control can, short of attested hardware. Against a third-party operator, a private source is protected by
the host terms and by who is minted an `ehost_`. The operator cannot reach any other listing's source, cannot reach this
listing's source after the session, and cannot publish anything.

## 5. Caching

- The source manifest: `Cache-Control: private, no-store`, shared or private.
- A chunk only the source names, from a private source: `private, no-store`, so neither a shared cache nor the browser
  keeps it.
- A chunk only the source names, from a shared source on a listable listing: `private, max-age=86400, immutable`, as
  before. `private` keeps it out of every shared cache.
- No `/world/` route uses the platform cache (`caches.default` is used only by `body_cache.mjs`, for computed listings),
  and no response carries `public`.

## 6. Decision 3 audit: the 11-19 changes against "most qualified ranked hosts can host"

Each change the 11-19 audit made was read for two things: does it narrow who may host, and does it route a player
anywhere but where the rank says.

| Change | Narrows who hosts? | Bypasses the rank? |
|---|---|---|
| `fleet.mjs` `loadView`: a reservation stops counting once the host has reported the arrival (H2) | No | No: it changes a seat count, not the order |
| `fleet.mjs` / `live.mjs` `arrived` op | No: called only for an admission made with that session's own `eshs_` | No |
| `forge/src/fleet_host.rs`: own `world/` per server, `remove_world`, `seats` (B1, H2) | No | No |
| `forge/src/studio_compat.rs`: a deployment needs Studio's key; a listing id must be a token (H6) | No: Studio's own Forge path still deploys; only a web page without the key is refused | No |
| `engine/src/dedicated.rs`: `server_download_dir`, `seated` | No | No: the `/live` heartbeat still carries `latency_ms` and `hardware`, the rank's inputs |
| `engine/src/server_mode.rs`: per-server directory, every open connection counts as someone to serve | No | No |
| `networking/src/session.rs`: `Hello` deadline, frame cap before admission, `seated_count`, decode depth | No: they bound what a connection may do, not who hosts | No |

None narrows who may host or bypasses the rank.

One older line did bypass it, and is fixed: `handleAllocate` walked the ranked rows but skipped an empty fleet session
("an empty fleet server is a warm server: the policy below assigns it"). When an empty fleet session outranked an empty
creator's host (better tier and ping), the player went to the lower ranked creator's host. Allocation now takes the first
row that has a seat, whoever hosts it, and an empty fleet session it takes is answered `warm: true`. Test: "the Quick
Join order decides: an empty fleet session ranked above an empty creator host gets the player" (both orders).

Observed, not changed: the placement policy (`fleet_policy.mjs`, owned by another worker today) chooses which machine
starts a new session by room on the machine, and the host agent's heartbeat carries no latency or hardware. The rank
applies to sessions once they are live. That is the design as written; it is noted so nobody reads the rank into the
start decision.

## 7. Deleted and held listings

- **Deletion mark** (`space_delete_mark.mjs`, `locks/deleted/{id}.json`): the listing reads as private with `deletion`
  set. The source is refused to everyone but an admin; an `eshs_` is not a credential at all on it, so a hosted session
  reads neither world. `handleAllocate` starts no session for it, even for its author (`409 not_hostable`).
- **Legal lock, serve block** (`locks/legal/{id}`, `serve_block`): the listing reads as quarantined, which already refused
  everyone but an admin; the source check refuses again.
- **Legal lock, content lock** (`content_lock`): the listing reads as held, so its author still passes the listing's gate.
  The source check is what refuses: the author and their sessions get no source while it holds. An admin does. The
  fleet starts no session under either kind of lock (`409 not_hostable`, which does not say why).
- **A review hold** (not legal): the author and their credentials still read their own source, as with every
  unapproved listing.

## 8. Findings and open items

**F1. Since 2026-11-12 the player world carries server code (HIGH, a fact about the premise, not a defect of this
change).** `echk_export.rs` (`Audience::Players => records`, "the whole Space, server code included", McKale's option A
of 2026-11-12) puts every record of a Space, ServerScriptService and server Script code included, in the player world
that every player downloads. So for any listing published from a Studio after that date, a private source keeps nothing
from players: its code is already in the player world. What the private source gives today is a guaranteed, complete
server world for hosted sessions and the author's own Edit. It keeps server code confidential only if the player world
stops carrying it. The audit's B4 failure scenario ("runs with zero server scripts") holds for listings published before
2026-11-12 and for any player world without the server services; the fleet's refusal covers both. McKale should know
this before anyone tells an author that Share Source off keeps their server code private.

**F2. A source committed after approval runs on hosted sessions without review (MEDIUM, open).** A source-only commit
does not send the listing back to review (`handleCommit`: the source "changes nothing they play"). Public readers get
such a source only after a review binds it (`sourceApproved`), but the author's side, hosted sessions included, reads it
at once, as the author's own server always has. Options: (a) keep, and rely on the server sandbox; (b) send a listing
whose source changed back to review, which delists it until approved; (c) give an `eshs_` only a bound source on a
listable listing, which leaves a server-script-only update unhostable until the next world change. Security's call.

**F3. The review reads private server code (disclosure, open).** `package_scripts.mjs` reads the source package whenever
one is committed, and the judge and script reader send it to their model providers. With Share Source off this is new:
the author's private server code goes to the automated review. The privacy notice and publishing terms should say so.

**F4. Third-party host operators can read what they host (accepted by design, section 4).** Host terms, and who is minted
an `ehost_`, are the control.

**F5. Each world read with an `eshs_` is one call into the one `Fleet` object (LOW, operational).** A world of N chunks
costs N + 2 object calls per session start. Fine for Pong; for a city world, either a short per-isolate cache of the
positive answer (it would delay a revocation by its lifetime) or a world-read grant minted once per start. Measure first.

**F6. Listings published from an older Studio with Share Source off have no source world (LOW, expected).** The fleet
answers `409 no_source_world` for them until the author publishes from a current Studio. The fleet is not deployed, so
nothing live changes.

**F7. A webhook URL in a private source is a note, not a refusal.** Players cannot download a private source, so it is
not refused, but every host of the listing reads it (F4), so Studio tells the author and names the files. Under F1 the
same file may already be in the player world, where the refusal still applies.

## 9. Tests and checks

Worker (`npm test` in `infrastructure/cloudflare/api`):

- `tests/private_source.test.mjs`: 8 tests, as listed in section 2.
- `tests/fleet.test.mjs`: "the fleet starts no session for a listing with no source world; the creator's own host of it
  is still joined"; "the fleet starts no session for a listing that is deleted or under a legal lock, even for its
  author"; "a start for a listing whose source world is gone by the next heartbeat is never sent"; "the Quick
  Join order decides: an empty fleet session ranked above an empty creator host gets the player". The fixture's listings
  now carry a source world.
- `tests/world.test.mjs`: the source test now commits with Share Source off and checks it is private.

Mutations run (each restored in place afterwards):

- `sourceReader` with the deleted or locked check switched off: the deletion and lock test fails ("the author").
- `handleAllocate` with the warm skip put back: the Quick Join order test fails (`'home' !== 'w1'`).
- `handleAllocate` passing `hostable: true` always: the no source world test fails (it got `starting`).

Rust (not compiled; the orchestrator runs these):

```text
cargo test -p eustress-engine --lib -- a_hosted_session_never_falls_back_to_the_player_world a_webhook_in_a_private_source_is_a_note_naming_files_only a_world_that_cannot_be_downloaded_reads_as_host_failed_with_its_reason
```
