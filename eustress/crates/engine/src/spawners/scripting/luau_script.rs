//! `LuauScriptSpawner` — Wave 5.C scripting-class spawner.
//!
//! Implements [`ClassSpawner`] for [`ClassName::LuauScript`] per
//! `docs/architecture/CLASS_REGISTRY.md` §8.12 (scripting row) + §2 (trait).
//!
//! ## What this is
//!
//! `LuauScript` is the server-side Luau script class — the Eustress
//! equivalent of Roblox `Script` (`RunContext = Server`). The Wave 4
//! Roblox importer already routes Roblox `Script` → `LuauScript` via the
//! `ClassName::from_str` alias (`"Script" | "LuauScript"`), so this is the
//! landing class for imported server scripts.
//!
//! The entity is a **pure source carrier**: it holds the Luau body on its
//! [`LuauScript`] component and nothing visual. No mesh, no physics, no
//! material, no LOD model.
//!
//! ## Execution is NOT this spawner's job
//!
//! Critical boundary (task spec + §8.12): the spawner only builds the
//! entity and stores the source. The existing mlua runtime + Soul/Luau
//! compile-on-play systems own execution — they detect the script
//! component and drive load/run themselves. (Note: the legacy cold-load
//! path at `file_loader.rs:753` currently spawns `.lua` files as
//! `SoulScript` + `SoulScriptData{run_context: Luau}`; the registry path
//! introduced here is the more granular `LuauScript` class the spec
//! mandates. Both store the source body; neither executes it.)
//!
//! Bundle attached:
//!
//! - [`Transform`] (identity — scripts live in the hierarchy but have no
//!   spatial intrinsics)
//! - [`Visibility`] (default — never rendered)
//! - [`Instance`] (with `class_name = ClassName::LuauScript`)
//! - [`LuauScript`] (carries the `source` string + `enabled` + run context)
//! - [`Name`] (mirrors `Instance.name`)
//!
//! See [`soul_script`](super::soul_script) for the fully-annotated
//! reference impl; the four scripting spawners are near-identical modulo
//! the source-carrying component type.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue};
use eustress_common::luau::{LuauRunContext, LuauScript};

use super::soul_script::SOURCE_KEY;

/// Zero-sized spawner for [`ClassName::LuauScript`]. Stateless; per-spawn
/// data flows through the [`PropertyBag`].
#[derive(Default)]
pub struct LuauScriptSpawner;

impl ClassSpawner for LuauScriptSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::LuauScript
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("Script")
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
                    class_name: ClassName::LuauScript,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                LuauScript {
                    name: name.clone(),
                    source,
                    source_path: String::new(),
                    enabled,
                    run_context: LuauRunContext::Server,
                    loaded: false,
                    error: None,
                },
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        // Stub persistence — source round-trips via the script file +
        // `_instance.toml`; the script-group rkyv mirror lights up later.
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
                if let Some(mut script) = entity_mut.get_mut::<LuauScript>() {
                    script.name = new_name.to_string();
                }
            }
            if let Some(archivable) = props.get_bool("metadata.archivable") {
                if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                    instance.archivable = archivable;
                }
            }
            if let Some(mut script) = entity_mut.get_mut::<LuauScript>() {
                if let Some(new_source) = props.get_string(SOURCE_KEY) {
                    script.source = new_source.to_string();
                    script.loaded = false; // force the runtime to re-load
                }
                if let Some(enabled) = props.get_bool("script.enabled") {
                    script.enabled = enabled;
                }
            }
        }

        false // script edits never require a respawn — the runtime reloads
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // Scripts have no LOD model. Empty for all four tiers.
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // Roblox `Script` → `LuauScript`. The body lives in the Roblox
        // `Source` property; `Enabled` toggles execution.
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
        // Mirror the `_instance.toml` shape:
        //
        //     [metadata]
        //     class_name = "LuauScript"
        //     name = "ServerMain"
        //
        //     [script]
        //     source = "main.luau"
        //     enabled = true
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
            toml::Value::String("LuauScript".to_string()),
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

        if let Some(script) = world.entity(entity).get::<LuauScript>() {
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
    fn class_name_is_luau_script() {
        assert_eq!(LuauScriptSpawner.class_name(), ClassName::LuauScript);
    }

    #[test]
    fn luau_script_spawner_is_object_safe() {
        let boxed: Box<dyn ClassSpawner> = Box::new(LuauScriptSpawner);
        assert_eq!(boxed.class_name(), ClassName::LuauScript);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = LuauScriptSpawner;
        for tier in [
            LodTier::Hero,
            LodTier::Active,
            LodTier::Streamed,
            LodTier::Horizon,
        ] {
            assert!(spawner.lod_components(tier).is_empty());
        }
    }

    /// Roblox `Script` maps Source + Enabled into the canonical key space.
    #[test]
    fn import_from_roblox_maps_source_and_enabled() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str {
                "Script"
            }
            fn name(&self) -> &str {
                "ServerMain"
            }
            fn property(
                &self,
                key: &str,
            ) -> Option<eustress_common::class_registry::RobloxPropertyValue> {
                use eustress_common::class_registry::RobloxPropertyValue as V;
                match key {
                    "Source" => Some(V::String("print('hi')".to_string())),
                    "Enabled" => Some(V::Bool(true)),
                    _ => None,
                }
            }
            fn children(&self) -> Vec<&dyn RobloxInstance> {
                Vec::new()
            }
            fn referent(&self) -> u64 {
                1
            }
        }
        let bag = LuauScriptSpawner.import_from_roblox(&Mock);
        assert_eq!(bag.get_string("metadata.name"), Some("ServerMain"));
        assert_eq!(bag.get_string(SOURCE_KEY), Some("print('hi')"));
        assert_eq!(bag.get_bool("script.enabled"), Some(true));
    }

    #[test]
    fn import_from_toml_reads_source_and_enabled() {
        let toml_src = r#"
            [metadata]
            class_name = "LuauScript"
            name = "ServerMain"

            [script]
            source = "main.luau"
            enabled = false
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = LuauScriptSpawner.import_from_toml(&value);
        assert_eq!(bag.get_string("metadata.name"), Some("ServerMain"));
        assert_eq!(bag.get_string(SOURCE_KEY), Some("main.luau"));
        assert_eq!(bag.get_bool("script.enabled"), Some(false));
    }

    #[test]
    fn deserialize_empty_returns_empty_bag() {
        assert!(LuauScriptSpawner.deserialize(&[]).is_empty());
    }
}
