# 03 - Multiplayer

> Replication, transport, Studio dev servers, Team Create, the published-simulation
> join path, and the dual-format storage contract that all three consume.
>
> **State: no multiplayer exists.** Two half-stacks are present, neither wired.
> Zero packets move between any two processes.

## Pass changelog

- **P1 (2026-05-14):** 11 feature rows; 56 R + 42 I + 15 wiring gaps.
- **P3 (2026-05-14):** Networking row upgraded per critic; JWT escalated P0.
- **P4 (2026-05-14):** Retrofit to per-feature-card format. 16 cards.
- **2026-05-16:** Storage pivot recorded (`.pak` to `.eustress`).
- **P5 (2026-08-28):** **Full rewrite against code.** Prior passes were forecast
  documents scored from design intent; several ratings were wrong in the
  optimistic direction. Transport regraded from green to absent. Team Create and
  Studio dev servers added, both missing from every prior pass. Storage contract,
  transport selection, and an Alpha-blocker list added. Findings below are
  code-verified with `file:line`, or marked otherwise.

---

## Verdict

Multiplayer is not partially built. It is two disconnected halves, each missing
the part the other has.

| Half | Has | Lacks |
| --- | --- | --- |
| `eustress-networking` | Ownership arbitration, AOI, delta tracking, prediction and interpolation scaffolding | Any transport. Any entity ever tagged for replication. |
| `engine/src/play_server` | A real quinn QUIC endpoint, a real `GameMessage` protocol with per-variant channels | An accept loop, a join handshake, replication tagging, a reachable trigger |

Neither is reachable from a running build. Studio does not link the first, and the
second is gated behind an enum variant nothing constructs.

---

## Feature index

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Transport (QUIC / replication substrate) | RED, absent |
| 2 | Entity replication tagging | RED, architectural hole |
| 3 | Ownership arbitration | GREEN logic, unreachable |
| 4 | Studio dev server (press Play, second client joins) | RED, dead button |
| 5 | Team Create (collaborative editing) | RED, does not exist |
| 6 | Client join from published simulation | RED, chain broken in 5 places |
| 7 | Dedicated server (`crates/server`) | RED, cannot open a world |
| 8 | Dual-format storage (`.pak` TOML + `.eustress` Fjall) | AMBER, ships both, drifts |
| 9 | Identity and join tokens | RED, P0, plus a forgeable-identity defect |
| 10 | Publish and download pipeline | AMBER, works, leaks, cannot update |

---

## Feature 1 - Transport

**State:** absent. Not partial.

`crates/common/eustress-networking` is 5,140 lines that never open a socket.

- The crate declares `lightyear` and imports it **zero times**. No `quinn`, no
  `UdpSocket`, no `TcpListener` in any of its 13 files.
- `src/transport.rs` (300 lines) is `rcgen` certificate generation plus a
  bandwidth counter. There is no endpoint, listener, or connection in it, despite
  a module doc claiming "QUIC transport with TLS 1.3".
- `server.rs:189` marks the server `Running` under a comment that Lightyear setup
  "would go here". `process_connections` and `process_disconnections` have empty
  bodies.
- `server.rs:318` computes real deltas, discards them, and records an estimated
  byte count.
- `client.rs:228` sets state to `Syncing` under "simulate immediate connection".
  `sample_and_send_input` builds a real `PlayerInput`, buffers it, discards it.
  `handle_state_updates` is empty.
- `p2p.rs:438` is a stub. The `p2p` feature is off: `bevy_quinnet` has no Bevy
  0.19 release.

**The version trap.** `eustress/Cargo.toml:73` pins `lightyear = "0.19"`. That is
a coincidence of numbering, not a Bevy 0.19 statement. lightyear 0.19 targets Bevy
0.15, three majors stale. The crate manifest comment reading "Lightyear 0.17 (Bevy
0.17 compatible)" is the same misreading recorded a second time. Anyone treating
that pin as "already on the right version" is wrong by four Bevy releases.

---

## Feature 2 - Entity replication tagging

**State:** architectural hole. This is the finding that makes Feature 1 moot.

The only two sites in the workspace that **originate** a `Replicated` component are
`eustress-networking/src/physics.rs:308` and `:326`, tuple-inserted as
`crate::replication::Replicated::default()`.

`server.rs:264`'s `insert(Replicated::new(net_id))` runs inside `assign_network_ids`,
whose query is filtered `With<Replicated>`. It can only re-stamp an entity that is
already tagged. It can never originate one.

`physics.rs` sits behind `#[cfg(feature = "physics")]` (`lib.rs:55`), and `physics`
is not in `default = ["server", "client"]`.

**Therefore: in every build that exists, zero entities are ever marked
`Replicated`.** A perfect transport dropped in tomorrow would replicate nothing.
Any plan that stops at "add a transport" does not produce multiplayer.

Neither originating function has a caller even when the feature is enabled.

---

## Feature 3 - Ownership arbitration

**State:** the logic is genuinely good and currently unreachable.

`eustress-networking/src/ownership.rs` (689 lines) implements arbitration with
locks, distance checks, transfer cooldown, ping-based contention resolution, and
`NetworkOwnershipRule { ServerOnly, ClientClaimable, SpawnOwner, Inherit, LocalOnly }`.
It is transport-independent: it manipulates a `NetworkOwner` component and messages,
nothing else.

This file should survive any transport decision. It is the best-engineered code in
either half-stack, and no candidate library ships an equivalent.

---

## Feature 4 - Studio dev server

**State:** a fully wired dead button.

Pressing Play in Studio is 100% in-process. `handle_start_play` (`play_mode.rs:628`)
has a Solo arm that only enables physics and a WithCharacter arm that spawns a
character and camera. Neither touches a socket or spawns a process.

The server path exists and cannot be reached:

- `PlayServerPlugin` is added unconditionally (`app_core.rs:220`). It is a real
  `quinn::Endpoint::server` (`play_server/server.rs:146`) with a real `GameMessage`
  protocol and per-variant channel classification.
- Its trigger system `start_play_server_if_server_mode` **is** registered
  (`play_mode.rs:1719`, `OnEnter(PlayModeState::Playing)`) and runs on every Play.
- Its guard is `play_type == PlayModeType::Server` (`play_mode.rs:2397`). Nothing
  constructs that variant. The only `StartPlayEvent` writers produce `Solo`
  (`play_mode.rs:1606`), `WithCharacter` (`:1610`), and `PlayModeType::default()`,
  which is `Solo` (`simulation/plugin.rs:588`).
- `accept_connection` (`play_server/server.rs:186`) has zero call sites.
  `PlayServer::start` binds the endpoint and returns without looping on
  `endpoint.accept()`. `pending_connections()` clones rather than drains, so
  `PlayerConnectedEvent` would refire every frame. `incoming_messages` is never
  written.

F9 and the ribbon reach `SlintAction::StartServer` (`slint_ui.rs:2173`), which
writes `MenuActionEvent` (`slint_ui.rs:6065`), and then nothing happens.
`keybindings.rs:1889` deliberately leaves the match arm empty on the documented
grounds that `StartServer` and `StopServer` are "owned by the networking plugin",
a plugin the engine does not compile. The button is not unfinished. It is wired to
an owner that was removed.

**Security note, before this is ever made reachable.** `play_server/server.rs:145`
binds `0.0.0.0`, with a self-signed cert and `with_no_client_auth`. That must
become `127.0.0.1` with an explicit opt-in toggle before the guard is ever
satisfiable. The pattern to copy already landed for the stream node, which now
defaults to `Ipv4Addr::LOCALHOST` and requires `bind_all()` for off-host reach
(`stream-node/src/config.rs:46`).

---

## Feature 5 - Team Create

**State:** does not exist at any layer. No prior audit pass covered it.

1. Studio does not link the networking crate. `engine/Cargo.toml:98` has the
   dependency commented out.
2. `ui/slint/collaboration.slint` (277 lines) is imported by `main.slint` and
   **never instantiated**. Searching the Slint tree for `Collaboration {` returns
   nothing. Toggling the panel renders empty space.
3. View > Collaborate (`ribbon.slint:1137`) and `ToggleCollaboration` on
   Ctrl+Shift+L (`keybindings.rs:348`) flip a bool nothing reads.
4. `CollaborationState { connected, users, room_id }` is `init_resource`d twice
   (`slint_ui.rs:1234`, `:1353`) and never written. There is no
   `ResMut<CollaborationState>` in the crate.
5. `ui/mod.rs:865 update_collaboration_cursors()` is an empty stub.
6. There is no CRDT, OT, session, lock, or presence mechanism for editing. `loro`
   appears only inside the disabled `p2p` module of a crate Studio does not link.

Collaborative editing needs a CRDT over WorldDb, not the gameplay replication
stack. It is a separate project from Features 1 to 4 and should be scoped as one.
It is **out of scope for Alpha**.

---

## Feature 6 - Client join from published simulation

**State:** the chain breaks in five places.

1. **The client has no networking.** `crates/client` declares `eustress-networking`;
   across 5,261 lines the only reference is a commented-out import
   (`client/src/main.rs:27`). `ClientNetworkPlugin` is never added. Its sole CLI
   argument is `args.get(1)`, a local scene path.
2. **No launch handoff.** `eustress://` **is** registered by the installer
   (`installer/windows/eustress-engine.iss:99-103`) pointing at
   `eustress-engine.exe "%1"`, and the web dashboard already calls
   `set_href("eustress://new-project")` (`dashboard.rs:203`). But no binary parses
   the URL from argv, so Studio launches and drops the payload, and the
   registration targets Studio rather than the client. Note that `eustress://` is
   simultaneously the MCP resource scheme (`mcp-server/src/uri.rs:66`). Pick one
   namespace deliberately before adding a third meaning for session joins.
3. **`/play` can never return a server.** `handlePlaySimulation`
   (`infrastructure/cloudflare/api/src/index.js:3637`) scans KV `node:*` for
   `node.simulation_id`, `node.address`, `node.port`. The only writer of `node:*`
   is `handleNodeHeartbeat` (`:3208`, route `/api/node/heartbeat`), the Bliss node
   heartbeat, which sets none of those three fields. Every request falls through to
   `status:'spawn'`, rendered as "No server available."
4. **The dedicated server cannot open a world.** `crates/server/src/main.rs` adds
   `ServerNetworkPlugin` but never writes `StartServer`, so `ServerState` stays
   `Stopped` and every replication system is gated off. `universe_root` is computed
   (`:227`) and used once, in an `info!` (`:248`). Nothing loads it. Its manifest
   declares no `eustress-worlddb`, no `eustress-fjall`, no `eustress-space`, so it
   could not read a `.eustress` world even if it tried.
5. **No auth on join.** `/play` mints no token and nothing validates one.

---

## Feature 8 - Dual-format storage

**State:** both formats ship. They drift. The drift is silent.

### What is true today

- `world-db` is in `core`, which is in `default`. Fjall is on in every normal build.
- The engine manifest states the contract plainly: the DB "is the authoritative
  store and the disk TOML hierarchy is the import seed, not a live mirror"
  (`engine/Cargo.toml:242-246`).
- **Ctrl+S does not write TOML for ordinary Parts.** `write_instance_definition`
  (`space/instance_loader.rs`) begins
  `if active_db::put_instance(toml_path, instance) { return Ok(()); }`.
  `put_instance` (`space/active_db.rs:504`) returns true for any instance that is
  not file-natured and has no child entities, which is every plain leaf Part. It
  writes a bincode `<rel>#bin` row into the Fjall `tree` partition, and
  `_instance.toml` goes stale on disk.
- **`package_universe_to_pak` packs everything.** It is a `read_dir` walk
  (`ui/file_event_handler.rs:987`) with a denylist of six exact names plus `*.lock`
  and `*.tmp`. It does **not** skip `world.fjalldb/`. A `.pak` therefore ships the
  stale TOML **and** the live database, plus `world.fjalldb.bak-*`, `.reorg-bak`,
  `header.bin.bak-*`, `.eustress/output.log`, screenshots, and `trash/`, which is
  content the creator deliberately deleted.
- It hot-copies an open LSM keyspace file by file and never takes `commit_lock`.
  The only `SyncAll` outside the commit sites is in `Drop`. A downloader can
  receive a database that was mid-write.
- `WorldHeader::mark_migrated()` has exactly one occurrence in the repo: its own
  definition. `space_is_migrated()` is therefore false on every Space, so
  `terrain_voxel_load.rs` returns early while `world_db_plugin.rs` re-imports voxel
  chunks on every open.
- `bake_to_echk` (`worlddb/src/bake.rs`) is real code with a proper container and
  **zero callers**. No `.echk` file exists on disk. Its own module doc records two
  defects: it walks only `tree`, so Morton cores are absent, and `#bin` keys all
  bucket into chunk (0,0).

### The recommendation

Keep both formats, with a time-phased authority and exactly one regeneration point.
Do not invert the existing author-time contract. Fix the publish boundary.

**1. Author time: Fjall stays authoritative.** This is the documented, shipped
design. `reconcile_disk_toml_into_tree` (`world_db_plugin.rs:127`, called
unconditionally) already pulls hand-edited and git-merged TOML back in on open, so
git-friendliness survives.

**2. Publish time: regenerate the TOML half from Fjall.** This is the anti-drift
mechanism and the only new invariant. In `do_publish` (`ui/file_event_handler.rs:514`),
before packaging, run `eustress_space::export` (`eustress-space/src/lib.rs:348`) per
Space over a staging directory. `export` already iterates `db.iter_instance_cores()`
and projects each core to TOML. Without this step, every entity that
`bake_cores::bake_once` moved from `tree` into `entities` is invisible in the
published TOML. That is total, silent content loss for the human-readable half,
unreported only because publish has never been round-tripped.

**3. Checkpoint before tarring.** Add `WorldDb::checkpoint_to(&Path)` to
`worlddb/src/backend.rs`, following the existing default-method pattern. Implement
it in `fjall_backend.rs` by taking `commit_lock`, issuing `SyncAll`, and copying
partitions under the lock. Publish tars the checkpoint, never the live directory.

**4. Allowlist, not denylist.** A denylist cannot stop new debris. Emit exactly:
`pak.toml` (a new manifest carrying format version, engine version, space list, and
a blake3 per half), `space.toml`, `simulation.toml`, the service directories,
`header.bin`, the checkpointed `world.fjalldb/`, `assets/`, `references/`, and
`.eustress/thumbnail.*`. Nothing else. This is a privacy fix as much as a size fix:
today a `.pak` carries absolute paths containing the creator's Windows username,
and their deleted `trash/` contents.

**5. Who opens what.**
- **Studio**: Space directory, Fjall authoritative, TOML reconciled in on open.
  Unchanged.
- **Dev server and dedicated server**: opens the binary half. Add `eustress-worlddb`
  to `crates/server/Cargo.toml`. That single line is the highest-leverage edit in
  the storage area. Prefer routing through `eustress-headless`, which already calls
  the same `app_core::add_core_sim_plugins` Studio uses, rather than maintaining a
  second code path.
- **Client**: reads the TOML half via `eustress_common::space_read`, which is
  TOML-native and already attaches `Collider::cuboid` and `RigidBody::Static`
  (`space_read.rs:378`). This limits the client to the Part subset, which is correct
  for Alpha, and means the client links no LSM engine.

**6. Falsifiable no-drift test.** `eustress-space export` reconstructs a TOML tree
from cores. CI extracts a published `.pak`, runs `export` against its `.eustress`
half, and asserts tree equality. If the two halves of one publish ever disagree, the
build fails. This converts the contract from a promise into a test.

**7. Cut `.echk` from Alpha.** Zero callers, no files on disk, two known defects,
and it is a third representation of data that already has two. Mark the module
`NOT WIRED - POST-ALPHA` and build nothing on it.

### Publish defects to fix in the same pass

- `experience_id` is only ever written `None`, and `execute_publish_upload` never
  persists the returned `sim_id`. Every publish mints a fresh UUID at the Worker,
  so **a creator who ships a broken world can never update that listing.**
- `prepare_publish_manifests` (`file_event_handler.rs:1041`) writes manifests to
  `space_root/.eustress/` while every reader looks in `universe_root/.eustress/`.
- The blake3 computed at publish is stored locally in `.eustress/.last_publish_hash`,
  never uploaded, and no consumer verifies anything.
- `client/src/systems/space_fetch.rs:152` GETs `/api/simulations/{id}/space`; the
  Worker serves that path for PUT only. GET is `/download`.

---

## Transport decision

**Adopt `lightyear 0.29`, pinned exactly, behind a non-default `multiplayer`
feature. Keep `ownership.rs`. Keep `play_server` as a documented fallback.**

Three independent design passes reached this conclusion separately, each citing
sources checked at the time of writing:

- lightyear 0.29 targets Bevy 0.19. The version map is 0.28 and 0.29 to Bevy 0.19,
  0.26 and 0.27 to 0.18, 0.25 to 0.17.
- `lightyear_avian3d` 0.29 depends on `avian3d ^0.7` and `bevy ^0.19`, an exact
  match for this workspace (`Cargo.toml:49` and `:86`). This avoids the classic
  duplicate-Avian failure that silently breaks physics replication.
- lightyear provides HostServer mode, a single App containing both client and
  server plugins, which is exactly the shape of Studio Play. `lightyear_crossbeam`
  gives in-process IO, so Phase 4 binds no port for tests and the integration test
  needs no socket.

Feature set: `default-features = false, features = ["netcode", "udp", "avian3d"]`.
Explicitly not `webtransport` or `websocket`. The engine already links `quinn 0.11`
and `rustls 0.23` with ring, and a second QUIC and TLS stack in a build that already
takes 10 to 15 minutes is not acceptable.

**Why not finish `play_server` by hand.** The QUIC setup is correct, but what
remains is not a socket. It is the accept loop, stream demux, reconnect, a snapshot
ring, delta encoding against per-client acked baselines, rollback and resimulation
against Avian, and interpolation buffers. Two independent attempts in this repo
stalled at that same wall, because it is a multi-year library rather than a feature.

**Why this is not a rewrite.** Zero packets move today. Adopting a library into an
empty socket is cheaper than finishing three stubs.

**What survives:** `ownership.rs` in full; `play_server/protocol.rs`'s `GameMessage`
taxonomy, ported to lightyear channel registrations rather than deleted; and
`eustress-networking`'s `scale.rs` and wire types as registered components.

**What must die:** there are currently three parallel replication vocabularies:
`eustress_networking::replication::Replicated`,
`play_server::replication::ReplicatedEntity` with its own `NetworkId`, and
lightyear's marker would be a fourth. Collapse to one in the same phase as adoption,
not after. Every week it is deferred adds call sites to two dead vocabularies.

**Who tags entities.** Replication is a property of authored data, not of a spawn
call site. For Alpha the surface is deliberately one component on one entity type.
`eustress_common::avatar::AvatarControl` already has `LocalPlayer` and `Remote`
variants, and `SpawnAvatar` (`common/src/avatar/mod.rs:117`) is already documented as
the only public way to create a character. Both shells route through it
(`play_mode.rs:869`, `client/src/main.rs:169`). Tag the locally-controlled avatar in
`spawn_play_mode_character` (`play_mode_runtime.rs:154`), and consume it on the client
by spawning with `AvatarControl::Remote`. The shared avatar runtime already handles
`Remote` correctly. Nothing else in the world is replicated in Alpha: anchored
geometry is identical on every peer because every peer loaded the same sealed
artifact, which is content-addressing rather than network traffic.

**Fallback if a spike fails:** `bevy_replicon 0.42` with `bevy_replicon_renet 0.18`,
both confirmed Bevy 0.19. lightyear is built on replicon, so this reimplements only
interpolation, which for avatar transforms is a timestamped ring buffer.

---

## Public Alpha blockers

Ordered by severity. Each is code-verified.

**B1. Identity is forgeable.** `engine/src/auth.rs:16` sets
`const DEV_MODE: bool = true`. `do_email_login` falls back to `mock_login`
(`:390`, used at `:298`) on any network-shaped error and fabricates a full
`AuthUser` from the typed email. `do_steam_login` returns `mock_steam_login()`
unconditionally (`:450`). That same `AuthState` stamps `user_id` and `username`
onto every `ToolContext` (`engine_bridge/protocol.rs:2680`), so audit identity is
forgeable by blocking DNS. Gate the mock paths behind `#[cfg(debug_assertions)]`
so they cannot compile into a release binary.

**B2. Bliss node HTTP API is world-reachable and unauthenticated.**
`crates/bliss/src/api.rs:272` binds `0.0.0.0`, `:115` sets `allow_origin(Any)`,
and there is no auth layer. It is started unconditionally from `StudioAuthPlugin`.

**B3. Gallery is public by default with no moderation.** `is_public: true` is
written at publish (`index.js:4080`) with no review queue and no report route.
Flip it to `false` plus a `moderation_status` field, so Alpha content goes public
by a human flipping a KV key.

**B4. No auth on the multiplayer join path.** Feature 6, item 5. `/play` should
return 503 for Alpha rather than leaving a route that publishes an unauthenticated
endpoint.

**B5. Silent content loss on publish.** Feature 8. Entities baked into `entities`
cores are absent from the published TOML half, and the packaged database may be torn.

**B6. Creators cannot update a published listing.** Feature 8, `experience_id`.

**B7. KYC collection is disproportionate for Alpha.** Collecting government ID from
testers with no retention policy and no report path is the largest regulatory
exposure in the repo. Self-declared date of birth plus Terms, deferring KYC to first
payout, is already the proposed direction in [08_IDENTITY_TRUST](08_IDENTITY_TRUST.md).

**B8. No lockfile.** `eustress/Cargo.lock` is untracked, and the workspace follows
`slint` from a git branch. Every `hashFiles('eustress/Cargo.lock')` cache key in
`release.yml` therefore hashes the empty string. A moving networking dependency with
no lockfile means a host and a client can disagree about the wire format with no way
to diagnose it. Commit the lockfile before adopting any transport.

**B9. CI builds neither the server nor the client.** `ci.yml` runs
`cargo tree -p eustress-engine` and nothing else for these crates.
`target/debug/eustress-server.exe` is dated 2026-03-13 while `server/src/main.rs`
was edited 2026-06-28, so the server has not been built since the Bevy 0.19
migration. Compile rot in the multiplayer crates is invisible.

### Not blockers, recorded to prevent re-reporting

- **The engine bridge is not an RCE.** `ToolRegistry::dispatch`
  (`tools/src/registry.rs:298`) authorizes before handler lookup via
  `capability::authorize`. `run_bash` is classified `Capability::Execute`
  (`capability.rs:147`); `Permissions::standard()` grants only Read and Write; and
  the bridge builds its `ToolContext` with exactly `Permissions::standard()`
  (`engine_bridge/protocol.rs:2705`). Unclassified tools fail closed, and a
  regression test covers the bridge principal specifically. The residual issue is
  smaller: the bridge has no peer authentication for the Read and Write surface it
  does expose.
- **The stream node no longer binds all interfaces.** It is now optional, held out
  of `core`, and defaults to `Ipv4Addr::LOCALHOST` (`stream-node/src/config.rs:46`),
  with off-host reach requiring an explicit `bind_all()`.

---

## Phased plan

Each phase has an observable exit test. No phase is complete on inspection.

**Phase 0 - Stop the bleeding.** Effort S.
Gate `DEV_MODE` behind `debug_assertions`; bind the Bliss API to loopback; flip
`is_public` to false; return 503 from `/play`; commit `Cargo.lock`; add
`cargo build -p eustress-server -p eustress-client` to CI.
*Exit:* CI fails on a deliberately broken `crates/server`, and a release binary
cannot mock-login with the network unplugged.

**Phase 1 - Storage contract.** Effort M.
Regenerate TOML from Fjall at publish; add `checkpoint_to`; replace the denylist
with an allowlist; persist `sim_id`; move the manifests to the Universe root.
*Exit:* CI extracts a published `.pak`, runs `eustress-space export` against its
`.eustress` half, and asserts tree equality. Republishing an unmodified Space
produces byte-identical halves.

**Phase 2 - Server can open a world.** Effort S.
Add `eustress-worlddb` to `crates/server`, or retire `crates/server` in favour of
`eustress-headless`. Load `universe_root` instead of logging it.
*Exit:* a headless process opens a published `.eustress` and reports a non-zero
entity count matching what Studio shows for the same Space.

**Phase 3 - Transport spike.** Effort M.
Build `spikes/netspike/` as a separate cargo workspace, excluded from the engine
workspace and its `target/`, depending only on bevy 0.19, avian3d 0.7, lightyear 0.29.
*Exit:* two Apps in one process, one avatar replicated, prediction and rollback
active, with no engine crate involved. If this fails, fall back to replicon before
any engine code is written.

**Phase 4 - Studio dev server.** Effort M.
Create `crates/common/eustress-net` owning protocol registration, `NetAvatar`, and
the handshake, behind a non-default `multiplayer` feature. Wire `Action::StartServer`
to a real system. Bind `127.0.0.1`, with LAN behind an explicit toggle and a token.
*Exit:* press Play in Studio, launch `eustress-client --connect 127.0.0.1:<port>`,
and see two avatars move in both windows.

**Phase 5 - Client join from a published simulation.** Effort L.
Parse `eustress://` from argv; register the client for play URLs; have the server
register `address`, `port`, and `simulation_id`; mint and validate a single-use join
token bound to user, sim, server, and expiry.
*Exit:* a second machine clicks Play on the website and lands in the first machine's
session, and a forged or expired token is rejected with a logged reason.

**Out of Alpha scope:** Team Create, `.echk`, matchmaking, voice, spectator, ranked,
always-on servers, mobile players.

---

## Honest Alpha positioning

The shippable Alpha is Studio, a local dev server, a private-by-default publish loop,
and a real client. Stranger-to-stranger public multiplayer is Phase 5 and should not
be claimed before it exists.

[docs_publishing](../../eustress/crates/web/src/pages/docs_publishing.rs) currently
promises Forge servers spinning up automatically via Nomad, live player counts, and
rejoin-on-publish semantics. None of that path is connected. Align the copy with
Phase 5, or remove it before launch.

---

## Cross-system dependencies

- **C2 / canonical create** - server-side spawns route through `instance_create`.
- **C7 / Avian only** - the deterministic step is shared; lightyear's `avian3d`
  feature must unify with the workspace `avian3d 0.7` rather than duplicate it.
- **C11 / dual-format container** - Feature 8 is the contract. The server opens the
  binary half, the client opens the TOML half, both ship in one `.pak`.
- **C13 / `eustress://`** - the scheme is registered but unparsed, and it collides
  with the MCP resource scheme.
- **C14 / KYC-deferred** - B7.
- Depends on [04_ASSET_PIPELINE](04_ASSET_PIPELINE.md),
  [01_CLIENT_PLAYER](01_CLIENT_PLAYER.md), [08_IDENTITY_TRUST](08_IDENTITY_TRUST.md),
  and [16_PERSISTENCE_DATASTORE](16_PERSISTENCE_DATASTORE.md).

---

## Open questions

- Q3.1 Does `eustress-space` ship in the installer as the documented recovery tool?
  "Run `eustress-space verify`" is the difference between a support ticket and a
  lost world.
- Q3.2 One namespace for `eustress://`, or a second scheme for session joins?
- Q3.3 Retire `crates/server`, or keep it and give it storage dependencies?
- Q3.4 Does Team Create target a CRDT over the `tree` partition, or over the causal
  op-log in `worlddb/src/mutations.rs`?
- Q3.5 LAN dev servers: a token in the invite, or trust-the-network with a warning?
