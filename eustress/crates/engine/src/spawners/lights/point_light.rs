//! `PointLightSpawner` — `ClassSpawner` for `ClassName::PointLight`.
//!
//! Worked-example implementation from `CLASS_REGISTRY.md` Appendix B
//! plus the per-light backing plan in `LIGHTING_AUDIT.md` §4.2.
//!
//! ## Components attached on spawn
//!
//! - `bevy_pbr::PointLight` — the live rendered light
//!   (`color/intensity/range/radius/shadows_enabled`).
//! - `eustress_common::classes::EustressPointLight` — the Eustress
//!   authoring component (the one the Properties panel + scripts mutate).
//! - `eustress_common::classes::Instance` — class identity + name.
//! - `bevy::prelude::Transform` — pose.
//! - `bevy::core::Name` — Bevy diagnostic name (mirrors `Instance.name`).
//!
//! Texture cookies (`bevy_pbr::PointLightTexture`) are NOT attached yet
//! — that's `LIGHTING_AUDIT.md` step 11 (Wave 3+), and the legacy
//! `spawn_point_light` documents it as a `TODO` (`spawn.rs:416`). This
//! spawner mirrors that gap so its first-spawn behavior is identical to
//! the legacy path the file_loader still uses.
//!
//! ## apply_edit — never respawns
//!
//! Every PointLight authoring property maps to a cheap mutation on the
//! existing `bevy_pbr::PointLight` component. Per spec §2.1 +
//! LIGHTING_AUDIT.md §4.2 "Hot-reload contract", `apply_edit` returns
//! `false` for every prop.
//!
//! ## Serialize / deserialize
//!
//! Per spec §2.1 the bytes are a tagged rkyv-style archive — first byte
//! is the schema tag (Appendix A `group=4 = light` bits), the rest is
//! the deterministic light-field payload. This spawner uses `bincode`
//! to encode a fixed-order struct so the byte representation is stable
//! across rust toolchain upgrades (bincode 1.x freezes the wire shape
//! per the bincode crate's docs). Per `LIGHTING_AUDIT.md` §4.2 "Binary-ECS
//! rkyv layout option 1", this is the cold-tail-only shape — Wave 5
//! upgrades to a typed `ArchLight` slot in `ArchInstanceCore` when load
//! perf measurement shows the round-trip cost matters.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, EustressPointLight, Instance, PropertyValue};

use super::toml_helpers::{
    color_to_color3, descriptor_bool, descriptor_color3, descriptor_f32, descriptor_string,
    read_descriptor_bool, read_descriptor_color3, read_descriptor_f32, read_descriptor_string,
    read_transform_section, transform_to_toml,
};
use super::wire;

/// `ClassSpawner` for `ClassName::PointLight`.
///
/// Stateless unit struct — every spawn pulls its inputs from the
/// `PropertyBag` and emits a fresh entity. Default-constructible so the
/// [`crate::class_registry::RegisterClassExt::register_class`] path
/// works without a custom builder.
#[derive(Default)]
pub struct PointLightSpawner;

impl ClassSpawner for PointLightSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::PointLight
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("PointLight")
            .to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);
        let uuid = props.get_uuid().unwrap_or("").to_string();

        let color = color_from_bag(props, "light.color").unwrap_or(Color::WHITE);
        let brightness = props
            .get_f32("light.brightness")
            .unwrap_or(EustressPointLight::default().brightness);
        let range = props.get_f32("light.range").unwrap_or(60.0);
        let radius = props.get_f32("light.radius").unwrap_or(0.0);
        let shadows = props.get_bool("light.shadows").unwrap_or(true);
        let texture = props
            .get_string("appearance.texture")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let transform = props.get_transform("transform").copied().unwrap_or_default();

        ctx.commands
            .spawn((
                PointLight {
                    color,
                    intensity: brightness,
                    range,
                    radius,
                    shadows_enabled: shadows,
                    ..default()
                },
                transform,
                Instance {
                    name: name.clone(),
                    class_name: ClassName::PointLight,
                    archivable,
                    id: 0,
                    ai: false,
                    uuid,
                },
                EustressPointLight {
                    color,
                    brightness,
                    range,
                    radius,
                    shadows,
                    texture,
                },
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let light = world.entity(entity).get::<EustressPointLight>();
        let transform = world.entity(entity).get::<Transform>();
        let instance = world.entity(entity).get::<Instance>();

        let wire = wire::WireLightCommon {
            metadata: wire::WireMetadata {
                name: instance.map(|i| i.name.clone()).unwrap_or_default(),
                archivable: instance.map(|i| i.archivable).unwrap_or(true),
                uuid: instance.map(|i| i.uuid.clone()).unwrap_or_default(),
            },
            transform: wire::wire_transform(transform.copied().unwrap_or_default()),
            payload: wire::WirePayload::PointLight(wire::WirePointLight {
                color: wire::color_to_rgba(
                    light.map(|l| l.color).unwrap_or(Color::WHITE),
                ),
                brightness: light.map(|l| l.brightness).unwrap_or(0.0),
                range: light.map(|l| l.range).unwrap_or(0.0),
                radius: light.map(|l| l.radius).unwrap_or(0.0),
                shadows: light.map(|l| l.shadows).unwrap_or(true),
                texture: light.and_then(|l| l.texture.clone()),
            }),
        };
        wire::encode(wire::TAG_POINT_LIGHT, &wire)
    }

    fn deserialize(&self, bytes: &[u8]) -> PropertyBag {
        let Some(wire) = wire::decode(wire::TAG_POINT_LIGHT, bytes) else {
            return PropertyBag::new();
        };
        let Some(payload) = wire.payload.into_point_light() else {
            warn!("PointLightSpawner::deserialize: payload variant mismatch");
            return PropertyBag::new();
        };
        let mut bag = PropertyBag::with_capacity(10);
        // Canonical key order per spec §4.3: metadata → transform → light
        // → appearance, mirroring the on-disk PointLight.instance.toml
        // schema below.
        bag.set(
            "metadata.name",
            PropertyValue::String(wire.metadata.name),
        );
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
        bag.set("light.radius", PropertyValue::Float(payload.radius));
        bag.set("light.shadows", PropertyValue::Bool(payload.shadows));
        if let Some(texture) = payload.texture {
            bag.set("appearance.texture", PropertyValue::String(texture));
        }
        bag
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // Sync the Eustress authoring component first so a downstream
        // Changed<EustressPointLight> watcher (spec'd in
        // LIGHTING_AUDIT.md §4.2 — not yet built) sees the same view as
        // the renderer's PointLight gets in the same frame.
        if let Some(mut e) = world.entity_mut(entity).get_mut::<EustressPointLight>() {
            if let Some(c) = read_color(props, "light.color") {
                e.color = c;
            }
            if let Some(b) = props.get_f32("light.brightness") {
                e.brightness = b;
            }
            if let Some(r) = props.get_f32("light.range") {
                e.range = r;
            }
            if let Some(r) = props.get_f32("light.radius") {
                e.radius = r;
            }
            if let Some(s) = props.get_bool("light.shadows") {
                e.shadows = s;
            }
            if let Some(t) = props.get_string("appearance.texture") {
                e.texture = if t.is_empty() { None } else { Some(t.to_string()) };
            }
        }
        if let Some(mut pl) = world.entity_mut(entity).get_mut::<PointLight>() {
            if let Some(c) = read_color(props, "light.color") {
                pl.color = c;
            }
            if let Some(b) = props.get_f32("light.brightness") {
                pl.intensity = b;
            }
            if let Some(r) = props.get_f32("light.range") {
                pl.range = r;
            }
            if let Some(r) = props.get_f32("light.radius") {
                pl.radius = r;
            }
            if let Some(s) = props.get_bool("light.shadows") {
                pl.shadows_enabled = s;
            }
        }
        // Per spec §2.1 + LIGHTING_AUDIT.md §4.2 — every PointLight prop
        // is a cheap in-place mutation. No respawn ever required.
        false
    }

    fn lod_components(&self, tier: LodTier) -> ComponentBundle {
        // LIGHTING_AUDIT.md §4.2 "LOD policy" plus the Visibility-Hidden
        // pattern from `CLASS_REGISTRY.md` Appendix B §B.2.
        //
        // Wave 2/3 LOOP-3 breaker: we touch VISUAL components ONLY. No
        // collider / RigidBody changes — physics LOD is Wave 4.
        match tier {
            LodTier::Hero | LodTier::Active | LodTier::Streamed | LodTier::Horizon => {
                ComponentBundle::empty()
            }
        }
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(8);
        bag.set("metadata.name", PropertyValue::String(rbx.name().into()));
        bag.set("metadata.archivable", PropertyValue::Bool(true));
        // Roblox `Brightness` is a unitless 0..N multiplier; multiply by
        // 800 to land in physically-based lumens — matches the worked
        // example in CLASS_REGISTRY.md Appendix B §B.2.
        if let Some(b) = rbx.property("Brightness").and_then(|p| p.as_f32()) {
            bag.set("light.brightness", PropertyValue::Float(b * 800.0));
        }
        if let Some(r) = rbx.property("Range").and_then(|p| p.as_f32()) {
            bag.set("light.range", PropertyValue::Float(r));
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

        // [transform]
        if let Some(transform_value) = read_transform_section(toml_value) {
            bag.set("transform", PropertyValue::Transform(transform_value));
        }

        // [Light] (Roblox-style PascalCase keys per the template).
        if let Some(light) = toml_value.get("Light").or_else(|| toml_value.get("light")) {
            if let Some(brightness) = read_descriptor_f32(light, "Brightness") {
                bag.set("light.brightness", PropertyValue::Float(brightness));
            }
            if let Some(color3) = read_descriptor_color3(light, "Color") {
                bag.set("light.color", PropertyValue::Color3(color3));
            }
            if let Some(range) = read_descriptor_f32(light, "Range") {
                bag.set("light.range", PropertyValue::Float(range));
            }
            if let Some(radius) = read_descriptor_f32(light, "Radius") {
                bag.set("light.radius", PropertyValue::Float(radius));
            }
            if let Some(shadows) = read_descriptor_bool(light, "Shadows") {
                bag.set("light.shadows", PropertyValue::Bool(shadows));
            }
        }

        // [Appearance]
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
        let light = world.entity(entity).get::<EustressPointLight>();

        // [metadata]
        let mut meta = toml::value::Table::new();
        meta.insert("class_name".into(), "PointLight".into());
        if let Some(inst) = instance {
            meta.insert("name".into(), inst.name.clone().into());
            meta.insert("archivable".into(), inst.archivable.into());
            if !inst.uuid.is_empty() {
                meta.insert("uuid".into(), inst.uuid.clone().into());
            }
        }
        root.insert("metadata".into(), toml::Value::Table(meta));

        // [transform]
        if let Some(t) = transform {
            root.insert("transform".into(), transform_to_toml(*t));
        }

        // [Light] — use Roblox-style PascalCase keys to match the
        // template shape on disk.
        if let Some(l) = light {
            let mut light_section = toml::value::Table::new();
            light_section.insert("Brightness".into(), descriptor_f32(l.brightness));
            light_section.insert("Color".into(), descriptor_color3(color_to_color3(l.color)));
            light_section.insert("Range".into(), descriptor_f32(l.range));
            light_section.insert("Radius".into(), descriptor_f32(l.radius));
            light_section.insert("Shadows".into(), descriptor_bool(l.shadows));
            root.insert("Light".into(), toml::Value::Table(light_section));

            // [Appearance] (only emitted when a texture is present —
            // matches the template's "empty value is omitted" intent;
            // a future round-trip will re-add the empty default from
            // the template merge step).
            if let Some(texture) = &l.texture {
                let mut appearance = toml::value::Table::new();
                appearance.insert("Texture".into(), descriptor_string(texture));
                root.insert("Appearance".into(), toml::Value::Table(appearance));
            }
        }

        toml::Value::Table(root)
    }
}

// ============================================================================
// Local helpers — PropertyBag-side; TOML helpers live in toml_helpers.rs
// ============================================================================

/// Read `Color` from the bag tolerating either the canonical
/// `PropertyValue::Color` or the `Color3` linear-RGB triple form. Returns
/// `None` only when the key is missing or carries an incompatible type.
pub(super) fn read_color(bag: &PropertyBag, key: &str) -> Option<Color> {
    if let Some(c) = bag.get_color(key) {
        return Some(c);
    }
    bag.get_color3(key).map(|[r, g, b]| Color::srgb(r, g, b))
}

/// Promote a `Color3` PropertyValue under the given key into a `Color`,
/// used by the spawn paths where the `Color` accessor may be absent but
/// the template-form `Color3` is present.
pub(super) fn color_from_bag(bag: &PropertyBag, key: &str) -> Option<Color> {
    read_color(bag, key)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn build_bag() -> PropertyBag {
        let mut bag = PropertyBag::new();
        bag.set("metadata.name", PropertyValue::String("Lamp".into()));
        bag.set("metadata.archivable", PropertyValue::Bool(true));
        bag.set(
            "light.color",
            PropertyValue::Color(Color::srgb(1.0, 0.8, 0.5)),
        );
        bag.set("light.brightness", PropertyValue::Float(12345.0));
        bag.set("light.range", PropertyValue::Float(42.0));
        bag.set("light.radius", PropertyValue::Float(0.5));
        bag.set("light.shadows", PropertyValue::Bool(true));
        bag
    }

    #[test]
    fn class_name_is_point_light() {
        assert_eq!(PointLightSpawner.class_name(), ClassName::PointLight);
    }

    #[test]
    fn spawn_attaches_required_components() {
        let mut world = World::new();
        let entity = world
            .spawn((
                PointLight {
                    intensity: 12345.0,
                    range: 42.0,
                    radius: 0.5,
                    shadows_enabled: true,
                    color: Color::srgb(1.0, 0.8, 0.5),
                    ..default()
                },
                Transform::default(),
                Instance {
                    name: "Lamp".into(),
                    class_name: ClassName::PointLight,
                    archivable: true,
                    id: 0,
                    ai: false,
                    uuid: String::new(),
                },
                EustressPointLight {
                    color: Color::srgb(1.0, 0.8, 0.5),
                    brightness: 12345.0,
                    range: 42.0,
                    radius: 0.5,
                    shadows: true,
                    texture: None,
                },
                Name::new("Lamp"),
            ))
            .id();

        // The serialize→deserialize round-trip is the byte-equivalence
        // gate the worked example calls out; we can exercise it without
        // running a full Bevy spawn pipeline.
        let bytes = PointLightSpawner.serialize(&world, entity);
        assert!(
            !bytes.is_empty(),
            "PointLightSpawner::serialize must produce a non-empty tagged buffer"
        );
        assert_eq!(bytes[0], super::wire::TAG_POINT_LIGHT);
        let restored = PointLightSpawner.deserialize(&bytes);
        assert_eq!(restored.get_string("metadata.name"), Some("Lamp"));
        assert_eq!(restored.get_f32("light.brightness"), Some(12345.0));
        assert_eq!(restored.get_f32("light.range"), Some(42.0));
        assert_eq!(restored.get_bool("light.shadows"), Some(true));
    }

    #[test]
    fn apply_edit_returns_false_for_every_prop() {
        let mut world = World::new();
        let entity = world
            .spawn((
                PointLight::default(),
                EustressPointLight::default(),
                Transform::default(),
            ))
            .id();
        let bag = build_bag();
        let respawn = PointLightSpawner.apply_edit(&mut world, entity, &bag);
        assert!(
            !respawn,
            "no PointLight prop should require a respawn — LIGHTING_AUDIT.md §4.2"
        );
        let pl = world.entity(entity).get::<PointLight>().unwrap();
        assert_eq!(pl.intensity, 12345.0);
        assert!(pl.shadows_enabled);
    }

    #[test]
    fn import_from_toml_reads_template_shape() {
        let toml_str = r#"
            [metadata]
            class_name = "PointLight"
            archivable = true

            [transform]
            position = [0.0, 2.0, 0.0]
            rotation = [0.0, 0.0, 0.0, 1.0]
            scale = [1.0, 1.0, 1.0]

            [Light]
            Brightness = { type = "float", value = 100000.0 }
            Color      = { type = "Color3", value = [1.0, 1.0, 1.0] }
            Range      = { type = "float", value = 60.0 }
            Radius     = { type = "float", value = 0.0 }
            Shadows    = { type = "bool",  value = true }
        "#;
        let value: toml::Value = toml::from_str(toml_str).unwrap();
        let bag = PointLightSpawner.import_from_toml(&value);
        assert_eq!(bag.get_f32("light.brightness"), Some(100000.0));
        assert_eq!(bag.get_f32("light.range"), Some(60.0));
        assert_eq!(bag.get_bool("light.shadows"), Some(true));
        assert!(bag.get_transform("transform").is_some());
    }

    #[test]
    fn deserialize_rejects_wrong_tag() {
        // A foreign tag byte should yield an empty bag, never panic.
        let bytes = vec![0xFFu8; 32];
        let bag = PointLightSpawner.deserialize(&bytes);
        assert!(bag.is_empty());
    }
}
