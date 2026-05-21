# 10 — Telemetry & Observability

> Event-topic backbone (`EustressStream`), history tee, Workshop / file / simulation
> producers, cross-process bridging (TCP / SHM / QUIC), error reporting, metrics,
> dashboards, alerting. **This was misnamed "Streaming" in P1.** Real Space
> Streaming lives in [05_SPACE_STREAMING.md](05_SPACE_STREAMING.md).

## Pass changelog

- **P2 (2026-05-14):** New system doc (inherits + deepens P1 "Streaming" content); 12 features, 8 cards expanded, 12 wiring gaps. Renamed from event-streaming to Telemetry & Observability to disambiguate from Space Streaming.
- **P4 (2026-05-14):** Internal contradiction resolved: persistence is **per-topic opt-in; `history.*` + `workshop.tool.*` default ON; everything else opt-in**. Throughput claim "85M msg/sec" is in-process single-core (not aggregate, not over network); document. mmap retention at 1 KB avg × full throughput = 73 TB/week → persistence plan needs explicit sampling-per-topic policy.

---

## Concept summary

Eustress Telemetry is a **unified event-streaming platform** that captures "what just happened" across Studio, Client, Server, Forge, and Backend — enabling real-time collaboration (Workshop context), debugging (History panel, Timeline), analytics (leaderboards, churn), and automated crash reporting (Sentry).

The architecture is **three-tier**:
1. **Core (complete)**: `EustressStream` library — zero-copy in-process pub/sub, mmap / io_uring persistence, ~85M msg/sec in-process throughput.
2. **Bridging (~70%)**: `StreamNode` TCP broker + REST/SSE API; engine_bridge protocol; MCP tools (`stream_publish`, `stream_subscribe`); SHM bridge (SPSC ring for same-host); QUIC transport optional.
3. **Producers (~20%)**: History tee wired ✅; Workshop / file-watcher / simulation / play-mode producers stubbed; crash reporter (Sentry), metrics scraper (Prometheus), dashboards (Grafana) all **absent**.

The stream is the platform's **central nervous system**: every system is either a producer or a consumer. History panel, Timeline, Workshop AI context, MCP `query_stream_events`, future backend telemetry ingest all consume from it.

---

## Implementation snapshot

**Crates:**
- [eustress-stream](../../eustress/crates/stream/) — `EustressStream`, `Producer`, `Consumer`, `Topic`; mmap + io_uring backends; zero-copy `MessageView`; 26 files; benches show ~85M msg/sec.
- [eustress-stream-node](../../eustress/crates/stream-node/) — TCP server, REST+SSE API, MCP tool wrappers, SHM SPSC bridge, QUIC transport (feature-gated). 14 files.
- [engine/src/undo.rs](../../eustress/crates/engine/src/) + `HistoryStreamPlugin` — drains `UndoStack` each frame → `history.<kind>` topics ✅
- [engine/src/engine_bridge/](../../eustress/crates/engine/src/engine_bridge/) — JSON-RPC; no `SubscribeTopic` yet.

**Working:**
- ✅ Stream library (in-process, persistent)
- ✅ HistoryStreamPlugin (the only fully-wired producer)
- ✅ ChangeQueue resource → scene_deltas + agent_commands + agent_observations
- ✅ StreamNode TCP node on port 33000+ (8-partition fan-out for scene deltas)
- ✅ MCP tools: `stream_topics`, `stream_subscribe`, `stream_publish`

**Stubbed (fields exist, producers absent):**
- `ToolResult.stream_topic` — Workshop tool dispatch doesn't publish
- File-watcher events → `workspace.file.*` (not published)
- Simulation clock + watchpoints → `workshop.simulation.*` (not published)
- Play-mode transitions → `workshop.play.*` (not published)

**Absent:**
- Sentry / GlitchTip / DataDog error reporting
- Prometheus metrics exporter (`/metrics` endpoint)
- Grafana dashboards
- AlertManager rules
- Log shipping (fluent-bit / CloudWatch)
- PII scrubber
- Per-topic schema registry (today: freeform JSON)
- Sampling rate controls
- ChangeQueueConfig defaults to in-memory (persistence opt-in, default off)

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Event-topic backbone (ring buffers, named topics) | ✅ |
| 2 | Persistence (mmap / io_uring) | 🟡 opt-in default off |
| 3 | History tee (UndoStack → history.*) | ✅ |
| 4 | Workshop tool tee (ToolResult → workshop.tool.*) | 🔴 stubbed |
| 5 | Simulation / watchpoint tee | 🔴 stubbed |
| 6 | File-watcher tee | 🔴 stubbed |
| 7 | Play-mode tee | 🔴 stubbed |
| 8 | Cross-process bridging (TCP/QUIC/SHM/engine_bridge) | 🟡 mostly wired |
| 9 | Per-topic schema registry (protobuf + version) | 🔴 |
| 10 | Error reporting (Sentry + crash uploader) | 🔴 |
| 11 | Metrics + dashboards (Prometheus + Grafana) | 🔴 |
| 12 | Alerting + monitoring (AlertManager) | 🔴 |

---

## Detailed per-feature cards (top 8)

### Feature 1 — Event-topic backbone

**State:** ✅ complete · **Effort:** Done · **Risk:** Low · **Touches:** all
**Sub-features:** Named topics · ring-buffer eviction · async callback consumers · async durability · `<system>.<noun>.<verb>` naming convention

**Concept.** Producers publish to a named topic; consumers subscribe via callback; ring buffer prevents unbounded growth; durability is best-effort async write to storage backend.

**Forecasted feedback (R)**
- R1.1 Ring eviction is silent → slow consumers lose history without knowing.
- R1.2 Topic naming convention isn't enforced; drift inevitable.
- R1.3 Per-topic schema absent; payloads are JSON values; consumer parsing is brittle.
- R1.4 Backpressure: a slow consumer holds the ring at high watermark; need "drop oldest" policy.
- R1.5 Thread-safety story is unclear (single-threaded producer? multi-threaded safe?).

**Implications (I)**
- *Architectural:* the stream is the platform's observability backbone — silent loss = silent bugs.
- *Cross-system:* schema versioning is the *public API* question; bind early.
- *Operational cost:* in-memory ring is ~free; persistence ~5% throughput cost.
- *Strategic:* clean stream APIs are the gate to a real third-party plugin ecosystem.

**Risks (X)**
- X1.1 Schema drift between producer and consumer = silent payload mis-parse.
- X1.2 Without "gap markers", debugging dropped events is impossible.

**Mitigations (M)**
- M1.1 Emit `<topic>.gap` marker when ring evicts under a slow subscriber.
- M1.2 Protobuf + version baseline; document SLA per public topic.

---

### Feature 4 — Workshop tool tee

**State:** 🔴 stubbed · **Effort:** S (1–2 days) · **Risk:** Low · **Touches:** [02], [07], [10]
**Sub-features:** `ToolResult.stream_topic` payload · pre/post dispatch hooks · approval-state events · cost/token usage in payload · MCP-invoked + Studio-invoked unified

**Concept.** Every Workshop tool dispatch publishes the result to `workshop.tool.<name>` topic — args, success / failure, content, cost. Workshop re-reads its own topic for context injection on the next turn.

**Forecasted feedback (R)**
- R4.1 Field exists; no caller writes it.
- R4.2 MCP-invoked tools must publish the same topic as Studio-invoked ones, or observers see inconsistent state.
- R4.3 Approval-state (pending/approved/denied) is its own event class.
- R4.4 Cost in payload is the foundation for [09_ECONOMY] AI budget tracking.

**Implications (I)**
- *Cross-system:* enables Workshop self-recall, MCP observability, History panel completeness, Timeline cost overlay.
- *Architectural:* small change, large unlock — high ROI.

**Risks (X)** — X4.1 If only some tools publish, downstream consumers can't trust completeness.

**Mitigations (M)** — M4.1 Wrap dispatch in a single trace-emitting helper; all tools route through it.

---

### Feature 5 — Simulation / watchpoint tee

**State:** 🔴 stubbed · **Effort:** M (3–4 days) · **Risk:** Med (firehose) · **Touches:** [02], [10], [11]
**Sub-features:** Aggregated tick stream (120 Hz → 10 Hz) · watchpoint-change events · breakpoint hits · play-mode transitions · type-safe watchpoints

**Concept.** SimulationClock ticks at 120 Hz; publishing every tick at every entity is a firehose. Pre-aggregate to ~10 Hz (or event-driven on state change). Watchpoints + breakpoints emit on change. Play-mode transitions in their own topic.

**Forecasted feedback (R)**
- R5.1 Today none publish.
- R5.2 Watchpoints are string-keyed; type safety lost.
- R5.3 120 Hz firehose kills throughput if not aggregated.
- R5.4 Play-mode events overlap with multiplayer session events — separate topics or shared.

**Implications (I)**
- *Cross-system:* Timeline panel renders nothing without this; bug-report replay needs captured sim stream.
- *Architectural:* aggregation policy must be configurable.
- *Operational cost:* 10 Hz × N watchpoints × M entities = thousands msg/sec; tolerable.

**Risks (X)** — X5.1 Aggregation loses sub-aggregation-window spikes; configurable per watchpoint.

**Mitigations (M)** — M5.1 Per-watchpoint "raw vs. aggregated" flag.

---

### Feature 8 — Cross-process bridging

**State:** 🟡 70% · **Effort:** L (1–2 weeks) · **Risk:** Med (auth) · **Touches:** [02], [07], [10]
**Sub-features:** engine_bridge `SubscribeTopic` / `UnsubscribeTopic` · stream-node lifecycle (engine spawns? manual?) · ShmBridge SPSC ring · TCP token-gating · multi-engine federation

**Concept.** Stream is in-process by default. For LSP / MCP / plugins, engine_bridge exposes `SubscribeTopic` (route to in-process EustressStream); stream-node TCP broker handles cross-host; ShmBridge is fastest IPC for same-host single-publisher.

**Forecasted feedback (R)**
- R8.1 `engine_bridge.BridgeRequest` enum has no SubscribeTopic.
- R8.2 stream-node binary exists; engine doesn't launch it.
- R8.3 ShmBridge unused; TCP is the only conceived bridge.
- R8.4 Open localhost TCP = local-RCE risk; need token gate.
- R8.5 Multi-engine federation (Universe-wide stream across many Studio instances) not designed.

**Implications (I)**
- *Architectural:* engine_bridge is the seam between Studio internals and external ecosystem.
- *Cross-system:* unlocking subscribe → MCP and LSP both go reactive; major unblock.

**Risks (X)** — X8.1 Token-less TCP = local code execution by any process.

**Mitigations (M)** — M8.1 Random session token written to `.eustress/engine.port`; clients authenticate.

---

### Feature 10 — Error reporting

**State:** 🔴 · **Effort:** L (3–4 weeks) · **Risk:** High (PII) · **Touches:** [01], [02], [03], [06], [10]
**Sub-features:** Sentry SDK · panic hook · validation error capture · minidump uploader · PII scrubber · symbolication · opt-in flag

**Concept.** Engine panics + validation errors + crash minidumps → Sentry (or GlitchTip for self-host). Symbol files uploaded per release for source-line mapping. PII scrubbed before send.

**Forecasted feedback (R)**
- R10.1 telemetry.rs has TODO comments only.
- R10.2 PII scrubber config absent; needs allow/deny rules.
- R10.3 Symbolication for release-mode binaries needs sym-server upload on release.
- R10.4 Sentry's cheap tier ($29/mo) is plenty for MVP.

**Implications (I)**
- *Architectural:* opt-in but on by default; respect [08_IDENTITY] consent.
- *Operational cost:* small; high leverage.
- *Strategic:* Pareto: top 5 crashes = 80% impact; visibility is gold.

**Risks (X)**
- X10.1 PII leakage if scrubber misconfigured.
- X10.2 Sentry quota exceeded under burst (single bug = thousand reports).

**Mitigations (M)**
- M10.1 Rate-limit per-error-key (e.g. 100/min); group by stack signature.
- M10.2 Test PII scrubber on real crash dumps before public launch.

---

### Feature 11 — Metrics + dashboards

**State:** 🔴 · **Effort:** L (1–2 weeks) · **Risk:** Med · **Touches:** [10], [12]
**Sub-features:** Prometheus `/metrics` endpoint · scrape config · Grafana dashboards · cardinality budget

**Concept.** Engine/server expose `/metrics` (Prometheus exposition format). Prometheus scrapes; Grafana renders dashboards: stream throughput, ring eviction, consumer lag, crash rate, simulation tick rate, leaderboard CCU.

**Forecasted feedback (R)**
- R11.1 No `/metrics` endpoint today.
- R11.2 Cardinality budget: high-cardinality labels (user_id, entity_id) explode Prometheus storage.
- R11.3 Grafana templates need to ship in `infrastructure/forge/grafana/`.

**Implications (I)** — *Operational:* without this, ops decisions are unguided.

**Risks (X)** — X11.1 Label cardinality explodes → Prometheus OOM.

**Mitigations (M)** — M11.1 Histogram instead of per-user counter; bucket users by tier.

---

### Feature 9 — Schema registry

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [10]
**Sub-features:** protobuf schemas · schema versioning · per-topic spec doc · contract tests

**Concept.** Each public topic has a `.proto` (or JSONSchema) defining its payload. Versioned. Consumer SDK auto-generates types. Drift triggers a CI test failure.

**Forecasted feedback (R)** — R9.1 No registry today; payloads freeform JSON. R9.2 Protobuf adds build-step complexity but is the industry standard.

**Implications (I)** — *Strategic:* without versioned schemas, public stream API is a permanent breaking-change risk.

**Risks (X)** — X9.1 Schema break between producer and consumer is silent.

**Mitigations (M)** — M9.1 CI test publishes + consumes every schema; fails on drift.

---

### Feature 2 — Persistence (mmap / io_uring)

**State:** 🟡 opt-in default off · **Effort:** M (2 weeks) · **Risk:** Med · **Touches:** [10]
**Sub-features:** mmap backend (cross-platform) · io_uring backend (Linux) · per-topic opt-in · startup replay strategy · disk format versioning

**Concept.** Append events to an mmap'd file (or io_uring queue) per topic. Restart replays from the segment to restore consumer state. Disk format has a version header.

**Forecasted feedback (R)**
- R2.1 `ChangeQueueConfig` defaults in-memory; persistence per-topic opt-in.
- R2.2 Replay strategy: full history or skip-to-latest? Configurable.
- R2.3 Persistence cost ~5% throughput; warm-load progress UI needed.

**Implications (I)** — *Cross-system:* History panel restoration across Studio restarts; debugger replay.

**Risks (X)** — X2.1 Startup can hang on large rings; add timeout + skip-to-latest.

**Mitigations (M)** — M2.1 Per-topic policy; default `history.*` + `workshop.tool.*` persistent.

---

## Wiring / import gaps (top 12)

1. Workshop tool producer call site
2. File-watcher producer (after debouncer)
3. Simulation clock producer (aggregated)
4. Watchpoint stream producer
5. Play-mode producer
6. `engine_bridge.SubscribeTopic` / `UnsubscribeTopic`
7. stream-node spawn in engine startup
8. ShmBridge enable in StreamNodePlugin
9. Persistence default flip (mmap on for `history.*` + `workshop.tool.*`)
10. Replication.* events from [03_MULTIPLAYER]
11. Asset.* events from [04_ASSET_PIPELINE]
12. Sentry / crash uploader (Feature 10)

---

## Cross-system dependencies

All six P1 systems + AI Platform + Identity + Economy + Simulation + Infrastructure consume from or produce into Telemetry.

```
Client (play.*) ─────────────┐
Multiplayer (replication.*) ──┤
Asset Pipeline (asset.*) ─────┤  ┌─► Stream topics ─► StreamNode TCP ─► Backend ingest ─► Dashboards
Studio (history.*, tool.*) ───┤──┤                                    (Prometheus)        (Grafana)
Simulation (sim.*) ───────────┤  └─► (PII scrub)     ─► Sentry         (Alerts)
Identity (identity.*) ────────┘
```

---

## Open policy questions

- Q10.1 Per-topic schema registry: protobuf, JSONSchema, freeform?
- Q10.2 Default persistence per topic — opt-in or opt-out?
- Q10.3 Stream-node lifecycle — engine-spawns vs. separate-binary vs. both?
- Q10.4 Retention: time-, size-, or count-based?
- Q10.5 Public-API stability per topic (which are stable, which internal?)
- Q10.6 Sentry vs. self-hosted GlitchTip vs. DataDog vendor choice
- Q10.7 PII handling rules + audit-log policy
- Q10.8 Sampling: uniform, adaptive, per-user?
