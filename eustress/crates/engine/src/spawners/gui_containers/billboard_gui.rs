//! `BillboardGui` spawner — 3D world-space UI surface that faces the
//! active camera.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.5. Mirrors
//! `engine::spawn::spawn_billboard_gui` (Insert menu) +
//! `engine::space::gui_loader::billboard_class_from_props` (cold-load
//! mapping) so the registry cutover stays byte-equivalent.
//!
//! The spawner attaches:
//! - [`Transform`] + [`Visibility`] — `Transform.translation` carries
//!   the `units_offset` from the bag (same as the current Insert path).
//! - [`Instance`] (with `class_name = BillboardGui`)
//! - [`Name`]
//! - [`eustress_common::classes::BillboardGui`] (the class component)
//! - [`eustress_common::gui::billboard_renderer::BillboardGuiMarker`] —
//!   the engine's `sync_billboard_properties` system watches
//!   `Changed<BillboardGuiMarker>` and pushes size/visibility/depth
//!   updates each frame, so Properties-panel edits remain live.
//! - [`crate::spawn::BillboardAdornee`] — tracks the adornee entity for
//!   the billboard's position follower.
//! - [`crate::space::instance_loader::InstanceFile`] when
//!   `ctx.source_path` is `Some`.
//!
//! Child entities (TextLabel, ImageLabel, etc.) are spawned by their
//! own spawners (Wave 3.C / 3.D — GUI leaves). This spawner deliberately
//! does NOT recurse into a child list; the file_loader walks the
//! directory tree and dispatches one spawner per directory entry.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{BillboardGui, ClassName, Instance, PropertyValue};
use eustress_common::gui::billboard_renderer::BillboardGuiMarker;

/// Spawner for the [`BillboardGui`] class.
#[derive(Default)]
pub struct BillboardGuiSpawner;

impl ClassSpawner for BillboardGuiSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::BillboardGui
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("instance.name")
            .or_else(|| props.get_string("name"))
            .unwrap_or("BillboardGui")
            .to_string();
        let archivable = props
            .get_bool("metadata.archivable")
            .or_else(|| props.get_bool("archivable"))
            .unwrap_or(true);
        let uuid = props.get_uuid().unwrap_or("").to_string();

        let instance = Instance {
            name: name.clone(),
            class_name: ClassName::BillboardGui,
            archivable,
            id: 0,
            uuid,
            ai: false,
        };

        // Hydrate the class component from the bag — start from defaults
        // so partial bags still produce a coherent BillboardGui.
        let mut gui = BillboardGui::default();
        if let Some(v) = props.get_bool("gui.enabled") { gui.enabled = v; }
        if let Some(v) = props.get_bool("gui.active") { gui.active = v; }
        if let Some(v) = props.get_bool("gui.always_on_top") { gui.always_on_top = v; }
        if let Some(v) = props.get_bool("gui.clips_descendants") { gui.clips_descendants = v; }
        if let Some(v) = props.get_bool("gui.reset_on_spawn") { gui.reset_on_spawn = v; }
        if let Some(v) = props.get_bool("gui.stiffness_by_distance") { gui.stiffness_by_distance = v; }
        if let Some(v) = props.get_bool("gui.face_camera") { gui.face_camera = v; }
        if let Some(v) = props.get_f32("gui.max_distance") { gui.max_distance = v; }
        if let Some(v) = props.get_f32("gui.distance_lower_limit") { gui.distance_lower_limit = v; }
        if let Some(v) = props.get_f32("gui.distance_upper_limit") { gui.distance_upper_limit = v; }
        if let Some(v) = props.get_f32("gui.distance_step") { gui.distance_step = v; }
        if let Some(v) = props.get_f32("gui.brightness") { gui.brightness = v; }
        if let Some(v) = props.get_f32("gui.light_influence") { gui.light_influence = v; }
        if let Some(v) = props.get_vec3("gui.units_offset") {
            gui.units_offset = [v.x, v.y, v.z];
        }
        if let Some(v) = props.get_i32("gui.z_index") { gui.z_index = v; }

        // Mirror spawn::spawn_billboard_gui: position offset from parent
        // comes from `units_offset`.
        let offset = Vec3::new(
            gui.units_offset[0],
            gui.units_offset[1],
            gui.units_offset[2],
        );

        // Mirror the engine's PIXELS_PER_METER (50 px/m) used by the
        // BillboardGuiMarker — same resolution the renderer's atlas uses.
        let marker_size = {
            let [w, h] = gui.size.to_pixels(50.0, 50.0);
            [w.max(1.0), h.max(1.0)]
        };

        let marker = BillboardGuiMarker {
            size: marker_size,
            max_distance: gui.max_distance,
            always_on_top: gui.always_on_top,
            face_camera: gui.face_camera,
            visible: gui.enabled,
            z_index: gui.z_index,
            ..Default::default()
        };

        let adornee = gui.adornee;
        let entity = ctx.commands.spawn((
            Transform::from_translation(offset),
            Visibility::default(),
            instance,
            marker,
            crate::spawn::BillboardAdornee {
                target_name: None,
                target_entity: adornee,
            },
            gui,
            Name::new(name.clone()),
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
            if let Some(mut gui) = entity_mut.get_mut::<BillboardGui>() {
                if let Some(v) = props.get_bool("gui.enabled") { gui.enabled = v; }
                if let Some(v) = props.get_bool("gui.active") { gui.active = v; }
                if let Some(v) = props.get_bool("gui.always_on_top") { gui.always_on_top = v; }
                if let Some(v) = props.get_bool("gui.clips_descendants") { gui.clips_descendants = v; }
                if let Some(v) = props.get_bool("gui.reset_on_spawn") { gui.reset_on_spawn = v; }
                if let Some(v) = props.get_bool("gui.face_camera") { gui.face_camera = v; }
                if let Some(v) = props.get_f32("gui.max_distance") { gui.max_distance = v; }
                if let Some(v) = props.get_f32("gui.distance_lower_limit") { gui.distance_lower_limit = v; }
                if let Some(v) = props.get_f32("gui.distance_upper_limit") { gui.distance_upper_limit = v; }
                if let Some(v) = props.get_f32("gui.distance_step") { gui.distance_step = v; }
                if let Some(v) = props.get_f32("gui.brightness") { gui.brightness = v; }
                if let Some(v) = props.get_f32("gui.light_influence") { gui.light_influence = v; }
                if let Some(v) = props.get_i32("gui.z_index") { gui.z_index = v; }
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
            if let Some(v) = gui.get("reset_on_spawn").and_then(|v| v.as_bool()) {
                bag.set("gui.reset_on_spawn", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("stiffness_by_distance").and_then(|v| v.as_bool()) {
                bag.set("gui.stiffness_by_distance", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("face_camera").and_then(|v| v.as_bool()) {
                bag.set("gui.face_camera", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("max_distance").and_then(|v| v.as_float()) {
                bag.set("gui.max_distance", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("distance_lower_limit").and_then(|v| v.as_float()) {
                bag.set("gui.distance_lower_limit", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("distance_upper_limit").and_then(|v| v.as_float()) {
                bag.set("gui.distance_upper_limit", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("distance_step").and_then(|v| v.as_float()) {
                bag.set("gui.distance_step", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("brightness").and_then(|v| v.as_float()) {
                bag.set("gui.brightness", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("light_influence").and_then(|v| v.as_float()) {
                bag.set("gui.light_influence", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("units_offset").and_then(|v| v.as_array()) {
                if v.len() == 3 {
                    let x = v[0].as_float().unwrap_or(0.0) as f32;
                    let y = v[1].as_float().unwrap_or(0.0) as f32;
                    let z = v[2].as_float().unwrap_or(0.0) as f32;
                    bag.set("gui.units_offset", PropertyValue::Vector3(Vec3::new(x, y, z)));
                }
            }
            if let Some(v) = gui.get("z_index").and_then(|v| v.as_integer()) {
                bag.set("gui.z_index", PropertyValue::Int(v as i32));
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
                PropertyValue::Bool(b) => {
                    target.insert(field.to_string(), toml::Value::Boolean(*b));
                }
                PropertyValue::Int(i) => {
                    target.insert(field.to_string(), toml::Value::Integer(*i as i64));
                }
                PropertyValue::Float(f) => {
                    target.insert(field.to_string(), toml::Value::Float(*f as f64));
                }
                PropertyValue::Vector3(v) => {
                    let arr = toml::Value::Array(vec![
                        toml::Value::Float(v.x as f64),
                        toml::Value::Float(v.y as f64),
                        toml::Value::Float(v.z as f64),
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

const SCHEMA_TAG: u8 = 0xC2;

fn export_bag(world: &World, entity: Entity) -> PropertyBag {
    let mut bag = PropertyBag::new();
    if let Some(instance) = world.get::<Instance>(entity) {
        bag.set("instance.name", PropertyValue::String(instance.name.clone()));
        bag.set("metadata.archivable", PropertyValue::Bool(instance.archivable));
        if !instance.uuid.is_empty() {
            bag.set("metadata.uuid", PropertyValue::String(instance.uuid.clone()));
        }
    }
    if let Some(gui) = world.get::<BillboardGui>(entity) {
        bag.set("gui.enabled", PropertyValue::Bool(gui.enabled));
        bag.set("gui.active", PropertyValue::Bool(gui.active));
        bag.set("gui.always_on_top", PropertyValue::Bool(gui.always_on_top));
        bag.set("gui.clips_descendants", PropertyValue::Bool(gui.clips_descendants));
        bag.set("gui.reset_on_spawn", PropertyValue::Bool(gui.reset_on_spawn));
        bag.set("gui.stiffness_by_distance", PropertyValue::Bool(gui.stiffness_by_distance));
        bag.set("gui.face_camera", PropertyValue::Bool(gui.face_camera));
        bag.set("gui.max_distance", PropertyValue::Float(gui.max_distance));
        bag.set("gui.distance_lower_limit", PropertyValue::Float(gui.distance_lower_limit));
        bag.set("gui.distance_upper_limit", PropertyValue::Float(gui.distance_upper_limit));
        bag.set("gui.distance_step", PropertyValue::Float(gui.distance_step));
        bag.set("gui.brightness", PropertyValue::Float(gui.brightness));
        bag.set("gui.light_influence", PropertyValue::Float(gui.light_influence));
        bag.set(
            "gui.units_offset",
            PropertyValue::Vector3(Vec3::new(
                gui.units_offset[0], gui.units_offset[1], gui.units_offset[2],
            )),
        );
        bag.set("gui.z_index", PropertyValue::Int(gui.z_index));
    }
    bag
}
