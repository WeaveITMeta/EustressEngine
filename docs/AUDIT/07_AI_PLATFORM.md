# 07 — AI Platform

> The cross-cutting foundation-model brain. Workshop chat, Claude bridge,
> 52-tool registry across 9 modes, embedvec HNSW DB, spatial-llm,
> FLUX/TripoSR generation, Project Korah, MCP server, model dispatcher.

## Pass changelog

- **P2 (2026-05-14):** New system doc; 15 features, 8 cards expanded, 12 wiring gaps.
- **P4 (2026-05-14):** State corrections from secondary critique: MCP `SubscribeTopic` **absent** in `BridgeRequest::MethodName` enum. FoundationModelDispatcher (Feature 12) is **fully absent** (not "pseudocode") — `APEX_ENGINE.md` referenced but the file is missing. Spatial-LLM (Feature 6): modules exist but **none call Claude** — state 🟡 → **🔴** (no wiring). Image-to-Code two-step flow remains fragile until Vision call is embedded in tool handler.

---

## Concept summary

The **AI Platform** is the unified foundation-model infrastructure that any system can call into. It is *distinct from* the Studio's Workshop UI (one surface) and the Asset Pipeline (one consumer). It is the cross-cutting brain serving the Studio's in-engine Workshop agent, out-of-process MCP servers for IDEs (Claude Desktop, Cursor, Windsurf), the Asset Pipeline's enhancement loop, the Client's quest narrative, and future GameLLM integrations.

The architecture is **disaggregated**: every call is async request/response with no frame blocking. Workshop runs agentic tool-use loops (52 tools across 9 modes). `image_to_code` converts UI screenshots → Rune scripts. `document_to_code` turns specs → working code. `embedvec` is an HNSW vector DB for semantic entity search. `spatial-llm` handles 3D reasoning. `texture-gen` is procedural (Perlin FBM, 8 presets) — not AI. FLUX (image gen) and TripoSR (mesh gen) are Python-server stubs.

Unlike traditional engines (UE5, Unity) that call external APIs, Eustress makes **the LLM itself a first-class subsystem**: prompts can be grounded in Symbolica-derived physics laws (Kernel Law System), outputs validated against constraints, results fed back into optimisation loops without leaving the engine. This is `APEX_ENGINE.md` (FoundationModelDispatcher, ModelTier enum: Local <50 µs / Fast 7B / Deep 70B MoE / Frontier GPT-4) and Project Korah (4-phase generation Foundation → Structure → Objects → Detail).

---

## Implementation snapshot

**Crates:**
- [eustress-workshop](../../eustress/crates/workshop/) — physical-digital twin, registry, knowledge, procurement
- [eustress-tools](../../eustress/crates/tools/) — 52 tools across 15 modules
- [eustress-embedvec](../../eustress/crates/embedvec/) — HNSW vector DB, 10 modules; embedder is hash-based (no ML)
- [eustress-spatial-llm](../../eustress/crates/spatial-llm/) — client + context + generation + indexing + prompt + query
- [eustress-mcp-server](../../eustress/crates/mcp-server/) — stdio JSON-RPC, MCP protocol v2025-06-18
- [eustress-texture-gen](../../eustress/crates/texture-gen/) — procedural Perlin FBM (8 presets)
- [engine/src/workshop/claude_bridge.rs](../../eustress/crates/engine/src/workshop/) — async Claude polling

**Key docs:**
- [APEX_ENGINE.md](../architecture/APEX_ENGINE.md), [ENHANCEMENT_PIPELINE.md](../architecture/ENHANCEMENT_PIPELINE.md), [PROJECT_KORAH.md](../architecture/PROJECT_KORAH.md), [WORKSHOP_TOOLS.md](../development/WORKSHOP_TOOLS.md), [EUSTRESS_WORKSHOP.md](../innovation/EUSTRESS_WORKSHOP.md)

**Tool count:** 52 (36 General + 16 mode-specific). 4 entity, 2 file, 5 script, 5 memory, 1 diff, 4 git, 10 simulation, 2 physics, 1 shell, 2 spatial.

**Model dispatcher status:** BYOK Claude API (Sonnet) hot-pathed for Workshop. ModelTier + FoundationModelDispatcher are pseudocode only.

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Workshop conversational agent (tool-use loop) | ✅ |
| 2 | Image-to-Code (Vision API) | 🟡 |
| 3 | Document-to-Code | ✅ |
| 4 | Auto-doc generation | 🟡 |
| 5 | Embedvec semantic search (HNSW) | 🟡 framework |
| 6 | Spatial LLM (3D reasoning) | 🟡 modules drafted |
| 7 | MCP Server (stdio JSON-RPC) | 🟡 |
| 8 | Procedural texture generator (no AI) | ✅ |
| 9 | Enhancement pipeline (FLUX + TripoSR Python) | 🟡 stub mode |
| 10 | Project Korah (4-phase world-building) | 🔴 |
| 11 | Physics-grounded inference (Symbolica) | 🔴 |
| 12 | MoE dispatcher (vLLM tensor parallel) | 🔴 |
| 13 | ECEF planetary coords (APEX B) | 🔴 |
| 14 | FEM+SPH engineering sim (APEX C) | 🔴 |
| 15 | Tool-manufacturing pipeline (.tool.toml IoT) | 🟡 |

---

## Detailed per-feature cards (top 8)

### Feature 1 — Workshop conversational agent

**State:** ✅ wired · **Effort:** Done · **Risk:** Med (cost) · **Touches:** [02], [08], [09], [10]
**Sub-features:** chat UI · tool approval gate · agentic loop · mode-gated tools · BYOK API key · system-prompt assembly from active modes + memories + rules

**Concept.** A side-panel chat that sends conversation + tool defs to Claude, dispatches approved tools through `eustress-tools` registry, loops results back. Memories via `mcp__eustress__remember` / `recall`. Pinned-first behaviour in `SoulService`. Active modes (General + up to 8 specialist modes) gate the available tool surface.

**Forecasted feedback (R)**
- R1.1 No cost tracking — a user can burn $500+/session unnoticed.
- R1.2 Stacking modes (Manufacturing + Supply Chain + Finance) inflates to 45+ tools → Claude context bloat.
- R1.3 Tool approval is modal/blocking; parallel tool calls queue linearly.
- R1.4 System prompt assembly can overflow token budget (rules + memories ≈ 50 KB → 10K tokens before the convo).
- R1.5 No per-session token cap or per-day cap.
- R1.6 No streaming-event consumer ([10_TELEMETRY] Workshop tool tee) — agent can't see live ECS state.
- R1.7 BillboardGui / ScreenGui on_click → SoulScript callback is a known TODO.

**Implications (I)**
- *Architectural:* Workshop is the largest single dispatcher into `eustress-tools`; tool changes must respect mode filtering.
- *Cross-system:* Every new ECS / file action needs a Workshop tool wrapper.
- *Migration:* tool registry version must surface in Workshop so AI can self-check capability.
- *Operational cost:* Claude API is the largest variable cost line — needs a budget surface.
- *Support burden:* hallucinated tool calls (wrong args) need clear error surfacing.
- *Strategic / competitive:* Workshop is a top-3 differentiator; tooling depth matters more than chat polish.

**Risks (X)**
- X1.1 Token exhaustion silently truncates Claude responses.
- X1.2 Mode-leak (Manufacturing active → procurement tools usable → real $ spent without confirmation).
- X1.3 BYOK key in plain-text settings — accidental commit to repo.

**Mitigations (M)**
- M1.1 `WorkshopBudget` resource: warn at 70%, throttle at 95%.
- M1.2 Mode-filter tools by active set; soft cap at 12 tools per request.
- M1.3 Encrypt BYOK at rest; never log.

---

### Feature 2 — Image-to-Code (Vision API)

**State:** 🟡 tool defined; vision call upstream · **Effort:** M (~5 days) · **Risk:** Med · **Touches:** [02], [07], [10]
**Sub-features:** base64 + prompt to `structured_data` · target language (Rune / Luau / ScreenGui) · approval-gated · vision-token cost not tracked

**Concept.** Drop an image into Workshop; Claude Vision interprets it → emits a Rune script reproducing the layout / logic. Today the tool returns `(base64, prompt)` to `structured_data` and the agent must call Vision separately — a two-step flow.

**Forecasted feedback (R)**
- R2.1 Two-step (load-image → send-to-claude) is fragile; vision should be embedded in the tool handler.
- R2.2 No preview-before-commit; generated script lands in `SoulService/.generated/` immediately.
- R2.3 Rune/Luau only — no ScreenGui generation despite the schema claim.
- R2.4 Vision tokens (~1000/img) not budgeted alongside text tokens.
- R2.5 Hallucination — LLM invents non-existent properties; needs schema-validated output.

**Implications (I)**
- *Cross-system:* if ScreenGui generation lands, Studio's UI builder gets an AI shortcut.
- *Operational cost:* Vision is 5× the cost per call vs. text-only.
- *Support burden:* every wrong generation = a support ticket; staged-output review window helps.
- *Strategic:* fastest "wow" moment for new creators — high marketing value.

**Risks (X)**
- X2.1 Generated code referencing non-existent classes / properties produces immediate crash.
- X2.2 Privacy — image of a sensitive UI mockup goes to Anthropic.

**Mitigations (M)**
- M2.1 Schema-validated post-processor rejects unknown classes.
- M2.2 Opt-in flag for image upload; warn user before send.

---

### Feature 5 — Embedvec semantic search (HNSW)

**State:** 🟡 framework · **Effort:** L · **Risk:** Med · **Touches:** [02], [06], [04]
**Sub-features:** HNSW index · embedder trait · Knowledge / Ledger / Memory / Ontology stores · spatial + persistence features

**Concept.** A vector DB for semantic entity search ("find me all torches" → returns entities whose embeddings are nearest to a torch concept). Used by Workshop for context retrieval, by the Studio's Insert search, and (planned) by the Website's gallery semantic search.

**Forecasted feedback (R)**
- R5.1 All embedders today are **hash-based** (`SimpleHashEmbedder`, `ReflectPropertyEmbedder`) — no semantic meaning. Queries return effectively random results.
- R5.2 HNSW is in-memory only; no persistence between sessions (the `persistence` feature flag exists but is unwired).
- R5.3 No Rune/Luau script API to call `embedvec.query(...)`; only Workshop can.
- R5.4 No multi-modal (image + text together) — "find assets like this screenshot" impossible.
- R5.5 Curse of dimensionality at 384-D once a real model lands — quantisation or PCA needed.

**Implications (I)**
- *Architectural:* once a real embedder lands, every other text-search surface (Insert, Gallery, Memory) gets free upgrade.
- *Cross-system:* [06_WEBSITE] gallery semantic search depends on this (Feature 4 wiring gap there).
- *Operational cost:* CPU embedder via `candle` (all-MiniLM-L6-v2, 22M params) ~5 ms/embedding; acceptable.
- *Strategic:* AI-native discovery is a competitive moat — keyword search is table-stakes.

**Risks (X)**
- X5.1 Stale embeddings: entity property changes without reindex → wrong results.
- X5.2 In-memory only → restart loses everything; current state is useless across sessions.

**Mitigations (M)**
- M5.1 Auto-reindex on `Changed<Component>` ECS observer (Bevy native).
- M5.2 Enable `rocksdb` persistence feature; flush on save.

---

### Feature 7 — MCP Server (stdio JSON-RPC)

**State:** 🟡 binary built · **Effort:** L · **Risk:** Med · **Touches:** [02], [07], [10]
**Sub-features:** stdio JSON-RPC · MCP v2025-06-18 protocol · shared tool registry · universe resolution · file watcher (stub) · live ECS bridge (absent)

**Concept.** Out-of-process tool access for IDEs (Claude Desktop, Cursor, Windsurf). Same handlers as Studio in-process. Universe path discovered via env var or argument. File-watcher in `watcher.rs` is stubbed (no inotify subscription).

**Forecasted feedback (R)**
- R7.1 File watcher stubbed → IDEs don't get live file-change notifications.
- R7.2 No multi-universe support: two Cursor windows on two projects = two MCP servers, no shared state.
- R7.3 `ToolContext.live` field is unimplemented → spatial raycasts / live entity queries can't round-trip.
- R7.4 No `/resources/subscribe` MCP method → IDEs poll instead of push.

**Implications (I)**
- *Cross-system:* with live-bridge wired, MCP + LSP both become reactive — major unblock for external tooling.
- *Architectural:* the bridge port (`.eustress/engine.port`) becomes the cross-process integration point.
- *Strategic:* MCP-first is the bet that wins over the Claude Desktop / Cursor user base.

**Risks (X)**
- X7.1 Unauth localhost TCP → any process can inject tool calls.
- X7.2 IPC latency (~10 ms/round-trip) unsuitable for real-time raycasts in a loop.

**Mitigations (M)**
- M7.1 Token-gate the port (random per session, written to `.eustress/engine.port`).
- M7.2 Batch raycasts; don't round-trip per-ray.

---

### Feature 9 — Enhancement pipeline (FLUX + TripoSR)

**State:** 🟡 design / stub · **Effort:** L · **Risk:** High · **Touches:** [01], [04], [07]
**Sub-features:** Python `generation_server.py` · FLUX.1 texture generation · TripoSR mesh generation · SHA256 cache key · Bevy AssetServer integration · concurrent-limit (hardcoded 2)

**Concept.** A user enters an enhancement prompt + detail level; the engine hashes (prompt, category, detail) → cache lookup → if miss, dispatch to Python generation server (FLUX → texture, TripoSR → mesh) → cache result → spawn / replace asset. Stub mode works locally without GPUs.

**Forecasted feedback (R)**
- R9.1 Python server is out-of-process and Python-only — no Rust client wired in code.
- R9.2 No cost tracking — a user could spawn 100 requests, $500+ in credits.
- R9.3 Cache hit gives no UI feedback; users see "instant" results without explanation.
- R9.4 Server OOM → silent crash; no model fallback.
- R9.5 Hardcoded concurrent-limit (2) → no adaptive queue.
- R9.6 FLUX is non-deterministic (temperature > 0) → cached prompts that look re-runnable surprise users.

**Implications (I)**
- *Cross-system:* [04_ASSETS] caches the outputs; [01_CLIENT] consumes via enhancement pipeline; [05_SPACE_STREAMING] can request enhanced LOD0 meshes for hero items.
- *Operational cost:* GPU-bound; each request ~$0.05–0.10. Costs add up fast.
- *Migration:* Python server can be swapped for a Rust worker later; the contract is the HTTP API.
- *Support burden:* "AI is broken" tickets when generation server is down — needs a status indicator.

**Risks (X)**
- X9.1 Model drift across versions → projects look different after engine update.
- X9.2 Generation server outage → enhancement pipeline silently no-ops.

**Mitigations (M)**
- M9.1 Pin a `model_signature` in the cache key; refuse to use cache hits from older models.
- M9.2 Health-check the gen server; show a fallback procedural texture when down.

---

### Feature 10 — Project Korah (4-phase world-building)

**State:** 🔴 architecture only · **Effort:** XL · **Risk:** Critical (scope) · **Touches:** [02], [03], [04], [05], [07]
**Sub-features:** Foundation → Structure → Objects → Detail phases · multi-user sync (CRDT) · quality validator (CLIP / poly count / nav-coverage) · asset-request API to enhancement pipeline

**Concept.** "From imagination to inhabitation in seconds, not months." A user describes a world; Korah generates it in 4 phases, validating quality at each step. Designed to run multi-user (collaborative world-building). Currently a `korah` Cargo feature flag exists but no modules in code.

**Forecasted feedback (R)**
- R10.1 Implementation is **completely missing** — purely architectural doc.
- R10.2 Quality validator is multi-modal (CLIP similarity + performance + gameplay) — large dependency.
- R10.3 Asset-generation API undefined: how does Korah ask for "a medieval house"? Stub spec.
- R10.4 Multi-user sync would overlap with Multiplayer Studio (02 Feature 15) — unify.
- R10.5 Iterative refinement loop (generate → validate → refine → regenerate) can spend real money in tens of iterations.

**Implications (I)**
- *Architectural:* Korah is a meta-feature spanning AI + Multiplayer Studio + Asset Pipeline + Space Streaming.
- *Strategic:* if shipped, this is the "Eustress moment" — defining feature vs. Roblox / Unity.
- *Operational cost:* uncapped iterative gen is a budget hole — must cap per-phase iteration count.
- *Support burden:* "Korah didn't understand my prompt" tickets will be unbounded.

**Risks (X)**
- X10.1 Scope creep — Korah touches every other system; partial impl = technical debt.
- X10.2 LLM cost explosion in refinement loops.

**Mitigations (M)**
- M10.1 Defer Korah until Multiplayer Studio (02 F15), Asset enhancement (07 F9), Space Streaming (05) all ship.
- M10.2 Cap iteration count per phase (e.g. 3 retries max).

---

### Feature 11 — Physics-grounded inference (Symbolica + Kernel Law)

**State:** 🔴 concept only · **Effort:** XL · **Risk:** High · **Touches:** [07], [11_SIM]
**Sub-features:** prompt grounding from Kernel Law registry · output validation vs. physics constraints · recursive feedback loop (AI proposes → physics validates → AI refines)

**Concept.** Per `APEX_ENGINE.md` Pillar A. Workshop / Korah prompts include physics-law context (Nernst, Butler-Volmer, etc.) drawn from `eustress_common::realism::laws`. LLM-generated code is validated against the constraint engine; violations rejected.

**Forecasted feedback (R)**
- R11.1 KernelLawRegistry exists; no LLM bridge.
- R11.2 Validation needs a constraint AST; today's laws are functions, not constraints.
- R11.3 Symbolica integration is a separate research project — pin scope.

**Implications (I)**
- *Cross-system:* [11_SIMULATION] is the consumer for physics-validated AI outputs.
- *Strategic:* this is the differentiator for engineering-grade AI gen (battery cells, mechanisms, structures).

**Risks (X)** — X11.1 Symbolica dependency adoption is heavy.

**Mitigations (M)** — M11.1 Start with a small constraint vocabulary (gravity sign, mass non-negative).

---

### Feature 15 — Tool-manufacturing pipeline

**State:** 🟡 registry + guides; IoT untested · **Effort:** L · **Risk:** Med · **Touches:** [07], [09_ECONOMY]
**Sub-features:** `.tool.toml` file-is-DB · IoT telemetry (MQTT) · build guides · procurement (Amazon PA-API)

**Concept.** A creator describes a tool / fixture / device in a `.tool.toml`. Workshop generates build guides, procurement lists, IoT sensor wiring. Connects to the Bliss / manufacturing economy.

**Forecasted feedback (R)**
- R15.1 Registry resolution works; IoT MQTT thread pool untested.
- R15.2 Procurement API requires Amazon PA-API credentials per user — onboarding friction.
- R15.3 No moderation on auto-generated build guides (safety-critical content).

**Implications (I)** — *Strategic:* Eustress's only non-game vertical; unique vs. game-engine peers.

**Risks (X)** — X15.1 Auto-generated electrical / chemical guides risk user injury → liability.

**Mitigations (M)** — M15.1 Warn / require human review for any high-energy or chemical guide.

---

## Wiring / import gaps (top 12)

1. Vision API in `image_to_code` (one-shot, not two-step)
2. ML embedder for embedvec (`candle` + `all-MiniLM-L6-v2`)
3. RocksDB persistence for embedvec
4. MCP file watcher (`notify` subscription)
5. Live ECS bridge from MCP server (`.eustress/engine.port`)
6. Spatial LLM query implementation (`SpatialQuery` trait wired to a backend)
7. FoundationModelDispatcher (APEX Pillar A)
8. ECEF + WorldPosition (APEX Pillar B)
9. FEM integration (APEX Pillar C) — `fenris` in workspace deps
10. Physics-grounded prompt assembler from KernelLawRegistry
11. Korah phase state machine + quality validator
12. Enhancement-pipeline Rust HTTP client (replace Python stub eventually)

---

## Cross-system dependencies

- **[02_STUDIO]** Workshop UI; tool approval; image-to-code preview.
- **[04_ASSETS]** caches AI-generated meshes/textures; signs them (when [08] signing lands).
- **[08_IDENTITY]** BYOK key management; consent + audit log.
- **[09_ECONOMY]** AI call billing; per-user budgets.
- **[10_TELEMETRY]** Workshop tool tee, generation events, cost stream.
- **[11_SIMULATION]** physics-validated AI outputs; Korah quality validation.

---

## Open policy questions

- Q7.1 Per-user / per-project cost caps — daily? monthly?
- Q7.2 Local fallback (Llama-7B via `candle`) for offline mode — ship it?
- Q7.3 Training-data consent for generated content (creator opt-in?).
- Q7.4 Model versioning — auto-upgrade on Claude N+1 release, or opt-in?
- Q7.5 Attribution: % AI-generated stamp on published projects?
- Q7.6 Workshop in published Client (player dev mode) or strictly Studio-only?
