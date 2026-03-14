//! # GUI Element Loader — Parse .textlabel.toml, .frame.toml, etc. into Bevy UI entities
//!
//! ## Table of Contents
//!
//! 1. GuiTomlDefinition — deserialization structs for GUI TOML files
//! 2. load_gui_definition — parse a GUI TOML file from disk
//! 3. spawn_gui_element — spawn a Bevy UI entity with proper rendering components
//!
//! ## Architecture
//!
//! GUI TOML files in StarterGui use this format:
//!   [instance]     — name
//!   [metadata]     — class_name, archivable
//!   [gui]          — position, size, background_color, border_size, z_index, etc.
//!   [text]         — text, text_color, font_size, alignment (TextLabel/TextButton only)
//!
//! Each element is spawned with Bevy UI components (Node, BackgroundColor, Text, etc.)
//! so they render visually in the viewport overlay. ScreenGui is a fullscreen root container;
//! child elements (Frame, TextLabel, TextButton) are positioned absolutely within it.

use bevy::prelude::*;
use bevy::ui::{self};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::spawn::{TextLabelMarker, TextBoxMarker};

// ============================================================================
// 1. TOML deserialization structs
// ============================================================================

/// Top-level GUI TOML file structure
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlFile {
    #[serde(default)]
    pub instance: GuiTomlInstance,
    #[serde(default)]
    pub metadata: GuiTomlMetadata,
    #[serde(default)]
    pub gui: GuiTomlProperties,
    #[serde(default)]
    pub text: Option<GuiTomlText>,
    // ScreenGui files may use instance_loader format with [asset], [transform], [properties]
    #[serde(default)]
    pub asset: Option<toml::Value>,
    #[serde(default)]
    pub transform: Option<toml::Value>,
    #[serde(default)]
    pub properties: Option<toml::Value>,
}

/// [instance] section
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlInstance {
    #[serde(default)]
    pub name: String,
}

/// [metadata] section
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlMetadata {
    #[serde(default)]
    pub class_name: String,
    #[serde(default)]
    pub archivable: bool,
}

/// [gui] section — shared visual properties
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlProperties {
    #[serde(default)]
    pub position: [f32; 2],
    #[serde(default = "default_size")]
    pub size: [f32; 2],
    #[serde(default)]
    pub anchor_point: [f32; 2],
    #[serde(default = "default_bg_color")]
    pub background_color: [f32; 4],
    #[serde(default)]
    pub border_size: f32,
    #[serde(default = "default_border_color")]
    pub border_color: [f32; 4],
    #[serde(default)]
    pub corner_radius: f32,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub z_index: i32,
}

/// [text] section — text-specific properties for TextLabel, TextButton, TextBox
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlText {
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_text_color")]
    pub text_color: [f32; 4],
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub font_family: String,
    #[serde(default = "default_left")]
    pub text_x_alignment: String,
    #[serde(default = "default_center")]
    pub text_y_alignment: String,
}

fn default_size() -> [f32; 2] { [100.0, 30.0] }
fn default_bg_color() -> [f32; 4] { [0.2, 0.2, 0.2, 0.8] }
fn default_border_color() -> [f32; 4] { [0.5, 0.5, 0.5, 1.0] }
fn default_text_color() -> [f32; 4] { [1.0, 1.0, 1.0, 1.0] }
fn default_font_size() -> f32 { 14.0 }
fn default_left() -> String { "left".to_string() }
fn default_center() -> String { "center".to_string() }
fn default_true() -> bool { true }

// ============================================================================
// 2. Create / Write GUI TOML files
// ============================================================================

/// Create a default GUI TOML definition for a given class name.
///
/// Returns a `GuiTomlFile` with sensible defaults that can be serialized to disk.
pub fn create_default_gui_toml(class_name: &str, display_name: &str) -> GuiTomlFile {
    let has_text = matches!(
        class_name,
        "TextLabel" | "TextButton" | "TextBox"
    );

    let text = if has_text {
        Some(GuiTomlText {
            text: display_name.to_string(),
            text_color: default_text_color(),
            font_size: default_font_size(),
            font_family: String::new(),
            text_x_alignment: "center".to_string(),
            text_y_alignment: "center".to_string(),
        })
    } else {
        None
    };

    let size = match class_name {
        "ScreenGui" => [0.0, 0.0],     // ScreenGui is fullscreen, size is ignored
        "Frame" | "ScrollingFrame" => [200.0, 150.0],
        _ => default_size(),
    };

    let bg = match class_name {
        "ScreenGui" => [0.0, 0.0, 0.0, 0.0], // Transparent
        "TextButton" => [0.25, 0.25, 0.3, 0.9],
        _ => default_bg_color(),
    };

    GuiTomlFile {
        instance: GuiTomlInstance {
            name: display_name.to_string(),
        },
        metadata: GuiTomlMetadata {
            class_name: class_name.to_string(),
            archivable: true,
        },
        gui: GuiTomlProperties {
            position: [0.0, 0.0],
            size,
            anchor_point: [0.0, 0.0],
            background_color: bg,
            border_size: if class_name == "TextBox" { 1.0 } else { 0.0 },
            border_color: default_border_color(),
            corner_radius: if class_name == "TextButton" { 4.0 } else { 0.0 },
            visible: true,
            z_index: 0,
        },
        text,
        asset: None,
        transform: None,
        properties: None,
    }
}

/// Write a GUI TOML definition to disk
pub fn write_gui_toml(path: &Path, gui_def: &GuiTomlFile) -> Result<(), String> {
    let content = toml::to_string_pretty(gui_def)
        .map_err(|e| format!("Failed to serialize GUI TOML: {}", e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
    }
    std::fs::write(path, content)
        .map_err(|e| format!("Failed to write GUI TOML {:?}: {}", path, e))?;
    Ok(())
}

// ============================================================================
// 3. load_gui_definition — parse from disk
// ============================================================================

/// Load and parse a GUI TOML file from disk
pub fn load_gui_definition(path: &Path) -> Result<GuiTomlFile, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read GUI file {:?}: {}", path, e))?;
    let parsed: GuiTomlFile = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse GUI TOML {:?}: {}", path, e))?;
    Ok(parsed)
}

/// Determine the GUI element type from the file extension
pub fn gui_class_from_extension(path: &Path) -> &'static str {
    let path_str = path.to_string_lossy();
    if path_str.ends_with(".screengui.toml") { return "ScreenGui"; }
    if path_str.ends_with(".textlabel.toml") { return "TextLabel"; }
    if path_str.ends_with(".textbutton.toml") { return "TextButton"; }
    if path_str.ends_with(".frame.toml") { return "Frame"; }
    if path_str.ends_with(".imagelabel.toml") { return "ImageLabel"; }
    if path_str.ends_with(".imagebutton.toml") { return "ImageButton"; }
    if path_str.ends_with(".scrollingframe.toml") { return "ScrollingFrame"; }
    if path_str.ends_with(".textbox.toml") { return "TextBox"; }
    if path_str.ends_with(".viewportframe.toml") { return "ViewportFrame"; }
    "Frame" // default
}

/// Extract display name from filename (everything before first dot)
pub fn gui_display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.splitn(2, '.').next())
        .unwrap_or("Unknown")
        .to_string()
}

// ============================================================================
// 3. spawn_gui_element — create Bevy UI entity with rendering components
// ============================================================================

/// Spawn a GUI element entity with proper Bevy UI rendering components.
///
/// Reads the TOML file, determines the element type from the file extension,
/// and spawns with Node, BackgroundColor, Text, etc. so it renders visually.
pub fn spawn_gui_element(
    commands: &mut Commands,
    path: &Path,
    gui_def: &GuiTomlFile,
) -> Entity {
    let gui_type = gui_class_from_extension(path);
    let display_name = if !gui_def.instance.name.is_empty() {
        gui_def.instance.name.clone()
    } else {
        gui_display_name(path)
    };

    let gui = &gui_def.gui;
    let class_name = match gui_type {
        "ScreenGui" => eustress_common::classes::ClassName::ScreenGui,
        "TextLabel" => eustress_common::classes::ClassName::TextLabel,
        "TextButton" => eustress_common::classes::ClassName::TextButton,
        "Frame" => eustress_common::classes::ClassName::Frame,
        "ImageLabel" => eustress_common::classes::ClassName::ImageLabel,
        "ImageButton" => eustress_common::classes::ClassName::ImageButton,
        "ScrollingFrame" => eustress_common::classes::ClassName::ScrollingFrame,
        "TextBox" => eustress_common::classes::ClassName::TextBox,
        "ViewportFrame" => eustress_common::classes::ClassName::ViewportFrame,
        _ => eustress_common::classes::ClassName::Frame,
    };

    let instance = eustress_common::classes::Instance {
        name: display_name.clone(),
        class_name,
        archivable: gui_def.metadata.archivable,
        id: 0,
        ai: false,
    };

    let loaded_from = super::file_loader::LoadedFromFile {
        path: path.to_path_buf(),
        file_type: super::file_loader::FileType::GuiElement,
        service: "StarterGui".to_string(),
    };

    match gui_type {
        "ScreenGui" => spawn_screen_gui_element(commands, instance, loaded_from, &display_name),
        "TextLabel" => spawn_text_label_element(commands, instance, loaded_from, &display_name, gui, gui_def.text.as_ref()),
        "TextButton" => spawn_text_button_element(commands, instance, loaded_from, &display_name, gui, gui_def.text.as_ref()),
        "Frame" => spawn_frame_element(commands, instance, loaded_from, &display_name, gui),
        "ScrollingFrame" => spawn_scrolling_frame_element(commands, instance, loaded_from, &display_name, gui),
        "TextBox" => spawn_text_box_element(commands, instance, loaded_from, &display_name, gui, gui_def.text.as_ref()),
        // ImageLabel, ImageButton, ViewportFrame — spawn as frames for now (need asset loading)
        _ => spawn_frame_element(commands, instance, loaded_from, &display_name, gui),
    }
}

/// ScreenGui — fullscreen absolute overlay root container
fn spawn_screen_gui_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
) -> Entity {
    commands.spawn((
        instance,
        loaded_from,
        Name::new(display_name.to_string()),
        // Fullscreen Bevy UI root
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            ..default()
        },
        GlobalZIndex(100), // Above 3D scene, below Slint overlay
        // Transparent so it doesn't block the 3D view
        BackgroundColor(Color::NONE),
    )).id()
}

/// Frame — container with background color and optional border
fn spawn_frame_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
) -> Entity {
    let bg = to_color(&gui.background_color);
    let border = to_color(&gui.border_color);

    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(gui.position[0]),
            top: Val::Px(gui.position[1]),
            width: Val::Px(gui.size[0]),
            height: Val::Px(gui.size[1]),
            border: UiRect::all(Val::Px(gui.border_size)),
            ..default()
        },
        BackgroundColor(bg),
        ui::BorderColor::from(border),
        ZIndex(gui.z_index),
    )).id();
    commands.entity(entity).insert(loaded_from);
    entity
}

/// ScrollingFrame — frame with scroll overflow
fn spawn_scrolling_frame_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
) -> Entity {
    let bg = to_color(&gui.background_color);
    let border = to_color(&gui.border_color);

    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(gui.position[0]),
            top: Val::Px(gui.position[1]),
            width: Val::Px(gui.size[0]),
            height: Val::Px(gui.size[1]),
            border: UiRect::all(Val::Px(gui.border_size)),
            overflow: Overflow::scroll(),
            ..default()
        },
        BackgroundColor(bg),
        ui::BorderColor::from(border),
        ZIndex(gui.z_index),
    )).id();
    commands.entity(entity).insert(loaded_from);
    commands.entity(entity).insert(ScrollPosition::default());
    entity
}

/// TextLabel — non-interactive text display with background
fn spawn_text_label_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
    text_props: Option<&GuiTomlText>,
) -> Entity {
    let bg = to_color(&gui.background_color);
    let (text_str, text_color, font_size, justify, align_items) = resolve_text_props(text_props);

    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        TextLabelMarker,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(gui.position[0]),
            top: Val::Px(gui.position[1]),
            width: Val::Px(gui.size[0]),
            height: Val::Px(gui.size[1]),
            justify_content: justify,
            align_items,
            ..default()
        },
        BackgroundColor(bg),
        ZIndex(gui.z_index),
    )).id();
    commands.entity(entity).insert(loaded_from);
    commands.entity(entity).insert((Text::new(text_str),
        TextColor(text_color),
        TextFont { font_size, ..default() },
        bevy::ui::widget::Label,
    ));
    entity
}

/// TextButton — clickable text with background and border
fn spawn_text_button_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
    text_props: Option<&GuiTomlText>,
) -> Entity {
    let bg = to_color(&gui.background_color);
    let border = to_color(&gui.border_color);
    let (text_str, text_color, font_size, _justify, _align) = resolve_text_props(text_props);

    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(gui.position[0]),
            top: Val::Px(gui.position[1]),
            width: Val::Px(gui.size[0]),
            height: Val::Px(gui.size[1]),
            border: UiRect::all(Val::Px(gui.border_size)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(bg),
        ui::BorderColor::from(border),
        ZIndex(gui.z_index),
    )).id();
    commands.entity(entity).insert(loaded_from);
    commands.entity(entity).insert((
        bevy::ui::widget::Button,
        Interaction::default(),
        Text::new(text_str),
        TextColor(text_color),
        TextFont { font_size, ..default() },
    ));
    entity
}

/// TextBox — text input field
fn spawn_text_box_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
    text_props: Option<&GuiTomlText>,
) -> Entity {
    let bg = to_color(&gui.background_color);
    let border = to_color(&gui.border_color);
    let (text_str, text_color, font_size, _justify, _align) = resolve_text_props(text_props);

    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        TextBoxMarker,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(gui.position[0]),
            top: Val::Px(gui.position[1]),
            width: Val::Px(gui.size[0]),
            height: Val::Px(gui.size[1]),
            border: UiRect::all(Val::Px(gui.border_size)),
            padding: UiRect::all(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(bg),
        ui::BorderColor::from(border),
        ZIndex(gui.z_index),
    )).id();
    commands.entity(entity).insert(loaded_from);
    commands.entity(entity).insert((
        Interaction::default(),
        Text::new(text_str),
        TextColor(text_color),
        TextFont { font_size, ..default() },
    ));
    entity
}

// ============================================================================
// Helpers
// ============================================================================

/// Convert a [f32; 4] RGBA array to a Bevy Color
fn to_color(rgba: &[f32; 4]) -> Color {
    Color::srgba(rgba[0], rgba[1], rgba[2], rgba[3])
}

/// Extract text properties from optional [text] section, with defaults
fn resolve_text_props(text_props: Option<&GuiTomlText>) -> (String, Color, f32, JustifyContent, AlignItems) {
    match text_props {
        Some(t) => {
            let text_color = to_color(&t.text_color);
            let justify = match t.text_x_alignment.as_str() {
                "left" => JustifyContent::FlexStart,
                "center" => JustifyContent::Center,
                "right" => JustifyContent::FlexEnd,
                _ => JustifyContent::FlexStart,
            };
            let align = match t.text_y_alignment.as_str() {
                "top" => AlignItems::FlexStart,
                "center" => AlignItems::Center,
                "bottom" => AlignItems::FlexEnd,
                _ => AlignItems::Center,
            };
            (t.text.clone(), text_color, t.font_size, justify, align)
        }
        None => (
            String::new(),
            Color::WHITE,
            14.0,
            JustifyContent::FlexStart,
            AlignItems::Center,
        ),
    }
}
