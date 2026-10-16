//! The engine bridge's `parameters.*` methods: read and edit an instance's
//! Parameters (its `[parameters]`, labelled `key` in the default domain and
//! `domain.key` in any other) over JSON-RPC. The MCP `parameters_get` and
//! `parameters_set` tools call them.
//!
//! An edit runs through the Properties panel's own rules
//! ([`runtime::edit_parameter_value`] for a value, [`runtime::apply_parameter_draft`]
//! to add one, change its type or bind it to a Connector) and is saved by the
//! same system: into the world database's text, the instance file, or the
//! binary mirror. Like an edit in a Properties row, it is not an undo step.
//! The one difference from the row: an empty value never deletes a Parameter
//! here; this method only adds and changes.

use bevy::prelude::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use eustress_common::classes::Instance;
use eustress_common::parameters::{parameter_label, InstanceParameters, ParameterBinding, ParameterValue};

use super::{BridgeError, BridgeRequest, BridgeResponse};
use crate::space::parameters_runtime::{self as runtime, ParametersSynced};
use crate::space::InstanceFile;

// ---------------------------------------------------------------------------
// Finding the instance
// ---------------------------------------------------------------------------

pub(super) struct Target {
    pub(super) entity: Entity,
    pub(super) name: String,
    pub(super) class: String,
    uuid: String,
    /// The instance record, absolute.
    file: Option<PathBuf>,
    /// The same, relative to the Space, with `/`.
    path: Option<String>,
}

fn str_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// The Space-relative forms one instance record answers to: the record itself
/// (`Workspace/Tower/_instance.toml`, `Workspace/Base.part.toml`), its folder
/// (`Workspace/Tower`), and a flat record without its extensions
/// (`Workspace/Base`).
fn path_forms(rel: &str) -> Vec<String> {
    let mut forms = vec![rel.to_string()];
    if let Some(folder) = rel.strip_suffix("/_instance.toml") {
        forms.push(folder.to_string());
    } else if let Some((dir, file)) = rel.rsplit_once('/') {
        if let Some(stem) = file.split('.').next().filter(|s| !s.is_empty()) {
            forms.push(format!("{dir}/{stem}"));
        }
    }
    forms
}

fn normalise(path: &str) -> String {
    path.trim().replace('\\', "/").trim_matches('/').to_string()
}

/// The instance a request names: `path` (relative to the Space), `uuid`,
/// `name`, or `selected: true` for the Explorer's selection. A name that more
/// than one instance has is refused with their paths, so the caller can pick.
pub(super) fn resolve(world: &mut World, params: &Value) -> Result<Target, String> {
    let space_root = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone());
    let rel_of = |file: &Path| -> Option<String> {
        let root = space_root.as_ref()?;
        file.strip_prefix(root).ok().map(|p| normalise(&p.to_string_lossy()))
    };

    let selected = if params.get("selected").and_then(Value::as_bool).unwrap_or(false) {
        let explorer = world.get_resource::<crate::ui::slint_ui::UnifiedExplorerState>();
        match explorer.map(|e| &e.selected) {
            Some(crate::ui::slint_ui::SelectedItem::Entity(e)) => Some(*e),
            _ => return Err("nothing is selected in the Explorer".to_string()),
        }
    } else {
        None
    };
    let uuid = str_param(params, "uuid");
    let path = str_param(params, "path").map(normalise);
    let name = str_param(params, "name");
    if selected.is_none() && uuid.is_none() && path.is_none() && name.is_none() {
        return Err("name the instance: `path` (relative to the Space, such as Workspace/Tower), \
                    `uuid`, `name`, or `selected: true`"
            .to_string());
    }

    let mut q = world.query::<(Entity, &Instance, Option<&InstanceFile>)>();
    let hits: Vec<Target> = q
        .iter(world)
        .filter(|(entity, inst, file)| {
            if let Some(e) = selected {
                return *entity == e;
            }
            if let Some(u) = uuid {
                return inst.uuid == u;
            }
            if let Some(p) = &path {
                return file
                    .and_then(|f| rel_of(&f.toml_path))
                    .is_some_and(|rel| path_forms(&rel).iter().any(|form| form == p));
            }
            name.is_some_and(|n| inst.name == n)
        })
        .map(|(entity, inst, file)| Target {
            entity,
            name: inst.name.clone(),
            class: inst.class_name.as_str().to_string(),
            uuid: inst.uuid.clone(),
            file: file.map(|f| f.toml_path.clone()),
            path: file.and_then(|f| rel_of(&f.toml_path)),
        })
        .collect();

    match hits.len() {
        1 => Ok(hits.into_iter().next().expect("one hit")),
        0 => Err(match (uuid, &path, name) {
            (Some(u), _, _) => format!("no instance has uuid '{u}'"),
            (_, Some(p), _) => format!("no instance is at '{p}' in this Space"),
            (_, _, Some(n)) => format!("no instance is named '{n}'"),
            _ => "the selection is not an instance".to_string(),
        }),
        n => {
            let shown: Vec<String> = hits
                .iter()
                .take(10)
                .map(|t| t.path.clone().unwrap_or_else(|| format!("uuid {}", t.uuid)))
                .collect();
            Err(format!(
                "{n} instances match: {}{}. Pass `path` or `uuid` to choose one",
                shown.join(", "),
                if n > shown.len() { ", ..." } else { "" }
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Describing Parameters
// ---------------------------------------------------------------------------

fn value_json(value: &ParameterValue) -> Value {
    match value {
        ParameterValue::Bool(b) => json!(b),
        ParameterValue::Int(i) => json!(i),
        ParameterValue::Float(f) => json!(f),
        ParameterValue::String(s) => json!(s),
        ParameterValue::Vector3(v) => json!(v),
        ParameterValue::Color(c) => json!(c),
        ParameterValue::EntityRef(e) => json!(e),
        // Shown parsed when it is valid JSON, else as the text it holds.
        ParameterValue::Json(text) => serde_json::from_str(text).unwrap_or_else(|_| json!(text)),
        ParameterValue::Binary(bytes) => json!({ "bytes": bytes.len() }),
    }
}

fn binding_json(binding: &ParameterBinding) -> Value {
    json!({
        "connector": binding.connector,
        "field": binding.field,
        "refresh_seconds": binding.refresh_seconds,
        "publish": binding.publish,
        "last_read_s": binding.last_read_s,
        "last_error": binding.last_error,
        // A Connector owns a sourced value; parameters.set refuses to edit it.
        "read_only": binding.value_is_read_only(),
        "description": runtime::describe_binding(binding),
    })
}

fn row(params: &InstanceParameters, domain: &str, key: &str, value: &ParameterValue) -> Value {
    json!({
        "label": parameter_label(domain, key),
        "domain": domain,
        "key": key,
        "type": value.type_name(),
        "value": value_json(value),
        // The form the Properties row shows, and parameters.set accepts.
        "text": value.edit_string(),
        "state": format!("{:?}", params.binding_state(domain, key)),
        "binding": params.binding(domain, key).map(binding_json),
    })
}

fn rows(params: &InstanceParameters, domain_filter: Option<&str>) -> Vec<Value> {
    let mut out: Vec<(String, String, Value)> = Vec::new();
    for (domain, keys) in &params.domains {
        if domain_filter.is_some_and(|d| d != domain) {
            continue;
        }
        for (key, value) in keys {
            out.push((domain.clone(), key.clone(), row(params, domain, key, value)));
        }
    }
    out.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    out.into_iter().map(|(_, _, v)| v).collect()
}

pub(super) fn target_json(t: &Target) -> Value {
    json!({ "name": t.name, "class": t.class, "uuid": t.uuid, "path": t.path })
}

/// A binary-ECS instance keeps its Parameters in its core, not in a record a
/// reader can open.
fn readable_record(t: &Target) -> Option<&Path> {
    t.file.as_deref().filter(|p| !p.to_string_lossy().contains("__bin_"))
}

// ---------------------------------------------------------------------------
// parameters.get
// ---------------------------------------------------------------------------

/// `parameters.get`: an instance's Parameters, each with its type, value and
/// binding. Reads; changes nothing. An instance whose Parameters were not
/// loaded yet is shown with what its record holds, as selecting it would.
pub fn parameters_get(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let result = (|| -> Result<Value, String> {
        let t = resolve(world, &req.params)?;
        let mut params = world.get::<InstanceParameters>(t.entity).cloned().unwrap_or_default();
        let synced = world.get::<ParametersSynced>(t.entity).is_some();
        if !synced {
            if let Some(record) = readable_record(&t) {
                runtime::sync_from_store(&mut params, record);
            }
        }
        let rows = rows(&params, str_param(&req.params, "domain"));
        Ok(json!({ "instance": target_json(&t), "count": rows.len(), "parameters": rows }))
    })();
    match result {
        Ok(v) => BridgeResponse::ok(req.id.clone(), v),
        Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
    }
}

// ---------------------------------------------------------------------------
// parameters.set
// ---------------------------------------------------------------------------

/// The text form of a JSON value for a Parameter of type `type_name`, which
/// the Properties row's own parser then reads.
fn value_text(value: &Value, type_name: &str) -> Result<String, String> {
    let wrong = || format!("{value} is not a {type_name}");
    match (type_name, value) {
        ("Bool", Value::Bool(b)) => Ok(b.to_string()),
        // The row reads any other text as false; refuse it instead.
        ("Bool", Value::String(s)) => match s.trim() {
            "true" | "True" | "1" => Ok("true".into()),
            "false" | "False" | "0" => Ok("false".into()),
            _ => Err(wrong()),
        },
        ("Int" | "Float" | "EntityRef", Value::Number(n)) => Ok(n.to_string()),
        ("Vector3" | "Color", Value::Array(items)) => items
            .iter()
            .map(|v| v.as_f64().map(|n| n.to_string()).ok_or_else(wrong))
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join(", ")),
        ("String", Value::Number(n)) => Ok(n.to_string()),
        ("String", Value::Bool(b)) => Ok(b.to_string()),
        // Any JSON is a Json value; its text is what is stored.
        ("Json", Value::String(s)) => Ok(s.clone()),
        ("Json", v) => Ok(v.to_string()),
        (_, Value::String(s)) => Ok(s.clone()),
        _ => Err(wrong()),
    }
}

/// Load the instance's stored Parameters into its component before an edit,
/// so saving the edit never drops ones the component had not read: what the
/// Properties panel does before its edits.
fn ensure_synced(world: &mut World, t: &Target) {
    if world.get::<InstanceParameters>(t.entity).is_none() {
        world.entity_mut(t.entity).insert(InstanceParameters::new());
    }
    if world.get::<ParametersSynced>(t.entity).is_some() {
        return;
    }
    let record = readable_record(t).map(Path::to_path_buf);
    let mut entity = world.entity_mut(t.entity);
    let marker = {
        let mut params = entity.get_mut::<InstanceParameters>().expect("inserted above");
        match record {
            Some(record) => runtime::sync_from_store(&mut params, &record),
            None => ParametersSynced { persisted: runtime::persisted_signature(&params) },
        }
    };
    entity.insert(marker);
}

/// `parameters.set`: add or change one Parameter. See the module docs.
pub fn parameters_set(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let result = (|| -> Result<Value, String> {
        let p = &req.params;
        if world
            .get_resource::<State<crate::play_mode::PlayModeState>>()
            .is_some_and(|s| *s.get() != crate::play_mode::PlayModeState::Editing)
        {
            return Err("the editor is in Play. Stop first: an edit during Play would be saved \
                        while Stop restores the scene from before Play"
                .to_string());
        }
        if world
            .get_resource::<crate::space::file_loader::LoadInProgress>()
            .is_some_and(|l| l.active)
        {
            return Err("the Space is still loading; try again when it has settled".to_string());
        }
        let label = str_param(p, "label").ok_or("missing `label`: the Parameter, `key` or `domain.key`")?;
        let (domain, key) = runtime::label_parts(label);
        let label = parameter_label(&domain, &key);
        let type_param = str_param(p, "type");
        let bind = p.get("bind").filter(|v| !v.is_null());
        let unbind = p.get("unbind").and_then(Value::as_bool).unwrap_or(false);
        if bind.is_some() && unbind {
            return Err("pass `bind` or `unbind`, not both".to_string());
        }

        let t = resolve(world, p)?;
        ensure_synced(world, &t);

        let mut params = world.get::<InstanceParameters>(t.entity).cloned().unwrap_or_default();
        let existing = params.get(&domain, &key).cloned();
        let existing_binding = params.binding(&domain, &key).cloned();
        let value = p.get("value").filter(|v| !v.is_null());

        if type_param.is_none() && bind.is_none() && !unbind {
            // A plain value edit, read as the Parameter's own type.
            let Some(current) = &existing else {
                return Err(format!(
                    "{} has no Parameter '{label}'; pass `type` (one of {}) to add it",
                    t.name,
                    ParameterValue::EDITABLE_TYPES.join(", ")
                ));
            };
            let value = value.ok_or("missing `value`")?;
            let text = value_text(value, current.type_name())?;
            if text.trim().is_empty() {
                return Err("an empty value would delete the Parameter; parameters.set never deletes".to_string());
            }
            runtime::edit_parameter_value(&mut params, &label, &text)
                .unwrap_or_else(|| Err(format!("no Parameter '{label}'")))?;
        } else {
            // Adding one, changing its type, or (un)binding it: the dialog's rules.
            let type_name = match (type_param, &existing) {
                (Some(ty), _) => ty.to_string(),
                (None, Some(current)) => current.type_name().to_string(),
                (None, None) => return Err(format!("pass `type` to add '{label}'")),
            };
            let text = match value {
                Some(v) => value_text(v, &type_name)?,
                None => match (&existing, bind) {
                    // A bound Parameter waits for its first read.
                    (_, Some(_)) => String::new(),
                    (Some(current), None) if current.type_name() == type_name => current.edit_string(),
                    _ => return Err("missing `value`".to_string()),
                },
            };
            let still_bound = !unbind && existing_binding.as_ref().is_some_and(ParameterBinding::is_sourced);
            let (bound, connector, field, refresh) = match bind {
                Some(b) => (
                    true,
                    b.get("connector").and_then(Value::as_str).unwrap_or("").to_string(),
                    b.get("field").and_then(Value::as_str).unwrap_or("").to_string(),
                    b.get("refresh_seconds").and_then(Value::as_u64).map(|n| n.to_string()).unwrap_or_default(),
                ),
                None if still_bound => {
                    let b = existing_binding.as_ref().expect("still bound");
                    (
                        true,
                        b.connector.clone().unwrap_or_default(),
                        b.field.clone().unwrap_or_default(),
                        b.refresh_seconds.map(|n| n.to_string()).unwrap_or_default(),
                    )
                }
                None => (false, String::new(), String::new(), String::new()),
            };
            if !bound && text.trim().is_empty() {
                return Err("an empty value would delete the Parameter; parameters.set never deletes".to_string());
            }
            let draft = runtime::ParameterDraft {
                is_edit: existing.is_some(),
                original: if existing.is_some() { label.clone() } else { String::new() },
                domain: domain.clone(),
                name: key.clone(),
                type_name,
                value: text,
                bound,
                connector,
                field,
                refresh,
            };
            runtime::apply_parameter_draft(&mut params, &draft)?;
        }

        // Written back whole, so the save system sees one change and persists it.
        let after = params.get(&domain, &key).map(|v| row(&params, &domain, &key, v));
        *world.get_mut::<InstanceParameters>(t.entity).expect("synced above") = params;
        if let Some(mut dirty) = world.get_resource_mut::<eustress_common::change_queue::PanelDirtyFlags>() {
            dirty.properties = true;
        }
        Ok(json!({
            "instance": target_json(&t),
            "parameter": after,
            "added": existing.is_none(),
            // Saved by the Parameters save system within the next frame.
            "saved": "queued",
        }))
    })();
    match result {
        Ok(v) => BridgeResponse::ok(req.id.clone(), v),
        Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_answers_to_its_folder_and_its_stem() {
        assert_eq!(
            path_forms("Workspace/Tower/_instance.toml"),
            vec!["Workspace/Tower/_instance.toml", "Workspace/Tower"]
        );
        assert_eq!(path_forms("Workspace/Base.part.toml"), vec!["Workspace/Base.part.toml", "Workspace/Base"]);
        assert_eq!(normalise("\\Workspace\\Tower\\"), "Workspace/Tower");
    }

    #[test]
    fn values_are_read_as_their_type_and_ambiguous_bools_are_refused() {
        assert_eq!(value_text(&json!(true), "Bool").unwrap(), "true");
        assert_eq!(value_text(&json!("0"), "Bool").unwrap(), "false");
        assert!(value_text(&json!("maybe"), "Bool").is_err(), "the row would read this as false");
        assert_eq!(value_text(&json!(42), "Int").unwrap(), "42");
        assert_eq!(value_text(&json!([1, 2.5, 3]), "Vector3").unwrap(), "1, 2.5, 3");
        assert!(value_text(&json!([1, "x", 3]), "Vector3").is_err());
        assert_eq!(value_text(&json!({ "a": 1 }), "Json").unwrap(), "{\"a\":1}");
        assert!(value_text(&json!([1, 2]), "Float").is_err());
    }
}
