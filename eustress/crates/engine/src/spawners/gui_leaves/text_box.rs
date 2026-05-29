//! `TextBoxSpawner` — Wave 3.C GUI leaf.
//!
//! User-editable text field. See `text_label.rs` for the structural
//! pattern and CLASS_REGISTRY.md §8.6.

use bevy::prelude::*;
use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, TextBox};

#[derive(Default)]
pub struct TextBoxSpawner;

impl ClassSpawner for TextBoxSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::TextBox
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("TextBox")
            .to_string();
        let uuid = props.get_uuid().unwrap_or_default().to_string();
        let archivable = props.get_bool("metadata.archivable").unwrap_or(true);

        ctx.commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Instance {
                    name: name.clone(),
                    class_name: ClassName::TextBox,
                    archivable,
                    id: 0,
                    uuid,
                    ai: false,
                },
                TextBox::default(),
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, _world: &World, _entity: Entity) -> Vec<u8> { Vec::new() }
    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag { PropertyBag::new() }

    fn apply_edit(&self, _world: &mut World, _entity: Entity, _props: &PropertyBag) -> bool {
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, _rbx: &dyn RobloxInstance) -> PropertyBag { PropertyBag::new() }
    fn import_from_toml(&self, _toml_value: &toml::Value) -> PropertyBag { PropertyBag::new() }
    fn export_to_toml(&self, _world: &World, _entity: Entity) -> toml::Value {
        toml::Value::Table(toml::value::Table::new())
    }
}
