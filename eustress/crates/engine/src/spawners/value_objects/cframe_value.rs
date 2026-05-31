//! `CFrameValueSpawner` — `ClassSpawner` for [`ClassName::CFrameValue`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md` §1
//! (ValueObjects, Wave 6.A). See `string_value.rs` for the shared rationale.
//!
//! `CFrameValue` holds one coordinate frame (position + rotation). Roblox
//! `CFrameValue.Value` is a `CFrame`; Eustress maps it onto a Bevy
//! [`Transform`] (scale stays at identity — a CFrame carries no scale).
//!
//! ## Composite value vs the Wave 2 Roblox adapter
//!
//! Same constraint as `Vector3Value` / `Color3Value`: the Wave 2
//! [`RobloxPropertyValue`] cannot carry a `CFrame`, so `import_from_roblox`
//! captures the name only and the Wave 4 importer fills `value`. The TOML
//! path carries `position` (`[x,y,z]`) + `rotation` (quaternion `[x,y,z,w]`)
//! and round-trips losslessly today.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{CFrameValue, ClassName, Instance, PropertyValue};
use eustress_common::{Attributes, Tags};

/// Read a `[properties]` sub-table into a [`Transform`] (position + rotation,
/// identity scale). Returns `None` when neither `position` nor `rotation` is
/// present.
fn toml_transform(props: &toml::Value) -> Option<Transform> {
    let pos = props.get("position").and_then(|v| v.as_array());
    let rot = props.get("rotation").and_then(|v| v.as_array());
    if pos.is_none() && rot.is_none() {
        return None;
    }

    let mut transform = Transform::IDENTITY;
    if let Some(p) = pos {
        let c = |i: usize| p.get(i).and_then(|x| x.as_float()).unwrap_or(0.0) as f32;
        transform.translation = Vec3::new(c(0), c(1), c(2));
    }
    if let Some(r) = rot {
        let c = |i: usize, d: f64| r.get(i).and_then(|x| x.as_float()).unwrap_or(d) as f32;
        // Quaternion [x, y, z, w]; default identity (w = 1).
        transform.rotation = Quat::from_xyzw(c(0, 0.0), c(1, 0.0), c(2, 0.0), c(3, 1.0));
    }
    Some(transform)
}

/// Zero-sized spawner for [`ClassName::CFrameValue`].
#[derive(Default)]
pub struct CFrameValueSpawner;

impl ClassSpawner for CFrameValueSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::CFrameValue
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props.get_string("metadata.name").unwrap_or("CFrameValue").to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);
        let value = props.get_transform("value").cloned().unwrap_or(Transform::IDENTITY);

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::CFrameValue,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                CFrameValue { value },
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

            if let Some(t) = props.get_transform("value") {
                if let Some(mut comp) = entity_mut.get_mut::<CFrameValue>() {
                    comp.value = *t;
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
        // Wave 2 adapter cannot carry a CFrame (see module docs).
        let mut bag = PropertyBag::with_capacity(1);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);

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
            if let Some(transform) = toml_transform(props) {
                bag.set("value", PropertyValue::Transform(transform));
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("CFrameValue".to_string()),
        );

        if let Some(instance) = world.entity(entity).get::<Instance>() {
            meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
            meta.insert("archivable".to_string(), toml::Value::Boolean(instance.archivable));
            if !instance.uuid.is_empty() {
                meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
            }
        }
        root.insert("metadata".to_string(), toml::Value::Table(meta));

        if let Some(comp) = world.entity(entity).get::<CFrameValue>() {
            let t = &comp.value;
            let pos = vec![
                toml::Value::Float(t.translation.x as f64),
                toml::Value::Float(t.translation.y as f64),
                toml::Value::Float(t.translation.z as f64),
            ];
            let rot = vec![
                toml::Value::Float(t.rotation.x as f64),
                toml::Value::Float(t.rotation.y as f64),
                toml::Value::Float(t.rotation.z as f64),
                toml::Value::Float(t.rotation.w as f64),
            ];
            let mut props = toml::value::Table::new();
            props.insert("position".to_string(), toml::Value::Array(pos));
            props.insert("rotation".to_string(), toml::Value::Array(rot));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }

        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_cframe_value() {
        assert_eq!(CFrameValueSpawner.class_name(), ClassName::CFrameValue);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = CFrameValueSpawner;
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(spawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_position_and_rotation() {
        let toml_src = r#"
            [metadata]
            class_name = "CFrameValue"
            name = "Anchor"
            [properties]
            position = [1.0, 2.0, 3.0]
            rotation = [0.0, 0.0, 0.0, 1.0]
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = CFrameValueSpawner.import_from_toml(&value);
        let t = bag.get_transform("value").expect("transform present");
        assert_eq!(t.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(t.rotation, Quat::IDENTITY);
    }

    #[test]
    fn stub_persistence_round_trips_through_empty() {
        assert!(CFrameValueSpawner.deserialize(&[]).is_empty());
    }
}
