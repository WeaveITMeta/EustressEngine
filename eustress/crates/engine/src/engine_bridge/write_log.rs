//! The engine bridge's write log: one JSON line for every bridge call that
//! can change something, so that with several agents driving one Studio the
//! person can see which connection changed what.
//!
//! An accountability record, not tamper-proof evidence: it lives in the
//! person's own data folder and their own processes can edit it. It holds
//! what was done to what, never a value: the time (UTC), the Universe, the
//! method, the target (uuid, path or name), each parameter's name and JSON
//! type (an object's keys too), the result (ok, or the error code only: an
//! error's text can repeat what it refused), the connection's number, and
//! the name the client declared,
//! marked unverified.
//!
//! The file is `<local data>/Eustress/bridge-log/writes.log`, outside every
//! Universe (a Universe folder can be a git repository, and agents' file
//! tools reach it), in a folder only the user can open
//! ([`eustress_identity::private_file`]). It is appended to, and rotated to
//! `writes.1.log` past 1 MB, so it holds at most about 2 MB.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use super::protocol::{BridgeRequest, BridgeResponse, MethodName};

/// Size past which the log is rotated.
const MAX_BYTES: u64 = 1 << 20;

/// Whether `method` can change the Space, the editor's state or files: every
/// such call is logged.
pub(super) fn is_write(method: &MethodName) -> bool {
    use MethodName as M;
    matches!(
        method,
        M::EntityCreate
            | M::EntityUpdate
            | M::EntityDelete
            | M::EntityAddTag
            | M::EntityRemoveTag
            | M::EntityPromote
            | M::EntityDemote
            | M::PropertiesSet
            | M::ParametersSet
            | M::ClassCallMethod
            | M::AttributeSet
            | M::TransformSubtree
            | M::InstanceRename
            | M::InstanceReparent
            | M::InstanceClone
            | M::InstanceCreate
            | M::SpaceOpen
            | M::AssetsReloadMesh
            | M::DataBind
            | M::DataUnbind
            | M::DbExportToml
            | M::SnapshotSave
            | M::SnapshotRevert
            | M::SnapshotCancelRevert
            | M::PublishSubmit
            | M::PhysicsSet
            | M::SimSet
            | M::SimRun
            | M::SimPause
            | M::SimStop
            | M::ActionInvoke
            | M::ToolsCall
            | M::UiInvokeTool
            | M::UiClick
            | M::UiSetMode
            | M::InputInject
            | M::EngineShutdown
            | M::TerrainGenerate
            | M::TerrainFlat
            | M::TerrainSculpt
            | M::TerrainPaint
            | M::TerrainFill
            | M::TerrainReplaceMaterial
            | M::TerrainLayerCreate
            | M::TerrainClear
            | M::TerrainReload
    )
}

/// A JSON value's type name, never its content.
fn type_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// The keys that name a call's target.
const TARGET_KEYS: &[&str] = &["uuid", "path", "name", "selected", "parent_uuid", "new_parent_uuid", "class"];

/// What a request's parameters were, as names and types only: each
/// top-level parameter, and one level of keys inside an object parameter.
pub(super) fn shape(params: &Value) -> Value {
    let Some(map) = params.as_object() else { return json!(type_of(params)) };
    let mut out = Map::new();
    for (k, v) in map {
        if TARGET_KEYS.contains(&k.as_str()) {
            continue;
        }
        let entry = match v {
            Value::Object(inner) => {
                Value::Object(inner.iter().map(|(ik, iv)| (ik.clone(), json!(type_of(iv)))).collect())
            }
            other => json!(type_of(other)),
        };
        out.insert(k.clone(), entry);
    }
    Value::Object(out)
}

/// The call's target: the identifying parameters it was given.
pub(super) fn target(params: &Value) -> Value {
    let Some(map) = params.as_object() else { return Value::Null };
    Value::Object(
        TARGET_KEYS
            .iter()
            .filter_map(|k| map.get(*k).filter(|v| !v.is_object() && !v.is_array()).map(|v| (k.to_string(), v.clone())))
            .collect(),
    )
}

/// One log line for `req`, answered by `response` (`None`: accepted and
/// answered later).
pub(super) fn line(req: &BridgeRequest, response: Option<&BridgeResponse>, universe: Option<&Path>) -> String {
    let result = match response {
        None => json!("accepted"),
        Some(r) => match &r.error {
            None => json!("ok"),
            // The code only: a message can quote the value it refused.
            Some(e) => json!({ "error": e.code }),
        },
    };
    let mut entry = json!({
        "time": chrono::Utc::now().to_rfc3339(),
        "universe": universe.map(|u| u.display().to_string()),
        "method": format!("{:?}", req.method),
        "target": target(&req.params),
        "params": shape(&req.params),
        "result": result,
        "connection": req.conn,
    });
    if req.params.get("expose_to_clients").and_then(Value::as_bool) == Some(true) {
        entry["expose_to_clients"] = json!(true);
    }
    if let Some(client) = &req.client {
        let shown: String = client.chars().filter(|c| !c.is_control()).take(80).collect();
        entry["client"] = json!({ "name": shown, "verified": false });
    }
    entry.to_string()
}

/// Whether the log file has been made private and checked this run.
static CHECKED: Mutex<bool> = Mutex::new(false);

/// Append `line` to the log in `dir`, making the folder private and checking
/// the file the first time, and rotating past [`MAX_BYTES`].
pub(super) fn append_in(dir: &Path, line: &str, checked: &mut bool) -> std::io::Result<()> {
    use eustress_identity::private_file;
    let path = dir.join("writes.log");
    if !*checked {
        private_file::private_dir(dir)?;
        if !path.exists() {
            private_file::write_private(&path, b"")?;
        }
        private_file::verify(&path)?;
        *checked = true;
    }
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        std::fs::rename(&path, dir.join("writes.1.log"))?;
        private_file::write_private(&path, b"")?;
    }
    let mut file = match std::fs::OpenOptions::new().append(true).open(&path) {
        Ok(f) => f,
        // Deleted or moved since it was checked: make it again, privately,
        // and check it, rather than stop logging until a restart.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            *checked = false;
            private_file::private_dir(dir)?;
            private_file::write_private(&path, b"")?;
            private_file::verify(&path)?;
            *checked = true;
            std::fs::OpenOptions::new().append(true).open(&path)?
        }
        Err(e) => return Err(e),
    };
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")
}

/// The log's folder for this user.
fn log_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Eustress").join("bridge-log"))
}

/// Log `req` if it is a write. A failure to log is reported once per call in
/// the engine log and never fails the call itself.
pub(super) fn note(world: &bevy::prelude::World, req: &BridgeRequest, response: Option<&BridgeResponse>) {
    if !is_write(&req.method) {
        return;
    }
    let universe = world
        .get_resource::<crate::space::SpaceRoot>()
        .and_then(|r| r.0.parent().and_then(Path::parent).map(Path::to_path_buf));
    let entry = line(req, response, universe.as_deref());
    let Some(dir) = log_dir() else { return };
    let mut checked = match CHECKED.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Err(e) = append_in(&dir, &entry, &mut checked) {
        tracing::warn!("bridge write log: could not record a {:?} call ({e})", req.method);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, params: Value) -> BridgeRequest {
        let mut req: BridgeRequest =
            serde_json::from_value(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params, "client": "agent-x" }))
                .unwrap();
        req.conn = 7;
        req
    }

    /// A line names the call and the shape of what it was given, never a
    /// value.
    #[test]
    fn a_line_holds_names_and_types_never_values() {
        let req = request(
            "attribute.set",
            json!({ "uuid": "u-1", "values": { "ApiKey": "sk-secret-value", "Speed": 4812337 }, "replace_type": true }),
        );
        let text = line(&req, Some(&BridgeResponse::ok(json!(1), json!({}))), None);
        assert!(!text.contains("sk-secret-value"), "{text}");
        assert!(!text.contains("4812337"), "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["method"], "AttributeSet");
        assert_eq!(v["target"]["uuid"], "u-1");
        assert_eq!(v["params"]["values"]["ApiKey"], "string");
        assert_eq!(v["params"]["values"]["Speed"], "number");
        assert_eq!(v["result"], "ok");
        assert_eq!(v["connection"], 7);
        assert_eq!(v["client"], json!({ "name": "agent-x", "verified": false }));
    }

    #[test]
    fn a_refusal_and_expose_to_clients_are_recorded() {
        let req = request("instance.transform_subtree", json!({ "uuid": "u", "expose_to_clients": true }));
        // An error's text can quote the value it refused: only the code goes in.
        let refused = BridgeResponse::error(
            json!(1),
            super::super::protocol::BridgeError::invalid_params("refused 'sk-secret-value' for ApiKey"),
        );
        let text = line(&req, Some(&refused), None);
        assert!(!text.contains("sk-secret-value"), "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["result"], json!({ "error": -32602 }));
        assert_eq!(v["expose_to_clients"], true);
        assert_eq!(serde_json::from_str::<Value>(&line(&req, None, None)).unwrap()["result"], "accepted");
    }

    #[test]
    fn reads_are_not_logged_and_writes_are() {
        assert!(is_write(&MethodName::AttributeSet));
        assert!(is_write(&MethodName::PropertiesSet));
        assert!(!is_write(&MethodName::AttributeGet));
        assert!(!is_write(&MethodName::Ping));
    }

    /// A log file left readable to others (written some other way) is
    /// refused, not appended to.
    #[test]
    fn a_log_file_that_is_not_private_is_refused() {
        let dir = std::env::temp_dir().join(format!("eus_writelog_open_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        eustress_identity::private_file::private_dir(&dir).unwrap();
        // Written the ordinary way: the folder's inherited list, not a protected one.
        std::fs::write(dir.join("writes.log"), "").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(dir.join("writes.log"), std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
        let mut checked = false;
        assert!(append_in(&dir, "{}", &mut checked).is_err());
        assert!(!checked);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A log deleted mid-run is made again, privately, and logging goes on.
    #[test]
    fn a_deleted_log_is_made_again() {
        let dir = std::env::temp_dir().join(format!("eus_writelog_gone_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut checked = false;
        append_in(&dir, "{\"a\":1}", &mut checked).unwrap();
        std::fs::remove_file(dir.join("writes.log")).unwrap();
        append_in(&dir, "{\"a\":2}", &mut checked).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("writes.log")).unwrap(), "{\"a\":2}
");
        eustress_identity::private_file::verify(&dir.join("writes.log")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The log is appended to, kept private, and rotated past its size.
    #[test]
    fn the_log_is_private_appended_and_rotated() {
        let dir = std::env::temp_dir().join(format!("eus_writelog_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut checked = false;
        append_in(&dir, "{\"a\":1}", &mut checked).unwrap();
        append_in(&dir, "{\"a\":2}", &mut checked).unwrap();
        assert!(checked);
        eustress_identity::private_file::verify(&dir.join("writes.log")).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("writes.log")).unwrap(), "{\"a\":1}\n{\"a\":2}\n");
        std::fs::write(dir.join("writes.log"), vec![b'x'; (MAX_BYTES + 1) as usize]).unwrap();
        append_in(&dir, "{\"a\":3}", &mut checked).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("writes.log")).unwrap(), "{\"a\":3}\n");
        assert!(std::fs::metadata(dir.join("writes.1.log")).unwrap().len() > MAX_BYTES);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
