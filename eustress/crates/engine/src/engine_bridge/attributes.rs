//! The engine bridge's `attribute.get` and `attribute.set`: read an
//! instance's Attributes (in Play too) and set them in Edit mode. The MCP
//! `attribute_get` and `attribute_set` tools call them.
//!
//! A set is an agent's content edit, held to `properties.set`'s rules: Edit
//! only ([`super::properties::editing_only`]), one undo step per call, and
//! nothing written here: the live `Attributes` component changes and the
//! loader's `Changed<Attributes>` write-back patches the instance's
//! `[attributes]` table. So only an instance with a record of its own on disk
//! is accepted (a database-stored instance's save path drops attributes, and
//! an instance with no record would lose the change when Studio closes), and
//! only under a content service ([`CONTENT_ROOTS`]): a list, so a service
//! nobody has thought about is refused rather than writable.

use bevy::prelude::*;
use serde_json::{json, Map, Value};

use eustress_common::attributes::{AttributeValue, Attributes};
use eustress_common::classes::Instance;

use super::parameters::{resolve, target_json};
use super::{BridgeError, BridgeRequest, BridgeResponse};
use crate::space::InstanceFile;

/// The services whose instances `attribute.set` may change.
pub(super) const CONTENT_ROOTS: &[&str] = &[
    "Workspace",
    "ReplicatedStorage",
    "ReplicatedFirst",
    "ServerStorage",
    "ServerScriptService",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "Lighting",
    "SoundService",
    "Teams",
];

/// Longest String a set takes, in bytes.
pub(super) const MAX_SET_STRING: usize = 1024;
/// Longest String a get returns, in bytes; longer ones are cut and marked.
pub(super) const MAX_GET_STRING: usize = 4096;
/// Sets and removes in one call.
pub(super) const MAX_CHANGES: usize = 32;
/// Attributes on one instance after a set.
pub(super) const MAX_ATTRIBUTES: usize = 256;

fn answer(req: &BridgeRequest, result: Result<Value, String>) -> BridgeResponse {
    match result {
        Ok(v) => BridgeResponse::ok(req.id.clone(), v),
        Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
    }
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// The longest prefix of `s` within `max` bytes, cut on a character boundary.
fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// One attribute as JSON: `{type, value}` for the kinds a set takes (and
/// Vector3), `{type, display}` for the rest.
pub(super) fn value_json(v: &AttributeValue) -> Value {
    match v {
        AttributeValue::Number(n) => json!({ "type": "Number", "value": n }),
        AttributeValue::Int(i) => json!({ "type": "Int", "value": i }),
        AttributeValue::Bool(b) => json!({ "type": "Bool", "value": b }),
        AttributeValue::String(s) if s.len() > MAX_GET_STRING => {
            json!({ "type": "String", "value": cut(s, MAX_GET_STRING), "truncated": true, "bytes": s.len() })
        }
        AttributeValue::String(s) => json!({ "type": "String", "value": s }),
        AttributeValue::Color3(c) | AttributeValue::Color(c) => {
            let c = c.to_srgba();
            json!({ "type": v.type_name(), "value": [c.red, c.green, c.blue] })
        }
        AttributeValue::Vector3(p) => json!({ "type": "Vector3", "value": [p.x, p.y, p.z] }),
        other => json!({ "type": other.type_name(), "display": other.display_value() }),
    }
}

/// Every attribute of `attrs`, by name, in name order.
pub(super) fn attributes_json(attrs: Option<&Attributes>) -> Value {
    let mut names: Vec<(&String, &AttributeValue)> = attrs.map(|a| a.iter().collect()).unwrap_or_default();
    names.sort_by(|a, b| a.0.cmp(b.0));
    Value::Object(names.into_iter().map(|(k, v)| (k.clone(), value_json(v))).collect::<Map<_, _>>())
}

/// An attribute name a set takes: `[A-Za-z0-9_]`, 1 to 64 characters.
pub(super) fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(format!("'{}' is not an attribute name: letters, digits and _, 1 to 64 characters", cut(name, 80)));
    }
    Ok(())
}

/// The value a set stores for `given`. `existing` is the attribute's current
/// value: a whole number given for an Int stays an Int.
pub(super) fn parse_value(name: &str, given: &Value, existing: Option<&AttributeValue>) -> Result<AttributeValue, String> {
    match given {
        Value::Bool(b) => Ok(AttributeValue::Bool(*b)),
        Value::String(s) if s.len() > MAX_SET_STRING => {
            Err(format!("{name}: a String is at most {MAX_SET_STRING} bytes (this one is {})", s.len()))
        }
        Value::String(s) => Ok(AttributeValue::String(s.clone())),
        Value::Number(n) => {
            if matches!(existing, Some(AttributeValue::Int(_))) {
                if let Some(i) = n.as_i64() {
                    return Ok(AttributeValue::Int(i));
                }
            }
            match n.as_f64() {
                Some(f) if f.is_finite() => Ok(AttributeValue::Number(f)),
                _ => Err(format!("{name}: the number must be finite")),
            }
        }
        Value::Array(rgb) if rgb.len() == 3 => {
            let mut c = [0f32; 3];
            for (slot, v) in c.iter_mut().zip(rgb) {
                match v.as_f64() {
                    Some(f) if f.is_finite() && (0.0..=1.0).contains(&f) => *slot = f as f32,
                    _ => return Err(format!("{name}: a Color3 is [r, g, b], each from 0 to 1")),
                }
            }
            Ok(AttributeValue::Color3(Color::srgb(c[0], c[1], c[2])))
        }
        _ => Err(format!("{name}: the value must be a number, true/false, a string, or a Color3 [r, g, b] in 0-1")),
    }
}

// ---------------------------------------------------------------------------
// Where a set may write
// ---------------------------------------------------------------------------

/// The content service an instance record lies under, from its path relative
/// to the Space (`ServerStorage/Cars/Coupe/Chassis/Spine/_instance.toml`).
/// `Err` for a record outside [`CONTENT_ROOTS`], and for a service's own
/// record (the service root itself).
pub(super) fn content_root(rel: &str) -> Result<&str, String> {
    let rel = rel.trim_matches('/');
    let mut parts = rel.split('/');
    let root = parts.next().unwrap_or_default();
    if !CONTENT_ROOTS.contains(&root) {
        return Err(format!(
            "'{root}' is not a content service; attributes are set only under {}",
            CONTENT_ROOTS.join(", ")
        ));
    }
    let rest: Vec<&str> = parts.collect();
    // `Workspace/_instance.toml` or `Workspace/_service.toml`: the service.
    if rest.len() <= 1 && rest.first().is_none_or(|f| f.starts_with('_')) {
        return Err(format!("{root} itself is a service; set attributes on an instance under it"));
    }
    Ok(root)
}

/// The record a set's change is saved to, Space-relative with `/`, or why
/// there is none.
fn record_of(world: &World, entity: Entity) -> Result<String, String> {
    #[cfg(feature = "world-db")]
    if world.get::<crate::space::world_db_binary::BinaryEcsInstance>(entity).is_some() {
        return Err("this instance is stored in the world database, whose save does not keep attributes yet; \
                    the change would be lost"
            .to_string());
    }
    let none = "this instance has no record of its own on disk, so the change would last only until Studio closes";
    let file = world.get::<InstanceFile>(entity).ok_or(none)?;
    if file.toml_path.to_string_lossy().contains("__bin_") {
        return Err(none.to_string());
    }
    let root = world.get_resource::<crate::space::SpaceRoot>().ok_or("no Space is open")?;
    let rel = file
        .toml_path
        .strip_prefix(&root.0)
        .map_err(|_| "the instance's record is outside the open Space".to_string())?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

// ---------------------------------------------------------------------------
// attribute.get
// ---------------------------------------------------------------------------

/// `attribute.get`: an instance's attributes (`path`, `uuid`, `name` or
/// `selected`), typed. Works in Play: a Play copy is read like any instance.
pub fn attribute_get(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let result = (|| -> Result<Value, String> {
        let t = resolve(world, &req.params)?;
        let attrs = world.get::<Attributes>(t.entity);
        Ok(json!({
            "instance": target_json(&t),
            "count": attrs.map_or(0, Attributes::len),
            "attributes": attributes_json(attrs),
        }))
    })();
    answer(req, result)
}

// ---------------------------------------------------------------------------
// attribute.set
// ---------------------------------------------------------------------------

/// `attribute.set`: on the instance `uuid`, set `values` {Name: value} and
/// remove `remove` [Name], all or nothing, as one undo step. Changing an
/// attribute's type needs `replace_type: true`.
pub fn attribute_set(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    answer(req, set(world, &req.params))
}

fn set(world: &mut World, params: &Value) -> Result<Value, String> {
    super::properties::editing_only(world)?;
    let uuid = params
        .get("uuid")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .ok_or("attribute.set names its instance by `uuid` only")?;
    let t = resolve(world, &json!({ "uuid": uuid }))?;
    if CONTENT_ROOTS.contains(&t.class.as_str()) {
        return Err(format!("{} is a service; set attributes on an instance under it", t.class));
    }
    let rel = record_of(world, t.entity)?;
    let root = content_root(&rel)?.to_string();

    let values = match params.get("values") {
        None => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => return Err("`values` must be an object of {Name: value}".to_string()),
    };
    let remove: Vec<String> = match params.get("remove") {
        None => Vec::new(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or("`remove` must be a list of names".to_string()))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("`remove` must be a list of names".to_string()),
    };
    let changes = values.len() + remove.len();
    if changes == 0 {
        return Err("nothing to change: pass `values` and/or `remove`".to_string());
    }
    if changes > MAX_CHANGES {
        return Err(format!("{changes} changes in one call; at most {MAX_CHANGES}"));
    }
    let replace_type = params.get("replace_type").and_then(Value::as_bool).unwrap_or(false);

    let id = world.get::<Instance>(t.entity).map(|i| i.id).unwrap_or(0);
    if id == 0 {
        return Err("this instance has no undo id, so the change could not be undone; set it in the Properties panel".to_string());
    }

    // Every change is checked before anything changes.
    let old = world.get::<Attributes>(t.entity).cloned().unwrap_or_default();
    let mut new = old.clone();
    let mut set_rows = Vec::new();
    for (name, given) in &values {
        check_name(name)?;
        let value = parse_value(name, given, old.get(name))?;
        if let Some(prev) = old.get(name) {
            if prev.type_name() != value.type_name() && !replace_type {
                return Err(format!(
                    "{name} is a {} and the new value is a {}; pass replace_type: true to change its type",
                    prev.type_name(),
                    value.type_name()
                ));
            }
        }
        set_rows.push(json!({ "name": name, "type": value.type_name() }));
        new.set(name, value);
    }
    let mut removed = Vec::new();
    for name in &remove {
        check_name(name)?;
        if values.contains_key(name) {
            return Err(format!("{name} is both set and removed"));
        }
        if new.remove(name).is_some() {
            removed.push(name.clone());
        }
    }
    if new.len() > MAX_ATTRIBUTES {
        return Err(format!("the instance would carry {} attributes; at most {MAX_ATTRIBUTES}", new.len()));
    }

    let old_json = serde_json::to_string(&old.values).map_err(|e| e.to_string())?;
    let new_json = serde_json::to_string(&new.values).map_err(|e| e.to_string())?;
    // The loader's `Changed<Attributes>` write-back saves it to the record.
    world.entity_mut(t.entity).insert(new);
    if let Some(mut stack) = world.get_resource_mut::<crate::undo::UndoStack>() {
        stack.push_labeled(
            format!("Set {changes} attribute{}", if changes == 1 { "" } else { "s" }),
            crate::undo::Action::ChangeAttributes { id, old_attrs: old_json, new_attrs: new_json },
        );
    }
    // Names and types only: a String may hold something the person typed.
    let summary: Vec<String> = set_rows
        .iter()
        .map(|r| format!("{}:{}", r["name"].as_str().unwrap_or(""), r["type"].as_str().unwrap_or("")))
        .chain(removed.iter().map(|n| format!("-{n}")))
        .collect();
    info!("bridge attribute.set uuid={uuid} root={root} [{}]", summary.join(", "));
    Ok(json!({
        "instance": target_json(&t),
        "set": set_rows,
        "removed": removed,
        "undo_steps": 1,
        "saved_to": rel,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_reads_back_typed() {
        assert_eq!(value_json(&AttributeValue::Number(2.5)), json!({ "type": "Number", "value": 2.5 }));
        assert_eq!(value_json(&AttributeValue::Int(3)), json!({ "type": "Int", "value": 3 }));
        assert_eq!(value_json(&AttributeValue::Bool(true)), json!({ "type": "Bool", "value": true }));
        assert_eq!(value_json(&AttributeValue::String("x".into())), json!({ "type": "String", "value": "x" }));
        let c = value_json(&AttributeValue::Color3(Color::srgb(1.0, 0.5, 0.0)));
        assert_eq!(c["type"], "Color3");
        assert_eq!(c["value"], json!([1.0, 0.5, 0.0]));
        assert_eq!(value_json(&AttributeValue::Vector3(Vec3::new(1.0, 2.0, 3.0)))["value"], json!([1.0, 2.0, 3.0]));
        let long = "é".repeat(MAX_GET_STRING);
        let cut_json = value_json(&AttributeValue::String(long.clone()));
        assert_eq!(cut_json["truncated"], true);
        assert!(cut_json["value"].as_str().unwrap().len() <= MAX_GET_STRING);
        assert_eq!(value_json(&AttributeValue::BrickColor(5))["type"], "BrickColor");
        assert!(value_json(&AttributeValue::BrickColor(5)).get("display").is_some());
    }

    #[test]
    fn only_the_four_kinds_are_taken_and_each_is_checked() {
        assert_eq!(parse_value("A", &json!(1.5), None), Ok(AttributeValue::Number(1.5)));
        assert_eq!(parse_value("A", &json!(2), Some(&AttributeValue::Int(1))), Ok(AttributeValue::Int(2)));
        assert_eq!(parse_value("A", &json!(2), None), Ok(AttributeValue::Number(2.0)));
        assert_eq!(parse_value("A", &json!(false), None), Ok(AttributeValue::Bool(false)));
        assert!(matches!(parse_value("A", &json!([0.0, 0.5, 1.0]), None), Ok(AttributeValue::Color3(_))));
        assert!(parse_value("A", &json!([0.0, 0.5, 1.5]), None).is_err(), "out of 0-1");
        assert!(parse_value("A", &json!("x".repeat(MAX_SET_STRING + 1)), None).is_err(), "a 1 KB+ string");
        assert!(parse_value("A", &json!(null), None).is_err());
        assert!(parse_value("A", &json!({ "x": 1 }), None).is_err());
        assert!(parse_value("A", &json!([1, 2]), None).is_err());
    }

    #[test]
    fn names_are_letters_digits_and_underscores() {
        assert!(check_name("ChassisMaxSpeed").is_ok());
        assert!(check_name("SC4_Gear2").is_ok());
        for bad in ["", "a.b", "a b", "ä", &"x".repeat(65)] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_instances_under_a_content_service_are_writable() {
        assert_eq!(content_root("ServerStorage/Cars/Coupe/Chassis/Spine/_instance.toml"), Ok("ServerStorage"));
        assert_eq!(content_root("Workspace/Base.part.toml"), Ok("Workspace"));
        for refused in [
            "Workspace/_instance.toml",
            "Lighting/_service.toml",
            "CoreGui/Thing/_instance.toml",
            ".eustress/instances/x/_instance.toml",
            "Plugins/Road/_instance.toml",
            "ServerStorageX/A/_instance.toml",
        ] {
            assert!(content_root(refused).is_err(), "{refused}");
        }
    }

    /// The stored form of each kind a set takes comes back as the same value
    /// after the save and a reload (the loader's write-back and reader use
    /// these two conversions).
    #[test]
    fn a_set_value_survives_the_save_and_a_reload() {
        use eustress_common::datamodel::record::{attribute_to_toml, toml_to_attribute};
        for v in [
            AttributeValue::Number(2.5),
            AttributeValue::Int(7),
            AttributeValue::Bool(true),
            AttributeValue::String("MixTest".into()),
            AttributeValue::Color3(Color::srgb(0.25, 0.5, 1.0)),
        ] {
            let stored = attribute_to_toml(&v).unwrap_or_else(|| panic!("{} is not saved", v.type_name()));
            let mut table = toml::Table::new();
            table.insert("v".into(), stored);
            let text = toml::to_string(&table).unwrap();
            let read: toml::Table = text.parse().unwrap();
            let back = toml_to_attribute(&read["v"]).unwrap_or_else(|| panic!("{} did not reload", v.type_name()));
            assert_eq!(value_json(&back), value_json(&v), "{}", v.type_name());
        }
    }
}

/// The bridge path itself, on a bare World with the resources it reads.
#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::play_mode::PlayModeState;
    use crate::space::SpaceRoot;
    use crate::undo::{apply_undo_action, UndoStack};
    use eustress_common::classes::ClassName;
    use std::path::PathBuf;

    const UUID: &str = "f1898f14b74d4811995c1ab6c4ee2b18";

    /// A Space with one Part (the Spine) under ServerStorage, its record on
    /// disk, carrying MixTest = 1.0.
    fn space(tag: &str, rel: &str) -> (World, Entity, PathBuf) {
        let root = std::env::temp_dir().join(format!("eus_attr_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let record = root.join(rel);
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(&record, "[metadata]\nclass_name = \"Part\"\nname = \"Spine\"\n\n[attributes]\nMixTest = 1.0\n").unwrap();
        let mut world = World::new();
        world.insert_resource(SpaceRoot(root.clone()));
        world.insert_resource(UndoStack::default());
        world.insert_resource(State::new(PlayModeState::Editing));
        let mut attrs = Attributes::new();
        attrs.set("MixTest", AttributeValue::Number(1.0));
        let entity = world
            .spawn((
                Instance {
                    name: "Spine".into(),
                    class_name: ClassName::Part,
                    archivable: true,
                    id: 42,
                    ai: false,
                    uuid: UUID.into(),
                },
                InstanceFile { toml_path: record, mesh_path: PathBuf::new(), name: "Spine".into() },
                attrs,
            ))
            .id();
        (world, entity, root)
    }

    fn mix(world: &World, entity: Entity) -> Option<AttributeValue> {
        world.get::<Attributes>(entity).and_then(|a| a.get("MixTest").cloned())
    }

    #[test]
    fn a_set_during_play_is_refused() {
        let (mut world, entity, root) = space("play", "ServerStorage/Cars/Coupe/Chassis/Spine/_instance.toml");
        world.insert_resource(State::new(PlayModeState::Playing));
        let err = set(&mut world, &json!({ "uuid": UUID, "values": { "MixTest": 2.5 } })).unwrap_err();
        assert!(err.contains("Play"), "{err}");
        assert_eq!(mix(&world, entity), Some(AttributeValue::Number(1.0)), "nothing changed");
        let _ = std::fs::remove_dir_all(root);
    }

    /// A set changes the live attributes as one undo step, and undo puts
    /// back exactly what was there (an added name is gone again).
    #[test]
    fn a_set_is_one_undo_step() {
        let (mut world, entity, root) = space("undo", "ServerStorage/Cars/Coupe/Chassis/Spine/_instance.toml");
        let before = world.get::<Attributes>(entity).unwrap().values.clone();
        let reply = set(&mut world, &json!({ "uuid": UUID, "values": { "MixTest": 2.5, "Tint": [0.0, 0.5, 1.0] } })).unwrap();
        assert_eq!(reply["undo_steps"], 1);
        assert_eq!(reply["saved_to"], "ServerStorage/Cars/Coupe/Chassis/Spine/_instance.toml");
        assert_eq!(mix(&world, entity), Some(AttributeValue::Number(2.5)));
        assert!(matches!(world.get::<Attributes>(entity).unwrap().get("Tint"), Some(AttributeValue::Color3(_))));
        let action = world.resource_mut::<UndoStack>().undo().expect("one undo step");
        apply_undo_action(&action, &mut world);
        assert_eq!(world.get::<Attributes>(entity).unwrap().values, before, "undo restores the attributes exactly");
        assert!(world.resource_mut::<UndoStack>().undo().is_none(), "only one step was recorded");
        let _ = std::fs::remove_dir_all(root);
    }

    /// What the set leaves on the component is what the loader's write-back
    /// writes (the same conversion, the same patcher), and a reload of the
    /// record reads the same values.
    #[test]
    fn a_set_saved_by_the_write_back_reloads_the_same() {
        use eustress_common::datamodel::record::{attribute_to_toml, toml_to_attribute};
        let rel = "ServerStorage/Cars/Coupe/Chassis/Spine/_instance.toml";
        let (mut world, entity, root) = space("save", rel);
        set(&mut world, &json!({ "uuid": UUID, "values": { "MixTest": 2.5, "Flag": true, "Label": "tuned" } })).unwrap();
        let attrs = world.get::<Attributes>(entity).unwrap();
        let payload = attrs.values.iter().filter_map(|(k, v)| attribute_to_toml(v).map(|t| (k.clone(), t))).collect();
        crate::space::instance_loader::patch_tags_attributes_toml(&root.join(rel), None, Some(payload)).unwrap();
        let text = std::fs::read_to_string(root.join(rel)).unwrap();
        let def = crate::space::instance_loader::load_instance_definition_from_str(&text).unwrap();
        let saved = def.attributes.expect("an [attributes] table");
        for (name, want) in [
            ("MixTest", AttributeValue::Number(2.5)),
            ("Flag", AttributeValue::Bool(true)),
            ("Label", AttributeValue::String("tuned".into())),
        ] {
            assert_eq!(saved.get(name).and_then(toml_to_attribute), Some(want), "{name}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_set_outside_its_rules_changes_nothing() {
        let rel = "ServerStorage/Cars/Coupe/Chassis/Spine/_instance.toml";
        let cases: Vec<(&str, Value)> = vec![
            ("type change", json!({ "uuid": UUID, "values": { "MixTest": "fast" } })),
            ("bad name", json!({ "uuid": UUID, "values": { "a.b": 1 } })),
            ("2 KB string", json!({ "uuid": UUID, "values": { "S": "x".repeat(2048) } })),
            ("by name", json!({ "name": "Spine", "values": { "MixTest": 2.5 } })),
            ("too many", json!({ "uuid": UUID, "values": (0..33).map(|i| (format!("A{i}"), json!(1))).collect::<Map<_, _>>() })),
        ];
        for (what, params) in cases {
            let (mut world, entity, root) = space(&format!("rule{}", what.len()), rel);
            assert!(set(&mut world, &params).is_err(), "{what} must be refused");
            assert_eq!(mix(&world, entity), Some(AttributeValue::Number(1.0)), "{what}: nothing changed");
            let _ = std::fs::remove_dir_all(root);
        }
        let (mut world, _, root) = space("typeok", rel);
        assert!(set(&mut world, &json!({ "uuid": UUID, "values": { "MixTest": "fast" }, "replace_type": true })).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_target_outside_the_content_services_or_without_a_record_is_refused() {
        for rel in ["CoreGui/Spine/_instance.toml", "ServerStorage/_service.toml"] {
            let (mut world, entity, root) = space(&format!("root{}", rel.len()), rel);
            assert!(set(&mut world, &json!({ "uuid": UUID, "values": { "MixTest": 2.5 } })).is_err(), "{rel}");
            assert_eq!(mix(&world, entity), Some(AttributeValue::Number(1.0)));
            let _ = std::fs::remove_dir_all(root);
        }
        let (mut world, entity, root) = space("norecord", "ServerStorage/Cars/Spine/_instance.toml");
        world.entity_mut(entity).remove::<InstanceFile>();
        let err = set(&mut world, &json!({ "uuid": UUID, "values": { "MixTest": 2.5 } })).unwrap_err();
        assert!(err.contains("no record"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_get_reads_typed_values_in_play() {
        let (mut world, _, root) = space("get", "ServerStorage/Cars/Coupe/Chassis/Spine/_instance.toml");
        world.insert_resource(State::new(PlayModeState::Playing));
        let req: BridgeRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0", "id": 1, "method": "attribute.get", "params": { "uuid": UUID },
        }))
        .unwrap();
        let reply = attribute_get(&mut world, &req);
        let v = reply.result.expect("a get works in Play");
        assert_eq!(v["attributes"]["MixTest"], json!({ "type": "Number", "value": 1.0 }));
        let _ = std::fs::remove_dir_all(root);
    }
}
