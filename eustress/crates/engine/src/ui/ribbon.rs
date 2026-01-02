use bevy_egui::egui;
use bevy::prelude::*;
use super::{
    StudioState, ViewSelectorState, AssetManagerState, 
    CollaborationState, CommandBarState, Tool, FileEvent, RibbonTab,
    TabEntry, CustomTab, get_builtin_tabs,
};
use eustress_common::terrain::{TerrainConfig, TerrainMode, BrushMode};
use crate::play_mode::PlayModeState;
use crate::ui::context_menu::InsertObjectEvent;
use eustress_common::classes::ClassName;

/// Collected insert events from ribbon buttons (applied after UI rendering)
#[derive(Default)]
pub struct RibbonInsertActions {
    pub inserts: Vec<(ClassName, Option<Entity>)>,
    /// Validation error messages to show to user
    pub errors: Vec<String>,
}

/// Ribbon panel (top toolbar with tools and commands)
pub struct RibbonPanel;

impl RibbonPanel {
    /// Show ribbon with callback-based event collection (avoids ParamSet issues)
    pub fn show_with_callbacks(
        ctx: &egui::Context,
        state: &mut StudioState,
        view_state: &mut ViewSelectorState,
        asset_state: &mut AssetManagerState,
        collab_state: &mut CollaborationState,
        cmd_state: &mut CommandBarState,
        undo_stack: &crate::undo::UndoStack,
        keybindings: &crate::keybindings::KeyBindings,
        // Terrain
        terrain_mode: &TerrainMode,
        has_terrain: bool,
        // Play mode
        play_mode: PlayModeState,
        // Callbacks (mutable refs to collect actions)
        undo_requested: &mut bool,
        redo_requested: &mut bool,
        file_event: &mut Option<FileEvent>,
        menu_action: &mut Option<crate::keybindings::Action>,
        terrain_spawn: &mut Option<TerrainConfig>,
        terrain_toggle: &mut bool,
        terrain_brush: &mut Option<BrushMode>,
        plugin_action: &mut Option<String>,
        // Insert object actions
        insert_actions: &mut RibbonInsertActions,
    ) {
        use crate::keybindings::Action;
        
        // Helper macro to set file event
        macro_rules! set_file_event {
            ($event:expr) => {
                *file_event = Some($event);
            };
        }
        
        // Helper macro to set menu action
        macro_rules! set_menu_action {
            ($action:expr) => {
                *menu_action = Some($action);
            };
        }
        
        egui::TopBottomPanel::top("ribbon")
            .min_height(60.0)
            .show_separator_line(false)
            .frame(egui::Frame::new()
                .fill(egui::Color32::from_rgb(35, 35, 38))
                .inner_margin(egui::Margin { left: 4, right: 4, top: 4, bottom: 4 })
                // Ensure the ribbon paints flush with adjacent panels (prevents a thin gap).
                .outer_margin(egui::Margin::ZERO)
                .stroke(egui::Stroke::NONE)
                .shadow(egui::Shadow::NONE))
            .show(ctx, |ui| {
                // ════════════════════════════════════════════════════════════════
                // ROW 1: Menu Bar
                // ════════════════════════════════════════════════════════════════
                ui.horizontal(|ui| {
                    ui.style_mut().spacing.item_spacing.x = 2.0;
                    
                    // File Menu
                    ui.menu_button("File", |ui| {
                        // New Scene with icon
                        let new_response = ui.add_sized([200.0, 22.0], egui::Button::new(" New Scene          Ctrl+N"));
                        let new_rect = new_response.rect;
                        super::icons::draw_new_scene_icon(ui.painter(), new_rect.left_top() + egui::vec2(2.0, 2.0), 18.0);
                        if new_response.clicked() {
                            set_file_event!(FileEvent::NewScene);
                            ui.close();
                        }
                        
                        // Open Scene with icon
                        let open_response = ui.add_sized([200.0, 22.0], egui::Button::new(" Open Scene...      Ctrl+O"));
                        let open_rect = open_response.rect;
                        super::icons::draw_open_scene_icon(ui.painter(), open_rect.left_top() + egui::vec2(2.0, 2.0), 18.0);
                        if open_response.clicked() {
                            set_file_event!(FileEvent::OpenScene);
                            ui.close();
                        }
                        
                        // Save Scene with icon
                        let save_response = ui.add_sized([200.0, 22.0], egui::Button::new(" Save Scene         Ctrl+S"));
                        let save_rect = save_response.rect;
                        super::icons::draw_save_scene_icon(ui.painter(), save_rect.left_top() + egui::vec2(2.0, 2.0), 18.0);
                        if save_response.clicked() {
                            set_file_event!(FileEvent::SaveScene);
                            ui.close();
                        }
                        
                        // Save Scene As with icon
                        let save_as_response = ui.add_sized([200.0, 22.0], egui::Button::new(" Save As...   Ctrl+Shift+S"));
                        let save_as_rect = save_as_response.rect;
                        super::icons::draw_save_scene_as_icon(ui.painter(), save_as_rect.left_top() + egui::vec2(2.0, 2.0), 18.0);
                        if save_as_response.clicked() {
                            set_file_event!(FileEvent::SaveSceneAs);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("🚀 Publish...        Ctrl+P").clicked() {
                            set_file_event!(FileEvent::Publish);
                            ui.close();
                        }
                        if ui.button("🚀 Publish As...").clicked() {
                            set_file_event!(FileEvent::PublishAs);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("⌨ Keyboard Shortcuts...").clicked() {
                            state.show_keybindings_window = true;
                            ui.close();
                        }
                        if ui.button("@ Soul Settings...").clicked() {
                            state.show_soul_settings_window = true;
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Exit").clicked() {
                            if state.has_unsaved_changes {
                                state.show_exit_confirmation = true;
                            } else {
                                std::process::exit(0);
                            }
                            ui.close();
                        }
                    });
                    
                    // Edit Menu
                    ui.menu_button("Edit", |ui| {
                        let can_undo = undo_stack.can_undo();
                        let can_redo = undo_stack.can_redo();
                        
                        if ui.add_enabled(can_undo, egui::Button::new("Undo                Ctrl+Z")).clicked() {
                            *undo_requested = true;
                            ui.close();
                        }
                        if ui.add_enabled(can_redo, egui::Button::new("Redo                Ctrl+Y")).clicked() {
                            *redo_requested = true;
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Copy                Ctrl+C").clicked() {
                            set_menu_action!(Action::Copy);
                            ui.close();
                        }
                        if ui.button("Paste               Ctrl+V").clicked() {
                            set_menu_action!(Action::Paste);
                            ui.close();
                        }
                        if ui.button("Duplicate           Ctrl+D").clicked() {
                            set_menu_action!(Action::Duplicate);
                            ui.close();
                        }
                        if ui.button("Delete              Del").clicked() {
                            set_menu_action!(Action::Delete);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Select All          Ctrl+A").clicked() {
                            set_menu_action!(Action::SelectAll);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Group               Ctrl+G").clicked() {
                            set_menu_action!(Action::Group);
                            ui.close();
                        }
                        if ui.button("Ungroup             Ctrl+U").clicked() {
                            set_menu_action!(Action::Ungroup);
                            ui.close();
                        }
                    });
                    
                    // View Menu - Panel visibility ONLY (no camera)
                    ui.menu_button("View", |ui| {
                        ui.label("Panels");
                        
                        // Explorer toggle with checkbox
                        if ui.checkbox(&mut state.show_explorer, "Explorer              Ctrl+1").changed() {
                            ui.close();
                        }
                        
                        // Properties toggle with checkbox
                        if ui.checkbox(&mut state.show_properties, "Properties            Ctrl+2").changed() {
                            ui.close();
                        }
                        
                        // Output toggle with checkbox
                        if ui.checkbox(&mut state.show_output, "Output                Ctrl+3").changed() {
                            ui.close();
                        }
                        
                        // Command Bar toggle with checkbox (below Output)
                        if ui.checkbox(&mut cmd_state.show, "Command Bar           Ctrl+K").changed() {
                            ui.close();
                        }
                        
                        // Assets toggle with checkbox
                        if ui.checkbox(&mut asset_state.show, "Assets                Ctrl+4").changed() {
                            ui.close();
                        }
                        
                        // Collaboration toggle with checkbox
                        if ui.checkbox(&mut collab_state.show, "Collaborate           Ctrl+5").changed() {
                            ui.close();
                        }
                    });
                    
                    // Network Menu
                    ui.menu_button("Network", |ui| {
                        ui.label("-- Server --");
                        
                        if ui.button("  Start Local Server      F9").clicked() {
                            // Start embedded server
                            *menu_action = Some(Action::StartServer);
                            ui.close();
                        }
                        
                        if ui.button("  Stop Server").clicked() {
                            *menu_action = Some(Action::StopServer);
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- Forge Cloud --");
                        
                        if ui.button("  Connect to Forge...").clicked() {
                            state.show_forge_connect_window = true;
                            ui.close();
                        }
                        
                        if ui.button("  Allocate Server").clicked() {
                            *plugin_action = Some("network:forge-allocate".to_string());
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- Benchmark --");
                        
                        if ui.button("  Network Panel           Ctrl+N").clicked() {
                            state.show_network_panel = true;
                            ui.close();
                        }
                        
                        if ui.button("  Start Stress Test...").clicked() {
                            state.show_stress_test_window = true;
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- Synthetic Clients --");
                        
                        // Slider for synthetic client count
                        ui.horizontal(|ui| {
                            ui.label("  Clients:");
                            let mut count = state.synthetic_client_count;
                            if ui.add(egui::Slider::new(&mut count, 0..=100).show_value(true)).changed() {
                                state.synthetic_client_count = count;
                                state.synthetic_clients_changed = true;
                            }
                        });
                        
                        if ui.button("  Spawn Clients").clicked() {
                            *plugin_action = Some(format!("network:spawn-clients:{}", state.synthetic_client_count));
                            ui.close();
                        }
                        
                        if ui.button("  Disconnect All").clicked() {
                            *plugin_action = Some("network:disconnect-all".to_string());
                            ui.close();
                        }
                    });
                    
                    // Data Menu (Parameters & Data Sources)
                    ui.menu_button("Data", |ui| {
                        ui.label("-- Configuration --");
                        
                        if ui.button("🌐 Manage Global Sources...").clicked() {
                            state.show_global_sources_window = true;
                            ui.close();
                        }
                        
                        if ui.button("📂 Manage Domains...").clicked() {
                            state.show_domains_window = true;
                            ui.close();
                        }
                        
                        if ui.button("🔑 Global Variables...").clicked() {
                            state.show_global_variables_window = true;
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- Sync --");
                        
                        if ui.button("🔄 Sync Domain to Object Type...").clicked() {
                            state.show_sync_domain_modal = true;
                            state.sync_domain_config.reset();
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- Quick Add Source --");
                        
                        if ui.button("  + HTTP / REST API").clicked() {
                            state.show_global_sources_window = true;
                            state.quick_add_source_type = Some("REST".to_string());
                            ui.close();
                        }
                        if ui.button("  + GraphQL").clicked() {
                            state.show_global_sources_window = true;
                            state.quick_add_source_type = Some("GraphQL".to_string());
                            ui.close();
                        }
                        if ui.button("  + PostgreSQL").clicked() {
                            state.show_global_sources_window = true;
                            state.quick_add_source_type = Some("PostgreSQL".to_string());
                            ui.close();
                        }
                        if ui.button("  + CSV / Excel File").clicked() {
                            state.show_global_sources_window = true;
                            state.quick_add_source_type = Some("CSV".to_string());
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- Cloud Providers --");
                        
                        if ui.button("  + Firebase").clicked() {
                            state.show_global_sources_window = true;
                            state.quick_add_source_type = Some("Firebase".to_string());
                            ui.close();
                        }
                        if ui.button("  + Supabase").clicked() {
                            state.show_global_sources_window = true;
                            state.quick_add_source_type = Some("Supabase".to_string());
                            ui.close();
                        }
                        if ui.button("  + AWS S3").clicked() {
                            state.show_global_sources_window = true;
                            state.quick_add_source_type = Some("S3".to_string());
                            ui.close();
                        }
                        if ui.button("  + Azure Blob").clicked() {
                            state.show_global_sources_window = true;
                            state.quick_add_source_type = Some("AzureBlob".to_string());
                            ui.close();
                        }
                        if ui.button("  + Oracle Cloud").clicked() {
                            state.show_global_sources_window = true;
                            state.quick_add_source_type = Some("Oracle".to_string());
                            ui.close();
                        }
                    });
                    
                    // Terrain Menu
                    ui.menu_button("Terrain", |ui| {
                        // Generate section
                        ui.label("-- Generate --");
                        if ui.button("  Small (128m)").clicked() {
                            *terrain_spawn = Some(TerrainConfig::small());
                            ui.close();
                        }
                        if ui.button("  Medium (512m)").clicked() {
                            *terrain_spawn = Some(TerrainConfig::default());
                            ui.close();
                        }
                        if ui.button("  Large (2km)").clicked() {
                            *terrain_spawn = Some(TerrainConfig::large());
                            ui.close();
                        }
                        
                        ui.separator();
                        
                        // Edit mode toggle
                        let edit_label = if *terrain_mode == TerrainMode::Editor {
                            "[x] Edit Mode (T)"
                        } else {
                            "[ ] Edit Mode (T)"
                        };
                        if ui.add_enabled(has_terrain, egui::Button::new(edit_label)).clicked() {
                            *terrain_toggle = true;
                            ui.close();
                        }
                        
                        ui.separator();
                        
                        // Brush tools (only when terrain exists)
                        ui.label("-- Brush Tools --");
                        ui.add_enabled_ui(has_terrain && *terrain_mode == TerrainMode::Editor, |ui| {
                            if ui.button("  ^ Raise (1)").clicked() {
                                *terrain_brush = Some(BrushMode::Raise);
                                ui.close();
                            }
                            if ui.button("  v Lower (2)").clicked() {
                                *terrain_brush = Some(BrushMode::Lower);
                                ui.close();
                            }
                            if ui.button("  ~ Smooth (3)").clicked() {
                                *terrain_brush = Some(BrushMode::Smooth);
                                ui.close();
                            }
                            if ui.button("  = Flatten (4)").clicked() {
                                *terrain_brush = Some(BrushMode::Flatten);
                                ui.close();
                            }
                            if ui.button("  # Paint (5)").clicked() {
                                *terrain_brush = Some(BrushMode::PaintTexture);
                                ui.close();
                            }
                        });
                        
                        ui.separator();
                        
                        // Import/Export
                        ui.label("-- Assets --");
                        if ui.button("  Import Heightmap...").clicked() {
                            // Handled by terrain_plugin UI
                            ui.close();
                        }
                        if ui.add_enabled(has_terrain, egui::Button::new("  Export Heightmap...")).clicked() {
                            // Handled by terrain_plugin UI
                            ui.close();
                        }
                    });
                    
                    // Plugin Menu (right of Terrain)
                    ui.menu_button("Plugin", |ui| {
                        ui.label("-- Plugins --");
                        
                        if ui.button("📂 View Plugins Folder").clicked() {
                            // Open the workspace plugins directory (for Rune scripts)
                            let plugins_dir = std::path::PathBuf::from("plugins");
                            let _ = std::fs::create_dir_all(&plugins_dir);
                            
                            // Get absolute path
                            let abs_path = std::env::current_dir()
                                .map(|cwd| cwd.join(&plugins_dir))
                                .unwrap_or(plugins_dir);
                            
                            #[cfg(target_os = "windows")]
                            { let _ = std::process::Command::new("explorer").arg(&abs_path).spawn(); }
                            
                            #[cfg(target_os = "macos")]
                            { let _ = std::process::Command::new("open").arg(&abs_path).spawn(); }
                            
                            #[cfg(target_os = "linux")]
                            { let _ = std::process::Command::new("xdg-open").arg(&abs_path).spawn(); }
                            
                            ui.close();
                        }
                        
                        if ui.button("🔄 Reload Plugins").clicked() {
                            *plugin_action = Some("system:reload-plugins".to_string());
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- Installed --");
                        
                        // MindSpace plugin entry
                        if ui.button("MindSpace").clicked() {
                            *plugin_action = Some("mindspace:toggle-panel".to_string());
                            ui.close();
                        }
                        
                        ui.separator();
                        
                        if ui.button("🛒 Plugin Marketplace...").clicked() {
                            // TODO: Open marketplace
                            ui.close();
                        }
                    });
                    
                    // MindSpace Menu (from MindSpace v2 plugin)
                    ui.menu_button("MindSpace", |ui| {
                        ui.label("-- Panel --");
                        if ui.button("Toggle Panel          Ctrl+M").clicked() {
                            *plugin_action = Some("mindspace:toggle-panel".to_string());
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- Labels --");
                        
                        if ui.button("➕ Add Label            Ctrl+L").clicked() {
                            *plugin_action = Some("mindspace:add-label".to_string());
                            ui.close();
                        }
                        
                        if ui.button("➖ Remove Label").clicked() {
                            *plugin_action = Some("mindspace:remove-label".to_string());
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- Connections --");
                        
                        if ui.button("🔗 Connect Nodes        Ctrl+K").clicked() {
                            *plugin_action = Some("mindspace:connect".to_string());
                            ui.close();
                        }
                        
                        if ui.button("🔍 Search               Ctrl+F").clicked() {
                            *plugin_action = Some("mindspace:search".to_string());
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- File --");
                        
                        if ui.button("📄 New Mind Map").clicked() {
                            *plugin_action = Some("mindspace:new".to_string());
                            ui.close();
                        }
                        
                        if ui.button("📤 Export...").clicked() {
                            *plugin_action = Some("mindspace:export".to_string());
                            ui.close();
                        }
                        
                        if ui.button("📥 Import...").clicked() {
                            *plugin_action = Some("mindspace:import".to_string());
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("-- AI Tools --");
                        
                        if ui.button("🤖 AI Suggestions").clicked() {
                            *plugin_action = Some("mindspace:ai-suggest".to_string());
                            ui.close();
                        }
                        
                        if ui.button("📐 Auto Layout").clicked() {
                            *plugin_action = Some("mindspace:ai-layout".to_string());
                            ui.close();
                        }
                        
                        if ui.button("📝 Generate Summary").clicked() {
                            *plugin_action = Some("mindspace:ai-summarize".to_string());
                            ui.close();
                        }
                    });
                    
                    // Help Menu
                    ui.menu_button("Help", |ui| {
                        ui.label(egui::RichText::new("Open in Browser Tab").small().color(egui::Color32::GRAY));
                        if ui.button("Documentation").clicked() {
                            state.browser_open_request = Some(("https://docs.eustress.dev".to_string(), "Documentation".to_string()));
                            ui.close();
                        }
                        if ui.button("API Reference").clicked() {
                            state.browser_open_request = Some(("https://docs.eustress.dev/api".to_string(), "API Reference".to_string()));
                            ui.close();
                        }
                        if ui.button("Tutorials").clicked() {
                            state.browser_open_request = Some(("https://docs.eustress.dev/tutorials".to_string(), "Tutorials".to_string()));
                            ui.close();
                        }
                        if ui.button("Soul Scripting Guide").clicked() {
                            state.browser_open_request = Some(("https://docs.eustress.dev/soul".to_string(), "Soul Guide".to_string()));
                            ui.close();
                        }
                        ui.separator();
                        ui.label(egui::RichText::new("Open in System Browser").small().color(egui::Color32::GRAY));
                        if ui.button("Website").clicked() {
                            let _ = open::that("https://eustress.dev");
                            ui.close();
                        }
                        if ui.button("Discord Community").clicked() {
                            let _ = open::that("https://discord.gg/eustress");
                            ui.close();
                        }
                        if ui.button("GitHub").clicked() {
                            let _ = open::that("https://github.com/eustressengine/eustress");
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("About Eustress Engine").clicked() {
                            // TODO: About dialog
                            ui.close();
                        }
                    });
                }); // End Row 1
                
                // ════════════════════════════════════════════════════════════════
                // ROW 2: Ribbon Tabs
                // ════════════════════════════════════════════════════════════════
                ui.horizontal(|ui| {
                    ui.style_mut().spacing.item_spacing.x = 2.0;
                    
                    let tab_height = 28.0;
                    let tab_width = 75.0;
                    
                    // Built-in tab colors
                    let builtin_colors: std::collections::HashMap<&str, [u8; 3]> = [
                        ("home", [70, 130, 180]),    // Steel blue
                        ("model", [100, 149, 237]),  // Cornflower blue
                        ("ui", [147, 112, 219]),     // Medium purple
                        ("terrain", [60, 179, 113]), // Medium sea green
                        ("test", [255, 140, 0]),     // Dark orange
                    ].into_iter().collect();
                    
                    // Render tabs from visible_tabs list
                    for (idx, tab_entry) in state.visible_tabs.clone().iter().enumerate() {
                        let (label, icon, tab, base_color) = match tab_entry {
                            TabEntry::BuiltIn(ribbon_tab) => {
                                let builtin = get_builtin_tabs();
                                let info = builtin.iter().find(|b| &b.tab == ribbon_tab);
                                let name = info.map(|i| i.name).unwrap_or("Tab");
                                let icon = info.map(|i| i.icon).unwrap_or("");
                                let id = info.map(|i| i.id).unwrap_or("");
                                let color = builtin_colors.get(id).copied().unwrap_or([80, 80, 100]);
                                (name.to_string(), icon.to_string(), *ribbon_tab, color)
                            }
                            TabEntry::Plugin { name, .. } => {
                                (name.clone(), "".to_string(), RibbonTab::Custom(idx), [100, 100, 150])
                            }
                            TabEntry::Custom(custom_idx) => {
                                let custom = state.custom_tabs.get(*custom_idx);
                                let name = custom.map(|c| c.name.as_str()).unwrap_or("Custom");
                                let icon = custom.map(|c| c.icon.as_str()).unwrap_or("");
                                let color = custom.map(|c| c.color).unwrap_or([80, 80, 100]);
                                (name.to_string(), icon.to_string(), RibbonTab::Custom(*custom_idx), color)
                            }
                        };
                        
                        let is_selected = state.ribbon_tab == tab;
                        
                        // Colorize tabs - brighter when selected, dimmer when not
                        let fill = if is_selected {
                            egui::Color32::from_rgb(base_color[0], base_color[1], base_color[2])
                        } else {
                            // Dimmed version
                            egui::Color32::from_rgb(
                                (base_color[0] as f32 * 0.5) as u8,
                                (base_color[1] as f32 * 0.5) as u8,
                                (base_color[2] as f32 * 0.5) as u8,
                            )
                        };
                        
                        let display_text = if icon.is_empty() {
                            label.clone()
                        } else {
                            format!("{} {}", icon, label)
                        };
                        
                        let btn = egui::Button::new(
                            egui::RichText::new(&display_text).size(12.0).strong().color(
                                if is_selected { egui::Color32::WHITE } else { egui::Color32::from_rgb(200, 200, 200) }
                            )
                        )
                        .fill(fill)
                        .min_size(egui::vec2(tab_width, tab_height))
                        .rounding(6.0);
                        
                        let response = ui.add(btn);
                        
                        // Right-click context menu for tab management
                        response.context_menu(|ui| {
                            if ui.button("📋 Reorder Tabs...").clicked() {
                                state.tab_manager.show_reorder_modal = true;
                                ui.close();
                            }
                            ui.separator();
                            if matches!(tab_entry, TabEntry::Custom(_)) {
                                if ui.button("✏️ Edit Tab...").clicked() {
                                    if let TabEntry::Custom(custom_idx) = tab_entry {
                                        state.tab_manager.editing_custom_tab_index = Some(*custom_idx);
                                        state.tab_manager.editing_custom_tab = state.custom_tabs.get(*custom_idx).cloned().unwrap_or_default();
                                        state.tab_manager.show_edit_custom_tab_modal = true;
                                    }
                                    ui.close();
                                }
                                if ui.button("🗑️ Remove Tab").clicked() {
                                    // Remove from visible_tabs
                                    state.visible_tabs.retain(|t| t != tab_entry);
                                    ui.close();
                                }
                            } else {
                                if ui.button("👁️ Hide Tab").clicked() {
                                    state.visible_tabs.retain(|t| t != tab_entry);
                                    ui.close();
                                }
                            }
                        });
                        
                        if response.clicked() {
                            state.ribbon_tab = tab;
                        }
                    }
                    
                    ui.add_space(4.0);
                    
                    // Plus button to add new tab
                    let plus_btn = egui::Button::new(
                        egui::RichText::new("+").size(16.0).strong().color(egui::Color32::WHITE)
                    )
                    .fill(egui::Color32::from_rgb(60, 60, 70))
                    .min_size(egui::vec2(28.0, tab_height))
                    .rounding(6.0);
                    
                    if ui.add(plus_btn).on_hover_text("Add or manage tabs").clicked() {
                        state.tab_manager.show_add_tab_modal = true;
                        state.tab_manager.add_tab_search.clear();
                    }
                    
                    ui.add_space(10.0);
                    
                    // Play controls always visible on right side
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let play_btn_size = egui::vec2(75.0, 28.0); // Match tab size
                        
                        let is_playing = play_mode == PlayModeState::Playing;
                        let is_paused = play_mode == PlayModeState::Paused;
                        let is_running = is_playing || is_paused;
                        
                        // Stop button - red, only bright when running
                        let stop_color = if is_running {
                            egui::Color32::from_rgb(200, 60, 60)
                        } else {
                            egui::Color32::from_rgb(100, 50, 50)
                        };
                        let stop_btn = egui::Button::new(egui::RichText::new("⏹ Stop").size(11.0))
                            .fill(stop_color)
                            .min_size(play_btn_size);
                        if ui.add(stop_btn).on_hover_text("Stop (F8)").clicked() {
                            state.stop_requested = true;
                        }
                        
                        // Pause button
                        let pause_color = if is_paused {
                            egui::Color32::from_rgb(220, 180, 50)
                        } else {
                            egui::Color32::from_rgb(120, 100, 40)
                        };
                        let pause_btn = egui::Button::new(egui::RichText::new("⏸ Pause").size(11.0))
                            .fill(pause_color)
                            .min_size(play_btn_size);
                        if ui.add(pause_btn).on_hover_text("Pause (F6)").clicked() {
                            state.pause_requested = true;
                        }
                        
                        // Play button
                        let play_color = if is_playing {
                            egui::Color32::from_rgb(50, 150, 220)
                        } else {
                            egui::Color32::from_rgb(40, 80, 120)
                        };
                        let play_btn = egui::Button::new(egui::RichText::new("▶ Play").size(11.0))
                            .fill(play_color)
                            .min_size(play_btn_size);
                        if ui.add(play_btn).on_hover_text("Play (F5)").clicked() {
                            state.play_with_character_requested = true;
                        }
                        
                        // Run button
                        let run_color = if is_playing && !is_paused {
                            egui::Color32::from_rgb(50, 180, 50)
                        } else {
                            egui::Color32::from_rgb(40, 100, 40)
                        };
                        let run_btn = egui::Button::new(egui::RichText::new("▶ Run").size(11.0))
                            .fill(run_color)
                            .min_size(play_btn_size);
                        if ui.add(run_btn).on_hover_text("Run (F7)").clicked() {
                            state.play_solo_requested = true;
                        }
                        
                        // Timer when running
                        if is_running {
                            let play_time = ctx.data(|d| {
                                d.get_temp::<f32>(egui::Id::new("runtime_play_time")).unwrap_or(0.0)
                            });
                            let minutes = (play_time / 60.0).floor() as u32;
                            let seconds = (play_time % 60.0).floor() as u32;
                            let time_str = format!("{:02}:{:02}", minutes, seconds);
                            let time_color = if is_paused {
                                egui::Color32::from_rgb(220, 180, 50)
                            } else {
                                egui::Color32::from_rgb(100, 255, 100)
                            };
                            ui.label(egui::RichText::new(format!("⏱ {}", time_str))
                                .size(11.0)
                                .color(time_color)
                                .monospace());
                        }
                    });
                }); // End Row 2 (Tabs)
                
                // ════════════════════════════════════════════════════════════════
                // ROW 3: Tab Content (changes based on selected tab)
                // ════════════════════════════════════════════════════════════════
                ui.add_space(2.0);
                
                // Tab content area with consistent styling
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(45, 45, 48))
                    .inner_margin(egui::Margin { left: 8, right: 8, top: 4, bottom: 4 })
                    .rounding(2.0)
                    .show(ui, |ui| {
                        ui.set_min_height(32.0);
                        
                        match state.ribbon_tab {
                            RibbonTab::Home => {
                                Self::render_home_tab(ui, state, view_state, keybindings, menu_action);
                            }
                            RibbonTab::Model => {
                                Self::render_model_tab(ui, state, menu_action, insert_actions);
                            }
                            RibbonTab::UI => {
                                Self::render_ui_tab(ui, state, insert_actions);
                            }
                            RibbonTab::Terrain => {
                                Self::render_terrain_tab(ui, terrain_mode, has_terrain, terrain_spawn, terrain_toggle, terrain_brush);
                            }
                            RibbonTab::Test => {
                                Self::render_test_tab(ui, state, plugin_action, menu_action);
                            }
                            RibbonTab::MindSpace => {
                                Self::render_mindspace_tab(ui, plugin_action);
                            }
                            RibbonTab::Plugin(_idx) => {
                                // Plugin tabs are rendered via TabRegistry - handled separately
                                // For now, show a placeholder
                                ui.label("Plugin tab content rendered via TabRegistry");
                            }
                            RibbonTab::Custom(idx) => {
                                Self::render_custom_tab(ui, state, idx, insert_actions);
                            }
                        }
                    });
            });
    }
    
    // ════════════════════════════════════════════════════════════════════════════
    // HOME TAB - Camera, Transform Tools
    // ════════════════════════════════════════════════════════════════════════════
    fn render_home_tab(
        ui: &mut egui::Ui,
        state: &mut StudioState,
        view_state: &mut ViewSelectorState,
        keybindings: &crate::keybindings::KeyBindings,
        menu_action: &mut Option<crate::keybindings::Action>,
    ) {
        use crate::keybindings::Action;
        use egui_material_icons::icons::*;
        
        ui.horizontal(|ui| {
            // === CAMERA SECTION ===
            Self::render_section(ui, "Camera", |ui| {
                ui.horizontal(|ui| {
                    // Camera dropdown
                    ui.menu_button(format!("{} View", ICON_VIDEOCAM), |ui| {
                        if ui.button(format!("{} Focus Selection       F", ICON_CENTER_FOCUS_STRONG)).clicked() {
                            *menu_action = Some(Action::FocusSelection);
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("Projection");
                        
                        let is_perspective = matches!(view_state.current_mode, super::ViewMode::Perspective);
                        if ui.radio(is_perspective, "Perspective").clicked() {
                            view_state.current_mode = super::ViewMode::Perspective;
                            ui.close();
                        }
                        
                        let is_ortho = matches!(view_state.current_mode, super::ViewMode::Orthographic);
                        if ui.radio(is_ortho, "Orthographic").clicked() {
                            view_state.current_mode = super::ViewMode::Orthographic;
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("View Direction");
                        
                        let is_top = matches!(view_state.current_mode, super::ViewMode::Top);
                        if ui.radio(is_top, format!("{} Top", ICON_ARROW_DOWNWARD)).clicked() {
                            view_state.current_mode = super::ViewMode::Top;
                            ui.close();
                        }
                        
                        let is_front = matches!(view_state.current_mode, super::ViewMode::Front);
                        if ui.radio(is_front, format!("{} Front", ICON_ARROW_BACK)).clicked() {
                            view_state.current_mode = super::ViewMode::Front;
                            ui.close();
                        }
                        
                        let is_side = matches!(view_state.current_mode, super::ViewMode::Side);
                        if ui.radio(is_side, format!("{} Side", ICON_ARROW_FORWARD)).clicked() {
                            view_state.current_mode = super::ViewMode::Side;
                            ui.close();
                        }
                        
                        ui.separator();
                        ui.label("Overlays");
                        
                        ui.checkbox(&mut view_state.show_grid, format!("{} Show Grid", ICON_GRID_ON));
                        ui.checkbox(&mut view_state.show_wireframe, format!("{} Wireframe", ICON_GRID_4X4));
                        ui.checkbox(&mut view_state.show_gizmos, format!("{} Show Gizmos", ICON_3D_ROTATION));
                    });
                });
            });
            
            ui.separator();
            
            // === TOOLS SECTION ===
            Self::render_section(ui, "Tools", |ui| {
                ui.horizontal(|ui| {
                    let tool_btn_size = egui::vec2(65.0, 24.0);
                    let selected_color = egui::Color32::from_rgb(50, 100, 150);
                    let normal_color = egui::Color32::from_rgb(55, 55, 60);
                    
                    // Select Tool
                    let select_color = if state.current_tool == Tool::Select { selected_color } else { normal_color };
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Select", ICON_NEAR_ME)).size(11.0))
                        .fill(select_color)
                        .min_size(tool_btn_size))
                        .on_hover_text(format!("Select ({})", keybindings.get_string(Action::SelectTool)))
                        .clicked() {
                        state.current_tool = Tool::Select;
                    }
                    
                    // Move Tool
                    let move_color = if state.current_tool == Tool::Move { selected_color } else { normal_color };
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Move", ICON_OPEN_WITH)).size(11.0))
                        .fill(move_color)
                        .min_size(tool_btn_size))
                        .on_hover_text(format!("Move ({})", keybindings.get_string(Action::MoveTool)))
                        .clicked() {
                        state.current_tool = Tool::Move;
                    }
                    
                    // Scale Tool
                    let scale_color = if state.current_tool == Tool::Scale { selected_color } else { normal_color };
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Scale", ICON_ASPECT_RATIO)).size(11.0))
                        .fill(scale_color)
                        .min_size(tool_btn_size))
                        .on_hover_text(format!("Scale ({})", keybindings.get_string(Action::ScaleTool)))
                        .clicked() {
                        state.current_tool = Tool::Scale;
                    }
                    
                    // Rotate Tool
                    let rotate_color = if state.current_tool == Tool::Rotate { selected_color } else { normal_color };
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Rotate", ICON_ROTATE_RIGHT)).size(11.0))
                        .fill(rotate_color)
                        .min_size(tool_btn_size))
                        .on_hover_text(format!("Rotate ({})", keybindings.get_string(Action::RotateTool)))
                        .clicked() {
                        state.current_tool = Tool::Rotate;
                    }
                });
            });
            
            ui.separator();
            
            // === EDIT SECTION ===
            Self::render_section(ui, "Edit", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(65.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Group", ICON_WORKSPACES)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Group selected objects (Ctrl+G)")
                        .clicked() {
                        *menu_action = Some(Action::Group);
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Ungroup", ICON_WORKSPACES_OUTLINE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Ungroup selected (Ctrl+Shift+G)")
                        .clicked() {
                        *menu_action = Some(Action::Ungroup);
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Lock", ICON_LOCK)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Lock selected parts")
                        .clicked() {
                        *menu_action = Some(Action::LockSelection);
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Unlock", ICON_LOCK_OPEN)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Unlock selected parts")
                        .clicked() {
                        *menu_action = Some(Action::UnlockSelection);
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Anchor", ICON_ANCHOR)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Toggle anchor on selected parts")
                        .clicked() {
                        *menu_action = Some(Action::ToggleAnchor);
                    }
                });
            });
            
            ui.separator();
            
            // === UTILITY SECTION ===
            Self::render_section(ui, "Utility", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(65.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Find", ICON_SEARCH)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Find in scene (Ctrl+F)")
                        .clicked() {
                        state.show_find_dialog = true;
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Settings", ICON_SETTINGS)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Editor settings")
                        .clicked() {
                        state.show_settings_window = true;
                    }
                });
            });
        });
    }
    
    // ════════════════════════════════════════════════════════════════════════════
    // MODEL TAB - Parts, Models, Constraints, Effects
    // ════════════════════════════════════════════════════════════════════════════
    fn render_model_tab(
        ui: &mut egui::Ui,
        _state: &mut StudioState,
        menu_action: &mut Option<crate::keybindings::Action>,
        insert_actions: &mut RibbonInsertActions,
    ) {
        use egui_material_icons::icons::*;
        use crate::keybindings::Action;
        
        ui.horizontal(|ui| {
            // === PARTS SECTION ===
            Self::render_section(ui, "Parts", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Part", ICON_VIEW_IN_AR)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Part")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::Part, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Mesh", ICON_DEPLOYED_CODE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert MeshPart")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::MeshPart, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Union", ICON_JOIN_FULL)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Union Operation")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::UnionOperation, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Seat", ICON_CHAIR)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Seat")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::Seat, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Vehicle", ICON_DIRECTIONS_CAR)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Vehicle Seat")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::VehicleSeat, None));
                    }
                });
            });
            
            ui.separator();
            
            // === STRUCTURE SECTION ===
            Self::render_section(ui, "Structure", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Model", ICON_ACCOUNT_TREE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Model container")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::Model, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Folder", ICON_FOLDER)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Folder")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::Folder, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Spawn", ICON_PERSON_PIN_CIRCLE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert SpawnLocation")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::SpawnLocation, None));
                    }
                });
            });
            
            ui.separator();
            
            // === CONSTRAINTS SECTION ===
            // Note: Attachment must come first - constraints require attachments to function
            Self::render_section(ui, "Constraints", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Attach", ICON_PUSH_PIN)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Attachment (add to Part first)")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::Attachment, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Weld", ICON_LINK)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Weld Constraint (requires Attachments)")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::WeldConstraint, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Motor", ICON_SETTINGS)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Motor6D (requires Attachments)")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::Motor6D, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Beam", ICON_LINEAR_SCALE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Beam (requires 2 Attachments)")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::Beam, None));
                    }
                });
            });
            
            ui.separator();
            
            // === EFFECTS SECTION ===
            Self::render_section(ui, "Effects", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(75.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Particles", ICON_BUBBLE_CHART)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Particle Emitter (add to Part or Attachment)")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::ParticleEmitter, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} PointLight", ICON_LIGHTBULB)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert PointLight (add to Part or Attachment)")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::PointLight, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Spotlight", ICON_HIGHLIGHT)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Spotlight (add to Part or Attachment)")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::SpotLight, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Sound", ICON_VOLUME_UP)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Insert Sound (add to Part or Attachment)")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::Sound, None));
                    }
                });
            });
            
            ui.separator();
            
            // === CSG SECTION ===
            Self::render_section(ui, "CSG", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Negate", ICON_JOIN_INNER)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Negate selected part (CSG subtract)")
                        .clicked() {
                        *menu_action = Some(Action::CSGNegate);
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Intersect", ICON_FILTER_CENTER_FOCUS)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Intersect selected parts (CSG)")
                        .clicked() {
                        *menu_action = Some(Action::CSGIntersect);
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Separate", ICON_CALL_SPLIT)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Separate union into parts")
                        .clicked() {
                        *menu_action = Some(Action::CSGSeparate);
                    }
                });
            });
            
            ui.separator();
            
            // === IMPORT/EXPORT SECTION ===
            Self::render_section(ui, "Assets", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Import", ICON_UPLOAD_FILE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Import model file (OBJ, FBX, GLTF)")
                        .clicked() {
                        // TODO: Open import dialog
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Export", ICON_DOWNLOAD)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Export selected as model file")
                        .clicked() {
                        // TODO: Open export dialog
                    }
                });
            });
        });
    }
    
    // ════════════════════════════════════════════════════════════════════════════
    // UI TAB - GUI Elements
    // ════════════════════════════════════════════════════════════════════════════
    fn render_ui_tab(
        ui: &mut egui::Ui,
        _state: &mut StudioState,
        insert_actions: &mut RibbonInsertActions,
    ) {
        use egui_material_icons::icons::*;
        
        ui.horizontal(|ui| {
            // === CREATE SECTION ===
            Self::render_section(ui, "Containers", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(80.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Screen", ICON_DESKTOP_WINDOWS)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Create a 2D screen overlay GUI")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::ScreenGui, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Billboard", ICON_SIGNPOST)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Create a 3D world-space billboard GUI")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::BillboardGui, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Surface", ICON_CROP_SQUARE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Create a GUI on a part's surface")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::SurfaceGui, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Frame", ICON_CHECK_BOX_OUTLINE_BLANK)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Create a container frame")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::Frame, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Scroll", ICON_VIEW_DAY)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Create a scrolling frame")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::ScrollingFrame, None));
                    }
                });
            });
            
            ui.separator();
            
            // === DISPLAY SECTION ===
            Self::render_section(ui, "Display", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(75.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Text", ICON_TEXT_FIELDS)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Create a text label")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::TextLabel, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Image", ICON_IMAGE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Create an image label")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::ImageLabel, None));
                    }
                    
                });
            });
            
            ui.separator();
            
            // === INTERACTIVE SECTION ===
            Self::render_section(ui, "Interactive", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0); // Reduced from 80 to fit all 7 buttons
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} TextBtn", ICON_SMART_BUTTON)).size(10.0)).min_size(btn_size))
                        .on_hover_text("Create a clickable text button")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::TextButton, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} ImageBtn", ICON_TOUCH_APP)).size(10.0)).min_size(btn_size))
                        .on_hover_text("Create a clickable image button")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::ImageButton, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} TextBox", ICON_EDIT)).size(10.0)).min_size(btn_size))
                        .on_hover_text("Create a text input box")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::TextBox, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Viewport", ICON_VRPANO)).size(10.0)).min_size(btn_size))
                        .on_hover_text("Create a 3D viewport frame")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::ViewportFrame, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Video", ICON_PLAY_CIRCLE)).size(10.0)).min_size(btn_size))
                        .on_hover_text("Create a video player frame")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::VideoFrame, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Document", ICON_DESCRIPTION)).size(10.0)).min_size(btn_size))
                        .on_hover_text("Create a document viewer")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::DocumentFrame, None));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} WebFrame", ICON_WEB)).size(10.0)).min_size(btn_size))
                        .on_hover_text("Create an embedded web browser frame")
                        .clicked() {
                        insert_actions.inserts.push((ClassName::WebFrame, None));
                    }
                });
            });
            
            ui.separator();
            
            
            // === LAYOUT SECTION ===
            Self::render_section(ui, "Layout", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(55.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{}", ICON_ALIGN_HORIZONTAL_LEFT)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Align left")
                        .clicked() {
                        // TODO: Align left
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{}", ICON_ALIGN_HORIZONTAL_CENTER)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Align center")
                        .clicked() {
                        // TODO: Align center
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{}", ICON_ALIGN_HORIZONTAL_RIGHT)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Align right")
                        .clicked() {
                        // TODO: Align right
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{}", ICON_VIEW_COLUMN)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Distribute evenly")
                        .clicked() {
                        // TODO: Distribute
                    }
                });
            });
        });
    }
    
    // ════════════════════════════════════════════════════════════════════════════
    // TERRAIN TAB - Terrain Tools
    // ════════════════════════════════════════════════════════════════════════════
    fn render_terrain_tab(
        ui: &mut egui::Ui,
        terrain_mode: &TerrainMode,
        has_terrain: bool,
        terrain_spawn: &mut Option<TerrainConfig>,
        terrain_toggle: &mut bool,
        terrain_brush: &mut Option<BrushMode>,
    ) {
        use egui_material_icons::icons::*;
        
        ui.horizontal(|ui| {
            // === GENERATE SECTION ===
            Self::render_section(ui, "Generate", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(65.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Small", ICON_LANDSCAPE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Generate small terrain (128m)")
                        .clicked() {
                        *terrain_spawn = Some(TerrainConfig::small());
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Medium", ICON_TERRAIN)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Generate medium terrain (512m)")
                        .clicked() {
                        *terrain_spawn = Some(TerrainConfig::default());
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Large", ICON_PUBLIC)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Generate large terrain (2km)")
                        .clicked() {
                        *terrain_spawn = Some(TerrainConfig::large());
                    }
                });
            });
            
            ui.separator();
            
            // === EDIT SECTION ===
            Self::render_section(ui, "Edit", |ui| {
                ui.horizontal(|ui| {
                    let is_editing = *terrain_mode == TerrainMode::Editor;
                    let edit_color = if is_editing {
                        egui::Color32::from_rgb(50, 150, 50)
                    } else {
                        egui::Color32::from_rgb(55, 55, 60)
                    };
                    
                    let edit_icon = if is_editing { ICON_CHECK } else { ICON_EDIT };
                    if ui.add_enabled(has_terrain, 
                        egui::Button::new(egui::RichText::new(format!("{} Edit Mode", edit_icon)).size(11.0))
                            .fill(edit_color)
                            .min_size(egui::vec2(80.0, 24.0)))
                        .on_hover_text("Toggle terrain edit mode (T)")
                        .clicked() {
                        *terrain_toggle = true;
                    }
                });
            });
            
            ui.separator();
            
            // === BRUSHES SECTION ===
            Self::render_section(ui, "Brushes", |ui| {
                ui.add_enabled_ui(has_terrain && *terrain_mode == TerrainMode::Editor, |ui| {
                    ui.horizontal(|ui| {
                        let btn_size = egui::vec2(60.0, 24.0);
                        
                        if ui.add(egui::Button::new(egui::RichText::new(format!("{} Raise", ICON_ARROW_UPWARD)).size(11.0)).min_size(btn_size))
                            .on_hover_text("Raise terrain (1)")
                            .clicked() {
                            *terrain_brush = Some(BrushMode::Raise);
                        }
                        
                        if ui.add(egui::Button::new(egui::RichText::new(format!("{} Lower", ICON_ARROW_DOWNWARD)).size(11.0)).min_size(btn_size))
                            .on_hover_text("Lower terrain (2)")
                            .clicked() {
                            *terrain_brush = Some(BrushMode::Lower);
                        }
                        
                        if ui.add(egui::Button::new(egui::RichText::new(format!("{} Smooth", ICON_BLUR_ON)).size(11.0)).min_size(btn_size))
                            .on_hover_text("Smooth terrain (3)")
                            .clicked() {
                            *terrain_brush = Some(BrushMode::Smooth);
                        }
                        
                        if ui.add(egui::Button::new(egui::RichText::new(format!("{} Flatten", ICON_HORIZONTAL_RULE)).size(11.0)).min_size(btn_size))
                            .on_hover_text("Flatten terrain (4)")
                            .clicked() {
                            *terrain_brush = Some(BrushMode::Flatten);
                        }
                        
                        if ui.add(egui::Button::new(egui::RichText::new(format!("{} Paint", ICON_BRUSH)).size(11.0)).min_size(btn_size))
                            .on_hover_text("Paint texture (5)")
                            .clicked() {
                            *terrain_brush = Some(BrushMode::PaintTexture);
                        }
                        
                        if ui.add(egui::Button::new(egui::RichText::new(format!("{} Region", ICON_CROP_FREE)).size(11.0)).min_size(btn_size))
                            .on_hover_text("Select region")
                            .clicked() {
                            *terrain_brush = Some(BrushMode::Region);
                        }
                        
                        if ui.add(egui::Button::new(egui::RichText::new(format!("{} Fill", ICON_FORMAT_COLOR_FILL)).size(11.0)).min_size(btn_size))
                            .on_hover_text("Fill region with material")
                            .clicked() {
                            *terrain_brush = Some(BrushMode::Fill);
                        }
                    });
                });
            });
            
            ui.separator();
            
            // === WATER SECTION ===
            Self::render_section(ui, "Water", |ui| {
                ui.add_enabled_ui(has_terrain, |ui| {
                    ui.horizontal(|ui| {
                        let btn_size = egui::vec2(70.0, 24.0);
                        
                        if ui.add(egui::Button::new(egui::RichText::new(format!("{} Sea Level", ICON_WATER)).size(11.0)).min_size(btn_size))
                            .on_hover_text("Set water/sea level")
                            .clicked() {
                            // TODO: Open sea level dialog
                        }
                    });
                });
            });
            
            ui.separator();
            
            // === ASSETS SECTION ===
            Self::render_section(ui, "Assets", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Import", ICON_TERRAIN)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Import heightmap")
                        .clicked() {
                        // TODO: Open import heightmap dialog
                    }
                    
                    if ui.add_enabled(has_terrain, egui::Button::new(egui::RichText::new(format!("{} Clear", ICON_DELETE_FOREVER)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Clear all terrain")
                        .clicked() {
                        // TODO: Clear terrain with confirmation
                    }
                });
            });
        });
    }
    
    // ════════════════════════════════════════════════════════════════════════════
    // TEST TAB - Play Testing & Network
    // ════════════════════════════════════════════════════════════════════════════
    fn render_test_tab(
        ui: &mut egui::Ui,
        state: &mut StudioState,
        plugin_action: &mut Option<String>,
        menu_action: &mut Option<crate::keybindings::Action>,
    ) {
        use crate::keybindings::Action;
        use egui_material_icons::icons::*;
        
        ui.horizontal(|ui| {
            // === SERVER SECTION ===
            Self::render_section(ui, "Server", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Start", ICON_PLAY_ARROW)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Start local server (F9)")
                        .clicked() {
                        *menu_action = Some(Action::StartServer);
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Stop", ICON_STOP)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Stop server")
                        .clicked() {
                        *menu_action = Some(Action::StopServer);
                    }
                });
            });
            
            ui.separator();
            
            // === CLIENTS SECTION ===
            Self::render_section(ui, "Clients", |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{}", ICON_PEOPLE));
                    let mut count = state.synthetic_client_count;
                    if ui.add(egui::DragValue::new(&mut count).range(0..=100)).changed() {
                        state.synthetic_client_count = count;
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Spawn", ICON_PERSON_ADD)).size(11.0))).on_hover_text("Spawn synthetic clients").clicked() {
                        *plugin_action = Some(format!("network:spawn-clients:{}", state.synthetic_client_count));
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Disconnect", ICON_PERSON_REMOVE)).size(11.0))).on_hover_text("Disconnect all clients").clicked() {
                        *plugin_action = Some("network:disconnect-all".to_string());
                    }
                });
            });
            
            ui.separator();
            
            // === PLAY SECTION ===
            Self::render_section(ui, "Play", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Play Here", ICON_PLAY_CIRCLE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Play at current camera position")
                        .clicked() {
                        state.play_with_character_requested = true;
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Pause", ICON_PAUSE)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Pause simulation")
                        .clicked() {
                        state.pause_requested = true;
                    }
                });
            });
            
            ui.separator();
            
            // === NETWORK SECTION ===
            Self::render_section(ui, "Network", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(65.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Server", ICON_DNS)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Start dedicated server")
                        .clicked() {
                        *menu_action = Some(Action::StartServer);
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Client", ICON_PERSON)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Join as client")
                        .clicked() {
                        // TODO: Open join dialog
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Local", ICON_COMPUTER)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Start local server + client")
                        .clicked() {
                        *menu_action = Some(Action::StartServer);
                    }
                });
            });
            
            ui.separator();
            
            // === BENCHMARK SECTION ===
            Self::render_section(ui, "Benchmark", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Network", ICON_WIFI)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Open network panel")
                        .clicked() {
                        state.show_network_panel = true;
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Stress", ICON_SPEED)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Start stress test")
                        .clicked() {
                        state.show_stress_test_window = true;
                    }
                });
            });
            
            ui.separator();
            
            // === DEBUG SECTION ===
            Self::render_section(ui, "Debug", |ui| {
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(70.0, 24.0);
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Console", ICON_TERMINAL)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Open output console")
                        .clicked() {
                        state.show_output = true;
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Debug", ICON_BUG_REPORT)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Toggle debug mode")
                        .clicked() {
                        // TODO: Toggle debug overlay
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new(format!("{} Cleanup", ICON_CLEANING_SERVICES)).size(11.0)).min_size(btn_size))
                        .on_hover_text("Reset simulation state")
                        .clicked() {
                        state.stop_requested = true;
                    }
                });
            });
        });
    }
    
    // ════════════════════════════════════════════════════════════════════════════
    // CUSTOM TAB - User-defined buttons
    // ════════════════════════════════════════════════════════════════════════════
    fn render_custom_tab(
        ui: &mut egui::Ui,
        state: &mut StudioState,
        custom_idx: usize,
        insert_actions: &mut RibbonInsertActions,
    ) {
        let custom_tab = state.custom_tabs.get(custom_idx).cloned();
        
        if let Some(tab) = custom_tab {
            ui.horizontal(|ui| {
                Self::render_section(ui, &tab.name, |ui| {
                    ui.horizontal(|ui| {
                        let btn_size = egui::vec2(70.0, 24.0);
                        
                        for button in &tab.buttons {
                            let label = if button.icon.is_empty() {
                                button.name.clone()
                            } else {
                                format!("{} {}", button.icon, button.name)
                            };
                            if ui.add(egui::Button::new(egui::RichText::new(&label).size(11.0)).min_size(btn_size))
                                .on_hover_text(match &button.action {
                                    super::CustomTabAction::InsertObject(class) => format!("Insert {}", class),
                                    super::CustomTabAction::PluginAction(action) => format!("Run: {}", action),
                                    super::CustomTabAction::RunScript(script) => format!("Script: {}", script),
                                    super::CustomTabAction::OpenUrl(url) => format!("Open: {}", url),
                                })
                                .clicked() {
                                // Handle button action
                                match &button.action {
                                    super::CustomTabAction::InsertObject(class_str) => {
                                        // Try to parse the class name and insert
                                        if let Ok(class) = ClassName::from_str(class_str) {
                                            insert_actions.inserts.push((class, None));
                                        }
                                    }
                                    super::CustomTabAction::OpenUrl(url) => {
                                        let _ = open::that(url);
                                    }
                                    super::CustomTabAction::PluginAction(_action) => {
                                        // Plugin actions handled by plugin system
                                    }
                                    super::CustomTabAction::RunScript(_script) => {
                                        // Script execution handled by script system
                                    }
                                }
                            }
                        }
                        
                        // Edit button
                        if ui.small_button("Edit").on_hover_text("Edit this tab").clicked() {
                            state.tab_manager.editing_custom_tab_index = Some(custom_idx);
                            state.tab_manager.editing_custom_tab = tab.clone();
                            state.tab_manager.show_edit_custom_tab_modal = true;
                        }
                    });
                });
            });
        } else {
            ui.label(egui::RichText::new("Custom tab not found").color(egui::Color32::RED));
        }
    }
    
    // ════════════════════════════════════════════════════════════════════════════
    // MINDSPACE TAB - Labels, Connections, AI Tools
    // ════════════════════════════════════════════════════════════════════════════
    fn render_mindspace_tab(
        ui: &mut egui::Ui,
        plugin_action: &mut Option<String>,
    ) {
        ui.horizontal(|ui| {
            let btn_size = egui::vec2(70.0, 24.0);
            let selected_color = egui::Color32::from_rgb(100, 80, 180); // Purple for MindSpace
            let normal_color = egui::Color32::from_rgb(55, 55, 60);
            let green_color = egui::Color32::from_rgb(60, 140, 60); // Green for Link
            let blue_color = egui::Color32::from_rgb(60, 100, 180); // Blue for Import
            let red_color = egui::Color32::from_rgb(180, 60, 60); // Red for Export
            
            // === PANEL SECTION ===
            Self::render_section(ui, "Panel", |ui| {
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("👁 Toggle").size(11.0))
                        .fill(normal_color)
                        .min_size(btn_size))
                        .on_hover_text("Toggle MindSpace panel (Ctrl+M)")
                        .clicked() {
                        *plugin_action = Some("mindspace:toggle_panel".to_string());
                    }
                });
            });
            
            ui.separator();
            
            // === LABELS SECTION ===
            Self::render_section(ui, "Labels", |ui| {
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("➕ Add").size(11.0))
                        .fill(selected_color)
                        .min_size(btn_size))
                        .on_hover_text("Add label to selected entity (Ctrl+L)")
                        .clicked() {
                        *plugin_action = Some("mindspace:add_label".to_string());
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new("➖ Remove").size(11.0))
                        .fill(normal_color)
                        .min_size(btn_size))
                        .on_hover_text("Remove label from selected entity")
                        .clicked() {
                        *plugin_action = Some("mindspace:remove_label".to_string());
                    }
                });
            });
            
            ui.separator();
            
            // === CONNECTIONS SECTION ===
            Self::render_section(ui, "Connect", |ui| {
                ui.horizontal(|ui| {
                    // Link button - GREEN
                    if ui.add(egui::Button::new(egui::RichText::new("🔗 Link").size(11.0))
                        .fill(green_color)
                        .min_size(btn_size))
                        .on_hover_text("Connect two nodes with Beam (Ctrl+K)")
                        .clicked() {
                        *plugin_action = Some("mindspace:link".to_string());
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new("🔍 Search").size(11.0))
                        .fill(normal_color)
                        .min_size(btn_size))
                        .on_hover_text("Search nodes (Ctrl+F)")
                        .clicked() {
                        *plugin_action = Some("mindspace:search".to_string());
                    }
                });
            });
            
            ui.separator();
            
            // === FILE SECTION (removed New button) ===
            Self::render_section(ui, "File", |ui| {
                ui.horizontal(|ui| {
                    // Import button - BLUE
                    if ui.add(egui::Button::new(egui::RichText::new("📥 Import").size(11.0))
                        .fill(blue_color)
                        .min_size(btn_size))
                        .on_hover_text("Import mind map")
                        .clicked() {
                        *plugin_action = Some("mindspace:import".to_string());
                    }
                    
                    // Export button - RED
                    if ui.add(egui::Button::new(egui::RichText::new("📤 Export").size(11.0))
                        .fill(red_color)
                        .min_size(btn_size))
                        .on_hover_text("Export mind map")
                        .clicked() {
                        *plugin_action = Some("mindspace:export".to_string());
                    }
                });
            });
            
            ui.separator();
            
            // === AI TOOLS SECTION ===
            Self::render_section(ui, "AI Tools", |ui| {
                ui.horizontal(|ui| {
                    // Suggest button - use lightbulb icon (Material Design: 💡 or text fallback)
                    if ui.add(egui::Button::new(egui::RichText::new("💡 Suggest").size(11.0))
                        .fill(egui::Color32::from_rgb(60, 100, 60))
                        .min_size(btn_size))
                        .on_hover_text("Get AI suggestions for connections")
                        .clicked() {
                        *plugin_action = Some("mindspace:ai_suggest".to_string());
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new("📐 Layout").size(11.0))
                        .fill(egui::Color32::from_rgb(60, 100, 60))
                        .min_size(btn_size))
                        .on_hover_text("Auto-arrange nodes with AI")
                        .clicked() {
                        *plugin_action = Some("mindspace:ai_layout".to_string());
                    }
                    
                    if ui.add(egui::Button::new(egui::RichText::new("📝 Summary").size(11.0))
                        .fill(egui::Color32::from_rgb(60, 100, 60))
                        .min_size(btn_size))
                        .on_hover_text("Generate AI summary")
                        .clicked() {
                        *plugin_action = Some("mindspace:ai_summarize".to_string());
                    }
                });
            });
        });
    }
    
    // ════════════════════════════════════════════════════════════════════════════
    // HELPER: Render a labeled section
    // ════════════════════════════════════════════════════════════════════════════
    fn render_section<R>(ui: &mut egui::Ui, label: &str, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).size(10.0).color(egui::Color32::GRAY));
            add_contents(ui)
        }).inner
    }
    
    // ════════════════════════════════════════════════════════════════════════════
    // TAB MANAGEMENT MODALS
    // ════════════════════════════════════════════════════════════════════════════
    
    /// Show the Add Tab modal
    pub fn show_add_tab_modal(ctx: &egui::Context, state: &mut StudioState) {
        if !state.tab_manager.show_add_tab_modal {
            return;
        }
        
        egui::Window::new("Add Tab")
            .collapsible(false)
            .resizable(true)
            .default_width(400.0)
            .default_height(350.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Search bar
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    ui.add(egui::TextEdit::singleline(&mut state.tab_manager.add_tab_search)
                        .hint_text("Search tabs...")
                        .desired_width(200.0));
                });
                
                ui.add_space(8.0);
                
                // Category tabs
                ui.horizontal(|ui| {
                    let categories = ["All", "Built-in", "Plugins", "Custom"];
                    for (i, cat) in categories.iter().enumerate() {
                        let selected = state.tab_manager.add_tab_category == i;
                        if ui.selectable_label(selected, *cat).clicked() {
                            state.tab_manager.add_tab_category = i;
                        }
                    }
                });
                
                ui.separator();
                
                // Scrollable list of available tabs
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    let search = state.tab_manager.add_tab_search.to_lowercase();
                    let category = state.tab_manager.add_tab_category;
                    
                    // Built-in tabs
                    if category == 0 || category == 1 {
                        for builtin in get_builtin_tabs() {
                            if !search.is_empty() && !builtin.name.to_lowercase().contains(&search) {
                                continue;
                            }
                            
                            // Check if already visible
                            let already_visible = state.visible_tabs.iter().any(|t| {
                                matches!(t, TabEntry::BuiltIn(tab) if tab == &builtin.tab)
                            });
                            
                            ui.horizontal(|ui| {
                                ui.label(builtin.icon);
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(builtin.name).strong());
                                    ui.label(egui::RichText::new(builtin.description).size(10.0).color(egui::Color32::GRAY));
                                });
                                
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add_space(16.0); // Margin for scrollbar
                                    if already_visible {
                                        ui.label(egui::RichText::new("Added").color(egui::Color32::GREEN));
                                    } else {
                                        if ui.button("+ Add").clicked() {
                                            state.visible_tabs.push(TabEntry::BuiltIn(builtin.tab));
                                        }
                                    }
                                });
                            });
                            ui.add_space(4.0);
                        }
                    }
                    
                    // Custom tabs
                    if category == 0 || category == 3 {
                        for (idx, custom) in state.custom_tabs.clone().iter().enumerate() {
                            if !search.is_empty() && !custom.name.to_lowercase().contains(&search) {
                                continue;
                            }
                            
                            let already_visible = state.visible_tabs.iter().any(|t| {
                                matches!(t, TabEntry::Custom(i) if *i == idx)
                            });
                            
                            let row_response = ui.horizontal(|ui| {
                                ui.label(&custom.icon);
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(&custom.name).strong());
                                    ui.label(egui::RichText::new(format!("{} buttons - double-click to edit", custom.buttons.len())).size(10.0).color(egui::Color32::GRAY));
                                });
                                
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.add_space(16.0); // Margin for scrollbar
                                    
                                    // Edit button
                                    if ui.small_button("Edit").clicked() {
                                        state.tab_manager.editing_custom_tab_index = Some(idx);
                                        state.tab_manager.editing_custom_tab = custom.clone();
                                        state.tab_manager.show_edit_custom_tab_modal = true;
                                    }
                                    
                                    if already_visible {
                                        ui.label(egui::RichText::new("Added").color(egui::Color32::GREEN));
                                    } else {
                                        if ui.button("+ Add").clicked() {
                                            state.visible_tabs.push(TabEntry::Custom(idx));
                                        }
                                    }
                                });
                            }).response;
                            
                            // Double-click to edit
                            if row_response.double_clicked() {
                                state.tab_manager.editing_custom_tab_index = Some(idx);
                                state.tab_manager.editing_custom_tab = custom.clone();
                                state.tab_manager.show_edit_custom_tab_modal = true;
                            }
                            
                            ui.add_space(4.0);
                        }
                    }
                    
                    // Plugins placeholder
                    if category == 0 || category == 2 {
                        ui.label(egui::RichText::new("No plugin tabs installed").color(egui::Color32::GRAY).italics());
                    }
                });
                
                ui.separator();
                
                // Create custom tab button
                ui.horizontal(|ui| {
                    if ui.button("✨ Create Custom Tab...").clicked() {
                        state.tab_manager.editing_custom_tab_index = None;
                        state.tab_manager.editing_custom_tab = CustomTab {
                            name: "My Tab".to_string(),
                            icon: "".to_string(),
                            color: [100, 120, 180], // Default blue
                            buttons: Vec::new(),
                        };
                        state.tab_manager.show_edit_custom_tab_modal = true;
                    }
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            state.tab_manager.show_add_tab_modal = false;
                        }
                        
                        if ui.button("📋 Reorder...").clicked() {
                            state.tab_manager.show_reorder_modal = true;
                        }
                    });
                });
            });
    }
    
    /// Show the Reorder Tabs modal
    pub fn show_reorder_modal(ctx: &egui::Context, state: &mut StudioState) {
        if !state.tab_manager.show_reorder_modal {
            return;
        }
        
        egui::Window::new("Reorder Tabs")
            .collapsible(false)
            .resizable(false)
            .default_width(350.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Use arrow buttons to reorder tabs:");
                ui.add_space(8.0);
                
                let mut swap_indices: Option<(usize, usize)> = None;
                let mut remove_idx: Option<usize> = None;
                
                // Collect tab info first to avoid borrow issues
                let tab_info: Vec<(String, String, bool)> = state.visible_tabs.iter().map(|tab_entry| {
                    match tab_entry {
                        TabEntry::BuiltIn(ribbon_tab) => {
                            let builtin = get_builtin_tabs();
                            let info = builtin.iter().find(|b| &b.tab == ribbon_tab);
                            (info.map(|i| i.name).unwrap_or("Tab").to_string(), 
                             info.map(|i| i.icon).unwrap_or("").to_string(),
                             false) // Built-in tabs can't be removed
                        }
                        TabEntry::Plugin { name, .. } => (name.clone(), "".to_string(), true),
                        TabEntry::Custom(custom_idx) => {
                            let custom = state.custom_tabs.get(*custom_idx);
                            (custom.map(|c| c.name.clone()).unwrap_or("Custom".to_string()), 
                             custom.map(|c| c.icon.clone()).unwrap_or("".to_string()),
                             true) // Custom tabs can be removed
                        }
                    }
                }).collect();
                
                let tab_count = tab_info.len();
                
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (idx, (name, icon, can_remove)) in tab_info.iter().enumerate() {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(45, 45, 48))
                            .inner_margin(egui::Margin { left: 8, right: 8, top: 4, bottom: 4 })
                            .rounding(4.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Position number
                                    ui.label(egui::RichText::new(format!("{}.", idx + 1)).color(egui::Color32::GRAY));
                                    
                                    // Move up button
                                    ui.add_enabled_ui(idx > 0, |ui| {
                                        if ui.button("^").on_hover_text("Move up").clicked() {
                                            swap_indices = Some((idx, idx - 1));
                                        }
                                    });
                                    
                                    // Move down button
                                    ui.add_enabled_ui(idx < tab_count - 1, |ui| {
                                        if ui.button("v").on_hover_text("Move down").clicked() {
                                            swap_indices = Some((idx, idx + 1));
                                        }
                                    });
                                    
                                    ui.add_space(8.0);
                                    
                                    // Tab name with icon
                                    let display = if icon.is_empty() {
                                        name.clone()
                                    } else {
                                        format!("{} {}", icon, name)
                                    };
                                    ui.label(egui::RichText::new(display).strong());
                                    
                                    // Remove button (only for non-built-in)
                                    if *can_remove {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.small_button("X").on_hover_text("Remove from ribbon").clicked() {
                                                remove_idx = Some(idx);
                                            }
                                        });
                                    }
                                });
                            });
                        ui.add_space(2.0);
                    }
                });
                
                // Apply swap
                if let Some((a, b)) = swap_indices {
                    state.visible_tabs.swap(a, b);
                }
                
                // Apply remove
                if let Some(idx) = remove_idx {
                    state.visible_tabs.remove(idx);
                }
                
                ui.add_space(8.0);
                ui.separator();
                
                ui.horizontal(|ui| {
                    if ui.button("Reset to Default").clicked() {
                        state.visible_tabs = vec![
                            TabEntry::BuiltIn(RibbonTab::Home),
                            TabEntry::BuiltIn(RibbonTab::Model),
                            TabEntry::BuiltIn(RibbonTab::UI),
                            TabEntry::BuiltIn(RibbonTab::Terrain),
                            TabEntry::BuiltIn(RibbonTab::Test),
                        ];
                    }
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Done").clicked() {
                            state.tab_manager.show_reorder_modal = false;
                        }
                    });
                });
            });
    }
    
    /// Show the Edit Custom Tab modal
    pub fn show_edit_custom_tab_modal(ctx: &egui::Context, state: &mut StudioState) {
        if !state.tab_manager.show_edit_custom_tab_modal {
            return;
        }
        
        let is_new = state.tab_manager.editing_custom_tab_index.is_none();
        let title = if is_new { "Create Custom Tab" } else { "Edit Custom Tab" };
        
        // Available built-in buttons to choose from
        let builtin_buttons = vec![
            ("Part", "Insert Part", super::CustomTabAction::InsertObject("Part".to_string())),
            ("Sphere", "Insert Sphere", super::CustomTabAction::InsertObject("Sphere".to_string())),
            ("Cylinder", "Insert Cylinder", super::CustomTabAction::InsertObject("Cylinder".to_string())),
            ("Wedge", "Insert Wedge", super::CustomTabAction::InsertObject("Wedge".to_string())),
            ("Model", "Insert Model", super::CustomTabAction::InsertObject("Model".to_string())),
            ("Folder", "Insert Folder", super::CustomTabAction::InsertObject("Folder".to_string())),
            ("SpawnLocation", "Insert SpawnLocation", super::CustomTabAction::InsertObject("SpawnLocation".to_string())),
            ("PointLight", "Insert PointLight", super::CustomTabAction::InsertObject("PointLight".to_string())),
            ("SpotLight", "Insert SpotLight", super::CustomTabAction::InsertObject("SpotLight".to_string())),
            ("SurfaceLight", "Insert SurfaceLight", super::CustomTabAction::InsertObject("SurfaceLight".to_string())),
            ("Fire", "Insert Fire Effect", super::CustomTabAction::InsertObject("Fire".to_string())),
            ("Smoke", "Insert Smoke Effect", super::CustomTabAction::InsertObject("Smoke".to_string())),
            ("Sparkles", "Insert Sparkles Effect", super::CustomTabAction::InsertObject("Sparkles".to_string())),
            ("ParticleEmitter", "Insert ParticleEmitter", super::CustomTabAction::InsertObject("ParticleEmitter".to_string())),
            ("SoulScript", "Insert SoulScript", super::CustomTabAction::InsertObject("SoulScript".to_string())),
            ("ScreenGui", "Insert ScreenGui", super::CustomTabAction::InsertObject("ScreenGui".to_string())),
            ("Frame", "Insert Frame", super::CustomTabAction::InsertObject("Frame".to_string())),
            ("TextLabel", "Insert TextLabel", super::CustomTabAction::InsertObject("TextLabel".to_string())),
            ("TextButton", "Insert TextButton", super::CustomTabAction::InsertObject("TextButton".to_string())),
            ("ImageLabel", "Insert ImageLabel", super::CustomTabAction::InsertObject("ImageLabel".to_string())),
            ("Sound", "Insert Sound", super::CustomTabAction::InsertObject("Sound".to_string())),
        ];
        
        egui::Window::new(title)
            .collapsible(false)
            .resizable(true)
            .default_width(500.0)
            .default_height(400.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // Tab name and icon
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.add(egui::TextEdit::singleline(&mut state.tab_manager.editing_custom_tab.name)
                        .desired_width(150.0));
                    
                    ui.add_space(16.0);
                    ui.label("Icon:");
                    ui.add(egui::TextEdit::singleline(&mut state.tab_manager.editing_custom_tab.icon)
                        .desired_width(40.0));
                });
                
                ui.add_space(8.0);
                
                // Color picker
                ui.horizontal(|ui| {
                    ui.label("Tab Color:");
                    
                    let mut color = egui::Color32::from_rgb(
                        state.tab_manager.editing_custom_tab.color[0],
                        state.tab_manager.editing_custom_tab.color[1],
                        state.tab_manager.editing_custom_tab.color[2],
                    );
                    
                    if ui.color_edit_button_srgba(&mut color).changed() {
                        state.tab_manager.editing_custom_tab.color = [color.r(), color.g(), color.b()];
                    }
                    
                    ui.add_space(16.0);
                    ui.label("Preview:");
                    let preview_text = if state.tab_manager.editing_custom_tab.icon.is_empty() {
                        state.tab_manager.editing_custom_tab.name.clone()
                    } else {
                        format!("{} {}", state.tab_manager.editing_custom_tab.icon, state.tab_manager.editing_custom_tab.name)
                    };
                    let preview_btn = egui::Button::new(
                        egui::RichText::new(&preview_text).size(12.0).strong().color(egui::Color32::WHITE)
                    )
                    .fill(color)
                    .min_size(egui::vec2(75.0, 28.0))
                    .rounding(6.0);
                    ui.add(preview_btn);
                });
                
                ui.add_space(8.0);
                ui.separator();
                
                // Buttons section with reordering
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Buttons").strong());
                    ui.label(egui::RichText::new("(drag to reorder)").size(10.0).color(egui::Color32::GRAY));
                });
                
                let mut remove_idx: Option<usize> = None;
                let mut swap_indices: Option<(usize, usize)> = None;
                let button_count = state.tab_manager.editing_custom_tab.buttons.len();
                
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    for idx in 0..button_count {
                        let button = &mut state.tab_manager.editing_custom_tab.buttons[idx];
                        
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(40, 40, 45))
                            .inner_margin(4.0)
                            .rounding(4.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Reorder buttons
                                    ui.add_enabled_ui(idx > 0, |ui| {
                                        if ui.small_button("^").on_hover_text("Move up").clicked() {
                                            swap_indices = Some((idx, idx - 1));
                                        }
                                    });
                                    ui.add_enabled_ui(idx < button_count - 1, |ui| {
                                        if ui.small_button("v").on_hover_text("Move down").clicked() {
                                            swap_indices = Some((idx, idx + 1));
                                        }
                                    });
                                    
                                    ui.separator();
                                    
                                    // Icon
                                    ui.add(egui::TextEdit::singleline(&mut button.icon).desired_width(30.0).hint_text("Icon"));
                                    
                                    // Name
                                    ui.add(egui::TextEdit::singleline(&mut button.name).desired_width(80.0).hint_text("Name"));
                                    
                                    // Action type display
                                    let action_str = match &button.action {
                                        super::CustomTabAction::InsertObject(class) => format!("Insert: {}", class),
                                        super::CustomTabAction::PluginAction(action) => format!("Plugin: {}", action),
                                        super::CustomTabAction::RunScript(script) => format!("Script: {}", script),
                                        super::CustomTabAction::OpenUrl(url) => format!("URL: {}", url),
                                    };
                                    ui.label(egui::RichText::new(action_str).size(10.0).color(egui::Color32::LIGHT_GRAY));
                                    
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("X").on_hover_text("Remove").clicked() {
                                            remove_idx = Some(idx);
                                        }
                                    });
                                });
                            });
                        ui.add_space(2.0);
                    }
                });
                
                // Apply swaps and removes
                if let Some((a, b)) = swap_indices {
                    state.tab_manager.editing_custom_tab.buttons.swap(a, b);
                }
                if let Some(idx) = remove_idx {
                    state.tab_manager.editing_custom_tab.buttons.remove(idx);
                }
                
                ui.add_space(8.0);
                
                // Add button from picker
                ui.horizontal(|ui| {
                    ui.label("Add Button:");
                    egui::ComboBox::from_id_salt("add_button_picker")
                        .selected_text("Select action...")
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            ui.set_min_width(250.0);
                            for (name, tooltip, action) in &builtin_buttons {
                                if ui.selectable_label(false, *name).on_hover_text(*tooltip).clicked() {
                                    state.tab_manager.editing_custom_tab.buttons.push(super::CustomTabButton {
                                        name: name.to_string(),
                                        icon: "".to_string(),
                                        action: action.clone(),
                                    });
                                }
                            }
                            ui.separator();
                            if ui.selectable_label(false, "Custom URL...").clicked() {
                                state.tab_manager.editing_custom_tab.buttons.push(super::CustomTabButton {
                                    name: "Link".to_string(),
                                    icon: "".to_string(),
                                    action: super::CustomTabAction::OpenUrl("https://".to_string()),
                                });
                            }
                            if ui.selectable_label(false, "Run Script...").clicked() {
                                state.tab_manager.editing_custom_tab.buttons.push(super::CustomTabButton {
                                    name: "Script".to_string(),
                                    icon: "".to_string(),
                                    action: super::CustomTabAction::RunScript("script_name".to_string()),
                                });
                            }
                        });
                });
                
                ui.add_space(8.0);
                ui.separator();
                
                // Save/Cancel buttons
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        state.tab_manager.show_edit_custom_tab_modal = false;
                    }
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(if is_new { "Create" } else { "Save" }).clicked() {
                            if is_new {
                                let new_idx = state.custom_tabs.len();
                                state.custom_tabs.push(state.tab_manager.editing_custom_tab.clone());
                                state.visible_tabs.push(TabEntry::Custom(new_idx));
                            } else if let Some(idx) = state.tab_manager.editing_custom_tab_index {
                                if idx < state.custom_tabs.len() {
                                    state.custom_tabs[idx] = state.tab_manager.editing_custom_tab.clone();
                                }
                            }
                            state.tab_manager.show_edit_custom_tab_modal = false;
                        }
                    });
                });
            });
    }
}
