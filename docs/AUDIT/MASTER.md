# Eustress Platform — Concept vs. Implementation Audit

> **Master checklist.** Each subsystem has a dedicated file in this folder.
> Pass through every system, deepen the concept ↔ implementation gap, list
> as many forecasted-feedback and implication bullets as you can per item,
> then iterate.

## Ground truth — the state every row below is measured against

Five facts about the tree itself. They bound how much of this audit is checkable, so they precede it.

**1. The workspace compiles.** `cargo build --workspace` exits 0 as of 2026-08-08 at commit
`71ccf6fe`, across **40 members**, and exits 0 again on an immediate incremental rerun with no
`cargo clean` between them. Three members were broken, from three unrelated causes:

| Member | Cause | Remedy |
| --- | --- | --- |
| `eustress-backend` | `crates/backend/src/marketplace.rs` calls four `Database` methods that `crates/backend/src/db.rs` never defines — `find_marketplace_item_by_id`, `has_purchased`, `get_user_balance`, `purchase_item` | Removed from `[workspace] members`; 41 → 40. Unmodified on disk, no longer built. See [09_ECONOMY](09_ECONOMY.md) |
| `benches/instance-capacity` | Eight wgpu-29 API drifts the Bevy 0.19 migration missed | Fixed in place |
| `eustress-server` | Uses `serde_json::Value` without declaring the dependency | Fixed in place |

**2. The root cause is CI, and it is shared by all three.** No CI job compiles more than one package.
`.github/workflows/ci.yml` runs `cargo deny`, a naga shader pass, and a `cargo tree` grep;
`.github/workflows/linux-engine.yml:48` runs `cargo check --package eustress-engine`;
`release.yml` builds the same lone package. **No CI job runs `cargo test` at any point**, so the
workspace's **2,070 `#[test]` and `#[tokio::test]` functions** across `eustress/crates` and
`eustress/benches` have never executed in continuous integration. Three of forty members could rot
unnoticed because nothing ever built them, and any subsystem state in this audit that rests on "the
tests cover it" rests on tests no automated system has ever run.

**3. A control the code says is wired is not necessarily a control a user can reach.**
`eustress/crates/engine/src/tool_metadata.rs:1479` marks `insert:script` as `wired: true`, and that
flag is honest about what it asserts — a handler exists. It asserts nothing about reachability.
**Insert Script has a measured click-depth of −1: there is no mouse route to creating a script from a
cold start at all.** Reachability and wiring are two different audits, and only the first is a
statement about users. See [02_STUDIO_ENGINE](02_STUDIO_ENGINE.md).

**4. 64 of 104 customer-facing quantitative claims are UNSUPPORTED** — 61.5%, against 12 MEASURED,
19 TARGET and 9 CONFIG_DEFAULT. Any figure quoted outward is presumed unsupported until it is
classified. Remediation is deferred and carried as an open debt; it is not written off, and the
threshold that caught it was deliberately not relaxed.

**5. Licence phrasing is correct everywhere and the check is not automated.** Every licensing
misstatement is corrected across 6,315 scanned files — 27 unallowlisted hits reduced to 0 — and the
licence is stated as **PolyForm Shield 1.0.0, source-available**, never as open source. A verifier at
`docs/PROMPTS/harness/checkers/licence_phrasing.py` exits 5 on any unallowlisted hit. **It is
CI-ready and unwired**, so today the correctness is a fact about the tree, not a property it is
prevented from losing.

---

## Iteration tracker

| Pass | Date | Driver | Focus | Output |
| --- | --- | --- | --- | --- |
| **1** ✅ | 2026-05-14 | Studio session | Initial scaffold, six systems, metric dashboard, cross-cuts (C1–C10) | MASTER + 6 system files; 65 features, 588 R+I bullets |
| **2** ✅ | 2026-05-14 | Studio session | Fix `.eustress → .pak`; rewrite Streaming as **Space Streaming**; add Multiplayer Studio; six new systems (AI, Identity, Economy, Telemetry, Sim, Infra); per-feature card format; deepen Implications | MASTER pass 2 + 12 system files + factual corrections |
| **3** ✅ | 2026-05-14 | Studio session | **Criticism pass.** Fix unreadable top-of-doc tables (3-col). Critique-driven addenda to 01/02/03/04/06 (factual errors, missing features, shallow-implication deepening). State corrections to 08+12. **Add all 8 new system docs**: 13 Terrain, 14 Geo, 15 Mobile, 16 DataStore, 17 Plugin, 18 CAD, 19 Realism, 20 Search. | All 5 P2 tables shrunk · 5 P3 addenda · **8 new docs** · MASTER P3 |
| **4** ✅ | 2026-05-14 | Studio session | **Full retrofit** of 01/02/03/04/06 to per-feature-card format (R/I/X/M with multi-category Implications). Addendum blocks removed; content baked into cards. **Secondary critique** dispatched over 07–20; state corrections to 07/08/09/10/11/12/14/16/19. **C13–C16** added (`eustress://` protocol, KYC-deferred OAuth, plugin sandbox = WASM-first, per-platform asset variants). | 5 docs rewritten · 8 docs with state corrections · 4 new cross-cuts · MASTER P4 |
| **5** ✅ | 2026-05-16 | Studio session | **Storage pivot.** Engine moved from file-system-first (`_instance.toml` folders) to a **Fjall log-structured-merge-tree key-value store behind the `WorldDb` trait**. `.eustress` is the world-container directory (holds `world.fjalldb/`, `header.bin`, `chunks/*.echk` + `manifest.toml`, human-editable `schema/` + service folders); `.pak` is **no longer** the published format. Corrections folded into 01–06, 16, MASTER P2-corrections + C11/C12 + new C17. | 21 files reviewed; `.pak`→`.eustress` corrections; C11 rewritten, C12 rewritten, **C17 added (WorldDb storage standard)** |
| 6 | _pending_ | — | _pending user direction_ — candidate work: production-readiness sprint (close every 🔴 in 01–20); tighten metrics-driven dashboards in MASTER; produce a single "P0/P1/P2 punch list" derived from all 20 docs; per-system roadmap with effort buckets | — |

### P4 critique findings (consolidated; per-doc state corrections applied)

P4 dispatched 3 critique agents over 07–20. Key state corrections + missing features now folded into the affected docs:

**07_AI_PLATFORM** — MCP `SubscribeTopic` confirmed absent (no variant in `BridgeRequest::MethodName`). FoundationModelDispatcher is **fully absent** (not "pseudocode") — `APEX_ENGINE.md` referenced but missing. Spatial-LLM is **stub** (no Claude calls); should be 🔴 not 🟡. Image-to-Code two-step flow remains fragile.

**08_IDENTITY_TRUST** — The Cloudflare Worker's `JURISDICTIONS` ontology is **populated**: **46 country entries** at `infrastructure/cloudflare/api/src/index.js:39-98`, each carrying `name`, `natural_ids`, `r2_prefix`, and `age_of_majority`, plus a `fallback` requiring one of passport / national ID / driver's licence for anything unlisted. The "72 countries" claim overstates coverage by 26 and must be restated as 46-plus-fallback; the data itself is real and KYC is not blocked on it. Succession `create()` + `verify()` are signature-complete but **activation logic (inactivity timer, heir-claim window, proof-of-life) is 0%** — true state is ~40%, not 80%. `.pak` signing dependency for asset chain not yet wired.

**09_ECONOMY** — **Marketplace state inflated**: P2/P3 said 75% but `purchase_item` handler calls `state.db.purchase_item()` with no Bliss-debit logic — true state is **🟡 40%**. Stripe Connect (Feature 3) is **gated by KYC (Feature 14 in [08])** — not independent 5%; effective state is **🔴 0%** until [08] lands. Bliss dual-nature (arcade token + Proof-of-Contribution crypto) carries **regulatory arbitrage risk** if BLS gains market value — needs legal counsel before public launch. **The marketplace backend no longer builds and is unlisted from the workspace**: `crates/backend/src/marketplace.rs` calls `find_marketplace_item_by_id`, `has_purchased`, `get_user_balance` and `purchase_item` on `state.db`, none of which `crates/backend/src/db.rs` defines. Three of the four are mechanically implementable — the first is a naming mismatch against `get_marketplace_item_by_id` at `db.rs:282`, and the other two map onto the existing purchases table. `get_user_balance` is not: there is **no balance table, column or method anywhere in `db.rs`**, and `db.rs:5` records that user identity lives in Cloudflare KV rather than in this database. **Where a Bliss balance lives is an unmade design decision, and the marketplace's 40% state is blocked on it, not on code.** The crate is unmodified on disk; re-listing it in `[workspace] members` is a one-line revert once that decision exists.

**10_TELEMETRY** — Doc **contradicts itself**: line 56 says "default off"; line 264 mitigation M2.1 says "default `history.*` + `workshop.tool.*` persistent". Resolve to: **per-topic opt-in; `history.*` + `workshop.tool.*` default persistent; everything else opt-in**. At claimed 85M msg/sec throughput with 1 KB avg msg + 7-day retention → 73 TB/week — persistence plan is missing. Sampling strategy undefined per-topic.

**11_SIMULATION_DEBUGGER** — Watchman cooldown is **wall-time, not sim-time** — at 10⁶x scale, miss sub-30-second spikes. V-Cell electrochemistry is **lumped 0-D** (no spatial electrochemistry / ion transport) — validation gap vs. real cells not flagged. `ElectrochemicalState` and `ThermodynamicState` are **coupled in both directions**: `eustress/crates/engine/src/simulation/electrochemistry.rs:208-219` reads `ThermodynamicState.temperature` into the kinetics and passes it to the temperature-dependent Nernst, Butler-Volmer, and Tafel terms in `eustress/crates/common/src/realism/laws/electrochemistry.rs`, and `:279-292` feeds `heat_generation` back into the temperature with a Newton cooling term. The real gap is that the thermal node is lumped and its constants are hardcoded — `thermal_mass` at `:281` and `r_thermal` at `:288` — so there is no spatial gradient and no temperature dependence of the transport properties. Symbolica `use symbolica::atom::Atom` in `causal.rs` — **partially wired**, not "concept only"; solver.rs line 95 says "Full implementation requires Symbolica" but scaffold exists.

**02_STUDIO_ENGINE** — There is **no duplicate `StudioState`**. It is defined exactly once, at `eustress/crates/engine/src/ui/mod.rs:253`, and `eustress/crates/engine/src/ui/slint_ui.rs:218` carries a standing comment forbidding a local redefinition. Drain failures in this surface are a wiring class, not a duplicate-type class, and must be diagnosed as such. **The duplicate that does exist is a plugin, not a state.** `StudioUiPlugin` is defined twice — `slint_ui.rs:1230` (impl at `:1235`) and `ui/mod.rs:1017` (impl at `:1022`) — and **neither is mounted**. The live UI plugin is `SlintUiPlugin`, defined at `slint_ui.rs:1292` with its impl at `:1294`, added exactly once at `main.rs:277`. A registration that lands in `StudioUiPlugin` therefore executes never, which is the mechanism behind two historical whole-UI outages; the tree now carries standing comments at `lib.rs:177`, `class_registry/mod.rs:31` and `ui/spawn_events.rs:540` naming `SlintUiPlugin::build` as the only correct mount point. The **post-processing stack is not blocked on crate availability**: `eustress/crates/engine/Cargo.toml:133-134` declares `bevy_anti_alias` and `bevy_post_process` as live Bevy features, with an adjacent comment recording that `DefaultPlugins` registers them but no camera carries the components, so they are **inert rather than absent**. The work is attaching components to a camera and accepting a Bevy rebuild — not waiting on an upstream release. **Reachability is a separate audit from wiring.** A baseline census of the editor surface — 60 `.slint` files, 32,577 `.slint` lines, 37,533 lines of `ui/*.rs`, largest single file `ui/slint_ui.rs` at 23,103 lines, one `.slint` file carrying an accessibility role — puts **1,999 declared ribbon tools against 30 wired**. Of the top 20 commands most sit at click-depth 1 or 2, but **Insert Script is −1: no mouse route exists from a cold start.** The Insert menu's static items are Part/Sphere/Cylinder/Wedge/Model/Folder; its dynamic tail comes from `ui/insert_classes.rs::build_catalog`, which drops any class without a registered spawner, and `ClassName::LuauScript` has none; neither the viewport nor the Explorer context menu offers a script insert; and `modes/engineering.toml:145` carries `insert:script` only as a mode-manifest tool while `studio_modes.rs:570` leaves `ModeRegistry.active_id` empty at cold start. `tool_metadata.rs:1479` marks it `wired: true` and is not lying — that flag asserts a handler exists, never that a user can reach it.

**12_INFRASTRUCTURE** — Vault references in Nomad job specs but cluster **not deployed** — true state is "10% (references in code, no functional)" not 0%. Consul directory **empty** (verified); needs explicit acknowledgement. macOS notarisation 0% **confirmed** (P3 was right). **CI's coverage is one package and zero tests** — `ci.yml` gates `cargo deny`, a naga shader pass and a `cargo tree` grep; `linux-engine.yml:48` and `release.yml` each build `--package eustress-engine` alone; **no job runs `cargo build --workspace` and no job runs `cargo test`**. That is why three of forty workspace members were found broken from three unrelated causes the first time the whole workspace was compiled (see *Ground truth* above), and why the tree's 2,070 test functions have never run in continuous integration. "Release pipeline production" describes the packaging half accurately and says nothing about verification: the pipeline reliably ships a binary nothing has tested.

**13_TERRAIN_VOXEL** — Heightmap **export** missing (import works, round-trip broken). Brush overlay terrain-normal transform absent (visual artifact). Biome paint: `config.rs` has `splatmap` / `splat_cache` but no painting logic. Brush dirty-rect → [05] chunk re-bake coupling undefined; no throttle contract.

**14_GEO_COORDINATES** — `common/src/orbital/` removed 2026-08-01 as unreachable (no plugin ever registered it). No origin-rebase capability exists in any form. Replacement design is grid-cell coordinates (`i32` cell + cell-local `f32`), specified in [ORBITAL_GRID.md](../architecture/ORBITAL_GRID.md), not implemented. Time-of-day → sun/moon driver not wired from clock.

**15_MOBILE_PLATFORM** — Cargo.toml iOS section is **placeholder comment only** (`# iOS-specific dependencies as needed`); no actual deps. No JNI bridge file. No CI smoke-test for cross-compile.

**16_PERSISTENCE_DATASTORE** — Doc understates: `DataStoreService` struct **exists with `get_datastore()`**; `OrderedDataStore` **has full BTreeMap-backed range-query logic**; `DataStoreBackend` **trait is pluggable** (SQLite, Redis shaped). The gap is **script API bindings**, not the struct themselves. State should be 🟡 50%, not 🔴 0%.

**17_PLUGIN_EXTENSIBILITY** — MCP file-watcher: `is_interesting()` filter is **hardcoded paths**, not subscription-aware. LSP IDE-side extensions are **zero-code**; binary builds but no `vscode-eustress` etc. ABI versioning at plugin load not enforced.

**18_CAD_MESHGEOMETRY** — Bevel + loop-cut have **different walker needs** (edge-ring vs. vertex-ring); doc treats as single blocker. Extrude direction options limited: `both_sides` flag exists but "one-side" (single-direction) missing. FeatureTree re-compute is full-replay on any parameter change → cost concern at 10+ feature trees.

**19_REALISM_PHYSICS** — Symbolica is **partially wired** (`use symbolica` imports + feature flag in ARCHITECTURE.md); state should be 🟡 not 🔴. V-Cell `Nernst + Butler-Volmer` are **lumped 0-D models**; no spatial electrochemistry. Thermal and electrochemistry are **coupled both ways** — temperature enters the rate laws at `eustress/crates/engine/src/simulation/electrochemistry.rs:208-219,245` and heat returns to the thermal state at `:279-292`. What is missing is a spatially resolved thermal model and temperature-dependent transport, not the coupling itself. Fracture mechanics has `fracture_mesh.rs` but **no integration path to Avian** (visualisation only?).

**20_SEARCH_DISCOVERY** — Hash embeddings "useless" overstated — work for *some* applications (deterministic, fast); they're **suboptimal**, not useless. HNSW `M=16 / efConstruction=200 / efSearch=50` tuned for ≤1M vectors; **break-even at higher scale unanalysed**. Per-namespace isolation correct architecturally; **cross-namespace queries** (similar asset to entity?) non-trivial.

---

### P2 corrections applied

> **⚠️ Superseded 2026-05-16 by the P5 storage pivot.** Item #1 below ("`.eustress` is dead → `.pak`") is **OBSOLETE** — the corrected statement is item #1' immediately following it. Items #2 and #3 stand, with the storage clarification noted inline.

1. ~~**`.eustress` is dead** — the published archive format is `.pak` (zstd-compressed tar)…~~ **OBSOLETE (see #1').**
1'. **`.eustress` is the world-container directory (not dead, not `.pak`)** *(P5 correction, 2026-05-16)* — `.eustress` is a directory holding: `world.fjalldb/` (the Fjall log-structured-merge-tree database — live entity-component-system state), `header.bin` (world identity + schema version), after a publish bake `chunks/*.echk` + `manifest.toml`, plus human-editable `schema/` and service folders (scripts, graphical-user-interface TOML, `_service.toml`). **`.pak` is no longer the published/storage format.** The publish flow still uploads to `api.eustress.dev → R2` and the server still downloads + loads, but the archive now contains the Fjall database + baked chunks rather than per-instance TOML; the upload/download mechanism is unchanged. Client still never downloads directly — it opens `/play/{sim_id}` → backend allocates a server → QUIC join.
2. **Streaming was wrong** — what we called "Streaming" in P1 is now correctly named **Telemetry & Observability** (event-topic system: `EustressStream`, history tee, MCP `query_stream_events`, etc.). The real **Space Streaming** is server→client chunked content delivery for ChunkedWorld at 10M+ instance scale via `.echk` binary chunks (now produced by the `eustress-worlddb` `bake_to_echk` path — see [05_SPACE_STREAMING](05_SPACE_STREAMING.md)).
3. **Multiplayer Studio is a separate Studio feature** — collaborative real-time editing with cloud as source of truth (Loro CRDT), distinct from the single-author storage path. ~80% UI scaffold + Loro types exist; ~0% wired. *(Note: the single-author "file-system-first" baseline this contrasted against was itself superseded 2026-05-16 by the Fjall WorldDb store — see C17.)*

### P2 top-level findings

1. **Publish & server-download is real** — Studio publish flow (panel + thumbnail + tar+zstd + multipart R2 upload) works; server load works; no delta updates; no client direct download path needed. *(P5 update 2026-05-16: the upload mechanism is unchanged, but the archived payload is now the `.eustress` world container — Fjall database + baked `.echk` chunks — not the old `.pak` per-instance-TOML tar. See #1' in P2 corrections.)*
2. **Multiplayer Studio scaffold is shockingly complete** — `collaboration.slint` panel, `CollaborationState`, Loro CRDT, `presence_ws.rs`, `HostManager` chunk authority all exist. The wiring (event capture → CRDT op → server broadcast) is the missing layer.
3. **Space Streaming is fully designed and zero-wired** — `.echk` 56-byte packed-instance format, manifest layout, hysteresis radii (500/600/2000 m), and an `active_cap` **CONFIG DEFAULT** of 2.10M entities (never a measured entity count, and paired with no measured frame rate) all in code/docs. No encoder, no decoder, no dispatcher, no client spawner.
4. **AI Platform has 52 tools across 9 Workshop modes** — embedvec HNSW DB ready but no ML embedder; spatial-llm modules drafted; Project Korah architecture only; FoundationModelDispatcher pseudocode.
5. **Identity core crypto is 95% done; recovery / MFA / OAuth / CSAM detection / age gating are 0–20%.**
6. **Economy frontend is 90% complete; backend is 5–10%** — Steam IAP, Stripe Connect, subscription lifecycle, refund handling all stubs.
7. **Telemetry library + TCP/SHM brokers are production** (~85M msg/s in-process); ~80% of producers (Workshop tool, file-watcher, simulation, play-mode) unwired; no Sentry / Prometheus / Grafana.
8. **Simulation is 70% mature** — SimulationClock, watchpoints, breakpoints, V-Cell physics, Watchman alerts all production; script debugger UI, replay/seek, cross-platform determinism absent.
9. **Infrastructure 55%** — release pipeline production; macOS notarisation + Windows authenticode + Vault + Prometheus + multi-region all incomplete. The 55% is a packaging figure: CI compiles one package, never the workspace, and runs no tests at all, so nothing in the pipeline verifies what it ships.

---

## Metric dashboard

Cumulative totals after each pass. Refining an existing row counts toward "refined this pass"; only genuinely new rows bump totals.

| Metric | P1 | P2 | P3 | **P4** |
| --- | ---: | ---: | ---: | ---: |
| Systems audited | 6 | 12 | 20 | **20** |
| Top-level features | 65 | ≈ 165 | ≈ 250 | **≈ 250** (refined; +5 in retrofits) |
| Sub-features (named within rows) | ≈ 290 | ≈ 720 | ≈ 1050 | **≈ 1200** (cards add explicit `Sub-features:` lines) |
| Concept entries (per-item explanations) | 65 | ≈ 165 | ≈ 250 | **≈ 250** |
| Forecasted-feedback bullets (R) | 335 | ≈ 720 | ≈ 1050 | **≈ 1300** (retrofits added depth in 01–06) |
| Implication bullets (I) | 253 | ≈ 540 | ≈ 800 | **≈ 1100** (multi-category structure in 01–06) |
| Risk bullets (X) | 0 | ≈ 130 | ≈ 200 | **≈ 350** (every card in 01–06 has X) |
| Mitigation bullets (M) | 0 | ≈ 110 | ≈ 170 | **≈ 300** (every card in 01–06 has M) |
| Wiring / import gaps flagged | 86 | ≈ 200 | ≈ 295 | **≈ 350** (retrofit pass exposed more) |
| Cross-cutting concerns (C1–CN) | 10 | 12 | 12 | **16** *(+ C13 `eustress://`, C14 OAuth-deferred-KYC, C15 WASM-sandbox, C16 per-platform variants)* — **17 at P5** *(+ C17 WorldDb storage standard; C3/C11/C12 rewritten)* |
| Open questions | 25 | ≈ 80 | ≈ 130 | **≈ 160** |
| **P0 escalations** | — | — | 3 | **3** *(JWT auth: HTTP backend enforcement VERIFIED on every protected handler 2026-05-22 (extract_token + validate_token, claims.sub, projects.rs owner_id checks; browse endpoints public by design) — only the QUIC-handshake JWT remains (play_server with_no_client_auth), sequenced to V1.1 multiplayer · Play button · macOS notarisation)* |
| **State corrections in P4 critique** | — | — | — | **13** *(02 StudioState, 02 post-stack, 07 Spatial-LLM, 08 jurisdictions, 08 succession, 09 marketplace, 09 Stripe, 10 persistence, 11 Watchman, 11/19 electrochemistry-thermal coupling, 12 Vault, 14 HybridPosition, 16 DataStoreService, 19 Symbolica)* |

### P2 per-doc deltas

| Doc | Pass 2 action | Net new rows | Notes |
| --- | --- | ---: | --- |
| 01_CLIENT_PLAYER | Fact-check: `.pak`, Client is thin-renderer (not pak-downloader). Architecture overhaul on row 2/3/4. | 0 new, 4 corrected | Pass 3 should retrofit per-feature cards |
| 02_STUDIO_ENGINE | Add Feature 15: Multiplayer Studio (Loro CRDT, presence WS, collaboration panel) | +1 | UI scaffold 80% / wiring 0% |
| 03_MULTIPLAYER | (untouched in P2 — pass 3 to deepen) | 0 | |
| 04_ASSET_PIPELINE | Fact-check: `.pak` publish flow inserted; remove vague `.eustress` references | 0 new, 2 corrected | |
| 05_STREAMING → 05_SPACE_STREAMING | **Full rewrite.** Server→client chunked content delivery, `.echk` binary, hysteresis radii, 2.10M-entity `active_cap` **CONFIG DEFAULT** | +9 | Old content moved to 10_TELEMETRY |
| 06_WEBSITE | (untouched in P2 — pass 3 to deepen) | 0 | |
| **07_AI_PLATFORM** *(new)* | Workshop, Claude, FLUX, TripoSR, embedvec, spatial-llm, Korah, MCP server | +15 | |
| **08_IDENTITY_TRUST** *(new)* | Ed25519, KYC, OAuth gap, succession, witness, moderation, anti-cheat, CSAM | +15 | |
| **09_ECONOMY** *(new)* | Bliss, Premium, Marketplace, IAP, Stripe Connect, payouts, tax, refund | +12 | |
| **10_TELEMETRY** *(new)* | Event topics, history tee, Workshop/file/sim producers, Sentry, Prometheus | +12 | Inherits old "Streaming" content |
| **11_SIMULATION_DEBUGGER** *(new)* | SimulationClock, watchpoints, breakpoints, replay, SITL/HIL, debugger UI | +12 | |
| **12_INFRASTRUCTURE** *(new)* | Nomad/Consul/Terraform/Vault, R2, CI/CD, code signing, multi-region | +14 | |

---

## Subsystem index

| # | System | File | P2 highlights |
| ---: | --- | --- | --- |
| 01 | Client Player | [01_CLIENT_PLAYER.md](01_CLIENT_PLAYER.md) | Client is thin QUIC renderer — server downloads the `.eustress` world container; web `/play/{sim_id}` flow |
| 02 | Studio Engine | [02_STUDIO_ENGINE.md](02_STUDIO_ENGINE.md) | **+ Multiplayer Studio:** Loro CRDT scaffold, collaboration panel, presence WS, 0% wired |
| 03 | Multiplayer (replication, scripts, Forge) | [03_MULTIPLAYER.md](03_MULTIPLAYER.md) | Server-auth + script distribution still core gaps |
| 04 | Asset Pipeline (`.eustress` world container, R2, materials) | [04_ASSET_PIPELINE.md](04_ASSET_PIPELINE.md) | Published payload = `.eustress` (Fjall database + baked `.echk` chunks); full publish + server-download flow documented *(was `.pak` zstd-tar pre-P5)* |
| 05 | **Space Streaming** *(rewritten)* | [05_SPACE_STREAMING.md](05_SPACE_STREAMING.md) | `.echk` 56-byte packed instances; 2.10M-entity `active_cap` **CONFIG DEFAULT**, not a measurement; 0% wired |
| 06 | Website | [06_WEBSITE.md](06_WEBSITE.md) | KYC-first sign-up, Play button + checkout still missing |
| **07** | **AI Platform** *(new)* | [07_AI_PLATFORM.md](07_AI_PLATFORM.md) | Workshop (52 tools), embedvec (no ML embedder), Korah (0%) |
| **08** | **Identity & Trust** *(new)* | [08_IDENTITY_TRUST.md](08_IDENTITY_TRUST.md) | Ed25519 production; recovery / OAuth / CSAM / MFA missing |
| **09** | **Economy & Monetization** *(new)* | [09_ECONOMY.md](09_ECONOMY.md) | UI 90%, backend 5–10%; Bliss ledger + Stripe Connect missing |
| **10** | **Telemetry & Observability** *(new)* | [10_TELEMETRY.md](10_TELEMETRY.md) | EustressStream library production; producers stubbed; no Sentry |
| **11** | **Simulation & Debugger** *(new)* | [11_SIMULATION_DEBUGGER.md](11_SIMULATION_DEBUGGER.md) | Clock/watchpoints/V-Cell production; debugger UI + replay absent |
| **12** | **Infrastructure & DevOps** *(new)* | [12_INFRASTRUCTURE.md](12_INFRASTRUCTURE.md) | Release CI production; Vault + signing + multi-region missing |
| **13** | **Terrain & Voxel** *(P3 new)* | [13_TERRAIN_VOXEL.md](13_TERRAIN_VOXEL.md) | Brush, chunk LOD, navmesh, biomes, GeoTIFF |
| **14** | **Geo & Coordinates** *(P3 new)* | [14_GEO_COORDINATES.md](14_GEO_COORDINATES.md) | WGS84 ↔ ECEF, hybrid coords, orbital grid |
| **15** | **Mobile Platform** *(P3 new)* | [15_MOBILE_PLATFORM.md](15_MOBILE_PLATFORM.md) | iOS + Android shells unlinked from Rust core (biggest gap) |
| **16** | **Persistence & DataStore** *(P3 new)* | [16_PERSISTENCE_DATASTORE.md](16_PERSISTENCE_DATASTORE.md) | Backend SQLite, OrderedDataStore, SaveData |
| **17** | **Plugin & Extensibility** *(P3 new)* | [17_PLUGIN_EXTENSIBILITY.md](17_PLUGIN_EXTENSIBILITY.md) | MCP protocol, tool registry, LSP, plugin lifecycle |
| **18** | **CAD & Mesh Geometry** *(P3 new)* | [18_CAD_MESHGEOMETRY.md](18_CAD_MESHGEOMETRY.md) | truck BRep, half-edge, feature tree, STEP/IGES |
| **19** | **Realism & Physics Laws** *(P3 new)* | [19_REALISM_PHYSICS.md](19_REALISM_PHYSICS.md) | Symbolica, GPU SPH, fluid + thermal, materials science |
| **20** | **Search & Discovery** *(P3 new)* | [20_SEARCH_DISCOVERY.md](20_SEARCH_DISCOVERY.md) | embedvec HNSW, Insert menu, gallery semantic |

---

## Format note (P2 onward)

P1 used a single dense feature-matrix table per doc. The user flagged these as unreadable. **P2 format for new docs (07–12) is per-feature cards:**

```
### Feature N — Name

**State:** 🟢/🟡/🔴  · **Effort:** S/M/L/XL  · **Risk:** Low/Med/High/Critical
**Sub-features:** a · b · c

**Concept.** 2–4 sentences.

**Forecasted feedback (R)**
- R bullets — what reviewers / users will hit

**Implications (I)** — deeper than P1; multiple categories
- *Architectural:* …
- *Cross-system:* …
- *Migration:* …
- *Operational cost:* …
- *Support burden:* …
- *Strategic / competitive:* …

**Risks (X)** — what breaks if we ship or skip this
- X bullets

**Mitigations (M)**
- M bullets

**Touches:** [01_CLIENT], [02_STUDIO], …
```

Top-of-doc tables stay short (3 columns: # · name · state) — details live in cards.

P3 will retrofit this format to 01–06.

---

## Cross-cutting concerns (C1–C17)

| ID | Concern | Affected systems |
| --- | --- | --- |
| C1 | **Units boundary** — `authored ↔ meters` at every load/save (see [UNITS.md](../UNITS.md)) | Studio, Client, Multiplayer scripts, Asset Pipeline (mesh AABBs), Website (project list metadata) |
| C2 | **Canonical instance create** — every spawn surface routes through `eustress_common::instance_create::create_instance` | Studio (Insert + drag-drop + tools), MCP, Multiplayer (replicated spawns), Client (initial load), AI Platform tools |
| C3 | **Single-author storage** — file-system-first was the prior architecture; **superseded 2026-05-15 by the Fjall WorldDb store** (see C17); TOML retained as legacy/seed + human-editable schema. **Cloud-first** still applies for Multiplayer Studio sessions — toggle, not replace | Studio, Asset Pipeline, Multiplayer Studio (cloud mode) |
| C4 | **Stream-tee** for History / Workshop / MCP / Timeline / Telemetry | Studio, Telemetry, Multiplayer, Website (telemetry feed) |
| C5 | **Duplicate-type hazard** — single SoT for shared types | All Rust crates |
| C6 | **Slint is Rust** — `.slint` compiles to Rust | Studio UI, Mobile shells |
| C7 | **Physics is Avian, not Rapier** | Studio, Client, Multiplayer (server-auth), Simulation |
| C8 | **AI consent (`ai = true`)** — only consenting entities flow into training | Asset Pipeline, AI Platform, Telemetry, Multiplayer |
| C9 | **DisplayUnit vs. authored vs. engine-native** — Properties panel Stage 6.5 still open | Studio, Client (HUDs), Website |
| C10 | **Trash-rename undo invariant** — `fs::rename` to Trash; undo restores + `SpaceRescanNeeded` | Studio, Client (recorded session replay?) |
| **C11** *(P2; rewritten P5 2026-05-16)* | **`.eustress` is the world-container directory, not a dead alias of `.pak`** — `.eustress` holds `world.fjalldb/` + `header.bin` + baked `chunks/*.echk` + `manifest.toml` + human-editable `schema/`/service folders. **`.pak` is no longer the published/storage format.** Publish still uploads to R2 and the server still downloads + loads; only the archived payload changed (Fjall database + baked chunks, not per-instance TOML). Client never downloads, server does. | Studio (publish), Asset Pipeline, Multiplayer (server load), Website (Play button), Client (QUIC join) |
| **C12** *(P2; rewritten P5 2026-05-16)* | **Two storage modes**: single-author **Fjall WorldDb** (was file-system-first pre-P5) ↔ cloud-CRDT (Multiplayer Studio) — Studio detects and routes | Studio, Multiplayer Studio, Telemetry (history per-mode) |
| **C13** *(new P4)* | **`eustress://` deep-link protocol** — installer registers per-OS handler (Win MSI, macOS LSApplicationURLTypes, Linux .desktop MimeType); Website "Play" emits `eustress://play/{sim_id}?token={join_token}`; Client + server claim it; mobile uses universal links / app links. | Website (Play button), Client (launch flow), Mobile (universal-link), Identity (token issuance), Infrastructure (installer reg), Multiplayer (join handshake) |
| **C14** *(new P4)* | **KYC-deferred OAuth** — Discord / Google / GitHub OAuth-first signup; Ed25519 keypair generated post-OAuth and bound to the account; KYC required ONLY at first monetisation event (creator publish, marketplace sale, Bliss withdrawal). Lost-key recovery via OAuth-bound email + identity rotation. | Website (signup funnel), Identity (auth model), Economy (KYC gate moves), AI Platform (BYOK still works), Telemetry (consent surface) |
| **C15** *(new P4)* | **Plugin sandbox = WASM-first** — third-party plugins ship as `wasmtime`/`wasmer` modules by default; permission scoping via capability handshake (ECS-read / ECS-write / file-IO / network); native `.so`/`.dll` plugins are opt-in for first-party + signed authors only. Same model for IDE extensions (LSP) and Slint custom components. | Plugin (default runtime), Studio (loads plugins), AI Platform (MCP tools also adopt), Identity (plugin signing), Infrastructure (CI cross-compiles WASM targets) |
| **C16** *(new P4)* | **Per-platform asset variants** — every texture / mesh has a canonical source + per-platform bake (BC7 desktop, ASTC mobile, Basis universal); manifest tags the variant; Client resolves by `(platform, gpu-feature)` tuple. Mobile rendering tier (15 F10) cooperates with the variant chooser. | Asset Pipeline (bake), Client (variant choice), Mobile (force-ASTC), Space Streaming (chunk mesh refs), Infrastructure (CI bake matrix) |
| **C17** *(new P5, 2026-05-16)* | **WorldDb is the storage standard** — live entity-component-system state lives in a **Fjall log-structured-merge-tree key-value database** behind the Rust `WorldDb` trait (`eustress-worlddb` crate), wrapping stock **Fjall 2.11.2** (no fork yet — forking deferred until a measured bottleneck). The store is **hybrid**: an `entities` partition holds per-component zero-copy **rkyv**-serialized records for the typed core (Transform, BasePart, Tags, Attributes, instance metadata, asset reference, measure-unit) keyed by a **Morton Z-order** spatial curve; a `tree` partition holds **raw TOML bytes** for the extensible/text tail (scripts, graphical-user-interface TOML, `_service.toml`, plugin/extra sections, raw assets). TOML on disk is the **legacy/seed** form: a faithful importer mirrors a Space's whole tree into the `tree` partition on first open, after which **the database is authoritative**. `DataStoreService`/`OrderedDataStore` are backed (or being backed) by the same database via a pluggable backend (`ds_get/ds_set/ds_remove/ds_update/ds_range/ds_set_sorted`, plus Roblox-parity `DataStore`/`OrderedDataStore`/`DataStorePages`). **Known gap:** the Properties panel does **not** persist edits in the default build (legacy TOML write-back is gated behind an opt-in `toml` cargo feature; the Fjall mirror only writes Transform so far). **Open convergence decision (needs human):** worlddb's `.echk` version-0 container vs. the audit's 56-byte packed-instance wire format are two unreconciled chunk formats. | Studio (editing persistence), Client (load path), Multiplayer (server load), Asset Pipeline (publish payload), Space Streaming (`.echk` bake), Persistence/DataStore (backend), Plugin (extensible `tree` sections) |

---

## How to read this

- **For the project director:** start with the [Iteration tracker](#iteration-tracker) and the per-doc deltas above. The metric dashboard shows totals across passes.
- **For Claude (next pass):** every system file has a changelog at top; pick a system that hasn't been deepened recently or a cross-cutting concern bottlenecking multiple systems. P6 candidates: trace C17 (WorldDb) end-to-end through 01 / 02 / 04 / 05 / 16; resolve the `.echk` vs. 56-byte-packed-instance format convergence (flagged in C17, [04], [05]); audit the Properties-panel-no-persist gap (C17, [02], [16]); retrofit per-feature-card format to 01–06; and re-derive every state percentage in 01–20 against the two facts in *Ground truth* that most of them silently assume — that the code builds, and that its tests run.
- **For external readers:** the [Cross-cutting concerns](#cross-cutting-concerns-c1c17) table is the fastest summary of platform invariants. **C17 (WorldDb storage standard) is the P5 headline** — it supersedes the old "file-system-first" model and reframes C11 (`.eustress` is now the world container, not `.pak`) and C12 (single-author storage is now Fjall WorldDb).

---

## What this audit explicitly does NOT do

- **No code edits.** Edits land in this folder's `.md` files (and targeted source-of-truth docs they reference, e.g. UNITS.md or FEATURE_PARITY.md).
- **No prioritization.** Items are listed exhaustively; user decides what to ship.
- **No date estimates.** Effort tags (S/M/L/XL) are engineering-week shapes, not deadlines.
- **No duplication of [FEATURE_PARITY.md](../FEATURE_PARITY.md)** (Roblox-parity audit). This audit covers the *Eustress platform*, which extends beyond Roblox parity.
