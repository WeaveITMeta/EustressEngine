# 01 — Client Player

> The native runtime that plays a published `.eustress` world container. **Thin QUIC renderer** —
> joins a server (remote or embedded local), receives replicated game state,
> renders, runs Soul scripts, animates the character, handles input. It does **not**
> download the world container itself (servers do — see [04_ASSET_PIPELINE](04_ASSET_PIPELINE.md)).

## Pass changelog

- **P1 (2026-05-14):** 12 feature rows; 60 R + 48 I + 12 wiring gaps.
- **P2 (2026-05-14):** Architectural correction — Client is thin renderer, not pak-downloader. `.eustress` → `.pak`.
- **P3 (2026-05-14):** Critique addendum — 6 missing features (save game, audio device, DPI, error recovery, gen server, frame-rate); hot-reload extension-match bug (`.md` only, not `.rune`/`.lua`).
- **P4 (2026-05-14):** **Full retrofit to per-feature-card format (R/I/X/M).** 18 cards. Addendum blocks removed (content baked into cards). Cross-cuts C11–C16 applied.
- **Updated 2026-05-16: storage pivot.** `.pak` is no longer the published format — the server downloads + loads a `.eustress` world container (Fjall WorldDb database + baked `.echk` chunks). `.pak`→`.eustress` corrections applied to header, concept summary, feature index, R-bullets, and C3/C11 footers. See MASTER C17.

---

## Concept summary

The Client opens a `eustress://play/{sim_id}` URL (or `cargo run -p eustress-client` for dev), connects to an allocated game server via QUIC, and renders the replicated world. The server (remote in multiplayer, embedded locally for solo) downloads + extracts the `.eustress` world container (Fjall WorldDb database + baked `.echk` chunks; see MASTER C17), loads the Universe, accepts connections. The Client's responsibilities are *narrow*: Bevy world + Avian (rendering and physics-prediction), QUIC client, Soul VM (Rune + Luau), GUI bridge, character animation, input → server commands, asset stream consumer.

The Client is the receiving end of every other system: assets flow from [04](04_ASSET_PIPELINE.md), state from [03](03_MULTIPLAYER.md), chunks from [05](05_SPACE_STREAMING.md), AI enhancements from [07](07_AI_PLATFORM.md), launch tokens from [06](06_WEBSITE.md) + [08](08_IDENTITY_TRUST.md). Mobile shells punt their detail to [15_MOBILE_PLATFORM](15_MOBILE_PLATFORM.md).

---

## Implementation snapshot

- **Bin entry:** [client/src/main.rs](../../eustress/crates/client/src/main.rs)
- **Plugins:** `DefaultPlugins` + `PhysicsPlugins` (Avian, gravity 9.80665) + `LightingServicePlugin` + `PlayerServicePlugin` + `ClientTerrainPlugin` + `TeamServicePlugin` + `CharacterAnimationPlugin` + `EnhancementPlugin` (6 chained systems) + `ClientSoulPlugin` + `ClientPhysicsBridgePlugin` + `DistributedWorldPlugin` + `SkinnedCharacterPlugin` + `GuiBridgePlugin` + `LuauPlugin`.
- **Mobile core:** [player-mobile](../../eustress/crates/player-mobile/) — `cdylib`/`staticlib`; today a hardcoded cube; OS shells under [player-android/](../../eustress/crates/player-android/) + [player-ios/](../../eustress/crates/player-ios/) **do not link** the Rust core.

---

## Top-of-doc feature index

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Boot & runtime plumbing | ✅ |
| 2 | QUIC server join (was "`.pak` loader" in P1; payload is now the `.eustress` world container) | 🟠 |
| 3 | Session manifest from `/play/{sim_id}` | 🟡 |
| 4 | Asset fetch via server proxy | 🟡 |
| 5 | Enhancement pipeline (on-demand AI gen) | 🟡 |
| 6 | Quest / scene-connection executor | 🟡 |
| 7 | Soul scripting (Rune + Luau) | 🟡 |
| 8 | Networking / replication | 🟡 |
| 9 | Planetary coords / `WorldPosition` | 🟠 |
| 10 | Character / locomotion / animation | 🟡 |
| 11 | Mobile player parity *(detail → [15](15_MOBILE_PLATFORM.md))* | 🔴 |
| 12 | Graceful degradation | 🟠 |
| 13 | Save game / state persistence *(P3 add)* | 🔴 |
| 14 | Audio device handling *(P3 add)* | 🟡 |
| 15 | Display scaling / DPI *(P3 add)* | 🔴 |
| 16 | Boot error recovery / panic-to-dialog *(P3 add)* | 🔴 |
| 17 | Generation-server HTTP contract *(P3 add)* | 🟠 |
| 18 | Frame-rate / VSync override *(P3 add)* | 🔴 |

---

## Per-feature cards

### Feature 1 — Boot & runtime plumbing

**State:** ✅ · **Effort:** Done · **Risk:** Med · **Touches:** [01], [10_TELEMETRY], [12_INFRASTRUCTURE]
**Sub-features:** Bevy DefaultPlugins · Avian gravity 9.80665 m/s² · seven service plugins · default scene fallback · CLI arg routing

**Concept.** Bevy boots, Avian initialises, services register, default scene (or `--play <sim_id>`) loads. Deterministic within a tick — two clients hashed at the same tick produce the same state.

**Forecasted feedback (R)**
- R1.1 Cold-cache asset load blocks main thread on first mesh spawn (multi-second hitch).
- R1.2 Logging defaults to stdout; release builds should file-route.
- R1.3 Gravity hardcoded; should read `workspace.Gravity` per project.
- R1.4 Plugin order is locked; document or move to `bevy_plugin_dependency`.
- R1.5 DefaultPlugins may transitively pull in editor inspector — audit `--no-default-features`.

**Implications (I)**
- *Architectural:* boot order is brittle; refactor to declared dependencies before more plugins.
- *Cross-system:* publish `ClientStartupSummary` event to [10_TELEMETRY] for Discord Rich Presence + telemetry.
- *Operational:* every degradation in boot time hurts retention.
- *Support:* crash-on-boot is invisible if users just quit; needs Sentry minidump capture (Feature 16).
- *Strategic:* "fast launch" is a marketing line — protect it.

**Risks (X)**
- X1.1 First-run black screen (no splash + cold cache + slow GPU init) interpreted as "broken".
- X1.2 Plugin-order regression in a refactor crashes startup.

**Mitigations (M)**
- M1.1 Splash with progress bar reading from asset prefetch.
- M1.2 CI smoke-test: launch + screenshot after 5 s; diff against golden.

---

### Feature 2 — QUIC server join

**State:** 🟠 · **Effort:** L · **Risk:** Critical · **Touches:** [01], [03_MULTIPLAYER], [06_WEBSITE], [08_IDENTITY], C11, C13
**Sub-features:** `eustress://play/{sim_id}` protocol handler · join-token in TLS 1.3 handshake · embedded-server fallback for solo · region selection · reconnect on drop

**Concept.** Click "Play" in the gallery → OS launches Client with `eustress://play/{sim_id}?token={join_token}` → Client parses, connects via QUIC to the server allocated by Forge (or spawns an embedded server for solo). **No pak download on the Client side** — the server already has it.

**Forecasted feedback (R)**
- R2.1 OS protocol handler must be registered at install ([12_INFRASTRUCTURE] Feature 5).
- R2.2 Token expiry vs. user latency to click — what's the TTL?
- R2.3 Embedded-server mode for solo: same binary or separate?
- R2.4 Drag-drop a `.eustress` world container onto Client window for offline play (no server URL) — UX?
- R2.5 Region selection on first join.
- R2.6 Reconnect-after-drop needs session token survival.

**Implications (I)**
- *Architectural:* Client is now a thin renderer; legacy "load pak from disk" path can be deprecated.
- *Cross-system:* C13 deep-link protocol drives [06_WEBSITE] Play + [12_INFRASTRUCTURE] installer + [15_MOBILE] universal links.
- *Migration:* old projects that assumed Client-loads-pak now run through an embedded server transparently.
- *Operational:* localhost embedded server for solo = same QUIC stack tested both modes.
- *Support:* "Play button does nothing" = #1 ticket category before this lands.
- *Strategic:* THE conversion event for the whole platform.

**Risks (X)**
- X2.1 Token leaked via clipboard / shoulder-surf → unauthorised join.
- X2.2 Browser refuses to launch `eustress://` (popup blocker, OS denial).
- X2.3 NAT traversal failure for direct QUIC connect (rare with Forge as relay).

**Mitigations (M)**
- M2.1 Short-TTL tokens (e.g. 60 s); single-use.
- M2.2 Fallback message in browser: "Click here to open in installed app" + install prompt.
- M2.3 STUN/TURN via Forge for NAT cases.

---

### Feature 3 — Session manifest from `/play/{sim_id}`

**State:** 🟡 · **Effort:** M · **Risk:** Med · **Touches:** [01], [03_MULTIPLAYER], [06_WEBSITE]
**Sub-features:** session-creation REST call · server endpoint + join-token · initial replication payload · server version check · region echo

**Concept.** When the Client joins, the server sends a small session manifest: world-version, replication channel ID, initial entity snapshot, terms-of-service acceptance flag. **Not** the full project manifest (that lives on the server).

**Forecasted feedback (R)**
- R3.1 Server-version mismatch — Client must refuse old/incompatible versions.
- R3.2 Initial snapshot can be MB-scale at 10k entities; needs progress.
- R3.3 ToS acceptance gate on first multiplayer session.
- R3.4 Region echo for HUD (player sees "us-east-1 · 23 ms").

**Implications (I)**
- *Architectural:* the manifest contract is the Client-server schema boundary.
- *Cross-system:* [03_MULTIPLAYER] defines the message; both ends share `common::networking` types.
- *Migration:* version-skew matrix (`Client v0.4.1 + Server v0.4.0 = compatible?`).
- *Operational:* manifest size = network cost at every player join.
- *Strategic:* lets the website preview "23 ms · 47 players · open join" before the user even commits.

**Risks (X)**
- X3.1 Server downgrade-attack (claims older version Client accepts but with malicious payload).
- X3.2 Manifest mid-load disconnect leaves Client in half-spawned limbo.

**Mitigations (M)**
- M3.1 Pin minimum server version per Client release; refuse below it.
- M3.2 Atomic manifest apply (despawn-everything-on-disconnect contract).

---

### Feature 4 — Asset fetch via server proxy

**State:** 🟡 · **Effort:** M · **Risk:** Med · **Touches:** [01], [03_MULTIPLAYER], [04_ASSET_PIPELINE], [05_SPACE_STREAMING], C16
**Sub-features:** asset hash in replication payload · QUIC asset stream (or HTTP fallback) · LRU disk cache `~/.cache/eustress/assets/` · CRC verification · per-platform variant request (C16)

**Concept.** Client receives an `AssetHash` over replication for an entity → looks up local cache → if miss, requests bytes from the server (server proxies from R2 / its disk). Per-platform variant chooser (BC7 desktop, ASTC mobile) per C16. P5 might add direct-R2 fetch from Client.

**Forecasted feedback (R)**
- R4.1 Server proxy is simpler and avoids R2 auth on Client side; trade is 2× bandwidth (R2 → server → Client).
- R4.2 Concurrent-asset cap configurable; mobile lower.
- R4.3 Cache eviction policy LRU; quota per platform.
- R4.4 Hash verify must be streaming (don't block on 200 MB GLB).
- R4.5 Per-platform variant request via `accept-platform: macos-arm64,bc7,linear`.

**Implications (I)**
- *Architectural:* one transport (QUIC) → simpler firewall story than HTTP + QUIC.
- *Cross-system:* [04_ASSET_PIPELINE] caching, [05_SPACE_STREAMING] chunk delivery share this asset stream.
- *Migration:* if direct-R2 ships later, server-proxy fallback stays for restricted networks.
- *Operational:* server bandwidth cost scales with player count × asset churn.
- *Support:* "stuck on loading X" diagnostic needs per-asset progress.

**Risks (X)**
- X4.1 Asset poisoning if a peer joins a private server and serves bad assets — server-side allowlist needed.
- X4.2 Cache corruption across power-loss; periodic CRC scrub.

**Mitigations (M)**
- M4.1 Server-side asset allowlist matches the project's manifest hashes.
- M4.2 Cache index `manifest.json` validated on Client startup.

---

### Feature 5 — Enhancement pipeline (on-demand AI generation)

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [01], [04_ASSET_PIPELINE], [07_AI_PLATFORM], C8
**Sub-features:** 6 chained Bevy systems · SHA256 cache key `(prompt, category, detail)` · distance trigger · 2-thread concurrency · async future → handle apply

**Concept.** Distance-based enhancement scheduling. When the player nears a placeholder entity, hash → cache lookup → if miss, dispatch to the generation server ([07_AI_PLATFORM] Feature 9 — FLUX texture / TripoSR mesh / LLM narrative). Replace placeholder with generated asset.

**Forecasted feedback (R)**
- R5.1 Generation server contract is hardcoded `http://127.0.0.1:8002` — see Feature 17.
- R5.2 Mesh placeholder + generated GLB are both spawned → visual duplication.
- R5.3 Unload-on-distance is a function stub; long sessions OOM.
- R5.4 No cache eviction policy; disk fills indefinitely.
- R5.5 Quest LLM narrative call is a forward-declared stub; runtime panic if connection fires.
- R5.6 C8 consent: `ai = true` flag must filter (per-entity) what's allowed to enhance.
- R5.7 Two-player determinism: cache hash must include engine + model version.

**Implications (I)**
- *Architectural:* enhancement is the Client's USP; gate behind `--enhancement` flag until robust.
- *Cross-system:* [07_AI_PLATFORM] Feature 9 owns the generation contract; Client just consumes.
- *Migration:* swapping FLUX → FLUX 2.0 invalidates the cache; version-tag included.
- *Operational:* generation cost is real money; per-session budget surface in HUD.
- *Support:* "AI is broken" tickets need the gen-server status indicator.
- *Strategic:* the demo moment — get this right.

**Risks (X)**
- X5.1 Generation server outage → enhancement silently no-ops; player sees placeholder forever.
- X5.2 Determinism break across two players' caches → different content.

**Mitigations (M)**
- X5.1 Procedural fallback when gen server unreachable.
- X5.2 Cache key includes `(engine_version, model_signature)`.

---

### Feature 6 — Quest / scene-connection executor

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [01], [03_MULTIPLAYER], [07_AI_PLATFORM], [10_TELEMETRY]
**Sub-features:** condition AST · inventory + flags · visited-connection tracking · LLM narrative generation · per-player vs. world-state authority

**Concept.** Scene connections (doors, portals, dialog) have conditions referencing flags / inventory / NPC state. Executor evaluates, optionally calls an LLM for branching narrative, applies effects.

**Forecasted feedback (R)**
- R6.1 Condition parser is two-line; won't handle `NOT`, `OR`, list-membership.
- R6.2 Quest state in memory only; restart loses progress.
- R6.3 LLM narrative call: `spawn_llm_narrative_generation()` is declared but undefined — runtime panic.
- R6.4 Multiplayer divergence: two players hit same door → who's authoritative?
- R6.5 No editor surface for authoring connections.
- R6.6 No `quest.trace` stream topic (debug-ability).
- R6.7 Condition syntax has no version field; engine upgrade can silently break old projects.

**Implications (I)**
- *Architectural:* server-authoritative quest state for multiplayer; design now.
- *Cross-system:* schema in `common::scene` shared with Studio + [03_MULTIPLAYER]; LLM call routes through [07_AI_PLATFORM] FoundationModelDispatcher.
- *Migration:* version condition syntax.
- *Operational:* LLM call cost per-event; budget surface.
- *Support:* "door didn't open" without a trace is unsolvable.
- *Strategic:* the bridge from sandbox to AI-native narrative — until this lands, the Client is just a renderer.

**Risks (X)**
- X6.1 Hardcoded `127.0.0.1:8002` for LLM = stall when unreachable.
- X6.2 Inventory desync between Client and server in coop.

**Mitigations (M)**
- M6.1 LLM call timeout + fallback text.
- M6.2 Server is authority for inventory writes; Client predicts.

---

### Feature 7 — Soul scripting (Rune + Luau)

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [01], [02_STUDIO], [07_AI_PLATFORM], [17_PLUGIN]
**Sub-features:** Rune VM · Luau VM · ECS bindings · GUI bridge · `Units` API · hot-reload watcher · filesystem sandbox

**Concept.** Two VMs side-by-side. Rune for Rust-native users, Luau for Roblox-familiar. ECS bindings, GUI bridge, Units API ([UNITS.md](../UNITS.md)).

**Forecasted feedback (R)**
- R7.1 Hot-reload watcher polls correctly but matches only `.md` extensions — `.rune` / `.lua` edits silently ignored.
- R7.2 Rune vs. Luau parity table needs publication.
- R7.3 mlua has filesystem reach beyond project root — sandbox required.
- R7.4 Runtime errors must surface in-game HUD + stream tee (Feature error topic).
- R7.5 Memory leaks: scripts holding ECS handles need `on_disconnect`.
- R7.6 `Units.from_meters` / `Units.to_meters` lacks Client-side sample script.

**Implications (I)**
- *Architectural:* two VMs = maintenance tax forever; justify via different audiences.
- *Cross-system:* binding generator (`rune_ecs_module.rs` + `luau/runtime.rs`) must stay symmetric.
- *Migration:* Rune version pin matters for [17_PLUGIN] LSP compatibility.
- *Operational:* sandbox violations are security; jailing is a must.
- *Support:* "my script doesn't reload" → fix Feature 7.1 bug.
- *Strategic:* anti-cheat for client-side scripts (cosmetic only; server is authority for game state).

**Risks (X)**
- X7.1 `.rune` hot-reload broken silently — confirmed P3 finding.
- X7.2 Sandbox escape (script reads `/etc/passwd`) = security incident.

**Mitigations (M)**
- M7.1 Fix the extension match in `HotReloadWatcher`.
- M7.2 Path-jail at mlua bind time.

---

### Feature 8 — Networking / replication

**State:** 🟡 · **Effort:** L · **Risk:** High · **Touches:** [01], [03_MULTIPLAYER], C7, C11
**Sub-features:** QUIC transport · `Replicated` component · `NetworkTransform` / `NetworkVelocity` · `NetworkOwner` · AOI culling · client-side prediction · reconciliation

**Concept.** `DistributedWorldPlugin` mounted; `eustress-networking` library defines Replicated / NetworkTransform / NetworkVelocity / NetworkOwner. Client receives deltas, predicts inputs, reconciles on server snapshot.

**Forecasted feedback (R)**
- R8.1 `BasePart ↔ NetworkTransform` end-to-end sync verified absent in earlier audit — P3 critic says core lib is more complete than P1 thought.
- R8.2 Client prediction code: input buffer + replay-from-tick missing.
- R8.3 Avian deterministic-step cross-platform unproven (see [11_SIMULATION] Feature 8).
- R8.4 Lightyear is concept-influenced, not a direct dep — clarify.
- R8.5 Reconciliation snap visible without smoothing.

**Implications (I)**
- *Architectural:* prediction-and-reconcile is the Client's hot loop in multiplayer.
- *Cross-system:* [03_MULTIPLAYER] owns server-auth validation; Client trusts server's truth.
- *Migration:* solo-mode-first (embedded server) is a valid intermediate.
- *Operational:* bandwidth budget at 120 Hz tick × 200 nearby entities = blow past 5 Mbps without priority.
- *Support:* "I rubber-banded" tickets without `replication.*` trace are unsolvable.
- *Strategic:* multiplayer feel is the make-or-break of the platform.

**Risks (X)**
- X8.1 Float drift between platforms diverges predictions → permanent rubber-band.
- X8.2 No reconnect after TCP drop loses session.

**Mitigations (M)**
- M8.1 `--deterministic` flag locks SIMD math paths (cross-platform CRC).
- M8.2 Session resume with the same JWT for N minutes.

---

### Feature 9 — Planetary coords / `WorldPosition`

**State:** 🟠 · **Effort:** L · **Risk:** Med · **Touches:** [01], [11_SIMULATION], [14_GEO_COORDINATES]
**Sub-features:** DVec3 `WorldPosition` Component · `CameraWorldOrigin` Resource · origin rebasing every N km · sync `Transform` from `WorldPosition` in `Last`

**Concept.** f32 Transform breaks at ~10⁵ m. f64 WorldPosition stored, f32 Transform synthesised relative to a camera-tracking origin that snaps periodically. Avian steps in local frame; rebase post-step.

**Forecasted feedback (R)**
- R9.1 No system uses `WorldPosition` today; entirely f32 Transform.
- R9.2 Avian rebase ordering — must be in `Last`.
- R9.3 Origin-rebase artifact for 1 frame if not scheduled correctly.
- R9.4 Cross-platform f64 determinism for multiplayer prediction.
- R9.5 Astronomy (sun pos, time-of-day) depends on lat/lon.

**Implications (I)** — see [14_GEO_COORDINATES] for full coverage.
- *Architectural:* opting in flips every gameplay system that reads Transform.
- *Cross-system:* [03_MULTIPLAYER] prediction-determinism cross-platform.
- *Strategic:* Earth-scale demos are the differentiator.

**Risks (X)** — X9.1 Project that mixes WorldPosition and Transform → ghost positions.

**Mitigations (M)** — M9.1 Migration tool: project-level "go planetary" toggle.

---

### Feature 10 — Character / locomotion / animation

**State:** 🟡 · **Effort:** L · **Risk:** Med · **Touches:** [01], [03_MULTIPLAYER], [04_ASSET_PIPELINE]
**Sub-features:** R6/R15 rig · skinned mesh · state machine (idle/walk/run/jump/fall/sit/swim) · `LocomotionController` · IK retargeting · footstep events

**Concept.** Spawned player gets a humanoid model, animation controller, locomotion. Procedural overlays (hip bob, body lean).

**Forecasted feedback (R)**
- R10.1 State machine is declared; transitions unverified.
- R10.2 `Humanoid.WalkSpeed` / `HipHeight` not read by animation system → fixed-speed walk.
- R10.3 Retargeting causes foot sliding.
- R10.4 Skinned mesh import from the `.eustress` world container untested.
- R10.5 Footstep audio + particles hooks missing.
- R10.6 Network sync: server says "walking forward"; Client interpolates → message design needed.

**Implications (I)**
- *Architectural:* the animation pipeline owns `Motor6D.DesiredAngle` per frame; physics order matters.
- *Cross-system:* [03_MULTIPLAYER] message for "humanoid state"; [04_ASSETS] rigged-mesh import; [15_MOBILE] gyro for free-look.
- *Operational:* poor character feel is universally noticed first.
- *Strategic:* character feel is critical path.

**Risks (X)** — X10.1 Mid-animation desync between Client and server → twitchy character.

**Mitigations (M)** — M10.1 Server tags animation events with tick; Client clamps to tick window.

---

### Feature 11 — Mobile player parity

**State:** 🔴 · **Effort:** XL · **Risk:** High · **Touches:** [01], [15_MOBILE_PLATFORM]
**Sub-features:** see [15_MOBILE_PLATFORM.md](15_MOBILE_PLATFORM.md) — shared Rust core, OS shells, touch, lifecycle, IAP, asset variants

**Concept.** The same engine runs on iOS + Android via `player-mobile`. Detailed audit is now in [15_MOBILE_PLATFORM](15_MOBILE_PLATFORM.md). This row remains as a Client-side pointer.

**Forecasted feedback (R)** — see [15_MOBILE_PLATFORM] Features 1–12.

**Implications (I)**
- *Architectural:* the Client must abstract platform-specific input/render/lifecycle traits (shared with [15]).
- *Strategic:* mobile is a category-defining feature; half-shipped = retention disaster.

**Risks (X) / Mitigations (M)** — see [15_MOBILE_PLATFORM] cards.

---

### Feature 12 — Graceful degradation

**State:** 🟠 · **Effort:** M · **Risk:** Med · **Touches:** [01], [07_AI_PLATFORM], [09_ECONOMY]
**Sub-features:** offline mode · gen-server unreachable fallback · server unreachable · R2 unreachable · degraded HUD indicator · queued-state retry

**Concept.** When back-ends are unreachable, the Client still plays. Placeholder meshes instead of AI-generated. Embedded local server instead of cloud. Local cache instead of R2.

**Forecasted feedback (R)**
- R12.1 Today some paths panic; no documented degraded contract.
- R12.2 Offline play (LAN party, plane) is a captured user story.
- R12.3 Connection-state HUD shows R2 / server / AI reachability.
- R12.4 Generated-content fallback reuses placeholder + procedural texture.

**Implications (I)**
- *Architectural:* every external dependency needs an `is_reachable()` check + degraded path.
- *Cross-system:* [10_TELEMETRY] receives `degraded.*` events for ops; [12_INFRASTRUCTURE] uses signal to scale up failing regions.
- *Strategic:* reviewers *deliberately* test offline; degraded modes are part of the product.

**Risks (X)** — X12.1 Silent feature-off (e.g. quests stop firing) is worse than a clear error toast.

**Mitigations (M)** — M12.1 Persistent toast: "AI unavailable — projects feel different until restored".

---

### Feature 13 — Save game / state persistence  *(P3 add)*

**State:** 🔴 · **Effort:** M · **Risk:** Med · **Touches:** [01], [15_MOBILE], [16_PERSISTENCE]
**Sub-features:** quest state · inventory · flags · per-project save slot · auto-save tick · cloud-sync (Premium)

**Concept.** Save data lives at `~/.config/eustress/saves/{project_id}/`. Auto-save every N seconds + on quit. Premium-tier cloud sync. See [16_PERSISTENCE_DATASTORE] Feature 6.

**Forecasted feedback (R)**
- R13.1 No persistence today — quest progress dies on exit.
- R13.2 Save format: TOML (debuggable) vs. binary postcard (small)?
- R13.3 Mobile lifecycle (Feature 11 / [15]) needs auto-save on backgrounding.
- R13.4 Cloud-sync conflict (desktop + mobile) = last-writer-wins or merge?

**Implications (I)**
- *Architectural:* save schema versioning is critical (engine upgrade breaks old saves otherwise).
- *Cross-system:* [16_PERSISTENCE] owns the storage interface; Client is a consumer.
- *Migration:* mid-version engine upgrade must auto-migrate saves.
- *Operational:* cloud-sync uses Identity-bound storage; affects [08] + [09].
- *Strategic:* persistent player state = retention.

**Risks (X)** — X13.1 Save corruption mid-write on crash.

**Mitigations (M)** — M13.1 Atomic write-temp + rename; M13.2 double-buffered slots.

---

### Feature 14 — Audio device handling  *(P3 add)*

**State:** 🟡 · **Effort:** S · **Risk:** Low · **Touches:** [01]
**Sub-features:** `bevy_audio` enabled · device enumeration · fallback (default → null) · `EUSTRESS_AUDIO_DEVICE` env var · headless mode

**Concept.** `bevy_audio` is enabled (memory `feedback_audio_required`); but missing-default-device crashes the app silently. Fallback chain: env var → default device → null sink (silent OK).

**Forecasted feedback (R)**
- R14.1 Headless / CI / WSL all lack default audio device.
- R14.2 Volume slider in Settings panel persists per-user.
- R14.3 Spatial audio quality on mobile is platform-specific (Oboe Android, AVAudioEngine iOS).

**Implications (I)** — *Operational:* audio failures are silent today; loud users notice.

**Risks (X)** — X14.1 Device disappear (Bluetooth headset off) crashes mid-game.

**Mitigations (M)** — M14.1 Hot-detach handler in `cpal`/`oboe`.

---

### Feature 15 — Display scaling / DPI  *(P3 add)*

**State:** 🔴 · **Effort:** S · **Risk:** Low · **Touches:** [01], [02_STUDIO], [15_MOBILE]
**Sub-features:** DPI detection (winit / OS) · UI scale factor · `workspace.TargetDPI` override · per-user settings · multi-monitor differing-DPI

**Concept.** Hardcoded 1920×1080. Linux HiDPI = tiny UI. Read DPI from winit, scale UI accordingly. Persist per-user.

**Forecasted feedback (R)**
- R15.1 4K displays without scaling = unreadable.
- R15.2 Per-project `TargetDPI` lets creators design for a specific target.
- R15.3 Multi-monitor with mixed-DPI is non-trivial in Bevy.

**Implications (I)** — *Architectural:* Slint UI must respect scale factor (it does); Bevy rendering does too at viewport level.

**Risks (X)** — X15.1 Wrong scale on launch is a bad first impression.

**Mitigations (M)** — M15.1 Detect on launch; let user override in Settings.

---

### Feature 16 — Boot error recovery / panic-to-dialog  *(P3 add)*

**State:** 🔴 · **Effort:** M · **Risk:** Med · **Touches:** [01], [10_TELEMETRY], [12_INFRASTRUCTURE]
**Sub-features:** panic hook → native error dialog · Sentry minidump upload · recovery action (relaunch with `--safe-mode`) · PII scrub · symbolication

**Concept.** Missing asset / corrupt cache / failed init → stderr panic today. Replace with: native dialog "Eustress crashed: <short>. Send report?" → minidump uploaded to Sentry → user offered safe-mode relaunch.

**Forecasted feedback (R)**
- R16.1 No try-catch around plugin init.
- R16.2 Crash reports invisible if user just quits.
- R16.3 PII scrub before send (project paths, user IDs).
- R16.4 Symbolication via release-build pdb/dsym upload at CI time.

**Implications (I)** — *Operational:* crash visibility is high-leverage (top 5 crashes = 80% impact).

**Risks (X)** — X16.1 PII leakage if scrubber misconfigured.

**Mitigations (M)** — M16.1 Test scrubber on real crashes pre-launch.

---

### Feature 17 — Generation-server HTTP contract  *(P3 add)*

**State:** 🟠 · **Effort:** M · **Risk:** Med · **Touches:** [01], [07_AI_PLATFORM]
**Sub-features:** `POST /texture` / `/mesh` / `/narrative` · zstd response · SHA256 key · retry + backoff · health endpoint · shared client crate

**Concept.** Today the LLM narrative + enhancement pipeline call hardcoded URLs (`127.0.0.1:8002` / `:8001`). Refactor into a shared `GenerationServer` client crate; configurable URL; standard zstd response decoding; retry/backoff; health probe.

**Forecasted feedback (R)**
- R17.1 No shared client; every call site duplicates HTTP code.
- R17.2 No zstd decoding wrapper.
- R17.3 No retry/backoff.
- R17.4 No health probe → degraded mode unaware.

**Implications (I)** — *Cross-system:* [07_AI_PLATFORM] Feature 9 also calls; share crate.

**Risks (X)** — X17.1 Server slow → frame stall.

**Mitigations (M)** — M17.1 Async-only; never block frame.

---

### Feature 18 — Frame-rate / VSync override  *(P3 add)*

**State:** 🔴 · **Effort:** S · **Risk:** Low · **Touches:** [01], [15_MOBILE]
**Sub-features:** `workspace.MaxFrameRate` · VSync mode (Off / Fifo / Mailbox) · per-platform cap · adaptive sync (variable-rate)

**Concept.** VSync hardcoded `Fifo`. Mobile users want 60-cap to save battery; desktop power-users want uncapped + Mailbox. Read from project + per-user override.

**Forecasted feedback (R)**
- R18.1 Hardcoded VSync = no power-user control.
- R18.2 Mobile defaults must cap to save thermal/battery.
- R18.3 Per-platform tuning surface.

**Implications (I)** — *Operational:* battery drain on mobile is a top complaint.

**Risks (X)** — X18.1 Tearing if Off + variable refresh GPUs misconfigured.

**Mitigations (M)** — M18.1 Detect VRR; default Mailbox if supported.

---

## Wiring / import gaps (rolled up across P1–P4)

1. OS protocol handler registration for `eustress://` (C13) — Win MSI + macOS `LSApplicationURLTypes` + Linux `.desktop`
2. Join-token validation in QUIC handshake ([03_MULTIPLAYER] Feature 8 + this)
3. Session manifest schema (`common::networking::SessionManifest`)
4. Asset-stream protocol over QUIC (replaces hypothetical direct R2)
5. Generation-server shared client crate (Feature 17)
6. Mesh-replacement (despawn placeholder) in `apply_loaded_assets_system`
7. Asset unload pass implementation (`unload_distant_assets_system`)
8. Hot-reload extension match fix (`HotReloadWatcher` matches `.rune` + `.lua` + `.md`)
9. `WorldPosition` + `sync_render_positions` (planetary mode opt-in)
10. Network prediction + reconciliation systems
11. Mobile shell links to `player-mobile` (see [15_MOBILE_PLATFORM])
12. Save game writer / loader (Feature 13)
13. Audio device fallback chain (Feature 14)
14. DPI detection + per-user scale (Feature 15)
15. Panic hook + Sentry minidump (Feature 16)
16. `workspace.MaxFrameRate` reader + VRR detection (Feature 18)

---

## Cross-system dependencies

- **C1 / Units** — honour `metadata.unit` per [UNITS.md](../UNITS.md).
- **C2 / Canonical create** — every replicated spawn routes through `create_instance`.
- **C3 / Single-author storage** — file-system-first was the prior architecture; superseded 2026-05-15 by the Fjall WorldDb store (MASTER C17). Client loads from the extracted `.eustress` world container (Fjall database + baked `.echk` chunks); TOML retained as legacy/seed + human-editable schema.
- **C4 / Stream-tee** — gameplay events → [10_TELEMETRY].
- **C7 / Avian only** — never link Rapier transitively.
- **C8 / AI consent** — `ai = true` filter on enhancement.
- **C11 / `.eustress` world container** — Client never downloads; server does. *(Updated 2026-05-16: payload is the `.eustress` Fjall WorldDb container + baked `.echk` chunks, not `.pak`.)*
- **C13 / `eustress://` protocol** — Client claims the URL scheme.
- **C16 / per-platform asset variants** — Client requests via `accept-platform` header.
- Depends on **[03_MULTIPLAYER]** replication; **[04_ASSET_PIPELINE]** asset resolver; **[05_SPACE_STREAMING]** chunk consumer; **[07_AI_PLATFORM]** generation server; **[15_MOBILE]** for the mobile half; **[16_PERSISTENCE]** for save data.

---

## Open questions

- Q1.1 Embedded-server-for-solo: same binary or separate?
- Q1.2 Drag-drop offline-pak path supported?
- Q1.3 Workshop AI panel in shipped Client (dev mode) or strictly play-only?
- Q1.4 Update strategy — auto on launch, on next launch, manual?
- Q1.5 LSP / dev console on shipped Client — flag or never?
- Q1.6 Save schema versioning policy.
- Q1.7 Audio engine choice on mobile (Oboe vs. cpal Android).
