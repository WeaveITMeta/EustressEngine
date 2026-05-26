# IDENTITY & UUID-dedup Specification — Wave 1 SPEC

**Status:** Draft, spec only — no code changes in this wave
**Date:** 2026-05-26
**Scope:** Every entity entry surface across EustressEngine (TOML import, live ECS edit, cross-space copy/paste, Roblox import) shall key entity identity by UUID. This document defines the canonical model, the migration plan, and the entry-surface contract that ends the current path-based dedup gap.

---

## 0. Table of contents

1. [Problem — current path-keyed identity](#1-problem--current-path-keyed-identity)
2. [Unified identity model](#2-unified-identity-model)
3. [UUID generation per entry surface](#3-uuid-generation-per-entry-surface)
4. [The four entry-surface contract](#4-the-four-entry-surface-contract)
5. [Fjall layout & reverse indexes](#5-fjall-layout--reverse-indexes)
6. [Migration plan — existing path-keyed stores](#6-migration-plan--existing-path-keyed-stores)
7. [Backward compatibility — TOML schema addition](#7-backward-compatibility--toml-schema-addition)
8. [Conflict resolution](#8-conflict-resolution)
9. [Performance & sizing](#9-performance--sizing)
10. [API surface changes](#10-api-surface-changes)
11. [Cross-system implications](#11-cross-system-implications)
12. [Open questions](#12-open-questions)
13. [Risks & mitigations](#13-risks--mitigations)
14. [Test strategy](#14-test-strategy)
15. [Implementation order checklist — Wave 2](#15-implementation-order-checklist--wave-2)

---

## 1. Problem — current path-keyed identity

### 1.1 The path-keyed importer

The current TOML→Fjall importer uses the forward-slash **file path** as the canonical key for every imported artifact, including each `_instance.toml`. The relevant code is `eustress/crates/worlddb/src/import.rs`:

- `import.rs:62–66` — the doc comment commits the design: *"Keys are forward-slash relative paths from `space_root` (`Lighting/_service.toml`, `Workspace/Tower/MegaTower_Core/_instance.toml`, …)"*.
- `import.rs:115–119` — directory keys are built by string-concatenating the relative prefix with the directory name.
- `import.rs:122–126` — file keys are built the same way: `let rel = if rel_prefix.is_empty() { name.to_string() } else { format!("{rel_prefix}/{name}") };`.
- `import.rs:127–132` — the file bytes are written under that path via `db.put_file(&rel, &bytes)?`. There is **no read-back step** that asks "do we already have this entity by some other identity?"

The trait API the importer drives is itself path-keyed: `eustress/crates/worlddb/src/backend.rs:172–183`:

```rust
fn put_file(&self, rel_path: &str, bytes: &[u8]) -> Result<()>;
fn get_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>>;
fn delete_file(&self, rel_path: &str) -> Result<()>;
```

The Fjall backend's implementation seals this in: `eustress/crates/worlddb/src/fjall_backend.rs:502–514` normalises the rel path and writes to the `tree` partition directly. There is **no** reverse index from UUID → key.

### 1.2 Concrete consequences of path-keying

1. **Two paths = two entities.** If a user duplicates a folder, or copies `Workspace/Tower/MegaTower_Core/` to `Workspace/Backup/MegaTower_Core/`, the importer writes two unrelated `tree/<path>/_instance.toml` rows. The Explorer shows two entities. There is no way to know they describe the same conceptual entity.
2. **Re-import after rename creates orphans.** A user renaming `Workspace/Tower/` → `Workspace/MainTower/` on disk produces an importer that writes every child under the new prefix but leaves every old-prefixed row in Fjall. `tree_is_empty()` is `false`, so `convert_space_if_needed` (`eustress/crates/engine/src/space/auto_convert.rs:34`) short-circuits and never re-imports. The DB drifts permanently from disk.
3. **Live ECS edits round-trip through path.** Save-back code paths use `LoadedFromFile.toml_path` as the write target; deleting or moving the folder loses the connection.
4. **Roblox `.rbxl` re-import duplicates.** Each re-import resolves Roblox referents to fresh `Workspace/<Name>/<Name-N>/` directories; the importer cannot tell that the same `referent` was already imported. The user sees duplicated trees per re-import.
5. **Cross-space MOVE cannot preserve identity.** The clipboard `EditorClipboard::generate_new_id` (`eustress/crates/engine/src/clipboard.rs:365–375`) mints a fresh `u32` from `SystemTime::now().as_nanos()`. That id is local to the editor session and has no persistence; identity across spaces is impossible.
6. **The `uuid` field already exists on `Instance` and is unused as a key.** `eustress/crates/common/src/classes.rs:577–580`:

   ```rust
   /// Stable UUID persisted across sessions (for cross-reference and networking)
   /// Generated on first save, preserved across loads/restarts.
   #[serde(default)]
   pub uuid: String,
   ```

   `Default for Instance` (`classes.rs:588–599`) constructs `uuid: String::new()`. Three callers in `instance_loader.rs` initialise `uuid: String::new()` (`instance_loader.rs:1573, 1726, 1827`). It is a latent field, populated nowhere, queried nowhere, indexed nowhere.

### 1.3 What's already in the right place

The crate has the right substrate in three places:

- `eustress/crates/worlddb/src/rkyv_values.rs:228–254` defines `ArchInstanceCore`, the rkyv archive-model record that represents one entity's authoritative state in a single zero-copy blob. This is what should be keyed by UUID.
- `eustress/crates/worlddb/src/keys.rs:60–70` reserves `ComponentTypeId::INSTANCE_CORE = ComponentTypeId(8)` for that record.
- `eustress/crates/engine/src/space/arch_instance.rs:108–176` provides the lossless `instance_to_arch` / `arch_to_instance` bridge between the serde `InstanceDefinition` parse model and the rkyv archive model. This means UUID-keyed records can be both read and written without a second parse pass.

The substrate is in place; what's missing is the identity discipline.

---

## 2. Unified identity model

### 2.1 The three pieces

- **Canonical key:** every entity has a `uuid: String` — a 32-character lowercase-hex string derived from blake3 (truncated to 16 bytes / 128 bits). UUIDs are immutable for the lifetime of the entity.
- **Primary store:** Fjall partition `entities` keys each entity's `ArchInstanceCore` by `uuid`. One record per entity. Updates are in-place. Deletes remove the row.
- **Secondary indexes:** path, parent, class — all rebuilt at any time from the primary store.

### 2.2 Identity vs. naming

| Concept | Owned by | Mutable? | Used for |
|---|---|---|---|
| `uuid` | Eustress (deterministic hash) | No | Storage key, network refs, audit log keys, cross-space refs |
| `path` | User (folder + filename) | Yes (rename, move) | Human Explorer view, TOML import source, file-watcher |
| `name` (metadata.name) | User (display label) | Yes | Property panel, Slint Explorer column |

Renames change the **path**; the UUID does not move. A scripted query like `find_entity_by_uuid` is stable across file moves; `find_entity_by_path` is a convenience that walks the secondary index.

### 2.3 The contract one-liner

> **Every entity has exactly one UUID for its entire on-disk lifetime. Every entry surface either preserves or generates a UUID, never duplicates one. Every Fjall lookup keys by UUID; every path lookup is one hop through a reverse index.**

---

## 3. UUID generation per entry surface

The UUID is a `blake3(seed_bytes)` truncated to 128 bits → lowercase hex string (32 chars). The seed differs per surface so we get the right idempotency behaviour for each one.

### 3.1 TOML import — `import_space` and `auto_convert::convert_space_if_needed`

**Source:** the `[metadata]` section of `_instance.toml`.

**Rule:**

```text
if toml.metadata.uuid present and 32-char-hex:
    use it
else:
    uuid = hex(blake3(space_root_relative_path + "\x1f" + first_load_unix_nanos)[..16])
    write uuid back into toml.metadata.uuid
    flush TOML to disk so next reload preserves it
```

**Rationale:** the path + first-load-timestamp seed is *unique* (no two TOMLs hash to the same UUID — paths are unique within a space) and *deterministic* (the same Wave-2 first-load over the same disk state always produces the same UUID, so a parallel migration on a CI checkout matches a developer's local one if they migrate at the same wall-clock instant). The write-back guarantees that *every subsequent* import is the "uuid is present" branch — first-import is the only time the timestamp matters.

**Failure mode:** if the TOML is read-only on disk, we fall back to the in-memory UUID for this session and emit a `warn!` event with the path. The DB still keys correctly; the next writable load completes the write-back.

### 3.2 Studio create — `instance_create::create_instance` (canonical C2 path)

**Source:** `instance_create.rs:121` — every new-entity surface (Insert menu, Model ribbon, Toolbox, MCP `create_entity`, drag-drop import) routes through this function.

**Rule:**

```text
uuid = hex(blake3(uuid_v4_random_bytes + "\x1f" + creation_unix_nanos)[..16])
overrides.metadata.uuid = uuid
write to TOML in apply_overrides (same function that today writes metadata.name and metadata.unit)
```

The `uuid_v4` term ensures two simultaneous creates on the same wall-clock instant don't collide; the timestamp term makes audit-replay deterministic for a sequence of creates by the same user.

**Where to write it:** `instance_create.rs:201–214` already has the "ensure metadata table exists, then insert name" pattern. Adding a `uuid` insert next to the `name` insert is one extra `meta_table.insert("uuid".to_string(), toml::Value::String(uuid))` call. The Wave-2 PR is small.

### 3.3 Cross-space MOVE — clipboard with `is_cut == true`

**Source:** `EditorClipboard::is_cut` (`clipboard.rs:279`), the "Cut mode (delete originals after paste)" flag.

**Rule:**

```text
uuid stays the same.
On paste-target write:
    1. read source ArchInstanceCore from source-space's entities partition (by uuid).
    2. write the same uuid + same core (with new transform offset) to target-space's entities.
    3. delete the source row + reverse-index entries in source-space.
    4. emit an audit_log event: { kind: "MoveCrossSpace", uuid, src_space_id, dst_space_id }.
```

**Conflict case:** if `dst_space.entities[uuid]` already exists, the move *fails* with an error toast — moves preserve identity, not create-with-overwrite. The user resolves by either deleting the destination or selecting "Copy" instead of "Cut".

### 3.4 Cross-space COPY — clipboard with `is_cut == false`

**Rule:**

```text
copy_counter starts at 1 for the first paste, increments per subsequent paste.
new_uuid = hex(blake3(source_uuid + "\x1f" + target_space_id + "\x1f" + copy_counter_be8)[..16])
write the new uuid + a clone of the core to target entities partition.
DO NOT delete source.
```

**Why per-paste counter, not per-copy?** The user expects ten ctrl-V presses after one ctrl-C to produce ten distinct entities. The counter is reset on the next ctrl-C (which already happens in `EditorClipboard::clear` at `clipboard.rs:317`).

**Why include `target_space_id`?** So a copy from `SpaceA → SpaceB` and `SpaceA → SpaceC` of the same source produces distinct UUIDs in each target. Without it, paste-into-SpaceB and paste-into-SpaceC would collide if a future "move-from-B-to-C" landed.

### 3.5 Roblox `.rbxl` import

**Source:** the Roblox binary deserializer emits a `referent` string per Instance (96-bit Roblox ID, hex-encoded).

**Rule:**

```text
space_import_salt = blake3(space_id_bytes + "rbxl-v1")[..16]   # constant per (space, import-source)
uuid = hex(blake3(referent_bytes + "\x1f" + space_import_salt_bytes)[..16])
```

**Idempotency guarantee:** re-importing the same `.rbxl` into the same Space produces *exactly the same UUIDs*. The four-surface contract row 4 then guarantees UPDATE-not-INSERT, so the re-import refreshes Properties/Transform/Asset in place. The user sees one entity per Roblox referent, no matter how many times they re-import.

**Two-space safety:** importing the same `.rbxl` into two different spaces produces different UUIDs (because `space_import_salt` differs). Cross-space dedup is the user's explicit action via clipboard, not an accident of importing the same file twice.

### 3.6 Studio drag-drop import of a glTF / mesh

Glb-only imports without an enclosing `.rbxl` go through `create_instance` (Section 3.2). The drag-drop handler synthesizes the InstanceOverrides; the UUID is generated as for any other Studio create.

### 3.7 Procedural / scripted entity creation (Luau / Rune)

`SoulService` scripts that spawn entities call into `create_instance` via the engine bridge. Same as 3.2. The UUID is allocated by the engine, not the script — scripts cannot inject their own UUIDs (this would let a malicious script overwrite an existing entity's record by colliding the UUID). Scripts may *query* by UUID via `find_entity_by_uuid`.

---

## 4. The four entry-surface contract

The contract is a single decision table. Every entry surface chooses one row.

| # | Entry surface | UUID source | Fjall behaviour on commit | When UUID is in target Fjall |
|---|---|---|---|---|
| 1 | TOML import (`auto_convert` first-load, manual re-import) | `[metadata].uuid` if present, else generated by §3.1 + written back | UPDATE if `entities[uuid]` exists, INSERT if not | UPDATE-in-place |
| 2 | Live ECS edit (Properties panel, gizmo, MCP `update_entity`, file_watcher noticing a TOML edit) | known — read from `Instance` component (`classes.rs:580`) | always UPDATE | UPDATE (this is the only valid case for this surface) |
| 3a | Cross-space COPY | new uuid via §3.4 hash | always INSERT | error — would mean §3.4 hash collided, see §13.1 |
| 3b | Cross-space MOVE (Cut+Paste) | preserve source uuid (§3.3) | INSERT in target + DELETE in source | error toast — user must resolve (§3.3) |
| 4 | Roblox `.rbxl` import | `blake3(referent + space_import_salt)` (§3.5) | UPDATE if exists, INSERT if not | UPDATE-in-place (idempotent re-import) |
| 5 | Studio create (Insert menu, Toolbox, MCP `create_entity`, drag-drop) | new uuid via §3.2 | INSERT | error — would mean §3.2 random collided (essentially impossible) |
| 6 | Procedural script create (Luau/Rune `create_entity`) | new uuid via §3.2 (engine assigns, not script) | INSERT | error — same as 5 |

Surface 1 and 4 are the **only** two surfaces that can legitimately UPDATE an existing row when the caller "thinks they're creating". Both are explicit *imports* — the user knows they may be refreshing existing content. Surfaces 5 and 6 are *creates*; collision in those surfaces is a bug and surfaces an error.

---

## 5. Fjall layout & reverse indexes

### 5.1 Partition layout (after Wave-2)

The current `world.fjalldb/` has the partitions documented at `eustress/crates/worlddb/src/fjall_backend.rs:99–117`:
- `entities` — per-component KV (key encoder schema)
- `meta` — header mirror, tx counter
- `tree` — the path-keyed file mirror (the dedup-gap source)
- `datastore`, `datastore_ord` — Roblox DataStore parity

Wave-2 keeps all current partitions and adds three secondary-index partitions:

```text
world.fjalldb/
├── entities/               # primary — keyed by (component_type, entity_id) today
│   └── (Wave-2 adds INSTANCE_CORE rows keyed solely by uuid; see §5.2)
├── tree/                   # legacy — path-keyed file mirror, kept for fallback
├── meta/                   # header.bin mirror, tx counter
├── datastore/, datastore_ord/
├── path_to_uuid/           # NEW — secondary index, path -> uuid
├── uuid_to_path/           # NEW — secondary index, uuid -> last-known path (Explorer convenience)
└── class_index/            # NEW — secondary index, class_name -> [uuid...]
```

### 5.2 The primary `entities` row layout

Today: `entity:{schema_v}:{component_type_u16_be}:{entity_id_u64_be}` (`keys.rs:13`). The `entity_id` is `EntityId(u64)`, which corresponds to a Bevy `Entity::to_bits()` — **session-local**, not persistent.

Wave-2 introduces a new row variant for `ComponentTypeId::INSTANCE_CORE` (id 8) keyed by the **persistent uuid**, not the session-local entity id:

```text
F | schema_v(u8) | component=8(u16 be) | uuid_bytes(16)
                                       ^^^^^^^^^^^^^^^^
                                       NEW — was entity_id_u64_be
```

This is the long-term home for the rkyv `ArchInstanceCore`. The Morton spatial encoder (`MortonKeyEncoder::encode_spatial`, `keys.rs:268`) is unaffected — it keys by position for spatial scans, and the value at each spatial key is the *uuid* (16 bytes), not the inlined core. Lookups join: spatial-scan → uuid list → bulk-get cores by uuid.

`ComponentTypeId` ids 1–7 (Transform, BasePart, …) keep the existing `entity_id_u64_be` layout — they are session-local per-Bevy-entity component scratch; the binary-ECS load path materializes them from `INSTANCE_CORE` at boot.

### 5.3 The three secondary indexes

```text
path_to_uuid/<space_relative_path>     → <uuid_16_bytes>
uuid_to_path/<uuid_16_bytes>           → <space_relative_path_utf8>
class_index/<class_name>/<uuid_16_bytes> → <empty>
```

**`path_to_uuid`** is rebuilt every full import. Lookup answers "what entity lives at this path right now?" — a single Fjall `get`.

**`uuid_to_path`** is the reverse — answers "what's the human-readable path for this uuid?" Used by the Explorer to render a row, by error messages, and by the file-watcher to know what disk path to write to when a live ECS edit fires.

**`class_index`** is an empty-value marker — the prefix-scan answers "give me every Part / TextLabel / Model in this Space" in one Fjall `iter_with_prefix`. Avoids decoding 100k `ArchInstanceCore` records just to count classes.

Each secondary index is *derivable* — Wave-2 ships a `rebuild_indexes(db)` admin operation that can drop and rebuild all three from the primary `entities` rows. This is the crash-recovery primitive (§13.2).

### 5.4 The `tree` partition's revised role

`tree/` becomes the **bulk file mirror** for non-entity content (scripts, meshes, GUI TOMLs, .md docs, `_service.toml`). Entity `_instance.toml` files migrate out: their rkyv-archived form lives in `entities/<uuid>` (primary), and `tree/<path>/_instance.toml` is *no longer the source of truth* once a Space is migrated.

For one release window, the importer keeps writing to `tree/` *in addition to* the new primary store. The fallback read path is preserved. Wave-3 deletes the `tree/<*/_instance.toml>` rows after stamping the header's `migrated_to_uuid_at`.

---

## 6. Migration plan — existing path-keyed stores

The migration is per-Space, runs once, and is idempotent (a partial run can resume).

### 6.1 Detection

On `convert_space_if_needed` (`auto_convert.rs:33`) the new logic is:

```text
header = read header.bin
if header.migrated_to_uuid_at is Some(_):
    use the uuid-primary path -- nothing to do.
    return
if tree_is_empty():
    fresh space, no migration needed; new TOMLs already carry uuids via §3.1
    write header.migrated_to_uuid_at = now
    return
else:
    run migrate_tree_to_uuid(db, space_root)
```

### 6.2 Migrate algorithm

```text
fn migrate_tree_to_uuid(db, space_root):
    span = info_span!("identity.migrate", space=...)
    let mut counter = 0
    for (rel_path, bytes) in db.iter_tree() where rel_path.ends_with("/_instance.toml"):
        # 1. parse the TOML
        let raw = utf8(bytes)
        let mut def = InstanceDefinition::parse(raw)?

        # 2. extract or generate uuid
        let uuid = match def.metadata.uuid {
            Some(u) if is_valid_uuid(u) => u,
            _ => {
                let u = blake3(rel_path + "\x1f" + now_nanos)[..16].to_hex()
                def.metadata.uuid = Some(u.clone())
                # write back to disk (the canonical TOML)
                let new_raw = toml::to_string_pretty(&def)?
                db.put_file(&rel_path, new_raw.as_bytes())?       # tree partition
                std::fs::write(space_root.join(&rel_path), &new_raw)?  # disk
                u
            }
        }

        # 3. bake to rkyv ArchInstanceCore
        let core = instance_to_arch(&def)
        let core_bytes = encode_instance_core(&core)?

        # 4. write primary entities row + secondary indexes
        db.put_entity_core_by_uuid(&uuid, &core_bytes)?
        db.put_path_to_uuid(&rel_path, &uuid)?
        db.put_uuid_to_path(&uuid, &rel_path)?
        db.put_class_index(&core.class_name, &uuid)?

        counter += 1
        if counter % 1000 == 0:
            db.flush()?
            db.put_meta(b"migration_checkpoint", &counter.to_be_bytes())?  # resume token

    # 5. stamp the header as migrated
    header.migrated_to_uuid_at = Some(now_rfc3339())
    header.write(world_root)?
    db.put_meta(b"migration_checkpoint", b"done")?
    info!(target: "identity.migrate", entities = counter, "migration complete")
```

### 6.3 Resume-from-checkpoint

A migration killed at entity 50,000 of 200,000 should resume, not restart. The `migration_checkpoint` meta key records the last-flushed counter. On restart we:

1. Read `migration_checkpoint`. If `"done"`, skip.
2. Otherwise, iterate `tree/<*/_instance.toml>`, count entries, and only process indexes `>= checkpoint`. (Iteration order is Fjall's natural byte order — stable across runs given the same tree.)

This is good enough for "user closed the editor mid-migration"; for crash-safety we additionally write each `put_entity_core_by_uuid + put_path_to_uuid + put_uuid_to_path + put_class_index` as one atomic Fjall commit.

### 6.4 What happens to `tree/<path>/_instance.toml` after migration

Kept *as-is* for one release. The fallback read path is:

```text
fn read_entity_by_uuid_or_path(db, ref):
    if let Some(uuid) = path_to_uuid.get(ref):  # ref looked like a path
        return entities.get_uuid(uuid)
    if entities.get_uuid(ref).is_some():        # ref looked like a uuid
        return entities.get_uuid(ref)
    # legacy fallback — still on tree/
    if let Some(bytes) = tree.get_file(ref):
        # one-shot promotion: parse, bake, write to uuid-primary, return
        let def = InstanceDefinition::parse(bytes)?
        let uuid = ensure_uuid(&mut def, ref)
        db.put_entity_core_by_uuid(...)?
        return entities.get_uuid(uuid)
    Err(NotFound)
```

This lets a half-migrated Space keep working while the background migration finishes.

Wave-3 (next release) deletes the `tree/<*/_instance.toml>` rows after verifying `header.migrated_to_uuid_at` and `path_to_uuid` are populated for every `_instance.toml` in the tree.

### 6.5 Non-entity files (scripts, meshes, `_service.toml`)

These stay path-keyed in `tree/`. They have no notion of identity beyond their path; UUID-keying them adds complexity without solving any problem. The migration touches only `*/_instance.toml`.

---

## 7. Backward compatibility — TOML schema addition

### 7.1 The schema change

`InstanceMetadata` (`eustress/crates/engine/src/space/instance_loader.rs:565`) gains one field:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub uuid: Option<String>,
```

`#[serde(default)]` means TOMLs *without* the field deserialize cleanly. `skip_serializing_if = "Option::is_none"` means newly-emitted TOMLs that somehow lose the field don't write an empty `uuid = ""` line — only `Some(uuid_string)` produces output. (The migration always sets it to `Some`, so the skip clause is purely defensive against rounding-trip code that drops the field.)

The same field lands on the `Instance` ECS component — `eustress/crates/common/src/classes.rs:580` already has it as `pub uuid: String` (no `Option`). Wave-2 keeps the `String` form there (empty = "not yet known", which only happens during a transient spawn-before-load window).

### 7.2 The TOML schema, before and after

Before (current state, no uuid):

```toml
[metadata]
class_name = "Part"
name = "MainTower"
created = "2026-05-21T00:00:00Z"
unit = "m"

[transform]
position = [0.0, 0.0, 0.0]
# ...
```

After (Wave-2, uuid present):

```toml
[metadata]
class_name = "Part"
name = "MainTower"
created = "2026-05-21T00:00:00Z"
unit = "m"
uuid = "4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7"

[transform]
position = [0.0, 0.0, 0.0]
# ...
```

### 7.3 Format of the uuid value

- 32 lowercase hex characters, no dashes
- Source: `blake3(seed)` truncated to first 16 bytes (128 bits)
- Wave-2 validates with a regex on load: `^[0-9a-f]{32}$`
- Invalid format → log `warn!` + treat as "not present" → generate fresh

The format is deliberately not RFC-4122 (no dashes, no version nibble). We are not generating UUIDs in the RFC sense; we are generating deterministic content-addressed 128-bit identifiers. The "UUID" word is retained because it's what's already on the `Instance` field; documentation should clarify "128-bit blake3-derived hex string".

### 7.4 Where this field is written

The migration writes it (§6.2). The `instance_create::apply_overrides` function (`eustress/crates/common/src/instance_create.rs:184`) writes it for fresh creates. The live-edit save-back path (today the file_watcher's reverse mirror) writes it whenever it re-emits a TOML.

### 7.5 Old engine reading new TOMLs

Forward-compatible: `#[serde(default)]` on the metadata struct means an old engine parsing a Wave-2 TOML simply ignores the `uuid` field. It re-emits the TOML without the field (because the struct doesn't have it), and on the next load Wave-2 detects the missing uuid and regenerates from the path-based seed (§3.1). The regenerated uuid will differ from the original — the *path-based seed* gives the same uuid only if the file path hasn't moved AND the regeneration happens on the same `first_load_unix_nanos`, which it won't. So an old engine round-trip *changes the uuid*, with the consequence that any cross-references to it become stale.

**Recommendation:** once Wave-2 ships, version-gate via `header.world_schema_version`. A Space whose header says it was last opened by a Wave-2+ engine should refuse to open in a pre-Wave-2 engine (the existing `WorldSchemaVersion::is_future` check in `header.rs:80` already exists; the only change is to bump `WorldSchemaVersion::CURRENT` from `1` to `2`).

---

## 8. Conflict resolution

### 8.1 Two TOMLs at different paths claim the same uuid

**Cause:** the user copied a folder on disk including `_instance.toml`, didn't run the engine in between, and tried to re-import.

**Behaviour:**

```text
At import time, when path_to_uuid[uuid] is already set to a different path:
  1. log error!  "uuid collision: path A and path B both claim uuid U;
                  keeping A as canonical, renaming B's uuid"
  2. for the second TOML (B), regenerate uuid via §3.1 (path + now)
  3. write the new uuid back to B's TOML on disk
  4. proceed
```

The user-visible toast: *"Two entities had the same UUID — renamed one. Check the modified TOMLs."*

### 8.2 A TOML changes uuid between loads

**Cause:** user manually edited the `[metadata].uuid` field — perhaps copy-pasting a uuid from another entity.

**Behaviour:**

```text
On load:
  let on_disk_uuid = toml.metadata.uuid
  let path_index_uuid = path_to_uuid.get(rel_path)
  if on_disk_uuid != path_index_uuid:
      log warn!  "uuid changed for {rel_path}: was {old}, now {new}"
      if entities.get_uuid(new).is_some():
          # case A: the user pointed this TOML at another entity's uuid
          error!  refuse to load; the TOML's old entity is orphaned, the new one is owned by someone else
          surface: "Two TOMLs claim the same UUID. Choose: (a) keep old TOML's UUID, (b) merge into new"
      else:
          # case B: the user invented a uuid that doesn't yet exist
          # treat as the user splitting this entity into a new one
          - delete old entities[on_disk_uuid_old]
          - delete old path_to_uuid[rel_path] entries
          - insert new entities[on_disk_uuid_new] from this TOML
          - update indexes
          - log info!  "uuid rotated for {rel_path}: cross-refs to old uuid are now stale"
```

The case-B "treat as a split" is dangerous (audit log refs become stale) and gets a yellow Output-panel banner: *"UUID rotation detected. References from scripts, audit log, or external systems will not follow."*

### 8.3 Cross-space MOVE collides with existing uuid in target space

**Cause:** the user moved entity E (uuid U) from Space A → Space B, but Space B already had an entity with uuid U (because the user previously imported the same `.rbxl` into both spaces, §3.5).

**Behaviour:**

```text
fail the move with a toast:
  "Target space already contains an entity with this UUID
   (likely a previous Roblox import shared a referent).
   Choose Copy instead of Cut, or delete the target's existing copy first."
```

The move atomicity is preserved — source is *not* deleted. The user resolves manually.

### 8.4 UUID hash collision between two simultaneous creates

**Cause:** two `create_instance` calls landed on the same nanosecond AND `uuid_v4` happened to return the same bytes (probability ≈ 2⁻¹²⁸).

**Behaviour:** see §13.1 — the INSERT fails atomically; the higher layer retries with a re-randomized uuid_v4. Logged at `error!` so the cosmic event is visible.

### 8.5 TOML and `entities[uuid]` disagree on Properties

**Cause:** user edited the TOML on disk while the engine was running but the file_watcher missed the event (e.g. an editor that writes via temp-file + rename without an event the watcher subscribed to).

**Behaviour:**

```text
On next full load (engine restart) the file_watcher reconciles:
  if tree[path]/_instance.toml.bytes != serialize(entities[uuid_for_path]):
      precedence rule: TOML wins (the human-edited source)
      bake TOML to ArchInstanceCore, overwrite entities[uuid]
      log info!  "reconciled {path}: TOML edits applied to binary store"
```

The reverse case (engine wrote to Fjall while disk had a stale TOML) is *avoided* by the file_watcher writing every commit back to disk; if for some reason Fjall is ahead of disk, the same rule says TOML wins on next load, which loses the engine's writes. Wave-2 logs a `warn!` when Fjall is detected ahead of TOML so the user is aware.

---

## 9. Performance & sizing

### 9.1 UUID encoding choices and trade-offs

| Encoding | Bytes per uuid | Pro | Con |
|---|---|---|---|
| 32-char lowercase hex `String` | 32 (UTF-8 ASCII) | Human-readable, safe in TOML, easy to grep | 2x the entropy size |
| Base64 (22 chars) | 22 | Compacter | Slash/plus chars require quoting in some contexts |
| Raw `[u8; 16]` in indexes | 16 | Minimum size, native Fjall byte key | Not human-readable in `iter_tree` listings |
| `[u8; 16]` on the Instance component | 16 | No allocation, fits in cache line | Conversion at TOML serialize/deserialize boundary |

**Recommendation:**

- **TOML on disk:** 32-char hex `String` (already in metadata, already grep-able)
- **Fjall key bytes:** raw `[u8; 16]` (16 bytes per row, half the size of hex)
- **`Instance` component in RAM:** `[u8; 16]` once the Wave-2 migration finishes the `String` → `[u8; 16]` swap (Wave-3 task; Wave-2 keeps `String` for compatibility)

### 9.2 Index size

Per entity, Wave-2 stores:

- `entities/<uuid_16>` → encoded core (~200 bytes average per `ArchInstanceCore`)
- `path_to_uuid/<path>` → 16 bytes (uuid)
- `uuid_to_path/<uuid_16>` → variable path (50 bytes typical, e.g. `Workspace/Tower/MegaTower_Core/_instance.toml`)
- `class_index/<class>/<uuid_16>` → 0 bytes (empty marker)

Per-entity index overhead (excluding the primary `entities` row itself): `16 + 50 + 0 + (key overhead)` ≈ 80 bytes.

| Entity count | Primary store | Index overhead | Total |
|---|---|---|---|
| 10,000 | 2 MB | 0.8 MB | 2.8 MB |
| 100,000 | 20 MB | 8 MB | 28 MB |
| 1,000,000 | 200 MB | 80 MB | 280 MB |
| 10,000,000 | 2 GB | 800 MB | 2.8 GB |

The 800 MB index overhead at 10M entities is acceptable. The alternative (no `path_to_uuid` index, walking `entities` and decoding each `uuid_to_path` mapping to find the right one) is `O(N)` per path lookup — unacceptable for a Save button that queries 50 paths.

### 9.3 Lookup latencies (target)

| Operation | Mechanism | Target |
|---|---|---|
| `find_entity_by_uuid(uuid)` | Fjall point get on `entities/<uuid>` | < 100 μs |
| `find_entity_by_path(path)` | Fjall point get on `path_to_uuid/<path>` + `entities/<uuid>` | < 200 μs |
| `iter_class(class)` | Fjall prefix scan on `class_index/<class>/` | depends on count; ~1 μs per result + 100 μs per fetched core |
| `iter_spatial(chunk)` | Existing `MortonKeyEncoder::cell_prefix` scan returning uuids, then bulk-get cores | unchanged from today's spatial path |

### 9.4 Write amplification

Wave-2 turns each entity write into:

1. one `entities/<uuid>` PUT (the primary)
2. one `path_to_uuid/<path>` PUT (only when path changes; usually no-op)
3. one `uuid_to_path/<uuid>` PUT (only when path changes)
4. one `class_index/<class>/<uuid>` PUT (only when class changes; usually no-op)

For typical edits (gizmo move, color change), only the primary write runs — 1.0x amplification. For renames, all four run — 4.0x. Renames are rare.

### 9.5 blake3 cost

Blake3 hashing 16-byte seeds: ~50 ns per call on modern x86. Generating 100k UUIDs at create time costs 5 ms total. Migration of 1M entities adds 50 ms blake3 cost on top of the Fjall write time — negligible.

---

## 10. API surface changes

The trait changes are additive — every existing call signature is preserved. New methods are added to `WorldDb` (`eustress/crates/worlddb/src/backend.rs:117`).

### 10.1 New `WorldDb` trait methods

```rust
/// Primary store — entity core keyed by uuid (the 16-byte raw form).
fn put_entity_core_by_uuid(&self, uuid: &[u8; 16], core_bytes: &[u8]) -> Result<()>;
fn get_entity_core_by_uuid(&self, uuid: &[u8; 16]) -> Result<Option<Vec<u8>>>;
fn delete_entity_by_uuid(&self, uuid: &[u8; 16]) -> Result<()>;

/// Secondary indexes — rebuilt by rebuild_indexes() on corruption.
fn path_to_uuid(&self, rel_path: &str) -> Result<Option<[u8; 16]>>;
fn uuid_to_path(&self, uuid: &[u8; 16]) -> Result<Option<String>>;
fn iter_class(&self, class_name: &str) -> Result<Box<dyn Iterator<Item = Result<[u8; 16]>> + '_>>;

/// Maintenance — drop and rebuild all secondary indexes from the primary store.
fn rebuild_indexes(&self) -> Result<()>;
```

### 10.2 Unchanged signatures (semantic change only)

- `import_space(db, space_root)` (`import.rs:63`) — same signature. Internally, it now derives or extracts the uuid for each `_instance.toml` and routes through `put_entity_core_by_uuid` *in addition to* `put_file` (transitional dual-write).
- `convert_space_if_needed(space_root, db)` (`auto_convert.rs:33`) — same signature. Internally checks `header.migrated_to_uuid_at` and triggers migration if needed.

### 10.3 New `instance_create` helper

A small uuid-generator lives in `eustress/crates/common/src/instance_create.rs` next to `create_instance`:

```rust
/// Generate a fresh UUID for a Studio-create-style surface (§3.2).
pub fn fresh_uuid_for_create() -> String {
    let mut seed = [0u8; 32];
    // 16 bytes random
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut seed[..16]);
    // 16 bytes nanos (only first 8 used — pad to 16 for alignment)
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default()
        .as_nanos() as u128;
    seed[16..].copy_from_slice(&nanos.to_be_bytes());
    let hash = blake3::hash(&seed);
    hex::encode(&hash.as_bytes()[..16])
}
```

Called from `apply_overrides` (`instance_create.rs:184`) right next to the existing metadata writes.

### 10.4 `find_entity_by_*` helpers

Engine-side convenience wrappers in `eustress/crates/engine/src/space/world_db_binary.rs`:

```rust
pub fn find_entity_by_uuid(db: &dyn WorldDb, uuid: &str) -> Result<Option<ArchInstanceCore>>;
pub fn find_entity_by_path(db: &dyn WorldDb, path: &str) -> Result<Option<ArchInstanceCore>>;
pub fn find_entities_by_class(db: &dyn WorldDb, class: &str) -> Result<Vec<ArchInstanceCore>>;
```

These wrap the trait calls and handle the `[u8; 16]` ↔ hex `String` conversion at the boundary so the rest of the engine stays string-typed.

### 10.5 `delete_instance(uuid)`

```rust
pub fn delete_instance(db: &dyn WorldDb, uuid: &[u8; 16]) -> Result<()> {
    let mut batch = Commit::new();
    // primary
    batch.delete_entity_by_uuid(*uuid);
    // path_to_uuid
    if let Some(path) = db.uuid_to_path(uuid)? {
        batch.delete_path_to_uuid(&path);
    }
    // uuid_to_path
    batch.delete_uuid_to_path(*uuid);
    // class_index — need to read the core to know the class
    if let Some(core_bytes) = db.get_entity_core_by_uuid(uuid)? {
        let core = decode_instance_core(&core_bytes)?;
        batch.delete_class_index(&core.class_name, *uuid);
    }
    db.apply_commit(batch)?
}
```

The whole delete is one atomic commit — secondary indexes can never get out of sync with the primary.

---

## 11. Cross-system implications

### 11.1 Roblox importer

Today, re-importing a `.rbxl` duplicates the whole tree under a fresh `Workspace/<Name>-N/` folder. Wave-2 (§3.5) makes re-imports idempotent: the same `referent` always hashes to the same `uuid`, the four-surface contract (row 4) UPDATES the existing row, and the user sees one Tower with refreshed Properties instead of three Towers.

### 11.2 Cross-space copy/paste

`EditorClipboard` (`clipboard.rs:261`) currently mints `u32` ids in `generate_new_id` (`clipboard.rs:365`). Wave-2 replaces the per-paste id remap (`remap_ids`, `clipboard.rs:378`) with the §3.3 / §3.4 uuid logic. The `id_mapping: HashMap<u32, u32>` (`clipboard.rs:281`) becomes `id_mapping: HashMap<[u8; 16], [u8; 16]>` — same shape, real keys.

The clipboard's serialized form (`ClipboardEntityData2`, `clipboard.rs:21`) gains a `uuid: String` field so cross-process clipboard exchanges (the OS clipboard) carry identity, not just snapshots.

### 11.3 Multiplayer

Multiplayer entity refs (Wave-N) reference the persistent `uuid`, not a session-local `Entity::to_bits()`. The `eustress-fjall` per-store replication feed (`fjall_backend.rs:100–103`) is already byte-stream; it just needs to carry the new `entities/<uuid>` keys instead of (or in addition to) the `entities/<entity_id_u64>` keys.

The future "follow Player A's avatar" syscall becomes one `find_entity_by_uuid(avatar_uuid)` — no race condition between A's spawn and B's lookup, because both sides agreed on the uuid before either ECS spawn.

### 11.4 Audit log

Audit-log events today key by file path (where they reference an entity). Wave-2 keys by uuid:

```toml
[event]
kind = "TransformChange"
entity_uuid = "4f3a8c2b1e9d7654a0b8c2e3f4d5a6b7"
property = "translation"
old = [0, 0, 0]
new = [10, 0, 0]
timestamp = "..."
```

This makes the audit-log queryable independent of folder renames — *"show me every change to entity U over time"* works even if U has moved across three folders in two spaces.

### 11.5 MCP `find_entity` / `update_entity` / `delete_entity` tools

The MCP tool surface (e.g. `eustress/crates/mcp`) gains a `--uuid` parameter alongside the existing `--path` and `--name`. Path remains a convenient debug identifier; uuid is the stable cross-call reference.

### 11.6 `eustress-fjall` replication

`ReplOp::Put`/`ReplOp::Remove` (`fjall_backend.rs:507, 520`) carries byte-keys. Wave-2 just publishes more keys (the new `entities/<uuid>`, `path_to_uuid/*`, etc.) — no protocol-level change.

### 11.7 The `Instance::id` field

`eustress/crates/common/src/classes.rs:574` documents `pub id: u32` as the *"Unique entity ID (maps to Bevy Entity internally)"*. Wave-2 deprecates this field's role as the cross-session identifier — it remains for the *current Bevy session* but is no longer persisted as identity. (`Bevy Entity::to_bits()` is allocated per-session and never matches across runs.) The `uuid` field is the persistent identity; `id` is the Bevy live handle.

---

## 12. Open questions

These need a decision before Wave-2 implementation lands.

### 12.1 Hash function: blake3 vs sha256

**Choice locks-in forever** — once UUIDs are in TOMLs they cannot be re-hashed without invalidating all cross-refs.

| | blake3 | sha256 |
|---|---|---|
| Speed | ~6 GB/s on modern x86 | ~400 MB/s |
| Output | 256 bits, truncatable | 256 bits, truncatable |
| Crate | `blake3` (active) | `sha2` (stdlib of crypto, widely-used) |
| Collision resistance at 128 bits | birthday at 2⁶⁴ ≈ 18 quintillion | same |
| Audit-friendliness | newer, less-reviewed | older, FIPS-blessed |

**Recommendation: blake3.** Speed matters for 1M-entity migrations (blake3 reduces 50 ms cost to 5 ms). Both hash functions are equivalent at the 128-bit truncation point. blake3 is the existing convention in the rkyv-pivot world.

### 12.2 UUID storage format: hex `String` vs `[u8; 16]` on the Instance component

The Wave-2 plan stores hex `String` on the component (preserves the current `pub uuid: String` field shape and avoids a single-PR component-schema migration). Wave-3 considers `[u8; 16]` for memory wins.

**Recommendation:** ship Wave-2 as `String`, defer the binary form. The 16-byte savings on 100k entities = 1.6 MB; not worth blocking the dedup fix.

### 12.3 Should procedural scripts be allowed to inject UUIDs?

Currently the plan disallows it (§3.7) — scripts may *read* uuids but cannot *write* them. The motivation is to prevent a script from overwriting an arbitrary entity's record by colliding the uuid.

**Open:** is there a legitimate use case for "script imports content with a known uuid" (e.g. a level-loader Luau script reading a `levels.json` that contains uuids)? If so, we need a privileged `create_entity_with_uuid(uuid)` API gated by trust level.

### 12.4 What about non-Eustress UUIDs (e.g. user-imported assets carrying their own GUID)?

Mesh files / textures may carry external IDs (glTF asset hashes, Roblox AssetIDs). These are *asset identities*, not *entity identities*. The plan keeps them separate: asset manifest holds asset GUIDs, the entity's `[asset]` block references them by path/name; the entity itself has its own Eustress UUID.

### 12.5 Window between fresh-create and first-save

`create_instance` (§3.2) writes the uuid to TOML at creation time. There is no window where the entity exists in ECS without a uuid on disk — creation writes the TOML *before* the file_watcher spawns the ECS entity (this is already the canonical flow). The `Instance::uuid` component is populated from the TOML at spawn time. So `Instance::uuid` is `String::new()` only transiently inside the spawn function, never visible to consumers.

---

## 13. Risks & mitigations

### 13.1 UUID collision

Two `blake3(seed)[..16]` results colliding requires birthday ≈ 2⁶⁴ entities — about 18 quintillion. Practically zero.

**Mitigation if it happens anyway:** `put_entity_core_by_uuid` performs a `get` first when called from a §3.4 / §3.5 create surface. If the row exists *and* the contract row is "INSERT only," the call returns `Err(UuidCollision)`. The higher layer re-rolls (regenerates uuid with a fresh random component) and retries. Logged at `error!` so the cosmic event is visible.

### 13.2 Corrupt secondary index

A power-loss between primary-write and index-write leaves `entities[uuid]` populated but `path_to_uuid[path]` missing. Detection: at engine start, optionally run `rebuild_indexes()` if the meta key `last_clean_shutdown` is not within the last hour.

**Mitigation:** Wave-2 ships `rebuild_indexes()` (§10.1). It walks `entities`, decodes each core, derives the path from `core.extra[__meta].path` (the last-known path written to the core's cold tail at save time — Wave-2 adds this field), and rewrites `path_to_uuid` / `uuid_to_path` / `class_index`. Cost: one full `entities` scan + N writes. For a 100k-entity Space, ~5 seconds.

The Wave-2 primary store includes the path in `ArchInstanceCore.extra[__path]` precisely so the indexes are always *derivable* — no orphan-prevention complexity.

### 13.3 Migration partial failure

A migration that crashes at entity 50k of 200k must not leave the DB in an unloadable state.

**Mitigation:** per-entity migration is one atomic Fjall commit (primary + all four secondaries). The `migration_checkpoint` meta key is updated every 1000 entities. On restart, the migration resumes (§6.3). The fallback read path (§6.4) ensures partially-migrated content remains readable — Fjall reads return `entities[uuid]` if present, else fall back to `tree[path]` with one-shot promotion.

### 13.4 Disk full during TOML write-back

§3.1 writes the generated uuid back to TOML. If the disk is full or the file is read-only, the write fails.

**Mitigation:**

1. The uuid is still used for this session (in-memory).
2. A warn-level log records the failed write-back.
3. On next load, the path-based seed (§3.1) regenerates *the same uuid* if loaded at the same `first_load_unix_nanos` — but the timestamp is per-load, so it does *not* match. The uuid will rotate.
4. To preserve identity across the failed write, the engine maintains an in-memory "intended uuid for path" cache. If the same path appears in the next import within the same session, the cached uuid wins.

This is degraded; the fix is to get the user to make the file writable. A toast at the time of the failed write-back: *"Could not save UUID to {path} — readonly?"*

### 13.5 Migration changes user-visible TOMLs

The migration writes `uuid = ...` to every `_instance.toml`. Users with Git history see a one-time large commit of "added uuid field to every TOML."

**Mitigation:** documented in the release notes. Recommend users commit before opening the new engine. The change is single-purpose, mechanical, and easy to review (it adds exactly one line per `[metadata]` block).

### 13.6 TOML format drift

Wave-2 commits to the `uuid = "32-char-hex"` form. Any future format change (e.g. moving to base64) requires a migration to rewrite every TOML, *and* breaks every existing audit-log / script reference.

**Mitigation:** lock the format in this spec. The form is `lowercase hex, 32 chars, no separators` — forever.

---

## 14. Test strategy

### 14.1 Unit tests

In `eustress/crates/worlddb/src/import.rs`:
- TOML without uuid → migration generates one + writes it back
- TOML with uuid → preserved verbatim
- TOML with invalid uuid (`uuid = "not-hex"`) → warn + regenerate
- Two TOMLs with same uuid → first wins, second is renamed

In `eustress/crates/common/src/instance_create.rs`:
- `fresh_uuid_for_create()` returns 32-hex-char strings
- 10,000 calls in tight loop → all distinct (random component differs)

In `eustress/crates/engine/src/space/world_db_binary.rs`:
- `find_entity_by_uuid` returns same `ArchInstanceCore` after rebuild_indexes
- `find_entity_by_path` returns same as `find_entity_by_uuid(path_to_uuid(path))`

### 14.2 Integration tests

In `eustress/crates/engine/tests/identity_roundtrip.rs` (new file):

1. **Roblox re-import idempotency.** Import `tests/fixtures/Tower.rbxl` twice into a fresh space. Assert `iter_class("Part")` returns the same count both times. Diff entity bytes — should be identical.

2. **TOML round-trip preserves uuid.** Create entity via `create_instance`. Read the TOML. Confirm `uuid` field is present, hex, 32 chars. Restart engine (re-open space). Load entity. Confirm `Instance.uuid` matches the TOML.

3. **Cross-space COPY produces new uuid.** Create entity in Space A. Copy → paste into Space B. Source's uuid != destination's uuid. Source still exists. Paste again into Space B — third uuid, second copy still there.

4. **Cross-space MOVE preserves uuid.** Same as 3 but with Cut. Source removed. Destination's uuid == original.

5. **Path-keyed legacy fallback.** Open a pre-Wave-2 Space (no uuids in TOMLs, no `migrated_to_uuid_at` in header). Migration runs. Indexes populated. `find_entity_by_path` works. Re-open — `migrated_to_uuid_at` set, migration skips.

6. **Crash recovery.** Open a Space, start migrating, kill the process at 50% (use a SIGKILL after a deterministic-checkpoint hook). Re-open. Migration resumes from checkpoint. Final state matches a clean migration.

7. **Index rebuild.** Open a migrated Space. Manually corrupt the `path_to_uuid` partition (delete a key). Call `rebuild_indexes`. Verify lookup works again.

### 14.3 Telemetry

Wave-2 emits structured events under `target = "identity"`:

- `identity.migrate` — start/progress/done of migration
- `identity.uuid_generated` — debug-level, with surface tag and seed-input class
- `identity.collision` — error-level, the cosmic event
- `identity.path_to_uuid_miss` — warn-level, an entity in the tree without an index entry (rebuild triggered)

These flow through the existing `EustressStream` topics for the engine telemetry pipeline.

---

## 15. Implementation order checklist — Wave 2

A short, ordered PR series. Each item is independently mergeable and reviewable. Roughly one day each, with the migration PR the largest.

1. **Add `uuid: Option<String>` to `InstanceMetadata`.** File: `eustress/crates/engine/src/space/instance_loader.rs:565`. `#[serde(default, skip_serializing_if = "Option::is_none")]`. Update tests. No behaviour change. (PR 1)
2. **Add `fresh_uuid_for_create()` + `derive_uuid_for_import(path, ts)` helpers.** File: `eustress/crates/common/src/instance_create.rs`. Pure functions, unit tests. (PR 2)
3. **Add new `WorldDb` trait methods (§10.1).** Default-impl `Err("not supported")` on the trait so non-Fjall backends compile. Concrete impls on `FjallWorldDb`. New partitions: `path_to_uuid`, `uuid_to_path`, `class_index`. (PR 3)
4. **Add `migrated_to_uuid_at: Option<String>` to `WorldHeader`.** File: `eustress/crates/worlddb/src/header.rs:113`. `#[serde(default)]`. (PR 4)
5. **Wire `instance_create::apply_overrides` to write uuid into TOML.** File: `eustress/crates/common/src/instance_create.rs:184`. Adds one `meta_table.insert("uuid", ...)` next to the name insert. (PR 5)
6. **Write the migration routine `migrate_tree_to_uuid`.** New file: `eustress/crates/worlddb/src/migrate_identity.rs`. Uses `instance_to_arch` from the engine crate via a small new shared function. Includes resume-from-checkpoint. (PR 6)
7. **Update `convert_space_if_needed` to call migration when header missing `migrated_to_uuid_at`.** File: `eustress/crates/engine/src/space/auto_convert.rs:33`. (PR 7)
8. **Add `find_entity_by_uuid` / `find_entity_by_path` / `find_entities_by_class`.** File: `eustress/crates/engine/src/space/world_db_binary.rs`. (PR 8)
9. **Wire Roblox importer through §3.5 uuid derivation.** Files: Roblox importer crate (TBD — currently no `.rbxl` importer exists; the `rbxl` mentions in code are placeholders). If Roblox importer ships separately, this becomes a dependency. (PR 9, optional in Wave-2 if Roblox importer isn't yet shipped)
10. **Wire `EditorClipboard` paste through §3.3 / §3.4 uuid logic.** File: `eustress/crates/engine/src/clipboard.rs:240–414`. Replace `generate_new_id` with `mint_paste_uuid(source_uuid, target_space_id, counter)`. (PR 10)
11. **Add `delete_instance(uuid)` atomic delete.** Engine helper in `world_db_binary.rs`. (PR 11)
12. **Add `rebuild_indexes()` admin command.** Hooked into the Output panel's "Repair Indexes" button. (PR 12)
13. **Bump `WorldSchemaVersion::CURRENT` to `2`.** File: `eustress/crates/worlddb/src/header.rs:69`. Add migration registry entry from `1 → 2`. Forward-compat refuses to open a Schema-2 Space in a Schema-1 engine. (PR 13)
14. **Integration test suite from §14.2.** New file: `eustress/crates/engine/tests/identity_roundtrip.rs`. (PR 14)
15. **Release notes / docs update.** This document moves from `docs/architecture/IDENTITY.md` (spec) into the user-facing release notes' "What changed" section. (PR 15)

Wave-3 (next release, *not* in this spec) deletes the `tree/<*/_instance.toml>` rows after verifying every entity has a `path_to_uuid` index entry. Wave-4 swaps `Instance::uuid` from `String` to `[u8; 16]` for the RAM win (§9.1).

---

## Appendix A — File:line citations for the path-based code

The code that this spec replaces, for reviewer convenience:

| File:line | What |
|---|---|
| `eustress/crates/worlddb/src/import.rs:62–66` | Comment: "Keys are forward-slash relative paths from `space_root`" |
| `eustress/crates/worlddb/src/import.rs:115–119` | Building the rel directory key by string concat |
| `eustress/crates/worlddb/src/import.rs:122–126` | Building the rel file key by string concat |
| `eustress/crates/worlddb/src/import.rs:127–132` | `db.put_file(&rel, &bytes)` — the path-keyed write |
| `eustress/crates/worlddb/src/backend.rs:172–183` | `put_file` / `get_file` / `delete_file` trait methods — path-keyed surface |
| `eustress/crates/worlddb/src/fjall_backend.rs:502–514` | Concrete path-keyed implementation |
| `eustress/crates/engine/src/space/auto_convert.rs:33–71` | Per-Space seed logic — gates on `tree_is_empty`, never re-checks uuid |
| `eustress/crates/engine/src/bin/convert_to_eustress.rs:94–105` | Bulk CLI importer — same path-keyed loop |
| `eustress/crates/common/src/classes.rs:577–580` | The `pub uuid: String` field on `Instance` — currently unused as key |
| `eustress/crates/common/src/classes.rs:588–599` | `Default for Instance` — `uuid: String::new()` |
| `eustress/crates/engine/src/space/instance_loader.rs:1573,1726,1827` | Three call sites initialising `uuid: String::new()` (latent field) |
| `eustress/crates/engine/src/clipboard.rs:365–375,403–413` | `EditorClipboard::generate_new_id` — minted from `SystemTime::now().as_nanos()` per session |
| `eustress/crates/engine/src/clipboard.rs:281` | `id_mapping: HashMap<u32, u32>` — the session-local remap |
| `eustress/crates/engine/src/space/instance_loader.rs:565–600` | `InstanceMetadata` struct — gains the new `uuid` field in PR 1 |
| `eustress/crates/common/src/instance_create.rs:184–214` | `apply_overrides` — gains the uuid write in PR 5 |
| `eustress/crates/worlddb/src/header.rs:88–112` | `WorldHeader` — gains the `migrated_to_uuid_at` field in PR 4 |
| `eustress/crates/worlddb/src/rkyv_values.rs:228–254` | `ArchInstanceCore` — the rkyv record that will live under `entities/<uuid>` |
| `eustress/crates/engine/src/space/arch_instance.rs:108–176` | `instance_to_arch` — the parse-model → archive-model bridge the migration uses |
| `eustress/crates/worlddb/src/keys.rs:60–70` | `ComponentTypeId::INSTANCE_CORE = ComponentTypeId(8)` — the reserved id |

---

## Appendix B — Glossary

- **Eustress UUID:** 128-bit blake3-derived identifier, 32-char lowercase hex on disk and in RAM (Wave-2), `[u8; 16]` raw in Fjall keys.
- **Space:** one self-contained simulation root — `<Documents>/Eustress/<Universe>/Spaces/<Space>/`. Holds one `world.fjalldb/` and one TOML hierarchy.
- **Universe:** a collection of Spaces under one root, with shared `assets/` and `schema/`.
- **`ArchInstanceCore`:** the rkyv archive-model holding one entity's authoritative state in one zero-copy record (`worlddb/src/rkyv_values.rs:228`).
- **`InstanceDefinition`:** the serde parse-model that owns the `#[serde(flatten)]` machinery for rich TOMLs (`engine/src/space/instance_loader.rs:29`). Mapped to/from `ArchInstanceCore` by `arch_instance::instance_to_arch` (`engine/src/space/arch_instance.rs:108`).
- **Entry surface:** any code path that introduces a new or refreshed entity into the system — see §4's table for the four canonical ones.
- **The dedup gap:** the current bug where two paths describing the same conceptual entity become two separate rows in Fjall because path is the only identity.

---

*End of spec.*

---

### Critical Files for Implementation

The five files most critical to Wave 2 implementation:

- E:/Workspace/EustressEngine/eustress/crates/worlddb/src/import.rs
- E:/Workspace/EustressEngine/eustress/crates/worlddb/src/backend.rs
- E:/Workspace/EustressEngine/eustress/crates/engine/src/space/auto_convert.rs
- E:/Workspace/EustressEngine/eustress/crates/common/src/instance_create.rs
- E:/Workspace/EustressEngine/eustress/crates/engine/src/space/instance_loader.rs

(Honourable mentions for index-rebuild work: `eustress/crates/worlddb/src/fjall_backend.rs`, `eustress/crates/worlddb/src/header.rs`, and `eustress/crates/engine/src/space/arch_instance.rs`.)
