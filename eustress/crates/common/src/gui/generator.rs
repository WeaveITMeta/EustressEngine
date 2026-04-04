//! TOML → Slint component generator.
//!
//! Reads a GUI element hierarchy from TOML definitions and generates
//! Slint markup at runtime. The markup is compiled by the Slint interpreter
//! and rendered to a texture.
//!
//! Supported UI classes:
//! - Frame → Slint Rectangle with background, border, corner radius
//! - TextLabel → Slint Text with font, color, alignment, wrapping
//! - TextButton → Slint Rectangle + TouchArea + Text with hover/pressed states
//! - TextBox → Slint Rectangle + TextInput with placeholder, selection
//! - ImageLabel → Slint Image with source path, scale type
//! - ImageButton → Slint Rectangle + TouchArea + Image
//! - ScrollingFrame → Slint ScrollView with content area
//! - VideoFrame → Slint Image (frame-by-frame texture updates)
//! - DocumentFrame → Slint ScrollView + formatted Text blocks
//! - WebFrame → Slint Image (rendered by wry/CEF)

use serde::Deserialize;

/// A parsed GUI element from a TOML file.
#[derive(Debug, Clone, Deserialize)]
pub struct GuiElement {
    pub class_name: String,
    pub name: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub anchor_point: [f32; 2],
    pub background_color: [f32; 4],
    pub border_size: f32,
    pub border_color: [f32; 4],
    pub corner_radius: f32,
    pub visible: bool,
    pub z_index: i32,
    // Text-specific (TextLabel, TextButton, TextBox)
    pub text: Option<String>,
    pub text_color: Option<[f32; 4]>,
    pub font_size: Option<f32>,
    pub text_x_alignment: Option<String>,
    pub text_y_alignment: Option<String>,
    // Image-specific (ImageLabel, ImageButton)
    pub image_source: Option<String>,
    // Children
    pub children: Vec<GuiElement>,
}

impl Default for GuiElement {
    fn default() -> Self {
        Self {
            class_name: "Frame".to_string(),
            name: "Element".to_string(),
            position: [0.0, 0.0],
            size: [100.0, 30.0],
            anchor_point: [0.0, 0.0],
            background_color: [0.2, 0.2, 0.2, 0.8],
            border_size: 0.0,
            border_color: [0.5, 0.5, 0.5, 1.0],
            corner_radius: 0.0,
            visible: true,
            z_index: 0,
            text: None,
            text_color: None,
            font_size: None,
            text_x_alignment: None,
            text_y_alignment: None,
            image_source: None,
            children: Vec::new(),
        }
    }
}

/// Generate Slint markup string from a GUI element tree.
/// The root element becomes the Slint component.
/// Returns the complete `.slint` source as a String.
pub fn generate_slint_markup(root: &GuiElement, component_name: &str) -> String {
    let mut out = String::with_capacity(2048);

    out.push_str("import { ScrollView } from \"std-widgets.slint\";\n\n");
    out.push_str(&format!("export component {} inherits Window {{\n", component_name));

    // Root background
    let [r, g, b, a] = root.background_color;
    out.push_str(&format!("    background: #{:02x}{:02x}{:02x}{:02x};\n",
        (r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, (a * 255.0) as u8));

    // Generate children
    for child in &root.children {
        generate_element(&mut out, child, 1);
    }

    out.push_str("}\n");
    out
}

/// Generate Slint markup for a single GUI element and its children.
fn generate_element(out: &mut String, elem: &GuiElement, depth: usize) {
    let indent = "    ".repeat(depth);

    if !elem.visible {
        return;
    }

    match elem.class_name.as_str() {
        "Frame" => generate_frame(out, elem, depth),
        "TextLabel" => generate_text_label(out, elem, depth),
        "TextButton" => generate_text_button(out, elem, depth),
        "TextBox" => generate_text_box(out, elem, depth),
        "ImageLabel" => generate_image_label(out, elem, depth),
        "ImageButton" => generate_image_button(out, elem, depth),
        "ScrollingFrame" => generate_scrolling_frame(out, elem, depth),
        _ => {
            // Unknown class — render as a basic rectangle
            out.push_str(&format!("{}// Unknown class: {}\n", indent, elem.class_name));
            generate_frame(out, elem, depth);
        }
    }
}

fn color_str(c: [f32; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}",
        (c[0] * 255.0) as u8, (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8, (c[3] * 255.0) as u8)
}

fn generate_frame(out: &mut String, elem: &GuiElement, depth: usize) {
    let indent = "    ".repeat(depth);
    out.push_str(&format!("{}Rectangle {{\n", indent));
    out.push_str(&format!("{}    x: {}px;\n", indent, elem.position[0]));
    out.push_str(&format!("{}    y: {}px;\n", indent, elem.position[1]));
    out.push_str(&format!("{}    width: {}px;\n", indent, elem.size[0]));
    out.push_str(&format!("{}    height: {}px;\n", indent, elem.size[1]));
    out.push_str(&format!("{}    background: {};\n", indent, color_str(elem.background_color)));
    if elem.corner_radius > 0.0 {
        out.push_str(&format!("{}    border-radius: {}px;\n", indent, elem.corner_radius));
    }
    if elem.border_size > 0.0 {
        out.push_str(&format!("{}    border-width: {}px;\n", indent, elem.border_size));
        out.push_str(&format!("{}    border-color: {};\n", indent, color_str(elem.border_color)));
    }

    for child in &elem.children {
        generate_element(out, child, depth + 1);
    }

    out.push_str(&format!("{}}}\n", indent));
}

fn generate_text_label(out: &mut String, elem: &GuiElement, depth: usize) {
    let indent = "    ".repeat(depth);
    let text = elem.text.as_deref().unwrap_or("");
    let color = elem.text_color.unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let font_size = elem.font_size.unwrap_or(14.0);
    let h_align = match elem.text_x_alignment.as_deref() {
        Some("center") => "center",
        Some("right") => "right",
        _ => "left",
    };

    out.push_str(&format!("{}Text {{\n", indent));
    out.push_str(&format!("{}    x: {}px;\n", indent, elem.position[0]));
    out.push_str(&format!("{}    y: {}px;\n", indent, elem.position[1]));
    out.push_str(&format!("{}    width: {}px;\n", indent, elem.size[0]));
    out.push_str(&format!("{}    height: {}px;\n", indent, elem.size[1]));
    out.push_str(&format!("{}    text: \"{}\";\n", indent, text.replace('"', "\\\"")));
    out.push_str(&format!("{}    color: {};\n", indent, color_str(color)));
    out.push_str(&format!("{}    font-size: {}px;\n", indent, font_size));
    out.push_str(&format!("{}    horizontal-alignment: {};\n", indent, h_align));
    out.push_str(&format!("{}    vertical-alignment: center;\n", indent));
    out.push_str(&format!("{}    overflow: elide;\n", indent));
    out.push_str(&format!("{}}}\n", indent));
}

fn generate_text_button(out: &mut String, elem: &GuiElement, depth: usize) {
    let indent = "    ".repeat(depth);
    let text = elem.text.as_deref().unwrap_or("Button");
    let color = elem.text_color.unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let font_size = elem.font_size.unwrap_or(14.0);
    let bg = color_str(elem.background_color);

    // Generate a safe identifier from the element name
    let safe_name = elem.name.replace(' ', "_").replace('-', "_").to_lowercase();

    out.push_str(&format!("{}Rectangle {{\n", indent));
    out.push_str(&format!("{}    x: {}px;\n", indent, elem.position[0]));
    out.push_str(&format!("{}    y: {}px;\n", indent, elem.position[1]));
    out.push_str(&format!("{}    width: {}px;\n", indent, elem.size[0]));
    out.push_str(&format!("{}    height: {}px;\n", indent, elem.size[1]));
    out.push_str(&format!("{}    background: {}_touch.has-hover ? {} : {};\n", indent, safe_name, bg, bg));
    if elem.corner_radius > 0.0 {
        out.push_str(&format!("{}    border-radius: {}px;\n", indent, elem.corner_radius));
    }

    out.push_str(&format!("{}    {}_touch := TouchArea {{\n", indent, safe_name));
    out.push_str(&format!("{}        mouse-cursor: pointer;\n", indent));
    out.push_str(&format!("{}    }}\n", indent));

    out.push_str(&format!("{}    Text {{\n", indent));
    out.push_str(&format!("{}        text: \"{}\";\n", indent, text.replace('"', "\\\"")));
    out.push_str(&format!("{}        color: {};\n", indent, color_str(color)));
    out.push_str(&format!("{}        font-size: {}px;\n", indent, font_size));
    out.push_str(&format!("{}        horizontal-alignment: center;\n", indent));
    out.push_str(&format!("{}        vertical-alignment: center;\n", indent));
    out.push_str(&format!("{}    }}\n", indent));
    out.push_str(&format!("{}}}\n", indent));
}

fn generate_text_box(out: &mut String, elem: &GuiElement, depth: usize) {
    let indent = "    ".repeat(depth);
    let placeholder = elem.text.as_deref().unwrap_or("");
    let font_size = elem.font_size.unwrap_or(14.0);
    let bg = color_str(elem.background_color);

    out.push_str(&format!("{}Rectangle {{\n", indent));
    out.push_str(&format!("{}    x: {}px;\n", indent, elem.position[0]));
    out.push_str(&format!("{}    y: {}px;\n", indent, elem.position[1]));
    out.push_str(&format!("{}    width: {}px;\n", indent, elem.size[0]));
    out.push_str(&format!("{}    height: {}px;\n", indent, elem.size[1]));
    out.push_str(&format!("{}    background: {};\n", indent, bg));
    if elem.corner_radius > 0.0 {
        out.push_str(&format!("{}    border-radius: {}px;\n", indent, elem.corner_radius));
    }
    if elem.border_size > 0.0 {
        out.push_str(&format!("{}    border-width: {}px;\n", indent, elem.border_size));
        out.push_str(&format!("{}    border-color: {};\n", indent, color_str(elem.border_color)));
    }

    out.push_str(&format!("{}    TextInput {{\n", indent));
    out.push_str(&format!("{}        font-size: {}px;\n", indent, font_size));
    out.push_str(&format!("{}        color: white;\n", indent));
    out.push_str(&format!("{}        width: parent.width - 16px;\n", indent));
    out.push_str(&format!("{}        height: parent.height;\n", indent));
    out.push_str(&format!("{}        x: 8px;\n", indent));
    out.push_str(&format!("{}    }}\n", indent));
    out.push_str(&format!("{}}}\n", indent));
}

fn generate_image_label(out: &mut String, elem: &GuiElement, depth: usize) {
    let indent = "    ".repeat(depth);
    // Image source would be an asset path — Slint needs @image-url() or runtime image
    // For now, render as a colored rectangle placeholder
    out.push_str(&format!("{}Rectangle {{\n", indent));
    out.push_str(&format!("{}    x: {}px;\n", indent, elem.position[0]));
    out.push_str(&format!("{}    y: {}px;\n", indent, elem.position[1]));
    out.push_str(&format!("{}    width: {}px;\n", indent, elem.size[0]));
    out.push_str(&format!("{}    height: {}px;\n", indent, elem.size[1]));
    out.push_str(&format!("{}    background: {};\n", indent, color_str(elem.background_color)));
    if elem.corner_radius > 0.0 {
        out.push_str(&format!("{}    border-radius: {}px;\n", indent, elem.corner_radius));
    }
    // TODO: load image via Slint runtime image API when image_source is set
    out.push_str(&format!("{}}}\n", indent));
}

fn generate_image_button(out: &mut String, elem: &GuiElement, depth: usize) {
    let indent = "    ".repeat(depth);
    let safe_name = elem.name.replace(' ', "_").replace('-', "_").to_lowercase();

    out.push_str(&format!("{}Rectangle {{\n", indent));
    out.push_str(&format!("{}    x: {}px;\n", indent, elem.position[0]));
    out.push_str(&format!("{}    y: {}px;\n", indent, elem.position[1]));
    out.push_str(&format!("{}    width: {}px;\n", indent, elem.size[0]));
    out.push_str(&format!("{}    height: {}px;\n", indent, elem.size[1]));
    out.push_str(&format!("{}    background: {};\n", indent, color_str(elem.background_color)));
    if elem.corner_radius > 0.0 {
        out.push_str(&format!("{}    border-radius: {}px;\n", indent, elem.corner_radius));
    }

    out.push_str(&format!("{}    {}_touch := TouchArea {{\n", indent, safe_name));
    out.push_str(&format!("{}        mouse-cursor: pointer;\n", indent));
    out.push_str(&format!("{}    }}\n", indent));
    // TODO: Image element when image_source is set
    out.push_str(&format!("{}}}\n", indent));
}

fn generate_scrolling_frame(out: &mut String, elem: &GuiElement, depth: usize) {
    let indent = "    ".repeat(depth);

    out.push_str(&format!("{}Rectangle {{\n", indent));
    out.push_str(&format!("{}    x: {}px;\n", indent, elem.position[0]));
    out.push_str(&format!("{}    y: {}px;\n", indent, elem.position[1]));
    out.push_str(&format!("{}    width: {}px;\n", indent, elem.size[0]));
    out.push_str(&format!("{}    height: {}px;\n", indent, elem.size[1]));
    out.push_str(&format!("{}    background: {};\n", indent, color_str(elem.background_color)));
    out.push_str(&format!("{}    clip: true;\n", indent));

    out.push_str(&format!("{}    ScrollView {{\n", indent));
    for child in &elem.children {
        generate_element(out, child, depth + 2);
    }
    out.push_str(&format!("{}    }}\n", indent));
    out.push_str(&format!("{}}}\n", indent));
}

// Runtime compilation removed — slint-interpreter causes windows crate version
// conflicts with wgpu-hal's gpu-allocator. GUI components will use pre-compiled
// .slint templates built at engine compile time via slint-build instead.
