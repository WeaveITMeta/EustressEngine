# Causal Op-Log — Producer Wiring Plan

Phase 1, Way 8 (causal-audit stream). This is the executable plan for wiring the
**producers** of the durable mutation op-log. It is the output of an adversarial
investigation of the WorldDb commit path (2026-06-29); follow it rather than
re-deriving the commit topology.

## What already exists (storage half — DONE)

- `eustress-worlddb/src/mutations.rs` — `MutationRecord` (rkyv) with
  `{ tx_id, ts_nanos, actor, op (Create/Update/Delete), class_name, uuid,
  rel_path, before, after, parent_tx, reason }` + `encode_mutation`/`decode_mutation`.
- `eustress-worlddb/src/keys.rs` — `encode_mutation_key(seq)` / `decode_mutation_key`
  (tag `'U'`, big-endian sequence → ascending range scan = replay).
- `FjallWorldDb` — a `mutations` partition + `WorldDb::record_mutation(&[u8]) -> Result<u64>`
  (assigns its OWN monotonic op-log sequence, persists the high-water mark in
  `meta:mutation_seq`, returns the assigned seq) and `iter_mutations(min_seq, max_seq)`.

**Key API fact:** `record_mutation` takes only the encoded record and assigns the
op-log key itself. Callers do NOT supply a key. Set `MutationRecord.tx_id` to the
correlated commit tx (or `0`) before encoding — it is correlation, not the key.

## The architecture finding that sets the approach

`apply_commit` is **NOT** the chokepoint to hook. Two hard reasons:

1. **DB-bloat trap (adversarially verified).** Per-frame transforms for
   non-binary entities flow THROUGH `apply_commit`: `mirror_transform_changes`
   (`engine/src/space/world_db_plugin.rs:538-616`) builds a `CommitOp::Put` with
   `ComponentTypeId::TRANSFORM` only and calls `db.apply_commit()` at a
   2048-ops/frame budget. A naive "record every apply_commit" hook would append
   up to 2048 records per idle/physics frame → 100×–1000× bloat. Inside
   `apply_commit` the only discriminator is the `ComponentTypeId`.
2. **`apply_commit` misses the real creates.** The dominant semantic
   creates/deletes (MCP / script / importer) go through the **binary-ECS path**
   (`engine/src/space/active_db.rs`), which calls `put_instance_core` /
   `put_entity_core_by_uuid` / `put_class_index` **directly**, bypassing
   `apply_commit` — and carries **no commit tx** (hence the self-assigning
   `record_mutation` API above).

**Therefore: record at the SEMANTIC caller sites, never from `apply_commit`'s
generic loop, and never from `mirror_transform_changes` / `mirror_binary_ecs_changes`.**

## The three wiring sites

All best-effort: build the record, `record_mutation(&encode_mutation(&rec)?)`,
log-and-continue on `Err` (reuse `create_binary_instance`'s existing
partial-failure warn pattern). Always AFTER the durable state write succeeds.

| Site | File:line | op | uuid | class_name | rel_path | after | before |
|---|---|---|---|---|---|---|---|
| Create | `active_db.rs:536-589` (`create_binary_instance`) | `Create` | in hand | in hand (writes `put_class_index`) | known at TOML sites, else None | encoded core | None |
| Delete | `active_db.rs` delete path (`delete_instance_core` callers / `DespawnEntity`) | `Delete` | resolved pre-delete | resolve pre-delete | — | None | prior bytes (optional pass 1) |
| Update | `branch.rs:332-352` (`BranchHandle::commit`), AFTER `self.parent.apply_commit(commit)?` | `Update` | from overlay | class-index lookup | — | new bytes | optional pass 1 |

**The Update site needs the TRANSFORM-exclusion gate** — skip recording when
every overlay op is `CommitOp::Put` with `component == ComponentTypeId::TRANSFORM`
(that overlay may carry the per-frame transform mirror). Add a helper
`commit_is_semantic(&Commit) -> bool` (= true unless all ops are TRANSFORM puts)
and call it only at this one `apply_commit`-adjacent site.

## Pass-1 field population (honest placeholders)

- `tx_id` = correlated commit tx where one exists (Update site), else `0`.
- `ts_nanos` = wall clock at record time (keep OUT of any determinism-sensitive
  system — confine to the record-construction site).
- `op` = statically known per site.
- `actor` = `MutationActor::System` (placeholder — see follow-up).
- `reason` = `None`. `parent_tx` = `None`.
- `before` = `None` in pass 1 (filling it needs an extra read at Update/Delete
  sites — acceptable there, NEVER on any per-frame path).

## Causality follow-up (separate pass)

`actor`/`reason`/`parent_tx` are absent from `Commit`/`CommitOp`/`BranchHandle`
today. Threading real causality touches only 2–3 core sites:
1. `BranchHandle::commit` — branch carries actor/reason from creation context.
2. binary create/delete signatures in `active_db.rs` — add `actor`/`reason` args.
3. `mirror_transform_changes` is EXCLUDED (it cannot attribute an actor and must
   not record anyway).

MCP/bridge, file-watcher, importer, and rune/script paths contain **no**
`apply_commit` calls (they route through `create_instance`/`create_binary_instance`),
so threading does not fan out across handlers. Do NOT block the additive op-log
on causality — ship pass 1 with `actor=System`, enrich later.

## Empirical coverage finding (2026-06-29 live run)

Confirmed against a live engine (Voltec space, bridge up, `inspect_scene` green):
the producer fires ONLY on the explicit binary create/delete chokepoints — bridge
`entity.create`, editor Insert (`spawn_events`), bridge `entity.delete`,
keybindings, undo. It does NOT fire on the **disk / file-watcher create path**:
`mcp__eustress__create_entity` writes an `_instance.toml`, and the resulting
file-watcher spawn persists via the PER-FRAME mirror (`mirror_binary_ecs_changes`
→ `mirror_binary_core`), which is deliberately NOT recorded (bloat gate). So an AI
creating via the MCP `create_entity` tool is currently NOT captured in the op-log.

DISK-CREATE COVERAGE — RESOLVED 2026-06-29 (the earlier `mirror_binary_core`
prediction was WRONG). A follow-up investigation found disk/file-watcher creates
do NOT go through the binary mirror at all: a new `_instance.toml` spawns a TOML
(FileSystem-rep) entity via `instance_loader::spawn_instance` (NO BinaryEcsInstance
marker), so `mirror_binary_core` never touches it. The clean, one-shot, create-only
funnel is `file_watcher::handle_file_created`'s instance arm. The op-log now records
a Create there (`active_db::record_disk_create`, actor = `FileWatcher`), reading
`instance.metadata.uuid` (skip-with-warn if absent — never mint a phantom id).
The discriminator is control-flow reachability, structurally airtight: moves drive
the per-frame write systems (record nothing); residency stream-ins (`spawn_binary_core`)
and boot/cold-load (`spawn_file_entry`) never reach `handle_file_created`; and its
`is_loaded` guard + atomic-write re-route drop spurious/replace events. REMAINING
gaps: GUI-element disk-creates (separate `put_gui` identity surface) and the
Update/property-edit path (no clean create-vs-update chokepoint — `put_instance`
is an upsert).

LIVE-TEST GOTCHA: the disk-based MCP tools (`create_entity`, `execute_luau`) write
to the MCP server's CONFIGURED universe (e.g. Universe1), NOT the running engine's
universe (Voltec here) — so they cannot drive a live op-log test against a
mismatched engine. Bridge tools (`inspect_scene`, `invoke_action`,
`select_entity`) DO follow the live port. To trigger a binary create/delete live,
use `select_entity` + `invoke_action("Delete")` (hits `delete_binary_instance`),
an editor-Insert action, or launch the engine on the MCP-configured universe.

## Verification (the bloat-regression gate)

In a live space: scripted create + move (many idle/physics frames) + delete via
MCP, then `iter_mutations(0, u64::MAX)` and assert **exactly N semantic records
and ZERO transform records**. The "many frames" is essential — it is what would
expose a mis-gated per-frame storm. Needs a full engine build + live MCP drive.

## Risks (load-bearing)

- **Bloat (highest):** any record call reachable from the per-frame mirror
  reintroduces the storm. The "semantic sites only" rule is the single gate.
- **Atomicity:** `record_mutation` is a separate partition with no cross-partition
  txn — a crash between the state write and the record loses a record. Record
  AFTER the durable write, best-effort, never failing the real write.
- **Replication:** `record_mutation` publishes to `s_mutations`; a mis-gated flood
  floods that stream too — the same gate protects both.
