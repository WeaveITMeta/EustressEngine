# 05 — Space Streaming

> **Server-to-client (and disk-to-ECS) chunked content delivery.** How a Space
> with 10M+ instances loads progressively into a player's view without OOM
> or stall. *Distinct from* the EustressStream event-topic system, which is
> now [10_TELEMETRY.md](10_TELEMETRY.md).

## Pass changelog

- **P2 (2026-05-14):** Full rewrite — was previously documenting EustressStream topics. New scope: `.echk` chunk format, hysteresis radii, server dispatcher, client spawner, terrain streaming, LOD ladder. 9 features, ~50 R / ~40 I / ~30 X+M bullets.
- **Updated 2026-05-16: storage pivot.** The `.echk` chunk format + bake pipeline now have a **real implementation** in the `eustress-worlddb` crate (`bake_to_echk` with deterministic blake3 per-chunk hashing for delta upload, plus a `decode_echk` inverse) — Features 1 and 7 are no longer "spec only / absent" for the encode/decode core. **⚠️ Open convergence decision (needs human):** there are now **two chunk formats** — worlddb's `.echk` version-0 container vs. this audit's 56-byte packed-instance wire format. They are unreconciled; this doc does **not** assert one. See MASTER C17.

---

## Concept summary

A published Space can contain **millions** of instances — parts, foliage, decals, terrain voxels, animated entities. Loading every instance up-front on the Client is infeasible (memory, GPU, network). **Space Streaming** is the system that progressively delivers Space content to a connected client based on the player's position, prioritising nearby high-detail content and evicting distant content.

The Eustress architecture (designed in [CHUNKED_STORAGE.md](../development/CHUNKED_STORAGE.md), reflected in `eustress_common::streaming::*` modules) is **three-tier**:

- **Cold tier** — disk-resident `.echk` binary chunks, indexed by `(chunk_x, chunk_z)`. Compressed (LZ4 / Zstd). No frame cost.
- **Hot tier** — in-RAM `DashMap<ChunkCoord, ChunkData>` cache. Decompressed + parsed, ready for spawn. No ECS / GPU cost.
- **Active tier** — Bevy ECS entities with full `Transform`, `Mesh3d`, materials, physics. Counted against an `active_cap` (default 2.10M for the benchmark envelope).

Promotion (`Cold → Hot → Active`) and demotion (`Active → Hot → Cold`) is **hysteresis-driven**: an entity activates when within `active_radius` (default 500 m) and only demotes once outside `evict_radius` (default 600 m). The 100 m gap kills oscillation at the boundary.

Server-side, the server holds the canonical chunk store and dispatches chunks on request (via `RequestStreamAroundAsync` or replication channel). Studio also reads chunks for editing; chunk authority lets it patch a single chunk without rewriting the whole Space.

The system has a **complete formal spec** ([CHUNKED_STORAGE.md](../development/CHUNKED_STORAGE.md)) and a partially-built **Rust scaffold** in [common/src/streaming/](../../eustress/crates/common/src/streaming/) — but **zero end-to-end wiring**. The current `distance_chunking_system` in the Client is for **AI enhancement scheduling**, not streaming, and is mis-named.

---

## Implementation snapshot

**Crates:**
- [eustress-common::streaming](../../eustress/crates/common/src/streaming/) — types, chunk_grid, radius_gate, dirty_flusher, toml_watcher, instance_index, plugin
- [eustress-common::classes::ChunkedWorld](../../eustress/crates/common/src/classes/) — TOML class with `chunk_size`, `load_radius`, `unload_radius`, `lod_distances`, `compression`, `manifest_path`, `chunks_path`
- [docs/development/CHUNKED_STORAGE.md](../development/CHUNKED_STORAGE.md) — formal 56-byte `PackedInstance` spec
- [docs/EEP_SPECIFICATION.md](../EEP_SPECIFICATION.md) §ChunkedWorld — TOML schema

**What's coded (scaffolded):**
- `types.rs` — `InstanceId`, `InstanceBin` (`bytemuck::Pod`), `InstanceRecord` (Tier enum), `ChunkCoord` (2D), `StreamingConfig`
- `chunk_grid.rs` — `SpatialChunkGrid` (DashMap + RTree), O(log N) radius queries
- `radius_gate.rs` — `HysteresisRadiusGate` two-threshold promote/demote
- `dirty_flusher.rs` — async batched write-back (100 ms / 1000-instance batches)
- `toml_watcher.rs` — file-event classifier
- `instance_index.rs` — flat metadata index for Explorer

**What's missing:**
- `.echk` binary encoder + decoder
- Manifest TOML reader / writer
- Server-side chunk dispatcher
- Client chunk request protocol
- Client entity spawner from chunk data
- Mesh LOD ladder (no LOD0–LOD3 variants are read)
- Terrain voxel streaming
- Workspace `.glb.toml` → `.echk` "bake" tool

**Misleading code in Client:**
- [client/src/systems/distance_chunking.rs](../../eustress/crates/client/src/systems/) — this is the **AI enhancement scheduler**, NOT Space Streaming. The name collision needs fixing.

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | `.echk` chunk binary format (56-byte `PackedInstance`) | 🟡 *(2026-05-16: worlddb `.echk` v0 encoder/decoder real; audit's 56-byte wire format still spec — formats unreconciled, see C17)* |
| 2 | Manifest TOML round-trip (`manifest.toml`) | 🟠 spec only |
| 3 | Spatial grid + hysteresis radius gate | 🟡 scaffolded |
| 4 | Server chunk dispatcher (`RequestStreamAround`) | 🔴 absent |
| 5 | Client chunk requester + decoder | 🔴 absent |
| 6 | Client entity spawner (batch ECS materialisation) | 🔴 absent |
| 7 | Studio "bake" tool (TOML → `.echk`) and inverse | 🟡 *(2026-05-16: `eustress-worlddb::bake_to_echk` + `decode_echk` real; Studio command wiring still absent)* |
| 8 | Mesh LOD ladder + variant selection | 🔴 absent |
| 9 | Terrain voxel streaming (separate path) | 🔴 absent |

---

## Detailed per-feature cards

### Feature 1 — `.echk` chunk binary format

**State:** 🟡 *(see note)* · **Effort:** M · **Risk:** Med · **Touches:** [02_STUDIO], [04_ASSETS], [01_CLIENT], [03_MULTIPLAYER], C17
**Sub-features:** Magic header (`ECHK`+u32 version) · class/material/mesh tables (dedup) · `PackedInstance` C-layout (56 bytes) · LZ4/Zstd block compression · CRC integrity · *(2026-05-16)* worlddb `.echk` v0 container + deterministic per-chunk blake3

**Concept.** Each chunk is a self-contained binary file at `<ChunkedWorld>/chunks/<x>_<z>.echk`. Header includes magic bytes, version, flags, bounds, instance count, table offsets, and a CRC. Body is dedup'd class / material / mesh tables followed by a packed array of 56-byte `PackedInstance` structs — `bytemuck::Pod`, zero-copy GPU-uploadable. Compressed with LZ4 (fast) or Zstd (smaller).

> **⚠️ Updated 2026-05-16: storage pivot — open convergence decision.** The `eustress-worlddb` crate now ships a **real `.echk` version-0 container** with `bake_to_echk` (deterministic blake3 per-chunk hashing for delta upload) and a `decode_echk` inverse. This is a **different format** from the 56-byte packed-instance wire format specced here ([CHUNKED_STORAGE.md](../development/CHUNKED_STORAGE.md)). The two are **unreconciled**; the audit does **not** assert which becomes the on-disk/on-wire standard. **This needs a human decision** — do not infer a resolution.

**Forecasted feedback (R)**
- R1.1 Endianness: pick little-endian, document it, fail loud on big-endian readers.
- R1.2 Version field is u32 — semantic versioning or monotonic counter? Reviewers want a migration policy.
- R1.3 56-byte instance is tight; future fields (NetworkOwner, AnimationHandle) force a v2 layout — design extension points (e.g. trailing variable-size block).
- R1.4 Mesh / material table dedup is per-chunk; a 10M-instance world with 100 unique meshes wastes nothing per chunk, but cross-chunk dedup at the manifest layer is also wanted.
- R1.5 CRC vs. cryptographic signature — pick CRC for fast-fail integrity; signature is asset-pipeline concern.
- R1.6 Block compression vs. whole-file: block enables partial decode and parallel decompress.

**Implications (I)**
- *Architectural:* this format becomes the wire protocol between server and client — once shipped, breaking changes mean stranded projects.
- *Cross-system:* [04_ASSET_PIPELINE](04_ASSET_PIPELINE.md) needs an `.echk` content-hash on top of the existing world-container blake3 hashing — `eustress-worlddb::bake_to_echk` now produces a deterministic per-chunk blake3 for exactly this delta-upload purpose *(2026-05-16; was "on top of `.pak` hashing")*.
- *Migration:* old `.glb.toml` projects need a one-shot bake; the bake tool (Feature 7) is its own deliverable.
- *Operational cost:* tighter pack = less R2 egress; 56 B × 10M = 560 MB raw, ~150 MB LZ4 = real CDN savings.
- *Support burden:* opaque binary files are hard to debug — ship `eustress echk inspect <file>` from day one.
- *Strategic:* matches Roblox's `.rbxlx`-binary parity; differentiates from Unity AssetBundle by being human-bake-able.

**Risks (X)**
- X1 Float drift across compilers (e.g. fast-math) corrupts the bit-identical packed format.
- X2 Malicious chunk can claim huge `instance_count` → DoS the spawner; need bounds validation.
- X3 Endianness mistakes shipped to the wild are permanent.

**Mitigations (M)**
- M1 Reject anything with `instance_count > MAX_CHUNK_INSTANCES` (e.g. 100k) at decode.
- M2 Fuzz-test the decoder on random byte streams.
- M3 Validate CRC before allocating any instance buffer.

---

### Feature 2 — Manifest TOML round-trip

**State:** 🟠 spec only · **Effort:** S · **Risk:** Low · **Touches:** [02_STUDIO], [05_SPACE], [04_ASSETS]
**Sub-features:** `manifest.toml` schema · per-chunk hash + size + mtime · world bounds · chunk-coord → file path mapping

**Concept.** A `<ChunkedWorld>/manifest.toml` lists every chunk by coord, with its size, hash, last-modified time, and the path inside the chunks/ folder. Studio writes it on bake; server reads it to know what's available; the manifest is the single source of truth for "does this chunk exist".

**Forecasted feedback (R)**
- R2.1 Sorting (coord order vs. modification order) matters for diffs; pick coord-sorted.
- R2.2 Hash algorithm — match the platform's content-hash story (blake3 already used in publish).
- R2.3 Manifest size at 10M instances / 256 m chunks = a few MB; large but tolerable.
- R2.4 Partial-bake support: don't require the whole manifest to be rewritten when one chunk changes — append + sort on bake.

**Implications (I)**
- *Architectural:* manifest is the API between Studio bake and server dispatch.
- *Cross-system:* feeds [02_STUDIO]'s Chunk Editor panel listing and [10_TELEMETRY]'s `space.chunk_baked` event.
- *Migration:* easy — old projects without `manifest.toml` fall back to "no chunks present".

**Risks (X)**
- X1 Manifest written non-atomically can corrupt; use `write + rename`.
- X2 Hash drift if encoder changes silently → stale manifests.

**Mitigations (M)**
- M1 Always atomic write; include encoder-version in the manifest.

---

### Feature 3 — Spatial grid + hysteresis radius gate

**State:** 🟡 scaffolded · **Effort:** M · **Risk:** Low · **Touches:** [01_CLIENT], [02_STUDIO], [03_MULTIPLAYER]
**Sub-features:** 2D chunk-coord grid · R-tree for radius queries · two-threshold gate · per-tier transitions · cap enforcement

**Concept.** `SpatialChunkGrid` (DashMap + RTree) gives O(log N) "chunks within radius R" queries. `HysteresisRadiusGate` promotes a chunk when within `active_radius` (default 500 m), and only demotes when it crosses `evict_radius` (default 600 m). The 100 m hysteresis is critical at the boundary — without it, a stationary player at exactly 500 m would burn CPU oscillating.

**Forecasted feedback (R)**
- R3.1 2D vs. 3D grids: shipping 2D is fine for surface worlds; an aerial / underground world needs 3D (chunk_y).
- R3.2 R-tree on chunk centers is fine; balance on insert is the only cost.
- R3.3 `active_cap` (2.10M envelope) is configurable but needs a graceful overflow strategy (LRU evict by distance).
- R3.4 The Studio uses the same grid for the Chunk Editor — keep one impl.
- R3.5 Multi-camera (split-screen, NPC POV) needs union-of-radii.

**Implications (I)**
- *Architectural:* the gate is the central spawning policy; every entity-create surface (Studio Insert, Multiplayer replicated spawn, AI generation) should route through it.
- *Cross-system:* [03_MULTIPLAYER]'s AOI filter is conceptually the same thing — unify.
- *Operational cost:* tunable per-project; some worlds want 200 m / 250 m for tight FPS, others 2 km for open-world feel.
- *Strategic:* makes 10M+ instance worlds a reality vs. Unity which OOMs at ~100k loaded.

**Risks (X)**
- X1 Player teleport (Space change, Workshop "go to") blows the radius assumption — burst-spawn 100k entities in one frame.
- X2 The cap isn't tied to memory; a project with huge meshes blows GPU even under the cap.

**Mitigations (M)**
- M1 Teleport pre-warm: load chunks for the destination before the camera moves.
- M2 Budget instance count *and* approximate VRAM (per-mesh footprint estimate).

---

### Feature 4 — Server chunk dispatcher

**State:** 🔴 absent · **Effort:** L · **Risk:** High · **Touches:** [03_MULTIPLAYER], [01_CLIENT], [12_INFRASTRUCTURE]
**Sub-features:** `RequestStreamAround` RPC · per-client radius state · chunk-priority queue · concurrent-fetch cap · disk I/O budget · Forge integration for chunk-server scaling

**Concept.** The server holds the canonical chunk store (the extracted `.eustress` world container — Fjall WorldDb database + baked `.echk` chunks; was "the extracted `.pak` Universe" pre-2026-05-16). When a client connects (or moves), the server computes which chunks lie within the client's radius set, deltas against what the client already has, and streams missing chunks over QUIC. Priority queue orders by distance + (optionally) frustum.

**Forecasted feedback (R)**
- R4.1 Roblox-parity: `RequestStreamAroundAsync` is the API name (see FEATURE_PARITY row 18).
- R4.2 What's the wire format — raw `.echk` bytes, or a re-pack? Recommend raw to avoid re-encode.
- R4.3 Concurrent-fetch cap per client (e.g. 4 chunks in flight) prevents network thrash.
- R4.4 Server disk I/O budget: a popular world has 100 clients × 4 in-flight × N tick = lots of reads — need an mmap cache.
- R4.5 What about chunks that need server-side mutation (e.g. a player builds a wall) — chunk is dirty until next bake?
- R4.6 Per-region chunk fan-out for multi-region servers (Forge concern).
- R4.7 Bandwidth budget per player: 150 MB Space ÷ 2 min walk-time = ~1.3 MB/s — fine on most connections; mobile / satellite is the squeeze.

**Implications (I)**
- *Architectural:* this is the bridge from "static disk content" to "live multiplayer stream"; it's the contract every Client speaks.
- *Cross-system:* [03_MULTIPLAYER] replication channel either *carries* chunk packets (one shared channel) or runs alongside (two channels) — pick one early.
- *Migration:* old projects don't have chunks; server falls back to spawning every TOML entity (the current behaviour) — keep that path for solo & small worlds.
- *Operational cost:* server RAM scales with concurrent players × in-flight chunks; budget upfront.
- *Support burden:* "missing chunk" errors are user-visible (hole in the world) — log loudly.
- *Strategic:* this feature is *the* differentiator vs. Unity / Unreal for browser-scale UGC; without it the platform caps at ~50k entities per project.

**Risks (X)**
- X1 No backpressure → server thrashes when 50 players all teleport.
- X2 No client validation of chunk-hash against manifest → server can lie / replay old chunks.
- X3 Chunks served from disk on every request → 100× the I/O of memmap-cached.
- X4 No disconnect cleanup → server leaks per-client priority state.

**Mitigations (M)**
- M1 Per-client token bucket (max chunks/sec) + global I/O budget.
- M2 Client verifies chunk hash against the manifest; reject + re-request mismatches.
- M3 Mmap the chunks/ directory on server startup; serve from page cache.

---

### Feature 5 — Client chunk requester + decoder

**State:** 🔴 absent · **Effort:** L · **Risk:** High · **Touches:** [01_CLIENT], [03_MULTIPLAYER]
**Sub-features:** request scheduler (distance-priority) · async decoder (off main thread) · CRC verify · Cold/Hot/Active state machine · LRU eviction

**Concept.** Each client maintains its loaded-chunk set, requests missing chunks within `active_radius`, decodes them asynchronously (CRC, decompress, parse tables, decode instances), and feeds the result to the entity spawner. Eviction beyond `evict_radius` drops chunks back to a small Hot-cache LRU before disk-cold.

**Forecasted feedback (R)**
- R5.1 Decode must run off the main thread (rayon / tokio) — locking the render loop is unacceptable.
- R5.2 Decoding speed target: 2M instances/sec (per agent benchmark) — confirm on slowest target hardware (mobile).
- R5.3 Priority queue must be re-orderable mid-flight (player turns) — heap with lazy invalidation.
- R5.4 Memory cap on Hot tier: when full, evict the chunk furthest from camera that's not Active.
- R5.5 Cold-disk cache on the client: should client persist Hot chunks across launches? Solves cold-start UX.

**Implications (I)**
- *Architectural:* this is the largest per-frame system on the Client side.
- *Cross-system:* shares the spatial grid with [03_MULTIPLAYER] AOI replication (unify).
- *Migration:* clients running today's `distance_chunking_system` (AI scheduler) need to coexist with the new Space Streaming system; *rename* the old one to `enhancement_scheduler_system`.
- *Operational cost:* on disk, ~150 MB / 500 m radius — fine.
- *Support burden:* "chunk failed to decode" errors must surface in a debug HUD, not silent-skip.
- *Strategic:* this is the player-facing rendering path; performance here dictates whether large worlds feel smooth.

**Risks (X)**
- X1 Decode CPU pegs at 100% during fast travel; need a `decode_budget_ms_per_frame`.
- X2 No prefetch on direction-of-motion → chunks arrive late, player sees pop-in.
- X3 Network packet loss + slow re-request → visible hole.

**Mitigations (M)**
- M1 Time-budget the decoder (e.g. 4 ms/frame); reorder if budget exceeded.
- M2 Motion-aware prefetch — request a cone ahead of the camera vector.
- M3 Display a low-detail proxy (chunk-bounds AABB cube) until decode lands.

---

### Feature 6 — Client entity spawner (batch ECS materialisation)

**State:** 🔴 absent · **Effort:** L · **Risk:** Med · **Touches:** [01_CLIENT], [02_STUDIO]
**Sub-features:** batch insert · mesh-handle dedup via cache · material-handle dedup · physics-flag application · NetworkOwner stamping · canonical create routing (C2)

**Concept.** Given a decoded chunk (Vec of PackedInstance + tables), produce ECS entities. Mesh / material handles are looked up in caches (one Bevy `Handle` per unique asset). Physics flags (anchored, collidable) drive Avian component insertion. Each entity is routed through `eustress_common::instance_create::create_instance` (C2) so Studio paths and runtime paths agree.

**Forecasted feedback (R)**
- R6.1 Bevy `Commands.spawn_batch` is the right primitive; profile against 10k-instance batches.
- R6.2 Mesh / material handle dedup: lookup or load; LRU on mesh-cache memory.
- R6.3 NetworkOwner stamping: server-side dispatch decides ownership; client just records it.
- R6.4 Per-instance Color packed `u32` → Bevy `Color` — handle alpha pre-mult.
- R6.5 Hot-path allocator: spawning is the bottleneck — pre-allocate `Vec` capacity.

**Implications (I)**
- *Architectural:* routing through C2 (canonical create) keeps Studio + Client + Multiplayer in sync.
- *Cross-system:* C12 (storage mode) — single-author Fjall WorldDb (was file-system-first pre-2026-05-15) and Multiplayer-Studio-cloud both feed this same spawner.
- *Migration:* entities spawned from chunk vs. entities materialised from the Fjall WorldDb store (or its legacy/seed `_instance.toml` import) must end up indistinguishable in ECS.

**Risks (X)**
- X1 Bevy archetype churn from heterogeneous spawn batches — measure.
- X2 Spawn-time race with physics step — entities popping into the solver mid-tick is bad.

**Mitigations (M)**
- M1 Spawn in `Last` schedule slot; Avian sees them next frame.
- M2 Group spawn-batches by archetype (sort by class_id pre-spawn).

---

### Feature 7 — Studio bake tool (TOML → `.echk`) and inverse

**State:** 🟡 *(encode/decode core real in `eustress-worlddb`; Studio command + manifest wiring still absent)* · **Effort:** L · **Risk:** Med · **Touches:** [02_STUDIO], [04_ASSETS], C17
**Sub-features:** scan Workspace/ → group by chunk coord → encode `.echk` → write manifest · inverse: pick one chunk, extract to `.part.toml` for editing · partial re-bake (only changed chunks) · *(2026-05-16)* `eustress-worlddb::bake_to_echk` (deterministic blake3 per chunk) + `decode_echk` inverse

**Concept.** *Updated 2026-05-16: storage pivot.* A "Bake to Chunks" command in the Studio (or a CLI `eustress bake`) walks the loaded Universe (now the Fjall WorldDb store), sorts instances into chunk buckets by world position, encodes each bucket into an `.echk` file, and writes the manifest. The encode/decode core now exists in `eustress-worlddb` (`bake_to_echk` with deterministic per-chunk blake3 for delta upload; `decode_echk` inverse); the Studio command + manifest wiring is still absent. The inverse extracts a single chunk back to per-instance TOML for manual editing — then re-bakes when saving. *(Note the Feature 1 convergence flag: worlddb's `.echk` v0 container vs. the 56-byte packed-instance wire format are two unreconciled formats — needs a human decision.)*

**Forecasted feedback (R)**
- R7.1 Bake is slow on first run; partial re-bake (only changed chunks via `Changed<Transform>` watcher) is essential.
- R7.2 Roundtrip preservation: TOML comments, ordering, and custom fields must survive extract → re-bake.
- R7.3 Selection-based bake: bake "everything I have selected" for tutorial worlds.
- R7.4 Studio in **Multiplayer Studio mode** ([02_STUDIO] Feature 15) cannot bake mid-session — chunks are cloud-resident; bake on session-end.

**Implications (I)**
- *Architectural:* introduces a "build artifact" concept inside the Studio — like compiling code.
- *Cross-system:* the publish flow ([04_ASSETS] Feature 0) bundles the `.eustress` world container (Fjall WorldDb database + baked `.echk` + manifest), not per-instance TOML, for large worlds. *(Was the `.pak` flow pre-2026-05-16.)*
- *Migration:* old projects with no chunks just keep their per-instance TOML path; bake is opt-in.

**Risks (X)**
- X1 Bake errors mid-write leave a broken manifest → atomic file ops.
- X2 Extract overwrites a chunk's edits if multiple users are baking (Multiplayer Studio).

**Mitigations (M)**
- M1 Bake to a temp dir, atomic-rename on success.
- M2 In Multiplayer Studio, only the session host can bake; or queue and merge.

---

### Feature 8 — Mesh LOD ladder + variant selection

**State:** 🔴 absent · **Effort:** M · **Risk:** Med · **Touches:** [04_ASSETS], [01_CLIENT]
**Sub-features:** LOD0–LOD3 GLB variants per mesh · distance-driven swap · impostor billboards for far LOD · mesh-cache versioning

**Concept.** Each unique mesh asset has variants (LOD0 full-detail, LOD1 / LOD2 / LOD3 progressively coarser, plus an impostor billboard for very far). Distance bands ([256 m, 512 m, 1024 m] default) drive variant selection. Swaps are smooth (cross-fade or LOD pop-tolerant).

**Forecasted feedback (R)**
- R8.1 LOD variants must be generated automatically from LOD0 — no creator should hand-author four GLBs.
- R8.2 Impostor billboards (camera-facing quad with a baked atlas texture) work for distant trees / rocks; not for buildings.
- R8.3 Cross-fade requires double-rendering during transition — expensive at 2M entities.
- R8.4 LOD selection per-entity = O(N); use per-chunk LOD for the bulk + override per-entity only for hero items.
- R8.5 Mobile cap: force LOD2 minimum on phones.

**Implications (I)**
- *Architectural:* the mesh table in `.echk` should carry LOD-variant indices, not just one mesh_id.
- *Cross-system:* texture-gen / asset pipeline must emit LOD variants on import.
- *Operational cost:* 4 LODs × 1 mesh = 4× storage; impostor offsets it for high-poly assets.

**Risks (X)**
- X1 LOD pop is visually jarring; cross-fade is expensive.
- X2 Auto-generated LOD2/3 may break silhouette of important items.

**Mitigations (M)**
- M1 Per-mesh LOD config: distances + auto-gen on/off.
- M2 Pop-allow flag for ambient mesh; cross-fade for hero mesh.

---

### Feature 9 — Terrain voxel streaming

**State:** 🔴 absent · **Effort:** XL · **Risk:** High · **Touches:** [01_CLIENT], [02_STUDIO], [04_ASSETS]
**Sub-features:** terrain chunk = voxel + biome + erosion data · separate from instance chunks · same hysteresis radii · marching cubes / dual-contour decode · paint-brush write-back (Studio) · navmesh per-chunk

**Concept.** Terrain is a separate streaming path — voxel grids, not instances. Each terrain chunk decodes into a mesh via marching cubes / dual contouring on the GPU or CPU, plus a per-chunk navmesh and biome texture. Hysteresis radii reuse the instance-chunk numbers.

**Forecasted feedback (R)**
- R9.1 Voxel resolution: 1 m / cell or 0.5 m? Spec it.
- R9.2 Server-authoritative paint-brush: Studio in Multiplayer mode sends ops to server, server updates voxel grid, broadcasts dirty chunks.
- R9.3 GPU marching cubes is fastest; CPU fallback for mobile.
- R9.4 Navmesh per chunk is hard to stitch at chunk boundaries — need overlap.

**Implications (I)**
- *Architectural:* terrain is a heavy lift; defer until Feature 4–6 ship.
- *Cross-system:* [11_SIMULATION] needs deterministic terrain for replay.

**Risks (X)**
- X1 Boundary seams between adjacent chunks (texture / mesh).
- X2 Paint-brush latency in Multiplayer ruins the editing feel.

**Mitigations (M)**
- M1 Always emit a 1-cell overlap between adjacent chunks.
- M2 Optimistic local paint with server reconciliation.

---

## Wiring / import gaps (top 15)

1. `.echk` encoder in `eustress_common::streaming::echk_encoder`
2. `.echk` decoder + CRC verify
3. Manifest TOML reader / writer
4. `RequestStreamAround` RPC schema (shared with `03_MULTIPLAYER`)
5. Server-side chunk dispatcher + per-client state
6. Client chunk-priority queue
7. Client async decode worker pool
8. Client entity spawner (batch + dedup)
9. Rename `client::distance_chunking_system` → `enhancement_scheduler_system` to free the name
10. Studio "Bake to chunks" command + ribbon entry
11. Studio Chunk Editor panel
12. LOD variant generator (offline, in [04_ASSETS])
13. Terrain voxel chunk format (separate `.tchk`?)
14. Multiplayer-Studio bake-on-session-close hook
15. Telemetry events: `space.chunk_baked`, `space.chunk_streamed`, `space.chunk_decode_failed` (see [10_TELEMETRY])

---

## Cross-system dependencies

- **[01_CLIENT_PLAYER](01_CLIENT_PLAYER.md)** — chunk requester + decoder + spawner; rename existing AI scheduler.
- **[02_STUDIO_ENGINE](02_STUDIO_ENGINE.md)** — bake tool, Chunk Editor panel, Multiplayer-Studio integration.
- **[03_MULTIPLAYER](03_MULTIPLAYER.md)** — `RequestStreamAround` RPC, server dispatcher, AOI unification.
- **[04_ASSET_PIPELINE](04_ASSET_PIPELINE.md)** — LOD variant gen on mesh import; `.echk` content-hash in manifest.
- **[10_TELEMETRY](10_TELEMETRY.md)** — chunk-lifecycle events for monitoring + Workshop AI context.
- **[11_SIMULATION_DEBUGGER](11_SIMULATION_DEBUGGER.md)** — deterministic terrain for replay.
- **[12_INFRASTRUCTURE](12_INFRASTRUCTURE.md)** — server I/O budgeting + R2 layout for chunks.

---

## Open questions

- Q5.1 2D or 3D chunk grid by default? Underground / orbital worlds need 3D.
- Q5.2 LZ4 vs. Zstd default? LZ4 faster decode, Zstd 30% smaller.
- Q5.3 Chunk-size default? 256 m is the agent recommendation; smaller for dense interiors?
- Q5.4 Replicated state on chunks: when a player builds, does the chunk become dirty-locally or dirty-globally?
- Q5.5 Client-side cold cache across launches — yes (faster cold start) or no (always trust server)?
- Q5.6 Terrain on hold or in-scope for 1.0?
