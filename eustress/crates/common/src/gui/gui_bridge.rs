//! # GUI Bridge — applies Rune/Luau GUI commands to GuiElementDisplay components
//!
//! Shared between engine (editor) and client (player) so scripts work in both.
//!
//! Each frame:
//! 1. `snapshot_gui_state` builds a name→text map for gui_get_text()
//! 2. Scripts run and push GuiCommands via gui_set_text(), gui_set_visible(), etc.
//! 3. `apply_gui_commands` drains the commands and updates GuiElementDisplay components

use bevy::prelude::*;
use super::billboard_renderer::GuiElementDisplay;
use super::gui_commands::{drain_gui_commands, set_gui_snapshot, GuiCommand};

/// Snapshot GUI element text values for gui_get_text() in scripts.
/// Runs BEFORE script execution each frame.
pub fn snapshot_gui_state(
    gui_query: Query<(&Name, &GuiElementDisplay)>,
) {
    let mut snapshot = std::collections::HashMap::new();
    for (name, display) in &gui_query {
        if !display.text.is_empty() {
            snapshot.insert(name.as_str().to_string(), display.text.clone());
        }
    }
    set_gui_snapshot(snapshot);
}

/// Apply pending GUI commands from Rune/Luau scripts to GuiElementDisplay components.
/// Runs AFTER script execution each frame.
pub fn apply_gui_commands(
    mut gui_query: Query<(&Name, &mut GuiElementDisplay)>,
) {
    let commands = drain_gui_commands();
    if commands.is_empty() { return; }

    for cmd in commands {
        let target_name = match &cmd {
            GuiCommand::SetText { name, .. } => name,
            GuiCommand::SetVisible { name, .. } => name,
            GuiCommand::SetBgColor { name, .. } => name,
            GuiCommand::SetTextColor { name, .. } => name,
            GuiCommand::SetBorderColor { name, .. } => name,
            GuiCommand::SetPosition { name, .. } => name,
            GuiCommand::SetSize { name, .. } => name,
            GuiCommand::SetFontSize { name, .. } => name,
            GuiCommand::OnClick { name, .. } => name,
        };

        // Find the entity by Name component
        for (name, mut display) in &mut gui_query {
            if name.as_str() != target_name { continue; }

            match &cmd {
                GuiCommand::SetText { text, .. } => {
                    display.text = text.clone();
                }
                GuiCommand::SetVisible { visible, .. } => {
                    display.visible = *visible;
                }
                GuiCommand::SetBgColor { r, g, b, a, .. } => {
                    display.bg_color = [*r, *g, *b, *a];
                }
                GuiCommand::SetTextColor { r, g, b, a, .. } => {
                    display.text_color = [*r, *g, *b, *a];
                }
                GuiCommand::SetBorderColor { r, g, b, a, .. } => {
                    display.border_color = [*r, *g, *b, *a];
                }
                GuiCommand::SetPosition { x, y, .. } => {
                    display.x = *x;
                    display.y = *y;
                }
                GuiCommand::SetSize { w, h, .. } => {
                    display.width = *w;
                    display.height = *h;
                }
                GuiCommand::SetFontSize { size, .. } => {
                    display.font_size = *size;
                }
                GuiCommand::OnClick { .. } => {
                    // Click registration is handled by the input system, not here
                }
            }
            break; // Found the target, stop searching
        }
    }
}

/// Plugin to register GUI bridge systems.
/// Add this plugin in both engine and client apps.
pub struct GuiBridgePlugin;

impl Plugin for GuiBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            snapshot_gui_state,
            apply_gui_commands.after(snapshot_gui_state),
        ));
    }
}
