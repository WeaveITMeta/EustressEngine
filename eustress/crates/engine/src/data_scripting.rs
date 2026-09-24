//! Scripts reach the Data Platform.
//!
//! Rune's `eustress::data` module and Luau's `DataService` are two faces of one
//! implementation, [`data_call`], over the data crate's JSON front door
//! (`eustress_data::mine::api`): the requests the `mine_data` agent tool and the
//! `eustress data` command take, answered the same way. A script can mine a
//! file in its Space, ask graph questions of an edge list, or read a
//! Connector's rows with a Cypher, SQL or GraphQL statement of its own.
//!
//! ```lua
//! local DataService = game:GetService("DataService")
//! local late = DataService:Mine({ run = "classify", file = "Datasets/deliveries.csv", target = "late" })
//! print(DataService:Render(late))
//! for _, row in ipairs(DataService:Query("SupplierGraph", "MATCH (s:Supplier)-[:SUPPLIES]->(k:SKU) RETURN s.id AS supplier, k.id AS sku")) do
//!     print(row.supplier, row.sku)
//! end
//! ```
//!
//! Files resolve inside the open Space and nowhere else. A Connector must be
//! enabled before a script can read it, and its rows are kept for a few
//! seconds, so a script that asks every frame reaches the source a handful of
//! times a minute rather than sixty times a second. Reads block the calling
//! script, so a large query belongs in a script's setup, not its frame loop.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use eustress_data::mine::api;
use eustress_data::source::connector::{read_connector, ConnectorRead};
use eustress_data::{DataError, Frame};
use serde_json::{json, Value};

/// The open Space, for resolving script file names and Connectors. Written by
/// [`track_script_space`]; read from whichever thread a script runs on.
static SCRIPT_SPACE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// How long a Connector's rows are reused before a script's query reads the
/// source again.
const QUERY_TTL: Duration = Duration::from_secs(5);
/// The most rows one script query returns.
const QUERY_ROW_CAP: usize = 10_000;

struct CachedRows {
    connector: String,
    statement: Option<String>,
    at: Instant,
    rows: Value,
}

static QUERY_CACHE: Mutex<Vec<CachedRows>> = Mutex::new(Vec::new());

fn open_space() -> Option<PathBuf> {
    SCRIPT_SPACE.read().ok().and_then(|slot| slot.clone())
}

/// A script's file name, inside the Space. Absolute paths, drive letters and
/// `..` segments are refused rather than resolved.
fn resolve_in(space: Option<&Path>, name: &str) -> eustress_data::Result<PathBuf> {
    let space = space.ok_or_else(|| DataError::Schema(format!("`{name}` cannot be read: no Space is open")))?;
    let cleaned = name.replace('\\', "/");
    if cleaned.split('/').any(|segment| segment == "..") || cleaned.starts_with('/') || cleaned.contains(':') {
        return Err(DataError::Schema(format!("`{name}` is outside the Space")));
    }
    Ok(space.join(cleaned))
}

/// Read one of the Space's Connectors, the way every automated reader does:
/// through the data crate's shared rules, enabled Connectors only.
pub fn read_space_connector(space: &Path, name: &str, statement: Option<&str>) -> eustress_data::Result<Frame> {
    let attributes = eustress_common::parameters::connector_attributes(space, name).map_err(DataError::Schema)?;
    read_connector(name, attributes, &ConnectorRead { space_root: space, statement, require_enabled: true })
}

/// The one implementation behind both script languages.
///
/// - `mine`: a front-door request, answered with its reply.
/// - `query`: `{ connector, statement }`, answered with the Connector's rows as
///   a list of row objects. An empty or missing statement reads the query the
///   Connector is configured with.
/// - `render`: a reply, answered with its readable text.
pub fn data_call(op: &str, arg: Value) -> Result<Value, String> {
    match op {
        "mine" => {
            let space = open_space();
            let resolve = |name: &str| resolve_in(space.as_deref(), name);
            let connector = |name: &str, statement: Option<&str>| match space.as_deref() {
                Some(root) => read_space_connector(root, name, statement),
                None => Err(DataError::Schema(format!("Connector '{name}' cannot be read: no Space is open"))),
            };
            api::run_in(&arg, &api::Env { resolve: &resolve, connector: Some(&connector) }).map_err(|e| e.to_string())
        }
        "render" => Ok(Value::String(api::render(&arg))),
        "query" => {
            let connector = arg
                .get("connector")
                .and_then(Value::as_str)
                .filter(|c| !c.trim().is_empty())
                .ok_or("a query needs the name of a Connector")?
                .to_string();
            let statement =
                arg.get("statement").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
            query_rows(&connector, statement)
        }
        other => Err(format!("DataService has no operation '{other}'")),
    }
}

fn query_rows(connector: &str, statement: Option<String>) -> Result<Value, String> {
    if let Ok(cache) = QUERY_CACHE.lock() {
        let hit = cache
            .iter()
            .find(|c| c.connector == connector && c.statement == statement && c.at.elapsed() < QUERY_TTL);
        if let Some(hit) = hit {
            return Ok(hit.rows.clone());
        }
    }
    let reply = data_call(
        "mine",
        json!({ "run": "rows", "connector": connector, "statement": statement, "limit": QUERY_ROW_CAP }),
    )?;
    let rows = reply["report"]["rows"].clone();
    if let Ok(mut cache) = QUERY_CACHE.lock() {
        cache.retain(|c| c.at.elapsed() < QUERY_TTL && !(c.connector == connector && c.statement == statement));
        cache.push(CachedRows { connector: connector.to_string(), statement, at: Instant::now(), rows: rows.clone() });
    }
    Ok(rows)
}

/// Keep [`SCRIPT_SPACE`] on the open Space, and drop cached rows from the
/// previous one on a switch.
fn track_script_space(space_root: Option<Res<crate::space::SpaceRoot>>) {
    let Some(root) = space_root else { return };
    if !root.is_changed() {
        return;
    }
    if let Ok(mut slot) = SCRIPT_SPACE.write() {
        *slot = Some(root.0.clone());
    }
    if let Ok(mut cache) = QUERY_CACHE.lock() {
        cache.clear();
    }
}

/// Give Luau's `DataService` its implementation.
fn install_luau_data_service() {
    eustress_common::luau::play::instance::set_data_service(std::sync::Arc::new(data_call));
}

/// Installs the Luau `DataService` and keeps scripts pointed at the open Space.
/// The Rune module registers with the others, in `soul::rune_api`.
pub struct DataScriptingPlugin;

impl Plugin for DataScriptingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, install_luau_data_service)
            .add_systems(Update, track_script_space);
    }
}

/// Rune's `eustress::data` module.
///
/// ```rune
/// use eustress::data;
///
/// let reply = data::mine(#{ run: "rank", file: "Datasets/deliveries.csv", target: "late" })?;
/// log_info(data::render(reply)?);
/// let rows = data::query("Warehouse", "SELECT sku, qty FROM stock WHERE qty < 10")?;
/// ```
#[cfg(feature = "realism-scripting")]
pub mod rune_data {
    use rune::{ContextError, Module};
    use serde_json::{json, Value};

    /// A request as JSON. A string is read as JSON text, so a request built as
    /// text works as well as an object.
    fn to_json(value: rune::Value) -> Result<Value, String> {
        if let Ok(text) = rune::from_value::<String>(value.clone()) {
            return serde_json::from_str(&text).map_err(|e| format!("the request is not valid JSON: {e}"));
        }
        serde_json::to_value(&value).map_err(|e| format!("the request cannot be read: {e}"))
    }

    fn to_rune(reply: Value) -> Result<rune::Value, String> {
        serde_json::from_value(reply).map_err(|e| format!("the reply cannot be handed to Rune: {e}"))
    }

    /// `data::mine(request)`: run a request through the Data Platform's front
    /// door (describe, rows, graph, classify, regress, compare, rank,
    /// selection_check, forward_select, lasso, cluster, pca, rules,
    /// residuals) and return the reply, cautions included.
    #[rune::function]
    fn mine(request: rune::Value) -> Result<rune::Value, String> {
        to_rune(super::data_call("mine", to_json(request)?)?)
    }

    /// `data::describe(file)`: every column of a Space file, with its type,
    /// missing values and summary statistics.
    #[rune::function]
    fn describe(file: &str) -> Result<rune::Value, String> {
        to_rune(super::data_call("mine", json!({ "run": "describe", "file": file }))?)
    }

    /// `data::query(connector, statement)`: a Connector's rows. The statement
    /// (Cypher, SQL or GraphQL) replaces the Connector's configured query; an
    /// empty one reads the configured query.
    #[rune::function]
    fn query(connector: &str, statement: &str) -> Result<rune::Value, String> {
        to_rune(super::data_call("query", json!({ "connector": connector, "statement": statement }))?)
    }

    /// `data::render(reply)`: a reply as readable text.
    #[rune::function]
    fn render(reply: rune::Value) -> Result<String, String> {
        match super::data_call("render", to_json(reply)?)? {
            Value::String(text) => Ok(text),
            other => Ok(other.to_string()),
        }
    }

    /// The `eustress::data` module.
    pub fn create_data_module() -> Result<Module, ContextError> {
        let mut m = Module::with_crate_item("eustress", ["data"])?;
        m.function_meta(mine)?;
        m.function_meta(describe)?;
        m.function_meta(query)?;
        m.function_meta(render)?;
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_files_stay_inside_the_space() {
        let space = Path::new("/spaces/Plant");
        assert_eq!(resolve_in(Some(space), "Datasets/a.csv").unwrap(), space.join("Datasets/a.csv"));
        for bad in ["../other/a.csv", "Datasets/../../a.csv", "/etc/passwd", "C:/a.csv", "..\\a.csv"] {
            assert!(resolve_in(Some(space), bad).is_err(), "{bad}");
        }
        assert!(resolve_in(None, "a.csv").unwrap_err().to_string().contains("no Space"));
    }

    #[test]
    fn inline_rows_need_no_space() {
        let reply = data_call("mine", json!({ "run": "describe", "data": [{ "a": 1 }, { "a": 2 }] })).unwrap();
        assert_eq!(reply["report"]["rows"], 2);
        let text = data_call("render", reply).unwrap();
        assert!(text.as_str().unwrap().contains("2 rows"));
    }

    #[test]
    fn a_query_names_its_connector() {
        let e = data_call("query", json!({ "statement": "MATCH (n) RETURN n" })).unwrap_err();
        assert!(e.contains("Connector"), "{e}");
        assert!(data_call("teleport", json!({})).is_err());
    }
}
