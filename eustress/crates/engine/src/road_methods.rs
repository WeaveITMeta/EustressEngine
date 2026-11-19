//! The engine side of the `Road` class methods declared in
//! `eustress_common::class_methods`. A road today is a `TerrainSpline` in
//! Road mode, which answers to `Road` methods ([`is_road`]); the work is
//! pure math in `eustress_common::terrain` over what the last bake prepared.

use bevy::prelude::*;
use serde_json::{json, Value};

use eustress_common::classes::Instance;
use eustress_common::terrain::layer_instances::{layer_id, TerrainSpline};
use eustress_common::terrain::layers::SplineMode;
use eustress_common::terrain::road_audit::audit_road;
use eustress_common::terrain::road_junction::NetworkJunction;
use eustress_common::terrain::road_mesh::DesignedRoad;
use eustress_common::terrain::{TerrainBaked, TerrainRoot};

use crate::property_service::ClassMethodImpls;
use crate::space::InstanceFile;

/// Register every `Road` method this build implements.
pub fn register(app: &mut App) {
    ClassMethodImpls::register(app, "Road", "Audit", road_audit);
}

/// Whether `entity` is a road: a `TerrainSpline` in Road mode.
pub fn is_road(world: &World, entity: Entity) -> bool {
    world.get::<TerrainSpline>(entity).is_some_and(|s| s.mode == SplineMode::Road)
}

/// The classes whose methods `entity` (an instance of `class`) answers to:
/// its own, and `Road` when it is a road.
pub fn method_classes(world: &World, entity: Entity, class: &str) -> Vec<String> {
    let mut out = vec![class.to_string()];
    if class != "Road" && is_road(world, entity) {
        out.push("Road".to_string());
    }
    out
}

/// One road spline's layer id, name and Space-relative record path.
struct RoadName {
    id: u64,
    entity: Entity,
    name: String,
    path: Option<String>,
}

fn road_names(world: &mut World) -> Vec<RoadName> {
    let root = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone());
    let mut q = world.query::<(Entity, &TerrainSpline, Option<&Instance>, Option<&InstanceFile>)>();
    q.iter(world)
        .filter(|(_, s, _, _)| s.mode == SplineMode::Road)
        .map(|(entity, _, inst, file)| RoadName {
            id: layer_id(inst.map(|i| i.uuid.as_str()), entity),
            entity,
            name: inst.map_or_else(|| format!("{entity}"), |i| i.name.clone()),
            path: file.and_then(|f| {
                let root = root.as_ref()?;
                let rel = f.toml_path.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
                Some(rel.strip_suffix("/_instance.toml").map_or(rel.clone(), str::to_string))
            }),
        })
        .collect()
}

/// Replace every `"road": <layer id>` in `v` with the road's name and path,
/// since a layer id is a 64-bit number a JSON reader may not hold exactly.
fn name_roads(v: &mut Value, names: &[RoadName]) {
    match v {
        Value::Object(map) => {
            if let Some(id) = map.get("road").and_then(Value::as_u64) {
                let named = names.iter().find(|n| n.id == id);
                map.insert("road".to_string(), json!(named.map_or_else(|| format!("{id:016x}"), |n| n.name.clone())));
                map.insert("road_path".to_string(), json!(named.and_then(|n| n.path.clone())));
            }
            for (_, child) in map.iter_mut() {
                name_roads(child, names);
            }
        }
        Value::Array(list) => list.iter_mut().for_each(|c| name_roads(c, names)),
        _ => {}
    }
}

/// `Road:Audit(network?)`: see `eustress_common::terrain::road_audit`.
fn road_audit(world: &mut World, entity: Entity, args: &Value) -> Result<Value, String> {
    let network = args.get("network").and_then(Value::as_bool).unwrap_or(false);
    let names = road_names(world);
    let me = names.iter().find(|n| n.entity == entity).ok_or("this instance is not a road")?;
    let mut roots = world.query_filtered::<&TerrainBaked, With<TerrainRoot>>();
    let baked = roots.iter(world).next().ok_or("the Space has no terrain, so no road is built")?;
    if baked.wants_bake() {
        return Err("the terrain is re-baking its layers; try again when it has settled".to_string());
    }
    let prepared = baked.prepared().ok_or("the terrain has not baked its layers yet")?;
    let designs: Vec<(u64, &DesignedRoad)> =
        names.iter().filter_map(|n| Some((n.id, baked.baked_design(n.id)?))).collect();
    if !designs.iter().any(|(id, _)| *id == me.id) {
        return Err(format!(
            "{} is not a designed road (set its RoadClass), or it is disabled or has fewer than two points",
            me.name
        ));
    }
    let junctions: Vec<&NetworkJunction> = prepared.junctions().collect();
    let ids: Vec<u64> = if network { designs.iter().map(|(id, _)| *id).collect() } else { vec![me.id] };
    let audits: Vec<Value> = ids
        .iter()
        .filter_map(|id| audit_road(*id, &designs, &junctions))
        .map(|a| serde_json::to_value(a).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let mut out = json!({
        "roads": audits,
        "junctions_in_space": junctions.len(),
        "designed_roads_in_space": designs.len(),
    });
    name_roads(&mut out, &names);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_ids_become_road_names() {
        let names = vec![RoadName {
            id: 0xdead_beef_0000_0001,
            entity: Entity::PLACEHOLDER,
            name: "Main St".into(),
            path: Some("Workspace/Terrain/Layers/Main St".into()),
        }];
        let mut v = json!({ "roads": [{ "road": 0xdead_beef_0000_0001u64, "junctions": [{ "legs": [{ "road": 7u64 }] }] }] });
        name_roads(&mut v, &names);
        assert_eq!(v["roads"][0]["road"], "Main St");
        assert_eq!(v["roads"][0]["road_path"], "Workspace/Terrain/Layers/Main St");
        assert_eq!(v["roads"][0]["junctions"][0]["legs"][0]["road"], "0000000000000007");
        assert!(v["roads"][0]["junctions"][0]["legs"][0]["road_path"].is_null());
    }
}
