//! # Studio Plugin System
//!
//! A plugin architecture for extending Eustress Engine Studio with custom functionality.
//! Similar to Roblox's Plugin API, this allows developers to create tools, panels,
//! and automation that integrate seamlessly with the editor.
//!
//! ## Plugin Types
//! - **Panel Plugins**: Add custom panels to the UI (dockable windows)
//! - **Menu Plugins**: Add items to menus (File, Edit, View, Plugin menu)
//! - **Tool Plugins**: Add custom tools to the toolbar
//! - **Overlay Plugins**: Add on-screen UI elements
//!
//! ## Creating a Plugin
//! ```rust,ignore
//! use eustress_engine::studio_plugins::prelude::*;
//!
//! pub struct MyPlugin;
//!
//! impl StudioPlugin for MyPlugin {
//!     fn info(&self) -> PluginInfo {
//!         PluginInfo {
//!             id: "my-plugin".to_string(),
//!             name: "My Plugin".to_string(),
//!             version: "1.0.0".to_string(),
//!             author: "Your Name".to_string(),
//!             description: "Does something cool".to_string(),
//!         }
//!     }
//!
//!     fn on_enable(&mut self, api: &mut PluginApi) {
//!         api.add_menu_item("Plugin/My Plugin/Do Thing", || {
//!             println!("Thing done!");
//!         });
//!     }
//! }
//! ```

pub mod api;
pub mod manager;
#[allow(dead_code)]
pub mod registry;
#[allow(dead_code)]
pub mod builtin;
#[allow(dead_code)]
pub mod rune_api;
#[allow(dead_code)]
pub mod tab_api;
#[allow(dead_code)]
pub mod script_plugin;
#[allow(dead_code)]
pub mod api_extensions;

pub use api::*;
pub use manager::*;
pub use registry::*;

use bevy::prelude::*;
use bevy::ecs::message::MessageReader;
use tab_api::TabRegistry;

/// Prelude for plugin development
pub mod prelude {
    pub use super::api::{
        PluginApi, PluginInfo, StudioPlugin, PluginPanel, PluginMenuItem,
        SimClock, SimMode, ScheduledEvent, PropertyValue, SnapshotRequest, SimSnapshot,
    };
    pub use super::manager::PluginManager;
    pub use super::registry::PluginRegistry;
    
    // Rune API exports
    pub use super::rune_api::{
        RuneContext, RuneValue, BillboardOptions, BillboardRequest,
        ScreenUIElement, ScreenButton, ScreenSlider, HotReloadWatcher,
        // Layer 1-4 API types
        MenuRegistration, MenuItem,
        TabRegistration, TabSectionDef, TabButtonDef,
        PanelRequest,
        ScreenGuiRequest, ScreenGuiElementDef,
    };
    
    // Tab API exports
    pub use super::tab_api::{
        TabRegistry, PluginTab, TabSection, TabButton, TabButtonSize,
        DropdownItem, TabApi, CustomTabModal,
    };
    
    // Script Plugin exports
    pub use super::script_plugin::{
        ScriptPlugin, ScriptPluginInfo, ScriptType, ScriptPermission,
        ScriptPluginRegistry, ScriptPluginWrapper,
    };
    
    // API Extensions (SpatialVortex cross-space awareness)
    pub use super::api_extensions::{
        list_all_spaces, get_space_count, is_space_available,
        register_space, unregister_space,
        get_vortex_api_url, get_space_api_url,
        SPATIAL_VORTEX_URL,
        // WebTransport status
        TransportStatus, WebTransportStatusTracker,
        notify_transport_status, get_transport_status_string, is_transport_connected,
    };
    
    pub use bevy::prelude::*;
    // egui removed - using Slint UI
}

/// Message to trigger a plugin menu action
#[derive(bevy::ecs::message::Message)]
pub struct PluginMenuActionEvent {
    pub action_id: String,
}

impl PluginMenuActionEvent {
    pub fn new(action_id: impl Into<String>) -> Self {
        Self { action_id: action_id.into() }
    }
}

/// Message to trigger a plugin action from UI (e.g., MindSpace panel buttons)
#[derive(bevy::ecs::message::Message)]
pub struct PluginActionEvent {
    pub action_id: String,
}

impl PluginActionEvent {
    pub fn new(action_id: impl Into<String>) -> Self {
        Self { action_id: action_id.into() }
    }
}

/// Plugin system for Eustress Engine Studio
pub struct StudioPluginSystem;

impl Plugin for StudioPluginSystem {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<PluginManager>()
            .init_resource::<PluginRegistry>()
            .init_resource::<TabRegistry>()
            .init_resource::<SimClock>()
            .register_type::<SimClock>()
            .register_type::<SimMode>()
            .add_message::<PluginMenuActionEvent>()
            .add_message::<PluginActionEvent>()
            .add_systems(Startup, setup_builtin_plugins)
            .add_systems(Update, (
                advance_sim_clock,
                update_plugins,
                sync_plugin_tabs,
                handle_plugin_menu_actions,
                process_plugin_actions,
                sync_mindspace_selection,
            ).chain())
            // handle_plugin_action_events runs in PostUpdate to process messages written by apply_ui_actions
            .add_systems(PostUpdate, handle_plugin_action_events);
            // Plugin UI is now handled by Slint - see slint_ui.rs
    }
}

/// Advance the simulation clock each frame and sync with LightingService
fn advance_sim_clock(
    time: Res<Time>,
    mut clock: ResMut<SimClock>,
    mut lighting: ResMut<eustress_common::services::LightingService>,
) {
    clock.tick(time.delta_secs());
    
    // Sync SimClock with LightingService time
    // SimClock.current is in seconds, convert to time_of_day (0.0-1.0 = 24 hours)
    // Use modulo to wrap around days
    let sim_hours = (clock.current / 3600.0) % 24.0;
    let time_of_day = sim_hours / 24.0;
    
    // Only update if simulation is running (not paused)
    if !clock.paused && clock.speed > 0.0 {
        lighting.time_of_day = time_of_day as f32;
        lighting.update_clock_time();
    }
}

/// Setup built-in plugins
fn setup_builtin_plugins(
    _manager: ResMut<PluginManager>,
    registry: ResMut<PluginRegistry>,
    tab_registry: ResMut<TabRegistry>,
) {
    // Register built-in plugins
    info!("🔌 Initializing Studio Plugin System");
    
    // Plugins are loaded from Rune scripts in the plugins/ directory
    // The ScriptPluginManager handles loading and enabling script plugins
    
    info!("🔌 Plugin system ready. {} plugins loaded, {} custom tabs.", 
          registry.count(), tab_registry.tabs.len());
}

/// Update all active plugins with simulation clock
fn update_plugins(
    mut manager: ResMut<PluginManager>,
    mut registry: ResMut<PluginRegistry>,
    clock: Res<SimClock>,
) {
    // Tick all enabled plugins with clock
    manager.tick_with_clock(&mut registry, &clock);
}

/// Handle plugin menu action events
fn handle_plugin_menu_actions(
    mut manager: ResMut<PluginManager>,
    mut registry: ResMut<PluginRegistry>,
    mut events: MessageReader<PluginMenuActionEvent>,
) {
    for event in events.read() {
        manager.handle_menu_action(&mut registry, &event.action_id);
    }
}

/// Handle plugin action events from UI (e.g., MindSpace panel buttons)
fn handle_plugin_action_events(
    mut commands: Commands,
    mut events: MessageReader<PluginActionEvent>,
    selection_manager: Option<Res<crate::ui::BevySelectionManager>>,
    mut studio_state: ResMut<crate::ui::StudioState>,
    instance_query: Query<(Entity, &crate::classes::Instance)>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    let Some(selection_manager) = selection_manager else { return };
    use crate::classes::{Instance, ClassName, BillboardGui, TextLabel};
    
    for event in events.read() {
        info!("🔌 Processing plugin action: {}", event.action_id);
        
        match event.action_id.as_str() {
            "mindspace:toggle_panel" => {
                // Toggle MindSpace panel visibility
                studio_state.mindspace_panel_visible = !studio_state.mindspace_panel_visible;
                info!("🏷️ MindSpace panel toggled: {}", studio_state.mindspace_panel_visible);
            }
            "mindspace:add_label" => {
                // Get selected entity
                let selected_ids = selection_manager.0.read().get_selected();
                
                if selected_ids.is_empty() {
                    notifications.warning("Select an entity first to add a label");
                    continue;
                }
                
                // Find the entity from the selection ID
                let selected_id = &selected_ids[0];
                let mut target_entity: Option<Entity> = None;
                
                for (entity, _instance) in instance_query.iter() {
                    let entity_id = format!("{}v{}", entity.index(), entity.generation());
                    if &entity_id == selected_id {
                        target_entity = Some(entity);
                        break;
                    }
                }
                
                let Some(parent_entity) = target_entity else {
                    notifications.warning("Could not find selected entity");
                    continue;
                };
                
                // Get label text and font from studio state
                let label_text = if studio_state.mindspace_edit_buffer.is_empty() {
                    "Label".to_string()
                } else {
                    studio_state.mindspace_edit_buffer.clone()
                };
                let font = studio_state.mindspace_font;
                let font_size = studio_state.mindspace_font_size;
                
                info!("🏷️ Adding label '{}' to entity {:?}", label_text, parent_entity);
                
                // Use proper spawn helpers for correct component setup
                use crate::spawn::{spawn_billboard_gui, spawn_text_label};
                
                // Create BillboardGui
                let billboard_instance = Instance {
                    name: "BillboardGui".to_string(),
                    class_name: ClassName::BillboardGui,
                    archivable: true,
                    id: (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() % u32::MAX as u128) as u32,
                    ..Default::default()
                };
                let mut billboard_gui = BillboardGui::default();
                billboard_gui.adornee = Some(parent_entity);
                billboard_gui.units_offset = [0.0, 3.0, 0.0]; // Offset above the part
                billboard_gui.size = [200.0, 50.0];
                billboard_gui.always_on_top = true;
                
                // Spawn BillboardGui using proper helper (includes Transform, Visibility, Marker)
                let billboard_entity = spawn_billboard_gui(&mut commands, billboard_instance, billboard_gui);
                commands.entity(billboard_entity).insert(bevy::prelude::ChildOf(parent_entity));
                
                // Create TextLabel as child of BillboardGui
                let text_instance = Instance {
                    name: "TextLabel".to_string(),
                    class_name: ClassName::TextLabel,
                    archivable: true,
                    id: (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() % u32::MAX as u128) as u32,
                    ..Default::default()
                };
                let mut text_label = TextLabel::default();
                text_label.text = label_text.clone();
                text_label.font = font;
                text_label.font_size = font_size;
                text_label.text_color3 = [1.0, 1.0, 1.0]; // White text
                text_label.background_transparency = 0.5;
                
                // Spawn TextLabel using proper helper
                let text_entity = spawn_text_label(&mut commands, text_instance, text_label);
                commands.entity(text_entity).insert(bevy::prelude::ChildOf(billboard_entity));
                
                notifications.success(format!("Added label '{}' to entity", label_text));
                info!("✅ Created BillboardGui {:?} with TextLabel {:?} for entity {:?}", billboard_entity, text_entity, parent_entity);
            }
            "mindspace:update_label" => {
                // Update existing TextLabel with new text from MindSpace panel
                let editing_entity = studio_state.mindspace_editing_entity;
                let new_text = studio_state.mindspace_edit_buffer.clone();
                let new_font = studio_state.mindspace_font;
                let new_font_size = studio_state.mindspace_font_size;
                
                if let Some(text_entity) = editing_entity {
                    // Update the TextLabel component directly
                    if let Ok(mut entity_commands) = commands.get_entity(text_entity) {
                        entity_commands.entry::<eustress_common::classes::TextLabel>().and_modify(move |mut label| {
                            label.text = new_text;
                            label.font = new_font;
                            label.font_size = new_font_size;
                        });
                        notifications.success(format!("Updated label to '{}'", studio_state.mindspace_edit_buffer));
                        info!("✅ Updated TextLabel {:?} text to '{}'", text_entity, studio_state.mindspace_edit_buffer);
                    } else {
                        notifications.error("Failed to update label - entity not found");
                    }
                } else {
                    notifications.warning("No label selected to update");
                }
            }
            "mindspace:remove_label" => {
                // Get selected entity and remove its BillboardGui children
                let selected_ids = selection_manager.0.read().get_selected();
                
                if selected_ids.is_empty() {
                    notifications.warning("Select an entity first to remove its label");
                    continue;
                }
                
                notifications.info("Remove label not yet implemented");
            }
            "mindspace:set_source" => {
                notifications.info("Set source for connection");
            }
            "mindspace:connect" => {
                notifications.info("Connect nodes not yet implemented");
            }
            "mindspace:link" => {
                // Link two entities using Attachments and a Beam
                // First click sets source, second click creates the beam connection
                let selected_ids = selection_manager.0.read().get_selected();
                
                if selected_ids.is_empty() {
                    notifications.warning("Select an entity to start linking");
                    continue;
                }
                
                // Find the selected entity
                let selected_id = selected_ids[0].clone();
                let selected_entity = instance_query.iter()
                    .find(|(entity, instance)| {
                        let entity_id = format!("{}v{}", entity.index(), entity.generation());
                        entity_id == selected_id || instance.name == selected_id
                    })
                    .map(|(entity, _)| entity);
                
                if let Some(entity) = selected_entity {
                    if let Some(source) = studio_state.mindspace_link_source {
                        if source != entity {
                            // We have source and target - create Beam connection
                            use crate::classes::{Attachment, Beam};
                            use crate::spawn::{spawn_attachment, spawn_beam};
                            
                            // Create attachment on source entity
                            let source_attachment_instance = Instance {
                                name: "LinkAttachment0".to_string(),
                                class_name: ClassName::Attachment,
                                ..Default::default()
                            };
                            let source_attachment = Attachment::default();
                            let source_att_entity = spawn_attachment(&mut commands, source_attachment_instance, source_attachment, source);
                            commands.entity(source_att_entity).insert(ChildOf(source));
                            
                            // Create attachment on target entity
                            let target_attachment_instance = Instance {
                                name: "LinkAttachment1".to_string(),
                                class_name: ClassName::Attachment,
                                ..Default::default()
                            };
                            let target_attachment = Attachment::default();
                            let target_att_entity = spawn_attachment(&mut commands, target_attachment_instance, target_attachment, entity);
                            commands.entity(target_att_entity).insert(ChildOf(entity));
                            
                            // Create Beam connecting the two attachments
                            let beam_instance = Instance {
                                name: "MindSpaceLink".to_string(),
                                class_name: ClassName::Beam,
                                ..Default::default()
                            };
                            let mut beam = Beam::default();
                            // Use entity index as u32 ID for attachment references
                            beam.attachment0 = Some(source_att_entity.index().index());
                            beam.attachment1 = Some(target_att_entity.index().index());
                            // Green color sequence for the beam
                            beam.color_sequence = vec![(0.0, bevy::color::Color::srgb(0.3, 0.8, 0.3))];
                            beam.width0 = 0.1;
                            beam.width1 = 0.1;
                            beam.enabled = true;
                            
                            let beam_entity = spawn_beam(&mut commands, beam_instance, beam);
                            commands.entity(beam_entity).insert(ChildOf(source));
                            
                            notifications.success("Created link between entities");
                            info!("✅ Created Beam link from {:?} to {:?}", source, entity);
                            
                            // Clear source
                            studio_state.mindspace_link_source = None;
                        } else {
                            notifications.warning("Cannot link entity to itself");
                        }
                    } else {
                        // Set as source
                        studio_state.mindspace_link_source = Some(entity);
                        notifications.info("Source set. Select another entity and click Link again.");
                        info!("🔗 MindSpace: Link source set to {:?}", entity);
                    }
                } else {
                    notifications.error("Could not find selected entity");
                }
            }
            _ => {
                info!("🔌 Unknown plugin action: {}", event.action_id);
            }
        }
    }
}

/// Render UI for all active plugins
/// Note: Plugin UI is now handled by Slint integration
fn render_plugin_ui(
    mut manager: ResMut<PluginManager>,
    mut registry: ResMut<PluginRegistry>,
) {
    manager.render_ui(&mut registry);
}

/// Sync pending tab registrations from plugins to TabRegistry
fn sync_plugin_tabs(
    mut manager: ResMut<PluginManager>,
    mut tab_registry: ResMut<TabRegistry>,
) {
    // Collect pending tab registrations from all plugins
    let pending = manager.collect_pending_tabs();
    
    for (tabs, sections, buttons) in pending {
        // Register tabs
        for tab in tabs {
            tab_registry.register_tab(tab_api::PluginTab {
                id: tab.tab_id,
                label: tab.label,
                icon: tab.icon,
                priority: tab.priority,
                visible: true,
                sections: Vec::new(),
                owner_plugin: Some(tab.owner_plugin),
            });
        }
        
        // Add sections
        for section in sections {
            tab_registry.add_section(&section.tab_id, tab_api::TabSection {
                id: section.section_id,
                name: section.label.clone(),
                label: section.label,
                buttons: Vec::new(),
                collapsible: false,
                collapsed: false,
            });
        }
        
        // Add buttons
        for button in buttons {
            tab_registry.add_button(&button.tab_id, &button.section_id, tab_api::TabButton {
                id: button.button_id,
                label: button.label,
                icon: button.icon,
                tooltip: Some(button.tooltip),
                action_id: button.action_id,
                size: match button.size {
                    api::TabButtonSize::Small => tab_api::TabButtonSize::Small,
                    api::TabButtonSize::Normal => tab_api::TabButtonSize::Normal,
                    api::TabButtonSize::Large => tab_api::TabButtonSize::Large,
                },
                ..Default::default()
            });
        }
    }
}

/// Process pending actions from plugins
fn process_plugin_actions(
    mut commands: Commands,
    mut manager: ResMut<PluginManager>,
    mut clock: ResMut<SimClock>,
    selection_manager: Option<Res<crate::ui::BevySelectionManager>>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    let Some(selection_manager) = selection_manager else { return };
    // Collect all pending actions
    let actions = manager.collect_actions();
    
    for (plugin_id, action) in actions {
        match action {
            PluginAction::Select(entities) => {
                let sm = selection_manager.0.write();
                sm.clear();
                for entity in entities {
                    let entity_id = format!("{}v{}", entity.index(), entity.generation());
                    sm.select(entity_id);
                }
                info!("🔌 Plugin '{}' selected entities", plugin_id);
            }
            PluginAction::Spawn(_spawn_request) => {
                // TODO: Implement spawn via commands
                info!("🔌 Plugin '{}' requested spawn (not yet implemented)", plugin_id);
            }
            PluginAction::SetProperty { entity, property, value } => {
                // TODO: Implement property setting via commands
                info!("🔌 Plugin '{}' requested set property {} on {:?} (not yet implemented)", plugin_id, property, entity);
                let _ = (entity, property, value); // Suppress warnings
            }
            PluginAction::Delete(entities) => {
                // TODO: Implement delete via commands
                info!("🔌 Plugin '{}' requested delete {} entities (not yet implemented)", plugin_id, entities.len());
            }
            PluginAction::OpenFileDialog { title, filters: _, callback_id } => {
                // TODO: Implement file dialog
                info!("🔌 Plugin '{}' requested open file dialog '{}' (callback: {})", plugin_id, title, callback_id);
            }
            PluginAction::SaveFileDialog { title, default_name: _, filters: _, callback_id } => {
                // TODO: Implement file dialog
                info!("🔌 Plugin '{}' requested save file dialog '{}' (callback: {})", plugin_id, title, callback_id);
            }
            
            // === Simulation Control Actions ===
            PluginAction::SetSimSpeed(speed) => {
                clock.set_speed(speed);
                info!("🔌 Plugin '{}' set simulation speed to {}x", plugin_id, speed);
            }
            PluginAction::PauseSim => {
                clock.pause();
                info!("🔌 Plugin '{}' paused simulation", plugin_id);
            }
            PluginAction::ResumeSim => {
                clock.resume();
                info!("🔌 Plugin '{}' resumed simulation", plugin_id);
            }
            PluginAction::ResetSim => {
                clock.reset();
                info!("🔌 Plugin '{}' reset simulation clock", plugin_id);
            }
            PluginAction::SetSimMode(mode) => {
                clock.mode = mode;
                info!("🔌 Plugin '{}' set simulation mode to {:?}", plugin_id, mode);
            }
            PluginAction::StepSim(seconds) => {
                clock.step_by(seconds);
                info!("🔌 Plugin '{}' stepped simulation by {} seconds", plugin_id, seconds);
            }
            PluginAction::CancelEvent(event_id) => {
                // Event cancellation is handled in the API, just log
                info!("🔌 Plugin '{}' cancelled event '{}'", plugin_id, event_id);
            }
            PluginAction::QueryEntities { query_id, component_filter } => {
                // TODO: Implement bulk entity query
                info!("🔌 Plugin '{}' requested entity query '{}' with filter {:?} (not yet implemented)", 
                      plugin_id, query_id, component_filter);
            }
            PluginAction::RequestSnapshot(request) => {
                // TODO: Implement snapshot system
                info!("🔌 Plugin '{}' requested snapshot '{}' (not yet implemented)", plugin_id, request.label);
            }
            PluginAction::SpawnBillboardLabel { parent_entity, text, font_size, color } => {
                // Spawn BillboardGui > TextLabel hierarchy for MindSpace
                use crate::classes::{Instance, ClassName, BillboardGui, TextLabel};
                use crate::spawn::{spawn_billboard_gui, spawn_text_label};
                
                // Create BillboardGui as child of parent entity
                let billboard_instance = Instance {
                    name: "BillboardGui".to_string(),
                    class_name: ClassName::BillboardGui,
                    ..Default::default()
                };
                let mut billboard_gui = BillboardGui::default();
                billboard_gui.adornee = Some(parent_entity);
                billboard_gui.units_offset = [0.0, 3.0, 0.0]; // Offset above the part
                
                let billboard_entity = spawn_billboard_gui(&mut commands, billboard_instance, billboard_gui);
                commands.entity(billboard_entity).insert(ChildOf(parent_entity));
                
                // Create TextLabel as child of BillboardGui
                let text_instance = Instance {
                    name: "TextLabel".to_string(),
                    class_name: ClassName::TextLabel,
                    ..Default::default()
                };
                let text_label = TextLabel {
                    text: text.clone(),
                    font_size,
                    text_color3: [color[0], color[1], color[2]],
                    text_transparency: 1.0 - color[3],
                    background_transparency: 1.0, // Transparent background
                    size: [200.0, 50.0],
                    ..Default::default()
                };
                
                let text_entity = spawn_text_label(&mut commands, text_instance, text_label);
                commands.entity(text_entity).insert(ChildOf(billboard_entity));
                
                info!("🔌 Plugin '{}' spawned BillboardGui > TextLabel for {:?}", plugin_id, parent_entity);
            }
            PluginAction::SpawnScreenGui { gui_id, display_order, elements } => {
                // Spawn ScreenGui with UI hierarchy using Classes
                use crate::classes::{Instance, ClassName, ScreenGui, Frame, TextLabel, TextBox, TextButton};
                use crate::spawn::{spawn_screen_gui, spawn_frame, spawn_text_label, spawn_text_box, spawn_text_button};
                
                // Create ScreenGui root
                let screen_instance = Instance {
                    name: gui_id.clone(),
                    class_name: ClassName::ScreenGui,
                    ..Default::default()
                };
                let screen_gui = ScreenGui {
                    enabled: true,
                    display_order,
                    ..Default::default()
                };
                
                let screen_entity = spawn_screen_gui(&mut commands, screen_instance, screen_gui);
                
                // Recursively spawn child elements
                fn spawn_element(
                    commands: &mut Commands,
                    parent: Entity,
                    element: &api::ScreenGuiElement,
                ) {
                    match element {
                        api::ScreenGuiElement::Frame { id, position_offset, size_offset, background_color, background_transparency, border_color, border_size, children } => {
                            let instance = Instance {
                                name: id.clone(),
                                class_name: ClassName::Frame,
                                ..Default::default()
                            };
                            let frame = Frame {
                                visible: true,
                                position_offset: *position_offset,
                                size_offset: *size_offset,
                                background_color3: *background_color,
                                background_transparency: *background_transparency,
                                border_color3: *border_color,
                                border_size_pixel: *border_size,
                                ..Default::default()
                            };
                            let entity = spawn_frame(commands, instance, frame);
                            commands.entity(entity).insert(ChildOf(parent));
                            
                            // Spawn children
                            for child in children {
                                spawn_element(commands, entity, child);
                            }
                        }
                        api::ScreenGuiElement::TextLabel { id, text, font_size, text_color, position_offset, size_offset, background_transparency } => {
                            let instance = Instance {
                                name: id.clone(),
                                class_name: ClassName::TextLabel,
                                ..Default::default()
                            };
                            let label = TextLabel {
                                text: text.clone(),
                                font_size: *font_size,
                                text_color3: *text_color,
                                position: *position_offset,
                                size: *size_offset,
                                background_transparency: *background_transparency,
                                ..Default::default()
                            };
                            let entity = spawn_text_label(commands, instance, label);
                            commands.entity(entity).insert(ChildOf(parent));
                        }
                        api::ScreenGuiElement::TextBox { id, placeholder, font_size, position_offset, size_offset, text_color, background_color, border_color } => {
                            let instance = Instance {
                                name: id.clone(),
                                class_name: ClassName::TextBox,
                                ..Default::default()
                            };
                            let text_box = TextBox {
                                placeholder_text: placeholder.clone(),
                                font_size: *font_size,
                                position_offset: *position_offset,
                                size_offset: *size_offset,
                                text_color3: *text_color,
                                background_color3: *background_color,
                                border_color3: *border_color,
                                ..Default::default()
                            };
                            let entity = spawn_text_box(commands, instance, text_box);
                            commands.entity(entity).insert(ChildOf(parent));
                        }
                        api::ScreenGuiElement::TextButton { id, text, font_size, position_offset, size_offset, text_color, background_color, action_id: _ } => {
                            let instance = Instance {
                                name: id.clone(),
                                class_name: ClassName::TextButton,
                                ..Default::default()
                            };
                            let button = TextButton {
                                text: text.clone(),
                                font_size: *font_size,
                                position_offset: *position_offset,
                                size_offset: *size_offset,
                                text_color3: *text_color,
                                background_color3: *background_color,
                                ..Default::default()
                            };
                            let entity = spawn_text_button(commands, instance, button);
                            commands.entity(entity).insert(ChildOf(parent));
                        }
                    }
                }
                
                // Spawn all root elements
                for element in &elements {
                    spawn_element(&mut commands, screen_entity, element);
                }
                
                info!("🔌 Plugin '{}' spawned ScreenGui '{}' with {} elements", plugin_id, gui_id, elements.len());
            }
            PluginAction::RemoveScreenGui { gui_id } => {
                // TODO: Track spawned ScreenGuis and remove by ID
                info!("🔌 Plugin '{}' requested removal of ScreenGui '{}' (not yet implemented)", plugin_id, gui_id);
            }
            PluginAction::UpdateBillboardText { entity, text } => {
                // TODO: Find BillboardGui child of entity and update its TextLabel
                info!("🔌 Plugin '{}' requested update billboard text on {:?} to '{}' (not yet implemented)", plugin_id, entity, text);
            }
            PluginAction::RemoveBillboard { entity } => {
                // TODO: Find and despawn BillboardGui child of entity
                info!("🔌 Plugin '{}' requested removal of billboard from {:?} (not yet implemented)", plugin_id, entity);
            }
        }
    }
    
    // Collect and show notifications
    let plugin_notifications = manager.collect_notifications();
    for (_plugin_id, notification) in plugin_notifications {
        match notification.level {
            NotificationLevel::Info => notifications.info(&notification.message),
            NotificationLevel::Success => notifications.success(&notification.message),
            NotificationLevel::Warning => notifications.warning(&notification.message),
            NotificationLevel::Error => notifications.error(&notification.message),
        }
    }
}

/// Sync MindSpace panel text with selected entity's TextLabel
/// When selection changes, populate the edit buffer with the TextLabel text (if present)
fn sync_mindspace_selection(
    mut studio_state: ResMut<crate::ui::StudioState>,
    selection_manager: Option<Res<crate::ui::BevySelectionManager>>,
    // Query to find entities by their Instance component
    instance_query: Query<(Entity, &crate::classes::Instance)>,
    // Query to find BillboardGui children
    children_query: Query<&Children>,
    // Query to find TextLabel components
    text_label_query: Query<&eustress_common::classes::TextLabel>,
    // Query to check for BillboardGuiMarker
    billboard_marker_query: Query<(), With<crate::spawn::BillboardGuiMarker>>,
) {
    let Some(selection_manager) = selection_manager else { return };
    // Only sync when MindSpace panel is visible and in Edit mode
    if !studio_state.mindspace_panel_visible || studio_state.mindspace_mode != crate::ui::MindSpaceMode::Edit {
        return;
    }
    
    // Get current selection
    let selected_ids = selection_manager.0.read().get_selected();
    let current_selected = selected_ids.first().cloned();
    
    // Check if selection changed
    if current_selected == studio_state.mindspace_last_selected {
        return; // No change
    }
    
    // Selection changed - update tracking
    studio_state.mindspace_last_selected = current_selected.clone();
    studio_state.mindspace_editing_entity = None;
    
    // If nothing selected, clear the buffer
    let Some(selected_id) = current_selected else {
        studio_state.mindspace_edit_buffer.clear();
        return;
    };
    
    // Find the selected entity
    let selected_entity = instance_query.iter()
        .find(|(entity, instance)| {
            let entity_id = format!("{}v{}", entity.index(), entity.generation());
            entity_id == selected_id || instance.name == selected_id
        })
        .map(|(entity, _)| entity);
    
    let Some(entity) = selected_entity else {
        studio_state.mindspace_edit_buffer.clear();
        return;
    };
    
    // Look for BillboardGui child with TextLabel
    if let Ok(children) = children_query.get(entity) {
        for child in children.iter() {
            // Check if this child is a BillboardGui
            if billboard_marker_query.get(child).is_ok() {
                // Look for TextLabel children of the BillboardGui
                if let Ok(billboard_children) = children_query.get(child) {
                    for text_child in billboard_children.iter() {
                        if let Ok(text_label) = text_label_query.get(text_child) {
                            // Found a TextLabel - populate the buffer
                            studio_state.mindspace_edit_buffer = text_label.text.clone();
                            studio_state.mindspace_editing_entity = Some(text_child);
                            studio_state.mindspace_font = text_label.font;
                            studio_state.mindspace_font_size = text_label.font_size;
                            info!("🏷️ MindSpace: Populated text from TextLabel: '{}'", text_label.text);
                            return;
                        }
                    }
                }
            }
        }
    }
    
    // No TextLabel found - clear buffer for new label creation
    studio_state.mindspace_edit_buffer.clear();
}
