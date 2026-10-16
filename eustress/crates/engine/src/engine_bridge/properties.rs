//! The engine bridge's `properties.*` and `class.call_method` methods: read
//! and set any instance's properties, and call its class's methods, over
//! JSON-RPC. The MCP `get_properties`, `set_properties` and `call_method`
//! tools call them. The work is `crate::property_service`'s; this module
//! names the instances and shapes the replies.

use bevy::prelude::*;
use serde_json::{json, Value};

use eustress_common::class_methods::{self, MethodEffect};

use super::parameters::{resolve, target_json, Target};
use super::{BridgeError, BridgeRequest, BridgeResponse};
use crate::property_service::{self as service, ClassMethodImpls};

/// What `properties.get` cannot read or set yet, said in its reply.
const NOT_COVERED: &str = "Not covered here yet: service properties (select the service in the Explorer), \
     Bloom and SunRays, and renaming (Name). Parameters are read and set with parameters_get / \
     parameters_set, and Attributes with attribute_get / attribute_set.";

/// Refused during Play and while the Space loads; `attribute.set` shares it.
pub(super) fn editing_only(world: &World) -> Result<(), String> {
    if world
        .get_resource::<State<crate::play_mode::PlayModeState>>()
        .is_some_and(|s| *s.get() != crate::play_mode::PlayModeState::Editing)
    {
        return Err("the editor is in Play. Stop first: an edit during Play would be saved while Stop \
                    restores the scene from before Play"
            .to_string());
    }
    if world
        .get_resource::<crate::space::file_loader::LoadInProgress>()
        .is_some_and(|l| l.active)
    {
        return Err("the Space is still loading; try again when it has settled".to_string());
    }
    Ok(())
}

fn answer(req: &BridgeRequest, result: Result<Value, String>) -> BridgeResponse {
    match result {
        Ok(v) => BridgeResponse::ok(req.id.clone(), v),
        Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
    }
}

/// The methods a class declares, each with whether this build implements it.
fn methods_json(world: &World, class: &str) -> Vec<Value> {
    let impls = world.get_resource::<ClassMethodImpls>();
    class_methods::methods_of(class)
        .map(|m| {
            json!({
                "method": format!("{}:{}", m.class, m.name),
                "doc": m.doc,
                "params": class_methods::params_schema(m),
                "returns": m.returns,
                "effect": format!("{:?}", m.effect),
                "available": impls.is_some_and(|i| i.get(m.class, m.name).is_some()),
            })
        })
        .collect()
}

/// `properties.get`: every property of one instance, with its category,
/// type, value text, whether it can be set, and its unit, description and
/// choices where its class has them; and its class's methods.
pub fn properties_get(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let result = (|| -> Result<Value, String> {
        let t = resolve(world, &req.params)?;
        let rows = service::describe(world, t.entity)?;
        Ok(json!({
            "instance": target_json(&t),
            "count": rows.len(),
            "properties": rows.iter().map(service::Row::json).collect::<Vec<_>>(),
            "methods": methods_json(world, &t.class),
            "not_covered": NOT_COVERED,
        }))
    })();
    answer(req, result)
}

/// One `{path|uuid|name|selected, properties: {Name: value}}` entry.
fn entry(world: &mut World, params: &Value) -> Result<(Target, Vec<(String, Value)>), String> {
    let t = resolve(world, params)?;
    let props = params
        .get("properties")
        .and_then(Value::as_object)
        .ok_or("`properties` must be an object of {Name: value}")?;
    if props.is_empty() {
        return Err(format!("no properties given for {}", t.name));
    }
    Ok((t, props.iter().map(|(k, v)| (k.clone(), v.clone())).collect()))
}

/// `properties.set`: set properties on one instance (`path`/`uuid`/`name`/
/// `selected` plus `properties`) or several (`changes`: a list of those),
/// all or nothing, as one undo step.
pub fn properties_set(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let result = (|| -> Result<Value, String> {
        editing_only(world)?;
        let entries: Vec<(Target, Vec<(String, Value)>)> = match req.params.get("changes") {
            Some(Value::Array(list)) => {
                if list.is_empty() {
                    return Err("`changes` is empty".to_string());
                }
                list.iter()
                    .enumerate()
                    .map(|(i, item)| entry(world, item).map_err(|e| format!("changes[{i}]: {e}")))
                    .collect::<Result<_, _>>()?
            }
            Some(_) => return Err("`changes` must be a list of {path|uuid|name, properties}".to_string()),
            None => vec![entry(world, &req.params)?],
        };
        let targets: Vec<(Entity, Vec<(String, Value)>)> =
            entries.iter().map(|(t, props)| (t.entity, props.clone())).collect();
        let report = service::set(world, &targets)?;
        let changed: Vec<Value> = entries
            .iter()
            .map(|(t, _)| {
                let names: Vec<&String> =
                    report.changed.iter().filter(|(e, _)| *e == t.entity).map(|(_, n)| n).collect();
                json!({ "instance": target_json(t), "changed": names })
            })
            .collect();
        Ok(json!({
            "changed": changed,
            "undo_steps": report.undo_steps,
            "not_undoable": report.not_undoable,
        }))
    })();
    answer(req, result)
}

/// `class.call_method`: call a declared method on one instance. `method` is
/// `"Class:Name"`, so who may call it is known from the call alone; the
/// instance must be of that class.
pub fn class_call_method(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let result = (|| -> Result<Value, String> {
        let method = req.params.get("method").and_then(Value::as_str).ok_or("`method` (\"Class:Name\") is required")?;
        let (class, name) = class_methods::split_qualified(method)
            .ok_or_else(|| format!("`method` is \"Class:Name\", such as \"Road:Split\"; got {method:?}"))?;
        let decl = class_methods::declared(class, name).ok_or_else(|| format!("{class} declares no method {name}"))?;
        if decl.effect != MethodEffect::Read {
            editing_only(world)?;
        }
        let t = resolve(world, &req.params)?;
        if t.class != class {
            return Err(format!("{} is a {}, not a {class}", t.name, t.class));
        }
        let call = world
            .get_resource::<ClassMethodImpls>()
            .and_then(|i| i.get(class, name))
            .ok_or_else(|| format!("{class}:{name} is declared but this build does not implement it yet"))?;
        let args = req.params.get("args").cloned().unwrap_or_else(|| json!({}));
        let returned = call(world, t.entity, &args)?;
        Ok(json!({ "instance": target_json(&t), "method": method, "returned": returned }))
    })();
    answer(req, result)
}
