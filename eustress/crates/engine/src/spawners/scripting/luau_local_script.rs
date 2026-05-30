//! `LuauLocalScriptSpawner` — Wave 5.C scripting-class spawner.
//!
//! Implements [`ClassSpawner`] for [`ClassName::LuauLocalScript`] per
//! `docs/architecture/CLASS_REGISTRY.md` §8.12 (scripting row) + §2 (trait).
//!
//! ## What this is
//!
//! `LuauLocalScript` is the client-side Luau script class — the Eustress
//! equivalent of Roblox `LocalScript`. It always runs in client context
//! regardless of parent service. The Wave 4 importer routes Roblox
//! `LocalScript` → `LuauLocalScript` via the `from_str` alias.
//!
//! Like every scripting class it is a **pure source carrier** ([`LuauLocalScript`]
//! holds the `source` string) with no visual, no physics, no LOD model.
//! Execution belongs to the existing mlua runtime — this spawner only
//! builds the entity and stores the source.
//!
//! Bundle attached: [`Transform`] + [`Visibility`] + [`Instance`] +
//! [`LuauLocalScript`] + [`Name`]. See
//! [`soul_script`](super::soul_script) for the fully-annotated reference.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue};
use eustress_common::luau::LuauLocalScript;

use super::soul_script::SOURCE_KEY;

/// Zero-sized spawner for [`ClassName::LuauLocalScript`]. Stateless.
#[derive(Default)]
pub struct LuauLocalScriptSpawner;

impl ClassSpawner for LuauLocalScriptSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::LuauLocalScript
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("LocalScript")
            .to_string();
        let source = props.get_string(SOURCE_KEY).unwrap_or("").to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);
        let enabled = props.get_bool("script.enabled").unwrap_or(true);

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::LuauLocalScript,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                LuauLocalScript {
                    name: name.clone(),
                    source,
                    source_path: String::new(),
                    enabled,
                    loaded: false,
                    error: None,
                },
                Name::new(name),
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
            if let Some(new_name) = props.get_string("metadata.name") {
                if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                    instance.name = new_name.to_string();
                }
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(new_name.to_string());
                }
                if let Some(mut script) = entity_mut.get_mut::<LuauLocalScript>() {
                    script.name = new_name.to_string();
                }
            }
            if let Some(archivable) = props.get_bool("metadata.archivable") {
                if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                    instance.archivable = archivable;
                }
            }
            if let Some(mut script) = entity_mut.get_mut::<LuauLocalScript>() {
                if let Some(new_source) = props.get_string(SOURCE_KEY) {
                    script.source = new_source.to_string();
                    script.loaded = false;
                }
                if let Some(enabled) = props.get_bool("script.enabled") {
                    script.enabled = enabled;
                }
            }
        }

        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(3);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(source) = rbx.property("Source").and_then(|p| p.as_str().map(str::to_owned)) {
            bag.set(SOURCE_KEY, PropertyValue::String(source));
        }
        if let Some(enabled) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set("script.enabled", PropertyValue::Bool(enabled));
        }
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
        if let Some(script) = toml_value.get("script") {
            if let Some(source) = script.get("source").and_then(|v| v.as_str()) {
                bag.set(SOURCE_KEY, PropertyValue::String(source.to_string()));
            }
            if let Some(enabled) = script.get("enabled").and_then(|v| v.as_bool()) {
                bag.set("script.enabled", PropertyValue::Bool(enabled));
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("LuauLocalScript".to_string()),
        );

        if let Some(instance) = world.entity(entity).get::<Instance>() {
            meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
            meta.insert(
                "archivable".to_string(),
                toml::Value::Boolean(instance.archivable),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
            }
        }
        root.insert("metadata".to_string(), toml::Value::Table(meta));

        if let Some(script) = world.entity(entity).get::<LuauLocalScript>() {
            let mut s = toml::value::Table::new();
            s.insert("source".to_string(), toml::Value::String(script.source.clone()));
            s.insert("enabled".to_string(), toml::Value::Boolean(script.enabled));
            root.insert("script".to_string(), toml::Value::Table(s));
        }

        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_luau_local_script() {
        assert_eq!(
            LuauLocalScriptSpawner.class_name(),
            ClassName::LuauLocalScript
        );
    }

    #[test]
    fn luau_local_script_spawner_is_object_safe() {
        let boxed: Box<dyn ClassSpawner> = Box::new(LuauLocalScriptSpawner);
        assert_eq!(boxed.class_name(), ClassName::LuauLocalScript);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = LuauLocalScriptSpawner;
        for tier in [
            LodTier::Hero,
            LodTier::Active,
            LodTier::Streamed,
            LodTier::Horizon,
        ] {
            assert!(spawner.lod_components(tier).is_empty());
        }
    }

    #[test]
    fn import_from_toml_reads_source() {
        let toml_src = r#"
            [metadata]
            class_name = "LuauLocalScript"
            name = "ClientHud"

            [script]
            source = "hud.luau"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = LuauLocalScriptSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("metadata.name"), Some("ClientHud"));
        assert_eq!(bag.get_string(SOURCE_KEY), Some("hud.luau"));
    }

    #[test]
    fn deserialize_empty_returns_empty_bag() {
        assert!(LuauLocalScriptSpawner.deserialize(&[]).is_empty());
    }
}
