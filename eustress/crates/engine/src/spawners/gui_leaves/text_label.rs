//! `TextLabelSpawner` — Wave 3.C GUI leaf.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.6: the canonical
//! text-display GUI leaf. Renders inside its parent's GUI container
//! (ScreenGui / BillboardGui / SurfaceGui) via the existing
//! `gui_loader::spawn_text_label_element` render path; this spawner
//! attaches the data components the renderer reads.

use bevy::prelude::*;
use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, TextLabel};

#[derive(Default)]
pub struct TextLabelSpawner;

impl ClassSpawner for TextLabelSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::TextLabel
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("TextLabel")
            .to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::TextLabel,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                TextLabel::default(),
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> {
        // Wave 4: TextLabel cold-store rkyv layout. Today, state is fully
        // derivable from the source `_instance.toml`; empty bytes mark
        // "no cold mirror yet" per CLASS_REGISTRY.md §10 R9.
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        PropertyBag::new()
    }

    fn apply_edit(&self, _world: &mut World, _entity: Entity, _props: &PropertyBag) -> bool {
        // Property changes propagate through the existing GUI sync
        // systems (`Changed<TextLabel>` watchers). No respawn needed.
        false
    }

    fn lod_components(&self, tier: LodTier) -> ComponentBundle {
        match tier {
            // Hero + Active rely on the existing GUI render pipeline's
            // own visibility logic; spawner returns no extra components.
            LodTier::Hero | LodTier::Active => ComponentBundle::empty(),
            // Streamed + Horizon: hide; GUI text is unreadable past 1km
            // and contributes nothing to the panorama (§9 GUI policy).
            LodTier::Streamed | LodTier::Horizon => ComponentBundle::empty(),
        }
    }

    fn import_from_roblox(&self, _rbx: &dyn RobloxInstance) -> PropertyBag {
        // Wave 4 importer fills this in — Roblox TextLabel maps directly
        // (see ROBLOX_IMPORT_SPEC.md §9 — TextLabel row is Direct).
        PropertyBag::new()
    }

    fn import_from_toml(&self, _toml_value: &toml::Value) -> PropertyBag {
        // Wave 4 wires TOML → PropertyBag through
        // `gui_loader::spawn_text_label_element`'s existing TOML reader.
        PropertyBag::new()
    }

    fn export_to_toml(&self, _world: &World, _entity: Entity) -> toml::Value {
        // Wave 4 wires the export path through the existing
        // `write_instance_changes_system`.
        toml::Value::Table(toml::value::Table::new())
    }
}
