//! `ViewportFrameSpawner` — Wave 3.C GUI leaf.
//!
//! Renders a 3D sub-scene into a 2D GUI surface. The actual render-target
//! camera lives in the existing viewport plugin; this spawner attaches
//! the data component. See CLASS_REGISTRY.md §8.6.

use bevy::prelude::*;
use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, ViewportFrame};

#[derive(Default)]
pub struct ViewportFrameSpawner;

impl ClassSpawner for ViewportFrameSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::ViewportFrame
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("ViewportFrame")
            .to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::ViewportFrame,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                ViewportFrame::default(),
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> { Vec::new() }
    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag { PropertyBag::new() }

    fn apply_edit(&self, _world: &mut World, _entity: Entity, _props: &PropertyBag) -> bool {
        false
    }

    fn lod_components(&self, tier: LodTier) -> ComponentBundle {
        match tier {
            // ViewportFrame is expensive (mini render target) — at Hero
            // only. Active/Streamed/Horizon: hide (existing viewport
            // plugin handles the actual visibility toggle).
            LodTier::Hero | LodTier::Active => ComponentBundle::empty(),
            LodTier::Streamed | LodTier::Horizon => ComponentBundle::empty(),
        }
    }

    fn import_from_roblox(&self, _rbx: &dyn RobloxInstance) -> PropertyBag { PropertyBag::new() }
    fn import_from_toml(&self, _toml_value: &toml::Value) -> PropertyBag { PropertyBag::new() }
    fn export_to_toml(&self, _world: &World, _entity: Entity) -> toml::Value {
        toml::Value::Table(toml::value::Table::new())
    }
}
