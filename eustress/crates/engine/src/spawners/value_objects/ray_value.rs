//! `RayValueSpawner` — `ClassSpawner` for [`ClassName::RayValue`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md` §1
//! (ValueObjects, Wave 6.A). See `string_value.rs` for the shared rationale.
//!
//! `RayValue` holds one ray: an origin and a direction, stored as
//! `[ox, oy, oz, dx, dy, dz]`. Roblox `RayValue.Value` is a `Ray`.
//!
//! ## No native `[f32; 6]` in the `PropertyBag`
//!
//! [`PropertyValue`] has no six-float variant, so the ray crosses the bag as
//! TWO [`PropertyValue::Vector3`] entries — `value.origin` and
//! `value.direction` — which `spawn` reassembles into the component's
//! `[f32; 6]`. The TOML form mirrors this with `origin` / `direction` arrays
//! under `[properties]`. The Wave 2 Roblox adapter cannot carry a `Ray`
//! (`Value` arrives as [`RobloxPropertyValue::Other`]), so
//! `import_from_roblox` captures the name only; Wave 4 fills the ray.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue, RayValue};
use eustress_common::{Attributes, Tags};

/// Read a 3-element float array from a `toml::Value` as a [`Vec3`].
fn toml_vec3(v: &toml::Value) -> Option<Vec3> {
    let arr = v.as_array()?;
    let c = |i: usize| arr.get(i).and_then(|x| x.as_float()).unwrap_or(0.0) as f32;
    Some(Vec3::new(c(0), c(1), c(2)))
}

/// Zero-sized spawner for [`ClassName::RayValue`].
#[derive(Default)]
pub struct RayValueSpawner;

impl ClassSpawner for RayValueSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::RayValue
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props.get_string("metadata.name").unwrap_or("RayValue").to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        // Reassemble the [f32; 6] from the two Vector3 bag entries.
        let origin = props.get_vec3("value.origin").unwrap_or(Vec3::ZERO);
        let direction = props.get_vec3("value.direction").unwrap_or(Vec3::ZERO);
        let value = [origin.x, origin.y, origin.z, direction.x, direction.y, direction.z];

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::RayValue,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                RayValue { value },
                Name::new(name),
                Attributes::new(),
                Tags::new(),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            let new_name = props.get_string("metadata.name").map(str::to_string);

            if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                if let Some(ref n) = new_name {
                    instance.name = n.clone();
                }
                if let Some(archivable) = props.get_bool("metadata.archivable") {
                    instance.archivable = archivable;
                }
            }

            // Origin and direction can be edited independently — patch whichever
            // is present in the delta onto the existing six-float value.
            let origin = props.get_vec3("value.origin");
            let direction = props.get_vec3("value.direction");
            if origin.is_some() || direction.is_some() {
                if let Some(mut comp) = entity_mut.get_mut::<RayValue>() {
                    if let Some(o) = origin {
                        comp.value[0] = o.x;
                        comp.value[1] = o.y;
                        comp.value[2] = o.z;
                    }
                    if let Some(d) = direction {
                        comp.value[3] = d.x;
                        comp.value[4] = d.y;
                        comp.value[5] = d.z;
                    }
                }
            }

            if let Some(ref n) = new_name {
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(n.clone());
                }
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // Wave 2 adapter cannot carry a Ray (see module docs).
        let mut bag = PropertyBag::with_capacity(1);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(5);

        if let Some(meta) = toml_value.get("metadata") {
            if let Some(name) = meta.get("name").and_then(|v| v.as_str()) {
                bag.set("metadata.name", PropertyValue::String(name.to_string()));
            }
            if let Some(archivable) = meta.get("archivable").and_then(|v| v.as_bool()) {
                bag.set("metadata.archivable", PropertyValue::Bool(archivable));
            }
            if let Some(uuid) = meta.get("uuid").and_then(|v| v.as_str()) {
                bag.set("metadata.uuid", PropertyValue::String(uuid.to_string()));
            }
        }

        if let Some(props) = toml_value.get("properties") {
            if let Some(origin) = props.get("origin").and_then(toml_vec3) {
                bag.set("value.origin", PropertyValue::Vector3(origin));
            }
            if let Some(direction) = props.get("direction").and_then(toml_vec3) {
                bag.set("value.direction", PropertyValue::Vector3(direction));
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("RayValue".to_string()),
        );

        if let Some(instance) = world.entity(entity).get::<Instance>() {
            meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
            meta.insert("archivable".to_string(), toml::Value::Boolean(instance.archivable));
            if !instance.uuid.is_empty() {
                meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
            }
        }
        root.insert("metadata".to_string(), toml::Value::Table(meta));

        if let Some(comp) = world.entity(entity).get::<RayValue>() {
            let v = &comp.value;
            let origin = vec![
                toml::Value::Float(v[0] as f64),
                toml::Value::Float(v[1] as f64),
                toml::Value::Float(v[2] as f64),
            ];
            let direction = vec![
                toml::Value::Float(v[3] as f64),
                toml::Value::Float(v[4] as f64),
                toml::Value::Float(v[5] as f64),
            ];
            let mut props = toml::value::Table::new();
            props.insert("origin".to_string(), toml::Value::Array(origin));
            props.insert("direction".to_string(), toml::Value::Array(direction));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }

        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ray_value() {
        assert_eq!(RayValueSpawner.class_name(), ClassName::RayValue);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = RayValueSpawner;
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(spawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_origin_and_direction() {
        let toml_src = r#"
            [metadata]
            class_name = "RayValue"
            name = "AimRay"
            [properties]
            origin = [0.0, 1.0, 0.0]
            direction = [0.0, 0.0, 1.0]
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = RayValueSpawner.import_from_toml(&value);
        assert_eq!(bag.get_vec3("value.origin"), Some(Vec3::new(0.0, 1.0, 0.0)));
        assert_eq!(bag.get_vec3("value.direction"), Some(Vec3::new(0.0, 0.0, 1.0)));
    }

    #[test]
    fn stub_persistence_round_trips_through_empty() {
        assert!(RayValueSpawner.deserialize(&[]).is_empty());
    }
}
