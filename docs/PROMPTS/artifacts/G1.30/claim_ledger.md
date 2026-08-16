# Eustress Claim Ledger

Generated from `claim_ledger.json`. The JSON is authoritative; this file is generated and must not be hand-edited.

**Generated:** 2026-08-08T01:46:56Z · **Commit:** `71ccf6fe` · **Item:** G1.30

**Corpus:** 93 files · **Records:** 104 · MEASURED 12 · TARGET 19 · CONFIG_DEFAULT 9 · UNSUPPORTED 64

**Hardware:** Windows 11 Pro 10.0.26200, x64, rustc 1.95.0 (59807616e 2026-04-14), Python 3.13.14. The EustressStream benchmark artifact records its own platform: Windows 11 Pro, loopback (127.0.0.1).

## Class definitions

| Class | Definition |
|---|---|
| `MEASURED` | A number produced by running something, with a named exact command, named hardware, and an artifact path holding the result. |
| `TARGET` | A number the project intends to hit, including anything in a document headed with a design / pre-release / proposal status. |
| `CONFIG_DEFAULT` | The default value of a configuration field, cited as though it were an outcome. Carries the path:line of the field definition. |
| `UNSUPPORTED` | Everything else, including capability claims contradicted by docs/AUDIT/, and any number whose origin could not be established. |

## MEASURED — 12

Each row names the exact command that reproduces the number, the hardware it ran on, and the artifact holding the result.

| ID | Claim | Source | Number | Reproduction command | Artifact |
|---|---|---|---|---|---|
| C001 | &lt;div class="stat"&gt;622k msg/s · 411 µs · 20.4M shared-memory&lt;/div&gt; | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:517` | 622000 msg/s | `cd eustress && cargo bench -p eustress-stream-node --features quic --profile release` | `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md` |
| C002 | &lt;div class="stat"&gt;622k msg/s · 411 µs · 20.4M shared-memory&lt;/div&gt; | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:517` | 411.6 us | `cd eustress && cargo bench -p eustress-stream-node --features quic --profile release` | `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md` |
| C003 | &lt;div class="stat"&gt;622k msg/s · 411 µs · 20.4M shared-memory&lt;/div&gt; | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:517` | 20380000 msg/s | `cd eustress && cargo bench -p eustress-stream-node --features quic --profile release` | `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md` |
| C004 | \| TCP batch-256 \| 256 msgs \| **411.6 µs** \| **622,000 msg/s** \| **64×** \| | `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md:16` | 622000 msg/s | `cd eustress && cargo bench -p eustress-stream-node --features quic --profile release` | `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md` |
| C005 | \| **SHM publish** \| **1 msg** \| **49 ns** \| **20,380,000 msg/s** \| **2,112×** \| | `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md:24` | 20380000 msg/s | `cd eustress && cargo bench -p eustress-stream-node --features quic --profile release` | `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md` |
| C006 | \| TCP sequential \| 1 msg \| **103.6 µs** \| **9,651 msg/s** \| 1× \| | `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md:14` | 9651 msg/s | `cd eustress && cargo bench -p eustress-stream-node --features quic --profile release` | `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md` |
| C007 | 7. **Telemetry library + TCP/SHM brokers are production** (~85M msg/s in-process); ~80% of producers (Workshop tool, file-watcher, simulation, play-mode) unwired; no Sentry / Prometheus / Grafana. | `docs/AUDIT/MASTER.md:74` | 85000000 msg/s | `cd eustress && cargo bench -p eustress-stream-node --features quic --profile release` | `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md` |
| C008 | proposal. What exists: the `government` mode with twelve disciplines and 562 declared tool ids | `docs/architecture/GOVERNMENT_MODE.md:4` | 562 tool ids | `python -c "import re; t=open('eustress/crates/engine/modes/government.toml',encoding='utf-8').read(); ids=set(); [ids.update(re.findall(r'\"([a-z0-9]+:[a-z0-9_]+)\"', m)) for m in re.findall(r'tools\s*=\s*\[([^\]]*)\]', t, re.S)]; print(len([i for i in ids if not i.startswith('data:')]))"` | `eustress/crates/engine/modes/government.toml` |
| C009 | \| `government` mode, 12 disciplines, 562 tool ids \| [`modes/government.toml`](../../eustress/crates/engine/modes/government.toml) \| `studio_modes::government_mode_shape`; the existing all-manifests... | `docs/architecture/GOVERNMENT_MODE.md:701` | 562 tool ids | `python -c "import re; t=open('eustress/crates/engine/modes/government.toml',encoding='utf-8').read(); ids=set(); [ids.update(re.findall(r'\"([a-z0-9]+:[a-z0-9_]+)\"', m)) for m in re.findall(r'tools\s*=\s*\[([^\]]*)\]', t, re.S)]; print(len([i for i in ids if not i.startswith('data:')]))"` | `eustress/crates/engine/modes/government.toml` |
| C010 | **08_IDENTITY_TRUST** — The Cloudflare Worker's `JURISDICTIONS` ontology is **populated**: **46 country entries** at `infrastructure/cloudflare/api/src/index.js:39-98`, each carrying `name`, `natur... | `docs/AUDIT/MASTER.md:27` | 46 countries | `python -c "import re; ls=open('infrastructure/cloudflare/api/src/index.js',encoding='utf-8').read().splitlines(); print(sum(1 for l in ls[38:97] if re.match(r'^\s{4}[A-Z]{2}: \{', l)))"` | `infrastructure/cloudflare/api/src/index.js` |
| C011 | \| Render core \| Bevy 0.19 \| | `README.md:53` | 0.19 Bevy version | `grep -n '^bevy = ' eustress/Cargo.toml` | `eustress/Cargo.toml` |
| C012 | cargo build --workspace --release      # binaries → eustress/target/release/ | `README.md:98` | 0 build exit code | `cd eustress && cargo build --workspace &gt; build.log 2&gt;&1 ; echo BUILD_EXIT=$?` | `docs/PROMPTS/artifacts/G1.01/workspace_build.json` |

## TARGET — 19

Numbers the project intends to hit. None of them describes present behaviour.

| ID | Claim | Source | Number | Audit reference | Licence conflict |
|---|---|---|---|---|---|
| C022 | - **Simulation-first.** Built to drive millions of entities under real kernel-level laws; the design target is order-of-_a-year-of-simulation-per-second_ throughput, not just frames on screen. | `README.md:34` | 1 year of simulation per second |  |  |
| C023 | **Target:** A `.eustress` world holding **10,000,000 persisted entities** that renders at **≥60 FPS (16.6 ms)** with a **photorealistic** look on a single high-end GPU. | `docs/architecture/SCALING_ARCHITECTURE.md:5` | 10000000 persisted entities |  |  |
| C024 | **Target:** A `.eustress` world holding **10,000,000 persisted entities** that renders at **≥60 FPS (16.6 ms)** with a **photorealistic** look on a single high-end GPU. | `docs/architecture/SCALING_ARCHITECTURE.md:5` | 60 FPS |  |  |
| C025 | \| **Live ECS (active)** \| **≤ ~100K** \| Bevy World (Transform + render/physics) \| Yes \| | `docs/architecture/SCALING_ARCHITECTURE.md:25` | 100000 live ECS entities |  |  |
| C026 | 16.6 ms total. A defensible allocation at 60 FPS with ~100K live entities: | `docs/architecture/SCALING_ARCHITECTURE.md:71` | 16.6 ms frame budget |  |  |
| C027 | 2. GPU-driven cull + indirect draw shipped; CPU draw-submit &lt; 1.5 ms. | `docs/architecture/SCALING_ARCHITECTURE.md:213` | 1.5 ms CPU draw-submit |  |  |
| C028 | 3. ≥ 60 FPS sustained with the Track-A photoreal stack on the reference GPU. | `docs/architecture/SCALING_ARCHITECTURE.md:214` | 60 FPS |  |  |
| C029 | 4. Streaming hitches &lt; 2 ms p99 under fast camera motion. | `docs/architecture/SCALING_ARCHITECTURE.md:215` | 2 ms p99 hitch |  |  |
| C030 | \| **Combined** \| **Target: 200+ FPS at 10K** \| | `docs/development/BENCHMARK_VS_ENGINE_AUDIT.md:60` | 200 FPS at 10K entities |  |  |
| C031 | \| Streaming as the primary load/render path (the real fix at 10M-instance scale) \| High \| `space/streaming`, `world_db_plugin`, `sys_radius_gate` (audit 05) \| 3wk+ \| Do non-streaming FPS wins first \| | `LAUNCH_PLAN.md:55` | 10000000 instances |  |  |
| C032 | \| **Player Plus** \| $4.99 \| $49.99 \| 17% \| | `docs/monetization/SUBSCRIPTIONS.md:41` | 4.99 USD/month |  |  |
| C033 | \| **Creator Pro** \| $9.99 \| $99.99 \| 17% \| | `docs/monetization/SUBSCRIPTIONS.md:42` | 9.99 USD/month |  |  |
| C034 | \| **Bundle** \| $12.99 \| $129.99 \| 17% \| | `docs/monetization/SUBSCRIPTIONS.md:43` | 12.99 USD/month |  |  |
| C035 | ### Revenue After Steam Cut (30%) | `docs/monetization/SUBSCRIPTIONS.md:45` | 30 percent |  |  |
| C036 | \| **Revenue Share** \| 25% \| 40% \| | `docs/monetization/SUBSCRIPTIONS.md:156` | 40 percent revenue share |  |  |
| C037 | - Need to earn 466 Bliss/month in additional revenue | `docs/monetization/SUBSCRIPTIONS.md:176` | 466 Bliss/month |  |  |
| C038 | - **Constitutional 50/50.** Half of every Ticket dollar structurally funds builders, half funds the engine, written into how the tokens work, not a cut the platform can quietly change later (the "R... | `README.md:148` | 50 percent |  |  |
| C039 | &lt;h4&gt;✓ Pilot Program &lt;span class="free-pill"&gt;100% Free&lt;/span&gt;&lt;/h4&gt; | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:625` | 100 percent free |  |  |
| C040 | - **Outputs:** STEP export (truck-stepio glue), BOM from the assembly graph, technical | `docs/architecture/CAD_PLATFORM_PLAN.md:166` |   | `docs/AUDIT/18_CAD_MESHGEOMETRY.md` |  |

## CONFIG_DEFAULT — 9

The default value of a configuration field, cited as though it were an outcome. `config_field_path` is the definition site.

| ID | Claim | Source | Number | Config field |
|---|---|---|---|---|
| C013 | - **Active tier** — Bevy ECS entities with full `Transform`, `Mesh3d`, materials, physics. Counted against an `active_cap` whose **CONFIG DEFAULT** is 2.10M. That figure is a ceiling the configurat... | `docs/AUDIT/05_SPACE_STREAMING.md:23` | 2100000 entities | `eustress/crates/common/src/streaming/types.rs:290` |
| C014 | 3. **Space Streaming is fully designed and zero-wired** — `.echk` 56-byte packed-instance format, manifest layout, hysteresis radii (500/600/2000 m), and an `active_cap` **CONFIG DEFAULT** of 2.10M... | `docs/AUDIT/MASTER.md:70` | 2100000 entities | `eustress/crates/common/src/streaming/types.rs:290` |
| C015 | \| 05 \| **Space Streaming** *(rewritten)* \| [05_SPACE_STREAMING.md](05_SPACE_STREAMING.md) \| `.echk` 56-byte packed instances; 2.10M-entity `active_cap` **CONFIG DEFAULT**, not a measurement; 0% wir... | `docs/AUDIT/MASTER.md:127` | 2100000 entities | `eustress/crates/common/src/streaming/types.rs:290` |
| C016 | Promotion (`Cold → Hot → Active`) and demotion (`Active → Hot → Cold`) is **hysteresis-driven**: an entity activates when within `active_radius` (default 500 m) and only demotes once outside `evict... | `docs/AUDIT/05_SPACE_STREAMING.md:25` | 500 m | `eustress/crates/common/src/streaming/types.rs:287` |
| C017 | Promotion (`Cold → Hot → Active`) and demotion (`Active → Hot → Cold`) is **hysteresis-driven**: an entity activates when within `active_radius` (default 500 m) and only demotes once outside `evict... | `docs/AUDIT/05_SPACE_STREAMING.md:25` | 600 m | `eustress/crates/common/src/streaming/types.rs:288` |
| C018 | **20_SEARCH_DISCOVERY** — Hash embeddings "useless" overstated — work for *some* applications (deterministic, fast); they're **suboptimal**, not useless. HNSW `M=16 / efConstruction=200 / efSearch=... | `docs/AUDIT/MASTER.md:53` | 16 HNSW M | `eustress/crates/embedvec/src/resource.rs:57` |
| C019 | **20_SEARCH_DISCOVERY** — Hash embeddings "useless" overstated — work for *some* applications (deterministic, fast); they're **suboptimal**, not useless. HNSW `M=16 / efConstruction=200 / efSearch=... | `docs/AUDIT/MASTER.md:53` | 200 HNSW efConstruction | `eustress/crates/embedvec/src/resource.rs:58` |
| C020 | **20_SEARCH_DISCOVERY** — Hash embeddings "useless" overstated — work for *some* applications (deterministic, fast); they're **suboptimal**, not useless. HNSW `M=16 / efConstruction=200 / efSearch=... | `docs/AUDIT/MASTER.md:53` | 50 HNSW efSearch | `eustress/crates/embedvec/src/resource.rs:59` |
| C021 | \| Backend API    \| 7000  \| REST API                 \| | `START.md:80` | 7000 TCP port | `eustress/crates/backend/src/config.rs:30` |

## UNSUPPORTED — 64

No command, no artifact, or a capability contradicted by the audit ledger. `licence_conflict` marks a claim the LICENSE contradicts.

| ID | Claim | Source | Number | Audit reference | Licence conflict |
|---|---|---|---|---|---|
| C041 | &lt;div class="tag"&gt;Open-source · Physics-as-Code · Built for inventors&lt;/div&gt; | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:476` |   |  | YES |
| C042 | &lt;div class="desc"&gt;Free, open source. Windows, macOS, Linux. Vulkan / DX12 ready.&lt;/div&gt; | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:592` |   |  | YES |
| C043 | future for inventors. Built in the open, owned by the community.&lt;/p&gt; | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:642` |   |  | YES |
| C044 | &nbsp; Open source · MIT-friendly licensing · Git-diffable projects · No vendor lock-in | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:665` |   |  | YES |
| C045 | &lt;p&gt;Claude-powered agent with &lt;b&gt;full access to the running simulation&lt;/b&gt; via MCP tools. | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:501` |   | `docs/AUDIT/07_AI_PLATFORM.md` |  |
| C046 | &lt;p&gt;Shipped laws: &lt;b&gt;Nernst, Butler-Volmer, Tafel, ohmic IR drop, Monroe-Newman dendrite risk, | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:508` |   | `docs/AUDIT/19_REALISM_PHYSICS.md` |  |
| C047 | &lt;br&gt;Time compression up to &lt;b&gt;1 year per second&lt;/b&gt; for cycle-life and aging studies.&lt;/p&gt; | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:510` | 1 year per second |  |  |
| C048 | &lt;p&gt;Type-safe, sandboxed scripting with &lt;b&gt;direct ECS memory access&lt;/b&gt; — about 50× faster than | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:521` | 50 x faster than Lua FFI |  |  |
| C049 | &lt;span&gt;Real-time HUD · Workshop AI · 110 entities&lt;/span&gt; | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:535` | 110 entities |  |  |
| C050 | and edit every part of it. &lt;b&gt;Iterate in minutes, not months.&lt;/b&gt; Bring a thesis to a | `docs/marketing/UofA_Center_For_Innovation_Pilot.html:619` |   | `docs/AUDIT/MASTER.md` |  |
| C051 | It rests on a single load-bearing bet: **the engine simulates the world rather than rendering a scene.** Render-first engines draw what you tell them to draw; Eustress is built to _compute what wou... | `README.md:24` |   | `docs/AUDIT/05_SPACE_STREAMING.md` |  |
| C052 | - **AI-native.** A built-in **Workshop** AI assistant plus a **Model Context Protocol (MCP)** bridge let AI agents inspect, drive, and build inside a _live_ world, even from their own independent o... | `README.md:36` |   | `docs/AUDIT/07_AI_PLATFORM.md` |  |
| C053 | \| Language \| 100% Rust \| | `README.md:52` | 100 percent Rust |  |  |
| C054 | \| World store \| Binary, log-structured **WorldDb** on [Fjall](https://github.com/fjall-rs/fjall) (LSM-tree), holding live entity state as compact records so a world scales to millions of entities a... | `README.md:56` |   | `docs/AUDIT/05_SPACE_STREAMING.md` |  |
| C055 | \| Platforms \| Desktop (Windows, macOS, Linux); mobile player in progress \| | `README.md:57` |   | `docs/AUDIT/15_MOBILE_PLATFORM.md` |  |
| C056 | - **Live AI co-creation**: the Workshop assistant and MCP bridge let AI build with you, with its own independent camera to view its work | `README.md:106` |   | `docs/AUDIT/07_AI_PLATFORM.md` |  |
| C057 | - **Interactive charts & grids.** A Dataset opens as a chart tab: an auto-scaling plot with point hover, a least-squares fit and its equation, Chart / Grid / Split views, and adjustable axes, plus ... | `README.md:117` |   |  |  |
| C058 | - **Analysis built in.** The `eustress-data` crate (Polars / Arrow-backed) supplies the stats, curve fits, and clustering (k-means, kNN) that the **Data** ribbon runs on the selected Dataset. | `README.md:118` |   |  |  |
| C059 | - **Two-token wall.** **Tickets** (bought in USD, spent across the publication gallery) are buyer-money; **Bliss** (earned through contribution, cash-outable to USD) is builder-money. They never sh... | `README.md:147` |   | `docs/AUDIT/09_ECONOMY.md` |  |
| C060 | - **Merit-only ladder.** Install the engine, contribute (PRs, fixes), earn rank, become a Contributor, earn Bliss, publish, and cash out. Demand flows in, the constitution routes half to merit, and... | `README.md:149` |   | `docs/AUDIT/09_ECONOMY.md` |  |
| C061 | \| Kill the 4-FPS-at-scale regression (material + mesh-handle dedup, GPU instancing) \| High \| `space/streaming`, `instance_loader` VisibilityRange, `material_sync` \| 2-4d \| Green build first \| | `LAUNCH_PLAN.md:44` | 4 FPS |  |  |
| C062 | \| KYC jurisdictions dict populated (the "72 countries" is spec-only; dict is empty) \| High \| Cloudflare Worker JURISDICTIONS (audit 08) \| 2-4d \| Legal sign-off on the list \| | `LAUNCH_PLAN.md:59` | 72 countries | `docs/AUDIT/08_IDENTITY_TRUST.md` |  |
| C063 | \| Telemetry producers wired + a persistence plan (73 TB/week at claimed throughput) \| Med \| EustressStream producers (audit 10) \| 1-2wk \| Storage-budget decision \| | `LAUNCH_PLAN.md:60` | 73 TB/week | `docs/AUDIT/10_TELEMETRY.md` |  |
| C064 | \| Watchman cooldown to sim-time (it is wall-time, misses spikes at 10^6x) \| Low \| Watchman (audit 11) \| 0.5d \| None \| | `LAUNCH_PLAN.md:77` | 1000000 x sim-time scale | `docs/AUDIT/11_SIMULATION_DEBUGGER.md` |  |
| C065 | - macOS ARM64 (Apple Silicon) → `.dmg` with signed `.app` bundle | `RELEASE.md:110` |   | `docs/AUDIT/12_INFRASTRUCTURE.md` |  |
| C066 | - Linux x64 → `.tar.gz` with executable + `install.sh` + desktop file | `RELEASE.md:111` |   | `docs/AUDIT/12_INFRASTRUCTURE.md` |  |
| C067 | - `https://releases.eustress.dev/latest.json` (manifest, 5-minute cache) | `RELEASE.md:118` | 5 minute cache |  |  |
| C068 | - Windows build: ~20 min | `RELEASE.md:123` | 20 min |  |  |
| C069 | - macOS build: ~25 min | `RELEASE.md:124` | 25 min |  |  |
| C070 | - Linux build: ~15 min (fastest, mold linker) | `RELEASE.md:125` | 15 min |  |  |
| C071 | - Upload + publish: ~2 min | `RELEASE.md:126` | 2 min |  |  |
| C072 | Total: ~30 min from tag push to download link going live. | `RELEASE.md:128` | 30 min |  |  |
| C073 | First build takes ~5–10 minutes (Bevy 0.18 + Slint UI). Subsequent builds are incremental (~30s). | `START.md:15` | 10 min first build |  |  |
| C074 | First build takes ~5–10 minutes (Bevy 0.18 + Slint UI). Subsequent builds are incremental (~30s). | `START.md:15` | 30 s incremental build |  |  |
| C075 | First build takes ~5–10 minutes (Bevy 0.18 + Slint UI). Subsequent builds are incremental (~30s). | `START.md:15` | 0.18 Bevy version |  |  |
| C076 | │   ├── backend/         # API server | `START.md:67` |   |  |  |
| C077 | \| FPS \| 5,406 \| ~45 \| **120x slower** \| | `docs/development/BENCHMARK_VS_ENGINE_AUDIT.md:7` | 5406 FPS |  |  |
| C078 | \| FPS \| 5,406 \| ~45 \| **120x slower** \| | `docs/development/BENCHMARK_VS_ENGINE_AUDIT.md:7` | 45 FPS |  |  |
| C079 | \| Draw calls \| ~1 (instanced) \| ~10,000 \| **10,000x** \| | `docs/development/BENCHMARK_VS_ENGINE_AUDIT.md:9` | 10000 draw calls |  |  |
| C080 | The benchmark achieves 5,406 FPS because it's a minimal Bevy app with 1 material, 1 mesh, no physics, no UI, no file I/O, headless rendering. The engine will never match that number because it runs... | `docs/development/BENCHMARK_VS_ENGINE_AUDIT.md:62` | 5406 FPS |  |  |
| C081 | - **80-90% cost reduction** vs Kubernetes | `docs/architecture/EUSTRESS_FORGE.md:13` | 90 percent cost reduction |  |  |
| C082 | - **Millisecond-scale scaling** vs seconds-minutes | `docs/architecture/EUSTRESS_FORGE.md:14` |   |  |  |
| C083 | - **&lt;0.5% cluster overhead** vs 3-7% for K8s | `docs/architecture/EUSTRESS_FORGE.md:15` | 0.5 percent cluster overhead |  |  |
| C084 | \| Cluster Overhead \| 3-7% \| &lt;0.5% \| 85-95% \| | `docs/architecture/EUSTRESS_FORGE.md:248` | 95 percent overhead saving |  |  |
| C085 | \| Memory Per Pod \| 50-100MB \| 10-20MB \| 80% \| | `docs/architecture/EUSTRESS_FORGE.md:249` | 80 percent memory saving |  |  |
| C086 | \| 1K users \| $50 \| $19.50 \| 61% \| | `docs/architecture/EUSTRESS_FORGE.md:257` | 19.5 USD/month at 1K users |  |  |
| C087 | \| 1M users \| $5,000 \| $1,136 \| 77% \| | `docs/architecture/EUSTRESS_FORGE.md:260` | 1136 USD/month at 1M users |  |  |
| C088 | **09_ECONOMY** — **Marketplace state inflated**: P2/P3 said 75% but `purchase_item` handler calls `state.db.purchase_item()` with no Bliss-debit logic — true state is **🟡 40%**. Stripe Connect (Fea... | `docs/AUDIT/MASTER.md:29` | 40 percent complete | `docs/AUDIT/09_ECONOMY.md` |  |
| C089 | 4. **AI Platform has 52 tools across 9 Workshop modes** — embedvec HNSW DB ready but no ML embedder; spatial-llm modules drafted; Project Korah architecture only; FoundationModelDispatcher pseudocode. | `docs/AUDIT/MASTER.md:71` | 52 tools | `docs/AUDIT/07_AI_PLATFORM.md` |  |
| C090 | 5. **Identity core crypto is 95% done; recovery / MFA / OAuth / CSAM detection / age gating are 0–20%.** | `docs/AUDIT/MASTER.md:72` | 95 percent complete | `docs/AUDIT/08_IDENTITY_TRUST.md` |  |
| C091 | 6. **Economy frontend is 90% complete; backend is 5–10%** — Steam IAP, Stripe Connect, subscription lifecycle, refund handling all stubs. | `docs/AUDIT/MASTER.md:73` | 90 percent complete | `docs/AUDIT/09_ECONOMY.md` |  |
| C092 | 8. **Simulation is 70% mature** — SimulationClock, watchpoints, breakpoints, V-Cell physics, Watchman alerts all production; script debugger UI, replay/seek, cross-platform determinism absent. | `docs/AUDIT/MASTER.md:75` | 70 percent mature | `docs/AUDIT/11_SIMULATION_DEBUGGER.md` |  |
| C093 | 9. **Infrastructure 55%** — release pipeline production; macOS notarisation + Windows authenticode + Vault + Prometheus + multi-region all incomplete. | `docs/AUDIT/MASTER.md:76` | 55 percent complete | `docs/AUDIT/12_INFRASTRUCTURE.md` |  |
| C094 | Iterating 10M Bevy ECS entities once per frame, at an optimistic **5 ns each**, costs **50 ms** — that is **20 FPS before a single triangle is drawn or a single collider is stepped.** There is no r... | `docs/architecture/SCALING_ARCHITECTURE.md:17` | 5 ns per entity iteration |  |  |
| C095 | - **Benchmark generator**: [generate_benchmark_map.rs](eustress/crates/engine/src/bin/generate_benchmark_map.rs) `--binary-ecs N`. Ceiling tested ≈ **2.1M** entities. Measured: DashMap insert 37 ms... | `docs/architecture/SCALING_ARCHITECTURE.md:101` | 2100000 entities |  |  |
| C096 | - **Benchmark generator**: [generate_benchmark_map.rs](eustress/crates/engine/src/bin/generate_benchmark_map.rs) `--binary-ecs N`. Ceiling tested ≈ **2.1M** entities. Measured: DashMap insert 37 ms... | `docs/architecture/SCALING_ARCHITECTURE.md:101` | 37 ms DashMap insert |  |  |
| C097 | - **Benchmark generator**: [generate_benchmark_map.rs](eustress/crates/engine/src/bin/generate_benchmark_map.rs) `--binary-ecs N`. Ceiling tested ≈ **2.1M** entities. Measured: DashMap insert 37 ms... | `docs/architecture/SCALING_ARCHITECTURE.md:101` | 9.3 ms R-tree radius query |  |  |
| C098 | - **Benchmark generator**: [generate_benchmark_map.rs](eustress/crates/engine/src/bin/generate_benchmark_map.rs) `--binary-ecs N`. Ceiling tested ≈ **2.1M** entities. Measured: DashMap insert 37 ms... | `docs/architecture/SCALING_ARCHITECTURE.md:101` | 4.7 ms eviction |  |  |
| C099 | The Rust bet is real: **Bevy ECS with archetype storage achieves cache-coherent iteration over 10M+ entities at 2–4× the throughput of Unity DOTS** in published benchmarks (Bevy's own bench suite, ... | `docs/architecture/WORLD_CLASS_ENGINE.md:40` | 4 x Unity DOTS throughput |  |  |
| C100 | One honest limit: this was a sample, not an audit. A handful of the corpus's 1,512 files in one sub-folder, out of 118,974 objects in the whole Space. Enough to show the shape of the argument worki... | `docs/architecture/SPATIAL_INTELLIGENCE_ARCHITECTURE.md:182` | 118974 objects |  |  |
| C101 | **Status**: ✅ **Phases 1-4 COMPLETE** - Ready to paste and build | `docs/architecture/THE_LAST_GAME_ENGINE.md:19` |   |  |  |
| C102 | \| **Inspect** \| Read a space's state off disk; no sim \| ✅ **Works** \| [`eustress-space`](../../eustress/crates/eustress-space/src/main.rs) \| | `docs/architecture/HEADLESS_RUNTIME.md:27` |   |  |  |
| C103 | &gt; sketch-extrude-revolve workflow, mesh validation + repair, STEP / IGES import / export. | `docs/AUDIT/18_CAD_MESHGEOMETRY.md:4` |   | `docs/AUDIT/18_CAD_MESHGEOMETRY.md` |  |
| C104 | - **Live dispatch behind the buttons.** Of 562 declared government tool ids, exactly one — | `docs/architecture/GOVERNMENT_MODE.md:713` | 561 tool ids without a handler | `docs/AUDIT/07_AI_PLATFORM.md` |  |

## Unreadable sources

| Path | Bytes | NUL bytes | Lines | First bytes |
|---|---:|---:|---:|---|
| `docs/architecture/USD_NATIVE_FORMAT.md` | 54640 | 5674 | 1339 | `//! # Soul Script Editor` |

## Corpus

- `LAUNCH_PLAN.md`
- `README.md`
- `RELEASE.md`
- `START.md`
- `docs/AUDIT/05_SPACE_STREAMING.md`
- `docs/AUDIT/18_CAD_MESHGEOMETRY.md`
- `docs/AUDIT/MASTER.md`
- `docs/architecture/AI_GUARDIAN_POLICY_v1.0.md`
- `docs/architecture/APEX_ENGINE.md`
- `docs/architecture/AVATAR_AND_PARITY.md`
- `docs/architecture/BEVY_019_MIGRATION.md`
- `docs/architecture/CAD_PLATFORM_PLAN.md`
- `docs/architecture/CANONICAL_SNAPSHOT_SPEC.md`
- `docs/architecture/CAUSAL_OPLOG_WIRING.md`
- `docs/architecture/CLASS_REGISTRY.md`
- `docs/architecture/CLIENT_PUBLIC_LAUNCH.md`
- `docs/architecture/DATA_PLATFORM_PLAN.md`
- `docs/architecture/DECENTRALIZATION_PLAN.md`
- `docs/architecture/DECENTRALIZATION_QUESTIONNAIRE.md`
- `docs/architecture/ENHANCEMENT_PIPELINE.md`
- `docs/architecture/EUSTRESS_FORGE.md`
- `docs/architecture/EUSTRESS_FUNCTIONS.md`
- `docs/architecture/GAP_TAXONOMY.md`
- `docs/architecture/GAUSSIAN_SPLATTING.md`
- `docs/architecture/GAUSSIAN_SPLATTING_BATTLE_PLAN.md`
- `docs/architecture/GOVERNMENT_MODE.md`
- `docs/architecture/HEADLESS_RUNTIME.md`
- `docs/architecture/IDENTITY.md`
- `docs/architecture/IMPORT_STORAGE_AND_PORTABILITY.md`
- `docs/architecture/INFRASTRUCTURE.md`
- `docs/architecture/LIGHTING_AUDIT.md`
- `docs/architecture/MULTI_CLAUDE_ORCHESTRATION.md`
- `docs/architecture/ORBITAL_GRID.md`
- `docs/architecture/PPISP_RUST_PORT_PROPOSAL.md`
- `docs/architecture/PROJECT_KORAH.md`
- `docs/architecture/RENDER_CASCADE.md`
- `docs/architecture/ROBLOX_IMPORT_SPEC.md`
- `docs/architecture/SCALING_ARCHITECTURE.md`
- `docs/architecture/SITL_HIL_ARCHITECTURE.md`
- `docs/architecture/SPATIAL_INTELLIGENCE_ARCHITECTURE.md`
- `docs/architecture/STEM_STACK.md`
- `docs/architecture/TERRAIN_FJALL_MIGRATION.md`
- `docs/architecture/THE_LAST_GAME_ENGINE.md`
- `docs/architecture/USD_NATIVE_FORMAT.md`
- `docs/architecture/VCELL_CASE_STUDY.md`
- `docs/architecture/WORLD_CLASS_ENGINE.md`
- `docs/architecture/WORLD_ENGINE_PROGRAM.md`
- `docs/architecture/WORLD_MODEL_SIMULATOR_ROADMAP.md`
- `docs/benchmarks/BENCHMARK_EUSTRESSSTREAM.md`
- `docs/development/ADORNMENT_ARCHITECTURE.md`
- `docs/development/ASSET_INSTANCE_ARCHITECTURE.md`
- `docs/development/BASELINE_SIMULATION.md`
- `docs/development/BENCHMARK_VS_ENGINE_AUDIT.md`
- `docs/development/BLISS_MANUFACTURING_INTEGRATION.md`
- `docs/development/CHUNKED_STORAGE.md`
- `docs/development/CLASS_CONVERSION.md`
- `docs/development/DYNAMIC_GRAVITY_SYSTEM.md`
- `docs/development/FILE_SYSTEM_FIRST.md`
- `docs/development/FILE_WATCHER_HOT_RELOAD.md`
- `docs/development/KERNEL_LAW_SYSTEM.md`
- `docs/development/LIGHTING_SYSTEM.md`
- `docs/development/LOCAL_GEOSPATIAL.md`
- `docs/development/MACOS_ICON_BUNDLING.md`
- `docs/development/MANUFACTURING_DEAL_STRUCTURE.md`
- `docs/development/MANUFACTURING_PROGRAM.md`
- `docs/development/MATERIAL_SERVICE_ARCHITECTURE.md`
- `docs/development/MULTIPLAYER_SCRIPT_DISTRIBUTION.md`
- `docs/development/NLWEB_MANUFACTURING.md`
- `docs/development/PHASE1_PROGRESS.md`
- `docs/development/PROPERTIES_WRITEBACK_DESIGN.md`
- `docs/development/RECURSIVE_FEEDBACK_LOOP.md`
- `docs/development/RUNE_VM_INTEGRATION.md`
- `docs/development/SCRIPTING_API_CHECKLIST.md`
- `docs/development/SELECTION_SYSTEM.md`
- `docs/development/SERIALIZATION_AUDIT.md`
- `docs/development/SIMULATION_INTEGRATION_AUDIT.md`
- `docs/development/SIMULATION_SYSTEM.md`
- `docs/development/SLINT_GPU_ARCHITECTURE.md`
- `docs/development/SLINT_UI_SYSTEM.md`
- `docs/development/SOUL_SCRIPT_LOADING.md`
- `docs/development/SPACE_ARCHITECTURE.md`
- `docs/development/STUDIO_AUDIT_EGUI_TO_SLINT.md`
- `docs/development/TERRAIN_ARCHITECTURE.md`
- `docs/development/TOOLBOX_SYSTEM.md`
- `docs/development/TOOLS.md`
- `docs/development/TOOLSET.md`
- `docs/development/TOOLSET_CAD.md`
- `docs/development/TOOLSET_UX.md`
- `docs/development/UNIFIED_EXPLORER_PLAN.md`
- `docs/development/UNIFIED_TREE_DESIGN.md`
- `docs/development/WORKSHOP_TOOLS.md`
- `docs/marketing/UofA_Center_For_Innovation_Pilot.html`
- `docs/monetization/SUBSCRIPTIONS.md`
