//! `FaceControlsSpawner` — `ClassSpawner` for [`ClassName::FaceControls`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.D). Config-attach FACS
//! blendshape weights for animated faces. See the group [`mod`](super) docs.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, FaceControls, PropertyValue};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

const FIELDS: [(&str, &str); 8] = [
    ("JawDrop", "jaw_drop"),
    ("LeftEyeClosed", "left_eye_closed"),
    ("RightEyeClosed", "right_eye_closed"),
    ("LeftBrowUp", "left_brow_up"),
    ("RightBrowUp", "right_brow_up"),
    ("MouthLeft", "mouth_left"),
    ("MouthRight", "mouth_right"),
    ("FunnelLeft", "funnel"),
];

/// Zero-sized spawner for [`ClassName::FaceControls`].
#[derive(Default)]
pub struct FaceControlsSpawner;

impl ClassSpawner for FaceControlsSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::FaceControls
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::FaceControls, props);
        let name = instance.name.clone();
        let d = FaceControls::default();
        let comp = FaceControls {
            jaw_drop: props.get_f32("jaw_drop").unwrap_or(d.jaw_drop),
            left_eye_closed: props.get_f32("left_eye_closed").unwrap_or(d.left_eye_closed),
            right_eye_closed: props.get_f32("right_eye_closed").unwrap_or(d.right_eye_closed),
            left_brow_up: props.get_f32("left_brow_up").unwrap_or(d.left_brow_up),
            right_brow_up: props.get_f32("right_brow_up").unwrap_or(d.right_brow_up),
            mouth_left: props.get_f32("mouth_left").unwrap_or(d.mouth_left),
            mouth_right: props.get_f32("mouth_right").unwrap_or(d.mouth_right),
            funnel: props.get_f32("funnel").unwrap_or(d.funnel),
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                comp,
                Name::new(name),
                Attributes::new(),
                Tags::new(),
            ))
            .id();
        if let Some(parent) = ctx.parent_entity {
            ctx.commands.entity(entity).insert(ChildOf(parent));
        }
        entity
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        apply_metadata_edit(world, entity, props);
        if let Some(mut comp) = world.get_mut::<FaceControls>(entity) {
            if let Some(v) = props.get_f32("jaw_drop") { comp.jaw_drop = v; }
            if let Some(v) = props.get_f32("left_eye_closed") { comp.left_eye_closed = v; }
            if let Some(v) = props.get_f32("right_eye_closed") { comp.right_eye_closed = v; }
            if let Some(v) = props.get_f32("left_brow_up") { comp.left_brow_up = v; }
            if let Some(v) = props.get_f32("right_brow_up") { comp.right_brow_up = v; }
            if let Some(v) = props.get_f32("mouth_left") { comp.mouth_left = v; }
            if let Some(v) = props.get_f32("mouth_right") { comp.mouth_right = v; }
            if let Some(v) = props.get_f32("funnel") { comp.funnel = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(9);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        for (rbx_key, key) in FIELDS {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_f32()) {
                bag.set(key, PropertyValue::Float(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(9);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for (_, key) in FIELDS {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "FaceControls")),
        );
        if let Some(comp) = world.get::<FaceControls>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("jaw_drop".into(), toml::Value::Float(comp.jaw_drop as f64));
            props.insert("left_eye_closed".into(), toml::Value::Float(comp.left_eye_closed as f64));
            props.insert("right_eye_closed".into(), toml::Value::Float(comp.right_eye_closed as f64));
            props.insert("left_brow_up".into(), toml::Value::Float(comp.left_brow_up as f64));
            props.insert("right_brow_up".into(), toml::Value::Float(comp.right_brow_up as f64));
            props.insert("mouth_left".into(), toml::Value::Float(comp.mouth_left as f64));
            props.insert("mouth_right".into(), toml::Value::Float(comp.mouth_right as f64));
            props.insert("funnel".into(), toml::Value::Float(comp.funnel as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_face_controls() {
        assert_eq!(FaceControlsSpawner.class_name(), ClassName::FaceControls);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(FaceControlsSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "FaceControls"
            name = "Face"
            [properties]
            jaw_drop = 0.5
            funnel = 0.2
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = FaceControlsSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("jaw_drop"), Some(0.5));
        assert_eq!(bag.get_f32("funnel"), Some(0.2));
    }
}
