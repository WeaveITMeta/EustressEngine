//! `SurfaceLightSpawner` — `ClassSpawner` for `ClassName::SurfaceLight`.
//!
//! The entity carries the [`SurfaceLight`][SurfaceLightComponent]
//! authoring component + `Instance` + `Name`. The light itself is a
//! `bevy_pbr::SpotLight` on an emitter child that
//! `eustress_common::plugins::light_classes` places at the centre of the
//! parent part's `face`, aimed out of it with the cone `angle` — Roblox's
//! SurfaceLight. A face change re-aims that emitter, so no property needs a
//! respawn.
//!
//! The `texture` field is round-tripped through serialize/deserialize; the
//! renderer binding needs Bevy's `pbr_light_textures` feature, which the
//! engine does not enable.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{
    ClassName, Instance, PropertyValue, SurfaceLight as SurfaceLightComponent,
};

use super::point_light::{color_from_bag, read_color};
use super::toml_helpers::{
    color_to_color3, descriptor_bool, descriptor_color3, descriptor_enum, descriptor_f32,
    descriptor_string, read_descriptor_bool, read_descriptor_color3, read_descriptor_enum,
    read_descriptor_f32, read_descriptor_string, read_transform_section, transform_to_toml,
};
use super::wire;

/// Face → local-normal unit vector, Roblox's
/// `Front=-Z, Back=+Z, Top=+Y, Bottom=-Y, Right=+X, Left=-X`. The one
/// definition lives in `light_classes`, which aims the emitter with it.
#[cfg(test)]
fn face_to_local_normal(face: &str) -> Vec3 {
    eustress_common::plugins::light_classes::face_normal(face)
}

#[derive(Default)]
pub struct SurfaceLightSpawner;

impl ClassSpawner for SurfaceLightSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::SurfaceLight
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("SurfaceLight")
            .to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);
        let uuid = props.get_uuid().unwrap_or("").to_string();

        let defaults = SurfaceLightComponent::default();
        let color = color_from_bag(props, "light.color").unwrap_or(Color::WHITE);
        let brightness = props
            .get_f32("light.brightness")
            .unwrap_or(defaults.brightness);
        let range = props.get_f32("light.range").unwrap_or(defaults.range);
        let face = props
            .get_enum("light.face")
            .or_else(|| props.get_string("light.face"))
            .map(eustress_common::plugins::light_classes::normalize_face)
            .unwrap_or(defaults.face.as_str())
            .to_string();
        let angle = props.get_f32("light.angle").unwrap_or(defaults.angle);
        let shadows = props.get_bool("light.shadows").unwrap_or(defaults.shadows);
        // Roblox `Light.Enabled`; a disabled light emits nothing.
        let enabled = props.get_bool("light.enabled").unwrap_or(defaults.enabled);
        let texture = props
            .get_string("appearance.texture")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let transform = props.get_transform("transform").copied().unwrap_or_default();

        // The emitter (a SpotLight at the face, aimed out of it) is built
        // from this authoring component by `light_classes`, like every other
        // spawn path.
        ctx.commands
            .spawn((
                transform,
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::SurfaceLight,
                    archivable,
                    id: 0,
                    ai: false,
                    uuid,
                },
                SurfaceLightComponent {
                    color,
                    brightness,
                    range,
                    face,
                    angle,
                    shadows,
                    enabled,
                    texture,
                },
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let light = world.entity(entity).get::<SurfaceLightComponent>();
        let transform = world.entity(entity).get::<Transform>();
        let instance = world.entity(entity).get::<Instance>();

        let wire = wire::WireLightCommon {
            metadata: wire::WireMetadata {
                name: instance.map(|i| i.name.clone()).unwrap_or_default(),
                archivable: instance.map(|i| i.archivable).unwrap_or(true),
                uuid: instance.map(|i| i.uuid.clone()).unwrap_or_default(),
            },
            transform: wire::wire_transform(transform.copied().unwrap_or_default()),
            payload: wire::WirePayload::SurfaceLight(wire::WireSurfaceLight {
                color: wire::color_to_rgba(light.map(|l| l.color).unwrap_or(Color::WHITE)),
                brightness: light.map(|l| l.brightness).unwrap_or(0.0),
                range: light.map(|l| l.range).unwrap_or(0.0),
                face: light.map(|l| l.face.clone()).unwrap_or_else(|| "Front".into()),
                shadows: light.map(|l| l.shadows).unwrap_or(true),
                texture: light.and_then(|l| l.texture.clone()),
            }),
        };
        wire::encode(wire::TAG_SURFACE_LIGHT, &wire)
    }

    fn deserialize(&self, bytes: &[u8]) -> PropertyBag {
        let Some(wire) = wire::decode(wire::TAG_SURFACE_LIGHT, bytes) else {
            return PropertyBag::new();
        };
        let Some(payload) = wire.payload.into_surface_light() else {
            warn!("SurfaceLightSpawner::deserialize: payload variant mismatch");
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
        bag.set("light.face", PropertyValue::Enum(payload.face));
        bag.set("light.shadows", PropertyValue::Bool(payload.shadows));
        if let Some(texture) = payload.texture {
            bag.set("appearance.texture", PropertyValue::String(texture));
        }
        bag
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // Every property, the face included, is a mutation of the authoring
        // component: `light_classes` re-aims and rebuilds the emitter from
        // it, so nothing needs a respawn.
        let mut entity_mut = world.entity_mut(entity);
        if let Some(mut s) = entity_mut.get_mut::<SurfaceLightComponent>() {
            if let Some(c) = read_color(props, "light.color") {
                s.color = c;
            }
            if let Some(b) = props.get_f32("light.brightness") {
                s.brightness = b;
            }
            if let Some(r) = props.get_f32("light.range") {
                s.range = r;
            }
            if let Some(a) = props.get_f32("light.angle") {
                s.angle = a;
            }
            if let Some(f) = props.get_enum("light.face").or_else(|| props.get_string("light.face")) {
                s.face = eustress_common::plugins::light_classes::normalize_face(f).to_string();
            }
            if let Some(sh) = props.get_bool("light.shadows") {
                s.shadows = sh;
            }
            if let Some(en) = props.get_bool("light.enabled") {
                s.enabled = en;
            }
            if let Some(t) = props.get_string("appearance.texture") {
                s.texture = if t.is_empty() { None } else { Some(t.to_string()) };
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // §4.4 "LOD policy — same as SpotLight tiers" — Wave 3.LOD-system
        // fills the actual tier-driven transitions; this spawner ships
        // the empty-bundle placeholder.
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(8);
        bag.set("metadata.name", PropertyValue::String(rbx.name().into()));
        bag.set("metadata.archivable", PropertyValue::Bool(true));
        if let Some(b) = rbx.property("Brightness").and_then(|p| p.as_f32()) {
            bag.set("light.brightness", PropertyValue::Float(b));
        }
        if let Some(r) = rbx.property("Range").and_then(|p| p.as_f32()) {
            // Studs to metres (1 stud = 1 ft), as the importer converts it.
            bag.set("light.range", PropertyValue::Float(r * 0.3048));
        }
        if let Some(a) = rbx.property("Angle").and_then(|p| p.as_f32()) {
            bag.set("light.angle", PropertyValue::Float(a));
        }
        if let Some(face) = rbx.property("Face").and_then(|p| p.as_str().map(str::to_string)) {
            bag.set("light.face", PropertyValue::Enum(face));
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
            if let Some(r) = read_descriptor_f32(light, "Range") {
                bag.set("light.range", PropertyValue::Float(r));
            }
            if let Some(face) = read_descriptor_enum(light, "Face") {
                bag.set("light.face", PropertyValue::Enum(face));
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
        let light = world.entity(entity).get::<SurfaceLightComponent>();

        let mut meta = toml::value::Table::new();
        meta.insert("class_name".into(), "SurfaceLight".into());
        meta.insert("parent_required".into(), true.into());
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
            section.insert(
                "Face".into(),
                descriptor_enum(
                    l.face.as_str(),
                    &["Top", "Bottom", "Front", "Back", "Left", "Right"],
                ),
            );
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
    fn class_name_is_surface_light() {
        assert_eq!(SurfaceLightSpawner.class_name(), ClassName::SurfaceLight);
    }

    #[test]
    fn face_to_local_normal_covers_six_faces() {
        assert_eq!(face_to_local_normal("Top"), Vec3::Y);
        assert_eq!(face_to_local_normal("Bottom"), Vec3::NEG_Y);
        assert_eq!(face_to_local_normal("Front"), Vec3::NEG_Z);
        assert_eq!(face_to_local_normal("Back"), Vec3::Z);
        assert_eq!(face_to_local_normal("Right"), Vec3::X);
        assert_eq!(face_to_local_normal("Left"), Vec3::NEG_X);
        // Unknown defaults to Front.
        assert_eq!(face_to_local_normal("Diagonal"), Vec3::NEG_Z);
    }

    #[test]
    fn serialize_deserialize_roundtrip_with_face() {
        let mut world = World::new();
        let entity = world
            .spawn((
                Transform::default(),
                Instance {
                    name: "Wall".into(),
                    class_name: ClassName::SurfaceLight,
                    archivable: true,
                    id: 0,
                    ai: false,
                    uuid: String::new(),
                },
                SurfaceLightComponent {
                    color: Color::srgb(1.0, 0.95, 0.8),
                    brightness: 2.0,
                    range: 12.0,
                    face: "Top".to_string(),
                    angle: 120.0,
                    shadows: true,
                    enabled: true,
                    texture: None,
                },
                Name::new("Wall"),
            ))
            .id();
        let bytes = SurfaceLightSpawner.serialize(&world, entity);
        let bag = SurfaceLightSpawner.deserialize(&bytes);
        assert_eq!(bag.get_enum("light.face"), Some("Top"));
        assert_eq!(bag.get_f32("light.brightness"), Some(2.0));
        assert_eq!(bag.get_f32("light.range"), Some(12.0));
    }

    #[test]
    fn apply_edit_face_change_re_aims_without_respawn() {
        let mut world = World::new();
        let entity = world
            .spawn((SurfaceLightComponent::default(), Transform::default()))
            .id();
        let mut bag = PropertyBag::new();
        bag.set("light.face", PropertyValue::Enum("Enum.NormalId.Top".into()));
        let respawn = SurfaceLightSpawner.apply_edit(&mut world, entity, &bag);
        assert!(!respawn, "the emitter re-aims from the component; no respawn");
        let sl = world.entity(entity).get::<SurfaceLightComponent>().unwrap();
        assert_eq!(sl.face, "Top");
    }

    #[test]
    fn apply_edit_non_face_props_dont_require_respawn() {
        let mut world = World::new();
        let entity = world
            .spawn((SurfaceLightComponent::default(), Transform::default()))
            .id();
        let mut bag = PropertyBag::new();
        bag.set("light.brightness", PropertyValue::Float(5.0));
        let respawn = SurfaceLightSpawner.apply_edit(&mut world, entity, &bag);
        assert!(!respawn);
        let sl = world.entity(entity).get::<SurfaceLightComponent>().unwrap();
        assert_eq!(sl.brightness, 5.0);
    }
}
