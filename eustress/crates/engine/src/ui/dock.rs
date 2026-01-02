#![allow(dead_code)]

use bevy::prelude::*;
use bevy_egui::egui;
use egui_dock::{DockState, Style, TabViewer};
use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::{
    BevySelectionManager,
    AssetManagerState, CollaborationState, ToolboxState,
    ExplorerPanel, OutputPanel, OutputConsole, AssetManagerPanel, CollaborationPanel, ToolboxPanel,
    DynamicPropertiesPanel,
    HistoryPanel,
    SpawnPartEvent,
};

/// Tabs that can be docked in the workspace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Toolbox,
    Explorer,
    Properties,
    History,
    Output,
    AssetManager,
    Collaboration,
}

impl std::fmt::Display for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tab::Toolbox => write!(f, "🛠 Toolbox"),
            Tab::Explorer => write!(f, "📁 Explorer"),
            Tab::Properties => write!(f, "⚙ Properties"),
            Tab::History => write!(f, "📜 History"),
            Tab::Output => write!(f, "📋 Output"),
            Tab::AssetManager => write!(f, "📦 Assets"),
            Tab::Collaboration => write!(f, "👥 Collaborate"),
        }
    }
}

/// Resource containing the dock state
#[derive(Resource)]
pub struct StudioDockState {
    pub tree: DockState<Tab>,
    pub left_tab: LeftTab,
    pub right_tab: RightTab,
    pub secondary_right_tab: SecondaryRightTab,
    pub show_secondary_right: bool,
}

/// Left panel tabs
#[derive(Default, PartialEq, Clone, Copy)]
pub enum LeftTab {
    #[default]
    Explorer,
    Toolbox,
    Assets,
    MindSpace,
}

/// Right panel tabs
#[derive(Default, PartialEq, Clone, Copy)]
pub enum RightTab {
    #[default]
    Properties,
    History,
    Collaborate,
}

/// Secondary right panel tabs (Terrain only - MindSpace moved to left panel)
#[derive(Default, PartialEq, Clone, Copy)]
pub enum SecondaryRightTab {
    #[default]
    Terrain,
}

impl Default for StudioDockState {
    fn default() -> Self {
        // Create 3-column layout: Left panels | Center viewport | Right panels
        // Start with EMPTY tree - center will be clear for 3D viewport
        let tree = DockState::new(vec![]);
        
        Self {
            tree,
            left_tab: LeftTab::default(),
            right_tab: RightTab::default(),
            secondary_right_tab: SecondaryRightTab::default(),
            show_secondary_right: false,
        }
    }
}

/// Tab viewer implementation for rendering tab contents
pub struct StudioTabViewer<'a> {
    pub selection_manager: &'a BevySelectionManager,
    pub asset_state: &'a mut AssetManagerState,
    pub collab_state: &'a mut CollaborationState,
    pub toolbox_state: &'a mut ToolboxState,
    pub explorer_expanded: &'a mut super::ExplorerExpanded,
    pub explorer_state: &'a mut super::ExplorerState,
    pub advanced_search: &'a mut super::AdvancedSearchState,
    pub output_console: &'a mut OutputConsole,
    pub notifications: &'a mut crate::notifications::NotificationManager,
    pub command_history: &'a mut crate::commands::CommandHistory,
    pub world: &'a mut World,
}

impl TabViewer for StudioTabViewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui_dock::egui::WidgetText {
        tab.to_string().into()
    }

    fn ui(&mut self, ui: &mut egui_dock::egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Toolbox => {
                ToolboxPanel::show_content(ui, self.toolbox_state, self.notifications, self.world);
            }
            Tab::Explorer => {
                ExplorerPanel::show_content(ui, self.world, self.selection_manager, self.explorer_expanded, self.explorer_state, self.advanced_search);
            }
            Tab::Properties => {
                // Now handled by side panel with DynamicPropertiesPanel
                ui.label("Properties panel moved to side panel");
            }
            Tab::History => {
                // Now handled by side panel
                ui.label("History panel moved to side panel");
            }
            Tab::Output => {
                OutputPanel::show_content(ui, self.output_console);
            }
            Tab::AssetManager => {
                AssetManagerPanel::show_content(ui, self.asset_state);
            }
            Tab::Collaboration => {
                CollaborationPanel::show_content(ui, self.collab_state);
            }
        }
    }
}

/// Get custom dock style matching our theme
pub fn get_dock_style() -> Style {
    let mut style = Style::from_egui(&egui::Style::default());
    
    // Match our custom theme colors
    style.tab_bar.fill_tab_bar = true;
    style.tab_bar.show_scroll_bar_on_overflow = true;
    
    style
}

// NOTE: The main dock UI is implemented in dock_system() in mod.rs
// This file only contains dock state types and helper functions
