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
