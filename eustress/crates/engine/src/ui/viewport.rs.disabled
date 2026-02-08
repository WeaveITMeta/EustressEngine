#![allow(dead_code)]

use bevy_egui::egui;
use super::StudioState;
use crate::editor_settings::EditorSettings;

/// Show viewport overlay (FPS, gizmo hints, etc.)
pub fn show_overlay(ctx: &egui::Context, state: &StudioState) {
    show_overlay_with_settings(ctx, state, &EditorSettings::default(), 0, 0.0);
}

/// Show viewport overlay with editor settings and diagnostics
pub fn show_overlay_with_settings(
    ctx: &egui::Context, 
    state: &StudioState, 
    settings: &EditorSettings,
    selection_count: usize,
    fps: f32,
) {
    // Calculate panel offsets to position overlays within viewport
    let left_offset = if state.show_explorer { 260.0 } else { 10.0 };
    let right_offset = if state.show_properties { -310.0 } else { -10.0 };
    let top_offset = 50.0; // Account for ribbon
    let bottom_offset = if state.show_output { -160.0 } else { -10.0 };
    
    // Tool & Selection info (top-left of viewport)
    egui::Area::new(egui::Id::new("tool_info"))
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(left_offset, top_offset))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_black_alpha(180))
                .corner_radius(8.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    // Current tool with icon
                    let (tool_icon, tool_name, tool_color) = match state.current_tool {
                        super::Tool::Select => ("🖱", "SELECT", egui::Color32::from_rgb(100, 200, 255)),
                        super::Tool::Move => ("↔", "MOVE", egui::Color32::from_rgb(255, 100, 100)),
                        super::Tool::Rotate => ("🔄", "ROTATE", egui::Color32::from_rgb(100, 255, 100)),
                        super::Tool::Scale => ("⬌", "SCALE", egui::Color32::from_rgb(255, 255, 100)),
                    };
                    
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{} {}", tool_icon, tool_name))
                            .size(16.0)
                            .color(tool_color)
                            .strong());
                    });
                    
                    // Transform space
                    let space_text = match state.transform_mode {
                        super::TransformMode::Local => "Local",
                        super::TransformMode::Global => "World",
                    };
                    ui.label(egui::RichText::new(format!("Space: {}", space_text))
                        .size(12.0)
                        .color(egui::Color32::LIGHT_GRAY));
                    
                    // Snap info
                    let snap_text = if settings.snap_enabled {
                        format!("Snap: {} m", settings.snap_size)
                    } else {
                        "Snap: Off".to_string()
                    };
                    ui.label(egui::RichText::new(snap_text)
                        .size(12.0)
                        .color(if settings.snap_enabled { 
                            egui::Color32::from_rgb(100, 255, 100) 
                        } else { 
                            egui::Color32::GRAY 
                        }));
                    
                    // Selection count
                    if selection_count > 0 {
                        ui.label(egui::RichText::new(format!("Selected: {}", selection_count))
                            .size(12.0)
                            .color(egui::Color32::from_rgb(255, 200, 100)));
                    }
                });
        });
    
    // FPS counter (top-right of viewport)
    egui::Area::new(egui::Id::new("fps_counter"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(right_offset, top_offset))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_black_alpha(150))
                .corner_radius(6.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    let fps_color = if fps >= 55.0 {
                        egui::Color32::from_rgb(0, 255, 106)
                    } else if fps >= 30.0 {
                        egui::Color32::YELLOW
                    } else {
                        egui::Color32::RED
                    };
                    ui.label(egui::RichText::new(format!("FPS: {:.0}", fps))
                        .monospace()
                        .color(fps_color));
                });
        });
    
    // Keyboard shortcuts hint (bottom-left of viewport) - collapsible
    egui::Area::new(egui::Id::new("shortcuts_hint"))
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(left_offset, bottom_offset))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_black_alpha(180))
                .corner_radius(8.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("⌨ Shortcuts").size(12.0).color(egui::Color32::WHITE));
                    ui.separator();
                    
                    // Tool shortcuts - CORRECT ones
                    ui.label(egui::RichText::new("Alt+Z  Select").size(10.0).color(egui::Color32::LIGHT_GRAY));
                    ui.label(egui::RichText::new("Alt+X  Move").size(10.0).color(egui::Color32::LIGHT_GRAY));
                    ui.label(egui::RichText::new("Alt+C  Rotate").size(10.0).color(egui::Color32::LIGHT_GRAY));
                    ui.label(egui::RichText::new("Alt+V  Scale").size(10.0).color(egui::Color32::LIGHT_GRAY));
                    ui.separator();
                    ui.label(egui::RichText::new("Del    Delete").size(10.0).color(egui::Color32::LIGHT_GRAY));
                    ui.label(egui::RichText::new("Ctrl+D Duplicate").size(10.0).color(egui::Color32::LIGHT_GRAY));
                    ui.label(egui::RichText::new("Ctrl+G Group").size(10.0).color(egui::Color32::LIGHT_GRAY));
                    ui.label(egui::RichText::new("Ctrl+U Ungroup").size(10.0).color(egui::Color32::LIGHT_GRAY));
                    ui.label(egui::RichText::new("F      Focus").size(10.0).color(egui::Color32::LIGHT_GRAY));
                });
        });
    
    // Grid/Snap info (bottom-right of viewport)
    egui::Area::new(egui::Id::new("grid_info"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(right_offset, bottom_offset))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_black_alpha(150))
                .corner_radius(6.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    let grid_icon = if settings.show_grid { "✓" } else { "✗" };
                    ui.label(egui::RichText::new(format!("{} Grid", grid_icon))
                        .size(11.0)
                        .color(if settings.show_grid { egui::Color32::LIGHT_GREEN } else { egui::Color32::GRAY }));
                    
                    let snap_icon = if settings.snap_enabled { "✓" } else { "✗" };
                    ui.label(egui::RichText::new(format!("{} Snap ({})", snap_icon, settings.snap_size))
                        .size(11.0)
                        .color(if settings.snap_enabled { egui::Color32::LIGHT_GREEN } else { egui::Color32::GRAY }));
                    
                    ui.separator();
                    ui.label(egui::RichText::new("1/2/3 - Snap modes")
                        .size(10.0)
                        .color(egui::Color32::GRAY));
                });
        });
}
