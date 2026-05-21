# 03 — Multiplayer

> Server runtime, replication, Forge orchestration, networking transport,
> script distribution, friends / parties / teams, server-authoritative
> physics. **JWT validation is a P0 ship blocker.**

## Pass changelog

- **P1 (2026-05-14):** 11 feature rows; 56 R + 42 I + 15 wiring gaps.
- **P3 (2026-05-14):** Networking row 1 upgraded (closer to 🟢 per critic); server pak download ~70%; JWT escalated **P0**. + 5 features (ranked/MMR, lobby, region browser, spectator/replay, in-session moderation).
- **P4 (2026-05-14):** **Full retrofit to per-feature-card format.** 16 cards. Addendum blocks removed.
- **Updated 2026-05-16: storage pivot.** Server downloads + loads a `.eustress` world container (Fjall WorldDb database + baked `.echk` chunks) instead of a `.pak` zstd-tar; download/extract mechanism unchanged. `.pak`→`.eustress` corrections in Feature 4 + C3/C11 footers. See MASTER C17.

---

## Concept summary

A Space becomes a live shared session: Client launches, server allocated (by Forge or PIE), replicated entities sync over QUIC. Server is authoritative for game logic + physics + validation; Client predicts inputs and reconciles. Hybrid model adds bevy_quinnet P2P for voice/chat where server-auth latency would hurt.

Eustress's bet is that [Forge](../architecture/EUSTRESS_FORGE.md) — a Rust-native Nomad-based orchestrator — replaces K8s for game-server scheduling. Script distribution streams compiled `.rune` bytecode from server to Client on connect. Social features (friends, parties, presence) are designed across multiple crates; routes are mostly unwired.

---

## Implementation snapshot

- **Crates:** [eustress-server](../../eustress/crates/server/), [eustress-forge](../../eustress/crates/forge/), [eustress-forge-sdk](../../eustress/crates/forge-sdk/), [eustress-backend](../../eustress/crates/backend/), [eustress-identity](../../eustress/crates/identity/), [eustress-runtime](../../eustress/crates/runtime/), `eustress-networking`
- **Key types:** `Replicated`, `NetworkTransform`, `NetworkVelocity`, `NetworkOwner`, `ReplicationFilter`, `ReplicationGroup`, `RemoteEvent` (class only), `RemoteFunction` (class only)
- **Backend endpoints implemented:** `/health`, `/api/{simulations, favorites, gallery, marketplace, community, projects}/*`
- **Backend endpoints missing:** `/api/sessions`, `/api/friends`, `/api/parties`, `/api/teleport`, `/api/matchmaking`, `/api/lobbies`

---

## Top-of-doc feature index

| # | Feature | State |
| ---: | --- | :-: |
| 1 | QUIC transport + replication core | 🟢 transport / 🟡 client-prediction |
| 2 | RemoteEvent / RemoteFunction RPC | 🟠 |
| 3 | Server-authoritative validation | 🟠 |
| 4 | Server runtime (`eustress-server`) | 🟡 |
| 5 | Client prediction + rollback | 🟠 |
| 6 | Forge orchestrator | 🟡 |
| 7 | Backend REST API | 🟡 |
| 8 | Identity / auth (Ed25519 + JWT) **— game-server JWT gap is P0** | 🔴 P0 |
| 9 | Script distribution | 🟠 |
| 10 | Friends / parties / presence | 🟠 |
| 11 | Voice chat / spatial audio | 🟠 |
| 12 | Matchmaking + ranked / MMR *(P3 add)* | 🔴 |
| 13 | Lobby system + region browser *(P3 add)* | 🔴 |
| 14 | Spectator mode + replay sharing *(P3 add)* | 🔴 |
| 15 | In-session chat filter + moderation *(P3 add)* | 🔴 |
| 16 | Persistent always-on server per experience *(P3 add)* | 🔴 |

---

## Per-feature cards

### Feature 1 — QUIC transport + replication core

**State:** 🟢 transport / 🟡 client-prediction · **Effort:** M (close prediction gap) · **Risk:** Med · **Touches:** [01], [03], [05], C11
**Sub-features:** QUIC + TLS 1.3 · 120 Hz tick (24/60/144 configurable) · `Replicated` components · AOI spatial-hash culling · `NetworkOwner` · delta / full snapshot · `ReplicationFilter` distance override

**Concept.** Custom Bevy plugin on QUIC. Entities tagged `Replicated` propagate `NetworkTransform` / `NetworkVelocity` deltas under AOI filter. Server is authority.

**Forecasted feedback (R)**
- R1.1 Lightyear is concept-influenced (no direct dep); clarify in docs.
- R1.2 AOI radius default tunable per `_service.toml`.
- R1.3 Delta vs. full snapshot needs benchmarking.
- R1.4 Multi-instance server sharding by world region unaddressed.
- R1.5 Network sim (latency / loss) for QA not in test harness.
- R1.6 Bandwidth at 120 Hz × 200 entities will blow past 5 Mbps without priority.
- R1.7 Compressed Vec3 (16-bit/axis world-relative) vs. f32 — pick.

**Implications (I)**
- *Architectural:* lowest networking layer; instability anywhere cascades.
- *Cross-system:* unify AOI math with [05_SPACE_STREAMING] hysteresis grid.
- *Migration:* web players via WebTransport need browser-compatible handshake.
- *Operational:* tick rate is the cost knob.
- *Strategic:* deterministic replay (moderation / dispute) shares the tick clock.

**Risks (X)**
- X1.1 Float drift cross-platform → permanent rubber-band.
- X1.2 Replication lib churn (Lightyear migration) breaks compatibility.

**Mitigations (M)**
- M1.1 Cross-platform CRC determinism test in CI.
- M1.2 Pin internal lib version per release.

---

### Feature 2 — RemoteEvent / RemoteFunction RPC

**State:** 🟠 (Roblox API class only; no wire impl) · **Effort:** L · **Risk:** Med · **Touches:** [03], [07_AI]
**Sub-features:** `FireClient(player, ...)` · `FireAllClients(...)` · `InvokeServer(...)` · `OnServerEvent` · `OnClientInvoke` · wire format · rate limit · schema versioning · sanitisation

**Concept.** Roblox-parity API. Class types exist in `common/assets/class_schema`; runtime wire format and dispatch are absent.

**Forecasted feedback (R)**
- R2.1 No wire format chosen — postcard / bincode / protobuf?
- R2.2 Per-RemoteEvent schema versioning.
- R2.3 Rate-limit per RemoteEvent per player.
- R2.4 `FireServer` from untrusted client must be sanitised + abuser-logged.
- R2.5 `InvokeServer` default timeout.

**Implications (I)**
- *Architectural:* RPC is bedrock for every cross-boundary gameplay feature.
- *Cross-system:* bind to `EustressMessage` enum in `eustress-networking/protocol.rs`.
- *Migration:* Roblox-parity expectation drives API shape.

**Risks (X)** — X2.1 Replay attack on RPC without nonce.

**Mitigations (M)** — M2.1 Sequence number + HMAC per channel.

---

### Feature 3 — Server-authoritative validation

**State:** 🟠 (config; no enforcement) · **Effort:** L · **Risk:** Critical · **Touches:** [03], [08_IDENTITY], C7
**Sub-features:** speed cap · position bounds · teleport detection · per-script CPU budget · RPC rate limit · ownership check · declarative policy

**Concept.** Server validates every Client-submitted state change. Rejected state snaps Client back.

**Forecasted feedback (R)**
- R3.1 `max_speed` + `gravity` configurable; **validation not enforced**.
- R3.2 Position teleport detection: 100 m / tick = cheat; no check.
- R3.3 Per-script CPU budget for Luau / Rune absent (Soul has partial).
- R3.4 RPC replay-attack nonce missing.
- R3.5 Reconciliation snap visible without interpolation.

**Implications (I)**
- *Architectural:* unshippable without; every speedhack on day-one.
- *Cross-system:* validation rules grow with features; need declarative policy file.
- *Strategic:* reviewers test cheats *deliberately*.

**Risks (X)** — X3.1 No validation = day-one speed-hack videos.

**Mitigations (M)** — M3.1 Declarative policy YAML; load at server start.

---

### Feature 4 — Server runtime (`eustress-server`)

**State:** 🟡 (boots; world-container download ~70%; cleanup + auth gaps) · **Effort:** M · **Risk:** Med · **Touches:** [03], [04_ASSETS], [12_INFRASTRUCTURE], C11, C17
**Sub-features:** headless boot (MinimalPlugins) · CLI `--sim-id` · world-container download (`/api/simulations/{id}/download`) · zstd-tar extract → temp dir · open `.eustress` (Fjall WorldDb + baked `.echk` chunks) · QUIC server start · health endpoint · structured logging · graceful shutdown (Forge SDK hook)

**Concept.** *Updated 2026-05-16: storage pivot.* Headless binary. Downloads the `.eustress` world container from `api.eustress.dev`, decompresses, extracts, opens the Fjall WorldDb database (+ baked `.echk` chunks), loads as Universe, accepts QUIC. *(Was `.pak` zstd-tar of per-instance TOML pre-2026-05-16; the download/extract mechanism is unchanged, only the archived payload.)*

**Forecasted feedback (R)**
- R4.1 Builds ✅; F5 PIE launches embedded variant.
- R4.2 Streaming-pak-download resumption missing.
- R4.3 Metrics endpoint partial (no Prometheus scrape config).
- R4.4 Graceful shutdown SIGTERM hook from Forge SDK not called in `main.rs`.
- R4.5 Logging stdout; production needs structured JSON.
- R4.6 Per-instance memory limit enforcement absent.

**Implications (I)**
- *Architectural:* server is the heart; lifecycle + observability decide ops cost.
- *Cross-system:* sharing engine code → Studio bug crashes production server.
- *Operational:* graceful shutdown is the difference between "5-s blip" and "all players kicked".

**Risks (X)** — X4.1 SIGTERM without drain → mid-match disconnect.

**Mitigations (M)** — M4.1 Wire Forge SDK shutdown hook before any public exposure.

---

### Feature 5 — Client prediction + rollback

**State:** 🟠 (~40% per P3 critic; LocalClient state machine exists) · **Effort:** L · **Risk:** Critical · **Touches:** [01], [03], [11_SIMULATION]
**Sub-features:** input prediction · snapshot rollback · smoothed reconcile · input buffer · deterministic Avian step · float-mode lock

**Concept.** Client receives input → runs locally for low latency → reconciles when server's authoritative state arrives. On disagreement, rewind to server tick and re-simulate buffered inputs forward.

**Forecasted feedback (R)**
- R5.1 No prediction code today; inputs go server-only.
- R5.2 Avian doesn't natively support rollback → state snapshots + deterministic step.
- R5.3 Float determinism cross-platform is a swamp ([11_SIMULATION] Feature 8).
- R5.4 Prediction errors → visible snaps; per-component smoothing window.
- R5.5 Jitter buffer interacts with prediction; one knob.

**Implications (I)**
- *Architectural:* without prediction, multiplayer feels worse than single-player above 50 ms RTT.
- *Cross-system:* deterministic-step constrains every future physics feature.
- *Strategic:* ranked / MMR (Feature 12) requires prediction quality.

**Risks (X)** — X5.1 Avian's SIMD math diverges across CPU vendors.

**Mitigations (M)** — M5.1 `--deterministic` flag locks SIMD paths; CRC compare in CI.

---

### Feature 6 — Forge orchestrator

**State:** 🟡 (SDK ✅; lifecycle hooks in eustress-server absent) · **Effort:** L · **Risk:** Med · **Touches:** [03], [12_INFRASTRUCTURE]
**Sub-features:** `ForgeController` · Nomad job submission · Consul health checks · autoscaling · multi-region routing · session tracking (`PlayerSession` with ephemeral token) · spot interruption handling

**Concept.** Rust-native Nomad-based orchestrator. Spawn / terminate / route / scale / fail-over.

**Forecasted feedback (R)**
- R6.1 `eustress-server::main` doesn't register with Forge; lifecycle hooks unused.
- R6.2 Nomad HCL in `infrastructure/forge/`; Studio-side tooling absent.
- R6.3 Spot interruption handler missing systemd service.
- R6.4 Bin-packing optimisation open.
- R6.5 Cost attribution per-experience / per-creator → KYC tie.
- R6.6 PIE embedded server vs. Forge-managed parallel lifecycles.

**Implications (I)**
- *Architectural:* Forge is also asset-pipeline scheduler ([04] generation orchestrator).
- *Cross-system:* sessions / tokens shared with [08_IDENTITY].
- *Strategic:* cost story is the marketing claim — must hold up at scale.

**Risks (X)** — X6.1 Spot reclaim mid-match without drain = bad reviews.

**Mitigations (M)** — M6.1 Spot-warning systemd handler (2 min warning → migrate players).

---

### Feature 7 — Backend REST API

**State:** 🟡 (core ✅; sessions/friends/parties/matchmaking 🟠) · **Effort:** L · **Risk:** Med · **Touches:** [03], [06_WEBSITE], [09_ECONOMY], [16_PERSISTENCE]
**Sub-features:** Axum + sqlx + SQLite · `/health` · `/api/simulations/*` · `/api/favorites/*` · `/api/gallery/*` · `/api/marketplace/*` · `/api/community/*` · `/api/projects/*` · missing: sessions / friends / parties / teleport / matchmaking / lobbies

**Concept.** REST surface for the website + Studio publish + identity. Cloudflare Worker fronts auth + KYC.

**Forecasted feedback (R)**
- R7.1 Core CRUD shipped; multiplayer-adjacent endpoints absent.
- R7.2 No telemetry ingest endpoint ([10_TELEMETRY] Feature 11).
- R7.3 Pagination inconsistent (cursor vs. offset).
- R7.4 Webhook system (creator payouts, KYC results) missing.
- R7.5 GraphQL might suit deeply-nested gallery + creator views.

**Implications (I)**
- *Architectural:* backend = website's data plane; gaps cap site features.
- *Cross-system:* same Axum service can host engine_bridge WAN proxy.

**Risks (X)** — X7.1 SQLite single-writer bottleneck above ~100 writes/sec.

**Mitigations (M)** — M7.1 PG migration path via sqlx (drop-in).

---

### Feature 8 — Identity / auth (Ed25519 + JWT)  **— P0**

**State:** 🔴 **P0 ship blocker** (web auth ✅; game-server validates nothing) · **Effort:** S (high urgency) · **Risk:** Critical · **Touches:** [03], [08_IDENTITY], C13, C14
**Sub-features:** Cloudflare Worker issues JWT · Axum extractor validates · **game-server validates QUIC client JWT** *(MISSING)* · join-token via Forge SDK `allocate_session` *(MISSING wire)* · OAuth alternative ([08] Feature 15) · MFA ([08] Feature 16)

**Concept.** Cloudflare Worker mints JWT after Ed25519 challenge; Axum backend validates; **server must validate on QUIC handshake** before accepting any client.

**Forecasted feedback (R)**
- R8.1 **`eustress-server::main` has zero JWT validation on QUIC connect.** Day-1 cheat / impersonation hole.
- R8.2 Forge SDK has `PlayerSession` ephemeral tokens; not consumed.
- R8.3 No OAuth alternative; KYC-first signup halves the funnel.
- R8.4 Key rotation policy absent.
- R8.5 KYC mandatory at signup; should defer to first monetisation (C14).

**Implications (I)**
- *Strategic:* **Cannot ship public multiplayer until this lands.**
- *Cross-system:* identity stack is shared with website + asset signing.
- *Architectural:* JWT validation in QUIC handshake = one library, one rule.
- *Compliance:* without JWT, no audit trail on actions.

**Risks (X)**
- X8.1 Any process on QUIC port = unauthenticated join.
- X8.2 Impersonation: spoof another player's actions.
- X8.3 Server exhaustion DoS.

**Mitigations (M)**
- M8.1 **Wire JWT check at `accept_connection`. Block on this before any public launch.**
- M8.2 Token expiry tight (60 s) + single-use.
- M8.3 Rate-limit failed-auth attempts per IP.

---

### Feature 9 — Script distribution

**State:** 🟠 (design ✅; code 0%) · **Effort:** L · **Risk:** Med · **Touches:** [02_STUDIO], [03], [04_ASSETS]
**Sub-features:** server compiles `.soul → .rune` at boot · SHA-256 manifest to clients on connect · chunked download (64 KB) · RunContext (Server / Client / Both) gate · hot-reload during session · script signing

**Concept.** [MULTIPLAYER_SCRIPT_DISTRIBUTION.md](../development/MULTIPLAYER_SCRIPT_DISTRIBUTION.md) fully specs the protocol. Zero code today.

**Forecasted feedback (R)**
- R9.1 Compilation pipeline ([engine/src/soul/build_pipeline.rs]) exists; not triggered at server startup.
- R9.2 RunContext filtering at execution — server-only script must not run on client even if leaked.
- R9.3 Hot-script reload during live session — manifest version bump?
- R9.4 Script signing (same chain as asset signing — [08_IDENTITY]).
- R9.5 Bytecode tampering — client subbing its own `.rune`.

**Implications (I)** — *Strategic:* until script distribution lands, no project with scripts runs multiplayer.

**Risks (X)** — X9.1 Signed-bytecode mismatch = silent client-side cheat.

**Mitigations (M)** — M9.1 Client refuses unsigned bytecode in release.

---

### Feature 10 — Friends / parties / presence

**State:** 🟠 (types ✅; backend routes absent; presence WS not wired) · **Effort:** L · **Risk:** Med · **Touches:** [03], [06_WEBSITE], [08_IDENTITY]
**Sub-features:** add / remove / block list · presence WS (`presence_ws.rs`) · party invites · party teleport (Teleport service) · cross-platform IDs

**Concept.** Players add friends, see presence (online/offline/in-game), invite to party, teleport-as-party.

**Forecasted feedback (R)**
- R10.1 Types defined; **zero backend routes**.
- R10.2 Presence WS server unwired despite types.
- R10.3 Block-list semantics — blocked can share public server?
- R10.4 Cross-platform friends need platform-agnostic IDs.
- R10.5 Hide-presence per-friend setting.

**Implications (I)**
- *Strategic:* social retention multiplier; no friends list = no retention.
- *Cross-system:* presence WS = stream-node TCP topic per [10_TELEMETRY].

**Risks (X)** — X10.1 Spam / harassment without block enforcement.

**Mitigations (M)** — M10.1 Block list propagates to chat filter (Feature 15).

---

### Feature 11 — Voice chat / spatial audio

**State:** 🟠 · **Effort:** L · **Risk:** Med · **Touches:** [03], [15_MOBILE]
**Sub-features:** WebRTC data channels · Opus codec · spatial mixer · push-to-talk / open-mic · team channels · transcription (moderation)

**Concept.** Real-time voice over WebRTC; spatially mixed by audio engine; gated by team / proximity / permission.

**Forecasted feedback (R)**
- R11.1 Designed in `docs/services/chat.md`; no Rust integration.
- R11.2 SFU vs. mesh — pick (SFU scales).
- R11.3 Mobile Opus is fine; royalty-free.
- R11.4 Voice transcription + flagging is its own workstream.

**Implications (I)** — *Strategic:* voice unlocks team-based games; not P0 but critical retention layer.

**Risks (X)** — X11.1 Audio harassment without moderation pipeline.

**Mitigations (M)** — M11.1 Server-side transcription + ML toxicity flag.

---

### Feature 12 — Matchmaking + ranked / MMR  *(P3 add)*

**State:** 🔴 · **Effort:** XL · **Risk:** Med · **Touches:** [03], [06_WEBSITE]
**Sub-features:** ELO / Glicko-2 / TrueSkill · queue / region matching · skill-based pairing · decay · placement matches · smurf detection · queue-dodge penalty

**Concept.** Competitive multiplayer requires skill-based pairing + ranking. `MatchmakingCriteria` (region, min_players) exists in teleport service; no skill component, no ELO, no decay.

**Forecasted feedback (R)**
- R12.1 Choice of rating system (ELO / Glicko / TrueSkill).
- R12.2 Smurf detection via account-link signals.
- R12.3 Region preference + cross-region matching (high-RTT penalty).
- R12.4 Queue-dodge penalty.

**Implications (I)** — *Strategic:* required for competitive launch; not for casual.

**Risks (X)** — X12.1 Bad matchmaking = poor first-week experience.

**Mitigations (M)** — M12.1 Soft launch: rated unranked mode first.

---

### Feature 13 — Lobby system + region browser  *(P3 add)*

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [03], [06_WEBSITE]
**Sub-features:** `/api/lobbies` · `/api/servers/list` · region picker · per-game-mode filter · friend filter · in-progress / waiting state · password-protected lobbies

**Concept.** Server browser + lobby UI for users who don't want auto-matchmaking. List active servers, filter by region / mode / friend presence.

**Forecasted feedback (R)**
- R13.1 No `/api/lobbies` endpoint.
- R13.2 No `/api/servers/list`.
- R13.3 UX: gallery integration vs. separate page.
- R13.4 Password lobbies require token in URL.

**Implications (I)** — *Cross-system:* shares server-pool data with Forge.

**Risks (X)** — X13.1 Public lobbies attract trolls; need report flow.

**Mitigations (M)** — M13.1 Per-lobby kick / ban (host's authority).

---

### Feature 14 — Spectator mode + replay sharing  *(P3 add)*

**State:** 🔴 · **Effort:** XL · **Risk:** Med · **Touches:** [03], [11_SIMULATION], [10_TELEMETRY]
**Sub-features:** server-side tick recorder · replay file format · spectator-camera mode · highlight clip export · share to website / social

**Concept.** Server records ticks; users replay later or watch live. Shares determinism dependency with [11_SIMULATION] Feature 9.

**Forecasted feedback (R)**
- R14.1 Determinism foundation must land first ([11] Feature 8).
- R14.2 Replay format = tick stream + initial snapshot.
- R14.3 Highlight clip needs auto-detection (kill, win, etc.).

**Implications (I)** — *Strategic:* spectator is the social/streaming hook.

**Risks (X)** — X14.1 Replay file format breaks across engine versions.

**Mitigations (M)** — M14.1 Versioned + auto-migration (similar to save game schema).

---

### Feature 15 — In-session chat filter + moderation  *(P3 add)*

**State:** 🔴 · **Effort:** L · **Risk:** High · **Touches:** [03], [08_IDENTITY]
**Sub-features:** profanity filter · PII redaction · server-side enforcement · per-player mute · report-flag flow · escalation to moderation queue

**Concept.** `docs/services/chat.md` references server-side filter; no enforcement code. Coordinated with [08_IDENTITY] Feature 10 (moderation pipeline).

**Forecasted feedback (R)**
- R15.1 No enforcement code.
- R15.2 Voice transcription absent ([15] depends on voice).
- R15.3 Per-region rule variants (China content laws etc.).
- R15.4 False-positive impact: legit speech filtered → user anger.

**Implications (I)** — *Compliance:* required for COPPA / EU age gating.

**Risks (X)** — X15.1 Day-1 toxic content viral incident.

**Mitigations (M)** — M15.1 Ship with basic word-list before launch; ML in P5.

---

### Feature 16 — Persistent always-on server per experience  *(P3 add)*

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [03], [16_PERSISTENCE]
**Sub-features:** per-experience persistent state · always-on Forge job · world-state snapshots · join-existing vs. spawn-new logic

**Concept.** Some projects (social hubs, persistent worlds, MMOs) need a server that's *always* running. Today's model implies generic gameserver pool spawned per session.

**Forecasted feedback (R)**
- R16.1 Forge job spec for always-on differs from per-match.
- R16.2 State persistence via [16_PERSISTENCE] DataStore.
- R16.3 Cost attribution: who pays for empty always-on?
- R16.4 Multi-shard within experience (instancing).

**Implications (I)** — *Strategic:* social spaces (lobby world, town hub) need this.

**Risks (X)** — X16.1 Idle always-on = pure cost.

**Mitigations (M)** — M16.1 Auto-suspend after 24 h zero-players; resume on join.

---

## Wiring / import gaps

1. **JWT validation in QUIC handshake** (P0)
2. Join-token system via Forge SDK `allocate_session`
3. Client prediction + reconciliation systems
4. RemoteEvent / RemoteFunction wire format + dispatcher
5. Server-auth validation rules (speed / position / rate / ownership) + declarative policy
6. Script distribution protocol messages + chunk transfer
7. RunContext execution gate
8. `/api/friends` + presence WS server
9. `/api/parties` + teleport coordinator
10. `/api/matchmaking` + Forge integration for server selection
11. `/api/lobbies` + server list
12. Session lifecycle hooks in `eustress-server::main` (Forge SDK register + drain)
13. `/api/events/ingest` telemetry endpoint
14. Voice chat scaffolding (WebRTC + spatial mixer + transcription)
15. Replay / spectator (record ticks; replay format)
16. In-session chat filter (server-side enforcement)
17. Always-on server Forge job spec
18. Ranked / MMR backend

---

## Cross-system dependencies

- **C2 / Canonical create** — server-side spawns route through `create_instance`.
- **C3 / Single-author storage** — file-system-first was the prior architecture; superseded 2026-05-15 by the Fjall WorldDb store (MASTER C17). Server reads the same `.eustress` Fjall WorldDb container as Studio + Client; TOML retained as legacy/seed + human-editable schema.
- **C4 / Stream tee** — replication events tee to `replication.*`.
- **C7 / Avian only** — deterministic step shared with Client (Feature 5).
- **C8 / AI consent** — replicated state from `ai = true` entities is the only training-eligible stream.
- **C11 / `.eustress` world container** — server downloads the `.eustress` world container (Fjall WorldDb + baked `.echk` chunks; was `.pak` pre-2026-05-16); client never does.
- **C13 / `eustress://`** — Play button → backend allocates server → token → Client joins.
- **C14 / KYC-deferred OAuth** — JWT identifies editor; auth tier doesn't gate basic multiplayer.
- Depends on **[04_ASSET_PIPELINE]** for `.eustress` world-container download; **[01_CLIENT_PLAYER]** for replication consumer; **[10_TELEMETRY]** for `script.error` + `replication.*` topics; **[06_WEBSITE]** for sessions / friends UI; **[08_IDENTITY]** for everything auth.

---

## Open questions

- Q3.1 Lightyear migration vs. custom — decide before scaling tests.
- Q3.2 Voice stack — own / partner (Vivox / Photon)?
- Q3.3 Cross-platform party invites — deep-link spec.
- Q3.4 KYC for multiplayer — mandatory or optional with "verified" badge?
- Q3.5 Replay format — own or piggyback on QUIC + session token?
- Q3.6 Always-on server pricing model.
- Q3.7 Anti-cheat — server-only validation or also client-side fingerprinting?
