//! `ScrollingFrame` spawner — scrollable container element.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.5. Mirrors
//! `engine::spawn::spawn_scrolling_frame` (Insert) +
//! `engine::space::gui_loader::spawn_scrolling_frame_element` (cold
//! load) so the cutover stays byte-equivalent.
//!
//! Same component shape as `FrameSpawner` (Node + GuiElementDisplay) —
//! the visible difference is the `clip_children = true` on the display
//! (which the billboard renderer uses to clip child rendering) and the
//! `class_type = "ScrollingFrame"` discriminator string the renderer
//! reads to switch on scroll-bar behaviour.
//!
//! There is no engine-side `ScrollingFrameMarker` today — the
//! `ScrollingFrame` class component itself carries the scroll-bar
//! state (`canvas_position`, `scroll_bar_thickness`, …). The spawner
//! attaches the typed component; the task spec lists the marker as
//! optional and the current renderer doesn't need it.

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, PropertyValue, ScrollingFrame};
use eustress_common::gui::billboard_renderer::GuiElementDisplay;

/// Spawner for the [`ScrollingFrame`] class.
#[derive(Default)]
pub struct ScrollingFrameSpawner;

impl ClassSpawner for ScrollingFrameSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::ScrollingFrame
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("instance.name")
            .or_else(|| props.get_string("name"))
            .unwrap_or("ScrollingFrame")
            .to_string();
        let archivable = props
            .get_bool("metadata.archivable")
            .or_else(|| props.get_bool("archivable"))
            .unwrap_or(true);
        let uuid = props.get_uuid().unwrap_or("").to_string();

        let instance = Instance {
            name: name.clone(),
            class_name: ClassName::ScrollingFrame,
            archivable,
            id: 0,
            uuid,
            ai: false,
        };

        let mut frame = ScrollingFrame::default();
        if let Some(v) = props.get_bool("gui.visible") { frame.visible = v; }
        if let Some(c) = props.get_color3("gui.background_color3") { frame.background_color3 = c; }
        if let Some(v) = props.get_f32("gui.background_transparency") { frame.background_transparency = v; }
        if let Some(c) = props.get_color3("gui.border_color3") { frame.border_color3 = c; }
        if let Some(v) = props.get_i32("gui.border_size_pixel") { frame.border_size_pixel = v; }
        if let Some(v) = props.get_i32("gui.z_index") { frame.z_index = v; }
        if let Some(v) = props.get_i32("gui.layout_order") { frame.layout_order = v; }
        if let Some(v) = props.get_f32("gui.rotation") { frame.rotation = v; }
        if let Some(v) = props.get_vec2("gui.anchor_point") { frame.anchor_point = v; }
        if let Some(v) = props.get_vec2("gui.canvas_size") { frame.canvas_size = v; }
        if let Some(v) = props.get_vec2("gui.canvas_position") { frame.canvas_position = v; }
        if let Some(v) = props.get_bool("gui.scroll_bar_enabled_x") { frame.scroll_bar_enabled_x = v; }
        if let Some(v) = props.get_bool("gui.scroll_bar_enabled_y") { frame.scroll_bar_enabled_y = v; }
        if let Some(v) = props.get_i32("gui.scroll_bar_thickness") { frame.scroll_bar_thickness = v; }
        if let Some(v) = props.get_bool("gui.scrolling_enabled") { frame.scrolling_enabled = v; }
        if let Some(v) = props.get_f32("gui.scroll_bar_image_transparency") { frame.scroll_bar_image_transparency = v; }
        if let Some(c) = props.get_color3("gui.scroll_bar_image_color3") { frame.scroll_bar_image_color3 = c; }

        let display = make_display(&frame);

        let entity = ctx.commands.spawn((
            instance,
            frame,
            Name::new(name.clone()),
            // No bevy_ui Node — rendered via GuiElementDisplay (PERF: see
            // gui_loader::spawn_frame_element note).
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
            if let Some(mut frame) = entity_mut.get_mut::<ScrollingFrame>() {
                if let Some(v) = props.get_bool("gui.visible") { frame.visible = v; }
                if let Some(c) = props.get_color3("gui.background_color3") { frame.background_color3 = c; }
                if let Some(v) = props.get_f32("gui.background_transparency") { frame.background_transparency = v; }
                if let Some(c) = props.get_color3("gui.border_color3") { frame.border_color3 = c; }
                if let Some(v) = props.get_i32("gui.border_size_pixel") { frame.border_size_pixel = v; }
                if let Some(v) = props.get_i32("gui.z_index") { frame.z_index = v; }
                if let Some(v) = props.get_i32("gui.layout_order") { frame.layout_order = v; }
                if let Some(v) = props.get_f32("gui.rotation") { frame.rotation = v; }
                if let Some(v) = props.get_vec2("gui.anchor_point") { frame.anchor_point = v; }
                if let Some(v) = props.get_vec2("gui.canvas_size") { frame.canvas_size = v; }
                if let Some(v) = props.get_vec2("gui.canvas_position") { frame.canvas_position = v; }
                if let Some(v) = props.get_bool("gui.scroll_bar_enabled_x") { frame.scroll_bar_enabled_x = v; }
                if let Some(v) = props.get_bool("gui.scroll_bar_enabled_y") { frame.scroll_bar_enabled_y = v; }
                if let Some(v) = props.get_i32("gui.scroll_bar_thickness") { frame.scroll_bar_thickness = v; }
                if let Some(v) = props.get_bool("gui.scrolling_enabled") { frame.scrolling_enabled = v; }
                if let Some(v) = props.get_f32("gui.scroll_bar_image_transparency") { frame.scroll_bar_image_transparency = v; }
                if let Some(c) = props.get_color3("gui.scroll_bar_image_color3") { frame.scroll_bar_image_color3 = c; }
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
            if let Some(v) = gui.get("canvas_size").and_then(|v| v.as_array()) {
                if v.len() == 2 {
                    let x = v[0].as_float().unwrap_or(0.0) as f32;
                    let y = v[1].as_float().unwrap_or(0.0) as f32;
                    bag.set("gui.canvas_size", PropertyValue::Vector2([x, y]));
                }
            }
            if let Some(v) = gui.get("canvas_position").and_then(|v| v.as_array()) {
                if v.len() == 2 {
                    let x = v[0].as_float().unwrap_or(0.0) as f32;
                    let y = v[1].as_float().unwrap_or(0.0) as f32;
                    bag.set("gui.canvas_position", PropertyValue::Vector2([x, y]));
                }
            }
            if let Some(v) = gui.get("scroll_bar_enabled_x").and_then(|v| v.as_bool()) {
                bag.set("gui.scroll_bar_enabled_x", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("scroll_bar_enabled_y").and_then(|v| v.as_bool()) {
                bag.set("gui.scroll_bar_enabled_y", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("scroll_bar_thickness").and_then(|v| v.as_integer()) {
                bag.set("gui.scroll_bar_thickness", PropertyValue::Int(v as i32));
            }
            if let Some(v) = gui.get("scrolling_enabled").and_then(|v| v.as_bool()) {
                bag.set("gui.scrolling_enabled", PropertyValue::Bool(v));
            }
            if let Some(v) = gui.get("scroll_bar_image_transparency").and_then(|v| v.as_float()) {
                bag.set("gui.scroll_bar_image_transparency", PropertyValue::Float(v as f32));
            }
            if let Some(v) = gui.get("scroll_bar_image_color3").and_then(|v| v.as_array()) {
                if v.len() == 3 {
                    let r = v[0].as_float().unwrap_or(0.0) as f32;
                    let g = v[1].as_float().unwrap_or(0.0) as f32;
                    let b = v[2].as_float().unwrap_or(0.0) as f32;
                    bag.set("gui.scroll_bar_image_color3", PropertyValue::Color3([r, g, b]));
                }
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let bag = export_bag(world, entity);
        // Reuse the Frame spawner's emitter — same `[instance] /
        // [metadata] / [gui]` shape, only the key set differs.
        super::frame::emit_toml(&bag)
    }
}

const SCHEMA_TAG: u8 = 0xC5;

fn make_display(frame: &ScrollingFrame) -> GuiElementDisplay {
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
        // ScrollingFrame always clips children — that's the defining
        // behavioural difference vs. Frame at the renderer.
        clip_children: true,
        scroll_x: frame.canvas_position[0],
        scroll_y: frame.canvas_position[1],
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
        class_type: "ScrollingFrame".to_string(),
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
    if let Some(frame) = world.get::<ScrollingFrame>(entity) {
        bag.set("gui.visible", PropertyValue::Bool(frame.visible));
        bag.set("gui.background_color3", PropertyValue::Color3(frame.background_color3));
        bag.set("gui.background_transparency", PropertyValue::Float(frame.background_transparency));
        bag.set("gui.border_color3", PropertyValue::Color3(frame.border_color3));
        bag.set("gui.border_size_pixel", PropertyValue::Int(frame.border_size_pixel));
        bag.set("gui.z_index", PropertyValue::Int(frame.z_index));
        bag.set("gui.layout_order", PropertyValue::Int(frame.layout_order));
        bag.set("gui.rotation", PropertyValue::Float(frame.rotation));
        bag.set("gui.anchor_point", PropertyValue::Vector2(frame.anchor_point));
        bag.set("gui.canvas_size", PropertyValue::Vector2(frame.canvas_size));
        bag.set("gui.canvas_position", PropertyValue::Vector2(frame.canvas_position));
        bag.set("gui.scroll_bar_enabled_x", PropertyValue::Bool(frame.scroll_bar_enabled_x));
        bag.set("gui.scroll_bar_enabled_y", PropertyValue::Bool(frame.scroll_bar_enabled_y));
        bag.set("gui.scroll_bar_thickness", PropertyValue::Int(frame.scroll_bar_thickness));
        bag.set("gui.scrolling_enabled", PropertyValue::Bool(frame.scrolling_enabled));
        bag.set("gui.scroll_bar_image_transparency", PropertyValue::Float(frame.scroll_bar_image_transparency));
        bag.set("gui.scroll_bar_image_color3", PropertyValue::Color3(frame.scroll_bar_image_color3));
    }
    bag
}
