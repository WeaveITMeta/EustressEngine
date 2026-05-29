//! `SurfaceGui` spawner — UI rendered onto a face of a 3D part.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.5. Mirrors
//! `engine::spawn::spawn_surface_gui` (Insert menu) so the cutover stays
//! byte-equivalent.
//!
//! The spawner attaches:
//! - [`Transform`] + [`Visibility`] — face-aligned quad orientation is
//!   computed at render-time by the surface-GUI render system from
//!   `face: NormalId`; the spawner just provides the entity transform.
//! - [`Instance`] (with `class_name = SurfaceGui`)
//! - [`Name`]
//! - [`eustress_common::classes::SurfaceGui`] (the class component)
//! - [`crate::spawn::SurfaceGuiMarker`] — the engine-side marker the
//!   surface render system queries on (an empty marker struct; mirrors
//!   the existing spawn_surface_gui path exactly).
//! - [`crate::space::instance_loader::InstanceFile`] when `ctx.source_path`
//!   is `Some`.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{
    ClassName, HorizontalAlignment, Instance, NormalId, PropertyValue, SurfaceGui, VerticalAlignment,
};

/// Spawner for the [`SurfaceGui`] class.
#[derive(Default)]
pub struct SurfaceGuiSpawner;

impl ClassSpawner for SurfaceGuiSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::SurfaceGui
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("instance.name")
            .or_else(|| props.get_string("name"))
            .unwrap_or("SurfaceGui")
            .to_string();
        let archivable = props
            .get_bool("metadata.archivable")
            .or_else(|| props.get_bool("archivable"))
            .unwrap_or(true);
        let uuid = props.get_uuid().unwrap_or("").to_string();

        let instance = Instance {
            name: name.clone(),
            class_name: ClassName::SurfaceGui,
            archivable,
            id: 0,
            uuid,
            ai: false,
        };

        let mut gui = SurfaceGui::default();
        if let Some(v) = props.get_bool("gui.enabled") { gui.enabled = v; }
        if let Some(v) = props.get_bool("gui.active") { gui.active = v; }
        if let Some(v) = props.get_bool("gui.always_on_top") { gui.always_on_top = v; }
        if let Some(v) = props.get_bool("gui.clips_descendants") { gui.clips_descendants = v; }
        if let Some(v) = props.get_f32("gui.brightness") { gui.brightness = v; }
        if let Some(v) = props.get_f32("gui.light_influence") { gui.light_influence = v; }
        if let Some(v) = props.get_f32("gui.pixels_per_unit") { gui.pixels_per_unit = v; }
        if let Some(v) = props.get_f32("gui.max_distance") { gui.max_distance = v; }
        if let Some(v) = props.get_vec2("gui.canvas_size") { gui.canvas_size = v; }
        if let Some(face) = props.get_enum("gui.face") {
            gui.face = normal_id_from_str(face);
        }
        if let Some(align) = props.get_enum("gui.horizontal_alignment") {
            gui.horizontal_alignment = horizontal_from_str(align);
        }
        if let Some(align) = props.get_enum("gui.vertical_alignment") {
            gui.vertical_alignment = vertical_from_str(align);
        }

        let entity = ctx.commands.spawn((
            Transform::default(),
            Visibility::default(),
            instance,
            gui,
            Name::new(name.clone()),
            // Marker for the surface render system — mirrors the
            // existing engine::spawn::spawn_surface_gui path. The
            // engine-side SurfaceGuiMarker is intentionally a unit
            // struct; the SurfaceGui class component carries all the
            // configurable data (face / canvas_size / brightness / …).
            crate::spawn::SurfaceGuiMarker,
        )).id();

        if let Some(path) = ctx.source_path.as_ref() {
            ctx.commands.entity(entity).insert(crate::space::instance_loader::InstanceFile {
                toml_path: path.clone(),
                mesh_path: std::path::PathBuf::new(),
                name: name.clone(),
            });
        }

        if let Some(parent) = ctx.parent_entity {
            ctx.commands.entity(entity).insert(ChildOf(parent));
        }
        entity
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let bag = export_bag(world, entity);
        let mut buf = vec![SCHEMA_TAG];
        if let Ok(encoded) = bincode::serialize(&bag) {
            buf.extend(encoded);
        }
        buf
    }

    fn deserialize(&self, bytes: &[u8]) -> PropertyBag {
        let Some((&tag, payload)) = bytes.split_first() else {
            return PropertyBag::new();
        };
        if tag != SCHEMA_TAG {
            return PropertyBag::new();
        }
        bincode::deserialize::<PropertyBag>(payload).unwrap_or_default()
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if let Some(mut gui) = entity_mut.get_mut::<SurfaceGui>() {
                if let Some(v) = props.get_bool("gui.enabled") { gui.enabled = v; }
                if let Some(v) = props.get_bool("gui.active") { gui.active = v; }
                if let Some(v) = props.get_bool("gui.always_on_top") { gui.always_on_top = v; }
                if let Some(v) = props.get_bool("gui.clips_descendants") { gui.clips_descendants = v; }
                if let Some(v) = props.get_f32("gui.brightness") { gui.brightness = v; }
                if let Some(v) = props.get_f32("gui.light_influence") { gui.light_influence = v; }
                if let Some(v) = props.get_f32("gui.pixels_per_unit") { gui.pixels_per_unit = v; }
                if let Some(v) = props.get_f32("gui.max_distance") { gui.max_distance = v; }
                if let Some(v) = props.get_vec2("gui.canvas_size") { gui.canvas_size = v; }
                if let Some(face) = props.get_enum("gui.face") {
                    gui.face = normal_id_from_str(face);
                }
                if let Some(align) = props.get_enum("gui.horizontal_alignment") {
                    gui.horizontal_alignment = horizontal_from_str(align);
                }
                if let Some(align) = props.get_enum("gui.vertical_alignment") {
                    gui.vertical_alignment = vertical_from_str(align);
                }
            }
        }
        false
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, _rbx_instance: &dyn RobloxInstance) -> PropertyBag {
        PropertyBag::new()
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::new();
        let Some(table) = toml_value.as_table() else { return bag; };

        if let Some(inst) = table.get("instance").and_then(|v| v.as_table()) {
            if let Some(name) = inst.get("name").and_then(|v| v.as_str()) {
                bag.set("instance.name", PropertyValue::String(name.to_string()));
            }
        }
        if let Some(meta) = table.get("metadata").and_then(|v| v.as_table()) {
            if let Some(b) = meta.get("archivable").and_then(|v| v.as_bool()) {
                bag.set("metadata.archivable", PropertyValue::Bool(b));
            }
            if let Some(uuid) = meta.get("uuid").and_then(|v| v.as_str()) {
                bag.set("metadata.uuid", PropertyValue::String(uuid.to_string()));
            }
        }
        if let Some(gui) = table.get("gui").and_then(|v| v.as_table()) {
            if let Some(v) = gui.get("enabled").and_then(|v| v.as_bool()) {
                bag.set("gui.enabled", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("active").and_then(|v| v.as_bool()) {
                bag.set("gui.active", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("always_on_top").and_then(|v| v.as_bool()) {
                bag.set("gui.always_on_top", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("clips_descendants").and_then(|v| v.as_bool()) {
                bag.set("gui.clips_descendants", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("brightness").and_then(|v| v.as_float()) {
                bag.set("gui.brightness", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("light_influence").and_then(|v| v.as_float()) {
                bag.set("gui.light_influence", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("pixels_per_unit").and_then(|v| v.as_float()) {
                bag.set("gui.pixels_per_unit", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("max_distance").and_then(|v| v.as_float()) {
                bag.set("gui.max_distance", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("canvas_size").and_then(|v| v.as_array()) {
                if v.len() == 2 {
                    let w = v[0].as_float().unwrap_or(0.0) as f32;
                    let h = v[1].as_float().unwrap_or(0.0) as f32;
                    bag.set("gui.canvas_size", PropertyValue::Vector2([w, h]));
                }
            }
            if let Some(v) = gui.get("face").and_then(|v| v.as_str()) {
                bag.set("gui.face", PropertyValue::Enum(v.to_string()));
            }
            if let Some(v) = gui.get("horizontal_alignment").and_then(|v| v.as_str()) {
                bag.set("gui.horizontal_alignment", PropertyValue::Enum(v.to_string()));
            }
            if let Some(v) = gui.get("vertical_alignment").and_then(|v| v.as_str()) {
                bag.set("gui.vertical_alignment", PropertyValue::Enum(v.to_string()));
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
            let Some((section, field)) = key.split_once('.') else { continue; };
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
                PropertyValue::Enum(s) => {
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
                PropertyValue::Vector2(v) => {
                    let arr = toml::Value::Array(vec![
                        toml::Value::Float(v[0] as f64),
                        toml::Value::Float(v[1] as f64),
                    ]);
                    target.insert(field.to_string(), arr);
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

const SCHEMA_TAG: u8 = 0xC3;

fn normal_id_from_str(s: &str) -> NormalId {
    match s {
        "Back" => NormalId::Back,
        "Top" => NormalId::Top,
        "Bottom" => NormalId::Bottom,
        "Left" => NormalId::Left,
        "Right" => NormalId::Right,
        _ => NormalId::Front,
    }
}

fn horizontal_from_str(s: &str) -> HorizontalAlignment {
    match s {
        "Left" => HorizontalAlignment::Left,
        "Right" => HorizontalAlignment::Right,
        _ => HorizontalAlignment::Center,
    }
}

fn vertical_from_str(s: &str) -> VerticalAlignment {
    match s {
        "Top" => VerticalAlignment::Top,
        "Bottom" => VerticalAlignment::Bottom,
        _ => VerticalAlignment::Center,
    }
}

fn horizontal_to_str(a: HorizontalAlignment) -> &'static str {
    match a {
        HorizontalAlignment::Left => "Left",
        HorizontalAlignment::Center => "Center",
        HorizontalAlignment::Right => "Right",
    }
}

fn vertical_to_str(a: VerticalAlignment) -> &'static str {
    match a {
        VerticalAlignment::Top => "Top",
        VerticalAlignment::Center => "Center",
        VerticalAlignment::Bottom => "Bottom",
    }
}

fn export_bag(world: &World, entity: Entity) -> PropertyBag {
    let mut bag = PropertyBag::new();
    if let Some(instance) = world.get::<Instance>(entity) {
        bag.set("instance.name", PropertyValue::String(instance.name.clone()));
        bag.set("metadata.archivable", PropertyValue::Bool(instance.archivable));
        if !instance.uuid.is_empty() {
            bag.set("metadata.uuid", PropertyValue::String(instance.uuid.clone()));
        }
    }
    if let Some(gui) = world.get::<SurfaceGui>(entity) {
        bag.set("gui.enabled", PropertyValue::Bool(gui.enabled));
        bag.set("gui.active", PropertyValue::Bool(gui.active));
        bag.set("gui.always_on_top", PropertyValue::Bool(gui.always_on_top));
        bag.set("gui.clips_descendants", PropertyValue::Bool(gui.clips_descendants));
        bag.set("gui.brightness", PropertyValue::Float(gui.brightness));
        bag.set("gui.light_influence", PropertyValue::Float(gui.light_influence));
        bag.set("gui.pixels_per_unit", PropertyValue::Float(gui.pixels_per_unit));
        bag.set("gui.max_distance", PropertyValue::Float(gui.max_distance));
        bag.set("gui.canvas_size", PropertyValue::Vector2(gui.canvas_size));
        bag.set("gui.face", PropertyValue::Enum(gui.face.as_str().to_string()));
        bag.set(
            "gui.horizontal_alignment",
            PropertyValue::Enum(horizontal_to_str(gui.horizontal_alignment).to_string()),
        );
        bag.set(
            "gui.vertical_alignment",
            PropertyValue::Enum(vertical_to_str(gui.vertical_alignment).to_string()),
        );
    }
    bag
}
