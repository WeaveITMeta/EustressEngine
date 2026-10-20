//! `DirectionalLightSpawner` — `ClassSpawner` for
//! `ClassName::DirectionalLight`.
//!
//! Standalone directional light per `LIGHTING_AUDIT.md` §4.5 — distinct
//! from the celestial Sun/Moon path which is handled by the existing
//! `plugins::lighting_plugin::hydrate_lighting_entities` and stays out
//! of scope here (see AGENT_DISPATCH.md "Pre-existing Systems").
//!
//! The rendered `bevy::prelude::DirectionalLight` is built from the
//! authoring component by `eustress_common::plugins::light_classes`
//! (brightness 1 is 10,000 lux at daylight exposure, display-referred), so
//! this spawner attaches only the authoring component.
//!
//! Per §4.5 "LOD policy" directional lights have no falloff and aren't
//! LOD-tiered — `lod_components` returns an empty bundle for every
//! tier.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, EustressDirectionalLight, Instance, PropertyValue};

use super::point_light::{color_from_bag, read_color};
use super::toml_helpers::{
    color_to_color3, descriptor_bool, descriptor_color3, descriptor_f32, descriptor_string,
    read_descriptor_bool, read_descriptor_color3, read_descriptor_f32, read_descriptor_string,
    read_transform_section, transform_to_toml,
};
use super::wire;

#[derive(Default)]
pub struct DirectionalLightSpawner;

impl ClassSpawner for DirectionalLightSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::DirectionalLight
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("DirectionalLight")
            .to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);
        let uuid = props.get_uuid().unwrap_or("").to_string();

        let defaults = EustressDirectionalLight::default();
        let color = color_from_bag(props, "light.color").unwrap_or(Color::WHITE);
        let brightness = props
            .get_f32("light.brightness")
            .unwrap_or(defaults.brightness);
        let shadows = props.get_bool("light.shadows").unwrap_or(defaults.shadows);
        let shadow_depth_bias = props
            .get_f32("shadows.depth_bias")
            .unwrap_or(defaults.shadow_depth_bias);
        let shadow_normal_bias = props
            .get_f32("shadows.normal_bias")
            .unwrap_or(defaults.shadow_normal_bias);
        let texture = props
            .get_string("appearance.texture")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let enabled = props.get_bool("light.enabled").unwrap_or(defaults.enabled);

        let transform = props.get_transform("transform").copied().unwrap_or_default();

        ctx.commands
            .spawn((
                transform,
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::DirectionalLight,
                    archivable,
                    id: 0,
                    ai: false,
                    uuid,
                },
                EustressDirectionalLight {
                    color,
                    brightness,
                    shadows,
                    enabled,
                    shadow_depth_bias,
                    shadow_normal_bias,
                    texture,
                },
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let light = world.entity(entity).get::<EustressDirectionalLight>();
        let transform = world.entity(entity).get::<Transform>();
        let instance = world.entity(entity).get::<Instance>();

        let wire = wire::WireLightCommon {
            metadata: wire::WireMetadata {
                name: instance.map(|i| i.name.clone()).unwrap_or_default(),
                archivable: instance.map(|i| i.archivable).unwrap_or(true),
                uuid: instance.map(|i| i.uuid.clone()).unwrap_or_default(),
            },
            transform: wire::wire_transform(transform.copied().unwrap_or_default()),
            payload: wire::WirePayload::DirectionalLight(wire::WireDirectionalLight {
                color: wire::color_to_rgba(light.map(|l| l.color).unwrap_or(Color::WHITE)),
                brightness: light.map(|l| l.brightness).unwrap_or(1.0),
                shadows: light.map(|l| l.shadows).unwrap_or(true),
                shadow_depth_bias: light.map(|l| l.shadow_depth_bias).unwrap_or(0.02),
                shadow_normal_bias: light.map(|l| l.shadow_normal_bias).unwrap_or(1.8),
                texture: light.and_then(|l| l.texture.clone()),
            }),
        };
        wire::encode(wire::TAG_DIRECTIONAL_LIGHT, &wire)
    }

    fn deserialize(&self, bytes: &[u8]) -> PropertyBag {
        let Some(wire) = wire::decode(wire::TAG_DIRECTIONAL_LIGHT, bytes) else {
            return PropertyBag::new();
        };
        let Some(payload) = wire.payload.into_directional_light() else {
            warn!("DirectionalLightSpawner::deserialize: payload variant mismatch");
            return PropertyBag::new();
        };
        let mut bag = PropertyBag::with_capacity(10);
        bag.set("metadata.name", PropertyValue::String(wire.metadata.name));
        bag.set(
            "metadata.archivable",
            PropertyValue::Bool(wire.metadata.archivable),
        );
        if !wire.metadata.uuid.is_empty() {
            bag.set("metadata.uuid", PropertyValue::String(wire.metadata.uuid));
        }
        bag.set(
            "transform",
            PropertyValue::Transform(wire::wire_to_transform(&wire.transform)),
        );
        bag.set(
            "light.color",
            PropertyValue::Color(wire::rgba_to_color(payload.color)),
        );
        bag.set("light.brightness", PropertyValue::Float(payload.brightness));
        bag.set("light.shadows", PropertyValue::Bool(payload.shadows));
        bag.set(
            "shadows.depth_bias",
            PropertyValue::Float(payload.shadow_depth_bias),
        );
        bag.set(
            "shadows.normal_bias",
            PropertyValue::Float(payload.shadow_normal_bias),
        );
        if let Some(texture) = payload.texture {
            bag.set("appearance.texture", PropertyValue::String(texture));
        }
        bag
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        if let Some(mut e) = world.entity_mut(entity).get_mut::<EustressDirectionalLight>() {
            if let Some(c) = read_color(props, "light.color") {
                e.color = c;
            }
            if let Some(b) = props.get_f32("light.brightness") {
                e.brightness = b;
            }
            if let Some(s) = props.get_bool("light.shadows") {
                e.shadows = s;
            }
            if let Some(b) = props.get_f32("shadows.depth_bias") {
                e.shadow_depth_bias = b;
            }
            if let Some(b) = props.get_f32("shadows.normal_bias") {
                e.shadow_normal_bias = b;
            }
            if let Some(t) = props.get_string("appearance.texture") {
                e.texture = if t.is_empty() { None } else { Some(t.to_string()) };
            }
            if let Some(en) = props.get_bool("light.enabled") {
                e.enabled = en;
            }
        }
        // The rendered DirectionalLight follows the authoring component
        // through `light_classes`; nothing here writes it.
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // §4.5 "LOD policy — directional lights have no falloff. No LOD
        // tiering." Empty bundle for every tier.
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(8);
        bag.set("metadata.name", PropertyValue::String(rbx.name().into()));
        bag.set("metadata.archivable", PropertyValue::Bool(true));
        // Roblox doesn't ship a discrete DirectionalLight class — the
        // closest analogue is `Lighting.Brightness` (a scalar). We
        // accept a `Brightness` property here for symmetry with the
        // other lights, but a well-formed RBXL won't carry it.
        if let Some(b) = rbx.property("Brightness").and_then(|p| p.as_f32()) {
            bag.set("light.brightness", PropertyValue::Float(b));
        }
        if let Some(s) = rbx.property("Shadows").and_then(|p| p.as_bool()) {
            bag.set("light.shadows", PropertyValue::Bool(s));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(10);

        if let Some(meta) = toml_value.get("metadata") {
            if let Some(n) = meta.get("name").and_then(|v| v.as_str()) {
                bag.set("metadata.name", PropertyValue::String(n.into()));
            }
            if let Some(a) = meta.get("archivable").and_then(|v| v.as_bool()) {
                bag.set("metadata.archivable", PropertyValue::Bool(a));
            }
            if let Some(u) = meta.get("uuid").and_then(|v| v.as_str()) {
                if !u.is_empty() {
                    bag.set("metadata.uuid", PropertyValue::String(u.into()));
                }
            }
        }
        if let Some(t) = read_transform_section(toml_value) {
            bag.set("transform", PropertyValue::Transform(t));
        }
        if let Some(light) = toml_value.get("Light").or_else(|| toml_value.get("light")) {
            if let Some(b) = read_descriptor_f32(light, "Brightness") {
                bag.set("light.brightness", PropertyValue::Float(b));
            }
            if let Some(c) = read_descriptor_color3(light, "Color") {
                bag.set("light.color", PropertyValue::Color3(c));
            }
            if let Some(s) = read_descriptor_bool(light, "Shadows") {
                bag.set("light.shadows", PropertyValue::Bool(s));
            }
            if let Some(b) = read_descriptor_f32(light, "ShadowDepthBias") {
                bag.set("shadows.depth_bias", PropertyValue::Float(b));
            }
            if let Some(b) = read_descriptor_f32(light, "ShadowNormalBias") {
                bag.set("shadows.normal_bias", PropertyValue::Float(b));
            }
        }
        if let Some(app) = toml_value
            .get("Appearance")
            .or_else(|| toml_value.get("appearance"))
        {
            if let Some(t) = read_descriptor_string(app, "Texture") {
                if !t.is_empty() {
                    bag.set("appearance.texture", PropertyValue::String(t));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let instance = world.entity(entity).get::<Instance>();
        let transform = world.entity(entity).get::<Transform>();
        let light = world.entity(entity).get::<EustressDirectionalLight>();

        let mut meta = toml::value::Table::new();
        meta.insert("class_name".into(), "DirectionalLight".into());
        if let Some(inst) = instance {
            meta.insert("name".into(), inst.name.clone().into());
            meta.insert("archivable".into(), inst.archivable.into());
            if !inst.uuid.is_empty() {
                meta.insert("uuid".into(), inst.uuid.clone().into());
            }
        }
        root.insert("metadata".into(), toml::Value::Table(meta));

        if let Some(t) = transform {
            root.insert("transform".into(), transform_to_toml(*t));
        }
        if let Some(l) = light {
            let mut section = toml::value::Table::new();
            section.insert("Brightness".into(), descriptor_f32(l.brightness));
            section.insert("Color".into(), descriptor_color3(color_to_color3(l.color)));
            section.insert("Shadows".into(), descriptor_bool(l.shadows));
            section.insert(
                "ShadowDepthBias".into(),
                descriptor_f32(l.shadow_depth_bias),
            );
            section.insert(
                "ShadowNormalBias".into(),
                descriptor_f32(l.shadow_normal_bias),
            );
            root.insert("Light".into(), toml::Value::Table(section));

            if let Some(texture) = &l.texture {
                let mut appearance = toml::value::Table::new();
                appearance.insert("Texture".into(), descriptor_string(texture));
                root.insert("Appearance".into(), toml::Value::Table(appearance));
            }
        }
        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_directional_light() {
        assert_eq!(
            DirectionalLightSpawner.class_name(),
            ClassName::DirectionalLight
        );
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let mut world = World::new();
        let entity = world
            .spawn((
                DirectionalLight::default(),
                Transform::default(),
                Instance {
                    name: "StageSun".into(),
                    class_name: ClassName::DirectionalLight,
                    archivable: true,
                    id: 0,
                    ai: false,
                    uuid: String::new(),
                },
                EustressDirectionalLight {
                    color: Color::srgb(1.0, 0.97, 0.92),
                    brightness: 1.5,
                    shadows: true,
                    enabled: true,
                    shadow_depth_bias: 0.025,
                    shadow_normal_bias: 2.0,
                    texture: None,
                },
                Name::new("StageSun"),
            ))
            .id();
        let bytes = DirectionalLightSpawner.serialize(&world, entity);
        let bag = DirectionalLightSpawner.deserialize(&bytes);
        assert_eq!(bag.get_f32("light.brightness"), Some(1.5));
        assert_eq!(bag.get_bool("light.shadows"), Some(true));
        assert_eq!(bag.get_f32("shadows.depth_bias"), Some(0.025));
        assert_eq!(bag.get_f32("shadows.normal_bias"), Some(2.0));
    }

    #[test]
    fn apply_edit_updates_the_authoring_component() {
        let mut world = World::new();
        let entity = world
            .spawn((EustressDirectionalLight::default(), Transform::default()))
            .id();
        let mut bag = PropertyBag::new();
        bag.set("light.brightness", PropertyValue::Float(2.0));
        bag.set("light.enabled", PropertyValue::Bool(false));
        let respawn = DirectionalLightSpawner.apply_edit(&mut world, entity, &bag);
        assert!(!respawn);
        let light = world.entity(entity).get::<EustressDirectionalLight>().unwrap();
        assert_eq!(light.brightness, 2.0);
        assert!(!light.enabled);
    }
}
