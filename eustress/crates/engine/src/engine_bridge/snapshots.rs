//! The engine bridge's `snapshot.*` methods: restore points for the open
//! Space over JSON-RPC, which the MCP snapshot tools call. Each method turns
//! its request into a call to [`crate::space::snapshot`], the core the
//! History panel also uses, and its result back into a response; the rules
//! and the refusals live there.

use bevy::prelude::*;
use serde_json::{json, Value};

use super::{server, BridgeError, BridgeRequest, BridgeResponse};
use crate::space::checkpoint;
use crate::space::snapshot::{self, Origin};

const LIST_DEFAULT: usize = 20;
const LIST_MAX: usize = 200;
const DIFF_DEFAULT: usize = 200;
const DIFF_MAX: usize = 5_000;

fn param_str<'a>(req: &'a BridgeRequest, key: &str) -> Option<&'a str> {
    req.params
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// A bridge caller: an agent, named by the principal the MCP server passes.
fn caller(req: &BridgeRequest) -> Origin {
    Origin::agent(param_str(req, "requested_by"))
}

fn answer(req: &BridgeRequest, result: Result<Value, String>) -> BridgeResponse {
    match result {
        Ok(v) => BridgeResponse::ok(req.id.clone(), v),
        Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
    }
}

/// `snapshot.save`
pub fn snapshot_save(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let label = snapshot::clean_label(param_str(req, "label").unwrap_or("snapshot"));
    let unsaved_tabs = snapshot::dirty_code_tab_names(world);
    let result = snapshot::take_snapshot(world, &label, &caller(req), None, snapshot::Prune::Now).map(|t| {
        json!({
            "id": t.id,
            "sha": t.sha,
            "label": label,
            "database_checkpoint": t.checkpoint,
            "written": t.written,
            "unchanged": t.unchanged,
            "checkpoints_pruned": t.pruned,
            // Script tabs with edits still in the editor: not in the
            // snapshot, which holds the saved file.
            "unsaved_script_tabs": unsaved_tabs,
        })
    });
    answer(req, result)
}

/// `snapshot.list`
pub fn snapshot_list(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let result = (|| -> Result<Value, String> {
        let root = snapshot::space_root(world)?;
        let limit = req
            .params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map_or(LIST_DEFAULT, |n| n as usize)
            .clamp(1, LIST_MAX);
        let db_backed = checkpoint::is_db_backed(&root);
        let kept: std::collections::HashMap<String, u64> = if db_backed {
            checkpoint::list_checkpoints(&root).into_iter().map(|c| (c.id, c.bytes)).collect()
        } else {
            Default::default()
        };
        let snapshots: Vec<Value> = snapshot::list(&root, limit)?
            .into_iter()
            .map(|s| {
                let bytes = kept.get(&s.id).copied();
                json!({
                    "id": s.id,
                    "label": s.label,
                    "created": s.created,
                    "sha": s.sha,
                    // A database-backed Space reverts only with its checkpoint.
                    "revertible": !db_backed || bytes.is_some(),
                    "checkpoint_bytes": bytes,
                })
            })
            .collect();
        Ok(json!({
            "database_backed": db_backed,
            "checkpoints_kept": snapshot::KEEP_CHECKPOINTS,
            "revert_pending": checkpoint::restore_pending(&root),
            "last_revert_failure": checkpoint::last_restore_failure(&root),
            "snapshots": snapshots,
        }))
    })();
    answer(req, result)
}

/// `snapshot.diff`
pub fn snapshot_diff(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let result = (|| -> Result<Value, String> {
        let root = snapshot::space_root(world)?;
        let id = param_str(req, "id").ok_or("missing 'id': the snapshot to compare against")?;
        let limit = req
            .params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map_or(DIFF_DEFAULT, |n| n as usize)
            .clamp(1, DIFF_MAX);
        let snap = snapshot::find(&root, id)?;

        // Tracked files that differ from the snapshot (on disk now, against
        // its commit), then files git has never seen.
        // `-z`: status and path alternate, NUL-separated, never quoted.
        let tracked = snapshot::nul_paths(&snapshot::git(
            &root,
            &["diff", "--name-status", "--no-renames", "-z", &snap.sha, "--"],
        )?);
        let untracked = snapshot::nul_paths(&snapshot::git(&root, &["ls-files", "--others", "--exclude-standard", "-z"])?);
        let mut changes: Vec<Value> = Vec::new();
        for pair in tracked.chunks_exact(2) {
            let kind = match pair[0].chars().next() {
                Some('A') => "added",
                Some('D') => "deleted",
                _ => "modified",
            };
            changes.push(json!({ "path": pair[1], "change": kind }));
        }
        for path in untracked {
            changes.push(json!({ "path": path, "change": "added" }));
        }
        let total = changes.len();
        changes.truncate(limit);

        let unsaved = world
            .get_resource::<crate::ui::StudioState>()
            .map_or(false, |s| s.has_unsaved_changes);
        Ok(json!({
            "id": snap.id,
            "label": snap.label,
            "sha": snap.sha,
            "total": total,
            "truncated": total > changes.len(),
            "changes": changes,
            // Edits still only in the editor are not on disk, so not above.
            "unsaved_edits": unsaved,
            "unsaved_script_tabs": snapshot::dirty_code_tab_names(world),
            // Instances stored only in the database are not compared.
            "database_backed": checkpoint::is_db_backed(&root),
        }))
    })();
    answer(req, result)
}

/// Start `snapshot.revert`. Answered once the reopened Space has applied or
/// withdrawn the plan, from the core's revert pump.
pub fn begin_snapshot_revert(world: &mut World, pending: server::Pending) {
    let server::Pending { request: req, responder } = pending;
    let paths = match req.params.get("paths") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => match a.iter().map(|v| v.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>() {
            Some(list) => Some(list),
            None => {
                let _ = responder.send(BridgeResponse::error(
                    req.id.clone(),
                    BridgeError::invalid_params("'paths' holds only strings"),
                ));
                return;
            }
        },
        Some(_) => {
            let _ = responder.send(BridgeResponse::error(
                req.id.clone(),
                BridgeError::invalid_params("'paths' is a list of paths relative to the Space"),
            ));
            return;
        }
    };
    let Some(target) = param_str(&req, "id").map(str::to_owned) else {
        let _ = responder.send(BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params("missing 'id': the snapshot to revert to"),
        ));
        return;
    };

    // The responder moves into the reply; a refusal before the revert starts
    // answers through the same slot.
    let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(responder)));
    let id = req.id.clone();
    let reply_slot = slot.clone();
    let reply_id = id.clone();
    let reply: snapshot::RevertReply = Box::new(move |_world: &mut World, outcome: &snapshot::RevertOutcome| {
        let response = match &outcome.result {
            Ok(done) => BridgeResponse::ok(
                reply_id,
                json!({
                    "reverted_to": outcome.target,
                    "paths": outcome.paths,
                    // Revert to this to undo the revert.
                    "safety_snapshot": outcome.safety,
                    // The commit recording the revert, or null when it failed.
                    "audit_commit": done.audit_commit,
                    // True only when the wait timed out with the load unsettled.
                    "still_loading": done.still_loading,
                }),
            ),
            Err(e) => BridgeResponse::error(reply_id, BridgeError::internal(e.clone())),
        };
        if let Some(responder) = reply_slot.lock().ok().and_then(|mut s| s.take()) {
            if responder.send(response).is_err() {
                debug!("EngineBridge: snapshot.revert caller disconnected before it finished");
            }
        }
    });
    if let Err(e) = snapshot::start_revert(world, &target, paths.as_deref(), caller(&req), reply) {
        if let Some(responder) = slot.lock().ok().and_then(|mut s| s.take()) {
            let _ = responder.send(BridgeResponse::error(id, BridgeError::invalid_params(e)));
        }
    }
}

/// `snapshot.cancel_revert`
pub fn snapshot_cancel_revert(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    answer(req, snapshot::cancel_pending(world).map(|cancelled| json!({ "cancelled": cancelled })))
}
