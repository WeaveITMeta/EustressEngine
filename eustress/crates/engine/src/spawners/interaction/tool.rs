//! `ToolSpawner` — `ClassSpawner` for [`ClassName::Tool`].
//!
//! A `Tool` is an equippable Backpack item with a child `Handle` part. The
//! spawner attaches the [`Tool`] config component; the runtime equip /
//! activate behavior lives in [`crate::interaction::equip`].
//!
//! Mirrors the Wave 6.A ValueObject spawner shape (data-only attach + empty
//! LOD + stub Fjall persistence + TOML round-trip).

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, Tool};
use eustress_common::{Attributes, Tags};

use super::{export_metadata, import_metadata, instance_from_bag, read_vec3_array, vec3_to_toml};

/// Zero-sized spawner for [`ClassName::Tool`].
#[derive(Default)]
pub struct ToolSpawner;

impl ClassSpawner for ToolSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Tool
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::Tool, props);
        let name = instance.name.clone();
        let defaults = Tool::default();

        let tool = Tool {
            grip_pos: props.get_vec3("grip_pos").unwrap_or(defaults.grip_pos),
            grip_forward: props.get_vec3("grip_forward").unwrap_or(defaults.grip_forward),
            grip_right: props.get_vec3("grip_right").unwrap_or(defaults.grip_right),
            grip_up: props.get_vec3("grip_up").unwrap_or(defaults.grip_up),
            can_be_dropped: props.get_bool("can_be_dropped").unwrap_or(defaults.can_be_dropped),
            manual_activation_only: props
                .get_bool("manual_activation_only")
                .unwrap_or(defaults.manual_activation_only),
            requires_handle: props.get_bool("requires_handle").unwrap_or(defaults.requires_handle),
            tool_tip: props
                .get_string("tool_tip")
                .map(str::to_string)
                .unwrap_or(defaults.tool_tip),
            enabled: props.get_bool("enabled").unwrap_or(defaults.enabled),
        };

        let entity = ctx
            .commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                instance,
                tool,
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
        if let Ok(mut em) = world.get_entity_mut(entity) {
            if let Some(mut instance) = em.get_mut::<eustress_common::classes::Instance>() {
                if let Some(n) = props.get_string("metadata.name") {
                    instance.name = n.to_string();
                }
                if let Some(a) = props.get_bool("metadata.archivable") {
                    instance.archivable = a;
                }
            }
            if let Some(mut tool) = em.get_mut::<Tool>() {
                if let Some(v) = props.get_vec3("grip_pos") { tool.grip_pos = v; }
                if let Some(v) = props.get_vec3("grip_forward") { tool.grip_forward = v; }
                if let Some(v) = props.get_vec3("grip_right") { tool.grip_right = v; }
                if let Some(v) = props.get_vec3("grip_up") { tool.grip_up = v; }
                if let Some(v) = props.get_bool("can_be_dropped") { tool.can_be_dropped = v; }
                if let Some(v) = props.get_bool("manual_activation_only") { tool.manual_activation_only = v; }
                if let Some(v) = props.get_bool("requires_handle") { tool.requires_handle = v; }
                if let Some(v) = props.get_string("tool_tip") { tool.tool_tip = v.to_string(); }
                if let Some(v) = props.get_bool("enabled") { tool.enabled = v; }
            }
            if let Some(n) = props.get_string("metadata.name") {
                if let Some(mut name) = em.get_mut::<Name>() {
                    name.set(n.to_string());
                }
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("CanBeDropped").and_then(|p| p.as_bool()) {
            bag.set("can_be_dropped", PropertyValue::Bool(v));
        }
        if let Some(v) = rbx.property("ManualActivationOnly").and_then(|p| p.as_bool()) {
            bag.set("manual_activation_only", PropertyValue::Bool(v));
        }
        if let Some(v) = rbx.property("RequiresHandle").and_then(|p| p.as_bool()) {
            bag.set("requires_handle", PropertyValue::Bool(v));
        }
        if let Some(v) = rbx.property("ToolTip").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("tool_tip", PropertyValue::String(v));
        }
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set("enabled", PropertyValue::Bool(v));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(12);
        import_metadata(toml_value, &mut bag);

        if let Some(props) = toml_value.get("properties") {
            for (key, prop_key) in [
                ("grip_pos", "grip_pos"),
                ("grip_forward", "grip_forward"),
                ("grip_right", "grip_right"),
                ("grip_up", "grip_up"),
            ] {
                if let Some(arr) = props.get(key).and_then(|v| v.as_array()) {
                    bag.set(prop_key, PropertyValue::Vector3(read_vec3_array(arr)));
                }
            }
            for key in ["can_be_dropped", "manual_activation_only", "requires_handle", "enabled"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_bool()) {
                    bag.set(key, PropertyValue::Bool(v));
                }
            }
            if let Some(v) = props.get("tool_tip").and_then(|v| v.as_str()) {
                bag.set("tool_tip", PropertyValue::String(v.to_string()));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "Tool")),
        );
        if let Some(tool) = world.get::<Tool>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("grip_pos".into(), vec3_to_toml(tool.grip_pos));
            props.insert("grip_forward".into(), vec3_to_toml(tool.grip_forward));
            props.insert("grip_right".into(), vec3_to_toml(tool.grip_right));
            props.insert("grip_up".into(), vec3_to_toml(tool.grip_up));
            props.insert("can_be_dropped".into(), toml::Value::Boolean(tool.can_be_dropped));
            props.insert(
                "manual_activation_only".into(),
                toml::Value::Boolean(tool.manual_activation_only),
            );
            props.insert("requires_handle".into(), toml::Value::Boolean(tool.requires_handle));
            props.insert("tool_tip".into(), toml::Value::String(tool.tool_tip.clone()));
            props.insert("enabled".into(), toml::Value::Boolean(tool.enabled));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_tool() {
        assert_eq!(ToolSpawner.class_name(), ClassName::Tool);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(ToolSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_tool_tip_and_enabled() {
        let toml_src = r#"
            [metadata]
            class_name = "Tool"
            name = "Sword"
            [properties]
            tool_tip = "Slash"
            enabled = false
            can_be_dropped = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = ToolSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("metadata.name"), Some("Sword"));
        assert_eq!(bag.get_string("tool_tip"), Some("Slash"));
        assert_eq!(bag.get_bool("enabled"), Some(false));
        assert_eq!(bag.get_bool("can_be_dropped"), Some(false));
    }
}
