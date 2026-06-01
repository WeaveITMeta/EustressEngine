//! `UIPageLayoutSpawner` — `ClassSpawner` for [`ClassName::UIPageLayout`].
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 (Wave 7.B). Data-attach
//! modifier: paginated scrolling layout.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, PropertyValue, UIPageLayout};
use eustress_common::{Attributes, Tags};

use super::{apply_metadata_edit, export_metadata, import_metadata, instance_from_bag};

/// Zero-sized spawner for [`ClassName::UIPageLayout`].
#[derive(Default)]
pub struct UIPageLayoutSpawner;

impl ClassSpawner for UIPageLayoutSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::UIPageLayout
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let instance = instance_from_bag(ClassName::UIPageLayout, props);
        let name = instance.name.clone();
        let d = UIPageLayout::default();
        let comp = UIPageLayout {
            padding: props.get_f32("padding").unwrap_or(d.padding),
            animated: props.get_bool("animated").unwrap_or(d.animated),
            circular: props.get_bool("circular").unwrap_or(d.circular),
            easing_direction: props.get_string("easing_direction").map(str::to_string).unwrap_or(d.easing_direction),
            easing_style: props.get_string("easing_style").map(str::to_string).unwrap_or(d.easing_style),
            fill_direction: props.get_string("fill_direction").map(str::to_string).unwrap_or(d.fill_direction),
            gamepad_input_enabled: props.get_bool("gamepad_input_enabled").unwrap_or(d.gamepad_input_enabled),
            scroll_wheel_input_enabled: props.get_bool("scroll_wheel_input_enabled").unwrap_or(d.scroll_wheel_input_enabled),
            touch_input_enabled: props.get_bool("touch_input_enabled").unwrap_or(d.touch_input_enabled),
            tween_time: props.get_f32("tween_time").unwrap_or(d.tween_time),
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
        if let Some(mut comp) = world.get_mut::<UIPageLayout>(entity) {
            if let Some(v) = props.get_f32("padding") { comp.padding = v; }
            if let Some(v) = props.get_bool("animated") { comp.animated = v; }
            if let Some(v) = props.get_bool("circular") { comp.circular = v; }
            if let Some(v) = props.get_string("easing_direction") { comp.easing_direction = v.to_string(); }
            if let Some(v) = props.get_string("easing_style") { comp.easing_style = v.to_string(); }
            if let Some(v) = props.get_string("fill_direction") { comp.fill_direction = v.to_string(); }
            if let Some(v) = props.get_bool("gamepad_input_enabled") { comp.gamepad_input_enabled = v; }
            if let Some(v) = props.get_bool("scroll_wheel_input_enabled") { comp.scroll_wheel_input_enabled = v; }
            if let Some(v) = props.get_bool("touch_input_enabled") { comp.touch_input_enabled = v; }
            if let Some(v) = props.get_f32("tween_time") { comp.tween_time = v; }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(6);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(v) = rbx.property("Padding").and_then(|p| p.as_f32()) {
            bag.set("padding", PropertyValue::Float(v));
        }
        if let Some(v) = rbx.property("TweenTime").and_then(|p| p.as_f32()) {
            bag.set("tween_time", PropertyValue::Float(v));
        }
        for (rbx_key, key) in [
            ("Animated", "animated"),
            ("Circular", "circular"),
            ("GamepadInputEnabled", "gamepad_input_enabled"),
            ("ScrollWheelInputEnabled", "scroll_wheel_input_enabled"),
            ("TouchInputEnabled", "touch_input_enabled"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_bool()) {
                bag.set(key, PropertyValue::Bool(v));
            }
        }
        for (rbx_key, key) in [
            ("EasingDirection", "easing_direction"),
            ("EasingStyle", "easing_style"),
            ("FillDirection", "fill_direction"),
        ] {
            if let Some(v) = rbx.property(rbx_key).and_then(|p| p.as_str().map(str::to_string)) {
                bag.set(key, PropertyValue::String(v));
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(11);
        import_metadata(toml_value, &mut bag);
        if let Some(props) = toml_value.get("properties") {
            for key in ["padding", "tween_time"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_float()) {
                    bag.set(key, PropertyValue::Float(v as f32));
                }
            }
            for key in [
                "animated", "circular", "gamepad_input_enabled",
                "scroll_wheel_input_enabled", "touch_input_enabled",
            ] {
                if let Some(v) = props.get(key).and_then(|v| v.as_bool()) {
                    bag.set(key, PropertyValue::Bool(v));
                }
            }
            for key in ["easing_direction", "easing_style", "fill_direction"] {
                if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                    bag.set(key, PropertyValue::String(v.to_string()));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        root.insert(
            "metadata".to_string(),
            toml::Value::Table(export_metadata(world, entity, "UIPageLayout")),
        );
        if let Some(comp) = world.get::<UIPageLayout>(entity) {
            let mut props = toml::value::Table::new();
            props.insert("padding".into(), toml::Value::Float(comp.padding as f64));
            props.insert("animated".into(), toml::Value::Boolean(comp.animated));
            props.insert("circular".into(), toml::Value::Boolean(comp.circular));
            props.insert("easing_direction".into(), toml::Value::String(comp.easing_direction.clone()));
            props.insert("easing_style".into(), toml::Value::String(comp.easing_style.clone()));
            props.insert("fill_direction".into(), toml::Value::String(comp.fill_direction.clone()));
            props.insert("gamepad_input_enabled".into(), toml::Value::Boolean(comp.gamepad_input_enabled));
            props.insert("scroll_wheel_input_enabled".into(), toml::Value::Boolean(comp.scroll_wheel_input_enabled));
            props.insert("touch_input_enabled".into(), toml::Value::Boolean(comp.touch_input_enabled));
            props.insert("tween_time".into(), toml::Value::Float(comp.tween_time as f64));
            root.insert("properties".to_string(), toml::Value::Table(props));
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_ui_page_layout() {
        assert_eq!(UIPageLayoutSpawner.class_name(), ClassName::UIPageLayout);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        for tier in [LodTier::Hero, LodTier::Active, LodTier::Streamed, LodTier::Horizon] {
            assert!(UIPageLayoutSpawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_props() {
        let toml_src = r#"
            [metadata]
            class_name = "UIPageLayout"
            name = "Pages"
            [properties]
            circular = true
            tween_time = 0.5
            easing_style = "Sine"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = UIPageLayoutSpawner.import_from_toml(&value);
        assert_eq!(bag.get_bool("circular"), Some(true));
        assert_eq!(bag.get_f32("tween_time"), Some(0.5));
        assert_eq!(bag.get_string("easing_style"), Some("Sine"));
    }
}
