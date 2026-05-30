//! `LuauModuleScriptSpawner` — Wave 5.C scripting-class spawner.
//!
//! Implements [`ClassSpawner`] for [`ClassName::LuauModuleScript`] per
//! `docs/architecture/CLASS_REGISTRY.md` §8.12 (scripting row) + §2 (trait).
//!
//! ## What this is
//!
//! `LuauModuleScript` is the reusable Luau module class — the Eustress
//! equivalent of Roblox `ModuleScript`. It returns a table when `require()`d
//! and is shared between server and client contexts. The Wave 4 importer
//! routes Roblox `ModuleScript` → `LuauModuleScript` via the `from_str`
//! alias.
//!
//! Modules have **no `enabled` flag** (they run on demand via `require`),
//! so the spawner carries only the `source` body + name. Like every
//! scripting class it is a pure source carrier with no visual, no physics,
//! no LOD model. Execution / caching belongs to the existing mlua runtime —
//! this spawner only builds the entity and stores the source.
//!
//! Bundle attached: [`Transform`] + [`Visibility`] + [`Instance`] +
//! [`LuauModuleScript`] + [`Name`]. See
//! [`soul_script`](super::soul_script) for the fully-annotated reference.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue};
use eustress_common::luau::LuauModuleScript;

use super::soul_script::SOURCE_KEY;

/// Zero-sized spawner for [`ClassName::LuauModuleScript`]. Stateless.
#[derive(Default)]
pub struct LuauModuleScriptSpawner;

impl ClassSpawner for LuauModuleScriptSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::LuauModuleScript
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("ModuleScript")
            .to_string();
        let source = props.get_string(SOURCE_KEY).unwrap_or("").to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::LuauModuleScript,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                LuauModuleScript {
                    name: name.clone(),
                    source,
                    source_path: String::new(),
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
                if let Some(mut script) = entity_mut.get_mut::<LuauModuleScript>() {
                    script.name = new_name.to_string();
                }
            }
            if let Some(archivable) = props.get_bool("metadata.archivable") {
                if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                    instance.archivable = archivable;
                }
            }
            if let Some(mut script) = entity_mut.get_mut::<LuauModuleScript>() {
                if let Some(new_source) = props.get_string(SOURCE_KEY) {
                    script.source = new_source.to_string();
                    // Invalidate the module cache so the next `require`
                    // re-evaluates. The runtime owns the actual cache.
                    script.loaded = false;
                }
            }
        }

        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // Modules carry only `Source` (no `Enabled` — they run on require).
        let mut bag = PropertyBag::with_capacity(2);
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));
        if let Some(source) = rbx.property("Source").and_then(|p| p.as_str().map(str::to_owned)) {
            bag.set(SOURCE_KEY, PropertyValue::String(source));
        }
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
        if let Some(script) = toml_value.get("script") {
            if let Some(source) = script.get("source").and_then(|v| v.as_str()) {
                bag.set(SOURCE_KEY, PropertyValue::String(source.to_string()));
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();

        meta.insert(
            "class_name".to_string(),
            toml::Value::String("LuauModuleScript".to_string()),
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

        if let Some(script) = world.entity(entity).get::<LuauModuleScript>() {
            let mut s = toml::value::Table::new();
            s.insert("source".to_string(), toml::Value::String(script.source.clone()));
            root.insert("script".to_string(), toml::Value::Table(s));
        }

        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_luau_module_script() {
        assert_eq!(
            LuauModuleScriptSpawner.class_name(),
            ClassName::LuauModuleScript
        );
    }

    #[test]
    fn luau_module_script_spawner_is_object_safe() {
        let boxed: Box<dyn ClassSpawner> = Box::new(LuauModuleScriptSpawner);
        assert_eq!(boxed.class_name(), ClassName::LuauModuleScript);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = LuauModuleScriptSpawner;
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
            class_name = "LuauModuleScript"
            name = "MathUtils"

            [script]
            source = "math_utils.luau"
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = LuauModuleScriptSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("metadata.name"), Some("MathUtils"));
        assert_eq!(bag.get_string(SOURCE_KEY), Some("math_utils.luau"));
    }

    #[test]
    fn deserialize_empty_returns_empty_bag() {
        assert!(LuauModuleScriptSpawner.deserialize(&[]).is_empty());
    }
}
