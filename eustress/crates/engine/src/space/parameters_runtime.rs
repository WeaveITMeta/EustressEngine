//! Instance Parameters at runtime: keeping the component in step with the
//! stored instance, saving edits, and reading bound Parameters from their
//! Connectors.
//!
//! ## The store
//!
//! A file-backed instance keeps `[parameters]` and `[parameter_bindings]` in
//! its instance record: the text form in the world database when one is open,
//! the `_instance.toml` on disk otherwise, through the same funnel as every
//! instance write ([`super::active_db`]). A binary-ECS instance keeps them in
//! its core's cold tail, which the binary save mirror re-bakes when the entity
//! is marked dirty.
//!
//! Parts load their Parameters at spawn. Other classes load them the first
//! time something needs them: the Properties panel showing the instance, or an
//! edit. [`ParametersSynced`] records that an entity's component matches its
//! store, and the save path writes only synced entities, so it never replaces
//! Parameters it has not read.
//!
//! ## Bound Parameters
//!
//! A Parameter bound to a Connector is read through the data crate's shared
//! Connector reader: once when the binding appears, changes or is saved again
//! from the dialog, then every `refresh_seconds`. One fetch serves every
//! Parameter bound to the same Connector, fetches run on worker threads, and a
//! Connector that is not enabled is never read; the Parameter shows why
//! instead. A bound value is not written back to the store on every read: only
//! the binding's configuration is persistent, so a refresh never touches the
//! disk.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use bevy::prelude::*;
use eustress_common::parameters::{
    parameter_label, split_parameter_label, InstanceParameters, ParameterBinding, ParameterValue,
};

use super::file_loader::LoadInProgress;
use super::file_watcher::RecentlyWrittenFiles;
use super::InstanceFile;

/// This entity's `InstanceParameters` match its stored record. Holds the
/// signature of what was last persisted, so the save path writes when an edit
/// changes the persisted form and never for a bound value's refresh.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParametersSynced {
    pub persisted: u64,
}

/// Shortest refresh interval a binding may ask for, in seconds.
pub const MIN_REFRESH_SECONDS: u64 = 5;

/// The signature of what persisting `params` would write. A sourced key
/// contributes its type but not its value, since its value belongs to the
/// source and is re-read rather than stored.
pub fn persisted_signature(params: &InstanceParameters) -> u64 {
    let mut rows: Vec<(String, String)> = Vec::new();
    for (domain, keys) in &params.domains {
        for (key, value) in keys {
            let sourced = params.binding(domain, key).is_some_and(ParameterBinding::is_sourced);
            let shown = if sourced { format!("sourced {}", value.type_name()) } else { value.to_toml().to_string() };
            rows.push((parameter_label(domain, key), shown));
        }
    }
    for (domain, keys) in &params.bindings {
        for (key, binding) in keys {
            rows.push((format!("{} binding", parameter_label(domain, key)), binding.to_toml().to_string()));
        }
    }
    rows.sort();
    let mut h = DefaultHasher::new();
    rows.hash(&mut h);
    h.finish()
}

/// The component and its marker for an instance whose `[parameters]` and
/// `[parameter_bindings]` were just read, as a spawn path inserts them.
pub fn loaded_parameters(
    values: Option<&HashMap<String, toml::Value>>,
    bindings: Option<&toml::Value>,
) -> (InstanceParameters, ParametersSynced) {
    let params = InstanceParameters::from_toml(values, bindings);
    let synced = ParametersSynced { persisted: persisted_signature(&params) };
    (params, synced)
}

/// The Parameters in an instance's stored record: the world database's text
/// when it holds the instance, else the file. `None` when neither can be read.
pub fn read_stored_parameters(path: &Path) -> Option<InstanceParameters> {
    let text = super::active_db::get_instance_text(path).or_else(|| std::fs::read_to_string(path).ok())?;
    let doc: toml::Table = text.parse().ok()?;
    let values: Option<HashMap<String, toml::Value>> = doc
        .get("parameters")
        .and_then(|v| v.as_table())
        .map(|t| t.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    Some(InstanceParameters::from_toml(values.as_ref(), doc.get("parameter_bindings")))
}

/// Bring an unsynced component in step with its store: every stored key the
/// component lacks is added, so nothing on disk is lost when it is next saved.
/// Returns the marker to insert.
pub fn sync_from_store(params: &mut InstanceParameters, path: &Path) -> ParametersSynced {
    if let Some(stored) = read_stored_parameters(path) {
        for (domain, keys) in stored.domains {
            for (key, value) in keys {
                if params.get(&domain, &key).is_none() {
                    params.set(&domain, &key, value);
                }
            }
        }
        for (domain, keys) in stored.bindings {
            for (key, binding) in keys {
                if params.binding(&domain, &key).is_none() {
                    params.bind(&domain, &key, binding);
                }
            }
        }
    }
    ParametersSynced { persisted: persisted_signature(params) }
}

/// Write `[parameters]` and `[parameter_bindings]` into an instance record,
/// leaving every other section as it was. Returns whether the file on disk was
/// written (rather than the world database's copy).
fn patch_parameters(
    path: &Path,
    values: toml::map::Map<String, toml::Value>,
    bindings: toml::map::Map<String, toml::Value>,
) -> Result<bool, String> {
    let text = super::active_db::get_instance_text(path)
        .or_else(|| std::fs::read_to_string(path).ok())
        .ok_or_else(|| format!("no instance record at {}", path.display()))?;
    let mut doc: toml::Table = text.parse().map_err(|e| format!("{}: {e}", path.display()))?;
    if values.is_empty() {
        doc.remove("parameters");
    } else {
        doc.insert("parameters".into(), toml::Value::Table(values));
    }
    if bindings.is_empty() {
        doc.remove("parameter_bindings");
    } else {
        doc.insert("parameter_bindings".into(), toml::Value::Table(bindings));
    }
    if let Some(meta) = doc.get_mut("metadata").and_then(|m| m.as_table_mut()) {
        meta.insert("last_modified".into(), toml::Value::String(chrono::Utc::now().to_rfc3339()));
    }
    let out = toml::to_string_pretty(&doc).map_err(|e| format!("{}: {e}", path.display()))?;
    if super::active_db::put_instance_text(path, &out) {
        return Ok(false);
    }
    super::gui_loader::write_atomic(path, out.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

/// Load the Parameters of whatever the Explorer has selected, when that
/// instance has not been synced yet, so the Properties panel shows what the
/// store holds for any class.
fn sync_selected_parameters(
    mut commands: Commands,
    explorer: Option<Res<crate::ui::slint_ui::UnifiedExplorerState>>,
    mut q: Query<(&InstanceFile, &mut InstanceParameters), Without<ParametersSynced>>,
    mut panel_dirty: Option<ResMut<eustress_common::change_queue::PanelDirtyFlags>>,
) {
    let Some(explorer) = explorer else { return };
    let crate::ui::slint_ui::SelectedItem::Entity(entity) = explorer.selected else { return };
    let Ok((file, mut params)) = q.get_mut(entity) else { return };
    if file.toml_path.to_string_lossy().contains("__bin_") {
        return;
    }
    let marker = sync_from_store(&mut params, &file.toml_path);
    commands.entity(entity).insert(marker);
    // The panel may already have drawn this selection without them.
    if !params.domains.is_empty() || !params.bindings.is_empty() {
        if let Some(d) = panel_dirty.as_mut() {
            d.properties = true;
        }
    }
}

/// Persist edited Parameters. Writes only when the persisted form changed, so
/// a bound value's refresh never reaches the store.
fn save_parameter_changes(
    mut commands: Commands,
    mut q: Query<
        (Entity, &InstanceParameters, &mut ParametersSynced, Option<&InstanceFile>, Has<BinaryPersisted>),
        // A marker inserted this frame (through commands, with the signature
        // from before a same-frame edit) lands after this system has run, so
        // its arrival is what catches that edit.
        Or<(Changed<InstanceParameters>, Added<ParametersSynced>)>,
    >,
    mut recently_written: Option<ResMut<RecentlyWrittenFiles>>,
    load_in_progress: Option<Res<LoadInProgress>>,
) {
    if load_in_progress.is_some_and(|l| l.active) {
        return;
    }
    let mut jobs = Vec::new();
    for (entity, params, mut synced, file, binary) in q.iter_mut() {
        let signature = persisted_signature(params);
        if signature == synced.persisted {
            continue;
        }
        synced.persisted = signature;
        if binary {
            mark_binary_for_save(&mut commands, entity);
            continue;
        }
        let Some(file) = file else { continue };
        if file.toml_path.to_string_lossy().contains("__bin_") {
            continue;
        }
        if let Some(rw) = recently_written.as_mut() {
            rw.mark_written(file.toml_path.clone());
        }
        let (values, bindings) = params.to_toml_tables();
        jobs.push((file.toml_path.clone(), values, bindings));
    }
    if jobs.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for (path, values, bindings) in jobs {
            if let Err(e) = patch_parameters(&path, values, bindings) {
                tracing::error!("Parameters save failed: {e}");
            }
        }
    });
}

/// Marks an entity whose persistence is the binary save mirror.
#[cfg(feature = "world-db")]
type BinaryPersisted = super::world_db_binary::BinaryEcsInstance;
#[cfg(not(feature = "world-db"))]
type BinaryPersisted = NeverBinary;

/// Stand-in marker for builds without the world database, where nothing is
/// binary-persisted. Never inserted.
#[cfg(not(feature = "world-db"))]
#[derive(Component)]
pub struct NeverBinary;

#[cfg(feature = "world-db")]
fn mark_binary_for_save(commands: &mut Commands, entity: Entity) {
    super::world_db_binary::mark_binary_dirty(commands, entity);
}

#[cfg(not(feature = "world-db"))]
fn mark_binary_for_save(_commands: &mut Commands, _entity: Entity) {}

// ─────────────────────────────────────────────────────────────────────────────
// Bound Parameters
// ─────────────────────────────────────────────────────────────────────────────

/// One bound key waiting on a fetch.
#[derive(Debug, Clone)]
struct Wanted {
    entity: Entity,
    domain: String,
    key: String,
    field: String,
    type_name: String,
}

/// A finished fetch of one Connector, with a result for every key that waited
/// on it.
struct ConnectorOutcome {
    connector: String,
    results: Vec<(Entity, String, String, Result<ParameterValue, String>)>,
}

/// Scheduling state for bound reads.
#[derive(Resource)]
pub struct ParameterReads {
    tx: std::sync::mpsc::Sender<ConnectorOutcome>,
    /// Wrapped because `mpsc::Receiver` is `Send` but not `Sync`.
    rx: std::sync::Mutex<std::sync::mpsc::Receiver<ConnectorOutcome>>,
    /// Connectors with a fetch in flight, so a slow source is not re-entered.
    inflight: HashSet<String>,
    /// Seconds since startup at which each `(entity, label)` is next due.
    next_due: HashMap<(Entity, String), f64>,
    /// Each binding's configuration as last scheduled, so a new or changed
    /// binding reads at once.
    seen: HashMap<(Entity, String), u64>,
    seconds_to_scan: f32,
}

impl Default for ParameterReads {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            tx,
            rx: std::sync::Mutex::new(rx),
            inflight: HashSet::new(),
            next_due: HashMap::new(),
            seen: HashMap::new(),
            seconds_to_scan: 0.0,
        }
    }
}

fn binding_signature(binding: &ParameterBinding) -> u64 {
    let mut h = DefaultHasher::new();
    (&binding.connector, &binding.field, binding.refresh_seconds).hash(&mut h);
    h.finish()
}

/// Once a second: find bound keys that are due (new, changed, or past their
/// refresh), group them by Connector, and fetch each Connector once on a
/// worker thread.
fn schedule_parameter_reads(
    time: Res<Time>,
    space_root: Option<Res<super::SpaceRoot>>,
    mut reads: ResMut<ParameterReads>,
    q: Query<(Entity, &InstanceParameters)>,
) {
    reads.seconds_to_scan -= time.delta_secs();
    if reads.seconds_to_scan > 0.0 {
        return;
    }
    reads.seconds_to_scan = 1.0;
    let Some(space_root) = space_root else { return };
    let now = time.elapsed_secs_f64();
    let mut live: HashSet<(Entity, String)> = HashSet::new();
    let mut groups: HashMap<String, Vec<Wanted>> = HashMap::new();
    for (entity, params) in q.iter() {
        for (domain, keys) in &params.bindings {
            for (key, binding) in keys {
                let (Some(connector), Some(field)) = (
                    binding.connector.as_deref().filter(|c| !c.trim().is_empty()),
                    binding.field.as_deref().filter(|f| !f.trim().is_empty()),
                ) else {
                    continue;
                };
                let id = (entity, parameter_label(domain, key));
                live.insert(id.clone());
                let signature = binding_signature(binding);
                let changed = reads.seen.get(&id) != Some(&signature);
                // Neither read nor failed yet: a new binding, or one just saved
                // from the dialog (which clears both), so a Save also retries a
                // read that failed. A fetch in flight is not repeated (below).
                let unread = binding.last_read_s.is_none() && binding.last_error.is_none();
                let due = changed
                    || unread
                    || binding.refresh_seconds.is_some() && reads.next_due.get(&id).is_none_or(|t| now >= *t);
                if !due || reads.inflight.contains(connector) {
                    continue;
                }
                reads.seen.insert(id.clone(), signature);
                match binding.refresh_seconds {
                    Some(s) => {
                        reads.next_due.insert(id, now + s.max(MIN_REFRESH_SECONDS) as f64);
                    }
                    None => {
                        reads.next_due.remove(&id);
                    }
                }
                let type_name = params.get(domain, key).map_or("Float", ParameterValue::type_name).to_string();
                groups.entry(connector.to_string()).or_default().push(Wanted {
                    entity,
                    domain: domain.clone(),
                    key: key.clone(),
                    field: field.to_string(),
                    type_name,
                });
            }
        }
    }
    reads.seen.retain(|id, _| live.contains(id));
    reads.next_due.retain(|id, _| live.contains(id));
    for (connector, wanted) in groups {
        reads.inflight.insert(connector.clone());
        let tx = reads.tx.clone();
        let space = space_root.0.clone();
        std::thread::spawn(move || {
            let fetched = fetch_connector(&space, &connector);
            let results = wanted
                .into_iter()
                .map(|w| {
                    let value = fetched.as_ref().map_err(Clone::clone).and_then(|frame| read_field(frame, &w));
                    (w.entity, w.domain, w.key, value)
                })
                .collect();
            let _ = tx.send(ConnectorOutcome { connector, results });
        });
    }
}

/// Apply finished fetches: a value is recorded as read, a failure as the
/// binding's fault, which the Properties row then shows. A read that lands on
/// the selected instance marks the panel dirty, so the row updates in place.
fn apply_parameter_reads(
    time: Res<Time>,
    mut reads: ResMut<ParameterReads>,
    mut q: Query<&mut InstanceParameters>,
    explorer: Option<Res<crate::ui::slint_ui::UnifiedExplorerState>>,
    mut panel_dirty: Option<ResMut<eustress_common::change_queue::PanelDirtyFlags>>,
) {
    let outcomes: Vec<ConnectorOutcome> = match reads.rx.lock() {
        Ok(rx) => rx.try_iter().collect(),
        Err(_) => return,
    };
    let now = time.elapsed_secs_f64();
    let selected = explorer.as_ref().and_then(|e| match e.selected {
        crate::ui::slint_ui::SelectedItem::Entity(entity) => Some(entity),
        _ => None,
    });
    for outcome in outcomes {
        reads.inflight.remove(&outcome.connector);
        for (entity, domain, key, result) in outcome.results {
            let Ok(mut params) = q.get_mut(entity) else { continue };
            match result {
                Ok(value) => params.record_read(&domain, &key, value, now),
                Err(error) => params.record_error(&domain, &key, error),
            }
            if selected == Some(entity) {
                if let Some(d) = panel_dirty.as_mut() {
                    d.properties = true;
                }
            }
        }
    }
}

#[cfg(feature = "data")]
type Fetched = eustress_data::Frame;
#[cfg(not(feature = "data"))]
type Fetched = ();

#[cfg(feature = "data")]
fn fetch_connector(space: &Path, connector: &str) -> Result<Fetched, String> {
    use eustress_data::source::connector::{read_connector, ConnectorRead};
    let attributes = eustress_common::parameters::connector_attributes(space, connector)?;
    read_connector(connector, attributes, &ConnectorRead { space_root: space, statement: None, require_enabled: true })
        .map_err(|e| e.to_string())
}

#[cfg(not(feature = "data"))]
fn fetch_connector(_space: &Path, connector: &str) -> Result<Fetched, String> {
    Err(format!("this build cannot read Connectors, so '{connector}' is not read"))
}

/// The field's most recent value (its last non-empty row), as the key's type.
#[cfg(feature = "data")]
fn read_field(frame: &Fetched, w: &Wanted) -> Result<ParameterValue, String> {
    use eustress_data::ColumnData;
    let column = frame.column(&w.field).ok_or_else(|| {
        let names: Vec<String> = frame.specs().map(|s| s.name.clone()).collect();
        format!("the source has no field `{}`; it has: {}", w.field, names.join(", "))
    })?;
    let (number, text): (Option<f64>, String) = match column {
        ColumnData::F64(v) => {
            let x = v.iter().rev().flatten().copied().find(|x| x.is_finite()).ok_or("the field has no values")?;
            (Some(x), format!("{x}"))
        }
        ColumnData::I64(v) => {
            let x = v.iter().rev().flatten().copied().next().ok_or("the field has no values")?;
            (Some(x as f64), x.to_string())
        }
        ColumnData::Bool(v) => {
            let b = v.iter().rev().flatten().copied().next().ok_or("the field has no values")?;
            (Some(if b { 1.0 } else { 0.0 }), b.to_string())
        }
        ColumnData::Str(v) => {
            let s = v.iter().rev().flatten().next().ok_or("the field has no values")?;
            (s.trim().parse::<f64>().ok(), s.clone())
        }
    };
    // The error names the field and the type, never the source's data.
    let mismatch = || format!("the latest value of `{}` is not a {}", w.field, w.type_name);
    Ok(match w.type_name.as_str() {
        "Float" => ParameterValue::Float(number.ok_or_else(mismatch)?),
        "Int" => {
            let x = number.filter(|x| x.fract() == 0.0 && x.abs() < 9.0e15).ok_or_else(mismatch)?;
            ParameterValue::Int(x as i64)
        }
        "Bool" => ParameterValue::Bool(match text.trim() {
            "true" | "True" | "TRUE" | "1" | "yes" => true,
            "false" | "False" | "FALSE" | "0" | "no" => false,
            _ => return Err(mismatch()),
        }),
        "String" => ParameterValue::String(text),
        other => ParameterValue::parse(other, &text).ok_or_else(mismatch)?,
    })
}

#[cfg(not(feature = "data"))]
fn read_field(_frame: &Fetched, _w: &Wanted) -> Result<ParameterValue, String> {
    Err("this build cannot read Connectors".to_string())
}

/// How a bound Parameter's row explains itself: where the value comes from,
/// how often it is read, and the last failure when there is one.
pub fn describe_binding(binding: &ParameterBinding) -> String {
    let mut text = match (&binding.connector, &binding.field) {
        (Some(c), Some(f)) if binding.is_sourced() => format!("Read from Connector '{c}', field `{f}`"),
        (Some(c), None) if binding.is_sourced() => format!("Bound to Connector '{c}' with no field to read"),
        _ => "Owned by this instance".to_string(),
    };
    if binding.is_sourced() {
        match binding.refresh_seconds {
            Some(s) => text.push_str(&format!(", every {} s.", s.max(MIN_REFRESH_SECONDS))),
            None => text.push_str(", when the binding is set."),
        }
        if binding.last_read_s.is_none() && binding.last_error.is_none() {
            text.push_str(" Not read yet this session.");
        }
    }
    if binding.publish {
        text.push_str(" Changes publish to the domain's export targets.");
    }
    if let Some(e) = &binding.last_error {
        text.push_str(&format!(" Last read failed: {e}"));
    }
    text
}

/// A label's `(domain, key)`, owned.
pub fn label_parts(label: &str) -> (String, String) {
    let (domain, key) = split_parameter_label(label);
    (domain.to_string(), key.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// The Add/Edit Parameter modal
// ─────────────────────────────────────────────────────────────────────────────

/// What the Add/Edit Parameter modal holds when confirmed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParameterDraft {
    pub is_edit: bool,
    /// The label being edited; empty when adding.
    pub original: String,
    pub domain: String,
    pub name: String,
    pub type_name: String,
    pub value: String,
    /// Read the value from a Connector.
    pub bound: bool,
    pub connector: String,
    pub field: String,
    /// Seconds between reads; blank reads once.
    pub refresh: String,
}

/// A type's value before anything is known: what a bound Parameter holds
/// until its first read.
fn empty_value(type_name: &str) -> Option<ParameterValue> {
    Some(match type_name {
        "Float" => ParameterValue::Float(0.0),
        "Int" => ParameterValue::Int(0),
        "Bool" => ParameterValue::Bool(false),
        "String" => ParameterValue::String(String::new()),
        "Vector3" => ParameterValue::Vector3([0.0; 3]),
        "Color" => ParameterValue::Color([0.0, 0.0, 0.0, 1.0]),
        "EntityRef" => ParameterValue::EntityRef(None),
        "Json" => ParameterValue::Json("null".into()),
        _ => return None,
    })
}

/// Apply a confirmed draft to an instance's Parameters. Returns the label it
/// set, or the reason it refused, worded for the modal's error line; nothing is
/// changed on a refusal.
pub fn apply_parameter_draft(params: &mut InstanceParameters, draft: &ParameterDraft) -> Result<String, String> {
    use eustress_common::parameters::DEFAULT_PARAMETER_DOMAIN;

    let domain = match draft.domain.trim() {
        "" => DEFAULT_PARAMETER_DOMAIN,
        d => d,
    };
    let key = draft.name.trim();
    if key.is_empty() {
        return Err("Give the Parameter a name.".into());
    }
    if domain.contains('.') || key.contains('.') {
        return Err("Names cannot contain a dot: a dot separates a domain from its key.".into());
    }
    if !ParameterValue::EDITABLE_TYPES.contains(&draft.type_name.as_str()) {
        return Err(format!("'{}' is not a Parameter type.", draft.type_name));
    }
    let label = parameter_label(domain, key);
    let replacing_itself = draft.is_edit && draft.original == label;
    if !replacing_itself && params.get(domain, key).is_some() {
        return Err(format!("'{label}' already exists on this instance; edit that one instead."));
    }

    let text = draft.value.trim();
    let value = if text.is_empty() && draft.bound {
        empty_value(&draft.type_name).ok_or_else(|| format!("'{}' has no empty value.", draft.type_name))?
    } else {
        ParameterValue::parse(&draft.type_name, text).ok_or_else(|| match draft.type_name.as_str() {
            "Vector3" => format!("\"{text}\" is not a Vector3; write three numbers, x, y, z."),
            "Color" => format!("\"{text}\" is not a Color; write four numbers from 0 to 1, r, g, b, a."),
            "Int" => format!("\"{text}\" is not a whole number."),
            "Float" => format!("\"{text}\" is not a number."),
            other => format!("\"{text}\" is not a {other}."),
        })?
    };

    let binding = if draft.bound {
        let connector = draft.connector.trim();
        let field = draft.field.trim();
        if connector.is_empty() {
            return Err("Choose the Connector to read from.".into());
        }
        if field.is_empty() {
            return Err("Name the field to read, such as a column of the Connector's table.".into());
        }
        let refresh_seconds = match draft.refresh.trim() {
            "" => None,
            s => match s.parse::<u64>() {
                Ok(n) if n >= MIN_REFRESH_SECONDS => Some(n),
                _ => {
                    return Err(format!(
                        "Refresh is a whole number of seconds, {MIN_REFRESH_SECONDS} or more; leave it blank to read once."
                    ))
                }
            },
        };
        Some((connector.to_string(), field.to_string(), refresh_seconds))
    } else {
        None
    };

    // Everything checked: apply. A rename or a move to another domain carries
    // the old binding's publish setting across.
    let mut previous = None;
    if draft.is_edit && !draft.original.is_empty() {
        let (old_domain, old_key) = label_parts(&draft.original);
        previous = params.binding(&old_domain, &old_key).cloned();
        if draft.original != label {
            params.remove(&old_domain, &old_key);
        }
    }
    params.set(domain, key, value);
    match binding {
        Some((connector, field, refresh_seconds)) => {
            let publish = previous.as_ref().is_some_and(|b| b.publish);
            params.bind(
                domain,
                key,
                ParameterBinding {
                    connector: Some(connector),
                    field: Some(field),
                    refresh_seconds,
                    publish,
                    last_read_s: None,
                    last_error: None,
                },
            );
        }
        None => {
            params.unbind(domain, key);
        }
    }
    Ok(label)
}

/// Edit a Parameter row in place: the text is read as the row's own type, and
/// clearing the field deletes the Parameter (the Attribute rows' contract).
/// `None` when the instance has no such Parameter.
pub fn edit_parameter_value(params: &mut InstanceParameters, label: &str, text: &str) -> Option<Result<(), String>> {
    let (domain, key) = label_parts(label);
    let current = params.get(&domain, &key)?.clone();
    if params.binding(&domain, &key).is_some_and(ParameterBinding::value_is_read_only) {
        return Some(Err(format!("'{label}' is read from a Connector; turn its binding off to edit the value")));
    }
    if text.trim().is_empty() {
        params.remove(&domain, &key);
        return Some(Ok(()));
    }
    Some(match ParameterValue::parse(current.type_name(), text) {
        Some(value) => {
            params.set(&domain, &key, value);
            Ok(())
        }
        None => Err(format!("\"{}\" is not a {}", text.trim(), current.type_name())),
    })
}

/// Keeps Parameters in step with their store, saves edits, and reads bound
/// Parameters from their Connectors.
pub struct ParametersRuntimePlugin;

impl Plugin for ParametersRuntimePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParameterReads>().add_systems(
            Update,
            (
                sync_selected_parameters,
                save_parameter_changes,
                apply_parameter_reads,
                schedule_parameter_reads.after(apply_parameter_reads),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::parameters::DEFAULT_PARAMETER_DOMAIN;

    fn bound(refresh: Option<u64>) -> ParameterBinding {
        ParameterBinding {
            connector: Some("Sensors".into()),
            field: Some("temp_c".into()),
            refresh_seconds: refresh,
            ..Default::default()
        }
    }

    #[test]
    fn a_bound_value_refresh_leaves_the_signature_alone() {
        let mut p = InstanceParameters::new();
        p.set("hvac", "temperature", ParameterValue::Float(20.0));
        p.bind("hvac", "temperature", bound(Some(30)));
        let before = persisted_signature(&p);
        p.record_read("hvac", "temperature", ParameterValue::Float(23.5), 10.0);
        assert_eq!(persisted_signature(&p), before, "a read is not an edit");
        p.bind("hvac", "temperature", bound(Some(60)));
        assert_ne!(persisted_signature(&p), before, "a new refresh interval is");
    }

    #[test]
    fn a_local_value_edit_changes_the_signature() {
        let mut p = InstanceParameters::new();
        p.set(DEFAULT_PARAMETER_DOMAIN, "setpoint", ParameterValue::Float(20.0));
        let before = persisted_signature(&p);
        p.set(DEFAULT_PARAMETER_DOMAIN, "setpoint", ParameterValue::Float(21.0));
        assert_ne!(persisted_signature(&p), before);
    }

    fn draft(domain: &str, name: &str, type_name: &str, value: &str) -> ParameterDraft {
        ParameterDraft {
            domain: domain.into(),
            name: name.into(),
            type_name: type_name.into(),
            value: value.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_draft_adds_a_typed_parameter_in_its_domain() {
        let mut p = InstanceParameters::new();
        assert_eq!(apply_parameter_draft(&mut p, &draft("", "setpoint", "Float", "21.5")), Ok("setpoint".into()));
        assert_eq!(p.get(DEFAULT_PARAMETER_DOMAIN, "setpoint"), Some(&ParameterValue::Float(21.5)));
        assert_eq!(apply_parameter_draft(&mut p, &draft("hvac", "zone", "String", "north")), Ok("hvac.zone".into()));
        assert_eq!(p.get("hvac", "zone"), Some(&ParameterValue::String("north".into())));
    }

    #[test]
    fn a_draft_is_refused_with_a_reason_and_nothing_changes() {
        let mut p = InstanceParameters::new();
        apply_parameter_draft(&mut p, &draft("", "setpoint", "Float", "21.5")).unwrap();
        for (d, why) in [
            (draft("", "", "Float", "1"), "name"),
            (draft("hvac.zone", "t", "Float", "1"), "dot"),
            (draft("", "setpoint", "Float", "2"), "already exists"),
            (draft("", "offset", "Vector3", "1, 2"), "three numbers"),
            (draft("", "count", "Int", "2.5"), "whole number"),
            (draft("", "blob", "Binary", "00"), "not a Parameter type"),
        ] {
            let e = apply_parameter_draft(&mut p, &d).unwrap_err();
            assert!(e.contains(why), "{d:?}: {e}");
        }
        assert_eq!(p.domains.values().map(|k| k.len()).sum::<usize>(), 1, "only the first add landed");
    }

    #[test]
    fn a_bound_draft_needs_a_connector_a_field_and_a_sane_refresh() {
        let mut p = InstanceParameters::new();
        let mut d = draft("hvac", "temperature", "Float", "");
        d.bound = true;
        assert!(apply_parameter_draft(&mut p, &d).unwrap_err().contains("Connector"));
        d.connector = "Sensors".into();
        assert!(apply_parameter_draft(&mut p, &d).unwrap_err().contains("field"));
        d.field = "temp_c".into();
        d.refresh = "2".into();
        assert!(apply_parameter_draft(&mut p, &d).unwrap_err().contains("5 or more"));
        d.refresh = "30".into();
        assert_eq!(apply_parameter_draft(&mut p, &d), Ok("hvac.temperature".into()));
        assert_eq!(p.get("hvac", "temperature"), Some(&ParameterValue::Float(0.0)), "empty until the first read");
        let b = p.binding("hvac", "temperature").unwrap();
        assert_eq!((b.connector.as_deref(), b.field.as_deref(), b.refresh_seconds), (Some("Sensors"), Some("temp_c"), Some(30)));
    }

    #[test]
    fn an_edit_can_rename_move_and_unbind() {
        let mut p = InstanceParameters::new();
        let mut d = draft("hvac", "temperature", "Float", "20");
        d.bound = true;
        d.connector = "Sensors".into();
        d.field = "temp_c".into();
        apply_parameter_draft(&mut p, &d).unwrap();

        // Move it to another domain and stop reading it.
        let mut e = draft("plant", "temperature", "Float", "22");
        e.is_edit = true;
        e.original = "hvac.temperature".into();
        assert_eq!(apply_parameter_draft(&mut p, &e), Ok("plant.temperature".into()));
        assert!(p.get("hvac", "temperature").is_none() && p.binding("hvac", "temperature").is_none());
        assert_eq!(p.get("plant", "temperature"), Some(&ParameterValue::Float(22.0)));
        assert!(p.binding("plant", "temperature").is_none());

        // Saving an edit under its own label is not a duplicate.
        let mut same = draft("plant", "temperature", "Float", "23");
        same.is_edit = true;
        same.original = "plant.temperature".into();
        assert!(apply_parameter_draft(&mut p, &same).is_ok());
    }

    #[test]
    fn a_row_edit_keeps_the_type_and_refuses_a_sourced_value() {
        let mut p = InstanceParameters::new();
        p.set("hvac", "setpoint", ParameterValue::Float(20.0));
        assert_eq!(edit_parameter_value(&mut p, "hvac.setpoint", "21"), Some(Ok(())));
        assert_eq!(p.get("hvac", "setpoint"), Some(&ParameterValue::Float(21.0)));
        assert!(edit_parameter_value(&mut p, "hvac.setpoint", "warm").unwrap().is_err());
        assert!(edit_parameter_value(&mut p, "hvac.missing", "1").is_none());
        p.bind("hvac", "setpoint", bound(None));
        assert!(edit_parameter_value(&mut p, "hvac.setpoint", "25").unwrap().unwrap_err().contains("Connector"));
        p.unbind("hvac", "setpoint");
        assert_eq!(edit_parameter_value(&mut p, "hvac.setpoint", "  "), Some(Ok(())), "clearing deletes");
        assert!(p.get("hvac", "setpoint").is_none());
    }

    #[test]
    fn a_binding_explains_itself() {
        let mut b = bound(Some(2));
        assert!(describe_binding(&b).contains("every 5 s"), "the floor applies: {}", describe_binding(&b));
        b.last_error = Some("Connector 'Sensors' is disabled".into());
        assert!(describe_binding(&b).contains("disabled"));
        assert_eq!(describe_binding(&ParameterBinding::default()), "Owned by this instance");
    }

    #[cfg(feature = "data")]
    #[test]
    fn a_field_reads_its_latest_value_as_the_keys_type() {
        use eustress_data::{frame_from_columns, ColumnData, ColumnDtype, ColumnSpec};
        let frame = frame_from_columns(vec![
            (ColumnSpec::new("temp_c", ColumnDtype::F64), ColumnData::F64(vec![Some(20.0), Some(21.5), None])),
            (ColumnSpec::new("count", ColumnDtype::I64), ColumnData::I64(vec![Some(3), Some(4), Some(5)])),
            (ColumnSpec::new("state", ColumnDtype::Str), ColumnData::Str(vec![Some("off".into()), None, Some("on".into())])),
        ])
        .unwrap();
        let want = |field: &str, type_name: &str| Wanted {
            entity: Entity::PLACEHOLDER,
            domain: "d".into(),
            key: "k".into(),
            field: field.into(),
            type_name: type_name.into(),
        };
        assert_eq!(read_field(&frame, &want("temp_c", "Float")), Ok(ParameterValue::Float(21.5)));
        assert_eq!(read_field(&frame, &want("count", "Int")), Ok(ParameterValue::Int(5)));
        assert_eq!(read_field(&frame, &want("count", "String")), Ok(ParameterValue::String("5".into())));
        assert!(read_field(&frame, &want("state", "Float")).unwrap_err().contains("not a Float"));
        assert!(read_field(&frame, &want("pressure", "Float")).unwrap_err().contains("temp_c"));
    }
}
