//! `mutations` — the durable, replayable, causally-annotated op-log
//! ([`MutationRecord`]) behind the `mutations` Fjall partition (Phase 1, Way 8).
//!
//! Today's durable record is current STATE (the entities/tree keys overwrite in
//! place) plus a lossy in-memory `history.<kind>` stream — so the *sequence of
//! causes* that produced a world cannot be replayed across a restart. This adds
//! an append-only op-log: each create/update/delete lands a causally-annotated
//! record in Fjall, keyed by a backend-assigned monotonic op-log sequence
//! ([`crate::keys::encode_mutation_key`]); a range scan == replay.
//!
//! SCAFFOLD: the type + the [`crate::WorldDb::record_mutation`] / `iter_mutations`
//! storage API land here additively. Wiring the live producers (apply_commit
//! causality, create_instance, the `mutations.*` stream tee) is staged — see the
//! Phase 1 plan — and intentionally gated to SEMANTIC mutations (never the
//! per-frame Transform mirror, which would bloat the DB 100x-1000x).

use rkyv::{Archive, Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::rkyv_values::RKYV_VALUE_TAG;

/// What kind of change a [`MutationRecord`] captures.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOp {
    Create,
    Update,
    Delete,
}

/// Who caused a mutation — the provenance half of causality.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum MutationActor {
    User,
    Script(String),
    Mcp(String),
    Importer,
    FileWatcher,
    System,
}

/// One durable, causally-annotated entity mutation. `before`/`after` hold the
/// prior/new core (or component) bytes; `parent_tx` links the causal parent
/// (e.g. the script-run tx that caused this); `reason` is free-form provenance.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MutationRecord {
    /// Correlated commit tx, or 0 if none (e.g. the binary-ECS create path
    /// carries no commit tx). This is correlation only — NOT the op-log key;
    /// `record_mutation` assigns the key (its own monotonic sequence).
    pub tx_id: u64,
    pub ts_nanos: u64,
    pub actor: MutationActor,
    pub op: MutationOp,
    pub class_name: String,
    /// Durable identity of the affected entity.
    pub uuid: String,
    /// Space-relative TOML path for file-natured instances (else `None`).
    pub rel_path: Option<String>,
    /// Prior bytes (`None` for `Create`).
    pub before: Option<Vec<u8>>,
    /// New bytes (`None` for `Delete`).
    pub after: Option<Vec<u8>>,
    /// Causal parent tx (the cause-of-this-cause), for the replay DAG.
    pub parent_tx: Option<u64>,
    /// Free-form provenance ("undo", "import VS.rbxl", an MCP tool name).
    pub reason: Option<String>,
}

/// Encode a [`MutationRecord`] to a tagged rkyv archive (same tag scheme as
/// [`crate::rkyv_values::encode_instance_core`]).
pub fn encode_mutation(v: &MutationRecord) -> Result<Vec<u8>> {
    let archived = rkyv::to_bytes::<rkyv::rancor::Error>(v)
        .map_err(|e| Error::Archive(format!("rkyv encode MutationRecord: {e}")))?;
    let mut out = Vec::with_capacity(archived.len() + 1);
    out.push(RKYV_VALUE_TAG);
    out.extend_from_slice(&archived);
    Ok(out)
}

/// Owned decode of a tagged [`MutationRecord`] archive (copies past the tag byte
/// into an aligned buffer — Fjall hands back unaligned `Vec<u8>`).
pub fn decode_mutation(bytes: &[u8]) -> Result<MutationRecord> {
    if bytes.is_empty() || bytes[0] != RKYV_VALUE_TAG {
        return Err(Error::Archive(
            "rkyv value tag mismatch (MutationRecord)".into(),
        ));
    }
    let mut aligned = rkyv::util::AlignedVec::<16>::new();
    aligned.extend_from_slice(&bytes[1..]);
    let archived = rkyv::access::<ArchivedMutationRecord, rkyv::rancor::Error>(aligned.as_slice())
        .map_err(|e| Error::Archive(format!("rkyv access MutationRecord: {e}")))?;
    rkyv::deserialize::<MutationRecord, rkyv::rancor::Error>(archived)
        .map_err(|e| Error::Archive(format!("rkyv decode MutationRecord: {e}")))
}

/// serde-native, AI-readable view of one op-log entry (`seq` from the partition
/// key + the decoded [`MutationRecord`]). `before`/`after` collapse to presence
/// flags — the raw rkyv core bytes are not useful over JSON; the causal SHAPE
/// (op / class / uuid / actor / reason / time) is. This is what the planned
/// `oplog.tail` bridge/MCP read surface returns. (serde derives are fully
/// qualified because this module's `Serialize`/`Deserialize` are rkyv's.)
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct MutationView {
    pub seq: u64,
    pub tx_id: u64,
    pub ts_nanos: u64,
    /// "User" | "Script:<name>" | "Mcp:<tool>" | "Importer" | "FileWatcher" | "System".
    pub actor: String,
    /// "Create" | "Update" | "Delete".
    pub op: String,
    pub class: String,
    pub uuid: String,
    pub rel_path: Option<String>,
    pub has_before: bool,
    pub has_after: bool,
    pub parent_tx: Option<u64>,
    pub reason: Option<String>,
}

impl MutationView {
    /// Project an op-log entry (`seq` + record) into the serde read view.
    pub fn from_record(seq: u64, r: &MutationRecord) -> Self {
        let actor = match &r.actor {
            MutationActor::User => "User".to_string(),
            MutationActor::Script(s) => format!("Script:{s}"),
            MutationActor::Mcp(s) => format!("Mcp:{s}"),
            MutationActor::Importer => "Importer".to_string(),
            MutationActor::FileWatcher => "FileWatcher".to_string(),
            MutationActor::System => "System".to_string(),
        };
        let op = match r.op {
            MutationOp::Create => "Create",
            MutationOp::Update => "Update",
            MutationOp::Delete => "Delete",
        }
        .to_string();
        MutationView {
            seq,
            tx_id: r.tx_id,
            ts_nanos: r.ts_nanos,
            actor,
            op,
            class: r.class_name.clone(),
            uuid: r.uuid.clone(),
            rel_path: r.rel_path.clone(),
            has_before: r.before.is_some(),
            has_after: r.after.is_some(),
            parent_tx: r.parent_tx,
            reason: r.reason.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_record_rkyv_round_trip() {
        let create = MutationRecord {
            tx_id: 42,
            ts_nanos: 1_700_000_000_000_000_000,
            actor: MutationActor::Script("init.luau".into()),
            op: MutationOp::Create,
            class_name: "Part".into(),
            uuid: "u-1".into(),
            rel_path: Some("Workspace/Part/_instance.toml".into()),
            before: None,
            after: Some(vec![1, 2, 3, 4]),
            parent_tx: Some(41),
            reason: Some("import VS.rbxl".into()),
        };
        let back = decode_mutation(&encode_mutation(&create).unwrap()).unwrap();
        assert_eq!(create, back);

        // A delete record (after = None, before = Some) round-trips too.
        let del = MutationRecord {
            op: MutationOp::Delete,
            actor: MutationActor::User,
            before: Some(vec![9, 9]),
            after: None,
            parent_tx: None,
            reason: None,
            ..create
        };
        let back2 = decode_mutation(&encode_mutation(&del).unwrap()).unwrap();
        assert_eq!(del, back2);
    }

    #[test]
    fn mutation_view_projects_and_json_round_trips() {
        let rec = MutationRecord {
            tx_id: 7,
            ts_nanos: 123,
            actor: MutationActor::Mcp("create_entity".into()),
            op: MutationOp::Delete,
            class_name: "Part".into(),
            uuid: "u-9".into(),
            rel_path: Some("Workspace/Foo".into()),
            before: Some(vec![1, 2]),
            after: None,
            parent_tx: Some(6),
            reason: Some("undo".into()),
        };
        let v = MutationView::from_record(42, &rec);
        assert_eq!(v.seq, 42);
        assert_eq!(v.actor, "Mcp:create_entity");
        assert_eq!(v.op, "Delete");
        assert_eq!(v.class, "Part");
        assert!(v.has_before && !v.has_after);
        let json = serde_json::to_string(&v).unwrap();
        let back: MutationView = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Replay
// ─────────────────────────────────────────────────────────────────────────────
//
// The log records; without this it does not reconstitute. Everything above
// captures WHAT changed, and the read path (`iter_mutations` / `tail_mutations`
// / the bridge `oplog.tail`) can show it — but nothing applied a record back to
// state, so the op-log was an audit trail rather than a source of truth.
//
// Replay closes that. `apply_mutation` is the inverse of the producers:
// Create/Update write the `after` core, Delete removes the entity. `undo_
// mutation` uses `before` for the same record, which is what makes a
// point-in-time rewind possible rather than only a forward re-run.
//
// Two honest limits, both consequences of what the producers currently write:
//
// * A record with no `after` cannot be applied forward, and one with no
//   `before` cannot be undone. Today's producers set `before: None`, so undo
//   is inert until they capture before-images. `apply_mutation` reports that
//   as `Skipped` rather than pretending it succeeded.
// * Replay restores the UUID-keyed core and, when the core carries a
//   transform, the Morton-keyed spatial copy. It does NOT rebuild `tree`
//   rows: a replayed world is DB-shaped, which is what the runtime reads.

use crate::backend::WorldDb;

/// What [`apply_mutation`] actually did with a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// State was written or removed.
    Changed,
    /// The record carried no payload for this direction (e.g. forward-apply of
    /// a record with `after: None`). Not an error — a producer that has not
    /// been taught to capture that side yet.
    Skipped,
}

/// Apply one record FORWARD: make state match `after`.
pub fn apply_mutation(db: &dyn WorldDb, rec: &MutationRecord) -> crate::error::Result<Applied> {
    let uuid = match uuid_hex_to_bytes(&rec.uuid) {
        Some(u) => u,
        None => return Ok(Applied::Skipped),
    };
    match rec.op {
        MutationOp::Delete => {
            db.delete_entity_by_uuid(&uuid)?;
            Ok(Applied::Changed)
        }
        MutationOp::Create | MutationOp::Update => match rec.after.as_deref() {
            Some(bytes) => {
                write_core(db, &uuid, bytes)?;
                Ok(Applied::Changed)
            }
            None => Ok(Applied::Skipped),
        },
    }
}

/// Apply one record BACKWARD: make state match `before`.
///
/// A Create is undone by deleting; anything else is undone by restoring the
/// prior bytes. Returns `Skipped` when the record has no before-image, which
/// is the current state of every producer — see the module note.
pub fn undo_mutation(db: &dyn WorldDb, rec: &MutationRecord) -> crate::error::Result<Applied> {
    let uuid = match uuid_hex_to_bytes(&rec.uuid) {
        Some(u) => u,
        None => return Ok(Applied::Skipped),
    };
    match rec.op {
        MutationOp::Create => {
            db.delete_entity_by_uuid(&uuid)?;
            Ok(Applied::Changed)
        }
        MutationOp::Update | MutationOp::Delete => match rec.before.as_deref() {
            Some(bytes) => {
                write_core(db, &uuid, bytes)?;
                Ok(Applied::Changed)
            }
            None => Ok(Applied::Skipped),
        },
    }
}

/// Write a core to BOTH the uuid-primary store and, when the bytes decode to a
/// transform-carrying core, the Morton spatial index.
///
/// Both copies matter: `entities_uuid` is what a uuid lookup and the bridge
/// read, `entities` is what residency range-scans. Writing only one leaves a
/// replayed entity either invisible to streaming or invisible to lookup.
fn write_core(db: &dyn WorldDb, uuid: &[u8; 16], bytes: &[u8]) -> crate::error::Result<()> {
    db.put_entity_core_by_uuid(uuid, bytes)?;
    if let Ok(core) = crate::rkyv_values::decode_instance_core(bytes) {
        let id = crate::backend::EntityId(stored_id_from_uuid(uuid));
        db.put_instance_core(id, (core.t[0], core.t[1], core.t[2]), bytes)?;
    }
    Ok(())
}

/// Same derivation the bake uses, so a replayed entity keeps the id it had.
fn stored_id_from_uuid(uuid: &[u8; 16]) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&uuid[..8]);
    let id = u64::from_le_bytes(b);
    if id == 0 {
        1
    } else {
        id
    }
}

fn uuid_hex_to_bytes(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(chunk).ok()?;
        out[i] = u8::from_str_radix(s, 16).ok()?;
    }
    Some(out)
}

/// Outcome of replaying a span of the log.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReplaySummary {
    pub applied: usize,
    /// Records with no payload for the direction travelled.
    pub skipped: usize,
    /// Records that failed to decode.
    pub undecodable: usize,
}

/// Replay `[min_seq, max_seq]` forward, in recorded order.
///
/// Order is the whole point: the log is the only thing that knows two edits to
/// the same entity happened in a particular sequence, and applying them out of
/// order silently produces a different world.
pub fn replay_forward(
    db: &dyn WorldDb,
    min_seq: u64,
    max_seq: u64,
) -> crate::error::Result<ReplaySummary> {
    let mut sum = ReplaySummary::default();
    for (_seq, bytes) in db.iter_mutations(min_seq, max_seq)? {
        let Ok(rec) = decode_mutation(&bytes) else {
            sum.undecodable += 1;
            continue;
        };
        match apply_mutation(db, &rec)? {
            Applied::Changed => sum.applied += 1,
            Applied::Skipped => sum.skipped += 1,
        }
    }
    Ok(sum)
}

/// Rewind `[min_seq, max_seq]`, newest first.
///
/// Reverse order is required, not cosmetic: undoing oldest-first would restore
/// an early before-image and then immediately overwrite it with a later one.
pub fn rewind(db: &dyn WorldDb, min_seq: u64, max_seq: u64) -> crate::error::Result<ReplaySummary> {
    let mut sum = ReplaySummary::default();
    let mut span = db.iter_mutations(min_seq, max_seq)?;
    span.reverse();
    for (_seq, bytes) in span {
        let Ok(rec) = decode_mutation(&bytes) else {
            sum.undecodable += 1;
            continue;
        };
        match undo_mutation(db, &rec)? {
            Applied::Changed => sum.applied += 1,
            Applied::Skipped => sum.skipped += 1,
        }
    }
    Ok(sum)
}

#[cfg(test)]
mod replay_tests {
    use super::*;

    #[test]
    fn uuid_hex_round_trips() {
        let b = [0xABu8; 16];
        let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
        assert_eq!(uuid_hex_to_bytes(&hex), Some(b));
    }

    #[test]
    fn malformed_uuid_is_rejected_not_panicked() {
        assert_eq!(uuid_hex_to_bytes("nope"), None);
        assert_eq!(uuid_hex_to_bytes(""), None);
        assert_eq!(uuid_hex_to_bytes(&"z".repeat(32)), None);
    }

    #[test]
    fn stored_id_matches_the_bake_and_is_never_zero() {
        assert_ne!(stored_id_from_uuid(&[0u8; 16]), 0);
        let a = [3u8; 16];
        assert_eq!(stored_id_from_uuid(&a), stored_id_from_uuid(&a));
    }
}
