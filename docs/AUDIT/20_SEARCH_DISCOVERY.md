# 20 — Search & Discovery

> embedvec HNSW vector DB, full-text + semantic search, Insert-menu search, gallery
> discovery, property-path embeddings, recommendation systems. The **finding-things
> infrastructure** spanning Studio + Website + Workshop AI.

## Pass changelog

- **P3 (2026-05-14):** New doc; 9 features.

---

## Concept summary

Search & Discovery is a **cross-product concern**: it shows up in the Studio's Insert menu (find a Part or asset by name), the Website's gallery (find a project), the Workshop AI's context retrieval (find relevant memories / docs / similar entities), and the future marketplace recommender ("creators like you also build..."). One underlying engine ([eustress-embedvec](../../eustress/crates/embedvec/)) drives them all.

The current state is **infrastructure ready, embeddings naive**. The HNSW index, Sled persistence, RocksDB cold-tier, and per-property indexing are coded. The embedders are all hash-based (`SimpleHashEmbedder`, `ReflectPropertyEmbedder`) — meaning queries return effectively random results because no real semantic-distance learner is wired. Plugging in even `all-MiniLM-L6-v2` (22M-param transformer via `candle`) changes the system from "useless" to "production".

This doc is distinct from [07_AI_PLATFORM] (which describes the AI surface broadly) and [06_WEBSITE] (which describes the gallery UX) — it owns the **infrastructure** of indexing, querying, ranking, and invalidation.

---

## Implementation snapshot

**Crates / files:**
- [eustress-embedvec](../../eustress/crates/embedvec/) — HNSW + Sled persistence; ~10 modules: components, embedder, knowledge, ledger, memory, ontology, persistence, spatial, systems, plugin
- [eustress-web/src/api/gallery.rs](../../eustress/crates/web/src/api/) — gallery endpoint (substring search today)
- [engine/src/tools/insert/](../../eustress/crates/engine/src/) — Insert menu (substring filter)
- [common/src/services/teleport.rs](../../eustress/crates/common/src/services/) — `MatchmakingCriteria` (region + min_players)

**Working:**
- HNSW index structure
- Sled persistence (pure Rust)
- RocksDB cold-tier (feature-flagged)
- Per-property indexing
- Hash-based embedder

**Stubbed / missing:**
- ML embedder integration (`all-MiniLM-L6-v2` via `candle`)
- Multi-modal embeddings (image + text)
- Rune / Luau script API to query embedvec
- Backend `/api/gallery?semantic=true` endpoint
- Workshop AI direct query helper
- Search analytics (queries, click-through, ranking quality)
- Auto-reindex on entity property change

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | HNSW index + Sled persistence | ✅ |
| 2 | RocksDB cold tier | 🟡 feature-gated |
| 3 | Hash-based embedder | ✅ (placeholder) |
| 4 | ML embedder (`all-MiniLM-L6-v2`) | 🔴 |
| 5 | Multi-modal embeddings (image + text) | 🔴 |
| 6 | Insert-menu semantic search | 🟡 substring only |
| 7 | Gallery semantic search (`/api/gallery?semantic=true`) | 🔴 |
| 8 | Rune / Luau script API for embedvec | 🔴 |
| 9 | Auto-reindex on property change | 🔴 |

---

## Detailed per-feature cards (top 6)

### Feature 1 — HNSW index + Sled persistence

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [07], [20]
**Sub-features:** HNSW (Hierarchical Navigable Small World) approximate nearest-neighbor · `M`, `efConstruction`, `efSearch` tuning · per-namespace index · Sled disk-backed persistence · in-memory cache layer

**Concept.** HNSW is the de-facto vector-index algorithm — log-scale query time at the cost of approximate (not exact) results. Sled keeps the index on disk; loads on demand.

**Forecasted feedback (R)**
- R1.1 Default HNSW params (`M=16`, `efConstruction=200`, `efSearch=50`) work for ≤1M vectors; need re-tune at higher scale.
- R1.2 Sled vs. RocksDB — Sled is pure Rust (no C deps); RocksDB is faster at scale; both exist as feature flags.
- R1.3 No incremental save — full index rewrite on flush is wasteful.
- R1.4 Per-namespace isolation (entities vs. assets vs. memories) is correct architecture.

**Implications (I)**
- *Architectural:* one engine serves every search surface; consolidate.
- *Cross-system:* [07_AI_PLATFORM] knowledge / ledger / memory all use the same backbone.

**Risks (X)** — X1.1 Sled crash recovery is "mostly works"; verify under power-loss.

**Mitigations (M)** — M1.1 Periodic snapshot + checksum.

---

### Feature 4 — ML embedder integration

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [07], [20]
**Sub-features:** `candle` (Hugging Face Rust) loads `all-MiniLM-L6-v2` (22M params, ~80 MB) · per-text embedding (~5 ms CPU) · GPU acceleration optional · embedding cache · per-language model support

**Concept.** A real transformer model produces 384-dim semantic embeddings. Search becomes "find by meaning", not "find by substring". `candle` runs ONNX / safetensors models in pure Rust.

**Forecasted feedback (R)**
- R4.1 Model size (~80 MB) bundled or downloaded on first run?
- R4.2 First-embedding cold start is slow (~200 ms to load); pre-warm.
- R4.3 Embedding cache keyed by text-hash (xxhash64); same text never re-embeds.
- R4.4 GPU acceleration via `candle-cuda` / `candle-metal` for batched bulk index.
- R4.5 Multilingual model variants for [06_WEBSITE] i18n future.

**Implications (I)**
- *Architectural:* one ML model in the engine commits to its license + size.
- *Operational:* +80 MB engine binary; +200 ms cold start.
- *Strategic:* "semantic-native" platform messaging requires this.

**Risks (X)**
- X4.1 Mobile inference is slow / hot; force fallback to hash embedder on mobile.
- X4.2 Model licence (Apache 2.0 for MiniLM) is OK; verify upstream.

**Mitigations (M)**
- M4.1 Conditional compile: ML embedder feature; mobile = hash by default.
- M4.2 Bundle model in installer / download once.

---

### Feature 5 — Multi-modal embeddings

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [04_ASSETS], [07], [20]
**Sub-features:** CLIP-style dual encoder · image input · joint image+text embedding · "find assets like this screenshot" query · texture/material visual similarity

**Concept.** A user uploads a screenshot or drops an image in the Workshop; the system finds visually similar assets. Uses a CLIP-family model (e.g. `clip-vit-base-patch32`).

**Forecasted feedback (R)**
- R5.1 CLIP model size (~150 MB) doubles bundle.
- R5.2 GPU inference recommended for image embedding (CPU ~200 ms / image).
- R5.3 Image preprocessing pipeline (resize / normalise / tensor) needs care.
- R5.4 Cross-modal alignment (text-of-asset matches image-of-asset) is the magic.

**Implications (I)** — *Strategic:* "find similar to this picture" is the differentiator vs. text-only search.

**Risks (X)** — X5.1 CLIP biases (Western-art-skewed training data) hurt diverse content.

**Mitigations (M)** — M5.1 Allow user-trained / fine-tuned alternatives.

---

### Feature 6 — Insert-menu semantic search

**State:** 🟡 substring · **Effort:** S · **Risk:** Low · **Touches:** [02_STUDIO], [20]
**Sub-features:** type-ahead query · top-K results · ranking signals (recency, popularity, user history) · preview thumbnail · drag-to-spawn

**Concept.** Studio Insert menu currently does substring match on class/asset names. Replace with embedvec semantic — typing "torch" finds `Lamp`, `Lantern`, `Candle`, even if not named "torch".

**Forecasted feedback (R)**
- R6.1 Today's substring is fast (~1 ms / 10k items) — semantic must match latency.
- R6.2 Hybrid scoring: weight substring match higher when user is typing exactly the name.
- R6.3 Personalisation — user who builds dungeons sees `Torch` above `Candle`.

**Implications (I)** — *Architectural:* Insert menu is the most-used Studio UX — invisible improvement is real.

**Mitigations (M)** — M6.1 Pre-warm common terms on Studio start.

---

### Feature 7 — Gallery semantic search

**State:** 🔴 · **Effort:** M · **Risk:** Low · **Touches:** [06_WEBSITE], [20]
**Sub-features:** `/api/gallery?semantic=true&q=...` backend endpoint · per-project embedding (title + description + tags) · trending boost · personalised ranking

**Concept.** Website gallery search uses substring today. Semantic search returns "projects like this idea" — even when the project doesn't contain the exact keyword.

**Forecasted feedback (R)**
- R7.1 Backend endpoint not built.
- R7.2 Trending algorithm (play_count delta over time) needs a separate pipeline.
- R7.3 Personalisation requires user history (consent + Identity).
- R7.4 Cold-start (new gallery) returns featured / curated until data accumulates.

**Implications (I)** — *Strategic:* discovery is the retention lever for free-tier projects.

**Mitigations (M)** — M7.1 Hybrid: substring + semantic + popularity + recency.

---

### Feature 8 — Rune / Luau script API

**State:** 🔴 · **Effort:** S · **Risk:** Low · **Touches:** [07], [20]
**Sub-features:** `embedvec.query("warrior", 5)` returns top-5 entities · `embedvec.similar_to(entity_id, 5)` · `embedvec.add(entity_id, text)` · per-script-quota

**Concept.** Scripts can query the index directly. Use cases: NPC dialogue retrieval ("find quotes similar to current mood"), procedural placement ("find an enemy similar to this archetype"), AI quest generation.

**Forecasted feedback (R)** — R8.1 No bindings yet; embedvec only callable from Rust + Workshop.

**Implications (I)** — *Cross-system:* enables "AI-grade" gameplay features in user scripts.

**Mitigations (M)** — M8.1 Per-script rate limit on query calls.

---

## Wiring / import gaps (top 8)

1. `candle` + `all-MiniLM-L6-v2` ML embedder
2. RocksDB persistence flag enabled by default
3. CLIP-style multi-modal embedder
4. Insert menu → embedvec query
5. Gallery backend `/api/gallery?semantic=true`
6. Rune / Luau `embedvec` module
7. Auto-reindex via Bevy `Changed<Component>` observer
8. Search analytics ingest (queries, click-through, ranking quality)

---

## Cross-system dependencies

- **[02_STUDIO]** Insert menu + asset search.
- **[04_ASSET_PIPELINE]** asset metadata feeds indexer.
- **[06_WEBSITE]** gallery search + recommendations.
- **[07_AI_PLATFORM]** Workshop knowledge / memory retrieval.
- **[09_ECONOMY]** marketplace recommendation engine.
- **[10_TELEMETRY]** search analytics events.

---

## Open questions

- Q20.1 ML model bundled (+80 MB binary) or downloaded on first run?
- Q20.2 Single multilingual model or per-language models?
- Q20.3 CLIP licence acceptable for commercial use?
- Q20.4 Personalisation requires consent — opt-in or opt-out?
- Q20.5 Embedding refresh cadence for already-indexed items?
- Q20.6 Marketplace recommender — collaborative filtering or content-based?
