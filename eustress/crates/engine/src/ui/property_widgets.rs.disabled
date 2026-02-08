//! Property Widgets - Dynamic UI generation for PropertyAccess
//! Phase 2, Week 1: Properties Panel Migration

#![allow(dead_code)]

use bevy::prelude::*;
use bevy_egui::egui;
use crate::properties::{PropertyValue, PropertyDescriptor};

/// Render a property widget and return new value if changed
pub fn property_widget(
    ui: &mut egui::Ui,
    descriptor: &PropertyDescriptor,
    value: PropertyValue,
) -> Option<PropertyValue> {
    ui.horizontal(|ui| {
        // Label with category color
        ui.label(
            egui::RichText::new(&descriptor.name)
                .color(category_color(&descriptor.category))
        );
        
        // Read-only indicator
        if descriptor.read_only {
            ui.label("🔒");
        }
        
        // Type-specific widget
        match value {
            PropertyValue::String(s) => string_widget(ui, s),
            PropertyValue::Float(f) => float_widget(ui, descriptor, f),
            PropertyValue::Int(i) => int_widget(ui, descriptor, i),
            PropertyValue::Bool(b) => bool_widget(ui, b),
            PropertyValue::Vector2(v) => vector2_widget(ui, descriptor, v),
            PropertyValue::Vector3(v) => vector3_widget(ui, descriptor, v),
            PropertyValue::Color(c) => color_widget(ui, c),
            PropertyValue::Color3(c) => color3_widget(ui, c),
            PropertyValue::Transform(t) => transform_widget(ui, t),
            PropertyValue::Material(m) => material_widget(ui, m),
            PropertyValue::Enum(e) => enum_widget(ui, descriptor, e),
        }
    }).inner
}

/// String property editor
fn string_widget(ui: &mut egui::Ui, value: String) -> Option<PropertyValue> {
    let mut text = value;
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .desired_width(150.0)
    );
    
    if response.changed() {
        Some(PropertyValue::String(text))
    } else {
        None
    }
}

/// Float property editor with drag value
fn float_widget(ui: &mut egui::Ui, descriptor: &PropertyDescriptor, value: f32) -> Option<PropertyValue> {
    let mut val = value;
    // Use fixed_decimals AND range to prevent egui smart_aim panic bug
    let mut drag = egui::DragValue::new(&mut val)
        .speed(0.1)
        .fixed_decimals(2);
    
    // Clamp based on property name hints
    if descriptor.name.contains("Transparency") || descriptor.name.contains("Reflectance") {
        drag = drag.range(0.0..=1.0);
    } else if descriptor.name.contains("Brightness") {
        drag = drag.range(0.0..=10.0);
    } else if descriptor.name.contains("Range") {
        drag = drag.range(0.0..=100.0);
    } else {
        // Default range to prevent smart_aim panic
        drag = drag.range(-100000.0..=100000.0);
    }
    
    if ui.add(drag).changed() {
        Some(PropertyValue::Float(val))
    } else {
        None
    }
}

/// Integer property editor
fn int_widget(ui: &mut egui::Ui, _descriptor: &PropertyDescriptor, value: i32) -> Option<PropertyValue> {
    let mut val = value;
    // Add range to prevent smart_aim panic
    if ui.add(egui::DragValue::new(&mut val).speed(1.0).range(i32::MIN..=i32::MAX)).changed() {
        Some(PropertyValue::Int(val))
    } else {
        None
    }
}

/// Boolean property editor (checkbox)
fn bool_widget(ui: &mut egui::Ui, value: bool) -> Option<PropertyValue> {
    let mut val = value;
    ui.checkbox(&mut val, "");
    // Simply check if value changed after checkbox interaction
    if val != value {
        Some(PropertyValue::Bool(val))
    } else {
        None
    }
}

/// Vector2 property editor (2 drag values)
fn vector2_widget(ui: &mut egui::Ui, _descriptor: &PropertyDescriptor, value: [f32; 2]) -> Option<PropertyValue> {
    let mut vec = value;
    let mut changed = false;
    
    ui.horizontal(|ui| {
        ui.label("X:");
        changed |= ui.add(egui::DragValue::new(&mut vec[0])
            .speed(0.1)
            .fixed_decimals(2)
            .range(-100000.0..=100000.0)
        ).changed();
        ui.label("Y:");
        changed |= ui.add(egui::DragValue::new(&mut vec[1])
            .speed(0.1)
            .fixed_decimals(2)
            .range(-100000.0..=100000.0)
        ).changed();
    });
    
    if changed {
        Some(PropertyValue::Vector2(vec))
    } else {
        None
    }
}

/// Vector3 property editor (3 drag values)
fn vector3_widget(ui: &mut egui::Ui, descriptor: &PropertyDescriptor, value: Vec3) -> Option<PropertyValue> {
    let mut vec = value;
    let mut changed = false;
    
    // Determine range based on property type
    let is_size = descriptor.name == "Size";
    let (min_val, max_val) = if is_size {
        (0.1, 10000.0)  // Size must be positive
    } else {
        (-100000.0, 100000.0)  // Position can be negative
    };
    
    // Use fixed_decimals AND range to prevent egui smart_aim panic bug
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label("X:");
            changed |= ui.add(egui::DragValue::new(&mut vec.x)
                .speed(0.1)
                .fixed_decimals(2)
                .range(min_val..=max_val)
            ).changed();
        });
        ui.horizontal(|ui| {
            ui.label("Y:");
            changed |= ui.add(egui::DragValue::new(&mut vec.y)
                .speed(0.1)
                .fixed_decimals(2)
                .range(min_val..=max_val)
            ).changed();
        });
        ui.horizontal(|ui| {
            ui.label("Z:");
            changed |= ui.add(egui::DragValue::new(&mut vec.z)
                .speed(0.1)
                .fixed_decimals(2)
                .range(min_val..=max_val)
            ).changed();
        });
    });
    
    // Additional validation for Size (must be positive)
    if is_size {
        vec.x = vec.x.max(0.1);
        vec.y = vec.y.max(0.1);
        vec.z = vec.z.max(0.1);
    }
    
    if changed {
        Some(PropertyValue::Vector3(vec))
    } else {
        None
    }
}

/// Color property editor (standard egui color picker)
fn color_widget(ui: &mut egui::Ui, value: Color) -> Option<PropertyValue> {
    let srgba = value.to_srgba();
    let mut rgb = [srgba.red, srgba.green, srgba.blue];
    
    // Standard color picker (egui's built-in spectrum picker)
    if ui.color_edit_button_rgb(&mut rgb).changed() {
        Some(PropertyValue::Color(Color::srgb(rgb[0], rgb[1], rgb[2])))
    } else {
        None
    }
}

/// Color3 property editor ([f32; 3] RGB array)
fn color3_widget(ui: &mut egui::Ui, value: [f32; 3]) -> Option<PropertyValue> {
    let mut rgb = value;
    
    // Standard color picker (egui's built-in spectrum picker)
    if ui.color_edit_button_rgb(&mut rgb).changed() {
        Some(PropertyValue::Color3(rgb))
    } else {
        None
    }
}

/// Transform property editor (position + rotation)
fn transform_widget(ui: &mut egui::Ui, value: Transform) -> Option<PropertyValue> {
    let mut transform = value;
    let mut changed = false;
    
    // Use fixed_decimals to prevent egui smart_aim panic bug
    ui.vertical(|ui| {
        ui.label("Position:");
        ui.horizontal(|ui| {
            ui.label("X:");
            changed |= ui.add(egui::DragValue::new(&mut transform.translation.x).speed(0.1).fixed_decimals(2)).changed();
            ui.label("Y:");
            changed |= ui.add(egui::DragValue::new(&mut transform.translation.y).speed(0.1).fixed_decimals(2)).changed();
            ui.label("Z:");
            changed |= ui.add(egui::DragValue::new(&mut transform.translation.z).speed(0.1).fixed_decimals(2)).changed();
        });
        
        // Euler angles for rotation
        let (mut x, mut y, mut z) = transform.rotation.to_euler(EulerRot::XYZ);
        x = x.to_degrees();
        y = y.to_degrees();
        z = z.to_degrees();
        
        ui.label("Rotation (degrees):");
        ui.horizontal(|ui| {
            ui.label("X:");
            if ui.add(egui::DragValue::new(&mut x).speed(1.0).fixed_decimals(1)).changed() {
                changed = true;
            }
            ui.label("Y:");
            if ui.add(egui::DragValue::new(&mut y).speed(1.0).fixed_decimals(1)).changed() {
                changed = true;
            }
            ui.label("Z:");
            if ui.add(egui::DragValue::new(&mut z).speed(1.0).fixed_decimals(1)).changed() {
                changed = true;
            }
        });
        
        if changed {
            transform.rotation = Quat::from_euler(
                EulerRot::XYZ,
                x.to_radians(),
                y.to_radians(),
                z.to_radians()
            );
        }
    });
    
    if changed {
        Some(PropertyValue::Transform(transform))
    } else {
        None
    }
}

/// Enum property editor (dropdown)
fn enum_widget(ui: &mut egui::Ui, descriptor: &PropertyDescriptor, value: String) -> Option<PropertyValue> {
    let mut selected = value.clone();
    
    // Get enum options based on property name
    let options = get_enum_options(&descriptor.name);
    
    egui::ComboBox::from_id_salt(&descriptor.name)
        .selected_text(&selected)
        .show_ui(ui, |ui| {
            for option in options {
                ui.selectable_value(&mut selected, option.to_string(), option);
            }
        });
    
    if selected != value {
        Some(PropertyValue::Enum(selected))
    } else {
        None
    }
}

/// Material property editor (dropdown with type-to-select)
fn material_widget(ui: &mut egui::Ui, value: crate::classes::Material) -> Option<PropertyValue> {
    use crate::classes::Material;

    let mut current = value;
    let mut changed = false;

    // Define all materials in alphabetical order for type-to-select
    let materials = [
        Material::Brick, Material::Concrete, Material::CorrodedMetal, Material::DiamondPlate,
        Material::Fabric, Material::Foil, Material::Glass, Material::Granite,
        Material::Grass, Material::Ice, Material::Marble, Material::Metal,
        Material::Neon, Material::Plastic, Material::Sand, Material::Slate,
        Material::SmoothPlastic, Material::Wood, Material::WoodPlanks,
    ];

    // Get current material name for display
    let current_name = format!("{:?}", current);

    // Create a unique ID for this dropdown's type-to-select state
    let dropdown_id = egui::Id::new("material_dropdown");
    let type_select_id = egui::Id::new("material_type_select");

    // Check if dropdown popup is open - type-to-select only works when open
    let popup_open = ui.memory(|mem| mem.is_popup_open(dropdown_id));

    // Clear old type-select state if dropdown is not open
    if !popup_open {
        ui.memory_mut(|mem| {
            mem.data.remove::<char>(type_select_id);
        });
    }

    // Capture single key press input (A-Z keys) ONLY when popup is open
    // This prevents interference with other shortcuts
    let mut typed_char = None;
    if popup_open {
        ui.input(|input| {
            for event in &input.events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
                    // Only handle letter keys without modifiers (or just shift for uppercase)
                    let no_ctrl_alt = !modifiers.ctrl && !modifiers.alt && !modifiers.command;
                    if no_ctrl_alt {
                        match key {
                            egui::Key::A => typed_char = Some('A'),
                            egui::Key::B => typed_char = Some('B'),
                            egui::Key::C => typed_char = Some('C'),
                            egui::Key::D => typed_char = Some('D'),
                            egui::Key::E => typed_char = Some('E'),
                            egui::Key::F => typed_char = Some('F'),
                            egui::Key::G => typed_char = Some('G'),
                            egui::Key::H => typed_char = Some('H'),
                            egui::Key::I => typed_char = Some('I'),
                            egui::Key::J => typed_char = Some('J'),
                            egui::Key::K => typed_char = Some('K'),
                            egui::Key::L => typed_char = Some('L'),
                            egui::Key::M => typed_char = Some('M'),
                            egui::Key::N => typed_char = Some('N'),
                            egui::Key::O => typed_char = Some('O'),
                            egui::Key::P => typed_char = Some('P'),
                            egui::Key::Q => typed_char = Some('Q'),
                            egui::Key::R => typed_char = Some('R'),
                            egui::Key::S => typed_char = Some('S'),
                            egui::Key::T => typed_char = Some('T'),
                            egui::Key::U => typed_char = Some('U'),
                            egui::Key::V => typed_char = Some('V'),
                            egui::Key::W => typed_char = Some('W'),
                            egui::Key::X => typed_char = Some('X'),
                            egui::Key::Y => typed_char = Some('Y'),
                            egui::Key::Z => typed_char = Some('Z'),
                            _ => {}
                        }
                    }
                }
            }
        });
    }

    // Store typed character and find matching material
    if let Some(ch) = typed_char {
        ui.memory_mut(|mem| {
            mem.data.insert_temp(type_select_id, ch);
        });

        // Find first material starting with this letter
        for &mat in &materials {
            let mat_name = format!("{:?}", mat);
            if mat_name.starts_with(ch) {
                current = mat;
                changed = true;
                break;
            }
        }
    }

    // ComboBox with normal functionality
    egui::ComboBox::from_id_salt("material_dropdown")
        .selected_text(current_name)
        .show_ui(ui, |ui| {
            // Render all materials
            for &mat in &materials {
                if ui.selectable_value(&mut current, mat, format!("{:?}", mat)).changed() {
                    changed = true;
                    // Clear type-select state when manually selected
                    ui.memory_mut(|mem| {
                        mem.data.remove::<char>(type_select_id);
                    });
                }
            }
        });

    if changed {
        Some(PropertyValue::Material(current))
    } else {
        None
    }
}

/// Get enum options for a property
fn get_enum_options(property_name: &str) -> Vec<&'static str> {
    match property_name {
        "Material" => vec![
            "Plastic", "SmoothPlastic", "Metal", "CorrodedMetal",
            "DiamondPlate", "Foil", "Grass", "Ice", "Marble",
            "Granite", "Brick", "Pebble", "Sand", "Fabric",
            "Glass", "Wood", "WoodPlanks", "Slate", "Concrete", "Neon"
        ],
        "Shape" => vec![
            "Block", "Ball", "Cylinder", "Wedge", "CornerWedge", "Cone"
        ],
        "RigType" => vec![
            "None", "R6", "R15"
        ],
        "CameraType" => vec![
            "Fixed", "Watch", "Attach", "Track", "Follow", "Custom", "Scriptable"
        ],
        "MeshType" => vec![
            "Head", "Torso", "Wedge", "Sphere", "Cylinder", "FileMesh", "Brick", "Prism", "Pyramid"
        ],
        "Face" => vec![
            "Top", "Bottom", "Front", "Back", "Left", "Right"
        ],
        "Priority" => vec![
            "Core", "Idle", "Movement", "Action", "Action2", "Action3", "Action4"
        ],
        "Operation" => vec![
            "Union", "Subtract", "Intersect"
        ],
        _ => vec![]
    }
}

/// Get color for property category
fn category_color(category: &str) -> egui::Color32 {
    match category {
        "Data" => egui::Color32::from_rgb(100, 150, 255),
        "Transform" => egui::Color32::from_rgb(255, 200, 100),
        "Appearance" => egui::Color32::from_rgb(255, 150, 200),
        "Physics" => egui::Color32::from_rgb(150, 255, 150),
        "AssemblyPhysics" => egui::Color32::from_rgb(100, 255, 200),
        "Collision" => egui::Color32::from_rgb(255, 100, 100),
        "Character" => egui::Color32::from_rgb(200, 150, 255),
        "State" => egui::Color32::from_rgb(150, 200, 255),
        "Light" => egui::Color32::from_rgb(255, 255, 150),
        "Motion" => egui::Color32::from_rgb(150, 255, 255),
        "Animation" => egui::Color32::from_rgb(255, 180, 150),
        "Playback" => egui::Color32::from_rgb(180, 255, 180),
        "Spatial" => egui::Color32::from_rgb(200, 200, 255),
        "Emission" => egui::Color32::from_rgb(255, 220, 180),
        "Water" => egui::Color32::from_rgb(100, 200, 255),
        "Shape" => egui::Color32::from_rgb(220, 180, 255),
        "Behavior" => egui::Color32::from_rgb(180, 220, 255),
        _ => egui::Color32::GRAY,
    }
}

/// Render property with validation feedback
pub fn property_widget_with_validation(
    ui: &mut egui::Ui,
    descriptor: &PropertyDescriptor,
    value: PropertyValue,
    validation_error: Option<&str>,
) -> Option<PropertyValue> {
    let result = property_widget(ui, descriptor, value);
    
    // Show validation error if present
    if let Some(error) = validation_error {
        ui.label(
            egui::RichText::new(format!("⚠ {}", error))
                .color(egui::Color32::RED)
                .small()
        );
    }
    
    result
}

/// Material preset buttons
pub fn material_preset_buttons(ui: &mut egui::Ui) -> Option<String> {
    let mut selected = None;
    
    ui.label("Quick Materials:");
    ui.horizontal_wrapped(|ui| {
        let materials = vec![
            ("🔲", "Plastic"),
            ("✨", "SmoothPlastic"),
            ("⚙️", "Metal"),
            ("💎", "Glass"),
            ("🌟", "Neon"),
            ("🪵", "Wood"),
            ("🧱", "Brick"),
            ("🪨", "Granite"),
        ];
        
        for (icon, material) in materials {
            if ui.button(format!("{} {}", icon, material)).clicked() {
                selected = Some(material.to_string());
            }
        }
    });
    
    selected
}

/// Color preset buttons
pub fn color_preset_buttons(ui: &mut egui::Ui) -> Option<Color> {
    let mut selected = None;
    
    ui.label("Quick Colors:");
    ui.horizontal_wrapped(|ui| {
        let colors = vec![
            ("Red", Color::srgb(1.0, 0.0, 0.0)),
            ("Green", Color::srgb(0.0, 1.0, 0.0)),
            ("Blue", Color::srgb(0.0, 0.0, 1.0)),
            ("Yellow", Color::srgb(1.0, 1.0, 0.0)),
            ("Orange", Color::srgb(1.0, 0.5, 0.0)),
            ("Purple", Color::srgb(0.5, 0.0, 1.0)),
            ("White", Color::srgb(1.0, 1.0, 1.0)),
            ("Black", Color::srgb(0.0, 0.0, 0.0)),
        ];
        
        for (name, color) in colors {
            let srgba = color.to_srgba();
            let rgb = [srgba.red, srgba.green, srgba.blue];
            if ui.add(egui::Button::new("").fill(egui::Color32::from_rgb(
                (rgb[0] * 255.0) as u8,
                (rgb[1] * 255.0) as u8,
                (rgb[2] * 255.0) as u8
            )).min_size(egui::Vec2::new(20.0, 20.0)))
            .on_hover_text(name)
            .clicked() {
                selected = Some(color);
            }
        }
    });
    
    selected
}
