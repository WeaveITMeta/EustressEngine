//! The engine bridge's `datamodel.list`: on command, the instances of the
//! running Play DataModel under a root, so an agent can see what scripts made
//! (`Instance.new`, `Clone`) and how a script-built assembly is joined. The
//! MCP `list_play_instances` tool calls it.
//!
//! Read-only and on command only: it locks the Play DataModel for the one
//! walk a call asks for and does nothing per frame. Each row is an instance's
//! id, name, class, parent id and origin; a joint also names the two bodies
//! it holds: `Part0` and `Part1` for a weld or a no-collision constraint, and
//! the parents of `Attachment0` and `Attachment1` for the attachment-based
//! constraints (hinge, prismatic, spring, ...).

use bevy::prelude::*;
use serde_json::{json, Value};

use eustress_common::datamodel::{DataModel, InstanceId, Origin};
use eustress_common::play_session::PlayDataModel;

use super::{BridgeError, BridgeRequest, BridgeResponse};

/// Rows a call returns when it does not say.
const DEFAULT_LIMIT: usize = 2_000;
/// Rows a call may ask for.
const MAX_LIMIT: usize = 20_000;
/// Longest name a row carries, in bytes: a script can name an instance
/// anything, and a reply must stay bounded.
const MAX_NAME: usize = 256;

/// `name` cut to [`MAX_NAME`] bytes on a character boundary.
fn short(name: &str) -> &str {
    if name.len() <= MAX_NAME {
        return name;
    }
    let mut end = MAX_NAME;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// One body a joint holds: its id and name.
fn body_json(dm: &DataModel, id: Option<InstanceId>) -> Value {
    match id.filter(|i| dm.exists(*i)) {
        Some(i) => json!({ "id": i.0.to_string(), "name": short(dm.name_of(i).unwrap_or_default()) }),
        None => Value::Null,
    }
}

/// The two bodies a joint holds, if it is one: `Part0`/`Part1` for a weld or
/// a no-collision constraint, the parents of `Attachment0`/`Attachment1` for
/// an attachment-based constraint.
fn bodies(dm: &DataModel, id: InstanceId) -> Option<(Value, Value)> {
    let inst = dm.get(id)?;
    let as_id = |name: &str| inst.prop(name).and_then(|v| v.as_instance());
    if inst.prop("Part0").is_some() || inst.prop("Part1").is_some() {
        return Some((body_json(dm, as_id("Part0")), body_json(dm, as_id("Part1"))));
    }
    if inst.prop("Attachment0").is_some() || inst.prop("Attachment1").is_some() {
        let parent_of = |a: Option<InstanceId>| a.and_then(|a| dm.parent(a));
        return Some((body_json(dm, parent_of(as_id("Attachment0"))), body_json(dm, parent_of(as_id("Attachment1")))));
    }
    None
}

/// The instance a dotted `root` names, from `game`: `Workspace`,
/// `Workspace.PlayerCars`, `Workspace.PlayerCars.Coupe`.
fn find_root(dm: &DataModel, root: &str) -> Result<InstanceId, String> {
    let mut cur = dm.root();
    for (i, part) in root.split('.').map(str::trim).filter(|p| !p.is_empty()).enumerate() {
        // The first name may be a service, found by its class.
        let service = if i == 0 { dm.find_service(part) } else { None };
        cur = service
            .or_else(|| dm.find_first_child(cur, part, false))
            .ok_or_else(|| format!("nothing named '{part}' on the way to '{root}' in the Play DataModel"))?;
    }
    Ok(cur)
}

/// The listing for `params` from `dm`. Separate from the bridge so a test can
/// build its own DataModel.
fn list(dm: &DataModel, params: &Value) -> Result<Value, String> {
    let root_name = params.get("root").and_then(Value::as_str).unwrap_or("Workspace");
    let root = find_root(dm, root_name)?;
    let only_scripts = match params.get("origin").and_then(Value::as_str).unwrap_or("script") {
        "script" => true,
        "all" => false,
        other => return Err(format!("`origin` is \"script\" or \"all\", not \"{other}\"")),
    };
    let class = params.get("class").and_then(Value::as_str).filter(|c| !c.is_empty());
    let offset = params.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(DEFAULT_LIMIT, |l| (l as usize).min(MAX_LIMIT));

    let matching: Vec<InstanceId> = dm
        .descendants(root)
        .into_iter()
        .filter(|id| {
            dm.get(*id).is_some_and(|i| {
                !i.destroyed
                    && (!only_scripts || i.origin == Origin::Script)
                    && class.is_none_or(|c| i.is_a(c))
            })
        })
        .collect();
    let rows: Vec<Value> = matching
        .iter()
        .skip(offset)
        .take(limit)
        .filter_map(|id| {
            let inst = dm.get(*id)?;
            let mut row = json!({
                "id": id.0.to_string(),
                "name": short(&inst.name),
                "class": inst.class_name,
                "parent": inst.parent.map(|p| p.0.to_string()),
                "origin": if inst.origin == Origin::Script { "script" } else { "scene" },
            });
            if let Some((b0, b1)) = bodies(dm, *id) {
                row["body0"] = b0;
                row["body1"] = b1;
            }
            Some(row)
        })
        .collect();
    Ok(json!({
        "playing": true,
        "root": root_name,
        "structure_version": dm.structure_version,
        "total": matching.len(),
        "offset": offset,
        "count": rows.len(),
        // More rows than this page holds: page on with `offset`.
        "truncated": offset + rows.len() < matching.len(),
        "rows": rows,
    }))
}

/// `datamodel.list`: see the module docs. Outside Play there is no Play
/// DataModel, and the reply says so.
pub fn datamodel_list(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let Some(dm) = world.get_resource::<PlayDataModel>().map(|p| p.dm.clone()) else {
        return BridgeResponse::ok(
            req.id.clone(),
            json!({ "playing": false, "rows": [], "note": "not in Play: script-made instances exist only while Play runs" }),
        );
    };
    let result = list(&dm.lock(), &req.params);
    match result {
        Ok(v) => BridgeResponse::ok(req.id.clone(), v),
        Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::datamodel::DmValue;

    fn named(dm: &mut DataModel, class: &str, name: &str, parent: InstanceId) -> InstanceId {
        let id = dm.create(class);
        dm.get_mut(id).unwrap().name = name.into();
        dm.set_parent(id, Some(parent)).unwrap();
        id
    }

    /// A car a script built under Workspace.PlayerCars: two parts welded, a
    /// hinge between two attachments, and a scene part the default listing
    /// leaves out.
    fn car() -> (DataModel, InstanceId, InstanceId) {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").expect("a Workspace");
        let scene = named(&mut dm, "Part", "Baseplate", ws);
        dm.get_mut(scene).unwrap().origin = Origin::Scene;
        let cars = named(&mut dm, "Folder", "PlayerCars", ws);
        let model = named(&mut dm, "Model", "Coupe", cars);
        let body = named(&mut dm, "Part", "Body", model);
        let wheel = named(&mut dm, "Part", "WheelFL", model);
        let weld = named(&mut dm, "WeldConstraint", "BodyWeld", model);
        dm.set_prop(weld, "Part0", DmValue::Instance(body)).unwrap();
        dm.set_prop(weld, "Part1", DmValue::Instance(wheel)).unwrap();
        let a0 = named(&mut dm, "Attachment", "A0", body);
        let a1 = named(&mut dm, "Attachment", "A1", wheel);
        let hinge = named(&mut dm, "HingeConstraint", "Axle", model);
        dm.set_prop(hinge, "Attachment0", DmValue::Instance(a0)).unwrap();
        dm.set_prop(hinge, "Attachment1", DmValue::Instance(a1)).unwrap();
        (dm, weld, hinge)
    }

    fn row<'a>(v: &'a Value, name: &str) -> &'a Value {
        v["rows"].as_array().unwrap().iter().find(|r| r["name"] == name).unwrap_or_else(|| panic!("no row {name}: {v}"))
    }

    #[test]
    fn script_made_instances_are_listed_with_their_parents() {
        let (dm, _, _) = car();
        let v = list(&dm, &json!({ "root": "Workspace" })).unwrap();
        let names: Vec<&str> = v["rows"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
        assert!(!names.contains(&"Baseplate"), "the scene part is not script-made: {names:?}");
        assert!(names.contains(&"Coupe") && names.contains(&"WheelFL"), "{names:?}");
        assert_eq!(row(&v, "Body")["parent"], row(&v, "Coupe")["id"]);
        assert_eq!(row(&v, "Body")["class"], "Part");
        assert_eq!(v["structure_version"], json!(dm.structure_version));
        let all = list(&dm, &json!({ "root": "Workspace", "origin": "all" })).unwrap();
        assert_eq!(row(&all, "Baseplate")["origin"], "scene");
    }

    /// A name a script made huge is cut, so one row stays small.
    #[test]
    fn a_long_name_is_cut() {
        let (mut dm, weld, _) = car();
        dm.get_mut(weld).unwrap().name = "é".repeat(5_000);
        let v = list(&dm, &json!({ "root": "Workspace.PlayerCars", "class": "WeldConstraint" })).unwrap();
        let name = v["rows"][0]["name"].as_str().unwrap();
        assert!(name.len() <= MAX_NAME && !name.is_empty(), "{} bytes", name.len());
    }

    #[test]
    fn a_joint_names_the_two_bodies_it_holds() {
        let (dm, _, _) = car();
        let v = list(&dm, &json!({ "root": "Workspace.PlayerCars" })).unwrap();
        let weld = row(&v, "BodyWeld");
        assert_eq!((weld["body0"]["name"].as_str(), weld["body1"]["name"].as_str()), (Some("Body"), Some("WheelFL")));
        let hinge = row(&v, "Axle");
        assert_eq!(
            (hinge["body0"]["name"].as_str(), hinge["body1"]["name"].as_str()),
            (Some("Body"), Some("WheelFL")),
            "the attachments' parents"
        );
        assert!(row(&v, "Body").get("body0").is_none(), "a part is not a joint");
    }

    #[test]
    fn a_class_filter_root_and_page_are_honoured() {
        let (dm, _, _) = car();
        let v = list(&dm, &json!({ "root": "Workspace.PlayerCars.Coupe", "class": "Constraint" })).unwrap();
        let mut names: Vec<&str> = v["rows"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
        names.sort();
        assert_eq!(names, ["Axle"], "a WeldConstraint is not a Constraint");
        let full = list(&dm, &json!({ "root": "Workspace" })).unwrap();
        let page = list(&dm, &json!({ "root": "Workspace", "limit": 2, "offset": 1 })).unwrap();
        assert_eq!(page["count"], 2);
        assert_eq!(page["rows"].as_array().unwrap()[..], full["rows"].as_array().unwrap()[1..3], "rows 1 and 2");
        assert_eq!(page["total"], full["total"]);
        assert_eq!(page["truncated"], true, "a page short of the total says so");
        assert_eq!(full["truncated"], false);
        assert!(list(&dm, &json!({ "root": "Workspace.Nowhere" })).is_err());
        assert!(list(&dm, &json!({ "origin": "some" })).is_err());
    }

    #[test]
    fn outside_play_the_reply_says_so() {
        let mut world = World::new();
        let req: BridgeRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0", "id": 1, "method": "datamodel.list", "params": {},
        }))
        .unwrap();
        let reply = datamodel_list(&mut world, &req);
        assert_eq!(reply.result.unwrap()["playing"], false);
    }
}
