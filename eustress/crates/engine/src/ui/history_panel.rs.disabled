// History Panel - Visualize and navigate undo/redo history
use bevy_egui::egui;
use bevy::prelude::*;
use crate::undo::UndoStack;
use egui_material_icons::icons::{ICON_UNDO, ICON_REDO};

/// History panel showing undo/redo stack
pub struct HistoryPanel;

impl HistoryPanel {
    /// Show history panel using UndoStack
    pub fn show_with_undo_stack(
        ui: &mut egui::Ui,
        undo_stack: &UndoStack,
        undo_requested: &mut bool,
        redo_requested: &mut bool,
    ) {
        ui.add_space(4.0);  // Top padding
        ui.horizontal(|ui| {
            ui.add_space(8.0);  // Left padding
            ui.heading("📜 History");
        });
        ui.separator();
        
        // Get history info
        let can_undo = undo_stack.can_undo();
        let can_redo = undo_stack.can_redo();
        let last_action = undo_stack.last_action_description();
        let next_redo = undo_stack.next_redo_description();
        
        // Current state indicator
        ui.horizontal(|ui| {
            ui.add_space(8.0);  // Left padding
            if can_undo || can_redo {
                ui.label(egui::RichText::new("Actions recorded").color(egui::Color32::from_rgb(100, 200, 100)));
            } else {
                ui.label(egui::RichText::new("No actions yet").weak().italics());
            }
        });
        
        ui.separator();
        
        // Show last action (what would be undone)
        if let Some(desc) = &last_action {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("✅").color(egui::Color32::from_rgb(100, 200, 100)));
                ui.label(egui::RichText::new(format!("Last: {}", desc)).strong());
            });
        }
        
        // Show next redo action (if any)
        if let Some(desc) = &next_redo {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("⏸").color(egui::Color32::GRAY));
                ui.label(egui::RichText::new(format!("Undone: {}", desc)).weak());
            });
        }
        
        ui.separator();
        
        // Undo/Redo buttons
        ui.horizontal(|ui| {
            // Undo button with Material Icon
            let undo_button = egui::Button::new(if let Some(desc) = &last_action {
                format!("{} Undo: {}", ICON_UNDO, desc)
            } else {
                format!("{} Undo", ICON_UNDO)
            });
            
            if ui.add_enabled(can_undo, undo_button).clicked() {
                *undo_requested = true;
            }
            
            // Redo button with Material Icon
            let redo_button = egui::Button::new(if let Some(desc) = &next_redo {
                format!("{} Redo: {}", ICON_REDO, desc)
            } else {
                format!("{} Redo", ICON_REDO)
            });
            
            if ui.add_enabled(can_redo, redo_button).clicked() {
                *redo_requested = true;
            }
        });
        
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Ctrl + Z = Undo | Ctrl + Y = Redo").small().weak());
        
        ui.add_space(16.0);
        ui.separator();
        
        // Tips
        ui.label(egui::RichText::new("💡 Tips:").strong());
        ui.label(egui::RichText::new("• Move, rotate, or scale parts to record actions").small().weak());
        ui.label(egui::RichText::new("• Delete parts with Del key").small().weak());
        ui.label(egui::RichText::new("• Actions are automatically merged when editing quickly").small().weak());
    }
}
