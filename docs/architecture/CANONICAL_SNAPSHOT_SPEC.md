# Canonical Snapshot Spec — the publishable identity of a `.eustress` Space

Status: DRAFT v0.1 (2026-07-12), grounded in the Phase-0 recon of `worlddb`, `eustress-space`,
terrain, the Space directory, and identity. This is the "write the spec first" artifact: every
Phase-0 code decision derives from the per-partition policy table below. Companion to
`DECENTRALIZATION_PLAN.md` (§4, §6 Phase 0).

The snapshot is the deterministic, content-addressed serialization of one Space. Its BLAKE3
merkle root is the Space's on-chain identity, the thing the AI Guardian signs a verdict over,
and the thing a cold-fetching client re-hashes to verify. If two machines with the same logical
Space produce different roots, the whole decentralization layer breaks (dedup, moderation
caching, ownership proofs). So determinism is not a nice-to-have; it is the entire contract.

---

## 0. What the recon changed vs. the plan's earlier assumptions

1. **The Wave 9.A voxel store HAS landed.** Terrain voxels live in a real Fjall `voxels`
   partition for migrated Spaces (`worlddb::iter_all_voxel_chunks` exists; the plan's "Gap 1 =
   voxels not in DB" was wrong). The real terrain problem is a *triple* representation (disk
   `.r16`/`.png` heightfield + disk `voxel_chunks/*.bin` + the `voxels` partition) with no
   declared single source of truth.
2. **A Space is 13 Fjall partitions + a disk half, not "entities + tree."** Full content export
   needs a per-partition policy (this doc), not a single iterator.
3. **The existing `eustress-space export` covers 1 of 13 partitions** (Morton `INSTANCE_CORE`
   rows) and names files by *session-local* `Entity::to_bits()`. Byte-identical round-trip is
   structurally impossible on that projection. **Phase 0 is a NEW artifact, not an extension of
   the TOML-tree export.**
4. **`worlddb::bake.rs` already exists** — a dormant, byte-exact, sorted, length-prefixed,
   BLAKE3-hashed deterministic exporter for the tree partition (the `.echk` format). It is the
   skeleton to generalize, not to reinvent.
5. **`FjallWorldDb::open()` mutates its source** (stamps `meta:schema_version`, checkpoints
   `tx_counter` on `Drop`) and Fjall holds a single-process lock. A snapshot tool that opens the
   normal way corrupts its own determinism and cannot run against a live engine. **Read-only open
   is prerequisite #0.**

---

## 1. Hard rules (non-negotiable invariants)

- **R1 — Logical, never physical.** Never hash `world.fjalldb/` directory bytes. LSM
  journals/compaction/segment layout differ across machines and runs. Serialize logical
  key→value rows in sorted key order through the trait/backend only.
- **R2 — Verbatim value bytes.** Copy stored value bytes as-is (rkyv cores, LZ4 voxel blobs,
  Parquet dataset blobs, PNG splatmaps). Never decode-and-re-encode in the hash path. This is the
  single defense against rkyv-layout / lz4_flex / image-crate / compiler-version drift breaking
  the whole fleet's roots on a toolchain bump. Any future "repack" feature hashes *decoded*
  content on a separate path, never the canonical one.
- **R3 — Pin and record codec versions.** The snapshot manifest records the versions of every
  crate whose byte output it copies verbatim (`rkyv`, `lz4_flex`, `image`, `toml`, `blake3`,
  Fjall key-schema versions). A mismatch on import is a hard, loud error — not a silent
  divergence.
- **R4 — Read-only source.** Export opens the DB read-only: no `schema_version` stamp, no
  `tx_counter` checkpoint on `Drop`, fail-if-absent (never create). Export twice on one handle →
  identical bytes.
- **R5 — Explicit include list, never a directory walk.** Three disagreeing skip lists exist in
  the tree today (`file_loader.rs:551`, `package_universe_to_pak:931`, autosave gitignore). The
  snapshot uses one manifest-driven include list. Anything not on it is excluded by definition.
- **R6 — Canonical ordering is key-lexicographic, pinned to `FjallWorldDb`.** Never trust `dyn
  WorldDb` iteration order (Fjall = lexicographic, MemDb = HashMap, BranchHandle = BTreeMap). The
  exporter either binds the concrete `FjallWorldDb` or sorts explicitly. Entity ordering is by
  **UUID / encoded key**, never by session `Entity::to_bits()` or Morton-of-position.
- **R7 — Determinism is a property of the export pipeline at a save boundary.** Audit content
  (timestamps, `CreatorStamp` chain) is *real content* and is kept, not stripped. "Deterministic"
  means: `export(x) == export(x)` and `export(import(export(x))) == export(x)` — NOT
  "export is stable across an intervening resave." Import must write snapshot bytes back verbatim
  (no UUID minting, no header re-defaulting, no `CreatorStamp` append) or R7's second equality
  fails by construction.

---

## 2. Per-partition policy table (the core of the spec)

For each of the 13 Fjall partitions, the disposition is INCLUDE (hashed into the root),
EXCLUDE (session/derivable state), or NORMALIZE (transform before hashing).

| Partition | Disposition | Canonical order | Notes / rationale |
|-----------|-------------|-----------------|-------------------|
| `tree` | **INCLUDE** | rel-path lexicographic | The disk-superset human tree after the on-open reconcile. Byte-faithful file mirror. Strip `#bin` bincode caches (derivable). |
| `entities_uuid` | **INCLUDE** | 16-byte UUID order | The **authoritative, portable** entity store (rkyv `ArchInstanceCore` keyed by durable UUID). This — not the session-keyed `entities` partition — is the canonical entity set. |
| `path_to_uuid` | **NORMALIZE** | path lexicographic | Re-derivable from `entities_uuid` + tree; include only if cheaper to carry than rebuild. Decision: **re-derive on import**, exclude from root. |
| `uuid_to_path` | **NORMALIZE** | UUID order | Same as above — re-derive on import, exclude from root. |
| `class_index` | **NORMALIZE** | key order | `{class}\x1f{uuid}` → empty value; pure index. Re-derive on import, exclude from root. |
| `voxels` | **INCLUDE** | encoded-key (Morton) order | Terrain canonical arm for migrated Spaces. Verbatim LZ4 bytes. **Normalize empty-byte tombstones to absent** (branch merge writes `b""` for deleted chunks; a naive export would hash a row the twin Space lacks). |
| `datasets` | **INCLUDE** | 16-byte id order | Verbatim Parquet blobs. Same empty-byte tombstone normalization. |
| `datastore` | **INCLUDE** | key order | Two value shapes in one partition (plain raw bytes vs `[sort_be8][value]` for ordered stores) with no in-band discriminator — carry verbatim; the shape is implied by the store's ordered-ness, recorded in a per-store manifest entry. |
| `datastore_ord` | **EXCLUDE** | — | Derived index of `datastore`. Re-derive on import. |
| `timeseries` | **INCLUDE** | `(series, ts, seq)` order | Verbatim rows. Requires a new `list_series()` enumerator (see §4). |
| `mutations` | **EXCLUDE from content root; optional sidecar** | seq order | The causal op-log is *history*, not *content*. Embeds wall-clock `ts_nanos` and grows per session. Hash it as a separate, optional, non-root section if provenance replication is wanted later. |
| `entities` | **EXCLUDE** | — | Session-keyed flat `TRANSFORM` rows + Morton `INSTANCE_CORE` rows, both keyed on `Entity::to_bits()`, written by a per-frame budget-limited mirror. Content is a function of *runtime history*, not world state. The authoritative entity set is `entities_uuid`. |
| `meta` | **EXCLUDE** | — | `tx_counter` / `mutation_seq` / `migration_checkpoint` = session state, rewritten every commit. The *content-relevant* meta (`schema_version`) is carried in the snapshot manifest, not hashed from this partition. |

**Despawn-leak guard:** `FjallWorldDb::despawn` only tombstones 7 hardcoded component ids, so
rows under any other `ComponentTypeId` (and the Morton `INSTANCE_CORE` row) can survive as stale
bytes. Since `entities` is EXCLUDED and the canonical entity set is `entities_uuid`, this leak
does not reach the root — but the importer must not resurrect the `entities` partition from the
snapshot.

---

## 3. The disk half (the `.eustress` container beyond the DB)

Same sorted-`(rel_path, bytes)`-leaf treatment as `tree` rows, one include list:

**INCLUDE:**
- `header.bin` — **NORMALIZED**: carry `world_id` + `world_schema_version` + `header_format_version`
  + the `migrated_at` presence marker (it selects which terrain loader runs on the destination).
  Strip volatile fields into a non-hashed sidecar: `engine.written_at`, `engine.commit`,
  `engine.semver`, `migrated_to_uuid_at` timestamp.
- `Workspace/Terrain/**` — the entire heightfield sidecar set as opaque path-sorted bytes:
  `_terrain.toml`, `chunks/*.r16`, `splatmap/*.png`, `materials/*.mat.toml`. (Write-once
  deterministic artifacts.) For **migrated** Spaces the `voxels` partition is canonical and
  `voxel_chunks/*.bin` is a DERIVED mirror — see §5.
- `assets/**` at the Space root + **referenced Universe-level `.eustress/assets/{parts,meshes}`
  blobs**, content-addressed and deduped by hash. Closes the cross-root escape where
  `'../meshes/*.glb'` refs resolve *outside* the Space and a Space-only snapshot ships broken mesh
  refs (grey blocks on the fetching machine, plus the author's absolute path leaking into the
  runtime URL via the `strip_prefix` fallback).
- `schema/*.toml` — **materialized into the snapshot** from `common/assets/class_schema` +
  `service_templates`. Today these resolve from compile-time `CARGO_MANIFEST_DIR` with no
  deployed-build fallback, so a clean machine literally cannot interpret a snapshot. Materializing
  them into the snapshot closes the portability hole *and* the deployment hole at once.

**EXCLUDE (session/cache/machine-specific):**
- `world.fjalldb/` raw bytes (R1); `#bin` caches; `.git/`; `chunks/*.echk` bake output (derived);
- `.eustress/{local, trash, output.log, last_reconcile, .last_name}`; `Workspace/.generated/`;
  `*.bak-*`; `~/.eustress_engine/**` (identity/settings/tracker — none live inside a Space anyway);
- `.eustress/asset-index.toml` `source_path` and `sync.toml` `experience_id`/`bucket` — machine/
  cloud-specific; strip or exclude.

---

## 4. Missing `worlddb` read surface (additive trait wave, prerequisite for export)

Full-content export is impossible through today's trait. Add as one additive wave (Fjall-side
each is a trivial full prefix scan; trait defaults return `Err` so test stubs stay compilable,
mirroring the existing voxel/dataset default pattern). `BranchHandle` (the second `WorldDb` impl)
must implement or default each.

- `iter_entities_uuid() -> (uuid16, core_bytes)` in UUID byte order.
- `iter_meta()` — enumerate all meta keys (today only point-get; unknown keys are undiscoverable).
- `list_stores()` / `list_scopes(store)` or `iter_datastore_raw()` — datastore has no enumeration
  (`ds_range` needs a known store+scope).
- `list_series()` or `iter_timeseries_raw()` — timeseries has no enumeration.
- `iter_path_to_uuid()` / `iter_uuid_to_path()` — for the re-derive-on-import validation.
- `rebuild_indexes()` — **documented in `migrate_identity.rs` but does not exist.** Needed so an
  export can assert `entities_uuid` completeness first: the class_index→iter_class→get-core path
  silently drops any core whose index marker was lost (independent writes, crash between them).

Also: verify the engine-side `instance_to_arch` bake produces order-stable `ArchInstanceCore.extra`
(the crate-side `toml→EusValue` path sorts keys — `rkyv_values.rs:174` — but the engine bake was
not readable in recon scope). This is the one remaining place HashMap ordering could leak into
canonical `entities_uuid` bytes. **Must be verified before trusting the root.**

---

## 5. Terrain: resolve the triple-representation now

- **Migrated Space:** the `voxels` Fjall partition is canonical (per `TERRAIN_FJALL_MIGRATION.md`).
  Snapshot the logical `(10-byte key, LZ4 value)` rows in encoded-key order, verbatim. Include
  `header.bin`'s `migrated_at` marker (a Space restored without it re-seeds `voxels` from
  `voxel_chunks/*.bin` on first open — lossless *today* only because no Fjall-side voxel edits
  exist yet). **Policy: DROP `voxel_chunks/*.bin` from the migrated-Space snapshot** and rely on
  re-seed determinism (files stored opaque, identical bytes/keys). Alternative (b): include both
  and assert row-vs-file byte equality at export — doubles as the corruption check
  `TERRAIN_FJALL_MIGRATION.md` §9.D promised. **Recommendation: (a) now, (b) as the 9.D verify
  task.**
- **Legacy (non-migrated) Space:** heightfield sidecars (`.r16`/`.png`/`.mat.toml`) are canonical;
  snapshot the whole `Workspace/Terrain/` subtree opaque.
- **Two known lossy gaps to fix before the clean-machine test** (both cheap pre-Phase-0
  hardening): (1) splat paint is never persisted (`save_chunks_to_disk` has no splat counterpart);
  a round-trip test that compares only `.r16` PASSES while the painted world is wrong — the test
  MUST compare splat + voxel arms. (2) `save_terrain_to_disk` has no migrated-Space gate and writes
  clamped-junk `.r16`s (`height_scale=1.0` clamps everything >1 stud to `u16::MAX`) — gate it on
  `!space_is_migrated`.
- **Golden fixture:** worldgen `export_to_space` is already byte-identical-across-runs tested
  (`export.rs:879`) and cleans stale files — use it as the determinism fixture. Avoid
  heightmap-imported terrain as a fixture until its three portability bugs (unsigned coords,
  `default_space_root`, absolute-path comment) are fixed.

---

## 6. Container format (reuse `bake.rs`, generalize it)

- **Leaf hash recipe** (from `branch.rs` `digest()`): namespace tag byte + length-prefixed key +
  presence flag + length-prefixed value → BLAKE3. One namespace tag per partition + one for disk
  leaves.
- **Chunk container** (from `bake.rs` `.echk`): `magic + u32 version + count + length-prefixed
  sorted entries`, atomic temp+rename write, per-chunk BLAKE3, delta-aware skip. Generalize from
  tree-only to per-partition streams.
- **Merkle root:** content-define chunk boundaries over the concatenated canonical stream; BLAKE3
  merkle over chunk hashes. Root is prefixed `blake3:` on chain (document hashes like the policy
  file use `sha256:`; the two prefixes are the discriminator).
- **Manifest** (hashed as part of the root): snapshot format version, `world_id`, `world_schema_version`,
  the pinned codec versions (R3), the per-store datastore shape flags, the terrain arm in use, and
  the ordered list of chunk hashes.
- **Signature:** detached Ed25519 over the merkle root, using the creator's existing login keypair
  (the same key `do_challenge_auth` loads — factor it into a shared signer, don't re-parse TOML by
  lines). The chain record binds `{root, publisher_pubkey, moderation_status}` and is committed
  **before** replication. Cold-fetch clients re-hash fetched content and verify
  `root == chain-committed root` before load (this verification step exists nowhere today — it is
  what stops the "moderate R1, serve R2" bypass).

---

## 7. The determinism CI gate (three legs + cross-platform)

Fixture Space must contain **every representation arm** — the arms a naive exporter drops
silently: folder-form + flat instances, a custom mesh escaping to Universe assets, a `.mat.toml`,
GUI element TOMLs, scripts (`.luau`/`.soul`/`.rune`), terrain (heights + splat + voxels), a
DB-only binary `INSTANCE_CORE`, a datastore key, a dataset blob, a timeseries row, a
`uuid`-partition entity.

- **Leg A** — export twice on one handle → identical roots. Catches `open()`/`Drop` side effects
  (R4).
- **Leg B** — export → verbatim import into a fresh `.eustress` → re-export → identical roots.
  Catches UUID/timestamp minting on the import path (R7): the importer must NOT call the
  UUID-minting migration or header-defaulting.
- **Leg C** — export → run a session of runtime-only churn (camera orbit, physics settle → the
  `Transform` mirror commits, `tx_counter` bumps) → re-export → **unchanged** root. Proves the
  include/exclude policy (R6: `entities`/`meta` excluded).
- **Cross-platform** — CRLF (global `core.autocrlf=true` rewrites tree TOML on checkout; add
  `.gitattributes * -text` to the autosave repo and treat tree bytes as binary) and NTFS
  case-collision (two tree keys differing only by case import to one file on NTFS — reject
  case-only collisions at export time on Windows).
- Run `worlddb` tests with `--test-threads=1` (Windows keyspace-count limit).

---

## 8. Phase-0 build order (derived from the critique's priority list)

1. **Safety rails (prerequisite #0):** read-only `FjallWorldDb` open (R4); make `resolve_space`
   refuse a directory without Fjall artifacts (kills the "typo'd path → fresh empty keyspace →
   valid root over nothing → published" silent-corruption path). Verify Fjall lock behavior vs a
   live engine.
2. **This spec, ratified.** (You are reading it.)
3. **The additive read-surface wave (§4)** + `rebuild_indexes()` + verify `instance_to_arch` extra
   ordering.
4. **Kill file-level nondeterminism:** `BTreeMap` (sorted) emission for `InstanceDefinition.extra`;
   `.gitattributes` on the autosave repo; tree-key case policy.
5. **Terrain hardening (§5):** splat save counterpart; migrated-Space save gate; declare `voxels`
   canonical + pick the `.bin` policy.
6. **The exporter + importer** in the engine-free `eustress-space` crate (generalize `bake.rs`),
   hooked in-editor at `do_publish` (after `do_save_space` + `db.flush()`), replacing/augmenting
   `package_universe_to_pak`.
7. **The three-legged CI gate (§7)** on the all-arms fixture.
8. **Sign + verify the loop (§6):** detached Ed25519 over the root; chain record before
   replication; client re-hash on fetch.

Items 1, 3, 4, 5 are pure no-regret hardening — correct under every remaining open decision, and
several (splat save, migrated-save gate, `resolve_space` guard, `.gitattributes`) are latent bugs
worth fixing regardless of decentralization.
