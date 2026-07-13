# Eustress Decentralization — Locked Architecture Plan

Companion to `DECENTRALIZATION_QUESTIONNAIRE.md`. Questions 1–21 were answered by McKale
(2026-07-12); questions 22–30 are confirmed by the Moderation Framework Resolutions
(Grok 4.5, founder-directed, 2026-07-12), which also authored the AI Guardian Policy v1.0
(`AI_GUARDIAN_POLICY_v1.0.md`).

---

## 1. Locked decisions

| # | Decision |
|---|----------|
| 1 | All three properties, phased B → C → A. Account recovery = sign-up **email + the original identity TOML** presented together (two-factor recovery through the witness custodian). |
| 2 | **Full genesis** — no interim founder-authority era on mainnet. Conservative-libertarian shape: rules-as-code, no foundation bureaucracy, no protocol tax beyond what the economics already define. |
| 3 | Displaces nothing. Built alongside active work, in a **git worktree**. |
| 4 | Own Rust chain, **evolving `E:\OpenSource\Crypto\Bliss`** (github.com/bliss-foundation/bliss). |
| 5 | Proof-of-authority exists **only on private testnet**; mainnet genesis launches directly as bonded proof-of-stake (see §3, "Full genesis" reconciliation). |
| 6 | Chain stores small facts only — transactions, ownership records, content roots, head pointers, verdicts, storage attestations. Never content bytes. |
| 7 | **iroh** for the content-addressed blob layer (BLAKE3-verified sync over QUIC). |
| 8 | Published artifact = **deterministic canonical snapshot export** of `.eustress` (see §4). Causal op-log deltas later, as bandwidth optimization only. |
| 9 | **No Cloudflare KV authority from day one.** Nothing to demote: published-content storage is greenfield, and per #12 the ledger starts fresh — the Worker simply retires at genesis except the Stripe edge (#17) and an optional dumb cache (#10). |
| 10 | Non-authoritative Cloudflare cache in front of peer fetch is fine; it can vanish with zero data loss. |
| 11 | Version history public for public Spaces. Private Spaces publish encrypted blobs (chain sees ciphertext hash only) and are outside the Judge's reach until made public/discoverable. |
| 12 | **Fresh ledger at genesis.** No import of the Worker-era KV balances. |
| 13 | Emission stays proof-of-contribution: the live tail-emission + self-healing-treasury math becomes the chain's mint rule. Block producers earn through an uptime/production contribution bucket — no block rewards, no mining. |
| 14 | **Storage-as-contribution**: pinned-bytes × proven-uptime is a new contribution bucket paid from existing emission. Publishing costs a flat anti-spam BLS fee. On-chain publishes double as the server-verifiable artifacts the step-4 attestation hardening needed. |
| 15 | **On-chain peer-to-peer BLS transfers.** No new fiat edges, no exchange listings. |
| 16 | KYC-compliant; working with Draper Goren Blockchain Studio (dgb.vc — Tim Draper / Alon Goren early-stage blockchain venture studio). |
| 17 | Tickets stay exactly as they are (Stripe-side, bought-only, firewalled from BLS). |
| 18 | Self-custody keypair in the OS keychain + witness-assisted recovery per #1, opt-out for purists. |
| 19 | Ownership registry with **gift/transfer, no sale mechanism** at v1. |
| 20 | On-chain provenance enum `original \| imported \| derived`; the importer's source-asset knowledge feeds it. Imported Roblox content can be registered as imported, never as authored. |
| 21 | **Pre-listing gate**: content pins freely; public listing/discovery requires a recorded verdict. Legal must-remove list (child sexual abuse material, terror content, DMCA/court orders) is a separate enforced lane all compliant nodes honor. |

### Confirmed (22–30) — per the Moderation Framework Resolutions (2026-07-12)

| # | Confirmed position |
|---|--------------------|
| 22 | **C confirmed** — Grok 4.5 via xAI API now; moderator-node quorum later (pinned open-weights model, e.g. Qwen2.5-72B-Instruct or a Llama-3.1-70B variant, version+hash pinned, same policy prompt, 2-of-3 quorum signatures) slots in by changing `judge.type` only. The verdict record schema is a **day-one chain contract** (§5). Early phase: founder subsidizes or passes a small per-publish moderation fee; a human fallback path always exists. Known costs stand: listing candidates ship to xAI; an xAI outage pauses listing (never pinning). |
| 23 | **Constitution v1.0 WRITTEN** — `AI_GUARDIAN_POLICY_v1.0.md`, hash in §5. Harm-based prohibited categories + explicit satanic/occult/horror/religious-criticism clarification (theme alone never rejects). **Quality lane from answer #11 still open — see §5.** |
| 24 | **Confirmed** — on-chain `appeal` transaction referencing the original `verdict_id` + creator statement; human review (founder first, multi-sig/trusted set later); overturn/uphold recorded as a new verdict record with reviewer metadata; overturns auditable so the community sees the system is correctable. Review trigger: first 50 appeals or 3 months. |
| 25 | **Confirmed** — publish/update-time only. Runtime Guardian (live chat, dynamic in-Space builds) = separate v2+ workstream (light heuristics + user reporting + sweeps); does not block v1. |
| 26 | **Genesis set NAMED** (§6): `eustress-genesis-alpha-01` (Tucson primary), `eustress-genesis-ua-tech-01` (UA Tech Parks / Chamber partner machine or hosted VPS), `eustress-genesis-community-01` (public opt-in via the existing Full Node toggle; seeded then handed to a trusted early operator), plus a 4th cheap-VPS node if possible. A public genesis manifest documents hardware, location, admin contact, and uptime SLA per node. |
| 27 | **Confirmed** — headless `eustress-node` binary (validator + storage + future moderator module), static binary + config file, optional Docker; `eustress-node start --storage-pledge 200GB`; Prometheus metrics + status page. Honest floors: 4–8 GB RAM, **100 GB minimum usable disk pledge** (500 GB / 2 TB tiers later with proportional rewards), 4+ cores. |
| 28 | **Accepted with amendments**: added criteria — new test entity published end-to-end (local edit → submit → AI verdict on-chain → appears in discovery → fetchable from multiple nodes), appeal flow exercised at least once, every public entity gets a verdict record (Super Station = representative sample + all new publishes). **6-week timebox from kickoff.** Ruthless cuts if at risk: no moderation dashboard UI, no policy-tuning UI, no runtime moderation, no rewards accounting (transfers stay in; storage-reward payouts may slip past MVP). |
| 29 | **Confirmed** — local-first; the chain is a `git remote`, the local `.eustress` is the working copy. Treating the chain as a mounted filesystem is explicitly rejected. |
| 30 | **Confirmed** — Cloudflare Pages indefinitely for the engine web build; future optionality (desktop bundle with embedded node) preserved. |

---

## 2. What exists today (audited 2026-07-12)

**`E:\OpenSource\Crypto\Bliss`** — a real prototype, not vapor (~3,100 lines in the four core
crates, and it has run: `data/chain/{blobs,conf,db}` exists):

- `bliss-chain` — Block (347 ln), Blockchain (338), StateManager (591), RocksDB storage (255).
- `bliss-consensus` — ProofOfContribution producer selection + BlockProducer (552). **Single-producer selection — not Byzantine-fault-tolerant multi-validator consensus.** This is the largest missing piece.
- `bliss-node` — thin main (163). No peer-to-peer replication; QUIC is client→server only.
- `bliss-api` — Quinn/rustls QUIC server, length-prefixed bincode, Ed25519-signed events (819).
- Plus: `bliss-core` (economics/Amount/Equity), `bliss-crypto` (Ed25519, BLS-prefixed addresses), `bliss-events`, `bliss-distribution`, `bliss-wallet(+wasm)`, `bliss-sdk`, `bliss-cli`.
- Deps: tokio, quinn 0.11, rocksdb 0.22. No libp2p/iroh yet.

**EustressEngine side:** `crates/bliss` embeds Light/Full node config + CosignClient; the
tracker→heartbeat→emission loop is live against the Cloudflare Worker; `crates/eustress-space`
already opens/inspects/verifies/exports `.eustress` **without the engine** (the portability
proof); `crates/worlddb` has `header.bin` (EUSWORLD magic, engine + schema versions,
migration registry).

**Gap list to reach the locked design:** multi-validator BFT consensus; peer-to-peer
gossip/replication; iroh blob layer; ownership/verdict/attestation transaction types; the
mint-rule port; deterministic snapshot export; the Judge pipeline; recovery custodian.

---

## 3. "Full genesis" — how answer #2 reconciles with answer #5-A

The proof-of-authority phase is confined to the **private testnet**. Mainnet genesis does not
launch until bonded proof-of-stake is functional AND independent operators are online — then
block 0 is already the real thing. No handover schedule, no interim era where Eustress alone
controls mainnet; the "handover" happens before the network exists publicly.

Governance ethos (conservative-libertarian, as specified):

- Rules-as-code: emission, treasury, storage rewards, bonding — all consensus rules, changed
  only by node-operator adoption of a new version (voting with their software).
- No foundation, no council, no protocol treasury tax beyond the existing self-healing
  treasury math.
- Founder authority is exactly two documents: the Judge's constitution (§5) and the legal
  must-remove list — both versioned, hashed on chain, appealable (constitution) or
  law-bound (removal list).
- Everything else is permissionless: run a node, bond as a validator, pin any blob, publish
  anything (listing is curated; storage is not).

---

## 4. The published artifact — answer to question #8

**Does `.eustress` encode all the TOMLs, and is it reliable storage of a Space?**
Yes as a working store. Making it a *deterministically publishable* store is real Phase-0 work,
now fully specified in `CANONICAL_SNAPSHOT_SPEC.md` (written after a 6-domain code recon that
corrected several earlier assumptions here). Summary:

- A Space is **13 Fjall partitions** inside `world.fjalldb/` (`entities`, `entities_uuid`, `tree`,
  `voxels`, `datasets`, `datastore`, `datastore_ord`, `timeseries`, `mutations`, `meta`,
  `path_to_uuid`, `uuid_to_path`, `class_index`) **plus a disk half** (`header.bin`, `assets/`,
  `Workspace/Terrain/**`, and a documented-but-currently-unwritten `schema/`). Not "entities +
  tree." Full export needs a per-partition include/exclude/normalize policy — the spec's core
  table.
- **Correction (Wave 9.A DID land):** the earlier draft said the voxel store had not landed. It
  has — terrain voxels are a real `voxels` Fjall partition (`iter_all_voxel_chunks` exists). The
  actual terrain problem is *triple* representation (disk `.r16`/`.png` + disk `voxel_chunks/*.bin`
  + the `voxels` partition); the spec declares the partition canonical for migrated Spaces.
- **The existing `eustress-space export` is not the seed** it was assumed to be: it covers 1 of 13
  partitions and names files by *session-local* `Entity::to_bits()`, so byte-identical round-trip
  is structurally impossible on it. The real seed is `worlddb::bake.rs` — a dormant, byte-exact,
  sorted, length-prefixed, BLAKE3-hashed deterministic tree exporter to generalize.
- **Determinism blocker found:** `FjallWorldDb::open()` mutates its own source (stamps
  `schema_version`, checkpoints `tx_counter` on `Drop`) and holds a single-process lock — so a
  naive exporter corrupts its own root and can't run against a live engine. A read-only open is
  Phase-0 prerequisite #0. Other traps the spec neutralizes: session-keyed `entities` rows,
  timestamps/`CreatorStamp` audit chain, `HashMap` `extra` ordering, CRLF/`autocrlf`, NTFS
  case-collisions, and rkyv/lz4/image/compiler version coupling (defended by the verbatim-bytes
  rule).

Net: local `.eustress` = reliable working store; the canonical export (spec'd, not yet built) =
the publishable form. Its BLAKE3 merkle root is the Space's on-chain identity.

---

## 5. The Judge (from answer #11)

The Judge is a **curator, not a censor**. Its constitution encodes:

- **Quality bar** — is it well made? Rejects AI slop and low-effort/childish games. Judges
  craftsmanship, coherence, and whether the work is good for the people who will spend time
  in it. **Evidence-grounded, not self-reported:** the Judge is given actual multi-angle
  renders and a structured scene digest of the Space and must reason over what it actually
  sees — never a text description alone (see "Spatial Intelligence Review" below).
- **Gentlemanly maturity stance** — not prudish; mere mature content is not a violation.
  Judgment is about merit and benefit, not reflexive flagging.
- **Values lane** — the content boundaries McKale authors (the original charge: guarding the
  network's spaces against content he deems corrosive), written as adjudicable criteria.
- **Legal lane** (separate, non-negotiable, applies to *storage*, not just listing): child
  sexual abuse material, terror content, DMCA/court orders → enforced removal list.

Placement keeps the libertarian shape coherent: **pinning is permissionless, listing is
curated.** A rejected Space still exists, is still owned, is still loadable by direct address
— it just isn't surfaced by discovery. Verdicts go on chain; appeals produce on-chain
overturn records.

### Constitution status

**v1.2 is authored and stored**: `docs/architecture/AI_GUARDIAN_POLICY_v1.0.md` (filename
kept stable; internal `Version:` field is the source of truth). v1.0 harm lane authored by
Grok 4.5 under founder direction (2026-07-12); v1.1 quality lane and v1.2 spatial-intelligence
review requirement added same day per the founder's decisions below.
**Policy hash:** `sha256:0710516f8c16aa1145b97ed695959e208bd6c1b3b2b1e42f4180984435ef13bb`
(computed over the stored file; recorded here because a self-referential hash inside the
file would change the hash). Covers: eight harm-based prohibited categories, lean-toward-publish
+ flag-on-true-ambiguity guidance, the satanic/occult/horror/religious-criticism clarification
(theme alone never rejects; the "about vs. manual" test), the spatial intelligence review
requirement (below), the quality gate (below), strict JSON verdict output with mandatory
spatial evidence, and the appeals/false-positive/versioning loop.

### Quality Gate — RESOLVED: Option 1, hard gate

Policy v1.0 was harm-only — it would have published AI slop and low-effort childish games as
long as they were harmless. Answer #11 required the Judge to also curate quality. **Decided:
Option 1** — a second, independent verdict dimension `quality: featured | listed |
rejected_low_effort`. A quality reject blocks public listing exactly like a safety reject —
appealable, criteria-cited, policy-versioned (five narrow reject criteria: mass-produced
filler, non-functional/empty, blatant asset-flip, junk/test content never meant public, and
— added in v1.2 — no discernible spatial/aesthetic intent; `featured` is optional upside, never
required). Pinning stays free either way. Full text in the policy doc's "Quality Gate" section.

### Spatial Intelligence Review — RESOLVED: the Judge must actually see the Space

The founder's follow-up instruction: Grok must read and understand the actual spatial content
of a Space, not review a text description of it — and the network must showcase real value and
real aesthetics, rejecting AI slop and half-baked filler, not merely rubber-stamp anything that
isn't obviously harmful. **Decided and specified** in the policy doc's new "Spatial
Intelligence Review" section:

- **Input, every verdict:** multi-angle renders (overview orbit + close orbits on notable
  clusters) generated **headlessly at publish time**, plus the structured scene digest
  (entity/class histogram, world bounds, hierarchy, material/color variety — the same shape
  `eustress-space`'s `OpenReport` already computes).
- **Mechanism grounding this is REAL today, not proposed:** `ai_camera_capture` /
  `ai_camera_orbit` / `ai_camera_set_pose` already exist in the engine bridge
  (`engine/src/engine_bridge/protocol.rs`) and are proven for interactive AI-driven sessions.
  The gap is running the same primitives **headlessly** at publish time (no human driving the
  session) — a new Phase-4 deliverable (below), not new capability to invent.
- **Grounded rationale, enforced structurally:** the verdict schema gains a required
  `spatial_evidence` array — at least one entry naming something *specific* actually observed.
  An empty or generic entry is a malformed verdict, escalated to `flag_for_human_review`. This
  is what closes the loophole a text-only review can't: a slop-generator can describe its own
  output favorably; it cannot fake composition once the Judge is actually looking at renders.
- **Explicitly NOT yet a dependency:** `SPATIAL_INTELLIGENCE_ARCHITECTURE.md`'s proposed
  semantic spatial-embedding layer (`embedvec`'s Spatial Attention) would add a learned
  similarity signal (e.g. genuine-semantic-closeness duplicate detection) to the §7 dedup/
  triage layer — but `embedvec`'s default embedder is hash-only today (no ML model), so this
  is a documented future enhancement, not required for v1.2's review to function. v1.2 runs on
  renders + the scene digest alone.
- **Scaling interaction (§7):** vision-capable Judge calls cost more per submission than
  text-only calls did — this makes the §7 pre-Judge dedup/triage layer (skip already-judged
  content, cheap pre-filter before the full multi-modal call) more load-bearing, not less.

### Verdict record — day-one chain contract

Fields (per the framework resolutions, harmonized to our stack and the quality-gate + spatial-
review decisions): `verdict_id`, `entity_id`, `content_root` (the Space's **BLAKE3 merkle
root** — content identifiers use `blake3:` prefixes; plain document hashes like the policy file
use `sha256:`), `published_at`, `judge { type: "grok-4.5" | "moderator-node-quorum-v1",
model_identifier, policy_version, policy_hash }`, `verdict: publish | reject |
flag_for_human_review`, `quality: featured | listed | rejected_low_effort`,
`spatial_evidence: [String]` (required, non-empty), `rationale`, `confidence`,
`appeal_status: none | pending | overturned | upheld`, and an **Ed25519** signature over all
fields (bliss-crypto's native scheme). Public listing requires `verdict == publish AND
quality != rejected_low_effort`. Swapping the judge implementation never breaks historical
records.

---

## 6. Phases

**Phase 0 — no-regret groundwork** (worktree `E:\Workspace\EustressEngine-decentral`,
branch `decentral/phase-0`; nothing here depends on chain decisions. Full engineering detail:
`CANONICAL_SNAPSHOT_SPEC.md` §8 build order.)
0. **Safety rails FIRST — DONE (2026-07-12, worktree `decentral/phase-0`).**
   `FjallWorldDb::open_read_only` added: refuses to open (returns `Error::NotAKeyspace`,
   creates nothing) unless the directory already carries Fjall's own keyspace marker;
   `Error::ReadOnlyViolation` guards ALL ~20 mutating trait methods (`apply_commit` and every
   individual `put_*`/`delete_*`/`ds_*` call), not just the batched-commit path; `Drop` skips
   the tx-counter checkpoint and final persist on a read-only handle. `eustress-space`'s
   `resolve_space`/`open_backend` now use it, closing the exact typo'd-path →
   fresh-empty-keyspace → valid-root-over-nothing corruption path the spec identified. 4 new
   regression tests + the full existing 45+7 test suite green
   (`crates/worlddb/src/fjall_backend.rs` `read_only_tests` module).
1. **DONE (2026-07-12).** Additive `worlddb` read surface (spec §4): `iter_entities_uuid`,
   `iter_meta`, `iter_path_to_uuid`/`iter_uuid_to_path`, datastore/timeseries enumerators,
   `rebuild_indexes()` (class_index fully rebuildable from `entities_uuid`; path indexes are
   *repair-only*, documented — see item 2's blocker fix for why that distinction mattered).
   `instance_to_arch` extra-field ordering verified/fixed (see item 2's prereq pass).
2. **DONE (2026-07-12).** Canonical snapshot export/import in `eustress-space`, generalizing
   `worlddb::bake.rs`: per-partition policy (spec §2) + disk half (spec §3) → verbatim-byte
   leaves → BLAKE3 merkle root (`.snap` chunk format, 7 fixed namespaces + a metadata leaf, all
   in `eustress/crates/eustress-space/src/snapshot/`). Three determinism legs (A/B/C) +
   case-collision guard + R3 codec-mismatch guard, all passing. **Prerequisite nondeterminism
   fixes landed first** (spec §8 build order): `InstanceDefinition.extra` → `BTreeMap` (was
   `HashMap`, nondeterministic serialization order), `.gitattributes` (`* -text`) written into
   every Space's autosave repo at git-init time. **Adversarial verify caught a real blocker
   before this was called done**: the importer's `rebuild_indexes()` call does NOT invent
   `path_to_uuid`/`uuid_to_path` from nothing (it only repairs pre-existing partial data) —
   every snapshot-imported Space silently came back with zero path↔UUID mappings. Fixed with
   real content-sourced derivation (`derive_path_uuid_indexes`, walks the imported `tree`
   partition's `[metadata].uuid` fields). Also fixed: datastore plain/ordered shape mixing now
   fails the export loudly (ground-truth key-set cross-check, not a heuristic) instead of
   silently mis-decoding on import; `snapshot-import` now refuses a non-fresh destination
   instead of silently merging into it. **Independently re-verified** (compile + full test
   suite run a second time + the actual fix code read directly), not just trusted from the
   fixing agent's self-report. Final: **100 tests passing, 0 failures.**
   Honestly disclosed, deliberately deferred (not silently skipped): schema materialization
   only includes `schema/` if already on disk (doesn't reach into `common::class_schema_dir()`);
   cross-Universe mesh-reference closure not implemented (a Space with all assets under its own
   `assets/` round-trips correctly, one with `../`-escaping refs does not); the causal op-log
   mutations sidecar hash; Ed25519 signing of the root (needs item 4's identity work first);
   fixture/test coverage is representative, not exhaustive (no dedicated dataset-blob,
   timeseries-row, GUI-TOML, or script-file arm yet).
3. Terrain hardening (spec §5): splat-save counterpart, migrated-Space save gate, declare
   `voxels` canonical + `.bin` policy. (Wave 9.A voxel store already landed — no need to build it.)
4. Identity backup TOML at sign-up (spec §6 signer + identity recon): encrypt the private key
   CLIENT-SIDE before `toml_content` leaves the browser (today it transits the Worker + rests in
   the inbox in plaintext — the "email + TOML two-factor" collapses to one factor without this);
   standardize on `public_key` as the account id (client/server `user_id` split).
5. Bliss workspace on Windows — **compiles now; was fully RED before this session** (`cargo test
   --workspace` never built → 0 tests ran). Fixed: 2 compile blockers (`bliss-node` `ApiState`
   missing fields; `bliss-wallet-wasm` `KeyPair::address` → `Address::from_public_key(pk)`), the
   rustls 0.23 double-`CryptoProvider` panic (pin `ring` in `build_quic_endpoint`), a bare-fence
   doctest. Result: **79 tests pass** (was 0). Six pre-existing failures remain, all Phase-1
   test-harness/determinism items — NOT papered over: (a) five QUIC tests build a live
   `quinn::Endpoint` from plain `#[test]` fns with no tokio runtime (need `#[tokio::test]` or a
   lazy endpoint); (b) `test_tracker_session_lifecycle` — contribution creation keys on wall-clock
   elapsed, not the stated duration (same wall-clock family as the chain-consensus nondeterminism
   below). Optional later: evaluate RocksDB→fjall to unify the storage stack.

**Phase 1 — chain core** (testnet, proof-of-authority allowed here only)
Recon of `E:\OpenSource\Crypto\Bliss` found the chain is **not a trust anchor yet** — these are
prerequisites *before* any multi-validator work:
- **Close the mint holes:** `Mint`/`Distribution`/`ForkBridge` txs are accepted from the public
  mempool with zero signature/authority checks (unlimited public mint via a curl one-liner);
  `add_block` never verifies the producer signature; `ForkBridge` attestations are never verified;
  no emission/supply cap is enforced. Consensus (`ProofOfContribution`) is **dead code** — wholly
  unwired from `add_block`.
- **Remove consensus-visible nondeterminism** (also the BFT blocker): block header hashes an
  `f64` score via `to_le_bytes`; block/tx construction AND validation call `Utc::now()`; tx nonces
  use `rand::random()`. Two honest validators cannot agree on these bytes. Replace with
  fixed-point + proposer-timestamp + caller-supplied nonce. (Same wall-clock family as the
  `bliss-events` tracker failure.)
Then:
1. Multi-validator BFT: integrate a Rust BFT engine (malachite candidate) with
   proof-of-contribution-weighted, BLS-bonded validator selection replacing single-producer;
   replicated validator set + quorum certificates; block-tree storage + fork-choice replacing the
   linear `prev==tip` gate; P2P/mempool transport (`p2p_port` is currently unused).
2. Transaction set: transfers (#15), ownership registry ops (#19/#20), head-pointer updates
   (#11), storage attestations (#14), verdicts + removal-list entries (#21).
3. Mint rule: port the Worker's tail-emission + treasury math into consensus (#13); populate the
   `verified_contributions` map (defined but never written — if wired today it would reject every
   real block).
4. Two-machine testnet producing blocks under validator churn. The Phase-0 canonical snapshot is
   the natural state-sync/genesis artifact.
5. **Batched verdict commitments** (§7 scaling analysis): verdicts commit as a batch under one
   epoch Merkle root rather than one consensus round per verdict; individual verdicts stay
   provable via a Merkle proof against that root. Required before verdict-transaction volume can
   scale past a testnet's handful of publishes.

**Phase 2 — storage network**
1. iroh blob layer in `eustress-node` + engine light node; publish = export → chunk → announce.
2. Replication targeting 8–12 nodes/blob; storage proofs = random BLAKE3-chunk challenges in
   the existing 90-second heartbeat, attested on chain; storage-as-contribution bucket live.
3. Retrieval path: chain address → peer fetch → cold load in the engine (Cloudflare cache
   optional and non-authoritative).

**Phase 3 — ownership + identity UX**
1. Studio publish flow (File → Publish: sign, export, announce, register).
2. Keychain-held keys; email + identity-TOML recovery via witness custodian; opt-out.
3. Importer provenance flags feeding registry records; gift/transfer UI.

**Phase 4 — the Judge**
1. ~~Constitution authored~~ **done** (`AI_GUARDIAN_POLICY_v1.0.md`, v1.2, hash in §5:
   harm lane + quality lane + spatial-intelligence review requirement, all resolved); anchor
   the hash on chain.
2. **Publish-time Spatial Capture** *(new, v1.2 requirement)*: a headless capture step in the
   publish flow that generates the Judge's mandatory input — an overview orbit + close orbits
   on notable clusters via the engine's existing `ai_camera_capture`/`ai_camera_orbit`/
   `ai_camera_set_pose` bridge primitives (`engine/src/engine_bridge/protocol.rs`, proven live
   for interactive sessions; this deliverable is running them headlessly, on the headless
   runtime, with no human driving), plus the structured scene digest (the same fields
   `eustress-space`'s `OpenReport` already computes). Blocks item 3 — the Grok pipeline has
   nothing to send without this.
3. Grok 4.5 verdict pipeline: policy file as system prompt + the renders/digest from item 2 as
   multi-modal input, strict-JSON output (`verdict` + `quality` + required `spatial_evidence`),
   verdict record (§5 contract) written on chain; pre-listing gate in discovery.
4. Appeal transaction + overturn records; removal-list enforcement in node software.
5. **Pre-Judge triage layer** (§7 scaling analysis): content-hash dedup against already-judged
   BLAKE3 roots (never re-judge unchanged content); a cheap heuristic/classifier pre-filter so
   only genuinely borderline submissions reach the full multi-modal LLM call. More load-bearing
   now that Judge calls are vision calls, not text-only (§5 spatial-review scaling note) —
   needed before publish volume makes per-submission Grok calls the binding cost/latency
   constraint.

**MVP gate (testnet, end of Phase 4)** — all must pass (#28 as amended):
- 3 genesis-named nodes live across ≥2 physical machines / admin domains.
- Super Station (118,974 entities) through the gate (representative sample + all new
  publishes get verdict records).
- Cold-load on a completely clean machine with Cloudflare fully disabled.
- One on-chain BLS-signed transfer exercising the new publish + verdict flow.
- Kill one storage node mid-fetch → zero data loss, automatic recovery from replicas.
- One new test entity end-to-end: local edit → submit → AI verdict on-chain → appears in
  discovery → fetchable from multiple nodes.
- Appeal flow exercised at least once (manual human step acceptable).

**Timebox: 6 weeks from kickoff — kicked off 2026-07-12, MVP target 2026-08-23.** If any
criterion is at risk, cut polish (dashboards, tuning UI, rewards accounting) and ship the
core loop.

**Mainnet genesis gate:** bonded proof-of-stake live; ≥7 independent validators and ≥20
independent storage nodes recruited (thresholds tunable — McKale sets final numbers; the
named 3–4 genesis nodes are the testnet/MVP floor, not the mainnet bar); constitution
published + hash anchored; fresh ledger; public genesis manifest (hardware, location, admin
contact, uptime SLA per node); Worker retires to Stripe-edge + cache duty. Recruitment
funnel: the existing Full Node toggle becomes a pre-genesis waitlist with honest hardware
disclosure (4–8 GB RAM, 100 GB pledge floor).

---

## 7. Scaling to millions — today's ceiling and the growth path

The v1 design in this plan is an honest small-scale beta (MVP gate = 3 nodes, a handful of
test publishes). It does **not** scale to millions as specified. Three axes, three different
answers:

**Accounts/identity — trivial, no bottleneck.** Each account is a keypair; there's no shared
resource per-user. Millions of accounts is a non-issue at any point in this design.

**Storage — scales with content BYTES, not user count, and the design is already shaped
correctly.** iroh's content-addressed P2P model means capacity grows by adding storage nodes,
roughly linearly with total unique published bytes (not with head count — most users share
most of the moderation/chain infrastructure). The real gap today isn't the design, it's
**recruitment**: 3–4 named genesis nodes and a ≥20-node mainnet floor are nowhere near
web-scale capacity. That's an ops/growth problem the storage-as-contribution economics (§14)
are built to solve — more nodes get paid more as content volume grows — not an architecture
problem.

**The chain and the Judge are the real bottlenecks, and neither is scale-ready yet:**

- **Chain:** the recon found the Bliss chain is currently a **strictly linear, single-producer
  chain** (no P2P, no batching, no BFT) — see §2. A single producer has a hard TPS ceiling
  (block time × tx-per-block). Millions of publishes/transfers/verdicts per day plausibly need
  sustained tens-to-hundreds of TPS; today's implementation can't sustain that regardless of
  hardware. Phase 1's multi-validator BFT is necessary but not sufficient by itself — batching
  matters too: writing one verdict transaction per publish, one at a time, doesn't scale the
  way committing a **batch of verdicts under one epoch Merkle root** does (individual verdicts
  stay reconstructible/provable via a Merkle proof against that root, without each one being a
  separate consensus round). This batching design isn't in the plan yet — add it to Phase 1.

- **The Judge (Grok 4.5 via a single xAI API key) is the most acute constraint at "millions"
  scale**, for reasons independent of raw throughput:
  - **Single point of failure at global scale.** One outage or policy change at xAI halts
    *all* new listings platform-wide (pinning is unaffected). The bigger the network, the worse
    that optically and practically gets.
    - **Cost scales roughly linearly with publish volume.** The framework's own mitigation
    ("founder subsidizes or passes a small per-publish fee") is honest that this is founder-
    funded exposure today, not a solved problem — nobody has estimated what "millions of
    verdicts/month" actually costs, and it should be modeled before it's load-bearing.
  - **Rate limits on one API key** cap concurrent verdict throughput regardless of willingness
    to pay.

  **The plan already has the right long-term answer (§22): the Q2 migration to a
  moderator-node quorum** — a pinned open-weights model running ON the same storage/validator
  nodes, so moderation compute scales *horizontally with node count* instead of being capped by
  one API key. That migration is scheduled but not built; at real scale it's not optional, it's
  load-bearing.

  **Three cheap efficiency wins to add regardless of when the quorum migration lands:**
  1. **Content-hash dedup before invoking the Judge at all** — if a BLAKE3 root has already
     received a verdict, never re-judge it (catches re-publishes, forks of unchanged content,
     and republishing after a rejected edit is fixed elsewhere).
  2. **Cheap pre-filter triage** — a fast heuristic/classifier handles the obviously-fine bulk
     of submissions; only genuinely borderline content escalates to a full LLM call. Reduces
     the volume that ever needs the expensive path, not just the cost per call.
  3. **Batch multiple pending Spaces per LLM call** where the content fits, amortizing
     call overhead.

**Bottom line:** the design is *shaped* to reach millions (content-addressed storage, an
already-scheduled moderator quorum, an economics model that pays for more nodes as volume
grows) — but two specific pieces of engineering are missing and neither is in the current
Phase 1/4 task lists: **batched verdict commitments on the chain**, and **the actual triage/
dedup layer in front of the Judge**. Both should be added as explicit Phase 1/4 deliverables,
not assumed to fall out of "BFT" and "moderator quorum" on their own.

---

## 8. Open items on McKale's desk

1. Genesis operator commitments: UA Tech Parks / Chamber partner for
   `eustress-genesis-ua-tech-01`; trusted community operator for
   `eustress-genesis-community-01`.
2. Final **mainnet** thresholds (≥7 validators / ≥20 storage nodes / sustained days —
   suggested, not locked).
3. Review/edit the stored policy text itself (`AI_GUARDIAN_POLICY_v1.0.md`); any edit bumps
   the version and re-hashes.

(Quality lane — RESOLVED §5: Option 1, hard gate.)
