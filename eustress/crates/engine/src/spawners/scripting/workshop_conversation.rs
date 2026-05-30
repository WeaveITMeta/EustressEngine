//! `WorkshopConversationSpawner` — Wave 5.C scripting-class spawner (stub).
//!
//! Implements [`ClassSpawner`] for [`ClassName::WorkshopConversation`] per
//! `docs/architecture/CLASS_REGISTRY.md` §8.12 (scripting row, marked
//! "(stub)") + §2 (trait).
//!
//! ## What this is
//!
//! A `WorkshopConversation` is a saved AI Workshop session — the transcript
//! of a product-design conversation stored under `SoulService/Workshop/`.
//! The engine's Workshop panel persists each session as a folder with an
//! `_instance.toml` (`class_name = "WorkshopConversation"`, a `[properties]`
//! block carrying `session_id` / `product_name` / `message_count` /
//! `total_cost`) plus the transcript file — see
//! `crates/engine/src/workshop/persistence.rs:155`.
//!
//! `representation.rs::class_is_file_natured` groups
//! `SoulScript | WorkshopConversation` together: both are invisible,
//! file-natured nodes whose essential content is text. So this spawner
//! mirrors the scripting pattern exactly — it carries the transcript body
//! on a [`SoulScriptData`] component (the engine's generic script-source
//! carrier) and adds nothing visual.
//!
//! ## Boundary
//!
//! This is a **stub** (per spec §8.12): it builds the entity + stores the
//! transcript so the Explorer shows the conversation node and the Workshop
//! panel can open it. The live Workshop session lifecycle (LLM calls,
//! cost accounting, streaming) is owned entirely by the existing Workshop
//! systems — this spawner never drives a conversation.
//!
//! Bundle attached: [`Transform`] + [`Visibility`] + [`Instance`] +
//! [`SoulScriptData`] (transcript as `source`) + [`Name`].

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue};

use crate::soul::{SoulBuildStatus, SoulRunContext, SoulScriptData};

use super::soul_script::SOURCE_KEY;

/// Zero-sized spawner for [`ClassName::WorkshopConversation`]. Stateless.
#[derive(Default)]
pub struct WorkshopConversationSpawner;

impl ClassSpawner for WorkshopConversationSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::WorkshopConversation
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("WorkshopConversation")
            .to_string();

        // The transcript body is the "source". Empty when the bag omits it
        // (a freshly-created, still-empty session).
        let source = props.get_string(SOURCE_KEY).unwrap_or("").to_string();

        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::WorkshopConversation,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                SoulScriptData {
                    source,
                    dirty: false,
                    ast: None,
                    generated_code: None,
                    build_status: SoulBuildStatus::NotBuilt,
                    errors: Vec::new(),
                    // Workshop transcripts are not compiled to either VM;
                    // Rune is the inert default. The Workshop panel reads
                    // the source directly, never the build pipeline.
                    run_context: SoulRunContext::Rune,
                },
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        // Stub persistence — the transcript + session metadata round-trip
        // via the Workshop folder's `_instance.toml` + transcript file.
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // Conversations are not edited via the Properties panel — they grow
        // through the Workshop session lifecycle and persist to disk. We
        // honour only the cheap name mirror + an in-memory transcript
        // refresh when the bag carries one. Never request a respawn.
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(new_name) = props.get_string("metadata.name") {
                if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                    instance.name = new_name.to_string();
                }
                if let Some(mut name) = entity_mut.get_mut::<Name>() {
                    name.set(new_name.to_string());
                }
            }
            if let Some(archivable) = props.get_bool("metadata.archivable") {
                if let Some(mut instance) = entity_mut.get_mut::<Instance>() {
                    instance.archivable = archivable;
                }
            }
            if let Some(new_source) = props.get_string(SOURCE_KEY) {
                if let Some(mut data) = entity_mut.get_mut::<SoulScriptData>() {
                    data.source = new_source.to_string();
                }
            }
        }

        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // Invisible — conversations have no LOD model. Empty for all tiers.
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, _rbx: &dyn RobloxInstance) -> PropertyBag {
        // No Roblox cognate — Workshop conversations are an Eustress-only
        // concept. Per spec §2.1 return an empty bag; the importer logs a
        // warn line if it ever reaches here.
        PropertyBag::new()
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        // Mirror the Workshop `_instance.toml` shape written by
        // `workshop/persistence.rs:190`:
        //
        //     [metadata]
        //     class_name = "WorkshopConversation"
        //     name = "..."
        //     archivable = true
        //
        //     [properties]
        //     session_id = "..."
        //     product_name = "..."
        //     message_count = 12
        //     total_cost = 0.0431
        //
        // We surface the metadata + session properties + an optional
        // transcript `source` so a reload reconstructs the node. The
        // Workshop panel re-hydrates the live session from `session_id`.
        let mut bag = PropertyBag::with_capacity(7);

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
        if let Some(props_tbl) = toml_value.get("properties") {
            if let Some(sid) = props_tbl.get("session_id").and_then(|v| v.as_str()) {
                bag.set(
                    "conversation.session_id",
                    PropertyValue::String(sid.to_string()),
                );
            }
            if let Some(pname) = props_tbl.get("product_name").and_then(|v| v.as_str()) {
                bag.set(
                    "conversation.product_name",
                    PropertyValue::String(pname.to_string()),
                );
            }
            // Transcript body may be inlined under `properties.source` for a
            // Fjall-authoritative reload; the disk transcript file remains
            // the canonical editable copy.
            if let Some(source) = props_tbl.get("source").and_then(|v| v.as_str()) {
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
            toml::Value::String("WorkshopConversation".to_string()),
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

        // Stub export captures the in-memory transcript under
        // `[properties]`. The full session-metadata round-trip (session_id,
        // cost, message_count) stays owned by `workshop/persistence.rs`,
        // which is the authority on Workshop folder layout — this spawner
        // does not duplicate that write path.
        if let Some(data) = world.entity(entity).get::<SoulScriptData>() {
            let mut props_tbl = toml::value::Table::new();
            props_tbl.insert(
                "source".to_string(),
                toml::Value::String(data.source.clone()),
            );
            root.insert("properties".to_string(), toml::Value::Table(props_tbl));
        }

        toml::Value::Table(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_name_is_workshop_conversation() {
        assert_eq!(
            WorkshopConversationSpawner.class_name(),
            ClassName::WorkshopConversation
        );
    }

    #[test]
    fn workshop_conversation_spawner_is_object_safe() {
        let boxed: Box<dyn ClassSpawner> = Box::new(WorkshopConversationSpawner);
        assert_eq!(boxed.class_name(), ClassName::WorkshopConversation);
    }

    #[test]
    fn lod_bundle_is_empty_at_every_tier() {
        let spawner = WorkshopConversationSpawner;
        for tier in [
            LodTier::Hero,
            LodTier::Active,
            LodTier::Streamed,
            LodTier::Horizon,
        ] {
            assert!(spawner.lod_components(tier).is_empty());
        }
    }

    /// Reads the Workshop `_instance.toml` metadata + session properties.
    #[test]
    fn import_from_toml_reads_metadata_and_session() {
        let toml_src = r#"
            [metadata]
            class_name = "WorkshopConversation"
            name = "VSupreme Cooling Design"
            archivable = true

            [properties]
            session_id = "sess-abc123"
            product_name = "V-Supreme"
            message_count = 12
            total_cost = 0.0431
            source = "User: ...\nJARVIS: ..."
        "#;
        let value: toml::Value = toml::from_str(toml_src).unwrap();
        let bag = WorkshopConversationSpawner.import_from_toml(&value);
        assert_eq!(
            bag.get_string("metadata.name"),
            Some("VSupreme Cooling Design")
        );
        assert_eq!(
            bag.get_string("conversation.session_id"),
            Some("sess-abc123")
        );
        assert_eq!(
            bag.get_string("conversation.product_name"),
            Some("V-Supreme")
        );
        assert_eq!(
            bag.get_string(SOURCE_KEY),
            Some("User: ...\nJARVIS: ...")
        );
    }

    /// No Roblox cognate → empty bag.
    #[test]
    fn import_from_roblox_is_empty() {
        struct Mock;
        impl RobloxInstance for Mock {
            fn class_name(&self) -> &str {
                "Folder"
            }
            fn name(&self) -> &str {
                "x"
            }
            fn property(
                &self,
                _key: &str,
            ) -> Option<eustress_common::class_registry::RobloxPropertyValue> {
                None
            }
            fn children(&self) -> Vec<&dyn RobloxInstance> {
                Vec::new()
            }
            fn referent(&self) -> u64 {
                0
            }
        }
        assert!(WorkshopConversationSpawner.import_from_roblox(&Mock).is_empty());
    }

    #[test]
    fn deserialize_empty_returns_empty_bag() {
        assert!(WorkshopConversationSpawner.deserialize(&[]).is_empty());
    }
}
