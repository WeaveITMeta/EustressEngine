//! `ScreenGui` spawner — fullscreen Bevy UI root container.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.5. Mirrors the existing
//! `engine::space::gui_loader::spawn_screen_gui_element` (cold load) +
//! `engine::spawn::spawn_screen_gui` (Insert menu) paths so the registry
//! cutover (Wave 5) produces byte-identical entities.
//!
//! The spawner attaches:
//! - [`eustress_common::classes::Instance`] (with `class_name = ScreenGui`)
//! - [`bevy::core::Name`]
//! - [`eustress_common::classes::ScreenGui`] (the class component)
//! - A fullscreen [`bevy::ui::Node`] (`100% × 100%`, `PositionType::Absolute`)
//! - [`bevy::ui::BackgroundColor(Color::NONE)`] so the 3D viewport
//!   underneath shows through.
//! - [`bevy::ui::GlobalZIndex(100)`] — above 3D scene, below the Slint
//!   overlay (matches the existing gui_loader value).
//! - [`crate::space::instance_loader::InstanceFile`] when `ctx.source_path`
//!   is present (cold load from `_instance.toml`).
//!
//! Wave 3 scope:
//! - `spawn` is fully wired (mirrors current behavior).
//! - `serialize` / `deserialize` use a bincode round-trip of the
//!   [`PropertyBag`] with a single schema tag byte. Wave 5+ swaps to the
//!   class-specific rkyv mirror struct per spec Appendix A.
//! - `apply_edit` returns `false` — no GUI-container property change
//!   currently requires a respawn (color/transparency/visibility are
//!   live-edited via the component watchers).
//! - `lod_components` returns an empty bundle (GUI containers have no
//!   per-tier LOD components today; see RENDER_CASCADE.md).
//! - `import_from_roblox` returns an empty bag — wired in Wave 4 when
//!   the importer ships.
//! - `import_from_toml` / `export_to_toml` walk the `[gui]` table for
//!   ScreenGui's properties.

use bevy::prelude::*;
use bevy::ui::{self, Val, PositionType};

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue, ScreenGui};

/// Spawner for the [`ScreenGui`] class.
///
/// Stateless — every `ClassSpawner` lives behind `Box<dyn ClassSpawner>`
/// in the registry, so per-spawn mutability flows through `ctx`.
#[derive(Default)]
pub struct ScreenGuiSpawner;

impl ClassSpawner for ScreenGuiSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::ScreenGui
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        // Pull name + archivable from the bag with sensible defaults so
        // a hot-create (Insert menu) without an explicit bag still
        // produces a usable entity.
        let name = props
            .get_string("instance.name")
            .or_else(|| props.get_string("name"))
            .unwrap_or("ScreenGui")
            .to_string();
        let archivable = props
            .get_bool("metadata.archivable")
            .or_else(|| props.get_bool("archivable"))
            .unwrap_or(true);
        let uuid = props.get_uuid().unwrap_or("").to_string();

        let instance = Instance {
            name: name.clone(),
            class_name: ClassName::ScreenGui,
            archivable,
            id: 0,
            uuid,
            ai: false,
        };

        // Hydrate the class component from the bag. Each field falls
        // back to `ScreenGui::default()` so partial bags (e.g. only
        // `enabled` set) still produce a coherent entity.
        let defaults = ScreenGui::default();
        let gui = ScreenGui {
            enabled: props.get_bool("gui.enabled").unwrap_or(defaults.enabled),
            display_order: props
                .get_i32("gui.display_order")
                .unwrap_or(defaults.display_order),
            ignore_gui_inset: props
                .get_bool("gui.ignore_gui_inset")
                .unwrap_or(defaults.ignore_gui_inset),
            reset_on_spawn: props
                .get_bool("gui.reset_on_spawn")
                .unwrap_or(defaults.reset_on_spawn),
            clips_descendants: props
                .get_bool("gui.clips_descendants")
                .unwrap_or(defaults.clips_descendants),
            z_index_behavior: defaults.z_index_behavior,
            screen_insets: defaults.screen_insets,
        };

        let entity = ctx.commands.spawn((
            instance,
            gui,
            Name::new(name.clone()),
            // Fullscreen Bevy UI root — mirrors gui_loader::spawn_screen_gui_element.
            ui::Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                ..default()
            },
            ui::BackgroundColor(Color::NONE),
            // Above 3D scene, below Slint overlay (matches existing
            // value in gui_loader.rs).
            ui::GlobalZIndex(100),
        )).id();

        // Attach InstanceFile if we're cold-loading from disk so the
        // save_*_changes systems can find the source file later.
        if let Some(path) = ctx.source_path.as_ref() {
            ctx.commands.entity(entity).insert(crate::space::instance_loader::InstanceFile {
                toml_path: path.clone(),
                mesh_path: std::path::PathBuf::new(),
                name: name.clone(),
            });
        }

        // Parent if the caller pre-resolved one (file_loader path).
        if let Some(parent) = ctx.parent_entity {
            ctx.commands.entity(entity).insert(ChildOf(parent));
        }
        entity
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        // Wave 3 stub — round-trip the PropertyBag via bincode 1.x +
        // tag byte. Wave 5+ replaces with the class-specific rkyv mirror
        // per spec Appendix A.
        let bag = export_bag(world, entity);
        let mut buf = vec![SCHEMA_TAG];
        if let Ok(encoded) = bincode::serialize(&bag) {
            buf.extend(encoded);
        }
        buf
    }

    fn deserialize(&self, bytes: &[u8]) -> PropertyBag {
        // Reject the wrong tag — spec §2 says a tag mismatch returns an
        // empty bag and the caller logs the migration warning.
        let Some((&tag, payload)) = bytes.split_first() else {
            return PropertyBag::new();
        };
        if tag != SCHEMA_TAG {
            return PropertyBag::new();
        }
        bincode::deserialize::<PropertyBag>(payload).unwrap_or_default()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // GUI-container properties are all cheap mutations (booleans +
        // i32 display order). No respawn required — the Properties
        // panel's per-class writers already handle live updates via
        // Changed<ScreenGui> watchers in the runtime UI plugin.
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(mut gui) = entity_mut.get_mut::<ScreenGui>() {
                if let Some(v) = props.get_bool("gui.enabled") { gui.enabled = v; }
                if let Some(v) = props.get_i32("gui.display_order") { gui.display_order = v; }
                if let Some(v) = props.get_bool("gui.ignore_gui_inset") { gui.ignore_gui_inset = v; }
                if let Some(v) = props.get_bool("gui.reset_on_spawn") { gui.reset_on_spawn = v; }
                if let Some(v) = props.get_bool("gui.clips_descendants") { gui.clips_descendants = v; }
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // GUI containers have no per-tier LOD components today (per
        // RENDER_CASCADE.md — physics LOD is Wave 4, visual LOD only
        // covers parts + lights).
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, _rbx_instance: &dyn RobloxInstance) -> PropertyBag {
        // Wave 4 wires the importer; Wave 3 ships an empty bag so the
        // pipeline compiles and the importer's missing-spawner warn-log
        // path is testable.
        PropertyBag::new()
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::new();
        let Some(table) = toml_value.as_table() else { return bag; };

        // [instance].name + [metadata].archivable + .uuid
        if let Some(inst) = table.get("instance").and_then(|v| v.as_table()) {
            if let Some(name) = inst.get("name").and_then(|v| v.as_str()) {
                bag.set("instance.name", PropertyValue::String(name.to_string()));
            }
        }
        if let Some(meta) = table.get("metadata").and_then(|v| v.as_table()) {
            if let Some(archivable) = meta.get("archivable").and_then(|v| v.as_bool()) {
                bag.set("metadata.archivable", PropertyValue::Bool(archivable));
            }
            if let Some(uuid) = meta.get("uuid").and_then(|v| v.as_str()) {
                bag.set("metadata.uuid", PropertyValue::String(uuid.to_string()));
            }
        }
        // [gui] subset relevant to ScreenGui
        if let Some(gui) = table.get("gui").and_then(|v| v.as_table()) {
            if let Some(v) = gui.get("enabled").and_then(|v| v.as_bool()) {
                bag.set("gui.enabled", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("display_order").and_then(|v| v.as_integer()) {
                bag.set("gui.display_order", PropertyValue::Int(v as i32));
            }
            if let Some(v) = gui.get("ignore_gui_inset").and_then(|v| v.as_bool()) {
                bag.set("gui.ignore_gui_inset", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("reset_on_spawn").and_then(|v| v.as_bool()) {
                bag.set("gui.reset_on_spawn", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("clips_descendants").and_then(|v| v.as_bool()) {
                bag.set("gui.clips_descendants", PropertyValue::Bool(v));
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let bag = export_bag(world, entity);
        let mut root = toml::value::Table::new();

        let mut instance_table = toml::value::Table::new();
        let mut metadata_table = toml::value::Table::new();
        let mut gui_table = toml::value::Table::new();

        for (key, value) in bag.iter() {
            let (section, field) = match key.split_once('.') {
                Some(pair) => pair,
                None => continue,
            };
            let target = match section {
                "instance" => &mut instance_table,
                "metadata" => &mut metadata_table,
                "gui" => &mut gui_table,
                _ => continue,
            };
            match value {
                PropertyValue::String(s) => {
                    target.insert(field.to_string(), toml::Value::String(s.clone()));
                }
                PropertyValue::Bool(b) => {
                    target.insert(field.to_string(), toml::Value::Boolean(*b));
                }
                PropertyValue::Int(i) => {
                    target.insert(field.to_string(), toml::Value::Integer(*i as i64));
                }
                PropertyValue::Float(f) => {
                    target.insert(field.to_string(), toml::Value::Float(*f as f64));
                }
                _ => {}
            }
        }

        if !instance_table.is_empty() { root.insert("instance".to_string(), toml::Value::Table(instance_table)); }
        if !metadata_table.is_empty() { root.insert("metadata".to_string(), toml::Value::Table(metadata_table)); }
        if !gui_table.is_empty() { root.insert("gui".to_string(), toml::Value::Table(gui_table)); }
        toml::Value::Table(root)
    }
}

/// Schema tag byte for `ScreenGuiSpawner::serialize` — Wave 3 stub. Wave 5+
/// replaces this with the rkyv tag from spec Appendix A.
const SCHEMA_TAG: u8 = 0xC1;

/// Read the entity's class-relevant state back into a [`PropertyBag`].
/// Shared by [`ScreenGuiSpawner::serialize`] and
/// [`ScreenGuiSpawner::export_to_toml`] so both round-trip the same
/// canonical key order.
fn export_bag(world: &World, entity: Entity) -> PropertyBag {
    let mut bag = PropertyBag::new();
    if let Some(instance) = world.get::<Instance>(entity) {
        bag.set("instance.name", PropertyValue::String(instance.name.clone()));
        bag.set("metadata.archivable", PropertyValue::Bool(instance.archivable));
        if !instance.uuid.is_empty() {
            bag.set("metadata.uuid", PropertyValue::String(instance.uuid.clone()));
        }
    }
    if let Some(gui) = world.get::<ScreenGui>(entity) {
        bag.set("gui.enabled", PropertyValue::Bool(gui.enabled));
        bag.set("gui.display_order", PropertyValue::Int(gui.display_order));
        bag.set("gui.ignore_gui_inset", PropertyValue::Bool(gui.ignore_gui_inset));
        bag.set("gui.reset_on_spawn", PropertyValue::Bool(gui.reset_on_spawn));
        bag.set("gui.clips_descendants", PropertyValue::Bool(gui.clips_descendants));
    }
    bag
}
