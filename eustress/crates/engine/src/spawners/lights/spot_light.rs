//! `SpotLightSpawner` — `ClassSpawner` for `ClassName::SpotLight`.
//!
//! Follows the PointLight worked example with three deltas (per
//! `LIGHTING_AUDIT.md` §4.3):
//!
//! 1. The Bevy backing component is `bevy_pbr::SpotLight` (carries
//!    `inner_angle` + `outer_angle`).
//! 2. The Eustress authoring component exposes a single `angle` field
//!    (the outer cone half-angle in degrees). The spawner synthesizes
//!    `inner_angle = outer * 0.85` per `spawn.rs::spawn_spot_light`'s
//!    convention — open question §8 #2 in the audit; we keep the legacy
//!    convention so this PR is a behavioral no-op for existing SpotLights.
//! 3. The LOD policy table is the same as PointLight (no shadow → drop
//!    → cull) — see `LIGHTING_AUDIT.md` §4.3 "LOD policy".

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, EustressSpotLight, Instance, PropertyValue};

use super::point_light::{color_from_bag, read_color};
use super::toml_helpers::{
    color_to_color3, descriptor_bool, descriptor_color3, descriptor_f32, descriptor_string,
    read_descriptor_bool, read_descriptor_color3, read_descriptor_f32, read_descriptor_string,
    read_transform_section, transform_to_toml,
};
use super::wire;

/// `ClassSpawner` for `ClassName::SpotLight`.
#[derive(Default)]
pub struct SpotLightSpawner;

impl ClassSpawner for SpotLightSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::SpotLight
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("SpotLight")
            .to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);
        let uuid = props.get_uuid().unwrap_or("").to_string();

        let defaults = EustressSpotLight::default();
        let color = color_from_bag(props, "light.color").unwrap_or(Color::WHITE);
        let brightness = props
            .get_f32("light.brightness")
            .unwrap_or(defaults.brightness);
        let range = props.get_f32("light.range").unwrap_or(defaults.range);
        let angle_deg = props.get_f32("light.angle").unwrap_or(defaults.angle);
        let shadows = props.get_bool("light.shadows").unwrap_or(defaults.shadows);
        let texture = props
            .get_string("appearance.texture")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let transform = props.get_transform("transform").copied().unwrap_or_default();

        // Mirror the legacy `spawn_spot_light` convention:
        // inner = 0.85 * outer (see spawn.rs:450).
        let outer_rad = angle_deg.to_radians();
        let inner_rad = (angle_deg * 0.85).to_radians();

        ctx.commands
            .spawn((
                SpotLight {
                    color,
                    intensity: brightness,
                    range,
                    inner_angle: inner_rad,
                    outer_angle: outer_rad,
                    shadows_enabled: shadows,
                    ..default()
                },
                transform,
                Instance {
                    name: name.clone(),
                    class_name: ClassName::SpotLight,
                    archivable,
                    id: 0,
                    ai: false,
                    uuid,
                },
                EustressSpotLight {
                    color,
                    brightness,
                    range,
                    angle: angle_deg,
                    shadows,
                    texture,
                },
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let light = world.entity(entity).get::<EustressSpotLight>();
        let transform = world.entity(entity).get::<Transform>();
        let instance = world.entity(entity).get::<Instance>();

        let wire = wire::WireLightCommon {
            metadata: wire::WireMetadata {
                name: instance.map(|i| i.name.clone()).unwrap_or_default(),
                archivable: instance.map(|i| i.archivable).unwrap_or(true),
                uuid: instance.map(|i| i.uuid.clone()).unwrap_or_default(),
            },
            transform: wire::wire_transform(transform.copied().unwrap_or_default()),
            payload: wire::WirePayload::SpotLight(wire::WireSpotLight {
                color: wire::color_to_rgba(light.map(|l| l.color).unwrap_or(Color::WHITE)),
                brightness: light.map(|l| l.brightness).unwrap_or(0.0),
                range: light.map(|l| l.range).unwrap_or(0.0),
                angle_deg: light.map(|l| l.angle).unwrap_or(0.0),
                shadows: light.map(|l| l.shadows).unwrap_or(true),
                texture: light.and_then(|l| l.texture.clone()),
            }),
        };
        wire::encode(wire::TAG_SPOT_LIGHT, &wire)
    }

    fn deserialize(&self, bytes: &[u8]) -> PropertyBag {
        let Some(wire) = wire::decode(wire::TAG_SPOT_LIGHT, bytes) else {
            return PropertyBag::new();
        };
        let Some(payload) = wire.payload.into_spot_light() else {
            warn!("SpotLightSpawner::deserialize: payload variant mismatch");
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
        bag.set("light.range", PropertyValue::Float(payload.range));
        bag.set("light.angle", PropertyValue::Float(payload.angle_deg));
        bag.set("light.shadows", PropertyValue::Bool(payload.shadows));
        if let Some(texture) = payload.texture {
            bag.set("appearance.texture", PropertyValue::String(texture));
        }
        bag
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        if let Some(mut e) = world.entity_mut(entity).get_mut::<EustressSpotLight>() {
            if let Some(c) = read_color(props, "light.color") {
                e.color = c;
            }
            if let Some(b) = props.get_f32("light.brightness") {
                e.brightness = b;
            }
            if let Some(r) = props.get_f32("light.range") {
                e.range = r;
            }
            if let Some(a) = props.get_f32("light.angle") {
                e.angle = a;
            }
            if let Some(s) = props.get_bool("light.shadows") {
                e.shadows = s;
            }
            if let Some(t) = props.get_string("appearance.texture") {
                e.texture = if t.is_empty() { None } else { Some(t.to_string()) };
            }
        }
        if let Some(mut sl) = world.entity_mut(entity).get_mut::<SpotLight>() {
            if let Some(c) = read_color(props, "light.color") {
                sl.color = c;
            }
            if let Some(b) = props.get_f32("light.brightness") {
                sl.intensity = b;
            }
            if let Some(r) = props.get_f32("light.range") {
                sl.range = r;
            }
            if let Some(a) = props.get_f32("light.angle") {
                sl.outer_angle = a.to_radians();
                sl.inner_angle = (a * 0.85).to_radians();
            }
            if let Some(s) = props.get_bool("light.shadows") {
                sl.shadows_enabled = s;
            }
        }
        // Every SpotLight prop is a cheap mutation — LIGHTING_AUDIT.md §4.3
        // does not list any respawn-requiring property.
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // Same policy as PointLight per LIGHTING_AUDIT.md §4.3 — we ship
        // the empty bundle today; Wave 3.LOD-system fills the actual
        // tier-driven transitions when `apply_lod_transitions` lands.
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(8);
        bag.set("metadata.name", PropertyValue::String(rbx.name().into()));
        bag.set("metadata.archivable", PropertyValue::Bool(true));
        if let Some(b) = rbx.property("Brightness").and_then(|p| p.as_f32()) {
            // Same Roblox-units → lumens scale as PointLight (B.2).
            bag.set("light.brightness", PropertyValue::Float(b * 800.0));
        }
        if let Some(r) = rbx.property("Range").and_then(|p| p.as_f32()) {
            bag.set("light.range", PropertyValue::Float(r));
        }
        if let Some(a) = rbx.property("Angle").and_then(|p| p.as_f32()) {
            bag.set("light.angle", PropertyValue::Float(a));
        }
        if let Some(s) = rbx.property("Shadows").and_then(|p| p.as_bool()) {
            bag.set("light.shadows", PropertyValue::Bool(s));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(10);

        // [metadata]
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
            if let Some(r) = read_descriptor_f32(light, "Range") {
                bag.set("light.range", PropertyValue::Float(r));
            }
            if let Some(a) = read_descriptor_f32(light, "Angle") {
                bag.set("light.angle", PropertyValue::Float(a));
            }
            if let Some(s) = read_descriptor_bool(light, "Shadows") {
                bag.set("light.shadows", PropertyValue::Bool(s));
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
        let light = world.entity(entity).get::<EustressSpotLight>();

        let mut meta = toml::value::Table::new();
        meta.insert("class_name".into(), "SpotLight".into());
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
            section.insert("Range".into(), descriptor_f32(l.range));
            section.insert("Angle".into(), descriptor_f32(l.angle));
            section.insert("Shadows".into(), descriptor_bool(l.shadows));
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
    fn class_name_is_spot_light() {
        assert_eq!(SpotLightSpawner.class_name(), ClassName::SpotLight);
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let mut world = World::new();
        let entity = world
            .spawn((
                SpotLight {
                    color: Color::srgb(0.5, 0.5, 1.0),
                    intensity: 50_000.0,
                    range: 30.0,
                    outer_angle: 1.0,
                    inner_angle: 0.85,
                    shadows_enabled: true,
                    ..default()
                },
                Transform::default(),
                Instance {
                    name: "Stage".into(),
                    class_name: ClassName::SpotLight,
                    archivable: true,
                    id: 0,
                    ai: false,
                    uuid: String::new(),
                },
                EustressSpotLight {
                    color: Color::srgb(0.5, 0.5, 1.0),
                    brightness: 50_000.0,
                    range: 30.0,
                    angle: 60.0,
                    shadows: true,
                    texture: None,
                },
                Name::new("Stage"),
            ))
            .id();

        let bytes = SpotLightSpawner.serialize(&world, entity);
        assert!(!bytes.is_empty());
        let restored = SpotLightSpawner.deserialize(&bytes);
        assert_eq!(restored.get_string("metadata.name"), Some("Stage"));
        assert_eq!(restored.get_f32("light.angle"), Some(60.0));
        assert_eq!(restored.get_f32("light.range"), Some(30.0));
    }

    #[test]
    fn apply_edit_updates_inner_outer_angle_in_lockstep() {
        let mut world = World::new();
        let entity = world
            .spawn((
                SpotLight::default(),
                EustressSpotLight::default(),
                Transform::default(),
            ))
            .id();
        let mut bag = PropertyBag::new();
        bag.set("light.angle", PropertyValue::Float(90.0));
        let respawn = SpotLightSpawner.apply_edit(&mut world, entity, &bag);
        assert!(!respawn);
        let sl = world.entity(entity).get::<SpotLight>().unwrap();
        // Within float tolerance.
        assert!((sl.outer_angle - 90.0_f32.to_radians()).abs() < 1e-5);
        assert!((sl.inner_angle - (90.0_f32 * 0.85).to_radians()).abs() < 1e-5);
    }
}
