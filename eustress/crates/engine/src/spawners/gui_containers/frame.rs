//! `Frame` spawner — basic UI container element.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.5. Mirrors the two
//! existing spawn paths so the cutover stays byte-equivalent:
//! - `engine::spawn::spawn_frame` — Insert-menu / scene-restore path
//!   that emits a real Bevy UI `Node` so screen-space Frames work
//!   when parented to a ScreenGui.
//! - `engine::space::gui_loader::spawn_frame_element` — cold-load path
//!   that emits a minimal `Node{display:None}` plus a `GuiElementDisplay`
//!   carrying every visual property the billboard / surface renderers
//!   need. This is the path the file_loader uses when scanning
//!   StarterGui directories.
//!
//! Wave 3 chooses the gui_loader-style component set — the file_loader
//! is the primary spawn path during cutover, and the billboard /
//! surface-GUI renderers walk `GuiElementDisplay` directly without
//! caring about the Bevy `Node`. Screen-space Frames lose the live
//! `Node` until the runtime UI plugin's `sync_screen_gui_layout`
//! attaches one — same behaviour the existing gui_loader path has had
//! since the GUI-renderer refactor.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Frame, Instance, PropertyValue};
use eustress_common::gui::billboard_renderer::GuiElementDisplay;

/// Spawner for the [`Frame`] class.
#[derive(Default)]
pub struct FrameSpawner;

impl ClassSpawner for FrameSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Frame
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("instance.name")
            .or_else(|| props.get_string("name"))
            .unwrap_or("Frame")
            .to_string();
        let archivable = props
            .get_bool("metadata.archivable")
            .or_else(|| props.get_bool("archivable"))
            .unwrap_or(true);
        let uuid = props.get_uuid().unwrap_or("").to_string();

        let instance = Instance {
            name: name.clone(),
            class_name: ClassName::Frame,
            archivable,
            id: 0,
            uuid,
            ai: false,
        };

        // Hydrate the class component — start from defaults so partial
        // bags still produce a coherent Frame.
        let mut frame = Frame::default();
        if let Some(v) = props.get_bool("gui.visible") { frame.visible = v; }
        if let Some(c) = props.get_color3("gui.background_color3") { frame.background_color3 = c; }
        if let Some(v) = props.get_f32("gui.background_transparency") { frame.background_transparency = v; }
        if let Some(c) = props.get_color3("gui.border_color3") { frame.border_color3 = c; }
        if let Some(v) = props.get_i32("gui.border_size_pixel") { frame.border_size_pixel = v; }
        if let Some(v) = props.get_bool("gui.clips_descendants") { frame.clips_descendants = v; }
        if let Some(v) = props.get_i32("gui.z_index") { frame.z_index = v; }
        if let Some(v) = props.get_i32("gui.layout_order") { frame.layout_order = v; }
        if let Some(v) = props.get_f32("gui.rotation") { frame.rotation = v; }
        if let Some(v) = props.get_vec2("gui.anchor_point") { frame.anchor_point = v; }

        // GuiElementDisplay — the renderer-facing snapshot. Built from
        // the class component's UDim2 fields so the billboard / surface
        // GUI renderers can collect the subtree without going back to
        // the typed component.
        let display = make_display(&frame);

        let entity = ctx.commands.spawn((
            instance,
            frame,
            Name::new(name.clone()),
            // No bevy_ui Node — matches gui_loader::spawn_frame_element:
            // this Frame is rendered by the billboard / surface GUI / Slint
            // overlay renderer. The runtime UI plugin attaches a real Node
            // when this Frame is parented under a ScreenGui AND the
            // development UI is visible (a Display::None Node still pays
            // full ui_layout_system cost in bevy_ui 0.18).
            display,
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
            if let Some(mut frame) = entity_mut.get_mut::<Frame>() {
                if let Some(v) = props.get_bool("gui.visible") { frame.visible = v; }
                if let Some(c) = props.get_color3("gui.background_color3") { frame.background_color3 = c; }
                if let Some(v) = props.get_f32("gui.background_transparency") { frame.background_transparency = v; }
                if let Some(c) = props.get_color3("gui.border_color3") { frame.border_color3 = c; }
                if let Some(v) = props.get_i32("gui.border_size_pixel") { frame.border_size_pixel = v; }
                if let Some(v) = props.get_bool("gui.clips_descendants") { frame.clips_descendants = v; }
                if let Some(v) = props.get_i32("gui.z_index") { frame.z_index = v; }
                if let Some(v) = props.get_i32("gui.layout_order") { frame.layout_order = v; }
                if let Some(v) = props.get_f32("gui.rotation") { frame.rotation = v; }
                if let Some(v) = props.get_vec2("gui.anchor_point") { frame.anchor_point = v; }
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
            if let Some(v) = gui.get("visible").and_then(|v| v.as_bool()) {
                bag.set("gui.visible", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("background_color3").and_then(|v| v.as_array()) {
                if v.len() == 3 {
                    let r = v[0].as_float().unwrap_or(0.0) as f32;
                    let g = v[1].as_float().unwrap_or(0.0) as f32;
                    let b = v[2].as_float().unwrap_or(0.0) as f32;
                    bag.set("gui.background_color3", PropertyValue::Color3([r, g, b]));
                }
            }
            if let Some(v) = gui.get("background_transparency").and_then(|v| v.as_float()) {
                bag.set("gui.background_transparency", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("border_color3").and_then(|v| v.as_array()) {
                if v.len() == 3 {
                    let r = v[0].as_float().unwrap_or(0.0) as f32;
                    let g = v[1].as_float().unwrap_or(0.0) as f32;
                    let b = v[2].as_float().unwrap_or(0.0) as f32;
                    bag.set("gui.border_color3", PropertyValue::Color3([r, g, b]));
                }
            }
            if let Some(v) = gui.get("border_size_pixel").and_then(|v| v.as_integer()) {
                bag.set("gui.border_size_pixel", PropertyValue::Int(v as i32));
            }
            if let Some(v) = gui.get("clips_descendants").and_then(|v| v.as_bool()) {
                bag.set("gui.clips_descendants", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("z_index").and_then(|v| v.as_integer()) {
                bag.set("gui.z_index", PropertyValue::Int(v as i32));
            }
            if let Some(v) = gui.get("layout_order").and_then(|v| v.as_integer()) {
                bag.set("gui.layout_order", PropertyValue::Int(v as i32));
            }
            if let Some(v) = gui.get("rotation").and_then(|v| v.as_float()) {
                bag.set("gui.rotation", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("anchor_point").and_then(|v| v.as_array()) {
                if v.len() == 2 {
                    let x = v[0].as_float().unwrap_or(0.0) as f32;
                    let y = v[1].as_float().unwrap_or(0.0) as f32;
                    bag.set("gui.anchor_point", PropertyValue::Vector2([x, y]));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let bag = export_bag(world, entity);
        emit_toml(&bag)
    }
}

const SCHEMA_TAG: u8 = 0xC4;

/// Build a [`GuiElementDisplay`] from a [`Frame`] — mirrors the subset
/// `gui_loader::gui_display_from_props` populates for non-text frames.
fn make_display(frame: &Frame) -> GuiElementDisplay {
    GuiElementDisplay {
        x: frame.position.x.offset,
        y: frame.position.y.offset,
        width: frame.size.x.offset.max(1.0),
        height: frame.size.y.offset.max(1.0),
        position_udim2: [
            frame.position.x.scale, frame.position.x.offset,
            frame.position.y.scale, frame.position.y.offset,
        ],
        size_udim2: [
            frame.size.x.scale, frame.size.x.offset,
            frame.size.y.scale, frame.size.y.offset,
        ],
        anchor_point: frame.anchor_point,
        z_order: frame.z_index,
        visible: frame.visible,
        clip_children: frame.clips_descendants,
        scroll_x: 0.0,
        scroll_y: 0.0,
        bg_color: [
            frame.background_color3[0],
            frame.background_color3[1],
            frame.background_color3[2],
            1.0 - frame.background_transparency,
        ],
        border_size: frame.border_size_pixel as f32,
        border_color: [
            frame.border_color3[0],
            frame.border_color3[1],
            frame.border_color3[2],
            1.0,
        ],
        corner_radius: 0.0,
        text: String::new(),
        text_color: [1.0, 1.0, 1.0, 1.0],
        font_size: 14.0,
        font_weight: 400,
        text_align: "Center".to_string(),
        text_y_align: "Center".to_string(),
        text_stroke_color: [0.0, 0.0, 0.0, 0.0],
        text_scaled: false,
        image_path: String::new(),
        class_type: "Frame".to_string(),
        mouse_filter: "stop".to_string(),
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
    if let Some(frame) = world.get::<Frame>(entity) {
        bag.set("gui.visible", PropertyValue::Bool(frame.visible));
        bag.set("gui.background_color3", PropertyValue::Color3(frame.background_color3));
        bag.set("gui.background_transparency", PropertyValue::Float(frame.background_transparency));
        bag.set("gui.border_color3", PropertyValue::Color3(frame.border_color3));
        bag.set("gui.border_size_pixel", PropertyValue::Int(frame.border_size_pixel));
        bag.set("gui.clips_descendants", PropertyValue::Bool(frame.clips_descendants));
        bag.set("gui.z_index", PropertyValue::Int(frame.z_index));
        bag.set("gui.layout_order", PropertyValue::Int(frame.layout_order));
        bag.set("gui.rotation", PropertyValue::Float(frame.rotation));
        bag.set("gui.anchor_point", PropertyValue::Vector2(frame.anchor_point));
    }
    bag
}

/// Shared TOML emitter — both [`FrameSpawner::export_to_toml`] and
/// [`super::scrolling_frame::ScrollingFrameSpawner::export_to_toml`]
/// emit the same `[instance] / [metadata] / [gui]` sections, so the
/// formatting is centralised here to avoid drift.
pub(super) fn emit_toml(bag: &PropertyBag) -> toml::Value {
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
            PropertyValue::String(s) | PropertyValue::Enum(s) => {
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
                target.insert(field.to_string(), toml::Value::Array(vec![
                    toml::Value::Float(v[0] as f64),
                    toml::Value::Float(v[1] as f64),
                ]));
            }
            PropertyValue::Color3(c) => {
                target.insert(field.to_string(), toml::Value::Array(vec![
                    toml::Value::Float(c[0] as f64),
                    toml::Value::Float(c[1] as f64),
                    toml::Value::Float(c[2] as f64),
                ]));
            }
            _ => {}
        }
    }

    if !instance_table.is_empty() { root.insert("instance".to_string(), toml::Value::Table(instance_table)); }
    if !metadata_table.is_empty() { root.insert("metadata".to_string(), toml::Value::Table(metadata_table)); }
    if !gui_table.is_empty() { root.insert("gui".to_string(), toml::Value::Table(gui_table)); }
    toml::Value::Table(root)
}
