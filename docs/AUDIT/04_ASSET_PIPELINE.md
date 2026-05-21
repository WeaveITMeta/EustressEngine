# 04 — Asset Pipeline

> Creator → storage → cache → runtime. Covers meshes, textures, materials, audio,
> video, documents, scripts, bytecode, animation. **The `.eustress` world container
> is the published format** (a directory holding the Fjall WorldDb database + baked
> `.echk` chunks + manifest + human-editable schema; was `.pak` zstd-tar pre-2026-05-16);
> R2 via Cloudflare Worker (`api.eustress.dev`) is production.

## Pass changelog

- **P1 (2026-05-14):** 11 feature rows; 55 R + 41 I + 15 wiring gaps.
- **P2 (2026-05-14):** + Feature 0 `.pak` publish/server-load flow (verified). R2 tier state corrected ✅ (Worker fronts it).
- **P3 (2026-05-14):** + 12 missing-feature rows (FBX, collider gen, compression, mipmaps, LOD pre-bake, linting, license + AI-consent tags, physics-material, bone/skeleton).
- **P4 (2026-05-14):** **Full retrofit to per-feature-card format (R/I/X/M).** 24 cards. Addendum blocks removed.
- **Updated 2026-05-16: storage pivot.** Published payload is now the `.eustress` world container (Fjall WorldDb database + baked `.echk` chunks + `manifest.toml` + human-editable `schema/`/service folders), not a `.pak` zstd-tar of per-instance TOML. The publish/upload + server-download mechanism is **unchanged** — only the archived contents changed. `.pak`→`.eustress` corrections throughout (header, invariants, snapshot, Feature 0, C3/C11 footers). The `.eustress` references previously flagged as "false" in `scene_loader.rs` etc. are no longer false — `.eustress` is the canonical world-container directory. See MASTER C17.

---

## Concept summary

The Asset Pipeline owns every byte that isn't an instance TOML or a SoulScript source. Creator filesystem → optional AI generation ([07_AI_PLATFORM] Feature 9) → one or more hosts (R2 default / IPFS / S3 / local) → runtime caches → GPU memory.

Invariants:
1. **Content-addressed** — every asset has Blake3 (published world container; `.echk` chunks use deterministic per-chunk blake3 for delta upload) or SHA256/xxhash64 (cache); hash is primary key.
2. **Hot-reloadable** — edit a `.mat.toml` or `.png` in any DCC; engine reloads.
3. **Tier-fallback** — local → R2 → IPFS gateway → P2P → generate-on-demand.
4. **Consent-gated (C8)** — only `ai = true` entities flow into training.
5. **C11** — the `.eustress` world container is the only published format (Fjall WorldDb + baked `.echk` chunks; was `.pak` pre-2026-05-16); **Client never downloads** (server does).
6. **C16** — every texture / mesh has a per-platform variant (BC7 desktop, ASTC mobile).

---

## Implementation snapshot

- **Crates:** [eustress-texture-gen](../../eustress/crates/texture-gen/), [eustress-cad](../../eustress/crates/cad/) *(detail → [18])*, [eustress-mesh-edit](../../eustress/crates/mesh-edit/) *(detail → [18])*, [eustress-forge](../../eustress/crates/forge/), [eustress-forge-sdk](../../eustress/crates/forge-sdk/), [eustress-common::assets](../../eustress/crates/common/src/assets/), [eustress-common::material](../../eustress/crates/common/src/material/), [eustress-common::mesh](../../eustress/crates/common/src/mesh/), [eustress-backend](../../eustress/crates/backend/)
- **Publish flow:** [Studio publish](../../eustress/crates/engine/src/ui/file_event_handler.rs) (panel → manifest → blake3 dedup → multipart R2 upload) + [server load](../../eustress/crates/server/src/main.rs) (R2 fetch → zstd decode → tar extract → open `.eustress` Fjall WorldDb). *Updated 2026-05-16: archived payload is the `.eustress` world container (Fjall database + baked `.echk` chunks via `eustress-worlddb` `bake_to_echk`), not `.pak` per-instance TOML; mechanism unchanged.*
- **R2 hosting:** Cloudflare Worker at `api.eustress.dev` fronts R2; secrets `R2_ACCESS_KEY` / `R2_SECRET_KEY` / `CF_ACCOUNT_ID` in GitHub Actions.

---

## Top-of-doc feature index

| # | Feature | State |
| ---: | --- | :-: |
| 0 | `.eustress` world-container publish + server-load flow *(P2 add; payload pivoted 2026-05-16)* | ✅ |
| 1 | Content-addressed identity | ✅ |
| 2 | Local-filesystem tier | ✅ |
| 3 | S3-compatible tier (incl. MinIO) | ✅ |
| 4 | Cloudflare R2 tier (via Worker) | ✅ |
| 5 | IPFS / Pinata tier | 🟡 |
| 6 | P2P / BitTorrent tier | 🟠 |
| 7 | Forge generation orchestrator *(detail → [07_AI](07_AI_PLATFORM.md))* | 🟠 |
| 8 | Texture / mesh AI generators (FLUX / TripoSR / texture-gen) | 🟡 |
| 9 | Material registry + hot-reload | 🟡 |
| 10 | Persistent cache + invalidation | 🟠 |
| 11 | Audio / video / animation loaders | 🟠 |
| 12 | GLB/GLTF direct import *(P3 add)* | ✅ |
| 13 | Draco mesh decompression *(P3)* | 🟡 wired-but-unused |
| 14 | FBX import *(P3)* | 🔴 |
| 15 | Animation import → `AnimationClip` *(P3)* | 🟡 |
| 16 | Rigged / skinned mesh import *(P3)* | 🟡 |
| 17 | Collider / hull generation *(P3)* | 🔴 |
| 18 | Texture compression (BC7 / ASTC / Basis) *(P3, C16)* | 🔴 |
| 19 | Mipmap pre-bake (offline) *(P3)* | 🔴 |
| 20 | LOD pre-bake tooling *(P3)* | 🔴 |
| 21 | Asset linting / validation *(P3)* | 🔴 |
| 22 | License + AI-consent tagging in TOML *(P3, C8)* | 🔴 |
| 23 | Physics material assignment *(P3)* | 🔴 |

---

## Per-feature cards

### Feature 0 — `.eustress` world-container publish + server-load flow  *(P2 add; payload pivoted 2026-05-16)*

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [02_STUDIO], [03_MULTIPLAYER], [04], [05_SPACE_STREAMING], [12_INFRASTRUCTURE], C11, C17
**Sub-features:** publish panel (`publish.slint`) · manifest update (`publish.toml`, `publish-journal.toml`, `sync.toml`) · thumbnail capture (512×288) · zstd-3 tar pack · blake3 dedup hash · `.echk` bake (`eustress-worlddb::bake_to_echk`, deterministic per-chunk blake3) · POST `/api/simulations/publish` · single-PUT ≤100 MB / multipart (95 MB parts) ≥100 MB · server fetch + extract + open Fjall WorldDb

**Concept.** *Updated 2026-05-16: storage pivot.* Studio bakes the Universe into a `.eustress` world container (Fjall WorldDb database + `chunks/*.echk` + `manifest.toml` + human-editable `schema/`/service folders), tar+zstd-packs it, and uploads to R2 via the Cloudflare Worker. Server downloads + extracts + opens the Fjall database on boot. **No client-side download** (Client joins via QUIC). *(Was a `.pak` zstd-tar of per-instance TOML pre-2026-05-16; the publish/upload + server-download mechanism is unchanged — only the archived contents changed.)*

**Forecasted feedback (R)**
- R0.1 Multipart upload progress UI — show on Publish dialog.
- R0.2 Delta updates: the `.echk` bake now does deterministic per-chunk blake3 hashing (`eustress-worlddb`), enabling delta upload at chunk granularity; full-archive delta still absent (mitigated by blake3 dedup + zstd).
- R0.3 R2 bucket layout under Worker is opaque to creators.
- R0.4 Custom domain (`assets.eustress.dev`) requires DNS + R2 public-bucket.
- R0.5 EU + APAC region replication via R2 auto-replication — verify.
- R0.6 *(2026-05-16)* **Two unreconciled chunk formats** — worlddb's `.echk` version-0 container vs. the audit's 56-byte packed-instance wire format ([05] Feature 3). Which is the on-the-wire/on-disk standard is an open convergence decision (see MASTER C17) — **needs a human call; do not assume.**

**Implications (I)**
- *Architectural:* the `.eustress` world container (Fjall WorldDb + baked `.echk` chunks) is the source of truth for published projects; everything else derives. *(Was "`.pak` is the SoT" pre-2026-05-16.)*
- *Cross-system:* C11 dominates — Client never downloads. C17 (WorldDb storage standard) now governs the payload contents.
- *Migration:* `.eustress` references in `scene_loader.rs`, `eustress_format.rs:15`, `START.md:64`, `BUILD_FIX.md` are **no longer false** — `.eustress` is the canonical world-container directory; reconcile any docs still calling it "dead".
- *Operational:* R2 egress is free; CPU + network cost on Worker.
- *Strategic:* publish UX is the creator's daily proof "this works".

**Risks (X)**
- X0.1 Multipart upload mid-failure leaves orphan parts in R2.
- X0.2 Universe size > 95 MB without multipart trigger silently truncates.

**Mitigations (M)**
- M0.1 Worker garbage-collects orphan multipart uploads after 24 h.
- M0.2 Size check before single-PUT; auto-trigger multipart at threshold.

---

### Feature 1 — Content-addressed identity

**State:** ✅ · **Effort:** Done · **Risk:** Med · **Touches:** [04], [08_IDENTITY]
**Sub-features:** SHA256 (cache) · xxhash64 (fast cache) · blake3 (publish dedup) · base58 encoding · integrity verify on load

**Concept.** Every asset referenced by hash. Computed on save; verified on load. Decouples "where" from "what".

**Forecasted feedback (R)**
- R1.1 Mixing SHA256 + xxhash64 across the pipeline; pick one for IDs, the other for cache only.
- R1.2 Hash-on-save must not block main thread — async file pass.
- R1.3 Big-endian / little-endian; pick base58 (already chosen) and lock.
- R1.4 Hash mismatch on download should retry from different tier.
- R1.5 "What stops someone forging?" → hashes are integrity, not authenticity (signing in [08]).

**Implications (I)**
- *Architectural:* hash = join key for AI training, telemetry, leaderboards — stable forever.
- *Cross-system:* Forge can cache by hash globally; same texture 100 projects = 1 stored object.
- *Strategic:* content-addressed is a precondition for any CDN cost story.

**Risks (X)** — X1.1 Algorithm change post-launch invalidates all existing references.

**Mitigations (M)** — M1.1 Document hash algorithm per format; never change.

---

### Feature 2 — Local-filesystem tier

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [04]
**Sub-features:** `AssetSource::Local(PathBuf)` · project-relative paths · symlink follow (cycle-safe) · cache eviction LRU

**Concept.** Solo projects load directly. The "offline mode" of the Client reuses this tier.

**Forecasted feedback (R)** — R2.1 Path normalisation (Windows `\` vs. POSIX `/`); store with `/`. R2.2 Case-sensitivity divergence Linux vs. macOS / Windows. R2.3 Universe-level shared `assets/` for cross-Space reuse.

**Implications (I)** — *Strategic:* solo creators never need a server.

---

### Feature 3 — S3-compatible tier (incl. MinIO)

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [04]
**Sub-features:** `S3Config::from_env()` · `S3Config::minio()` · signed URLs · SSE-S3 / SSE-KMS · cross-region replication

**Concept.** Any S3 endpoint (AWS, MinIO, Backblaze B2). Used on Fly.io with MinIO for testing.

**Forecasted feedback (R)** — R3.1 `aws-sdk-s3` adds binary size; raw HTTP signing alternative. R3.2 Signed URL TTL short; rotate quarterly.

**Implications (I)** — *Strategic:* enterprise / on-prem buyers want their own S3 — broadens TAM.

---

### Feature 4 — Cloudflare R2 tier (via `api.eustress.dev` Worker)

**State:** ✅ · **Effort:** Done · **Risk:** Med · **Touches:** [04], [06_WEBSITE], [12_INFRASTRUCTURE]
**Sub-features:** Worker fronts R2 · `R2_ACCESS_KEY` / `R2_SECRET_KEY` / `CF_ACCOUNT_ID` secrets · bucket `eustress-releases` (engine + experiences) · custom CNAME `releases.eustress.dev` · cache TTL · multipart upload helpers

**Concept.** Eustress's default object store. Zero egress, ~$0.015/GB. Cloudflare Worker proxies — Studio + server never speak directly to R2.

**Forecasted feedback (R)**
- R4.1 Worker is the auth + routing layer; locks operations to a single edge.
- R4.2 Range requests (HTTP 206) for resumable download — verify R2 support.
- R4.3 Bandwidth-only pricing surprises if Worker request count blows up.
- R4.4 EU + APAC latency via R2 auto-replication is opaque — needs tested.

**Implications (I)**
- *Architectural:* R2 + Worker model decouples Eustress from any AWS dependency.
- *Cross-system:* same bucket hosts engine releases + experiences + thumbnails.
- *Operational:* Worker CPU + invocation count is the actual cost line, not storage.
- *Strategic:* validated working path; cleanup of "S3Config::r2() stub" lore in docs.

**Risks (X)** — X4.1 Cloudflare outage = no asset fetches. X4.2 Worker request limit hit at peak.

**Mitigations (M)** — M4.1 Failover to public R2 URL if Worker unreachable. M4.2 Cache aggressively at the edge.

---

### Feature 5 — IPFS / Pinata tier

**State:** 🟡 (gateway read only; no upload) · **Effort:** M · **Risk:** Low · **Touches:** [04]
**Sub-features:** `AssetSource::Ipfs { gateway, cid }` · multi-gateway fallback · CID → hash mapping · Pinata `pin_file()` API

**Concept.** Decentralised hosting. CIDs are content-addressed (compatible). Public gateways + Pinata for pinning.

**Forecasted feedback (R)** — R5.1 Read works; upload doesn't. R5.2 Gateway latency unpredictable (5 s cold). R5.3 Pinata free 1 GB cap.

**Implications (I)** — *Strategic:* opt-in per project; not the default.

---

### Feature 6 — P2P / BitTorrent tier

**State:** 🟠 (types only) · **Effort:** XL · **Risk:** Med · **Touches:** [04]
**Sub-features:** `AssetSource::P2P { info_hash, trackers }` · `ChunkTransferManager` · seeding · NAT traversal (µTP / WebRTC) · swarm health

**Concept.** Players re-seed assets. Pure design today.

**Forecasted feedback (R)** — R6.1 NAT traversal hard. R6.2 Legal: P2P turns every player into a host. R6.3 IPv6 helps but isn't ubiquitous.

**Implications (I)** — *Strategic:* defer until R2 + Pinata solid; P2P is a scale-out optimisation.

---

### Feature 7 — Forge generation orchestrator

**State:** 🟠 · **Effort:** L · **Risk:** Med · **Touches:** [04], [07_AI_PLATFORM], [12_INFRASTRUCTURE]
**Sub-features:** see [07_AI_PLATFORM] Feature 9

**Concept.** Detailed audit in [07_AI_PLATFORM]. Pointer here.

---

### Feature 8 — Texture / mesh AI generators

**State:** 🟡 procedural ✅ / AI 🟠 · **Effort:** L · **Risk:** Med · **Touches:** [04], [07_AI_PLATFORM]
**Sub-features:** procedural texture-gen (8 Perlin FBM presets) ✅ · FLUX (Python stub) · TripoSR (Python stub) · narrative LLM · provenance watermark · style guide embedding

**Concept.** Procedural texture-gen works in code. AI generators (FLUX / TripoSR) are Python-server stubs called from a hardcoded HTTP endpoint.

**Forecasted feedback (R)**
- R8.1 Python servers deploy + manage separately from Forge — contract.
- R8.2 Procedural fallback (when AI unreachable) — good!
- R8.3 Quality control: generated meshes may be non-manifold; heal via [18_CAD].
- R8.4 Watermarking / provenance — AI-gen assets must tag in TOML (C8).
- R8.5 Style consistency across project — reuse a "project style guide" embedding.

**Implications (I)**
- *Architectural:* one HTTP contract; the impl can swap (Python → Rust → cloud API).
- *Cross-system:* `texture-gen` ([04]) procedural + [07] AI gen share cache.
- *Migration:* model upgrades invalidate cache → key includes `model_signature`.
- *Operational:* generation cost scales fast; budget surface.
- *Strategic:* AI-gen quality is a marketing claim; under-deliver = backlash.

**Risks (X)** — X8.1 Generated content licence ambiguity (who owns?).

**Mitigations (M)** — M8.1 ToS-clear creator-owns; record provenance.

---

### Feature 9 — Material registry + hot-reload

**State:** ✅ load / 🟡 edit · **Effort:** L · **Risk:** Med · **Touches:** [02_STUDIO], [04]
**Sub-features:** `.mat.toml` parser · `MaterialRegistry` resource · Bevy `Handle<StandardMaterial>` · hot-reload via notify-rs · MaterialRegistryTracker (`!=` fix) · reactive state (collision events) · splatmap blending · multi-texture variants

**Concept.** `.mat.toml` parsed → handles cached → hot-reload re-parses. Properties-panel UI is the next gap.

**Forecasted feedback (R)**
- R9.1 Phase-2 Properties-panel material editor unwired.
- R9.2 `MaterialVariant` (beyond named enum) absent.
- R9.3 Splatmap multi-material per mesh designed; no shader.
- R9.4 Reactive state (temperature / wear / impact) in data model; not updated from collisions.
- R9.5 Texture variants per-platform (BC7 vs. ASTC) selection at load (C16).
- R9.6 Cross-Space materials (shared library) at Universe level.

**Implications (I)**
- *Architectural:* material editing is a daily workflow; UI parity with Properties matters.
- *Cross-system:* reactive materials = differentiator vs. Roblox.
- *Operational:* hot-reload is the gold-standard test that human-editable on-disk assets still drive the engine post-pivot (raw assets live in the WorldDb `tree` partition / human-editable folders, not the typed `entities` partition; was "the FS-first invariant" pre-2026-05-16).
- *Strategic:* reactive materials need physics-event tap.

**Risks (X)** — X9.1 Mid-edit reload while gizmo moving → visual flash.

**Mitigations (M)** — M9.1 Defer reload during interactive drag.

---

### Feature 10 — Persistent cache + invalidation

**State:** 🟠 · **Effort:** M · **Risk:** Med · **Touches:** [04], [16_PERSISTENCE]
**Sub-features:** `.eustress/cache/manifest.json` · mtime + hash fallback · size-bounded LRU · engine-version in cache key · cross-process file lock · cache portability

**Concept.** Today's cache is in-memory only; restart loses everything. Disk-backed cache index keyed by source hash. Mtime first, hash on mtime change, regenerate on miss.

**Forecasted feedback (R)** — R10.1 In-memory only. R10.2 Size-bounded LRU eviction. R10.3 Cache key includes engine version (wgsl shader change invalidates). R10.4 Concurrent process (Studio + Client same project) needs file lock. R10.5 Cache portability across machines.

**Implications (I)** — *Operational:* until this lands, every launch re-bakes.

**Risks (X)** — X10.1 Stale cache after engine update produces wrong textures.

**Mitigations (M)** — M10.1 Cache key includes `(engine_version, model_signature)`.

---

### Feature 11 — Audio / video / animation loaders

**State:** 🟠 · **Effort:** L · **Risk:** Med · **Touches:** [04], [01_CLIENT]
**Sub-features:** OGG / MP3 audio (Bevy native; Sound instance class wiring) · MP4 / WebM video (gstreamer / ffmpeg) · GLB-animation (Bevy native; Animation instance wiring) · KeyframeSequence asset

**Concept.** Three asset classes declared in taxonomy; loaders missing from instance side.

**Forecasted feedback (R)** — R11.1 `bevy_audio` enabled (memory); no `Sound` component spawning path. R11.2 Video: heavy deps (gstreamer / ffmpeg-next). R11.3 Animation: GLB import is Bevy-native; `Animation` instance class not wired. R11.4 Mobile codec licensing (MP4 H.264) — prefer royalty-free.

**Implications (I)** — *Strategic:* sound is foundational for game feel; ship before video. Animation parity is critical-path for character projects.

---

### Feature 12 — GLB/GLTF direct import  *(P3 add)*

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [04]
**Sub-features:** Bevy native loader · `space/instance_loader.rs` custom_mesh path · scene reference (`Model.glb#Scene0`)

**Concept.** Bevy-native; already wired in instance_loader. Was not previously called out in P1 audit; rectified.

**Forecasted feedback (R)** — R12.1 No streaming variant for huge GLBs (>500 MB).

**Implications (I)** — *Cross-system:* GLBs come from any DCC; preserve.

---

### Feature 13 — Draco mesh decompression  *(P3)*

**State:** 🟡 wired-but-unused · **Effort:** S · **Risk:** Low · **Touches:** [04]
**Sub-features:** `space/draco_decoder.rs` exists · not called from `instance_loader.rs`

**Concept.** Compressed GLBs (Draco) are widespread; the decoder is in the repo but never invoked.

**Forecasted feedback (R)** — R13.1 Wire into glTF pipeline.

**Implications (I)** — *Operational:* Draco-compressed GLBs are 5–10× smaller; faster cold load.

---

### Feature 14 — FBX import  *(P3)*

**State:** 🔴 · **Effort:** L · **Risk:** Low · **Touches:** [04]
**Sub-features:** Autodesk FBX SDK or open-source (`fbxcel`) · animation tracks · skeletons · materials

**Concept.** FBX is dominant in game art pipelines; absent.

**Forecasted feedback (R)** — R14.1 Roblox-parity expectation. R14.2 Autodesk SDK licensing.

**Implications (I)** — *Strategic:* without FBX, character/animation imports require Blender → GLB conversion.

---

### Feature 15 — Animation import → `AnimationClip`  *(P3)*

**State:** 🟡 · **Effort:** M · **Risk:** Low · **Touches:** [04], [01_CLIENT]
**Sub-features:** `animation_plugin.rs` auto-discovers AnimationPlayer on spawned GLTF · `Animation` instance-class wiring to playback missing

**Concept.** Bevy auto-imports GLB animation tracks. The `Animation` instance class in the Eustress taxonomy isn't wired to playback.

**Forecasted feedback (R)** — R15.1 `KeyframeSequence` API surface (Roblox-parity). R15.2 Animation blending state machine.

**Implications (I)** — *Cross-system:* Character animation ([01] Feature 10) consumer.

---

### Feature 16 — Rigged / skinned mesh import  *(P3)*

**State:** 🟡 · **Effort:** M · **Risk:** Low · **Touches:** [04]
**Sub-features:** `skinned_character.rs` foot IK + layered animation · explicit bone / skeleton import documented · Motor6D joint mapping

**Concept.** Skinned characters work via `skinned_character.rs`; explicit bone/skeleton import path not documented for content creators.

**Forecasted feedback (R)** — R16.1 Bone naming convention. R16.2 Motor6D auto-attach.

**Implications (I)** — *Strategic:* rigged characters = humanoid + creature support.

---

### Feature 17 — Collider / hull generation  *(P3)*

**State:** 🔴 · **Effort:** M · **Risk:** Low · **Touches:** [04], [11_SIMULATION]
**Sub-features:** convex hull · mesh-collider · simplified colliders (capsule, sphere) · physics-material binding

**Concept.** No mesh-to-collider codepath; Avian colliders need explicit construction. Affects every asset's collision behaviour.

**Forecasted feedback (R)** — R17.1 V-HACD or similar for concave shapes. R17.2 Per-asset collider hint in TOML.

**Implications (I)** — *Cross-system:* [11_SIMULATION] physics realism.

---

### Feature 18 — Texture compression (BC7 / ASTC / Basis)  *(P3, C16)*

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [04], [01_CLIENT], [15_MOBILE], C16
**Sub-features:** BC7 desktop · ASTC mobile · Basis Universal · per-platform variant selection · transcoder at runtime

**Concept.** C16: every texture has a canonical source + per-platform bake. Manifest tags variants; Client resolves by `(platform, gpu-feature)`.

**Forecasted feedback (R)** — R18.1 Bake matrix in CI (~3× variants per texture). R18.2 Basis transcoder runtime cost. R18.3 Multi-variant manifest schema.

**Implications (I)** — *Architectural:* C16 is the chooser. *Operational:* mobile binary size + VRAM cut significantly.

**Risks (X)** — X18.1 Wrong variant chosen → broken render on platform.

**Mitigations (M)** — M18.1 Conservative fallback to uncompressed PNG.

---

### Feature 19 — Mipmap pre-bake (offline)  *(P3)*

**State:** 🔴 · **Effort:** S · **Risk:** Low · **Touches:** [04]
**Sub-features:** offline mip chain · Mitchell-Netravali filter · sRGB-aware downsample

**Concept.** Bevy makes mips on GPU upload. Pre-baked mips speed cold load + better quality (Mitchell filter vs. GPU box).

**Forecasted feedback (R)** — R19.1 Storage cost ~30% more per texture (worth it).

**Implications (I)** — *Operational:* faster cold load = better first impression.

---

### Feature 20 — LOD pre-bake tooling  *(P3)*

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [04], [05_SPACE_STREAMING]
**Sub-features:** LOD0 (source) → LOD1 / LOD2 / LOD3 auto-decimation · impostor billboard for far · per-mesh config

**Concept.** [05] Feature 8 needs LOD variants. Auto-generate from LOD0; let creators override per-mesh.

**Forecasted feedback (R)** — R20.1 Auto-decimation can break silhouette of hero items. R20.2 Impostor billboards only for ambient.

**Implications (I)** — *Cross-system:* unblocks [05] LOD ladder.

---

### Feature 21 — Asset linting / validation  *(P3)*

**State:** 🔴 · **Effort:** M · **Risk:** Low · **Touches:** [02_STUDIO], [04]
**Sub-features:** degenerate-geometry detector · missing-texture reporter · license-tag presence · integrity (hash match) · poly budget check · NPOT texture warning

**Concept.** Lint pass on import / publish. Reports issues with severity (warning / error / blocker).

**Forecasted feedback (R)** — R21.1 Publish blocked on error-severity findings? R21.2 Auto-fix vs. report only.

**Implications (I)** — *Operational:* prevents broken-asset uploads.

---

### Feature 22 — License + AI-consent tagging in TOML  *(P3, C8)*

**State:** 🔴 · **Effort:** S · **Risk:** Med · **Touches:** [04], [07_AI_PLATFORM], [08_IDENTITY], [09_ECONOMY], C8
**Sub-features:** `license` field in `.mat.toml` / `.glb.toml` · `ai = true|false` consent flag · creator-id binding · revocation propagation

**Concept.** C8 enforcement requires per-asset consent flag. Today no field exists; consent is unenforced.

**Forecasted feedback (R)** — R22.1 Default value (`ai=false`)? R22.2 Revocation: creator flips `ai=false` later — propagate to embedvec / Workshop training.

**Implications (I)**
- *Compliance:* C8 unenforceable without this.
- *Cross-system:* [09_ECONOMY] marketplace listing requires license tag.

**Risks (X)** — X22.1 Default `ai=true` opt-out is GDPR-questionable.

**Mitigations (M)** — M22.1 Default `ai=false`; explicit opt-in.

---

### Feature 23 — Physics material assignment  *(P3)*

**State:** 🔴 · **Effort:** S · **Risk:** Low · **Touches:** [04], [11_SIMULATION], [19_REALISM]
**Sub-features:** density · friction · restitution · per-asset overrides · `PhysicsMaterial` enum from material name

**Concept.** Today material is visual only. Adding physics props (density / friction / restitution) per material unlocks realistic interactions.

**Forecasted feedback (R)** — R23.1 Schema: top-level `physics` block in `.mat.toml`. R23.2 Per-component override.

**Implications (I)** — *Cross-system:* [11_SIMULATION] V-Cell, [19_REALISM] fracture all depend.

---

## Wiring / import gaps

1. R2 multipart progress UI in publish dialog
2. Delta-pack support for incremental updates
3. Pinata upload client
4. P2P transfer manager
5. Generation server Rust HTTP client
6. CSAM / copyright moderation pre-upload
7. Persistent cache manifest (`.eustress/cache/manifest.json`)
8. Material editor panel writeback
9. Discrete-diffusion crate
10. Audio (`Sound`) spawning path
11. Animation (`Animation` / `KeyframeSequence`) instance wiring
12. Bytecode asset format (compiled Rune / Luau)
13. Draco mesh decompression wiring
14. Asset signing / provenance ([08_IDENTITY])
15. Chunk-to-TOML extractor (`tools/echk_extract.rs`)
16. FBX import
17. Collider / hull generator
18. Texture compression (BC7 / ASTC / Basis) bake pipeline + variant chooser
19. Mipmap pre-bake
20. LOD pre-bake tool
21. Asset lint pass
22. `license` + `ai = true` TOML schema
23. Physics-material per-`.mat.toml`

---

## Cross-system dependencies

- **C1 / Units** — mesh AABBs convert via units module on load.
- **C2 / Canonical create** — generated meshes route through `create_instance`.
- **C3 / Single-author storage** — file-system-first was the prior architecture; superseded 2026-05-15 by the Fjall WorldDb store (MASTER C17). Asset hot-reload (editing `.mat.toml`/`.png` on disk) survives the pivot — raw assets live in the WorldDb `tree` partition / human-editable folders, not the typed `entities` partition.
- **C8 / AI consent** — generation respects `ai = true`; tag in TOML (Feature 22).
- **C11 / `.eustress` world container** — publish flow is Feature 0 (payload pivoted to the `.eustress` Fjall WorldDb container 2026-05-16; was `.pak`).
- **C16 / Per-platform variants** — chooser per Feature 18.
- Depends on **[03_MULTIPLAYER]** for replication of asset references; **[05_SPACE_STREAMING]** for `asset.*` stream topics; **[01_CLIENT]** for runtime asset resolver; **[07_AI_PLATFORM]** for generation server.

---

## Open questions

- Q4.1 Default tier order (Local → R2 → IPFS → Generate?).
- Q4.2 R2 retention for free-tier user uploads.
- Q4.3 Who pays for AI generation (creator at publish or player at runtime)?
- Q4.4 Asset signing required at publish or optional?
- Q4.5 Mobile binary size budget — drives texture format choices.
- Q4.6 FBX licensing path (Autodesk SDK / open source)?
- Q4.7 Default `ai = true` policy (opt-in vs. opt-out)?
