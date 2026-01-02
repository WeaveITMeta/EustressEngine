#![allow(dead_code)]
#![allow(unused_variables)]

use bevy::prelude::*;
use bevy::ecs::schedule::common_conditions::run_once;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use std::sync::Arc;
use parking_lot::RwLock;

mod explorer;
mod output;
mod ribbon;
mod viewport;
pub mod command_bar;
mod view_selector;
mod asset_manager;
mod collaboration;
mod toolbox;
mod dock;
mod property_widgets;
mod dynamic_properties;
mod selection_sync;
mod class_icons;
mod file_dialogs;
mod history_panel;
mod spawn_events;
mod menu_events;
mod context_menu;
mod world_view;
mod ai_generation;
pub mod icons;
mod publish;
mod service_properties;
mod soul_panel;
mod script_editor;
mod docking;
mod attributes_ui;
pub mod explorer_search;
pub mod explorer_search_ui;
pub mod webview;
pub mod notifications;
pub mod cef_browser;

pub use explorer::{ExplorerPanel, ExplorerExpanded, ExplorerState, ExplorerToggleEvent, handle_explorer_toggle, ExplorerCache, ServiceType, ServiceOwner};
pub use service_properties::{ServicePropertiesPlugin, render_service_properties};
pub use world_view::{UIWorldSnapshot, UIActionQueue, UIAction, WorldViewPlugin};
#[allow(unused_imports)]
pub use output::{OutputPanel, OutputConsole, LogLevel, LogEntry, capture_bevy_logs, push_to_log_buffer, parse_and_push_log};
#[allow(unused_imports)]
pub use property_widgets::*;
#[allow(unused_imports)]
pub use dynamic_properties::{DynamicPropertiesPanel, DynamicPropertiesPlugin};
#[allow(unused_imports)]
pub use selection_sync::{sync_selection_to_properties, SelectionSyncPlugin};
pub use class_icons::{class_color, class_category, class_label, class_label_compact, class_filter_options, matches_filter, class_icon, class_tooltip};
pub use icons::{draw_class_icon, draw_service_icon, draw_brush_icon, draw_material_icon, draw_brush_shape_icon, ICON_SIZE};
pub use file_dialogs::{SceneFile, FileEvent, pick_open_file, pick_save_file};
pub use history_panel::HistoryPanel;
#[allow(unused_imports)]
pub use ribbon::RibbonPanel;
#[allow(unused_imports)]
pub use spawn_events::{
    SpawnPartEvent, PastePartEvent, SpawnEventsPlugin,
    SpawnTerrainEvent, ToggleTerrainEditEvent, SetTerrainBrushEvent,
    ImportTerrainEvent, ExportTerrainEvent,
};
pub use menu_events::MenuActionEvent;
pub use command_bar::{CommandBarPanel, CommandBarState};
#[allow(unused_imports)]
pub use view_selector::{ViewSelectorWidget, ViewSelectorState, ViewMode, handle_view_mode_changes, apply_wireframe_mode, handle_view_mode_shortcuts};
pub use asset_manager::{AssetManagerPanel, AssetManagerState};
pub use collaboration::{CollaborationPanel, CollaborationState, update_collaboration_cursors};
pub use toolbox::{ToolboxPanel, ToolboxState};
pub use dock::{StudioDockState, Tab as DockTab, LeftTab, RightTab};
pub use docking::{DockingLayout, DockDragState, DockZone, DockArea, PanelId, DockingPlugin};
#[allow(unused_imports)]
pub use context_menu::{ContextMenuPlugin, ContextMenuState};
pub use attributes_ui::{AttributesPanelState, render_attributes_panel, AddAttributeState, AddTagState, ParametersEditorState};
pub use explorer_search::{
    SearchQuery, SearchCriterion, SearchResult, CompareOp,
    ExplorerSearchEngine, AdvancedSearchState, FilterBuilderStep,
    SearchPresets, get_searchable_properties, PropertyInfo,
};
pub use explorer_search_ui::{
    show_advanced_search_panel, show_search_results, show_syntax_help,
};
pub use ai_generation::{AIGenerationPanel, AIGenerationUIPlugin, show_generation_queue};
#[allow(unused_imports)]
pub use soul_panel::{SoulPanelState, SoulPanelPlugin, SoulScriptEntry, ScriptBuildStatus};
pub use script_editor::{
    ScriptEditorState, ScriptEditorPlugin, OpenScriptEvent, render_tab_bar, render_script_editor,
    BrowserState, BrowserTabState, OpenBrowserEvent, render_browser_controls, render_browser_content,
    BrowserBookmarks, Bookmark, BookmarkFolder, HistoryEntry,
};

use crate::commands::{SelectionManager, TransformManager};
use crate::classes::{Instance, BasePart};

/// Bevy resource wrapping SelectionManager for UI access
#[derive(Resource)]
pub struct BevySelectionManager(pub Arc<RwLock<SelectionManager>>);

/// Bevy resource wrapping TransformManager for UI access
#[derive(Resource)]
pub struct BevyTransformManager(pub Arc<RwLock<TransformManager>>);

/// Main Studio UI Plugin - orchestrates all panels
pub struct StudioUiPlugin {
    pub selection_manager: Arc<RwLock<SelectionManager>>,
    pub transform_manager: Arc<RwLock<TransformManager>>,
}

/// Resource to track if egui context is ready (skip first frame)
#[derive(Resource)]
pub struct EguiReady(pub bool);

impl Default for EguiReady {
    fn default() -> Self {
        Self(false)
    }
}

/// Run condition: egui is ready (not first frame)
pub fn egui_is_ready(ready: Res<EguiReady>) -> bool {
    ready.0
}

/// Resource to track if egui wants keyboard input (updated each frame before shortcuts run)
#[derive(Resource, Default)]
pub struct EguiInputState {
    pub wants_keyboard: bool,
    pub wants_pointer: bool,
}

// ============================================================================
// Performance Tracking
// ============================================================================

/// Resource to track UI performance metrics
#[derive(Resource)]
pub struct UIPerformance {
    /// Frame time history (last 60 frames)
    pub frame_times: Vec<f32>,
    /// Current FPS
    pub fps: f32,
    /// Average frame time (ms)
    pub avg_frame_time_ms: f32,
    /// UI update budget (ms) - skip heavy updates if exceeded
    pub ui_budget_ms: f32,
    /// Last frame's UI time (ms)
    pub last_ui_time_ms: f32,
    /// Skip heavy UI updates this frame
    pub skip_heavy_updates: bool,
    /// Frame counter for throttling
    pub frame_counter: u64,
}

impl Default for UIPerformance {
    fn default() -> Self {
        Self {
            frame_times: Vec::with_capacity(60),
            fps: 60.0,
            avg_frame_time_ms: 16.67,
            ui_budget_ms: 8.0, // 8ms budget for UI (half of 16.67ms frame)
            last_ui_time_ms: 0.0,
            skip_heavy_updates: false,
            frame_counter: 0,
        }
    }
}

impl UIPerformance {
    /// Update performance metrics
    pub fn update(&mut self, delta_secs: f32) {
        let frame_time_ms = delta_secs * 1000.0;
        
        // Add to history
        self.frame_times.push(frame_time_ms);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }
        
        // Calculate averages
        if !self.frame_times.is_empty() {
            self.avg_frame_time_ms = self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
            self.fps = 1000.0 / self.avg_frame_time_ms;
        }
        
        // Determine if we should skip heavy updates
        self.skip_heavy_updates = self.last_ui_time_ms > self.ui_budget_ms;
        
        self.frame_counter += 1;
    }
    
    /// Check if we should throttle updates (every N frames)
    pub fn should_throttle(&self, interval: u64) -> bool {
        self.frame_counter % interval != 0
    }
    
    /// Record UI time for this frame
    pub fn record_ui_time(&mut self, time_ms: f32) {
        self.last_ui_time_ms = time_ms;
    }
}

impl Plugin for StudioUiPlugin {
    fn build(&self, app: &mut App) {
        app
            // Track egui readiness
            .init_resource::<EguiReady>()
            // Insert manager resources for UI access
            .insert_resource(BevySelectionManager(self.selection_manager.clone()))
            .insert_resource(BevyTransformManager(self.transform_manager.clone()))
            // Add UI state resources
            .init_resource::<StudioState>()
            .init_resource::<ExplorerDragSelect>()  // Explorer box selection state
            .init_resource::<CommandBarState>()
            .init_resource::<ViewSelectorState>()
            .init_resource::<AssetManagerState>()
            .init_resource::<CollaborationState>()
            .init_resource::<ToolboxState>()
            .init_resource::<StudioDockState>()
            .init_resource::<DynamicPropertiesPanel>()  // Phase 2: New properties panel
            .init_resource::<AttributesPanelState>()  // Attributes/Tags/Parameters UI state
            .init_resource::<OutputConsole>()  // Output console with logs
            .init_resource::<ExplorerCache>()  // Performance: cached entity list
            .init_resource::<UIPerformance>()  // Performance tracking
            .init_resource::<SceneFile>()  // Track current scene file
            .init_resource::<crate::commands::CommandHistory>()  // Phase 2 Week 4: Command history
            .init_resource::<PendingMenuActions>()  // Menu action queue
            .init_resource::<selection_sync::SelectionSyncState>()  // Selection sync state
            .init_resource::<EguiInputState>()  // Track egui input state for shortcuts
            .init_resource::<publish::PublishState>()  // Publish dialog state
            .init_resource::<crate::auth::AuthState>()  // Auth state for SSO
            .init_resource::<crate::soul::SoulServiceSettings>()  // Soul service settings (Claude API key)
            .init_resource::<DockingLayout>()  // Docking layout (panel positions)
            .init_resource::<DockDragState>()  // Drag-drop state for docking
            .init_resource::<webview::WebViewManager>()  // WebView manager for browser tabs
            .init_resource::<AdvancedSearchState>()  // Explorer advanced search state
            // Add events
            .add_message::<FileEvent>()
            .add_message::<MenuActionEvent>()
            .add_message::<ExplorerToggleEvent>()
            .add_message::<crate::commands::UndoCommandEvent>()
            .add_message::<crate::commands::RedoCommandEvent>()
            // Add plugins
            .add_plugins(SpawnEventsPlugin)
            .add_plugins(ContextMenuPlugin)
            .add_plugins(WorldViewPlugin)  // Central World access for UI panels
            .add_plugins(script_editor::ScriptEditorPlugin)  // Soul script tabbed editor
            .add_plugins(webview::WebViewPlugin)  // WebView manager for browser tabs
            .add_plugins(cef_browser::CefBrowserPlugin)  // CEF browser (macOS, requires --features cef)
            // Setup custom egui style - run in EguiPrimaryContextPass to ensure context exists
            // This also marks egui as ready after first successful run
            .add_systems(EguiPrimaryContextPass, setup_egui_style.run_if(run_once))
            // Add UI systems - ORDER MATTERS to prevent flickering!
            // Use EguiPrimaryContextPass schedule for egui systems (bevy_egui 0.38)
            // All egui systems skip first frame to avoid font panic (race condition in bevy_egui)
            // Ribbon MUST be first to claim top space
            .add_systems(EguiPrimaryContextPass, ribbon_system.run_if(egui_is_ready))
            // Command bar MUST render before dock_system so it appears BELOW Output panel
            // (egui bottom panels stack: first rendered = bottom-most)
            .add_systems(EguiPrimaryContextPass, command_bar_system_exclusive.after(ribbon_system).run_if(egui_is_ready))
            // Sync generated code before dock system renders
            .add_systems(EguiPrimaryContextPass, sync_generated_code_to_egui.after(command_bar_system_exclusive).run_if(egui_is_ready))
            // Then dock system - MUST run after ribbon_system so egui panel state is shared
            .add_systems(EguiPrimaryContextPass, dock_system.after(ribbon_system).run_if(egui_is_ready))
            // Other UI systems
            .add_systems(EguiPrimaryContextPass, keybindings_window_system.run_if(egui_is_ready))
            .add_systems(EguiPrimaryContextPass, soul_settings_window_system.run_if(egui_is_ready))
            .add_systems(EguiPrimaryContextPass, publish_dialog_system.run_if(egui_is_ready))
            .add_systems(EguiPrimaryContextPass, exit_confirmation_system.run_if(egui_is_ready))
            .add_systems(Update, handle_window_close_request)  // Handle X button close with unsaved changes check
            .add_systems(EguiPrimaryContextPass, login_dialog_system.run_if(egui_is_ready))
            .add_systems(EguiPrimaryContextPass, data_windows_system.run_if(egui_is_ready))
            // Non-egui systems stay in Update
            .add_systems(Update, sync_plugin_tabs_to_visible)  // Sync TabRegistry to visible_tabs
            .add_systems(Update, update_egui_input_state)  // Update egui input state BEFORE shortcuts
            .add_systems(Update, keyboard_shortcuts_exclusive.after(update_egui_input_state))  // Keyboard shortcuts - using unsafe cell
            .add_systems(Update, handle_menu_actions_exclusive)  // Handle menu button clicks
            .add_systems(Update, capture_bevy_logs)  // Capture logs from global buffer
            .add_systems(Update, sync_selection_to_properties)  // Phase 2: Sync selection to dynamic properties
            .add_systems(Update, handle_file_events_exclusive)  // Exclusive system - works!
            .add_systems(Update, handle_view_mode_shortcuts)  // Blender-style numpad shortcuts
            .add_systems(Update, handle_view_mode_changes)
            .add_systems(Update, apply_wireframe_mode)
            .add_systems(Update, update_collaboration_cursors)
            .add_systems(Update, handle_explorer_toggle)  // Handle explorer expand/collapse
            .add_systems(Update, crate::auth::auth_poll_system)  // Poll for auth results
            .add_systems(Update, handle_login_trigger)  // Handle login trigger from UI
            .add_systems(Update, handle_soul_build_trigger)  // Handle Soul script build button clicks
            .add_systems(Startup, try_restore_auth_session);  // Try to restore saved session
    }
}

/// Try to restore auth session on startup
fn try_restore_auth_session(mut auth_state: ResMut<crate::auth::AuthState>) {
    auth_state.try_restore_session();
}

/// Sync plugin tabs from TabRegistry to StudioState.visible_tabs
fn sync_plugin_tabs_to_visible(
    tab_registry: Res<crate::studio_plugins::tab_api::TabRegistry>,
    mut state: ResMut<StudioState>,
) {
    // Get all plugin tabs from registry
    let plugin_tabs = tab_registry.get_all_tabs();
    
    // Check if we need to update (avoid unnecessary updates)
    let current_plugin_count = state.visible_tabs.iter()
        .filter(|t| matches!(t, TabEntry::Plugin { .. }))
        .count();
    
    if plugin_tabs.len() != current_plugin_count {
        // Remove old plugin tabs
        state.visible_tabs.retain(|t| !matches!(t, TabEntry::Plugin { .. }));
        
        // Add plugin tabs from registry
        for tab in plugin_tabs {
            state.visible_tabs.push(TabEntry::Plugin {
                plugin_id: tab.id.clone(),
                name: tab.label.clone(),
            });
        }
    }
}

/// Handle login trigger from UI
fn handle_login_trigger(
    mut state: ResMut<StudioState>,
    mut auth_state: ResMut<crate::auth::AuthState>,
) {
    if state.trigger_login {
        state.trigger_login = false;
        auth_state.show_login();
    }
}

/// Handle Soul script build button clicks from the script editor UI
fn handle_soul_build_trigger(
    mut contexts: EguiContexts,
    mut build_events: MessageWriter<crate::soul::TriggerBuildEvent>,
    mut output: ResMut<OutputConsole>,
    script_editor_state: Res<ScriptEditorState>,
    mut script_query: Query<&mut crate::soul::SoulScriptData>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    
    // Check if there's a pending build entity from the UI
    let pending_entity: Option<Entity> = ctx.data(|d| {
        d.get_temp(egui::Id::new("pending_build_entity"))
    });
    
    if let Some(entity) = pending_entity {
        // Clear the pending build
        ctx.data_mut(|d| {
            d.remove::<Entity>(egui::Id::new("pending_build_entity"));
        });
        
        // CRITICAL: Sync the edit buffer to SoulScriptData.source before building
        // The editor stores changes in edit_buffers, but build reads from SoulScriptData
        if let Some(buffer_source) = script_editor_state.edit_buffers.get(&entity) {
            if let Ok(mut script_data) = script_query.get_mut(entity) {
                if script_data.source != *buffer_source {
                    info!("📝 Syncing edit buffer to SoulScriptData before build");
                    script_data.source = buffer_source.clone();
                    script_data.dirty = true;
                }
                
                // Log what we're building
                let source_preview = if buffer_source.len() > 50 {
                    format!("{}...", &buffer_source[..50])
                } else {
                    buffer_source.clone()
                };
                info!("🔨 Building Soul script: {}", source_preview);
                output.info(format!("🔨 Building Soul script (entity {:?})...", entity));
                
                // Send the build event
                build_events.write(crate::soul::TriggerBuildEvent { entity });
            } else {
                output.error(format!("❌ Build failed: Entity {:?} has no SoulScriptData component", entity));
            }
        } else {
            output.warning(format!("⚠ No edit buffer found for entity {:?}, building from saved source", entity));
            build_events.write(crate::soul::TriggerBuildEvent { entity });
        }
    }
}

/// Login dialog system
fn login_dialog_system(
    mut contexts: EguiContexts,
    mut auth_state: ResMut<crate::auth::AuthState>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    crate::auth::show_login_dialog(ctx, &mut auth_state);
}

/// Global studio state
#[derive(Resource)]
pub struct StudioState {
    pub show_explorer: bool,
    pub show_properties: bool,
    pub show_output: bool,
    pub show_keybindings_window: bool,
    pub show_terrain_editor: bool,
    pub show_soul_settings_window: bool,
    pub current_tool: Tool,
    pub transform_mode: TransformMode,
    
    // Play mode controls (set by ribbon, consumed by play_mode plugin)
    pub play_solo_requested: bool,
    pub play_with_character_requested: bool,
    pub pause_requested: bool,
    pub stop_requested: bool,
    
    // Plugin panels - Terrain and MindSpace share a panel
    pub mindspace_panel_visible: bool,
    pub secondary_panel_tab: SecondaryPanelTab,
    
    // Publish dialog
    pub show_publish_dialog: bool,
    pub publish_as_new: bool,
    
    // Auth
    pub trigger_login: bool,
    
    // Paste mode - when true, next click in viewport places pasted content
    pub pending_paste: bool,
    
    // Pending file action from ribbon menu
    pub pending_file_action: Option<FileEvent>,
    
    // Network panel and benchmark
    pub show_network_panel: bool,
    pub show_forge_connect_window: bool,
    pub show_stress_test_window: bool,
    pub synthetic_client_count: u32,
    
    // Data menu windows (Global Sources, Domains, Variables)
    pub show_global_sources_window: bool,
    pub show_domains_window: bool,
    pub show_global_variables_window: bool,
    pub synthetic_clients_changed: bool,
    
    // Quick add source type (set from Data menu)
    pub quick_add_source_type: Option<String>,
    
    // Sync Domain to Object Type modal
    pub show_sync_domain_modal: bool,
    pub sync_domain_config: SyncDomainModalState,
    
    // Ribbon tab state
    pub ribbon_tab: RibbonTab,
    /// Ordered list of visible tabs in the ribbon
    pub visible_tabs: Vec<TabEntry>,
    /// Custom user-defined tabs
    pub custom_tabs: Vec<CustomTab>,
    /// Tab manager modal state
    pub tab_manager: RibbonTabManagerState,
    
    /// Browser tab open request (url, title) - set by Help menu, consumed by ribbon_system
    pub browser_open_request: Option<(String, String)>,
    
    /// Find dialog visibility
    pub show_find_dialog: bool,
    /// Settings window visibility
    pub show_settings_window: bool,
    
    /// Track unsaved changes for exit confirmation
    pub has_unsaved_changes: bool,
    /// Show exit confirmation modal
    pub show_exit_confirmation: bool,
    
    // MindSpace panel state
    /// Current MindSpace mode (Edit or Connect)
    pub mindspace_mode: MindSpaceMode,
    /// Text buffer for label editing
    pub mindspace_edit_buffer: String,
    /// Font for labels
    pub mindspace_font: eustress_common::classes::Font,
    /// Font size for labels
    pub mindspace_font_size: f32,
    /// Entity whose TextLabel is currently being edited (None = new label mode)
    pub mindspace_editing_entity: Option<Entity>,
    /// Last selected entity ID to detect selection changes
    pub mindspace_last_selected: Option<String>,
    /// Source entity for Link operation (first click sets source, second creates beam)
    pub mindspace_link_source: Option<Entity>,
}

/// MindSpace panel mode
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MindSpaceMode {
    #[default]
    Edit,
    Connect,
}

/// State for the Sync Domain to Object Type modal
#[derive(Default, Clone)]
pub struct SyncDomainModalState {
    /// Target folder entity for sync
    pub target_folder: Option<Entity>,
    /// Selected domain from folder
    pub selected_domain: String,
    /// Target class type
    pub target_class: eustress_common::classes::SyncTargetClass,
    /// Spawn layout
    pub layout: eustress_common::classes::SpawnLayout,
    /// Spacing between entities
    pub spacing: [f32; 3],
    /// Origin offset
    pub origin_offset: [f32; 3],
    /// Default size
    pub default_size: [f32; 3],
    /// Default color
    pub default_color: [f32; 4],
    /// Field for entity name
    pub name_field: String,
    /// Field for color derivation
    pub color_field: String,
    /// Color mappings
    pub color_mappings: Vec<eustress_common::classes::ColorMapping>,
    /// Show billboard labels
    pub show_billboard: bool,
    /// Field for billboard text
    pub billboard_field: String,
    /// Billboard offset
    pub billboard_offset: [f32; 3],
    /// Billboard text alignment (Left, Center, Right)
    pub billboard_alignment: u8, // 0=Left, 1=Center, 2=Right
    /// Available schema fields (populated from domain)
    pub available_fields: Vec<String>,
    /// New color mapping being added
    pub new_color_value: String,
    pub new_color_rgba: [f32; 4],
}

/// Tab for the secondary panel (Terrain/MindSpace)
#[derive(Default, PartialEq, Clone, Copy)]
pub enum SecondaryPanelTab {
    #[default]
    Terrain,
    MindSpace,
}

/// Tab for the main ribbon toolbar
#[derive(Default, PartialEq, Clone, Copy, Debug)]
pub enum RibbonTab {
    #[default]
    Home,
    Model,
    UI,
    Terrain,
    Test,
    /// MindSpace plugin tab
    MindSpace,
    /// Plugin tab by index into TabRegistry
    Plugin(usize),
    /// Custom tab by index into custom_tabs list
    Custom(usize),
}

/// Built-in tab definition for the tab library
#[derive(Clone, Debug, PartialEq)]
pub struct BuiltInTab {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub tab: RibbonTab,
}

/// Plugin-provided tab definition
#[derive(Clone, Debug)]
pub struct PluginTab {
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
}

/// Custom user-defined tab
#[derive(Clone, Debug)]
pub struct CustomTab {
    pub name: String,
    pub icon: String,
    /// Tab color [R, G, B] 0-255
    pub color: [u8; 3],
    /// Buttons in this custom tab
    pub buttons: Vec<CustomTabButton>,
}

impl Default for CustomTab {
    fn default() -> Self {
        Self {
            name: String::new(),
            icon: String::new(),
            color: [80, 80, 100], // Default blue-gray
            buttons: Vec::new(),
        }
    }
}

/// Button in a custom tab
#[derive(Clone, Debug)]
pub struct CustomTabButton {
    pub name: String,
    pub icon: String,
    pub action: CustomTabAction,
}

/// Action for a custom tab button
#[derive(Clone, Debug)]
pub enum CustomTabAction {
    /// Insert an object by class name
    InsertObject(String),
    /// Run a plugin action
    PluginAction(String),
    /// Run a Soul script
    RunScript(String),
    /// Open a URL
    OpenUrl(String),
}

/// Tab entry in the visible tab bar (can be built-in, plugin, or custom)
#[derive(Clone, Debug, PartialEq)]
pub enum TabEntry {
    BuiltIn(RibbonTab),
    Plugin { plugin_id: String, name: String },
    Custom(usize),
}

/// State for the ribbon tab management modals
#[derive(Default, Clone)]
pub struct RibbonTabManagerState {
    /// Show the "Add Tab" modal
    pub show_add_tab_modal: bool,
    /// Show the "Reorder Tabs" modal
    pub show_reorder_modal: bool,
    /// Show the "Edit Custom Tab" modal
    pub show_edit_custom_tab_modal: bool,
    /// Index of custom tab being edited (None = creating new)
    pub editing_custom_tab_index: Option<usize>,
    /// Temporary custom tab being edited
    pub editing_custom_tab: CustomTab,
    /// Search filter for add tab modal
    pub add_tab_search: String,
    /// Selected category in add tab modal (0=All, 1=Built-in, 2=Plugins, 3=Custom)
    pub add_tab_category: usize,
    /// Index of tab being dragged in reorder modal
    pub dragging_tab_index: Option<usize>,
}

/// Get the list of all available built-in tabs
pub fn get_builtin_tabs() -> Vec<BuiltInTab> {
    vec![
        BuiltInTab {
            id: "home",
            name: "Home",
            description: "Camera controls and transform tools",
            icon: "",
            tab: RibbonTab::Home,
        },
        BuiltInTab {
            id: "model",
            name: "Model",
            description: "Parts, models, constraints, and effects",
            icon: "",
            tab: RibbonTab::Model,
        },
        BuiltInTab {
            id: "ui",
            name: "UI",
            description: "User interface elements and layouts",
            icon: "",
            tab: RibbonTab::UI,
        },
        BuiltInTab {
            id: "terrain",
            name: "Terrain",
            description: "Terrain generation and sculpting tools",
            icon: "",
            tab: RibbonTab::Terrain,
        },
        BuiltInTab {
            id: "test",
            name: "Test",
            description: "Server, clients, and benchmarking",
            icon: "",
            tab: RibbonTab::Test,
        },
        BuiltInTab {
            id: "mindspace",
            name: "MindSpace",
            description: "AI-powered 3D mind mapping and labeling",
            icon: "",
            tab: RibbonTab::MindSpace,
        },
    ]
}

impl Default for StudioState {
    fn default() -> Self {
        Self {
            show_explorer: true,  // Show by default
            show_properties: true, // Show by default
            show_output: true,     // Show by default
            show_keybindings_window: false,
            show_terrain_editor: false, // Hidden by default, shown when terrain exists
            show_soul_settings_window: false,
            current_tool: Tool::Select,
            transform_mode: TransformMode::Local,
            
            // Play mode
            play_solo_requested: false,
            play_with_character_requested: false,
            pause_requested: false,
            stop_requested: false,
            
            // Plugin panels
            mindspace_panel_visible: false,
            secondary_panel_tab: SecondaryPanelTab::default(),
            
            // Publish dialog
            show_publish_dialog: false,
            publish_as_new: false,
            
            // Auth
            trigger_login: false,
            
            // Paste mode
            pending_paste: false,
            
            // Pending file action
            pending_file_action: None,
            
            // Network panel and benchmark
            show_network_panel: false,
            show_forge_connect_window: false,
            show_stress_test_window: false,
            synthetic_client_count: 0,
            
            // Data menu windows
            show_global_sources_window: false,
            show_domains_window: false,
            show_global_variables_window: false,
            synthetic_clients_changed: false,
            
            // Quick add source type
            quick_add_source_type: None,
            
            // Sync Domain modal
            show_sync_domain_modal: false,
            sync_domain_config: SyncDomainModalState::default(),
            
            // Ribbon tab
            ribbon_tab: RibbonTab::default(),
            // Default visible tabs (built-in tabs only - plugin tabs added dynamically)
            visible_tabs: vec![
                TabEntry::BuiltIn(RibbonTab::Home),
                TabEntry::BuiltIn(RibbonTab::Model),
                TabEntry::BuiltIn(RibbonTab::UI),
                TabEntry::BuiltIn(RibbonTab::Terrain),
                TabEntry::BuiltIn(RibbonTab::Test),
                TabEntry::BuiltIn(RibbonTab::MindSpace),
            ],
            custom_tabs: Vec::new(),
            tab_manager: RibbonTabManagerState::default(),
            browser_open_request: None,
            
            // Find and Settings
            show_find_dialog: false,
            show_settings_window: false,
            
            // Exit confirmation
            has_unsaved_changes: false,
            show_exit_confirmation: false,
            
            // MindSpace panel state
            mindspace_mode: MindSpaceMode::default(),
            mindspace_edit_buffer: String::new(),
            mindspace_font: eustress_common::classes::Font::default(),
            mindspace_font_size: 16.0,
            mindspace_editing_entity: None,
            mindspace_last_selected: None,
            mindspace_link_source: None,
        }
    }
}

impl SyncDomainModalState {
    pub fn reset(&mut self) {
        *self = Self {
            spacing: [2.0, 0.0, 0.0],
            default_size: [4.0, 4.0, 4.0],
            default_color: [0.5, 0.5, 0.5, 1.0],
            billboard_offset: [0.0, 3.0, 0.0],
            new_color_rgba: [1.0, 0.0, 0.0, 1.0],
            ..Default::default()
        };
    }
    
    pub fn init_for_folder(&mut self, entity: Entity, domain: &str, fields: Vec<String>) {
        self.reset();
        self.target_folder = Some(entity);
        self.selected_domain = domain.to_string();
        self.available_fields = fields;
    }
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum Tool {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum TransformMode {
    #[default]
    Local,
    Global,
}

/// State for explorer drag selection (box selection)
#[derive(Resource, Default)]
pub struct ExplorerDragSelect {
    /// Is drag selection active?
    pub active: bool,
    /// Start position of drag (in UI coordinates)
    pub start_pos: Option<egui::Pos2>,
    /// Current position of drag
    pub current_pos: Option<egui::Pos2>,
    /// Entity IDs and their row rects collected during rendering
    pub row_rects: Vec<(Entity, egui::Rect)>,
}

impl ExplorerDragSelect {
    /// Get the selection rectangle (normalized so min < max)
    pub fn selection_rect(&self) -> Option<egui::Rect> {
        match (self.start_pos, self.current_pos) {
            (Some(start), Some(current)) => {
                Some(egui::Rect::from_two_pos(start, current))
            }
            _ => None,
        }
    }
    
    /// Get entities whose rows intersect with the selection rectangle
    pub fn get_selected_entities(&self) -> Vec<Entity> {
        if let Some(sel_rect) = self.selection_rect() {
            self.row_rects.iter()
                .filter(|(_, row_rect)| sel_rect.intersects(*row_rect))
                .map(|(entity, _)| *entity)
                .collect()
        } else {
            Vec::new()
        }
    }
    
    /// Clear the drag state
    pub fn clear(&mut self) {
        self.active = false;
        self.start_pos = None;
        self.current_pos = None;
    }
}

/// Ribbon (top toolbar) system
fn ribbon_system(
    mut contexts: EguiContexts,
    mut state: ResMut<StudioState>,
    mut view_state: ResMut<ViewSelectorState>,
    mut asset_state: ResMut<AssetManagerState>,
    mut collab_state: ResMut<CollaborationState>,
    mut cmd_state: ResMut<CommandBarState>,
    mut tool_states: ParamSet<(
        ResMut<crate::move_tool::MoveToolState>,
        ResMut<crate::rotate_tool::RotateToolState>,
        ResMut<crate::scale_tool::ScaleToolState>,
    )>,
    selection_and_undo: (Res<BevySelectionManager>, Res<crate::undo::UndoStack>),
    play_mode_state: Res<bevy::prelude::State<crate::play_mode::PlayModeState>>,
    mut events: ParamSet<(
        MessageWriter<crate::undo::UndoEvent>,
        MessageWriter<crate::undo::RedoEvent>,
        MessageWriter<FileEvent>,
        MessageWriter<MenuActionEvent>,
        MessageWriter<SpawnTerrainEvent>,
        MessageWriter<ToggleTerrainEditEvent>,
        MessageWriter<SetTerrainBrushEvent>,
    )>,
    mut plugin_events: MessageWriter<crate::studio_plugins::PluginMenuActionEvent>,
    mut browser_events: MessageWriter<OpenBrowserEvent>,
    mut insert_events: MessageWriter<context_menu::InsertObjectEvent>,
    keybindings: Res<crate::keybindings::KeyBindings>,
    // Terrain
    terrain_mode: Res<eustress_common::terrain::TerrainMode>,
    terrain_query: Query<Entity, With<eustress_common::terrain::TerrainRoot>>,
) {
    // Update tool states based on current tool selection
    tool_states.p0().active = state.current_tool == Tool::Move;
    tool_states.p1().active = state.current_tool == Tool::Rotate;
    tool_states.p2().active = state.current_tool == Tool::Scale;
    
    let Ok(ctx) = contexts.ctx_mut() else { 
        warn!("Ribbon: Failed to get egui context");
        return; 
    };
    
    // Paint the entire top area with background color to eliminate any gaps between panels
    // This covers the area from window top to below the ribbon and tab bar
    let screen_rect = ctx.screen_rect();
    let top_fill_rect = egui::Rect::from_min_max(
        screen_rect.min,
        egui::pos2(screen_rect.max.x, screen_rect.min.y + 100.0), // Cover ribbon (~60) + tab bar (28) + margin
    );
    ctx.layer_painter(egui::LayerId::background())
        .rect_filled(top_fill_rect, 0.0, egui::Color32::from_rgb(35, 35, 38));
    
    // Check if terrain exists
    let has_terrain = !terrain_query.is_empty();
    
    // We need to call RibbonPanel::show with individual event writers
    // Since ParamSet requires exclusive access, we'll handle events after the UI
    
    // Collect UI actions
    let mut undo_requested = false;
    let mut redo_requested = false;
    let mut file_event: Option<FileEvent> = None;
    let mut menu_action: Option<crate::keybindings::Action> = None;
    let mut terrain_spawn: Option<eustress_common::terrain::TerrainConfig> = None;
    let mut terrain_toggle = false;
    let mut terrain_brush: Option<eustress_common::terrain::BrushMode> = None;
    let mut plugin_action: Option<String> = None;
    let mut insert_actions = ribbon::RibbonInsertActions::default();
    
    // Show ribbon UI and collect actions
    let (_, undo_stack) = &selection_and_undo;
    ribbon::RibbonPanel::show_with_callbacks(
        ctx,
        &mut state,
        &mut view_state,
        &mut asset_state,
        &mut collab_state,
        &mut cmd_state,
        undo_stack,
        &keybindings,
        &terrain_mode,
        has_terrain,
        *play_mode_state.get(),
        // Callbacks
        &mut undo_requested,
        &mut redo_requested,
        &mut file_event,
        &mut menu_action,
        &mut terrain_spawn,
        &mut terrain_toggle,
        &mut terrain_brush,
        &mut plugin_action,
        &mut insert_actions,
    );
    
    // Show ribbon tab management modals
    ribbon::RibbonPanel::show_add_tab_modal(ctx, &mut state);
    ribbon::RibbonPanel::show_reorder_modal(ctx, &mut state);
    ribbon::RibbonPanel::show_edit_custom_tab_modal(ctx, &mut state);
    
    // Handle browser open request from Help menu
    if let Some((url, title)) = state.browser_open_request.take() {
        browser_events.write(OpenBrowserEvent::new(url, title));
    }
    
    // Send events based on collected actions
    if undo_requested {
        events.p0().write(crate::undo::UndoEvent);
    }
    if redo_requested {
        events.p1().write(crate::undo::RedoEvent);
    }
    if let Some(fe) = file_event {
        events.p2().write(fe);
    }
    if let Some(action) = menu_action {
        events.p3().write(MenuActionEvent::new(action));
    }
    if let Some(config) = terrain_spawn {
        events.p4().write(SpawnTerrainEvent { config });
    }
    if terrain_toggle {
        events.p5().write(ToggleTerrainEditEvent);
    }
    if let Some(mode) = terrain_brush {
        events.p6().write(SetTerrainBrushEvent { mode });
    }
    if let Some(action_id) = plugin_action {
        plugin_events.write(crate::studio_plugins::PluginMenuActionEvent::new(action_id));
    }
    
    // Process ribbon insert actions
    // GUI elements should be inserted into the selected parent if it's a valid GUI container
    // BillboardGui/SurfaceGui require BasePart or Attachment parent
    // Other GUI elements (Frame, ScrollingFrame, etc.) can be inserted into GUI containers
    let (selection_manager, _) = &selection_and_undo;
    
    for (class_name, parent) in insert_actions.inserts {
        use eustress_common::classes::ClassName;
        
        // Get current selection to use as potential parent
        let selected = selection_manager.0.read().get_selected();
        let first_selected = selected.first().and_then(|id_str| parse_entity_from_string(id_str));
        
        // Check if this is a GUI element that should respect selected parent
        let is_gui_element = matches!(class_name, 
            ClassName::ScreenGui | ClassName::Frame | ClassName::ScrollingFrame |
            ClassName::TextLabel | ClassName::ImageLabel | ClassName::TextButton |
            ClassName::ImageButton | ClassName::TextBox | ClassName::VideoFrame |
            ClassName::DocumentFrame | ClassName::WebFrame | ClassName::ViewportFrame
        );
        
        // BillboardGui and SurfaceGui require BasePart or Attachment parent
        if matches!(class_name, ClassName::BillboardGui | ClassName::SurfaceGui) {
            if let Some(parent_entity) = first_selected {
                // Insert with selected parent (validation happens in InsertObjectEvent handler)
                insert_events.write(context_menu::InsertObjectEvent {
                    class_name,
                    parent: Some(parent_entity),
                    position: bevy::prelude::Vec3::ZERO,
                });
            } else {
                // Show error notification - BillboardGui/SurfaceGui need a parent
                let gui_name = if class_name == ClassName::BillboardGui { "BillboardGui" } else { "SurfaceGui" };
                cmd_state.status = format!("⚠️ {} requires a Part or Attachment parent. Select one first.", gui_name);
            }
        } else if is_gui_element {
            // GUI elements: use selected parent if available, otherwise insert at root (StarterGui)
            let parent_to_use = if parent.is_some() { parent } else { first_selected };
            insert_events.write(context_menu::InsertObjectEvent {
                class_name,
                parent: parent_to_use,
                position: bevy::prelude::Vec3::ZERO,
            });
        } else {
            // Normal insert (3D objects, etc.)
            // For Folder: use selected parent if available, otherwise defaults to Workspace (not StarterGui)
            // The parent is determined by: explicit parent > selected entity > None (which means Workspace in handler)
            let parent_to_use = if parent.is_some() { 
                parent 
            } else if class_name == ClassName::Folder {
                // Folder defaults to selected entity or Workspace (None = Workspace in handler)
                first_selected
            } else {
                parent
            };
            insert_events.write(context_menu::InsertObjectEvent {
                class_name,
                parent: parent_to_use,
                position: bevy::prelude::Vec3::new(0.0, 5.0, 0.0),
            });
        }
    }
    
    // Show any validation errors from ribbon
    for error in insert_actions.errors {
        cmd_state.status = error;
    }
}

// Old panel systems removed - now using unified dock_system

/// Viewport overlay system (FPS, gizmo hints, etc.)
fn viewport_overlay_system(
    mut contexts: EguiContexts,
    state: Res<StudioState>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return; };
    viewport::show_overlay(ctx, &state);
}

/// Command bar system - renders at the very bottom of the screen
/// Sends commands to Claude API for Rune code generation and execution
fn command_bar_system_exclusive(
    mut contexts: EguiContexts,
    mut cmd_state: ResMut<CommandBarState>,
    build_state: Res<crate::soul::CommandBarBuildState>,
    mut build_events: MessageWriter<crate::soul::CommandBarBuildEvent>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return; };
    
    if !cmd_state.show {
        return;
    }
    
    // Sync building state from build pipeline
    cmd_state.building = build_state.building;
    if build_state.building {
        cmd_state.status = "🔄 Building...".to_string();
    } else if let Some(ref result) = build_state.result {
        if result.success {
            cmd_state.status = "✅ Success".to_string();
        } else {
            cmd_state.status = format!("❌ {}", result.error.as_deref().unwrap_or("Failed"));
        }
    }
    
    egui::TopBottomPanel::bottom("command_bar")
        .min_height(40.0)
        .max_height(150.0)
        .frame(egui::Frame::new()
            .fill(egui::Color32::from_rgb(35, 35, 38))  // Match other panels
            .stroke(egui::Stroke::NONE))
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                // Header with toggle and status
                ui.horizontal(|ui| {
                    ui.label("💻 Command Bar");
                    
                    // Show status
                    if !cmd_state.status.is_empty() {
                        ui.separator();
                        ui.label(&cmd_state.status);
                    }
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("X").clicked() {
                            cmd_state.show = false;
                        }
                        ui.label("Ctrl+K to toggle");
                    });
                });
                
                ui.separator();
                
                // Main input area
                ui.horizontal(|ui| {
                    ui.label(">");
                    
                    // Disable input while building
                    let response = ui.add_enabled(!cmd_state.building, egui::TextEdit::singleline(&mut cmd_state.input)
                        .font(egui::TextStyle::Monospace)
                        .hint_text("Describe what you want... (e.g., 'create a red cube at 0,5,0')")
                        .desired_width(f32::INFINITY)
                    );
                    
                    // Handle Enter key - send to Claude API
                    let should_execute = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let button_clicked = ui.add_enabled(!cmd_state.building, egui::Button::new("Execute")).clicked();
                    
                    if (should_execute || button_clicked) && !cmd_state.input.is_empty() && !cmd_state.building {
                        let input_clone = cmd_state.input.clone();
                        info!("💻 Command Bar: Sending to Claude: {}", input_clone);
                        
                        // Add to history as AI build entry
                        cmd_state.history.push(command_bar::CommandHistoryEntry {
                            english_input: input_clone.clone(),
                            rune_script: None, // Will be populated when Claude responds
                            is_ai_build: true,
                        });
                        cmd_state.history_index = None;
                        cmd_state.history_original_input = None;
                        
                        // Send build event
                        build_events.write(crate::soul::CommandBarBuildEvent {
                            command: input_clone,
                            use_cached: false,
                        });
                        
                        // Clear input and set status
                        cmd_state.input.clear();
                        cmd_state.status = "🔄 Sending to Claude...".to_string();
                        
                        if should_execute {
                            response.request_focus();
                        }
                    }
                    
                    // History navigation with arrow keys (VS Code style)
                    if response.has_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                            if !cmd_state.history.is_empty() {
                                let new_idx = match cmd_state.history_index {
                                    None => cmd_state.history.len() - 1,
                                    Some(idx) if idx > 0 => idx - 1,
                                    Some(idx) => idx,
                                };
                                let english_input = cmd_state.history[new_idx].english_input.clone();
                                cmd_state.history_index = Some(new_idx);
                                cmd_state.input = english_input.clone();
                                cmd_state.history_original_input = Some(english_input);
                            }
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                            if let Some(idx) = cmd_state.history_index {
                                if idx < cmd_state.history.len() - 1 {
                                    let english_input = cmd_state.history[idx + 1].english_input.clone();
                                    cmd_state.history_index = Some(idx + 1);
                                    cmd_state.input = english_input.clone();
                                    cmd_state.history_original_input = Some(english_input);
                                } else {
                                    cmd_state.history_index = None;
                                    cmd_state.history_original_input = None;
                                    cmd_state.input.clear();
                                }
                            }
                        }
                    }
                });
            });
        });
}

/// Sync generated code from SoulScriptData to egui temp data for Rune view
fn sync_generated_code_to_egui(
    mut contexts: EguiContexts,
    script_data_query: Query<(Entity, &crate::soul::SoulScriptData)>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    
    for (entity, script_data) in script_data_query.iter() {
        if let Some(ref code) = script_data.generated_code {
            ctx.data_mut(|d| {
                d.insert_temp(egui::Id::new(("generated_code", entity)), code.clone());
            });
        }
    }
}

/// Unified dock system for all dockable panels
/// Uses UIWorldSnapshot for read access and UIActionQueue for write actions
fn dock_system(
    mut contexts: EguiContexts,
    mut dock_state: ResMut<StudioDockState>,
    mut studio_state: ResMut<StudioState>,
    mut output_console: ResMut<OutputConsole>,
    mut action_queue: ResMut<UIActionQueue>,
    mut toolbox_state: ResMut<ToolboxState>,
    mut collab_state: ResMut<CollaborationState>,
    mut explorer_states: (ResMut<ExplorerDragSelect>, ResMut<AdvancedSearchState>),
    mut editor_states: (ResMut<ScriptEditorState>, ResMut<BrowserState>, ResMut<webview::WebViewManager>, ResMut<cef_browser::CefBrowserState>),
    mut soul_settings: ResMut<crate::soul::SoulServiceSettings>,
    mut modal_state: ResMut<world_view::PropertiesModalState>,
    world_snapshot: Res<UIWorldSnapshot>,
    expanded: Res<ExplorerExpanded>,
    undo_stack: Res<crate::undo::UndoStack>,
    mut undo_redo: (MessageWriter<crate::undo::UndoEvent>, MessageWriter<crate::undo::RedoEvent>),
    mut docking_and_services: ((ResMut<DockingLayout>, ResMut<DockDragState>, ResMut<AssetManagerState>), (ResMut<eustress_common::services::Workspace>, ResMut<service_properties::WorkspaceService>, ResMut<service_properties::PlayersService>, ResMut<service_properties::LightingService>, ResMut<service_properties::SoundServiceService>)),
) {
    let (mut drag_select, mut advanced_search) = explorer_states;
    let (mut script_editor_state, mut browser_state, mut webview_manager, mut cef_state) = editor_states;
    let (mut undo_events, mut redo_events) = undo_redo;
    let (docking, services) = docking_and_services;
    let (mut docking_layout, mut drag_state, mut asset_state) = docking;
    let (mut workspace_res, mut workspace_ui, mut players_service, mut lighting_service, mut sound_service) = services;
    let Ok(ctx) = contexts.ctx_mut() else { return };
 
    
    // Handle drag-drop completion
    if !ctx.input(|i| i.pointer.any_down()) && drag_state.is_dragging() {
        if let Some((panel, zone)) = drag_state.end_drag() {
            if docking_layout.move_panel(panel, zone) {
                info!("Moved panel {:?} to {:?}", panel, zone);
                // Save layout after move
                if let Err(e) = docking_layout.save() {
                    warn!("Failed to save dock layout: {}", e);
                }
            }
        }
    }
    
    // Render drop zone overlay if dragging
    docking::render_drop_zones(ctx, &mut drag_state, &docking_layout);
    
    let left_tab = dock_state.left_tab;
    let right_tab = dock_state.right_tab;
    
    // SCRIPT EDITOR TAB BAR - Always visible (Scene tab is always present)
    let script_editor_active = !script_editor_state.is_scene_active();
    {
        egui::TopBottomPanel::top("script_tab_bar")
            .exact_height(28.0)
            .show_separator_line(false)
            .frame(egui::Frame::new()
                .fill(egui::Color32::from_rgb(45, 45, 48))  // Slightly lighter to distinguish from ribbon
                .inner_margin(egui::Margin { left: 4, right: 4, top: 2, bottom: 4 })
                .outer_margin(egui::Margin::ZERO)
                .stroke(egui::Stroke::NONE)
                .shadow(egui::Shadow::NONE))
            .show(ctx, |ui| {
                render_tab_bar(ui, &mut script_editor_state);
            });
        
        // Handle entity_to_highlight from double-click on tab
        if let Some(entity) = script_editor_state.entity_to_highlight.take() {
            info!("Tab double-click: Highlighting entity {:?} in Explorer", entity);
            action_queue.push(UIAction::Select(vec![entity]));
        }
    }
    
    // BOTTOM PANEL - Output (always rendered when enabled)
    if studio_state.show_output {
        egui::TopBottomPanel::bottom("bottom_panel")
            .default_height(150.0)
            .min_height(100.0)
            .resizable(true)
            .frame(egui::Frame::new()
                .fill(egui::Color32::from_rgb(35, 35, 38))
                .inner_margin(egui::Margin { left: 4, right: 4, top: 4, bottom: 4 })
                .outer_margin(egui::Margin::ZERO)
                .stroke(egui::Stroke::NONE)
                .shadow(egui::Shadow::NONE))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("📋 Output");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("X").on_hover_text("Close Output").clicked() {
                            studio_state.show_output = false;
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    OutputPanel::show_content(ui, &mut output_console);
                });
            });
    }
    
    // LEFT PANEL - Explorer/Toolbox/Assets
    // Use a stable width that persists across frames and tab switches
    let left_panel_id = egui::Id::new("left_panel_width");
    let left_stored_width: f32 = ctx.data_mut(|d| d.get_persisted(left_panel_id)).unwrap_or(280.0);
    
    if studio_state.show_explorer {
        egui::SidePanel::left("left_panel")
            .default_width(left_stored_width)  // Use stored width to prevent auto-resize
            .min_width(220.0)
            .max_width(450.0)
            .resizable(true)
            .frame(egui::Frame::new()
                .fill(egui::Color32::from_rgb(35, 35, 38))
                .inner_margin(egui::Margin { left: 4, right: 4, top: 4, bottom: 4 })
                .outer_margin(egui::Margin::ZERO)
                .stroke(egui::Stroke::NONE)
                .shadow(egui::Shadow::NONE))
            .show(ctx, |ui| {
                // Store the current width for next frame (only updates when user resizes)
                let current_width = ui.available_width() + 8.0; // Account for margins
                if (current_width - left_stored_width).abs() > 2.0 {
                    ui.ctx().data_mut(|d| d.insert_persisted(left_panel_id, current_width));
                }
                // Tab header row with draggable tabs
                ui.horizontal(|ui| {
                    // Explorer tab - draggable
                    let explorer_response = ui.selectable_label(left_tab == dock::LeftTab::Explorer, "📁 Explorer");
                    if explorer_response.clicked() {
                        dock_state.left_tab = dock::LeftTab::Explorer;
                    }
                    if explorer_response.drag_started() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.start_drag(PanelId::Explorer, pos);
                        }
                    }
                    if explorer_response.dragged() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.update_drag(pos);
                        }
                    }
                    
                    // Toolbox tab - draggable
                    let toolbox_response = ui.selectable_label(left_tab == dock::LeftTab::Toolbox, "🛠 Toolbox");
                    if toolbox_response.clicked() {
                        dock_state.left_tab = dock::LeftTab::Toolbox;
                    }
                    if toolbox_response.drag_started() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.start_drag(PanelId::Toolbox, pos);
                        }
                    }
                    if toolbox_response.dragged() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.update_drag(pos);
                        }
                    }
                    
                    // Assets tab - draggable
                    let assets_response = ui.selectable_label(left_tab == dock::LeftTab::Assets, "📦 Assets");
                    if assets_response.clicked() {
                        dock_state.left_tab = dock::LeftTab::Assets;
                    }
                    if assets_response.drag_started() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.start_drag(PanelId::Assets, pos);
                        }
                    }
                    if assets_response.dragged() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.update_drag(pos);
                        }
                    }
                    
                    // MindSpace tab
                    let mindspace_response = ui.selectable_label(left_tab == dock::LeftTab::MindSpace, "MindSpace");
                    if mindspace_response.clicked() {
                        dock_state.left_tab = dock::LeftTab::MindSpace;
                    }
                    
                    // Close button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("X").on_hover_text("Close Panel (View menu to reopen)").clicked() {
                            studio_state.show_explorer = false;
                        }
                    });
                });
                ui.separator();
            
                egui::ScrollArea::vertical()
                    .id_salt("left_panel_scroll")
                    .show(ui, |ui| {
                        match left_tab {
                            dock::LeftTab::Explorer => {
                                // Explorer using UIWorldSnapshot with drag selection and advanced search
                                show_explorer_panel(ui, &world_snapshot, &expanded, &mut action_queue, &mut drag_select, &mut advanced_search);
                            }
                            dock::LeftTab::Toolbox => {
                                ToolboxPanel::show_content_simple(ui, &mut toolbox_state, &mut action_queue);
                            }
                            dock::LeftTab::Assets => {
                                AssetManagerPanel::show_content(ui, &mut asset_state);
                            }
                            dock::LeftTab::MindSpace => {
                                // MindSpace panel - mirrors the Rune script's render_panel function
                                ui.heading("MindSpace");
                                ui.add_space(4.0);
                                
                                // Mode selector
                                ui.horizontal(|ui| {
                                    let edit_selected = studio_state.mindspace_mode == MindSpaceMode::Edit;
                                    let connect_selected = studio_state.mindspace_mode == MindSpaceMode::Connect;
                                    
                                    if ui.selectable_label(edit_selected, "Edit").clicked() {
                                        studio_state.mindspace_mode = MindSpaceMode::Edit;
                                    }
                                    if ui.selectable_label(connect_selected, "Connect").clicked() {
                                        studio_state.mindspace_mode = MindSpaceMode::Connect;
                                    }
                                });
                                
                                ui.separator();
                                
                                match studio_state.mindspace_mode {
                                    MindSpaceMode::Edit => {
                                        // Edit mode UI
                                        let is_editing = studio_state.mindspace_editing_entity.is_some();
                                        if is_editing {
                                            ui.label("Editing existing label. Modify and click Update.");
                                        } else {
                                            ui.label("Select an entity to add or edit labels.");
                                        }
                                        ui.add_space(8.0);
                                        
                                        ui.horizontal(|ui| {
                                            ui.label("Text:");
                                            let text_response = ui.text_edit_singleline(&mut studio_state.mindspace_edit_buffer);
                                            // Press Enter to add/update label
                                            if text_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                                info!("🏷️ MindSpace: Enter pressed in text field");
                                                if is_editing {
                                                    action_queue.push(UIAction::PluginAction("mindspace:update_label".to_string()));
                                                } else {
                                                    action_queue.push(UIAction::PluginAction("mindspace:add_label".to_string()));
                                                }
                                            }
                                        });
                                        
                                        ui.horizontal(|ui| {
                                            ui.label("Font:");
                                            egui::ComboBox::from_id_salt("mindspace_font")
                                                .selected_text(format!("{:?}", studio_state.mindspace_font))
                                                .show_ui(ui, |ui| {
                                                    use eustress_common::classes::Font;
                                                    ui.selectable_value(&mut studio_state.mindspace_font, Font::SourceSans, "Source Sans");
                                                    ui.selectable_value(&mut studio_state.mindspace_font, Font::RobotoMono, "Roboto Mono");
                                                    ui.selectable_value(&mut studio_state.mindspace_font, Font::GothamBold, "Gotham Bold");
                                                    ui.selectable_value(&mut studio_state.mindspace_font, Font::GothamLight, "Gotham Light");
                                                    ui.selectable_value(&mut studio_state.mindspace_font, Font::Fantasy, "Fantasy");
                                                    ui.selectable_value(&mut studio_state.mindspace_font, Font::Bangers, "Bangers");
                                                    ui.selectable_value(&mut studio_state.mindspace_font, Font::Merriweather, "Merriweather");
                                                    ui.selectable_value(&mut studio_state.mindspace_font, Font::Nunito, "Nunito");
                                                    ui.selectable_value(&mut studio_state.mindspace_font, Font::Ubuntu, "Ubuntu");
                                                });
                                        });
                                        
                                        ui.horizontal(|ui| {
                                            ui.label("Size:");
                                            ui.add(egui::Slider::new(&mut studio_state.mindspace_font_size, 8.0..=48.0));
                                        });
                                        
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            // Show "Update Label" when editing, "Add Label" when creating new
                                            let button_text = if is_editing { "Update Label" } else { "Add Label" };
                                            if ui.button(button_text).clicked() {
                                                info!("🏷️ MindSpace: {} button clicked", button_text);
                                                if is_editing {
                                                    action_queue.push(UIAction::PluginAction("mindspace:update_label".to_string()));
                                                } else {
                                                    action_queue.push(UIAction::PluginAction("mindspace:add_label".to_string()));
                                                }
                                            }
                                            if ui.button("Remove").clicked() {
                                                info!("🏷️ MindSpace: Remove button clicked");
                                                action_queue.push(UIAction::PluginAction("mindspace:remove_label".to_string()));
                                            }
                                        });
                                    }
                                    MindSpaceMode::Connect => {
                                        // Connect mode UI
                                        ui.label("Connect Nodes");
                                        ui.add_space(8.0);
                                        ui.label("1. Select first node");
                                        ui.label("2. Click 'Set Source'");
                                        ui.label("3. Select second node");
                                        ui.label("4. Click 'Connect'");
                                        
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            if ui.button("Set Source").clicked() {
                                                action_queue.push(UIAction::PluginAction("mindspace:set_source".to_string()));
                                            }
                                            if ui.button("Connect").clicked() {
                                                action_queue.push(UIAction::PluginAction("mindspace:connect".to_string()));
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    });
            });
    }
    
    // RIGHT PANEL - Properties/History/Collaborate
    if studio_state.show_properties {
        egui::SidePanel::right("right_panel")
            .default_width(320.0)
            .min_width(280.0)
            .max_width(500.0)
            .resizable(true)
            .frame(egui::Frame::new()
                .fill(egui::Color32::from_rgb(35, 35, 38))
                .inner_margin(egui::Margin { left: 4, right: 4, top: 4, bottom: 4 })
                .outer_margin(egui::Margin::ZERO)
                .stroke(egui::Stroke::NONE)
                .shadow(egui::Shadow::NONE))
            .show(ctx, |ui| {
                // Tab header row with draggable tabs
                ui.horizontal(|ui| {
                    // Properties tab - draggable
                    let props_response = ui.selectable_label(right_tab == dock::RightTab::Properties, "⚙ Properties");
                    if props_response.clicked() {
                        dock_state.right_tab = dock::RightTab::Properties;
                    }
                    if props_response.drag_started() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.start_drag(PanelId::Properties, pos);
                        }
                    }
                    if props_response.dragged() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.update_drag(pos);
                        }
                    }
                    
                    // History tab - draggable
                    let history_response = ui.selectable_label(right_tab == dock::RightTab::History, "📜 History");
                    if history_response.clicked() {
                        dock_state.right_tab = dock::RightTab::History;
                    }
                    if history_response.drag_started() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.start_drag(PanelId::History, pos);
                        }
                    }
                    if history_response.dragged() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.update_drag(pos);
                        }
                    }
                    
                    // Collaborate tab - draggable
                    let collab_response = ui.selectable_label(right_tab == dock::RightTab::Collaborate, "👥 Collaborate");
                    if collab_response.clicked() {
                        dock_state.right_tab = dock::RightTab::Collaborate;
                    }
                    if collab_response.drag_started() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.start_drag(PanelId::Collaborate, pos);
                        }
                    }
                    if collab_response.dragged() {
                        if let Some(pos) = ui.ctx().pointer_interact_pos() {
                            drag_state.update_drag(pos);
                        }
                    }
                    
                    // Close button
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("X").on_hover_text("Close Panel (View menu to reopen)").clicked() {
                            studio_state.show_properties = false;
                        }
                    });
                });
                ui.separator();
            
            match right_tab {
                dock::RightTab::Properties => {
                    show_properties_panel(ui, &world_snapshot, &mut action_queue, expanded.selected_service, &mut soul_settings, &mut workspace_res, &mut workspace_ui, &mut players_service, &mut lighting_service, &mut sound_service);
                }
                dock::RightTab::History => {
                    // Undo/redo history panel
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let mut undo_requested = false;
                        let mut redo_requested = false;
                        HistoryPanel::show_with_undo_stack(ui, &undo_stack, &mut undo_requested, &mut redo_requested);
                        if undo_requested {
                            undo_events.write(crate::undo::UndoEvent);
                        }
                        if redo_requested {
                            redo_events.write(crate::undo::RedoEvent);
                        }
                    });
                }
                dock::RightTab::Collaborate => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        CollaborationPanel::show_content(ui, &mut collab_state);
                    });
                }
            }
            });
    }
    
    // CENTRAL PANEL - Script Editor (only when a script is active)
    // This replaces the 3D viewport area when editing scripts
    if script_editor_active {
        egui::CentralPanel::default()
            .frame(egui::Frame::new()
                .fill(egui::Color32::from_rgb(30, 30, 30))
                .stroke(egui::Stroke::NONE)
                .inner_margin(egui::Margin { left: 8, right: 8, top: 0, bottom: 4 })
                .outer_margin(egui::Margin::ZERO))
            .show(ctx, |ui| {
                if let Some(active_tab) = script_editor_state.tabs.get(script_editor_state.active_tab).cloned() {
                    if let Some(entity) = active_tab.entity {
                        // Render content based on tab type
                        match &active_tab.tab_type {
                            script_editor::EditorTabType::SoulScript => {
                                render_soul_script_editor(ui, ctx, &mut script_editor_state, &active_tab, entity);
                            }
                            script_editor::EditorTabType::ParametersEditor => {
                                render_parameters_editor_tab(ui, ctx, &active_tab, entity, &world_snapshot);
                            }
                            script_editor::EditorTabType::Document { doc_type } => {
                                render_document_viewer_tab(ui, &active_tab, entity, doc_type);
                            }
                            script_editor::EditorTabType::ImageViewer => {
                                render_image_viewer_tab(ui, &active_tab, entity);
                            }
                            script_editor::EditorTabType::VideoPlayer => {
                                render_video_player_tab(ui, &active_tab, entity);
                            }
                            script_editor::EditorTabType::WebBrowser => {
                                render_web_browser_tab(ui, script_editor_state.active_tab, &mut browser_state, &mut webview_manager, &mut cef_state);
                            }
                            script_editor::EditorTabType::Scene => {
                                // Scene tab should not reach here (is_scene_active would be true)
                            }
                        }
                    }
                }
            });
    }
    
    // Render properties panel modals (Add Tag, Add Attribute, Add Parameters)
    render_properties_modals(ctx, &mut modal_state, &mut action_queue);
}

/// Render the Soul Script editor content
fn render_soul_script_editor(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    script_editor_state: &mut script_editor::ScriptEditorState,
    active_tab: &script_editor::EditorTab,
    entity: Entity,
) {
                        // Header with 4px left padding
                        ui.horizontal(|ui| {
                            ui.add_space(4.0);  // Left padding for header
                            ui.label(egui::RichText::new(format!("📝 {}", active_tab.name)).strong().size(16.0));
                            ui.separator();
                            
                            // Build button - sends TriggerBuildEvent
                            // Check if build is in progress via temp data
                            let build_status: Option<String> = ctx.data(|d| {
                                d.get_temp(egui::Id::new("build_status"))
                            });
                            let is_building = build_status.is_some();
                            
                            if is_building {
                                // Show spinner and status during build
                                let status = build_status.unwrap_or_default();
                                ui.add(egui::Spinner::new());
                                ui.label(egui::RichText::new(&status).color(egui::Color32::from_rgb(100, 180, 255)));
                            } else {
                                if ui.button("🔨 Build").clicked() {
                                    // Send build event to trigger Soul script compilation
                                    info!("🔨 Build requested for SoulScript entity {:?}", entity);
                                    // Queue the build event via commands
                                    ctx.data_mut(|d| {
                                        d.insert_temp(egui::Id::new("pending_build_entity"), entity);
                                    });
                                }
                            }
                            
                            // Status indicator
                            if active_tab.dirty {
                                ui.label(egui::RichText::new("[*] Modified").color(egui::Color32::YELLOW));
                            } else if !is_building {
                                ui.label(egui::RichText::new("[OK]").color(egui::Color32::from_rgb(100, 200, 100)));
                            }
                            
                            // Push toggle to right corner
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // View mode toggle - Markdown | Rune (reversed order for right-to-left)
                                let rune_selected = script_editor_state.view_mode == script_editor::ScriptViewMode::Rune;
                                let md_selected = script_editor_state.view_mode == script_editor::ScriptViewMode::Markdown;
                                
                                if ui.selectable_label(rune_selected, "{ } Rune").clicked() {
                                    script_editor_state.view_mode = script_editor::ScriptViewMode::Rune;
                                }
                                if ui.selectable_label(md_selected, "# Markdown").clicked() {
                                    script_editor_state.view_mode = script_editor::ScriptViewMode::Markdown;
                                }
                            });
                        });
                        
                        ui.add_space(4.0);
                        
                        // Get or create edit buffer - use entity-specific ID for stable focus
                        let editor_id = egui::Id::new(("script_editor", entity));
                        
                        // Show content based on view mode
                        match script_editor_state.view_mode {
                            script_editor::ScriptViewMode::Markdown => {
                                // Clone source for editing to avoid borrow issues
                                let mut source = script_editor_state.edit_buffers.get(&entity).cloned().unwrap_or_default();
                                let original_source = source.clone();
                                
                                // Calculate line count
                                let line_count = source.lines().count().max(1);
                                
                                // Editor with line numbers
                                egui::ScrollArea::vertical()
                                    .id_salt(editor_id)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.horizontal_top(|ui| {
                                            // Line numbers column (fixed width)
                                            let line_num_width = 45.0;
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(line_num_width, ui.available_height()),
                                                egui::Layout::top_down(egui::Align::RIGHT),
                                                |ui| {
                                                    ui.style_mut().spacing.item_spacing.y = 0.0;
                                                    let line_height = ui.text_style_height(&egui::TextStyle::Monospace);
                                                    for i in 1..=line_count {
                                                        ui.add_sized(
                                                            [line_num_width - 8.0, line_height],
                                                            egui::Label::new(
                                                                egui::RichText::new(format!("{}", i))
                                                                    .monospace()
                                                                    .color(egui::Color32::from_rgb(100, 100, 110))
                                                            )
                                                        );
                                                    }
                                                }
                                            );
                                            
                                            // Separator line
                                            ui.add_space(4.0);
                                            let rect = ui.available_rect_before_wrap();
                                            ui.painter().vline(
                                                rect.left(),
                                                rect.y_range(),
                                                egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 65))
                                            );
                                            ui.add_space(8.0);
                                            
                                            // Code editor
                                            egui::TextEdit::multiline(&mut source)
                                                .id(editor_id.with("text"))
                                                .font(egui::TextStyle::Monospace)
                                                .code_editor()
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(40)
                                                .show(ui);
                                        });
                                    });
                                
                                // Update buffer and mark dirty if changed
                                if source != original_source {
                                    script_editor_state.edit_buffers.insert(entity, source);
                                    script_editor_state.mark_dirty(entity);
                                }
                            }
                            script_editor::ScriptViewMode::Rune => {
                                // Show generated Rune code (read-only)
                                // Need to get generated code from SoulScriptData
                                let generated_code: Option<String> = ctx.data(|d| {
                                    d.get_temp(egui::Id::new(("generated_code", entity)))
                                });
                                
                                if let Some(code) = generated_code {
                                    let line_count = code.lines().count().max(1);
                                    
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Generated Rune Code").small().color(egui::Color32::GRAY));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.small_button("📋 Copy").clicked() {
                                                ui.ctx().copy_text(code.clone());
                                            }
                                        });
                                    });
                                    
                                    egui::ScrollArea::vertical()
                                        .id_salt(editor_id.with("rune"))
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            ui.horizontal_top(|ui| {
                                                // Line numbers
                                                let line_num_width = 45.0;
                                                ui.allocate_ui_with_layout(
                                                    egui::vec2(line_num_width, ui.available_height()),
                                                    egui::Layout::top_down(egui::Align::RIGHT),
                                                    |ui| {
                                                        ui.style_mut().spacing.item_spacing.y = 0.0;
                                                        let line_height = ui.text_style_height(&egui::TextStyle::Monospace);
                                                        for i in 1..=line_count {
                                                            ui.add_sized(
                                                                [line_num_width - 8.0, line_height],
                                                                egui::Label::new(
                                                                    egui::RichText::new(format!("{}", i))
                                                                        .monospace()
                                                                        .color(egui::Color32::from_rgb(100, 100, 110))
                                                                )
                                                            );
                                                        }
                                                    }
                                                );
                                                
                                                ui.add_space(4.0);
                                                let rect = ui.available_rect_before_wrap();
                                                ui.painter().vline(
                                                    rect.left(),
                                                    rect.y_range(),
                                                    egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 65))
                                                );
                                                ui.add_space(8.0);
                                                
                                                // Read-only code display
                                                let mut code_display = code.clone();
                                                egui::TextEdit::multiline(&mut code_display)
                                                    .font(egui::TextStyle::Monospace)
                                                    .code_editor()
                                                    .desired_width(f32::INFINITY)
                                                    .desired_rows(40)
                                                    .interactive(false)
                                                    .show(ui);
                                            });
                                        });
                                } else {
                                    ui.centered_and_justified(|ui| {
                                        ui.label(egui::RichText::new("No Rune code generated yet.\nClick 'Build' to generate code from your Markdown.")
                                            .color(egui::Color32::GRAY));
                                    });
                                }
                            }
                        }
}

/// Render the Parameters Editor tab content
fn render_parameters_editor_tab(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    _active_tab: &script_editor::EditorTab,
    entity: Entity,
    snapshot: &UIWorldSnapshot,
) {
    use eustress_common::parameters::{DataSourceType, AuthType, AnonymizationMode, UpdateMode};
    
    // Get entity data from snapshot
    let entity_data = snapshot.get(entity);
    let has_parameters = entity_data.map(|e| e.has_parameters).unwrap_or(false);
    let _entity_name = entity_data.map(|e| e.name.clone()).unwrap_or_else(|| "Unknown".to_string());
    
    // ════════════════════════════════════════════════════════════════
    // Hierarchy Tabs: Global Sources | Domains | Instance
    // ════════════════════════════════════════════════════════════════
    let current_view: String = ctx.data(|d| {
        d.get_temp(egui::Id::new("params_editor_view"))
    }).unwrap_or_else(|| "instance".to_string());
    
    ui.horizontal(|ui| {
        let views = [
            ("🌐 Global Sources", "global"),
            ("📂 Domains", "domains"),
            ("📍 Instance", "instance"),
        ];
        for (label, view_id) in views {
            if ui.selectable_label(current_view == view_id, label).clicked() {
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("params_editor_view"), view_id.to_string());
                });
            }
        }
    });
    ui.separator();
    
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        
        match current_view.as_str() {
            "global" => render_global_sources_view(ui, ctx),
            "domains" => render_domains_view(ui, ctx),
            _ => render_instance_view(ui, ctx, entity, has_parameters, entity_data.and_then(|e| e.data_source_type)),
        }
        
        ui.add_space(40.0);
    });
}

/// Render the Global Sources management view (in-panel version)
fn render_global_sources_view(ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.heading("Global Data Sources");
    ui.weak("Define connection endpoints shared across all entities.");
    ui.add_space(12.0);
    
    ui.label("Use Data > Manage Global Sources... from the menu to configure sources.");
    ui.add_space(8.0);
    
    // Show hint to use the main window
    if ui.button("Open Global Sources Window").clicked() {
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::new("open_global_sources_window"), true);
        });
    }
}

/// Render the Domains configuration view (in-panel version)
fn render_domains_view(ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.heading("Domain Configurations");
    ui.weak("Define how entity types connect to global sources.");
    ui.add_space(12.0);
    
    ui.label("Use Data > Manage Domains... from the menu to configure domains.");
    ui.add_space(8.0);
    
    // Show hint to use the main window
    if ui.button("Open Domains Window").clicked() {
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::new("open_domains_window"), true);
        });
    }
}

/// Render the Instance-level configuration view
fn render_instance_view(ui: &mut egui::Ui, ctx: &egui::Context, entity: Entity, has_parameters: bool, actual_source_type: Option<eustress_common::parameters::DataSourceType>) {
    use eustress_common::parameters::{DataSourceType, AuthType, AnonymizationMode};
    
    // Status Banner - use ASCII checkmark to avoid font issues
    if has_parameters {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("[OK]").color(egui::Color32::from_rgb(100, 200, 100)).strong());
            ui.label(egui::RichText::new("Data source configured").color(egui::Color32::from_rgb(152, 195, 121)));
        });
    } else {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("[ ]").color(egui::Color32::GRAY));
            ui.label(egui::RichText::new("No data source configured").weak());
        });
    }
    ui.add_space(12.0);
        
        // ════════════════════════════════════════════════════════════════
        // Data Source Type Selection
        // ════════════════════════════════════════════════════════════════
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("📡 Data Source Type").strong().size(14.0));
            });
            ui.add_space(4.0);
            
            // Get current selection - prefer actual entity value, fall back to temp storage for UI changes
            let current_type: DataSourceType = actual_source_type.unwrap_or_else(|| {
                ctx.data(|d| {
                    d.get_temp(egui::Id::new(("params_source_type", entity)))
                }).unwrap_or(DataSourceType::None)
            });
            
            // Category tabs
            let categories = ["API", "File", "Database", "Messaging", "Cloud", "Healthcare", "Industrial/IoT", "Other"];
            
            ui.horizontal_wrapped(|ui| {
                for cat in categories {
                    let is_selected = current_type.category() == cat || (cat == "API" && current_type == DataSourceType::None);
                    if ui.selectable_label(is_selected, cat).clicked() {
                        // Just for visual feedback - actual selection happens below
                    }
                }
            });
            
            ui.add_space(8.0);
            
            // Show data sources for current category
            egui::Grid::new("data_source_grid")
                .num_columns(3)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    for source_type in DataSourceType::all_variants() {
                        if *source_type == DataSourceType::None {
                            continue;
                        }
                        
                        let is_selected = current_type == *source_type;
                        let btn = ui.selectable_label(is_selected, source_type.display_name());
                        
                        if btn.clicked() {
                            ctx.data_mut(|d| {
                                d.insert_temp(egui::Id::new(("params_source_type", entity)), *source_type);
                            });
                        }
                    }
                });
        });
        
        ui.add_space(12.0);
        
        // ════════════════════════════════════════════════════════════════
        // Connection Settings
        // ════════════════════════════════════════════════════════════════
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🔌 Connection Settings").strong().size(14.0));
            });
            ui.add_space(4.0);
            
            // Endpoint URL
            let mut endpoint: String = ctx.data(|d| {
                d.get_temp(egui::Id::new(("params_endpoint", entity)))
            }).unwrap_or_default();
            
            ui.horizontal(|ui| {
                ui.label("Endpoint URL:");
                if ui.add(egui::TextEdit::singleline(&mut endpoint).desired_width(400.0).hint_text("https://api.example.com/v1")).changed() {
                    ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::new(("params_endpoint", entity)), endpoint.clone());
                    });
                }
            });
            
            // Resource ID
            let mut resource_id: String = ctx.data(|d| {
                d.get_temp(egui::Id::new(("params_resource_id", entity)))
            }).unwrap_or_default();
            
            ui.horizontal(|ui| {
                ui.label("Resource ID:");
                if ui.add(egui::TextEdit::singleline(&mut resource_id).desired_width(200.0).hint_text("item-001")).changed() {
                    ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::new(("params_resource_id", entity)), resource_id.clone());
                    });
                }
            });
            
            // Domain
            let mut domain: String = ctx.data(|d| {
                d.get_temp(egui::Id::new(("params_domain", entity)))
            }).unwrap_or_default();
            
            ui.horizontal(|ui| {
                ui.label("Domain:");
                if ui.add(egui::TextEdit::singleline(&mut domain).desired_width(200.0).hint_text("Object, Entity, Item...")).changed() {
                    ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::new(("params_domain", entity)), domain.clone());
                    });
                }
            });
        });
        
        ui.add_space(12.0);
        
        // ════════════════════════════════════════════════════════════════
        // Authentication
        // ════════════════════════════════════════════════════════════════
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🔐 Authentication").strong().size(14.0));
            });
            ui.add_space(4.0);
            
            let current_auth: AuthType = ctx.data(|d| {
                d.get_temp(egui::Id::new(("params_auth_type", entity)))
            }).unwrap_or(AuthType::None);
            
            ui.horizontal(|ui| {
                ui.label("Auth Type:");
                egui::ComboBox::from_id_salt("auth_type_combo")
                    .selected_text(current_auth.display_name())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for auth in AuthType::all_variants() {
                            if ui.selectable_label(current_auth == *auth, auth.display_name()).clicked() {
                                ctx.data_mut(|d| {
                                    d.insert_temp(egui::Id::new(("params_auth_type", entity)), auth.clone());
                                });
                            }
                        }
                    });
            });
            
            // Show auth-specific fields
            match current_auth {
                AuthType::Bearer | AuthType::APIKey => {
                    let mut token_ref: String = ctx.data(|d| {
                        d.get_temp(egui::Id::new(("params_token_ref", entity)))
                    }).unwrap_or_default();
                    
                    ui.horizontal(|ui| {
                        ui.label("Token/Key Reference:");
                        if ui.add(egui::TextEdit::singleline(&mut token_ref).desired_width(300.0).hint_text("ENV:API_KEY or keychain:my-api-key")).changed() {
                            ctx.data_mut(|d| {
                                d.insert_temp(egui::Id::new(("params_token_ref", entity)), token_ref.clone());
                            });
                        }
                    });
                }
                AuthType::Basic => {
                    ui.weak("Configure username/password via secure credential storage");
                }
                AuthType::OAuth2 => {
                    ui.weak("Configure OAuth2 client ID, secret, and token endpoint");
                }
                _ => {}
            }
        });
        
        ui.add_space(12.0);
        
        // ════════════════════════════════════════════════════════════════
        // Update Strategy
        // ════════════════════════════════════════════════════════════════
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🔄 Update Strategy").strong().size(14.0));
            });
            ui.add_space(4.0);
            
            let update_modes = ["Manual", "Polling", "Webhook", "Streaming", "Event-Driven"];
            let mut current_mode: String = ctx.data(|d| {
                d.get_temp(egui::Id::new(("params_update_mode", entity)))
            }).unwrap_or_else(|| "Manual".to_string());
            
            ui.horizontal(|ui| {
                ui.label("Mode:");
                egui::ComboBox::from_id_salt("update_mode_combo")
                    .selected_text(&current_mode)
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for mode in update_modes {
                            if ui.selectable_label(current_mode == mode, mode).clicked() {
                                ctx.data_mut(|d| {
                                    d.insert_temp(egui::Id::new(("params_update_mode", entity)), mode.to_string());
                                });
                            }
                        }
                    });
            });
            
            // Mode-specific settings
            if current_mode == "Polling" {
                let mut interval: f32 = ctx.data(|d| {
                    d.get_temp(egui::Id::new(("params_poll_interval", entity)))
                }).unwrap_or(60.0);
                
                ui.horizontal(|ui| {
                    ui.label("Poll Interval (seconds):");
                    if ui.add(egui::DragValue::new(&mut interval).range(1.0..=3600.0).speed(1.0)).changed() {
                        ctx.data_mut(|d| {
                            d.insert_temp(egui::Id::new(("params_poll_interval", entity)), interval);
                        });
                    }
                });
            }
        });
        
        ui.add_space(12.0);
        
        // ════════════════════════════════════════════════════════════════
        // Privacy & Compliance (Healthcare-specific)
        // ════════════════════════════════════════════════════════════════
        let current_type: DataSourceType = ctx.data(|d| {
            d.get_temp(egui::Id::new(("params_source_type", entity)))
        }).unwrap_or(DataSourceType::None);
        
        if current_type.category() == "Healthcare" {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🏥 Privacy & Compliance").strong().size(14.0));
                });
                ui.add_space(4.0);
                
                let anon_modes = [
                    ("None (Real Data)", AnonymizationMode::None),
                    ("Hash Identifiers", AnonymizationMode::Hash),
                    ("Synthetic Data", AnonymizationMode::Synthetic),
                    ("Redact/Mask", AnonymizationMode::Redact),
                ];
                
                let current_anon: AnonymizationMode = ctx.data(|d| {
                    d.get_temp(egui::Id::new(("params_anon_mode", entity)))
                }).unwrap_or(AnonymizationMode::None);
                
                ui.horizontal(|ui| {
                    ui.label("PHI Protection:");
                    egui::ComboBox::from_id_salt("anon_mode_combo")
                        .selected_text(current_anon.display_name())
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (name, mode) in anon_modes {
                                if ui.selectable_label(current_anon == mode, name).clicked() {
                                    ctx.data_mut(|d| {
                                        d.insert_temp(egui::Id::new(("params_anon_mode", entity)), mode);
                                    });
                                }
                            }
                        });
                });
                
                ui.add_space(4.0);
                ui.weak("⚠ HIPAA/GDPR: Ensure proper data handling agreements are in place");
            });
            
            ui.add_space(12.0);
        }
        
        // ════════════════════════════════════════════════════════════════
        // Field Mappings (Source Path -> Property/Attribute)
        // ════════════════════════════════════════════════════════════════
        ui.group(|ui| {
            use eustress_common::parameters::MappingTargetType;
            
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🗺 Field Mappings").strong().size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("+ Add Mapping").clicked() {
                        // Get current mappings count and add a new empty one
                        let count: usize = ctx.data(|d| {
                            d.get_temp(egui::Id::new(("params_mapping_count", entity)))
                        }).unwrap_or(0);
                        ctx.data_mut(|d| {
                            d.insert_temp(egui::Id::new(("params_mapping_count", entity)), count + 1);
                            d.insert_temp(egui::Id::new(("params_mapping_source", entity, count)), String::new());
                            d.insert_temp(egui::Id::new(("params_mapping_target", entity, count)), String::new());
                            d.insert_temp(egui::Id::new(("params_mapping_type", entity, count)), MappingTargetType::Attribute);
                            d.insert_temp(egui::Id::new(("params_mapping_transform", entity, count)), String::new());
                        });
                    }
                });
            });
            ui.add_space(4.0);
            
            ui.weak("Map external data to Attributes, Colors, Physics, and more");
            ui.add_space(4.0);
            
            // Get current mappings
            let mapping_count: usize = ctx.data(|d| {
                d.get_temp(egui::Id::new(("params_mapping_count", entity)))
            }).unwrap_or(0);
            
            // Header row
            egui::Grid::new("field_mappings_grid")
                .num_columns(6)
                .spacing([6.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Source").strong());
                    ui.label(egui::RichText::new("->").strong());
                    ui.label(egui::RichText::new("Target Type").strong());
                    ui.label(egui::RichText::new("Name/Key").strong());
                    ui.label(egui::RichText::new("Transform").strong());
                    ui.label("");
                    ui.end_row();
                    
                    // Get discovered schema fields for dropdown (if available)
                    let domain: String = ctx.data(|d| {
                        d.get_temp(egui::Id::new(("params_domain", entity)))
                    }).unwrap_or_default();
                    let discovered_fields: Vec<String> = ctx.data(|d| {
                        d.get_temp(egui::Id::new(("schema_fields", &domain)))
                    }).unwrap_or_default();
                    let has_schema = !discovered_fields.is_empty();
                    
                    // Render each mapping
                    let mut to_remove: Option<usize> = None;
                    for i in 0..mapping_count {
                        let source: String = ctx.data(|d| {
                            d.get_temp(egui::Id::new(("params_mapping_source", entity, i)))
                        }).unwrap_or_default();
                        let mut target: String = ctx.data(|d| {
                            d.get_temp(egui::Id::new(("params_mapping_target", entity, i)))
                        }).unwrap_or_default();
                        let target_type: MappingTargetType = ctx.data(|d| {
                            d.get_temp(egui::Id::new(("params_mapping_type", entity, i)))
                        }).unwrap_or(MappingTargetType::Attribute);
                        let mut transform: String = ctx.data(|d| {
                            d.get_temp(egui::Id::new(("params_mapping_transform", entity, i)))
                        }).unwrap_or_default();
                        
                        // Source path - ComboBox if schema available, TextEdit otherwise
                        if has_schema {
                            egui::ComboBox::from_id_salt(format!("source_field_{}", i))
                                .selected_text(if source.is_empty() { "Select..." } else { &source })
                                .width(100.0)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(false, "📝 Custom...").clicked() {}
                                    ui.separator();
                                    for field in &discovered_fields {
                                        if ui.selectable_label(source == *field, field).clicked() {
                                            ctx.data_mut(|d| {
                                                d.insert_temp(egui::Id::new(("params_mapping_source", entity, i)), field.clone());
                                            });
                                        }
                                    }
                                });
                        } else {
                            let mut src = source.clone();
                            if ui.add(egui::TextEdit::singleline(&mut src)
                                .desired_width(100.0)
                                .hint_text("data.field")).changed() {
                                ctx.data_mut(|d| {
                                    d.insert_temp(egui::Id::new(("params_mapping_source", entity, i)), src);
                                });
                            }
                        }
                        
                        ui.label("->");
                        
                        // Target type dropdown (grouped by category)
                        egui::ComboBox::from_id_salt(format!("target_type_{}", i))
                            .selected_text(target_type.display_name())
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                let mut current_cat = "";
                                for tt in MappingTargetType::all_variants() {
                                    let cat = tt.category();
                                    if cat != current_cat {
                                        if !current_cat.is_empty() { ui.separator(); }
                                        ui.label(egui::RichText::new(cat).small().weak());
                                        current_cat = cat;
                                    }
                                    if ui.selectable_label(target_type == *tt, tt.display_name()).clicked() {
                                        ctx.data_mut(|d| {
                                            d.insert_temp(egui::Id::new(("params_mapping_type", entity, i)), *tt);
                                        });
                                    }
                                }
                            });
                        
                        // Target name/key (only for Attribute type, others are implicit)
                        let needs_name = target_type == MappingTargetType::Attribute;
                        if needs_name {
                            if ui.add(egui::TextEdit::singleline(&mut target)
                                .desired_width(80.0)
                                .hint_text("AttrName")).changed() {
                                ctx.data_mut(|d| {
                                    d.insert_temp(egui::Id::new(("params_mapping_target", entity, i)), target.clone());
                                });
                            }
                        } else {
                            ui.label(egui::RichText::new("(auto)").weak());
                        }
                        
                        // Transform expression (for conditional logic)
                        let transform_hint = match target_type {
                            MappingTargetType::Color => "value ? 'green' : 'red'",
                            MappingTargetType::Anchored | MappingTargetType::CanCollide | 
                            MappingTargetType::CanTouch | MappingTargetType::Locked |
                            MappingTargetType::Visible => "value == 'active'",
                            MappingTargetType::Transparency | MappingTargetType::Reflectance => "value / 100",
                            _ => "optional",
                        };
                        if ui.add(egui::TextEdit::singleline(&mut transform)
                            .desired_width(100.0)
                            .hint_text(transform_hint)).changed() {
                            ctx.data_mut(|d| {
                                d.insert_temp(egui::Id::new(("params_mapping_transform", entity, i)), transform.clone());
                            });
                        }
                        
                        // Delete button
                        if ui.small_button("X").clicked() {
                            to_remove = Some(i);
                        }
                        ui.end_row();
                    }
                    
                    // Handle removal (shift remaining mappings down)
                    if let Some(remove_idx) = to_remove {
                        ctx.data_mut(|d| {
                            // Shift all mappings after remove_idx down by one
                            for j in remove_idx..mapping_count - 1 {
                                let next_source: String = d.get_temp(egui::Id::new(("params_mapping_source", entity, j + 1))).unwrap_or_default();
                                let next_target: String = d.get_temp(egui::Id::new(("params_mapping_target", entity, j + 1))).unwrap_or_default();
                                let next_type: MappingTargetType = d.get_temp(egui::Id::new(("params_mapping_type", entity, j + 1))).unwrap_or(MappingTargetType::Attribute);
                                let next_transform: String = d.get_temp(egui::Id::new(("params_mapping_transform", entity, j + 1))).unwrap_or_default();
                                d.insert_temp(egui::Id::new(("params_mapping_source", entity, j)), next_source);
                                d.insert_temp(egui::Id::new(("params_mapping_target", entity, j)), next_target);
                                d.insert_temp(egui::Id::new(("params_mapping_type", entity, j)), next_type);
                                d.insert_temp(egui::Id::new(("params_mapping_transform", entity, j)), next_transform);
                            }
                            // Remove the last one and decrement count
                            d.remove::<String>(egui::Id::new(("params_mapping_source", entity, mapping_count - 1)));
                            d.remove::<String>(egui::Id::new(("params_mapping_target", entity, mapping_count - 1)));
                            d.insert_temp(egui::Id::new(("params_mapping_count", entity)), mapping_count - 1);
                        });
                    }
                });
            
            // Show hint if no mappings
            if mapping_count == 0 {
                ui.add_space(4.0);
                ui.weak("No mappings configured. Click '+ Add Mapping' to map source fields to Attributes.");
            }
        });
        
        ui.add_space(20.0);
        
        // ════════════════════════════════════════════════════════════════
        // Action Buttons
        // ════════════════════════════════════════════════════════════════
        ui.horizontal(|ui| {
            if ui.button("💾 Save Configuration").clicked() {
                // TODO: Save parameters to entity
            }
            if ui.button("🔍 Test Connection").clicked() {
                // TODO: Test the data source connection
            }
            if ui.button("📥 Fetch Sample Data").clicked() {
                // TODO: Fetch sample data from source
            }
        });
        
    ui.add_space(40.0);
}

/// Render the Document viewer tab content
fn render_document_viewer_tab(
    ui: &mut egui::Ui,
    active_tab: &script_editor::EditorTab,
    entity: Entity,
    doc_type: &script_editor::DocumentType,
) {
    let icon = match doc_type {
        script_editor::DocumentType::Pdf => "📕",
        script_editor::DocumentType::Docx => "📘",
        script_editor::DocumentType::Pptx => "📙",
        script_editor::DocumentType::Xlsx => "📗",
        script_editor::DocumentType::GoogleDoc => "📄",
        script_editor::DocumentType::GoogleSheet => "📊",
        script_editor::DocumentType::GoogleSlides => "📽",
        script_editor::DocumentType::Text => "📝",
    };
    
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("{} {}", icon, active_tab.name)).strong().size(16.0));
    });
    ui.separator();
    
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(100.0);
            ui.label(egui::RichText::new(format!("{}", icon)).size(64.0));
            ui.add_space(16.0);
            ui.label(egui::RichText::new(&active_tab.name).size(20.0));
            ui.add_space(8.0);
            ui.label(format!("Document Type: {:?}", doc_type));
            ui.label(format!("Entity: {:?}", entity));
            ui.add_space(16.0);
            ui.weak("Document viewer coming soon");
            ui.weak("Supports PDF, DOCX, PPTX, XLSX, and Google Docs");
        });
    });
}

/// Render the Image viewer tab content
fn render_image_viewer_tab(
    ui: &mut egui::Ui,
    active_tab: &script_editor::EditorTab,
    entity: Entity,
) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("🖼 {}", active_tab.name)).strong().size(16.0));
    });
    ui.separator();
    
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(100.0);
            ui.label(egui::RichText::new("🖼").size(64.0));
            ui.add_space(16.0);
            ui.label(egui::RichText::new(&active_tab.name).size(20.0));
            ui.label(format!("Entity: {:?}", entity));
            ui.add_space(16.0);
            ui.weak("Image viewer coming soon");
            ui.weak("Supports PNG, JPG, GIF, WebP, SVG");
        });
    });
}

/// Render the Video player tab content
fn render_video_player_tab(
    ui: &mut egui::Ui,
    active_tab: &script_editor::EditorTab,
    entity: Entity,
) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("🎥 {}", active_tab.name)).strong().size(16.0));
    });
    ui.separator();
    
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(100.0);
            ui.label(egui::RichText::new("🎥").size(64.0));
            ui.add_space(16.0);
            ui.label(egui::RichText::new(&active_tab.name).size(20.0));
            ui.label(format!("Entity: {:?}", entity));
            ui.add_space(16.0);
            ui.weak("Video player coming soon");
            ui.weak("Supports MP4, WebM, and streaming");
        });
    });
}

/// Render the Web Browser tab content
fn render_web_browser_tab(
    ui: &mut egui::Ui,
    tab_idx: usize,
    browser_state: &mut BrowserState,
    webview_manager: &mut webview::WebViewManager,
    cef_state: &mut cef_browser::CefBrowserState,
) {
    // Get the content area rect for positioning the webview
    let _content_rect = ui.available_rect_before_wrap();
    
    // Browser controls (URL bar, navigation)
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        
        let tab_state = browser_state.tab_states.entry(tab_idx).or_default();
        
        // Back button
        ui.add_enabled_ui(tab_state.can_go_back, |ui| {
            if ui.button("◀").on_hover_text("Go back").clicked() {
                tab_state.go_back();
                webview_manager.navigate(&tab_state.url);
            }
        });
        
        // Forward button
        ui.add_enabled_ui(tab_state.can_go_forward, |ui| {
            if ui.button("▶").on_hover_text("Go forward").clicked() {
                tab_state.go_forward();
                webview_manager.navigate(&tab_state.url);
            }
        });
        
        // Refresh button
        if ui.button("↻").on_hover_text("Refresh").clicked() {
            tab_state.refresh();
            webview_manager.navigate(&tab_state.url);
        }
        
        // Home button
        if ui.button("🏠").on_hover_text("Home").clicked() {
            let home = if browser_state.home_page.is_empty() {
                "https://docs.eustress.dev".to_string()
            } else {
                browser_state.home_page.clone()
            };
            tab_state.navigate(&home);
            webview_manager.navigate(&home);
        }
        
        ui.add_space(8.0);
        
        // URL bar with Enter to navigate
        let url_response = ui.add_enabled_ui(!tab_state.loading, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut tab_state.url_bar_text)
                    .desired_width(ui.available_width() - 100.0)
                    .hint_text("Enter URL...")
            )
        });
        
        // Navigate on Enter key
        if url_response.inner.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let url = tab_state.url_bar_text.clone();
            // Add https:// if no protocol specified
            let url = if !url.starts_with("http://") && !url.starts_with("https://") {
                format!("https://{}", url)
            } else {
                url
            };
            tab_state.navigate(&url);
            webview_manager.navigate(&url);
        }
        
        // Go button
        if ui.button("Go").clicked() {
            let url = tab_state.url_bar_text.clone();
            let url = if !url.starts_with("http://") && !url.starts_with("https://") {
                format!("https://{}", url)
            } else {
                url
            };
            tab_state.navigate(&url);
            webview_manager.navigate(&url);
        }
        
        // Open in external browser button
        if ui.button("↗").on_hover_text("Open in system browser").clicked() {
            let _ = open::that(&tab_state.url);
        }
        
        // Bookmark button
        if ui.button("☆").on_hover_text("Bookmark this page").clicked() {
            browser_state.bookmarks.quick_access.push(Bookmark {
                title: tab_state.title.clone(),
                url: tab_state.url.clone(),
                favicon: None,
            });
        }
    });
    
    ui.separator();
    
    // Bookmarks bar (if enabled and has bookmarks)
    if browser_state.show_bookmarks_bar && !browser_state.bookmarks.quick_access.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            for bookmark in browser_state.bookmarks.quick_access.clone() {
                if ui.small_button(&bookmark.title).on_hover_text(&bookmark.url).clicked() {
                    if let Some(state) = browser_state.tab_states.get_mut(&tab_idx) {
                        state.navigate(&bookmark.url);
                    }
                }
            }
        });
        ui.separator();
    }
    
    // Browser content area - track bounds for future webview integration
    let webview_rect = ui.available_rect_before_wrap();
    let bounds = webview::WebViewBounds {
        x: webview_rect.min.x,
        y: webview_rect.min.y,
        width: webview_rect.width(),
        height: webview_rect.height(),
    };
    
    // Get current state
    let tab_state = browser_state.tab_states.get(&tab_idx);
    
    // Update webview manager state (for future native webview integration)
    if let Some(state) = tab_state {
        if state.url != "about:blank" {
            webview_manager.show_browser(&state.url, bounds);
        } else {
            webview_manager.hide_browser();
        }
    }
    
    // Trigger content fetch if needed
    if let Some(tab_state) = browser_state.tab_states.get_mut(&tab_idx) {
        if tab_state.needs_fetch {
            tab_state.fetch_content();
        }
    }
    
    // Re-get tab state after potential mutation
    let tab_state = browser_state.tab_states.get(&tab_idx);
    
    if let Some(state) = tab_state {
        if state.loading {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.spinner();
                    ui.label("Loading...");
                });
            });
        } else if let Some(error) = &state.error {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(50.0);
                    ui.label(egui::RichText::new("⚠ Error").size(24.0).color(egui::Color32::RED));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(error).color(egui::Color32::LIGHT_RED));
                    ui.add_space(20.0);
                    if ui.button("Open in System Browser").clicked() {
                        let _ = open::that(&state.url);
                    }
                });
            });
        } else if state.url == "about:blank" {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.label(egui::RichText::new("New Tab").size(24.0).color(egui::Color32::GRAY));
                    ui.add_space(20.0);
                    ui.label("Enter a URL above to browse");
                });
            });
        } else if !state.content.is_empty() {
            // Try CEF rendering first if available
            let url = state.url.clone();
            if cef_browser::is_cef_enabled() && cef_state.is_available() {
                // CEF is available - use native rendering
                cef_browser::render_cef_browser_panel(ui, cef_state, &url, tab_idx);
            } else {
                // Fallback: Show parsed HTML content as text
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // CEF status banner
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("ℹ").color(egui::Color32::from_rgb(100, 150, 255)));
                            ui.label(egui::RichText::new(cef_browser::get_cef_status_message()).small().color(egui::Color32::GRAY));
                        });
                        ui.add_space(4.0);
                        
                        if state.content_type.contains("html") {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(&state.title).size(18.0).strong());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("🌐 Open in System Browser").clicked() {
                                        let _ = open::that(&state.url);
                                    }
                                });
                            });
                            ui.separator();
                            let text_content = strip_html_tags(&state.content);
                            ui.label(&text_content);
                        } else {
                            ui.label(&state.content);
                        }
                    });
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("No content");
            });
        }
    } else {
        ui.centered_and_justified(|ui| {
            ui.label("No browser state");
        });
    }
}

/// Strip HTML tags from content for basic text display
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut last_was_space = false;
    
    let html_lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let chars_lower: Vec<char> = html_lower.chars().collect();
    
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        
        // Check for script/style tags
        if i + 7 < chars.len() {
            let slice: String = chars_lower[i..i+7].iter().collect();
            if slice == "<script" {
                in_script = true;
            } else if slice == "</scrip" {
                in_script = false;
                // Skip to end of tag
                while i < chars.len() && chars[i] != '>' {
                    i += 1;
                }
                i += 1;
                continue;
            }
        }
        if i + 6 < chars.len() {
            let slice: String = chars_lower[i..i+6].iter().collect();
            if slice == "<style" {
                in_style = true;
            } else if slice == "</styl" {
                in_style = false;
                while i < chars.len() && chars[i] != '>' {
                    i += 1;
                }
                i += 1;
                continue;
            }
        }
        
        if in_script || in_style {
            i += 1;
            continue;
        }
        
        if c == '<' {
            in_tag = true;
            // Check for <br>, <p>, <div> etc to add newlines
            if i + 3 < chars.len() {
                let next: String = chars_lower[i..i+3].iter().collect();
                if next == "<br" || next == "<p>" || next == "<p " {
                    result.push('\n');
                    last_was_space = true;
                }
            }
            if i + 4 < chars.len() {
                let next: String = chars_lower[i..i+4].iter().collect();
                if next == "<div" || next == "</p>" {
                    result.push('\n');
                    last_was_space = true;
                }
            }
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            // Handle HTML entities
            if c == '&' && i + 1 < chars.len() {
                let rest: String = chars[i..].iter().take(10).collect();
                if rest.starts_with("&nbsp;") {
                    result.push(' ');
                    i += 5;
                } else if rest.starts_with("&lt;") {
                    result.push('<');
                    i += 3;
                } else if rest.starts_with("&gt;") {
                    result.push('>');
                    i += 3;
                } else if rest.starts_with("&amp;") {
                    result.push('&');
                    i += 4;
                } else if rest.starts_with("&quot;") {
                    result.push('"');
                    i += 5;
                } else {
                    result.push(c);
                }
            } else if c.is_whitespace() {
                if !last_was_space {
                    result.push(' ');
                    last_was_space = true;
                }
            } else {
                result.push(c);
                last_was_space = false;
            }
        }
        i += 1;
    }
    
    // Trim excessive newlines
    let lines: Vec<&str> = result.lines().collect();
    let mut final_result = String::new();
    let mut empty_count = 0;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            empty_count += 1;
            if empty_count <= 2 {
                final_result.push('\n');
            }
        } else {
            empty_count = 0;
            final_result.push_str(trimmed);
            final_result.push('\n');
        }
    }
    
    final_result
}

/// Show Explorer panel using UIWorldSnapshot with professional service hierarchy
fn show_explorer_panel(
    ui: &mut egui::Ui,
    snapshot: &UIWorldSnapshot,
    expanded: &ExplorerExpanded,
    action_queue: &mut UIActionQueue,
    drag_select: &mut ExplorerDragSelect,
    advanced_search: &mut AdvancedSearchState,
) {
    use explorer::ServiceType;
    use explorer_search_ui::{show_advanced_search_panel, show_search_results};
    
    // ════════════════════════════════════════════════════════════════════
    // Advanced Search Panel
    // ════════════════════════════════════════════════════════════════════
    
    // Note: show_advanced_search_panel requires &World but we only have UIWorldSnapshot
    // For now, show a simplified search UI that works with the snapshot
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        
        // Toggle advanced panel button
        let panel_icon = if advanced_search.show_panel { "▼" } else { "▶" };
        if ui.small_button(panel_icon).on_hover_text("Toggle advanced search").clicked() {
            advanced_search.show_panel = !advanced_search.show_panel;
        }
        
        // Main search input
        let response = ui.add(
            egui::TextEdit::singleline(&mut advanced_search.query.raw_query)
                .hint_text("🔍 Search... (e.g., class:Part anchored:true)")
                .desired_width(ui.available_width() - 30.0)
        );
        
        // Parse on change
        if response.changed() {
            advanced_search.query = explorer_search::SearchQuery::parse(&advanced_search.query.raw_query);
        }
        
        // Clear button
        if !advanced_search.query.raw_query.is_empty() {
            if ui.small_button("✕").on_hover_text("Clear search").clicked() {
                advanced_search.query.clear();
                advanced_search.results.clear();
            }
        }
    });
    
    // Show advanced filter builder if panel is open
    if advanced_search.show_panel {
        ui.add_space(4.0);
        
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(35, 35, 40))
            .rounding(4.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Quick Filters").strong());
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("Parts").clicked() {
                        advanced_search.query = explorer_search::SearchQuery::parse("class:Part");
                    }
                    if ui.small_button("Models").clicked() {
                        advanced_search.query = explorer_search::SearchQuery::parse("class:Model");
                    }
                    if ui.small_button("Lights").clicked() {
                        advanced_search.query = explorer_search::SearchQuery::parse("class:PointLight,SpotLight,SurfaceLight");
                    }
                    if ui.small_button("Scripts").clicked() {
                        advanced_search.query = explorer_search::SearchQuery::parse("class:SoulScript");
                    }
                    if ui.small_button("Anchored").clicked() {
                        advanced_search.query = explorer_search::SearchQuery::parse("anchored:true");
                    }
                    if ui.small_button("Transparent").clicked() {
                        advanced_search.query = explorer_search::SearchQuery::parse("transparency:>0");
                    }
                });
                
                // Show active filters as chips
                if !advanced_search.query.criteria.is_empty() {
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Active Filters").strong());
                    ui.horizontal_wrapped(|ui| {
                        for criterion in &advanced_search.query.criteria {
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(60, 80, 120))
                                .rounding(12.0)
                                .inner_margin(egui::vec2(8.0, 4.0))
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new(criterion.display()).small());
                                });
                        }
                    });
                }
            });
    }
    
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(2.0);
    
    // Constants for consistent styling
    const ROW_HEIGHT: f32 = 22.0;
    const INDENT_SIZE: f32 = 16.0;
    
    // Clear row rects at start of frame
    drag_select.row_rects.clear();
    
    // Get panel rect for drag selection bounds
    let panel_rect = ui.available_rect_before_wrap();
    
    // Handle drag selection input
    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let primary_down = ui.input(|i| i.pointer.primary_down());
    let primary_released = ui.input(|i| i.pointer.primary_released());
    let shift_held = ui.input(|i| i.modifiers.shift);
    
    // Start drag if clicking in panel area (not on a specific item)
    if let Some(pos) = pointer_pos {
        if panel_rect.contains(pos) {
            if primary_down && !drag_select.active && drag_select.start_pos.is_none() {
                // Will be set to active only if we drag far enough
                drag_select.start_pos = Some(pos);
            }
            
            if let Some(start) = drag_select.start_pos {
                // Check if we've dragged far enough to start selection
                let drag_dist = (pos - start).length();
                if drag_dist > 5.0 {
                    drag_select.active = true;
                }
                
                if drag_select.active {
                    drag_select.current_pos = Some(pos);
                }
            }
        }
    }
    
    // Track if we need to finalize selection after rendering
    // (row_rects are populated during rendering, so we must check after)
    let should_finalize_selection = primary_released && drag_select.active;
    let should_clear_start = primary_released && !drag_select.active;
    
    // Categorize root entities by service (excluding meta/internal classes)
    let mut workspace_entities: Vec<&world_view::EntitySnapshot> = Vec::new();
    let mut lighting_entities: Vec<&world_view::EntitySnapshot> = Vec::new();
    let mut soul_entities: Vec<&world_view::EntitySnapshot> = Vec::new();
    let mut starter_gui_entities: Vec<&world_view::EntitySnapshot> = Vec::new();
    
    for root in &snapshot.roots {
        if let Some(entity_data) = snapshot.get(*root) {
            // Skip meta/internal classes that shouldn't appear in explorer
            if is_meta_class(entity_data.class_name) {
                continue;
            }
            
            // Route based on ServiceOwner component if present (explicit service assignment)
            // Otherwise fall back to class-based routing
            if let Some(owner) = entity_data.service_owner {
                match owner {
                    ServiceType::Workspace => workspace_entities.push(entity_data),
                    ServiceType::Lighting => lighting_entities.push(entity_data),
                    ServiceType::SoulService => soul_entities.push(entity_data),
                    ServiceType::StarterGui => starter_gui_entities.push(entity_data),
                    _ => {} // Other services not yet implemented
                }
            } else {
                // Fallback: Route based on class type for entities without ServiceOwner
                // Check StarterGui FIRST for GUI elements (ScreenGui, Frame, etc.)
                if ServiceType::StarterGui.accepts_class(entity_data.class_name) {
                    starter_gui_entities.push(entity_data);
                } else if ServiceType::Workspace.accepts_class(entity_data.class_name) {
                    workspace_entities.push(entity_data);
                } else if ServiceType::Lighting.accepts_class(entity_data.class_name) {
                    lighting_entities.push(entity_data);
                } else if entity_data.class_name == crate::classes::ClassName::SoulScript {
                    soul_entities.push(entity_data);
                }
            }
        }
    }
    
    // Sort by class name (A-Z), then by instance name (A-Z)
    workspace_entities.sort_by(|a, b| {
        let class_cmp = a.class_name.as_str().cmp(b.class_name.as_str());
        if class_cmp == std::cmp::Ordering::Equal {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        } else {
            class_cmp
        }
    });
    lighting_entities.sort_by(|a, b| {
        let class_cmp = a.class_name.as_str().cmp(b.class_name.as_str());
        if class_cmp == std::cmp::Ordering::Equal {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        } else {
            class_cmp
        }
    });
    soul_entities.sort_by(|a, b| {
        let class_cmp = a.class_name.as_str().cmp(b.class_name.as_str());
        if class_cmp == std::cmp::Ordering::Equal {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        } else {
            class_cmp
        }
    });
    starter_gui_entities.sort_by(|a, b| {
        let class_cmp = a.class_name.as_str().cmp(b.class_name.as_str());
        if class_cmp == std::cmp::Ordering::Equal {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        } else {
            class_cmp
        }
    });
    
    // ════════════════════════════════════════════════════════════════════
    // Experience (Root)
    // ════════════════════════════════════════════════════════════════════
    
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::hover()
    );
    
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(45, 45, 50));
        ui.painter().text(
            rect.min + egui::vec2(4.0, 3.0),
            egui::Align2::LEFT_TOP,
            "🎮 Experience",
            egui::FontId::proportional(14.0),
            egui::Color32::WHITE,
        );
    }
    
    // Row index for alternating colors
    let mut row_index: usize = 0;
    
    // ════════════════════════════════════════════════════════════════════
    // Workspace Service - Contains 3D objects
    // ════════════════════════════════════════════════════════════════════
    
    render_service_node_snapshot(
        ui, ServiceType::Workspace, &workspace_entities, 
        expanded, snapshot, action_queue, ROW_HEIGHT, INDENT_SIZE, &mut row_index, &mut drag_select.row_rects
    );
    
    // ════════════════════════════════════════════════════════════════════
    // Players Service (empty placeholder)
    // ════════════════════════════════════════════════════════════════════
    
    render_empty_service_node(ui, ServiceType::Players, expanded, action_queue, ROW_HEIGHT, INDENT_SIZE, &mut row_index);
    
    // ════════════════════════════════════════════════════════════════════
    // Lighting Service - Contains lights and atmosphere
    // ════════════════════════════════════════════════════════════════════
    
    render_service_node_snapshot(
        ui, ServiceType::Lighting, &lighting_entities,
        expanded, snapshot, action_queue, ROW_HEIGHT, INDENT_SIZE, &mut row_index, &mut drag_select.row_rects
    );
    
    // ════════════════════════════════════════════════════════════════════
    // Other Services (empty placeholders)
    // ════════════════════════════════════════════════════════════════════
    
    // ════════════════════════════════════════════════════════════════════
    // SoulService - Contains Soul scripts
    // ════════════════════════════════════════════════════════════════════
    
    render_service_node_snapshot(
        ui, ServiceType::SoulService, &soul_entities,
        expanded, snapshot, action_queue, ROW_HEIGHT, INDENT_SIZE, &mut row_index, &mut drag_select.row_rects
    );
    
    render_empty_service_node(ui, ServiceType::ServerStorage, expanded, action_queue, ROW_HEIGHT, INDENT_SIZE, &mut row_index);
    
    // ════════════════════════════════════════════════════════════════════
    // StarterGui Service - Contains screen UI elements (ScreenGui, Frame, etc.)
    // ════════════════════════════════════════════════════════════════════
    
    render_service_node_snapshot(
        ui, ServiceType::StarterGui, &starter_gui_entities,
        expanded, snapshot, action_queue, ROW_HEIGHT, INDENT_SIZE, &mut row_index, &mut drag_select.row_rects
    );
    render_empty_service_node(ui, ServiceType::StarterPack, expanded, action_queue, ROW_HEIGHT, INDENT_SIZE, &mut row_index);
    render_empty_service_node(ui, ServiceType::StarterPlayer, expanded, action_queue, ROW_HEIGHT, INDENT_SIZE, &mut row_index);
    render_empty_service_node(ui, ServiceType::SoundService, expanded, action_queue, ROW_HEIGHT, INDENT_SIZE, &mut row_index);
    render_empty_service_node(ui, ServiceType::Teams, expanded, action_queue, ROW_HEIGHT, INDENT_SIZE, &mut row_index);
    
    // ════════════════════════════════════════════════════════════════════
    // Empty space click handler - deselect when clicking empty area
    // ════════════════════════════════════════════════════════════════════
    
    // Add clickable empty space for deselection
    let empty_space_height = 200.0;
    let (empty_rect, empty_response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), empty_space_height),
        egui::Sense::click()
    );
    
    // Visual feedback on hover
    if empty_response.hovered() && ui.is_rect_visible(empty_rect) {
        ui.painter().rect_filled(
            empty_rect,
            0.0,
            egui::Color32::from_rgba_unmultiplied(100, 100, 255, 10)
        );
    }
    
    // Deselect all when clicking on empty space (only if not drag selecting)
    if empty_response.clicked() && !drag_select.active {
        info!("Explorer: Clicked on empty space - clearing selection");
        action_queue.push(UIAction::ClearSelection);
    }
    
    // ════════════════════════════════════════════════════════════════════
    // Draw selection box overlay
    // ════════════════════════════════════════════════════════════════════
    
    if drag_select.active {
        if let Some(sel_rect) = drag_select.selection_rect() {
            // Draw selection box
            ui.painter().rect_filled(
                sel_rect,
                2.0,
                egui::Color32::from_rgba_unmultiplied(100, 150, 255, 40)
            );
            ui.painter().rect_stroke(
                sel_rect,
                2.0,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(100, 150, 255)),
                egui::StrokeKind::Outside,
            );
            
            // Highlight rows that would be selected
            for (_, row_rect) in &drag_select.row_rects {
                if sel_rect.intersects(*row_rect) {
                    ui.painter().rect_filled(
                        *row_rect,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(100, 150, 255, 60)
                    );
                }
            }
        }
    }
    
    // ════════════════════════════════════════════════════════════════════
    // Finalize drag selection AFTER rows are rendered (row_rects populated)
    // ════════════════════════════════════════════════════════════════════
    
    if should_finalize_selection {
        // Select all entities in the selection box
        let selected_entities = drag_select.get_selected_entities();
        if !selected_entities.is_empty() {
            if shift_held {
                // Add to selection
                for entity in selected_entities {
                    action_queue.push(UIAction::AddToSelection(entity));
                }
            } else {
                // Replace selection
                action_queue.push(UIAction::Select(selected_entities));
            }
        }
        drag_select.clear();
    } else if should_clear_start {
        // Click without drag - clear start pos
        drag_select.start_pos = None;
    }
}

/// Check if a class is a meta/internal class that shouldn't appear in explorer
/// Only abstract base classes are meta - concrete components should be visible
fn is_meta_class(class: crate::classes::ClassName) -> bool {
    use crate::classes::ClassName;
    matches!(class,
        // Abstract base classes only - not instantiable directly
        ClassName::Instance | ClassName::PVInstance | ClassName::BasePart
    )
}

/// Render a service node with entities from snapshot
fn render_service_node_snapshot(
    ui: &mut egui::Ui,
    service: explorer::ServiceType,
    entities: &[&world_view::EntitySnapshot],
    expanded: &ExplorerExpanded,
    snapshot: &UIWorldSnapshot,
    action_queue: &mut UIActionQueue,
    row_height: f32,
    indent_size: f32,
    row_index: &mut usize,
    row_rects: &mut Vec<(Entity, egui::Rect)>,
) {
    let is_expanded = expanded.is_service_expanded(service);
    let is_selected = expanded.is_service_selected(service);
    let has_children = !entities.is_empty();
    let child_count = entities.len();
    
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::click()
    );
    
    let is_hovered = response.hovered();
    
    // Alternating row colors
    let row_bg = if *row_index % 2 == 0 {
        egui::Color32::from_rgb(38, 38, 42)  // Dark grey
    } else {
        egui::Color32::from_rgb(48, 48, 52)  // Light grey
    };
    *row_index += 1;
    
    if ui.is_rect_visible(rect) {
        // Background - selection > hover > alternating
        let bg_color = if is_selected {
            egui::Color32::from_rgb(50, 100, 150)  // Selection blue
        } else if is_hovered {
            egui::Color32::from_rgb(60, 60, 65)
        } else {
            row_bg
        };
        ui.painter().rect_filled(rect, 0.0, bg_color);
        
        let mut x = rect.min.x + indent_size;
        
        // Expand arrow (drawn as triangle shape for reliable rendering)
        if has_children {
            let arrow_center = egui::pos2(x + 5.0, rect.min.y + row_height / 2.0);
            let arrow_size = 4.0;
            let arrow_color = egui::Color32::from_rgb(180, 180, 180);
            
            if is_expanded {
                // Down-pointing triangle (expanded)
                let points = vec![
                    egui::pos2(arrow_center.x - arrow_size, arrow_center.y - arrow_size * 0.5),
                    egui::pos2(arrow_center.x + arrow_size, arrow_center.y - arrow_size * 0.5),
                    egui::pos2(arrow_center.x, arrow_center.y + arrow_size * 0.5),
                ];
                ui.painter().add(egui::Shape::convex_polygon(points, arrow_color, egui::Stroke::NONE));
            } else {
                // Right-pointing triangle (collapsed)
                let points = vec![
                    egui::pos2(arrow_center.x - arrow_size * 0.5, arrow_center.y - arrow_size),
                    egui::pos2(arrow_center.x + arrow_size * 0.5, arrow_center.y),
                    egui::pos2(arrow_center.x - arrow_size * 0.5, arrow_center.y + arrow_size),
                ];
                ui.painter().add(egui::Shape::convex_polygon(points, arrow_color, egui::Stroke::NONE));
            }
        }
        x += 14.0;
        
        // Icon (vector)
        icons::draw_service_icon(ui.painter(), egui::pos2(x, rect.min.y + 2.0), service, 14.0);
        x += 18.0;
        
        // Name - brighter when selected, otherwise consistent gray text
        let name_color = if is_selected {
            egui::Color32::WHITE
        } else {
            service.text_color()
        };
        ui.painter().text(
            egui::pos2(x, rect.min.y + 3.0),
            egui::Align2::LEFT_TOP,
            service.name(),
            egui::FontId::proportional(13.0),
            name_color,
        );
        
        // Child count badge
        if child_count > 0 {
            let count_text = format!("({})", child_count);
            ui.painter().text(
                egui::pos2(rect.max.x - 30.0, rect.min.y + 4.0),
                egui::Align2::RIGHT_TOP,
                &count_text,
                egui::FontId::proportional(11.0),
                egui::Color32::from_rgb(120, 120, 120),
            );
        }
    }
    
    // Handle click - select service and toggle expansion
    if response.clicked() {
        // Clear entity selection and select this service
        action_queue.push(UIAction::ClearSelection);
        action_queue.push(UIAction::SelectService(service));
        // Also toggle expansion
        action_queue.push(UIAction::ToggleServiceExpanded(service));
    }
    
    // Context menu for inserting objects
    response.context_menu(|ui| {
        ui.set_max_width(140.0);
        
        // Insert submenu with hover
        ui.menu_button("Insert ▶", |ui| {
            ui.set_max_width(120.0);
            render_service_insert_menu(ui, service, action_queue);
        });
    });
    
    // Render children if expanded
    if is_expanded {
        for entity_data in entities {
            render_entity_row_snapshot(
                ui, entity_data, snapshot, expanded, action_queue, 2, row_height, indent_size, row_index, row_rects
            );
        }
    }
}

/// Render an empty service node (placeholder but interactive)
fn render_empty_service_node(
    ui: &mut egui::Ui,
    service: explorer::ServiceType,
    expanded: &ExplorerExpanded,
    action_queue: &mut UIActionQueue,
    row_height: f32,
    indent_size: f32,
    row_index: &mut usize,
) {
    let is_selected = expanded.is_service_selected(service);
    
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::click()
    );
    
    // Alternating row colors
    let row_bg = if *row_index % 2 == 0 {
        egui::Color32::from_rgb(38, 38, 42)  // Dark grey
    } else {
        egui::Color32::from_rgb(48, 48, 52)  // Light grey
    };
    *row_index += 1;
    
    if ui.is_rect_visible(rect) {
        // Background - selection > hover > alternating
        let bg_color = if is_selected {
            egui::Color32::from_rgb(50, 100, 150)  // Selection blue
        } else if response.hovered() {
            egui::Color32::from_rgb(60, 60, 65)
        } else {
            row_bg
        };
        ui.painter().rect_filled(rect, 0.0, bg_color);
        
        let mut x = rect.min.x + indent_size;
        
        // Expand arrow (empty services show no arrow, just spacing)
        // Draw a subtle dot to indicate "no children"
        ui.painter().text(
            egui::pos2(x + 4.0, rect.min.y + 2.0),
            egui::Align2::LEFT_TOP,
            "·",  // Subtle dot indicating empty
            egui::FontId::proportional(12.0),
            egui::Color32::from_rgb(80, 80, 80),
        );
        x += 14.0;
        
        // Icon (vector) with service color
        icons::draw_service_icon(ui.painter(), egui::pos2(x, rect.min.y + 2.0), service, 14.0);
        x += 18.0;
        
        // Name - brighter when selected
        let name_color = if is_selected {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_rgb(200, 200, 200)
        };
        ui.painter().text(
            egui::pos2(x, rect.min.y + 3.0),
            egui::Align2::LEFT_TOP,
            service.name(),
            egui::FontId::proportional(13.0),
            name_color,
        );
    }
    
    // Handle click - select service
    if response.clicked() {
        action_queue.push(UIAction::ClearSelection);
        action_queue.push(UIAction::SelectService(service));
    }
    
    // Context menu for inserting objects
    response.context_menu(|ui| {
        ui.set_max_width(140.0);
        
        // Insert submenu with hover
        ui.menu_button("Insert ▶", |ui| {
            ui.set_max_width(120.0);
            render_service_insert_menu(ui, service, action_queue);
        });
    });
}

/// Render the insert menu for a service (submenu items)
fn render_service_insert_menu(
    ui: &mut egui::Ui,
    service: explorer::ServiceType,
    action_queue: &mut UIActionQueue,
) {
    use crate::classes::ClassName;
    
    // Helper to create compact menu item
    let mut insert_item = |ui: &mut egui::Ui, label: &str, class: ClassName| {
        if ui.button(label).clicked() {
            action_queue.push(UIAction::SpawnIntoService { service, class_name: class });
            ui.close();
        }
    };
    
    match service {
        explorer::ServiceType::Workspace => {
            insert_item(ui, "Part", ClassName::Part);
            insert_item(ui, "MeshPart", ClassName::MeshPart);
            insert_item(ui, "Model", ClassName::Model);
            insert_item(ui, "Folder", ClassName::Folder);
            insert_item(ui, "SpawnLocation", ClassName::SpawnLocation);
            ui.separator();
            insert_item(ui, "Camera", ClassName::Camera);
        }
        explorer::ServiceType::Lighting => {
            insert_item(ui, "DirectionalLight", ClassName::DirectionalLight);
            insert_item(ui, "PointLight", ClassName::PointLight);
            insert_item(ui, "SpotLight", ClassName::SpotLight);
            insert_item(ui, "SurfaceLight", ClassName::SurfaceLight);
            ui.separator();
            insert_item(ui, "Sky", ClassName::Sky);
            insert_item(ui, "Atmosphere", ClassName::Atmosphere);
            insert_item(ui, "Clouds", ClassName::Clouds);
            ui.separator();
            insert_item(ui, "Sun", ClassName::Sun);
            insert_item(ui, "Moon", ClassName::Moon);
        }
        explorer::ServiceType::SoundService => {
            insert_item(ui, "Sound", ClassName::Sound);
        }
        explorer::ServiceType::SoulService => {
            insert_item(ui, "Soul Script", ClassName::SoulScript);
            insert_item(ui, "Folder", ClassName::Folder);
        }
        explorer::ServiceType::ServerStorage => {
            insert_item(ui, "Folder", ClassName::Folder);
            insert_item(ui, "Model", ClassName::Model);
        }
        explorer::ServiceType::StarterGui => {
            ui.label(egui::RichText::new("GUI").small().color(egui::Color32::GRAY));
            insert_item(ui, "ScreenGui", ClassName::ScreenGui);
            insert_item(ui, "BillboardGui", ClassName::BillboardGui);
            insert_item(ui, "SurfaceGui", ClassName::SurfaceGui);
            ui.separator();
            ui.label(egui::RichText::new("Elements").small().color(egui::Color32::GRAY));
            insert_item(ui, "Frame", ClassName::Frame);
            insert_item(ui, "TextLabel", ClassName::TextLabel);
            insert_item(ui, "TextButton", ClassName::TextButton);
            insert_item(ui, "TextBox", ClassName::TextBox);
            insert_item(ui, "ImageLabel", ClassName::ImageLabel);
            insert_item(ui, "ImageButton", ClassName::ImageButton);
            ui.separator();
            ui.label(egui::RichText::new("Other").small().color(egui::Color32::GRAY));
            insert_item(ui, "Folder", ClassName::Folder);
            insert_item(ui, "Soul Script", ClassName::SoulScript);
        }
        explorer::ServiceType::StarterPack => {
            insert_item(ui, "Folder", ClassName::Folder);
            insert_item(ui, "Soul Script", ClassName::SoulScript);
        }
        explorer::ServiceType::StarterPlayer => {
            insert_item(ui, "Soul Script", ClassName::SoulScript);
            insert_item(ui, "Folder", ClassName::Folder);
        }
        explorer::ServiceType::Teams => {
            ui.label(egui::RichText::new("(empty)").small().color(egui::Color32::GRAY));
        }
        explorer::ServiceType::Players | explorer::ServiceType::Chat => {
            ui.label(egui::RichText::new("(runtime)").small().color(egui::Color32::GRAY));
        }
        _ => {
            ui.label(egui::RichText::new("(empty)").small().color(egui::Color32::GRAY));
        }
    }
}

/// Render an entity row from snapshot
fn render_entity_row_snapshot(
    ui: &mut egui::Ui,
    entity_data: &world_view::EntitySnapshot,
    snapshot: &UIWorldSnapshot,
    expanded: &ExplorerExpanded,
    action_queue: &mut UIActionQueue,
    depth: usize,
    row_height: f32,
    indent_size: f32,
    row_index: &mut usize,
    row_rects: &mut Vec<(Entity, egui::Rect)>,
) {
    let has_children = !entity_data.children.is_empty();
    let is_expanded = expanded.is_expanded(entity_data.entity);
    let is_selected = snapshot.is_selected(entity_data.entity);
    
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::click_and_drag()
    );
    
    // Collect row rect for drag selection
    row_rects.push((entity_data.entity, rect));
    
    // Drag payload for reparenting - set the dragged entity
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        response.dnd_set_drag_payload(entity_data.entity);
    }
    
    // Check if this row is a drop target for reparenting
    let mut is_drop_target = false;
    if let Some(dragged_entity) = response.dnd_hover_payload::<Entity>() {
        // Don't allow dropping onto self
        if *dragged_entity != entity_data.entity {
            is_drop_target = true;
        }
    }
    
    // Handle drop - reparent the dragged entity to this entity
    if let Some(dragged_entity) = response.dnd_release_payload::<Entity>() {
        // Don't allow dropping onto self
        if *dragged_entity != entity_data.entity {
            action_queue.push(UIAction::Reparent { 
                child: *dragged_entity, 
                new_parent: entity_data.entity 
            });
        }
    }
    
    let is_hovered = response.hovered();
    
    // Alternating row colors
    let row_bg = if *row_index % 2 == 0 {
        egui::Color32::from_rgb(38, 38, 42)  // Dark grey
    } else {
        egui::Color32::from_rgb(48, 48, 52)  // Light grey
    };
    *row_index += 1;
    
    if ui.is_rect_visible(rect) {
        // Selection/hover/drop-target background, or alternating color
        if is_drop_target {
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(80, 120, 80)); // Green for drop target
        } else if is_selected {
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(50, 80, 120));
        } else if is_hovered {
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(60, 60, 65));
        } else {
            ui.painter().rect_filled(rect, 0.0, row_bg);
        }
        
        let mut x = rect.min.x + (depth as f32 * indent_size);
        
        // Expand arrow (drawn as triangle shape for reliable rendering)
        if has_children {
            let arrow_center = egui::pos2(x + 5.0, rect.min.y + row_height / 2.0);
            let arrow_size = 4.0;
            let arrow_color = egui::Color32::from_rgb(180, 180, 180);
            
            if is_expanded {
                // Down-pointing triangle (expanded)
                let points = vec![
                    egui::pos2(arrow_center.x - arrow_size, arrow_center.y - arrow_size * 0.5),
                    egui::pos2(arrow_center.x + arrow_size, arrow_center.y - arrow_size * 0.5),
                    egui::pos2(arrow_center.x, arrow_center.y + arrow_size * 0.5),
                ];
                ui.painter().add(egui::Shape::convex_polygon(points, arrow_color, egui::Stroke::NONE));
            } else {
                // Right-pointing triangle (collapsed)
                let points = vec![
                    egui::pos2(arrow_center.x - arrow_size * 0.5, arrow_center.y - arrow_size),
                    egui::pos2(arrow_center.x + arrow_size * 0.5, arrow_center.y),
                    egui::pos2(arrow_center.x - arrow_size * 0.5, arrow_center.y + arrow_size),
                ];
                ui.painter().add(egui::Shape::convex_polygon(points, arrow_color, egui::Stroke::NONE));
            }
        }
        x += 14.0;
        
        // Icon (vector)
        icons::draw_class_icon(ui.painter(), egui::pos2(x, rect.min.y + 2.0), entity_data.class_name, 14.0);
        x += 18.0;
        
        // Name
        let text_color = if is_selected {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_rgb(220, 220, 220)
        };
        ui.painter().text(
            egui::pos2(x, rect.min.y + 3.0),
            egui::Align2::LEFT_TOP,
            &entity_data.name,
            egui::FontId::proportional(13.0),
            text_color,
        );
    }
    
    // Handle click - SINGLE CLICK = SELECT ONLY
    if response.clicked() {
        info!("Explorer: Clicked on entity {:?} ({})", entity_data.entity, entity_data.name);
        // Clear service selection and select this entity
        action_queue.push(UIAction::ClearServiceSelection);
        action_queue.push(UIAction::Select(vec![entity_data.entity]));
    }
    
    // Handle DOUBLE-CLICK
    if response.double_clicked() {
        // SoulScript: Open in script editor
        if entity_data.class_name == crate::classes::ClassName::SoulScript {
            info!("Explorer: Double-clicked to open SoulScript {:?}", entity_data.entity);
            action_queue.push(UIAction::OpenScript(entity_data.entity));
        } else if has_children {
            // Other entities with children: Toggle expand/collapse
            info!("Explorer: Double-clicked to toggle expand on {:?}", entity_data.entity);
            action_queue.push(UIAction::ToggleExpanded(entity_data.entity));
        }
    }
    
    // Handle RIGHT-CLICK - Context menu (VS Code style)
    response.context_menu(|ui| {
        ui.set_min_width(180.0);
        
        // SoulScript-specific options
        if entity_data.class_name == crate::classes::ClassName::SoulScript {
            if ui.button("Open Script").clicked() {
                action_queue.push(UIAction::OpenScript(entity_data.entity));
                ui.close();
            }
            if ui.button("Build Script").clicked() {
                // Queue build via temp data
                ui.ctx().data_mut(|d| {
                    d.insert_temp(egui::Id::new("pending_build_entity"), entity_data.entity);
                });
                ui.close();
            }
            ui.separator();
            if ui.button("Rename...").clicked() {
                action_queue.push(UIAction::BeginRename(entity_data.entity));
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                action_queue.push(UIAction::Duplicate(entity_data.entity));
                ui.close();
            }
            ui.separator();
            if ui.button("Copy").clicked() {
                action_queue.push(UIAction::Copy(vec![entity_data.entity]));
                ui.close();
            }
            if ui.button("Cut").clicked() {
                action_queue.push(UIAction::Cut(vec![entity_data.entity]));
                ui.close();
            }
            if ui.button("Paste Into").clicked() {
                action_queue.push(UIAction::PasteInto(entity_data.entity));
                ui.close();
            }
            ui.separator();
            ui.add_enabled_ui(true, |ui| {
                if ui.button("Delete").clicked() {
                    action_queue.push(UIAction::Delete(vec![entity_data.entity]));
                    ui.close();
                }
            });
        } else {
            // Generic entity context menu
            if has_children {
                let expand_text = if is_expanded { "Collapse" } else { "Expand" };
                if ui.button(expand_text).clicked() {
                    action_queue.push(UIAction::ToggleExpanded(entity_data.entity));
                    ui.close();
                }
                ui.separator();
            }
            if ui.button("Rename...").clicked() {
                action_queue.push(UIAction::BeginRename(entity_data.entity));
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                action_queue.push(UIAction::Duplicate(entity_data.entity));
                ui.close();
            }
            ui.separator();
            if ui.button("Copy").clicked() {
                action_queue.push(UIAction::Copy(vec![entity_data.entity]));
                ui.close();
            }
            if ui.button("Cut").clicked() {
                action_queue.push(UIAction::Cut(vec![entity_data.entity]));
                ui.close();
            }
            if ui.button("Paste Into").clicked() {
                action_queue.push(UIAction::PasteInto(entity_data.entity));
                ui.close();
            }
            ui.separator();
            if ui.button("Delete").clicked() {
                action_queue.push(UIAction::Delete(vec![entity_data.entity]));
                ui.close();
            }
        }
    });
    
    // Render children if expanded
    if is_expanded && has_children {
        for child_entity in &entity_data.children {
            if let Some(child_data) = snapshot.get(*child_entity) {
                // Skip meta classes in children too
                if !is_meta_class(child_data.class_name) {
                    render_entity_row_snapshot(
                        ui, child_data, snapshot, expanded, action_queue, depth + 1, row_height, indent_size, row_index, row_rects
                    );
                }
            }
        }
    }
}

// ============================================================================
// LCD Property Rendering Helpers
// ============================================================================

/// Render a Vec3 property with LCD support for multi-selection
fn render_vec3_property_lcd(
    ui: &mut egui::Ui,
    label: &str,
    lcd_value: &world_view::LcdValue<Vec3>,
    fallback: Option<Vec3>,
    selected_entities: &[Entity],
    first_entity: Entity,
    multi_select: bool,
    action_queue: &mut UIActionQueue,
) {
    match lcd_value {
        world_view::LcdValue::Same(val) => {
            ui.label(label);
            let mut new_val = *val;
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("X:");
                changed |= ui.add(egui::DragValue::new(&mut new_val.x).speed(0.1).fixed_decimals(2)).changed();
                ui.label("Y:");
                changed |= ui.add(egui::DragValue::new(&mut new_val.y).speed(0.1).fixed_decimals(2)).changed();
                ui.label("Z:");
                changed |= ui.add(egui::DragValue::new(&mut new_val.z).speed(0.1).fixed_decimals(2)).changed();
            });
            if changed {
                if multi_select {
                    action_queue.push(UIAction::SetPropertyMulti {
                        entities: selected_entities.to_vec(),
                        property: label.to_string(),
                        value: crate::classes::PropertyValue::Vector3(new_val),
                    });
                } else {
                    action_queue.push(UIAction::SetProperty {
                        entity: first_entity,
                        property: label.to_string(),
                        value: crate::classes::PropertyValue::Vector3(new_val),
                    });
                }
            }
            ui.add_space(4.0);
        }
        world_view::LcdValue::Mixed => {
            // Mixed values - show editable widget with placeholder, any change applies to all
            ui.label(label);
            let mut new_val = fallback.unwrap_or(Vec3::ZERO);
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.weak("X:");
                changed |= ui.add(egui::DragValue::new(&mut new_val.x).speed(0.1).fixed_decimals(2)).changed();
                ui.weak("Y:");
                changed |= ui.add(egui::DragValue::new(&mut new_val.y).speed(0.1).fixed_decimals(2)).changed();
                ui.weak("Z:");
                changed |= ui.add(egui::DragValue::new(&mut new_val.z).speed(0.1).fixed_decimals(2)).changed();
                ui.weak("(mixed)");
            });
            if changed {
                action_queue.push(UIAction::SetPropertyMulti {
                    entities: selected_entities.to_vec(),
                    property: label.to_string(),
                    value: crate::classes::PropertyValue::Vector3(new_val),
                });
            }
            ui.add_space(4.0);
        }
        world_view::LcdValue::None => {}
    }
}

/// Render a Color property with LCD support for multi-selection
fn render_color_property_lcd(
    ui: &mut egui::Ui,
    label: &str,
    lcd_value: &world_view::LcdValue<Color>,
    fallback: Option<Color>,
    selected_entities: &[Entity],
    first_entity: Entity,
    multi_select: bool,
    action_queue: &mut UIActionQueue,
) {
    match lcd_value {
        world_view::LcdValue::Same(color) => {
            ui.horizontal(|ui| {
                ui.label(format!("{}:", label));
                let rgba = color.to_srgba();
                let mut rgb = [rgba.red, rgba.green, rgba.blue];
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    let new_color = Color::srgb(rgb[0], rgb[1], rgb[2]);
                    if multi_select {
                        action_queue.push(UIAction::SetPropertyMulti {
                            entities: selected_entities.to_vec(),
                            property: label.to_string(),
                            value: crate::classes::PropertyValue::Color(new_color),
                        });
                    } else {
                        action_queue.push(UIAction::SetProperty {
                            entity: first_entity,
                            property: label.to_string(),
                            value: crate::classes::PropertyValue::Color(new_color),
                        });
                    }
                }
            });
        }
        world_view::LcdValue::Mixed => {
            // Mixed values - show editable color picker, any change applies to all
            ui.horizontal(|ui| {
                ui.label(format!("{}:", label));
                let fallback_color = fallback.unwrap_or(Color::WHITE);
                let rgba = fallback_color.to_srgba();
                let mut rgb = [rgba.red, rgba.green, rgba.blue];
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    let new_color = Color::srgb(rgb[0], rgb[1], rgb[2]);
                    action_queue.push(UIAction::SetPropertyMulti {
                        entities: selected_entities.to_vec(),
                        property: label.to_string(),
                        value: crate::classes::PropertyValue::Color(new_color),
                    });
                }
                ui.weak("(mixed)");
            });
        }
        world_view::LcdValue::None => {}
    }
}

/// Render a float property with LCD support for multi-selection
fn render_float_property_lcd(
    ui: &mut egui::Ui,
    label: &str,
    lcd_value: &world_view::LcdValue<f32>,
    fallback: Option<f32>,
    selected_entities: &[Entity],
    first_entity: Entity,
    multi_select: bool,
    action_queue: &mut UIActionQueue,
    range: std::ops::RangeInclusive<f32>,
) {
    match lcd_value {
        world_view::LcdValue::Same(val) => {
            ui.horizontal(|ui| {
                ui.label(format!("{}:", label));
                let mut v = *val;
                if ui.add(egui::Slider::new(&mut v, range).fixed_decimals(2)).changed() {
                    if multi_select {
                        action_queue.push(UIAction::SetPropertyMulti {
                            entities: selected_entities.to_vec(),
                            property: label.to_string(),
                            value: crate::classes::PropertyValue::Float(v),
                        });
                    } else {
                        action_queue.push(UIAction::SetProperty {
                            entity: first_entity,
                            property: label.to_string(),
                            value: crate::classes::PropertyValue::Float(v),
                        });
                    }
                }
            });
        }
        world_view::LcdValue::Mixed => {
            // Mixed values - show editable slider, any change applies to all
            ui.horizontal(|ui| {
                ui.label(format!("{}:", label));
                let mut v = fallback.unwrap_or(0.0);
                if ui.add(egui::Slider::new(&mut v, range).fixed_decimals(2)).changed() {
                    action_queue.push(UIAction::SetPropertyMulti {
                        entities: selected_entities.to_vec(),
                        property: label.to_string(),
                        value: crate::classes::PropertyValue::Float(v),
                    });
                }
                ui.weak("(mixed)");
            });
        }
        world_view::LcdValue::None => {}
    }
}

/// Render a bool property with LCD support for multi-selection
fn render_bool_property_lcd(
    ui: &mut egui::Ui,
    label: &str,
    lcd_value: &world_view::LcdValue<bool>,
    fallback: Option<bool>,
    selected_entities: &[Entity],
    first_entity: Entity,
    multi_select: bool,
    action_queue: &mut UIActionQueue,
) {
    match lcd_value {
        world_view::LcdValue::Same(val) => {
            let mut v = *val;
            if ui.checkbox(&mut v, label).clicked() {
                if multi_select {
                    action_queue.push(UIAction::SetPropertyMulti {
                        entities: selected_entities.to_vec(),
                        property: label.to_string(),
                        value: crate::classes::PropertyValue::Bool(!*val),
                    });
                } else {
                    action_queue.push(UIAction::SetProperty {
                        entity: first_entity,
                        property: label.to_string(),
                        value: crate::classes::PropertyValue::Bool(!*val),
                    });
                }
            }
        }
        world_view::LcdValue::Mixed => {
            // Mixed values - show tri-state checkbox, clicking applies to all
            ui.horizontal(|ui| {
                // Use indeterminate state visual (dash)
                let mut checked = fallback.unwrap_or(false);
                if ui.checkbox(&mut checked, format!("{} (mixed)", label)).clicked() {
                    // Toggle to the new state and apply to all
                    action_queue.push(UIAction::SetPropertyMulti {
                        entities: selected_entities.to_vec(),
                        property: label.to_string(),
                        value: crate::classes::PropertyValue::Bool(checked),
                    });
                }
            });
        }
        world_view::LcdValue::None => {}
    }
}

/// Render a Material property with LCD support for multi-selection
fn render_material_property_lcd(
    ui: &mut egui::Ui,
    label: &str,
    lcd_value: &world_view::LcdValue<crate::classes::Material>,
    fallback: Option<crate::classes::Material>,
    selected_entities: &[Entity],
    first_entity: Entity,
    multi_select: bool,
    action_queue: &mut UIActionQueue,
) {
    match lcd_value {
        world_view::LcdValue::Same(mat) => {
            ui.horizontal(|ui| {
                ui.label(format!("{}:", label));
                let mut current = *mat;
                egui::ComboBox::from_id_salt("material_combo_lcd")
                    .selected_text(format!("{:?}", current))
                    .show_ui(ui, |ui| {
                        use crate::classes::Material;
                        for mat_opt in [Material::Plastic, Material::SmoothPlastic, Material::Wood, Material::WoodPlanks,
                                        Material::Metal, Material::CorrodedMetal, Material::DiamondPlate, Material::Foil, 
                                        Material::Grass, Material::Concrete, Material::Brick, Material::Granite,
                                        Material::Marble, Material::Slate, Material::Sand, Material::Fabric,
                                        Material::Glass, Material::Neon, Material::Ice] {
                            if ui.selectable_value(&mut current, mat_opt, format!("{:?}", mat_opt)).changed() {
                                if multi_select {
                                    action_queue.push(UIAction::SetPropertyMulti {
                                        entities: selected_entities.to_vec(),
                                        property: label.to_string(),
                                        value: crate::classes::PropertyValue::Material(current),
                                    });
                                } else {
                                    action_queue.push(UIAction::SetProperty {
                                        entity: first_entity,
                                        property: label.to_string(),
                                        value: crate::classes::PropertyValue::Material(current),
                                    });
                                }
                            }
                        }
                    });
            });
        }
        world_view::LcdValue::Mixed => {
            // Mixed values - show dropdown, selecting applies to all
            ui.horizontal(|ui| {
                ui.label(format!("{}:", label));
                let mut current = fallback.unwrap_or(crate::classes::Material::Plastic);
                egui::ComboBox::from_id_salt("material_combo_lcd_mixed")
                    .selected_text("(mixed)")
                    .show_ui(ui, |ui| {
                        use crate::classes::Material;
                        for mat_opt in [Material::Plastic, Material::SmoothPlastic, Material::Wood, Material::WoodPlanks,
                                        Material::Metal, Material::CorrodedMetal, Material::DiamondPlate, Material::Foil, 
                                        Material::Grass, Material::Concrete, Material::Brick, Material::Granite,
                                        Material::Marble, Material::Slate, Material::Sand, Material::Fabric,
                                        Material::Glass, Material::Neon, Material::Ice] {
                            if ui.selectable_value(&mut current, mat_opt, format!("{:?}", mat_opt)).changed() {
                                action_queue.push(UIAction::SetPropertyMulti {
                                    entities: selected_entities.to_vec(),
                                    property: label.to_string(),
                                    value: crate::classes::PropertyValue::Material(current),
                                });
                            }
                        }
                    });
            });
        }
        world_view::LcdValue::None => {}
    }
}

/// Show Properties panel using UIWorldSnapshot - EDITABLE version
/// Organized by category like Roblox Studio
fn show_properties_panel(
    ui: &mut egui::Ui,
    snapshot: &UIWorldSnapshot,
    action_queue: &mut UIActionQueue,
    selected_service: Option<explorer::ServiceType>,
    soul_settings: &mut crate::soul::SoulServiceSettings,
    workspace_res: &mut eustress_common::services::Workspace,
    workspace_ui: &mut service_properties::WorkspaceService,
    players_service: &mut service_properties::PlayersService,
    lighting_service: &mut service_properties::LightingService,
    sound_service: &mut service_properties::SoundServiceService,
) {
    ui.add_space(4.0);  // Top padding
    ui.horizontal(|ui| {
        ui.add_space(8.0);  // Left padding
        ui.heading("⚙ Properties");
    });
    
    // If entities are selected, show entity properties (takes priority over service)
    // This ensures clicking an entity in Explorer immediately shows its properties
    if !snapshot.selected_entities.is_empty() {
        // Fall through to entity properties below
    } else if let Some(service) = selected_service {
        // Only show service properties if NO entities are selected
        show_service_properties_inline(ui, service, soul_settings, workspace_res, workspace_ui, players_service, lighting_service, sound_service);
        return;
    }
    
    if snapshot.selected_entities.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.label("❓ No entity selected");
            ui.add_space(10.0);
            ui.weak("Click on an object in the viewport\nor a service in the Explorer\nto view its properties");
        });
        return;
    }
    
    // Get all selected entities for multi-select property editing
    let selected_entities: Vec<Entity> = snapshot.selected_entities.clone();
    let multi_select = selected_entities.len() > 1;
    
    // Compute LCD values for multi-selection
    let lcd = if multi_select { snapshot.compute_lcd() } else { world_view::LcdSnapshot::default() };
    
    // Wrap entity properties in ScrollArea
    egui::ScrollArea::vertical()
        .id_salt("properties_panel_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
    
    // Show properties for first selected entity (values displayed are from first entity)
    if let Some(entity) = snapshot.selected_entities.first() {
        if let Some(entity_data) = snapshot.get(*entity) {
            // Show multi-select indicator
            if multi_select {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("📦 {} objects selected", selected_entities.len()))
                        .color(egui::Color32::from_rgb(100, 180, 255)));
                });
                ui.add_space(4.0);
            }
            // ════════════════════════════════════════════════════════════════
            // Header - Name and Class
            // ════════════════════════════════════════════════════════════════
            ui.horizontal(|ui| {
                ui.add_space(8.0);  // Left padding
                ui.label("Name:");
                if multi_select {
                    // Multi-select: show "(multiple)" for names
                    ui.weak("— (multiple)");
                } else {
                    let name_id = egui::Id::new(("property_name_buffer", *entity));
                    
                    // Use egui memory to persist the edit buffer between frames
                    let mut name = ui.ctx().data_mut(|d| {
                        d.get_temp_mut_or_insert_with(name_id, || entity_data.name.clone()).clone()
                    });
                    
                    let text_edit_id = egui::Id::new(("property_name_edit", *entity));
                    let response = egui::TextEdit::singleline(&mut name)
                        .id(text_edit_id)
                        .show(ui);
                    
                    // Update the buffer in memory
                    if response.response.changed() {
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(name_id, name.clone());
                        });
                    }
                    
                    // Send the rename action when focus is lost (user finished editing)
                    if response.response.lost_focus() {
                        let current_name = ui.ctx().data(|d| d.get_temp::<String>(name_id)).unwrap_or_default();
                        if current_name != entity_data.name {
                            action_queue.push(UIAction::SetProperty {
                                entity: *entity,
                                property: "Name".to_string(),
                                value: crate::classes::PropertyValue::String(current_name),
                            });
                        }
                    }
                    
                    // Reset buffer if entity changed or name was updated externally
                    if !response.response.has_focus() {
                        ui.ctx().data_mut(|d| {
                            let stored: Option<String> = d.get_temp(name_id);
                            if stored.as_ref() != Some(&entity_data.name) {
                                d.insert_temp(name_id, entity_data.name.clone());
                            }
                        });
                    }
                }
            });
            
            ui.horizontal(|ui| {
                ui.add_space(8.0);  // Left padding
                ui.label("Class:");
                if multi_select {
                    if let Some(class) = lcd.class_name {
                        ui.label(egui::RichText::new(class.as_str()).strong());
                    } else {
                        ui.weak("— (mixed)");
                    }
                } else {
                    ui.label(egui::RichText::new(entity_data.class_name.as_str()).strong());
                }
            });
            
            // AI Training Opt-In (universal Instance property)
            if let Some(ai_enabled) = entity_data.ai {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);  // Left padding
                    let mut ai = ai_enabled;
                    if ui.checkbox(&mut ai, "AI Training")
                        .on_hover_text("Include this entity in SpatialVortex training data exports")
                        .changed() 
                    {
                        action_queue.push(world_view::UIAction::SetProperty {
                            entity: *entity,
                            property: "AI".to_string(),
                            value: crate::classes::PropertyValue::Bool(ai),
                        });
                    }
                });
            }
            
            // Assembly Mass for Model/Folder (computed from all BasePart descendants)
            if !multi_select && matches!(entity_data.class_name, crate::classes::ClassName::Model | crate::classes::ClassName::Folder) {
                if let Some(assembly_mass) = entity_data.assembly_mass {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);  // Left padding
                        ui.label("Assembly Mass:");
                        ui.label(egui::RichText::new(format!("{:.2} kg", assembly_mass)).weak());
                    });
                }
            }
            
            ui.add_space(8.0);
            
            // ════════════════════════════════════════════════════════════════
            // Appearance Category (with LCD support)
            // ════════════════════════════════════════════════════════════════
            let has_appearance = entity_data.color.is_some() || entity_data.material.is_some() || 
                                 entity_data.transparency.is_some() || entity_data.reflectance.is_some();
            if has_appearance {
                egui::CollapsingHeader::new("🎨 Appearance")
                    .default_open(true)
                    .show(ui, |ui| {
                        // Color - use LCD value for multi-select
                        let color_lcd = if multi_select { &lcd.color } else {
                            &match entity_data.color { Some(c) => world_view::LcdValue::Same(c), None => world_view::LcdValue::None }
                        };
                        render_color_property_lcd(ui, "Color", color_lcd, entity_data.color, &selected_entities, *entity, multi_select, action_queue);
                        
                        // Material - use LCD value for multi-select
                        let material_lcd = if multi_select { &lcd.material } else {
                            &match entity_data.material { Some(m) => world_view::LcdValue::Same(m), None => world_view::LcdValue::None }
                        };
                        render_material_property_lcd(ui, "Material", material_lcd, entity_data.material, &selected_entities, *entity, multi_select, action_queue);
                        
                        // Transparency - use LCD value for multi-select
                        let transparency_lcd = if multi_select { &lcd.transparency } else {
                            &match entity_data.transparency { Some(t) => world_view::LcdValue::Same(t), None => world_view::LcdValue::None }
                        };
                        render_float_property_lcd(ui, "Transparency", transparency_lcd, entity_data.transparency, &selected_entities, *entity, multi_select, action_queue, 0.0..=1.0);
                        
                        // Reflectance - use LCD value for multi-select
                        let reflectance_lcd = if multi_select { &lcd.reflectance } else {
                            &match entity_data.reflectance { Some(r) => world_view::LcdValue::Same(r), None => world_view::LcdValue::None }
                        };
                        render_float_property_lcd(ui, "Reflectance", reflectance_lcd, entity_data.reflectance, &selected_entities, *entity, multi_select, action_queue, 0.0..=1.0);
                    });
            }
            
            // ════════════════════════════════════════════════════════════════
            // Behavior Category (with LCD support)
            // ════════════════════════════════════════════════════════════════
            let has_behavior = entity_data.anchored.is_some() || entity_data.can_collide.is_some() || 
                               entity_data.can_touch.is_some() || entity_data.locked.is_some();
            if has_behavior {
                egui::CollapsingHeader::new("⚙ Behavior")
                    .default_open(true)
                    .show(ui, |ui| {
                        // Anchored - use LCD value for multi-select
                        let anchored_lcd = if multi_select { &lcd.anchored } else {
                            &match entity_data.anchored { Some(a) => world_view::LcdValue::Same(a), None => world_view::LcdValue::None }
                        };
                        render_bool_property_lcd(ui, "Anchored", anchored_lcd, entity_data.anchored, &selected_entities, *entity, multi_select, action_queue);
                        
                        // CanCollide - use LCD value for multi-select
                        let can_collide_lcd = if multi_select { &lcd.can_collide } else {
                            &match entity_data.can_collide { Some(c) => world_view::LcdValue::Same(c), None => world_view::LcdValue::None }
                        };
                        render_bool_property_lcd(ui, "CanCollide", can_collide_lcd, entity_data.can_collide, &selected_entities, *entity, multi_select, action_queue);
                        
                        // CanTouch - use LCD value for multi-select
                        let can_touch_lcd = if multi_select { &lcd.can_touch } else {
                            &match entity_data.can_touch { Some(ct) => world_view::LcdValue::Same(ct), None => world_view::LcdValue::None }
                        };
                        render_bool_property_lcd(ui, "CanTouch", can_touch_lcd, entity_data.can_touch, &selected_entities, *entity, multi_select, action_queue);
                        
                        // Locked - use LCD value for multi-select
                        let locked_lcd = if multi_select { &lcd.locked } else {
                            &match entity_data.locked { Some(l) => world_view::LcdValue::Same(l), None => world_view::LcdValue::None }
                        };
                        render_bool_property_lcd(ui, "Locked", locked_lcd, entity_data.locked, &selected_entities, *entity, multi_select, action_queue);
                    });
            }
            
            // ════════════════════════════════════════════════════════════════
            // Transform Category
            // ════════════════════════════════════════════════════════════════
            let has_transform = entity_data.position.is_some() || entity_data.orientation.is_some() || entity_data.size.is_some();
            if has_transform {
                egui::CollapsingHeader::new("📐 Transform")
                    .default_open(true)
                    .show(ui, |ui| {
                        // Position - use LCD value for multi-select
                        let pos_lcd = if multi_select { &lcd.position } else { 
                            &match entity_data.position { Some(p) => world_view::LcdValue::Same(p), None => world_view::LcdValue::None }
                        };
                        render_vec3_property_lcd(ui, "Position", pos_lcd, entity_data.position, &selected_entities, *entity, multi_select, action_queue);
                        
                        // Orientation - use LCD value for multi-select
                        let orient_lcd = if multi_select { &lcd.orientation } else {
                            &match entity_data.orientation { Some(o) => world_view::LcdValue::Same(o), None => world_view::LcdValue::None }
                        };
                        render_vec3_property_lcd(ui, "Orientation", orient_lcd, entity_data.orientation, &selected_entities, *entity, multi_select, action_queue);
                        
                        // Size - use LCD value for multi-select
                        let size_lcd = if multi_select { &lcd.size } else {
                            &match entity_data.size { Some(s) => world_view::LcdValue::Same(s), None => world_view::LcdValue::None }
                        };
                        render_vec3_property_lcd(ui, "Size", size_lcd, entity_data.size, &selected_entities, *entity, multi_select, action_queue);
                        
                        // Density - read-only display (computed from material)
                        if let Some(density) = entity_data.density {
                            ui.horizontal(|ui| {
                                ui.label("Density:");
                                ui.label(format!("{:.1} kg/m³", density));
                            });
                        }
                        
                        // Mass - read-only display (computed from density × volume)
                        if let Some(mass) = entity_data.mass {
                            ui.horizontal(|ui| {
                                ui.label("Mass:");
                                ui.label(format!("{:.2} kg", mass));
                            });
                        }
                    });
            }
            
            // ════════════════════════════════════════════════════════════════
            // Atmosphere Properties (for Atmosphere entities)
            // ════════════════════════════════════════════════════════════════
            if entity_data.class_name == crate::classes::ClassName::Atmosphere {
                egui::CollapsingHeader::new("☁ Atmosphere")
                    .default_open(true)
                    .show(ui, |ui| {
                        // Density slider
                        if let Some(density) = entity_data.atmosphere_density {
                            ui.horizontal(|ui| {
                                ui.label("Density:");
                                let mut val = density;
                                if ui.add(egui::Slider::new(&mut val, 0.0..=1.0)).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "AtmosphereDensity".to_string(),
                                        value: crate::classes::PropertyValue::Float(val),
                                    });
                                }
                            });
                        }
                        
                        // Offset slider
                        if let Some(offset) = entity_data.atmosphere_offset {
                            ui.horizontal(|ui| {
                                ui.label("Offset:");
                                let mut val = offset;
                                if ui.add(egui::Slider::new(&mut val, -1.0..=1.0)).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "AtmosphereOffset".to_string(),
                                        value: crate::classes::PropertyValue::Float(val),
                                    });
                                }
                            });
                        }
                        
                        // Glare slider
                        if let Some(glare) = entity_data.atmosphere_glare {
                            ui.horizontal(|ui| {
                                ui.label("Glare:");
                                let mut val = glare;
                                if ui.add(egui::Slider::new(&mut val, 0.0..=1.0)).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "AtmosphereGlare".to_string(),
                                        value: crate::classes::PropertyValue::Float(val),
                                    });
                                }
                            });
                        }
                        
                        // Haze slider
                        if let Some(haze) = entity_data.atmosphere_haze {
                            ui.horizontal(|ui| {
                                ui.label("Haze:");
                                let mut val = haze;
                                if ui.add(egui::Slider::new(&mut val, 0.0..=1.0)).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "AtmosphereHaze".to_string(),
                                        value: crate::classes::PropertyValue::Float(val),
                                    });
                                }
                            });
                        }
                        
                        ui.separator();
                        
                        // Color picker
                        if let Some(color) = entity_data.atmosphere_color {
                            ui.horizontal(|ui| {
                                ui.label("Color:");
                                let mut color_arr = [color[0], color[1], color[2]];
                                if ui.color_edit_button_rgb(&mut color_arr).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "AtmosphereColor".to_string(),
                                        value: crate::classes::PropertyValue::Color(Color::srgba(color_arr[0], color_arr[1], color_arr[2], 1.0)),
                                    });
                                }
                            });
                        }
                        
                        // Decay color picker
                        if let Some(decay) = entity_data.atmosphere_decay {
                            ui.horizontal(|ui| {
                                ui.label("Decay:");
                                let mut decay_arr = [decay[0], decay[1], decay[2]];
                                if ui.color_edit_button_rgb(&mut decay_arr).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "AtmosphereDecay".to_string(),
                                        value: crate::classes::PropertyValue::Color(Color::srgba(decay_arr[0], decay_arr[1], decay_arr[2], 1.0)),
                                    });
                                }
                            });
                        }
                        
                        ui.separator();
                        
                        // Presets
                        ui.label("Presets:");
                        ui.horizontal(|ui| {
                            if ui.button("Clear Day").clicked() {
                                action_queue.push(world_view::UIAction::SetAtmospherePreset { entity: *entity, preset: "clear_day".to_string() });
                            }
                            if ui.button("Sunset").clicked() {
                                action_queue.push(world_view::UIAction::SetAtmospherePreset { entity: *entity, preset: "sunset".to_string() });
                            }
                            if ui.button("Foggy").clicked() {
                                action_queue.push(world_view::UIAction::SetAtmospherePreset { entity: *entity, preset: "foggy".to_string() });
                            }
                        });
                    });
            }
            
            // ════════════════════════════════════════════════════════════════
            // BillboardGui Properties
            // ════════════════════════════════════════════════════════════════
            if entity_data.class_name == crate::classes::ClassName::BillboardGui {
                egui::CollapsingHeader::new("🖼️ BillboardGui")
                    .default_open(true)
                    .show(ui, |ui| {
                        // Active checkbox
                        ui.horizontal(|ui| {
                            ui.label("Active:");
                            let mut active = entity_data.billboard_active.unwrap_or(true);
                            if ui.checkbox(&mut active, "").changed() {
                                action_queue.push(world_view::UIAction::SetProperty {
                                    entity: *entity,
                                    property: "Active".to_string(),
                                    value: crate::classes::PropertyValue::Bool(active),
                                });
                            }
                        });
                        
                        // Enabled checkbox
                        ui.horizontal(|ui| {
                            ui.label("Enabled:");
                            let mut enabled = entity_data.billboard_enabled.unwrap_or(true);
                            if ui.checkbox(&mut enabled, "").changed() {
                                action_queue.push(world_view::UIAction::SetProperty {
                                    entity: *entity,
                                    property: "Enabled".to_string(),
                                    value: crate::classes::PropertyValue::Bool(enabled),
                                });
                            }
                        });
                        
                        // AlwaysOnTop checkbox
                        ui.horizontal(|ui| {
                            ui.label("AlwaysOnTop:");
                            let mut always_on_top = entity_data.billboard_always_on_top.unwrap_or(false);
                            if ui.checkbox(&mut always_on_top, "").changed() {
                                action_queue.push(world_view::UIAction::SetProperty {
                                    entity: *entity,
                                    property: "AlwaysOnTop".to_string(),
                                    value: crate::classes::PropertyValue::Bool(always_on_top),
                                });
                            }
                        });
                        
                        ui.separator();
                        
                        // Size
                        if let Some(size) = entity_data.billboard_size {
                            ui.horizontal(|ui| {
                                ui.label("Size:");
                                let mut x = size.x;
                                let mut y = size.y;
                                ui.add(egui::DragValue::new(&mut x).speed(1.0).prefix("X: "));
                                ui.add(egui::DragValue::new(&mut y).speed(1.0).prefix("Y: "));
                                if ui.button("Set").clicked() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "Size".to_string(),
                                        value: crate::classes::PropertyValue::Vector2([x, y]),
                                    });
                                }
                            });
                        }
                        
                        // UnitsOffset
                        if let Some(offset) = entity_data.billboard_units_offset {
                            ui.horizontal(|ui| {
                                ui.label("UnitsOffset:");
                                let mut x = offset.x;
                                let mut y = offset.y;
                                let mut z = offset.z;
                                ui.add(egui::DragValue::new(&mut x).speed(0.1).prefix("X: "));
                                ui.add(egui::DragValue::new(&mut y).speed(0.1).prefix("Y: "));
                                ui.add(egui::DragValue::new(&mut z).speed(0.1).prefix("Z: "));
                                if ui.button("Set").clicked() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "UnitsOffset".to_string(),
                                        value: crate::classes::PropertyValue::Vector3(bevy::math::Vec3::new(x, y, z)),
                                    });
                                }
                            });
                        }
                        
                        ui.separator();
                        
                        // MaxDistance
                        if let Some(max_dist) = entity_data.billboard_max_distance {
                            ui.horizontal(|ui| {
                                ui.label("MaxDistance:");
                                let mut val = max_dist;
                                if ui.add(egui::DragValue::new(&mut val).speed(1.0).range(0.0..=1000.0)).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "MaxDistance".to_string(),
                                        value: crate::classes::PropertyValue::Float(val),
                                    });
                                }
                            });
                        }
                        
                        // Brightness
                        if let Some(brightness) = entity_data.billboard_brightness {
                            ui.horizontal(|ui| {
                                ui.label("Brightness:");
                                let mut val = brightness;
                                if ui.add(egui::Slider::new(&mut val, 0.0..=2.0)).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "Brightness".to_string(),
                                        value: crate::classes::PropertyValue::Float(val),
                                    });
                                }
                            });
                        }
                        
                        // LightInfluence
                        if let Some(light_influence) = entity_data.billboard_light_influence {
                            ui.horizontal(|ui| {
                                ui.label("LightInfluence:");
                                let mut val = light_influence;
                                if ui.add(egui::Slider::new(&mut val, 0.0..=1.0)).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "LightInfluence".to_string(),
                                        value: crate::classes::PropertyValue::Float(val),
                                    });
                                }
                            });
                        }
                    });
            }
            
            // ════════════════════════════════════════════════════════════════
            // TextLabel Properties
            // ════════════════════════════════════════════════════════════════
            if entity_data.class_name == crate::classes::ClassName::TextLabel {
                egui::CollapsingHeader::new("📝 TextLabel")
                    .default_open(true)
                    .show(ui, |ui| {
                        // Text
                        if let Some(text) = &entity_data.textlabel_text {
                            ui.horizontal(|ui| {
                                ui.label("Text:");
                                let mut val = text.clone();
                                if ui.text_edit_singleline(&mut val).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "Text".to_string(),
                                        value: crate::classes::PropertyValue::String(val),
                                    });
                                }
                            });
                        }
                        
                        // FontSize
                        if let Some(font_size) = entity_data.textlabel_font_size {
                            ui.horizontal(|ui| {
                                ui.label("FontSize:");
                                let mut val = font_size;
                                if ui.add(egui::DragValue::new(&mut val).speed(0.5).range(1.0..=200.0)).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "FontSize".to_string(),
                                        value: crate::classes::PropertyValue::Float(val),
                                    });
                                }
                            });
                        }
                        
                        // Visible checkbox
                        ui.horizontal(|ui| {
                            ui.label("Visible:");
                            let mut visible = entity_data.textlabel_visible.unwrap_or(true);
                            if ui.checkbox(&mut visible, "").changed() {
                                action_queue.push(world_view::UIAction::SetProperty {
                                    entity: *entity,
                                    property: "Visible".to_string(),
                                    value: crate::classes::PropertyValue::Bool(visible),
                                });
                            }
                        });
                        
                        ui.separator();
                        
                        // TextColor3
                        if let Some(text_color) = entity_data.textlabel_text_color {
                            ui.horizontal(|ui| {
                                ui.label("TextColor3:");
                                let [r, g, b, _] = text_color.to_srgba().to_f32_array();
                                let mut color_arr = [r, g, b];
                                if ui.color_edit_button_rgb(&mut color_arr).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "TextColor3".to_string(),
                                        value: crate::classes::PropertyValue::Color(bevy::color::Color::srgb(color_arr[0], color_arr[1], color_arr[2])),
                                    });
                                }
                            });
                        }
                        
                        // BackgroundTransparency
                        if let Some(bg_trans) = entity_data.textlabel_background_transparency {
                            ui.horizontal(|ui| {
                                ui.label("BackgroundTransparency:");
                                let mut val = bg_trans;
                                if ui.add(egui::Slider::new(&mut val, 0.0..=1.0)).changed() {
                                    action_queue.push(world_view::UIAction::SetProperty {
                                        entity: *entity,
                                        property: "BackgroundTransparency".to_string(),
                                        value: crate::classes::PropertyValue::Float(val),
                                    });
                                }
                            });
                        }
                    });
            }
            
            // ════════════════════════════════════════════════════════════════
            // Tags Category - Full Editing Support
            // ════════════════════════════════════════════════════════════════
            egui::CollapsingHeader::new("🏷 Tags")
                .default_open(true)
                .show(ui, |ui| {
                    // Sort tags for consistent display
                    let mut sorted_tags: Vec<_> = entity_data.tags.iter().collect();
                    sorted_tags.sort();
                    
                    if sorted_tags.is_empty() {
                        ui.horizontal(|ui| {
                            ui.weak("No tags assigned");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(egui::Button::new("+ Add").min_size(egui::vec2(50.0, 18.0))).on_hover_text("Add new tag").clicked() {
                                    action_queue.push(world_view::UIAction::OpenAddTagDialog(*entity));
                                }
                            });
                        });
                    } else {
                        // Header with add button
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("{} tags", sorted_tags.len())).weak());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("➕").on_hover_text("Add new tag").clicked() {
                                    action_queue.push(world_view::UIAction::OpenAddTagDialog(*entity));
                                }
                            });
                        });
                        ui.separator();
                        
                        // Tags as removable chips
                        ui.horizontal_wrapped(|ui| {
                            for tag in &sorted_tags {
                                let tag_frame = egui::Frame::new()
                                    .fill(egui::Color32::from_rgb(50, 70, 100))
                                    .corner_radius(4.0)
                                    .inner_margin(egui::Margin::symmetric(6, 2));
                                
                                tag_frame.show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(*tag).color(egui::Color32::WHITE));
                                        if ui.small_button("×").on_hover_text("Remove tag").clicked() {
                                            action_queue.push(world_view::UIAction::RemoveTag {
                                                entity: *entity,
                                                tag: (*tag).clone(),
                                            });
                                        }
                                    });
                                });
                            }
                        });
                    }
                });
            
            // ════════════════════════════════════════════════════════════════
            // Attributes Category - Full Editing Support
            // ════════════════════════════════════════════════════════════════
            egui::CollapsingHeader::new("📦 Attributes")
                .default_open(true)
                .show(ui, |ui| {
                    // Sort attributes for consistent display
                    let mut sorted_attrs: Vec<_> = entity_data.attributes.iter().collect();
                    sorted_attrs.sort_by(|a, b| a.0.cmp(b.0));
                    
                    if sorted_attrs.is_empty() {
                        ui.horizontal(|ui| {
                            ui.weak("No attributes defined");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(egui::Button::new("+ Add").min_size(egui::vec2(50.0, 18.0))).on_hover_text("Add new attribute").clicked() {
                                    action_queue.push(world_view::UIAction::OpenAddAttributeDialog(*entity));
                                }
                            });
                        });
                    } else {
                        // Header row
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("{} attributes", sorted_attrs.len())).weak());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("➕").on_hover_text("Add new attribute").clicked() {
                                    action_queue.push(world_view::UIAction::OpenAddAttributeDialog(*entity));
                                }
                            });
                        });
                        ui.separator();
                        
                        // Attribute rows with edit/delete
                        for (key, value) in &sorted_attrs {
                            ui.horizontal(|ui| {
                                // Key with type indicator
                                let type_color = match value.type_name() {
                                    "String" => egui::Color32::from_rgb(152, 195, 121),
                                    "Number" | "Int" => egui::Color32::from_rgb(209, 154, 102),
                                    "Bool" => egui::Color32::from_rgb(198, 120, 221),
                                    "Vector3" | "Vector2" => egui::Color32::from_rgb(97, 175, 239),
                                    "Color" => egui::Color32::from_rgb(224, 108, 117),
                                    _ => egui::Color32::from_rgb(171, 178, 191),
                                };
                                ui.label(egui::RichText::new(format!("{}:", key)).color(type_color));
                                
                                // Value display (editable inline for simple types)
                                match value {
                                    eustress_common::attributes::AttributeValue::String(s) => {
                                        let mut val = s.clone();
                                        if ui.add(egui::TextEdit::singleline(&mut val).desired_width(100.0)).changed() {
                                            action_queue.push(world_view::UIAction::SetAttribute {
                                                entity: *entity,
                                                key: (*key).clone(),
                                                value: eustress_common::attributes::AttributeValue::String(val),
                                            });
                                        }
                                    }
                                    eustress_common::attributes::AttributeValue::Number(n) => {
                                        let mut val = *n as f32;
                                        if ui.add(egui::DragValue::new(&mut val).speed(0.1)).changed() {
                                            action_queue.push(world_view::UIAction::SetAttribute {
                                                entity: *entity,
                                                key: (*key).clone(),
                                                value: eustress_common::attributes::AttributeValue::Number(val as f64),
                                            });
                                        }
                                    }
                                    eustress_common::attributes::AttributeValue::Int(i) => {
                                        let mut val = *i as i32;
                                        if ui.add(egui::DragValue::new(&mut val).speed(1.0)).changed() {
                                            action_queue.push(world_view::UIAction::SetAttribute {
                                                entity: *entity,
                                                key: (*key).clone(),
                                                value: eustress_common::attributes::AttributeValue::Int(val as i64),
                                            });
                                        }
                                    }
                                    eustress_common::attributes::AttributeValue::Bool(b) => {
                                        let mut val = *b;
                                        if ui.checkbox(&mut val, "").changed() {
                                            action_queue.push(world_view::UIAction::SetAttribute {
                                                entity: *entity,
                                                key: (*key).clone(),
                                                value: eustress_common::attributes::AttributeValue::Bool(val),
                                            });
                                        }
                                    }
                                    _ => {
                                        // Complex types show read-only
                                        ui.label(value.display_value());
                                    }
                                }
                                
                                // Type badge
                                ui.weak(format!("[{}]", value.type_name()));
                                
                                // Delete button
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("🗑").on_hover_text("Remove attribute").clicked() {
                                        action_queue.push(world_view::UIAction::RemoveAttribute {
                                            entity: *entity,
                                            key: (*key).clone(),
                                        });
                                    }
                                });
                            });
                        }
                    }
                });
            
            // ════════════════════════════════════════════════════════════════
            // Parameters Category (Data Sources) - Simplified with Modal
            // ════════════════════════════════════════════════════════════════
            egui::CollapsingHeader::new("🔗 Parameters")
                .default_open(true)
                .show(ui, |ui| {
                    if entity_data.has_parameters {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Data source configured").color(egui::Color32::from_rgb(152, 195, 121)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("🗑 Remove").on_hover_text("Remove data source").clicked() {
                                    action_queue.push(world_view::UIAction::RemoveParameters(*entity));
                                }
                            });
                        });
                        ui.separator();
                        ui.weak("Configure in dedicated Parameters panel for full control");
                        if ui.button("📝 Open Parameters Editor").clicked() {
                            action_queue.push(world_view::UIAction::OpenParametersEditor(*entity));
                        }
                    } else {
                        ui.horizontal(|ui| {
                            ui.weak("No data source configured");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(egui::Button::new("+ Add").min_size(egui::vec2(50.0, 18.0))).on_hover_text("Connect to external data").clicked() {
                                    action_queue.push(world_view::UIAction::OpenAddParametersDialog(*entity));
                                }
                            });
                        });
                    }
                });
        }
    }
    
    // Add extra scroll padding at bottom
    ui.add_space(60.0);
    }); // End ScrollArea
}

// ============================================================================
// Properties Panel Modals
// ============================================================================

/// Render all properties panel modals
pub fn render_properties_modals(
    ctx: &egui::Context,
    modal_state: &mut world_view::PropertiesModalState,
    action_queue: &mut world_view::UIActionQueue,
) {
    // Add Tag Modal
    if modal_state.add_tag_open {
        if let Some(entity) = modal_state.add_tag_entity {
            render_add_tag_modal(ctx, modal_state, action_queue, entity);
        }
    }
    
    // Add Attribute Modal
    if modal_state.add_attr_open {
        if let Some(entity) = modal_state.add_attr_entity {
            render_add_attribute_modal(ctx, modal_state, action_queue, entity);
        }
    }
    
    // Add Parameters Modal
    if modal_state.add_params_open {
        if let Some(entity) = modal_state.add_params_entity {
            render_add_parameters_modal(ctx, modal_state, action_queue, entity);
        }
    }
}

/// Add Tag Modal - centered with common game + simulation tags
fn render_add_tag_modal(
    ctx: &egui::Context,
    modal_state: &mut world_view::PropertiesModalState,
    action_queue: &mut world_view::UIActionQueue,
    entity: Entity,
) {
    egui::Window::new("🏷 Add Tag")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([400.0, 450.0])
        .show(ctx, |ui| {
            ui.heading("Add Tag to Entity");
            ui.separator();
            
            // Custom tag input
            ui.horizontal(|ui| {
                ui.label("Custom Tag:");
                ui.text_edit_singleline(&mut modal_state.add_tag_custom);
                if ui.button("Add").clicked() && !modal_state.add_tag_custom.is_empty() {
                    action_queue.push(world_view::UIAction::AddTag {
                        entity,
                        tag: modal_state.add_tag_custom.clone(),
                    });
                    modal_state.add_tag_custom.clear();
                    modal_state.add_tag_open = false;
                }
            });
            
            ui.add_space(8.0);
            ui.separator();
            
            egui::ScrollArea::vertical().max_height(350.0).show(ui, |ui| {
                // Game Tags
                ui.heading("🎮 Game Tags");
                ui.horizontal_wrapped(|ui| {
                    let game_tags = [
                        "Interactable", "Collectible", "Hazard", "Platform", "Checkpoint",
                        "Spawn", "Trigger", "NPC", "Player", "Enemy", "Ally", "Boss",
                        "Weapon", "Armor", "Consumable", "Quest", "Objective", "Waypoint",
                        "Door", "Key", "Locked", "Unlocked", "Destructible", "Indestructible",
                    ];
                    for tag in game_tags {
                        if ui.button(tag).clicked() {
                            action_queue.push(world_view::UIAction::AddTag {
                                entity,
                                tag: tag.to_string(),
                            });
                            modal_state.add_tag_open = false;
                        }
                    }
                });
                
                ui.add_space(8.0);
                
                // Simulation Tags
                ui.heading("🔬 Simulation Tags");
                ui.horizontal_wrapped(|ui| {
                    let sim_tags = [
                        "Sensor", "Actuator", "Controller", "DataSource", "DataSink",
                        "Monitor", "Alert", "Threshold", "Calibrated", "Uncalibrated",
                        "Active", "Inactive", "Maintenance", "Critical", "Warning",
                        "Normal", "Anomaly", "Baseline", "Reference", "Target",
                    ];
                    for tag in sim_tags {
                        if ui.button(tag).clicked() {
                            action_queue.push(world_view::UIAction::AddTag {
                                entity,
                                tag: tag.to_string(),
                            });
                            modal_state.add_tag_open = false;
                        }
                    }
                });
                
                ui.add_space(8.0);
                
                // Healthcare/Medical Tags
                ui.heading("🏥 Healthcare Tags");
                ui.horizontal_wrapped(|ui| {
                    let health_tags = [
                        "Patient", "Device", "Vital", "Medication", "Procedure",
                        "Lab", "Imaging", "Diagnosis", "Treatment", "Monitoring",
                        "Emergency", "ICU", "Outpatient", "Inpatient", "Discharge",
                    ];
                    for tag in health_tags {
                        if ui.button(tag).clicked() {
                            action_queue.push(world_view::UIAction::AddTag {
                                entity,
                                tag: tag.to_string(),
                            });
                            modal_state.add_tag_open = false;
                        }
                    }
                });
                
                ui.add_space(8.0);
                
                // Industrial Tags
                ui.heading("🏭 Industrial Tags");
                ui.horizontal_wrapped(|ui| {
                    let industrial_tags = [
                        "Machine", "Conveyor", "Robot", "PLC", "SCADA",
                        "Production", "Quality", "Safety", "Efficiency", "Downtime",
                        "Scheduled", "Unscheduled", "Preventive", "Corrective",
                    ];
                    for tag in industrial_tags {
                        if ui.button(tag).clicked() {
                            action_queue.push(world_view::UIAction::AddTag {
                                entity,
                                tag: tag.to_string(),
                            });
                            modal_state.add_tag_open = false;
                        }
                    }
                });
            });
            
            ui.add_space(8.0);
            ui.separator();
            
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    modal_state.add_tag_open = false;
                }
            });
        });
}

/// Add Attribute Modal - with type selection for all 16 AttributeValue types
fn render_add_attribute_modal(
    ctx: &egui::Context,
    modal_state: &mut world_view::PropertiesModalState,
    action_queue: &mut world_view::UIActionQueue,
    entity: Entity,
) {
    use eustress_common::attributes::AttributeValue;
    use bevy::math::{Vec2, Vec3};
    use bevy::color::Color;
    use bevy::transform::components::Transform;
    
    // All 16 attribute types
    const ATTR_TYPES: [&str; 16] = [
        "String", "Number", "Int", "Bool",
        "Vector3", "Vector2", "Color", "BrickColor",
        "CFrame", "EntityRef", "UDim2", "Rect",
        "Font", "NumberRange", "NumberSequence", "ColorSequence",
    ];
    
    egui::Window::new("📦 Add Attribute")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([400.0, 320.0])
        .show(ctx, |ui| {
            ui.heading("Add Attribute to Entity");
            ui.separator();
            
            // Attribute name
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.add(egui::TextEdit::singleline(&mut modal_state.add_attr_name).desired_width(280.0));
            });
            
            // Attribute type dropdown
            ui.horizontal(|ui| {
                ui.label("Type:");
                egui::ComboBox::from_id_salt("attr_type_combo")
                    .selected_text(&modal_state.add_attr_type)
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        for t in ATTR_TYPES {
                            ui.selectable_value(&mut modal_state.add_attr_type, t.to_string(), t);
                        }
                    });
            });
            
            ui.add_space(8.0);
            
            // Value input based on type
            ui.group(|ui| {
                ui.label(egui::RichText::new("Value:").strong());
                match modal_state.add_attr_type.as_str() {
                    "String" => {
                        ui.text_edit_singleline(&mut modal_state.add_attr_value_str);
                    }
                    "Number" => {
                        let mut val = modal_state.add_attr_value_num as f32;
                        ui.add(egui::DragValue::new(&mut val).speed(0.1));
                        modal_state.add_attr_value_num = val as f64;
                    }
                    "Int" => {
                        let mut val = modal_state.add_attr_value_int as i32;
                        ui.add(egui::DragValue::new(&mut val).speed(1.0));
                        modal_state.add_attr_value_int = val as i64;
                    }
                    "Bool" => {
                        ui.checkbox(&mut modal_state.add_attr_value_bool, "Enabled");
                    }
                    "Vector3" => {
                        ui.horizontal(|ui| {
                            ui.label("X:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_vec3[0]).speed(0.1));
                            ui.label("Y:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_vec3[1]).speed(0.1));
                            ui.label("Z:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_vec3[2]).speed(0.1));
                        });
                    }
                    "Vector2" => {
                        ui.horizontal(|ui| {
                            ui.label("X:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_vec2[0]).speed(0.1));
                            ui.label("Y:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_vec2[1]).speed(0.1));
                        });
                    }
                    "Color" => {
                        ui.horizontal(|ui| {
                            ui.label("RGB:");
                            ui.color_edit_button_rgb(&mut modal_state.add_attr_value_color);
                        });
                    }
                    "BrickColor" => {
                        let mut val = modal_state.add_attr_value_brick_color as i32;
                        ui.horizontal(|ui| {
                            ui.label("BrickColor ID:");
                            ui.add(egui::DragValue::new(&mut val).range(0..=1032));
                        });
                        modal_state.add_attr_value_brick_color = val as u32;
                    }
                    "CFrame" => {
                        ui.label("Position (rotation defaults to identity):");
                        ui.horizontal(|ui| {
                            ui.label("X:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_vec3[0]).speed(0.1));
                            ui.label("Y:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_vec3[1]).speed(0.1));
                            ui.label("Z:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_vec3[2]).speed(0.1));
                        });
                    }
                    "EntityRef" => {
                        let mut val = modal_state.add_attr_value_int as i64;
                        ui.horizontal(|ui| {
                            ui.label("Entity ID:");
                            ui.add(egui::DragValue::new(&mut val).speed(1.0));
                        });
                        modal_state.add_attr_value_int = val;
                    }
                    "UDim2" => {
                        ui.horizontal(|ui| {
                            ui.label("ScaleX:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_udim2[0]).speed(0.01));
                            ui.label("OffsetX:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_udim2[1]).speed(1.0));
                        });
                        ui.horizontal(|ui| {
                            ui.label("ScaleY:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_udim2[2]).speed(0.01));
                            ui.label("OffsetY:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_udim2[3]).speed(1.0));
                        });
                    }
                    "Rect" => {
                        ui.horizontal(|ui| {
                            ui.label("Min X:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_rect[0]).speed(0.1));
                            ui.label("Min Y:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_rect[1]).speed(0.1));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max X:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_rect[2]).speed(0.1));
                            ui.label("Max Y:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_rect[3]).speed(0.1));
                        });
                    }
                    "Font" => {
                        ui.horizontal(|ui| {
                            ui.label("Family:");
                            ui.text_edit_singleline(&mut modal_state.add_attr_value_font_family);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Weight:");
                            let mut weight = modal_state.add_attr_value_font_weight as i32;
                            ui.add(egui::DragValue::new(&mut weight).range(100..=900).speed(100.0));
                            modal_state.add_attr_value_font_weight = weight as u16;
                        });
                    }
                    "NumberRange" => {
                        ui.horizontal(|ui| {
                            ui.label("Min:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_range[0]).speed(0.1));
                            ui.label("Max:");
                            ui.add(egui::DragValue::new(&mut modal_state.add_attr_value_range[1]).speed(0.1));
                        });
                    }
                    "NumberSequence" | "ColorSequence" => {
                        ui.label(egui::RichText::new("Creates default 2-keypoint sequence (0→1)").weak());
                    }
                    _ => {
                        ui.label("Select a type above");
                    }
                }
            });
            
            ui.add_space(8.0);
            ui.separator();
            
            ui.horizontal(|ui| {
                if ui.button("Add Attribute").clicked() && !modal_state.add_attr_name.is_empty() {
                    let value = match modal_state.add_attr_type.as_str() {
                        "String" => AttributeValue::String(modal_state.add_attr_value_str.clone()),
                        "Number" => AttributeValue::Number(modal_state.add_attr_value_num),
                        "Int" => AttributeValue::Int(modal_state.add_attr_value_int),
                        "Bool" => AttributeValue::Bool(modal_state.add_attr_value_bool),
                        "Vector3" => AttributeValue::Vector3(Vec3::new(
                            modal_state.add_attr_value_vec3[0],
                            modal_state.add_attr_value_vec3[1],
                            modal_state.add_attr_value_vec3[2],
                        )),
                        "Vector2" => AttributeValue::Vector2(Vec2::new(
                            modal_state.add_attr_value_vec2[0],
                            modal_state.add_attr_value_vec2[1],
                        )),
                        "Color" => AttributeValue::Color(Color::srgb(
                            modal_state.add_attr_value_color[0],
                            modal_state.add_attr_value_color[1],
                            modal_state.add_attr_value_color[2],
                        )),
                        "BrickColor" => AttributeValue::BrickColor(modal_state.add_attr_value_brick_color),
                        "CFrame" => AttributeValue::CFrame(Transform::from_xyz(
                            modal_state.add_attr_value_vec3[0],
                            modal_state.add_attr_value_vec3[1],
                            modal_state.add_attr_value_vec3[2],
                        )),
                        "EntityRef" => AttributeValue::EntityRef(modal_state.add_attr_value_int as u32),
                        "UDim2" => AttributeValue::UDim2 {
                            scale_x: modal_state.add_attr_value_udim2[0],
                            offset_x: modal_state.add_attr_value_udim2[1],
                            scale_y: modal_state.add_attr_value_udim2[2],
                            offset_y: modal_state.add_attr_value_udim2[3],
                        },
                        "Rect" => AttributeValue::Rect {
                            min: Vec2::new(modal_state.add_attr_value_rect[0], modal_state.add_attr_value_rect[1]),
                            max: Vec2::new(modal_state.add_attr_value_rect[2], modal_state.add_attr_value_rect[3]),
                        },
                        "Font" => AttributeValue::Font {
                            family: modal_state.add_attr_value_font_family.clone(),
                            weight: modal_state.add_attr_value_font_weight,
                            style: "Normal".to_string(),
                        },
                        "NumberRange" => AttributeValue::NumberRange {
                            min: modal_state.add_attr_value_range[0],
                            max: modal_state.add_attr_value_range[1],
                        },
                        "NumberSequence" => AttributeValue::NumberSequence(vec![
                            eustress_common::attributes::NumberSequenceKeypoint { time: 0.0, value: 0.0, envelope: 0.0 },
                            eustress_common::attributes::NumberSequenceKeypoint { time: 1.0, value: 1.0, envelope: 0.0 },
                        ]),
                        "ColorSequence" => AttributeValue::ColorSequence(vec![
                            eustress_common::attributes::ColorSequenceKeypoint { time: 0.0, color: Color::WHITE },
                            eustress_common::attributes::ColorSequenceKeypoint { time: 1.0, color: Color::WHITE },
                        ]),
                        _ => AttributeValue::String(String::new()),
                    };
                    action_queue.push(world_view::UIAction::SetAttribute {
                        entity,
                        key: modal_state.add_attr_name.clone(),
                        value,
                    });
                    modal_state.add_attr_open = false;
                }
                if ui.button("Cancel").clicked() {
                    modal_state.add_attr_open = false;
                }
            });
        });
}

/// Add Parameters Modal - with all DataSourceTypes organized by category
fn render_add_parameters_modal(
    ctx: &egui::Context,
    modal_state: &mut world_view::PropertiesModalState,
    action_queue: &mut world_view::UIActionQueue,
    entity: Entity,
) {
    use eustress_common::parameters::DataSourceType;
    
    egui::Window::new("🔗 Add Parameters")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([500.0, 500.0])
        .show(ctx, |ui| {
            ui.heading("Connect to External Data Source");
            ui.weak("Select a data source type to connect this entity to external systems");
            ui.separator();
            
            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                // General Data Formats
                ui.heading("📄 General Data Formats");
                ui.horizontal_wrapped(|ui| {
                    for (dst, label) in [
                        (DataSourceType::JSON, "JSON"),
                        (DataSourceType::CSV, "CSV"),
                        (DataSourceType::XML, "XML"),
                        (DataSourceType::Parquet, "Parquet"),
                        (DataSourceType::Excel, "Excel"),
                        (DataSourceType::GRPC, "gRPC"),
                    ] {
                        if ui.button(label).clicked() {
                            action_queue.push(world_view::UIAction::AddParameters { entity, source_type: dst });
                            modal_state.add_params_open = false;
                        }
                    }
                });
                
                ui.add_space(8.0);
                
                // Messaging & Streaming
                ui.heading("📨 Messaging & Streaming");
                ui.horizontal_wrapped(|ui| {
                    for (dst, label) in [
                        (DataSourceType::Kafka, "Kafka"),
                        (DataSourceType::AMQP, "AMQP/RabbitMQ"),
                        (DataSourceType::MQTT, "MQTT"),
                        (DataSourceType::WebSocket, "WebSocket"),
                        (DataSourceType::WebTransport, "WebTransport"),
                        (DataSourceType::SSE, "SSE"),
                    ] {
                        if ui.button(label).clicked() {
                            action_queue.push(world_view::UIAction::AddParameters { entity, source_type: dst });
                            modal_state.add_params_open = false;
                        }
                    }
                });
                
                ui.add_space(8.0);
                
                // Databases
                ui.heading("🗄 Databases");
                ui.horizontal_wrapped(|ui| {
                    for (dst, label) in [
                        (DataSourceType::PostgreSQL, "PostgreSQL"),
                        (DataSourceType::MySQL, "MySQL"),
                        (DataSourceType::SQLite, "SQLite"),
                        (DataSourceType::MongoDB, "MongoDB"),
                        (DataSourceType::Redis, "Redis"),
                        (DataSourceType::Snowflake, "Snowflake"),
                        (DataSourceType::BigQuery, "BigQuery"),
                    ] {
                        if ui.button(label).clicked() {
                            action_queue.push(world_view::UIAction::AddParameters { entity, source_type: dst });
                            modal_state.add_params_open = false;
                        }
                    }
                });
                
                ui.add_space(8.0);
                
                // Cloud Services
                ui.heading("☁ Cloud Services");
                ui.horizontal_wrapped(|ui| {
                    for (dst, label) in [
                        (DataSourceType::REST, "REST API"),
                        (DataSourceType::GraphQL, "GraphQL"),
                        (DataSourceType::S3, "AWS S3"),
                        (DataSourceType::AzureBlob, "Azure Blob"),
                        (DataSourceType::GCS, "Google Cloud Storage"),
                        (DataSourceType::Firebase, "Firebase"),
                        (DataSourceType::Supabase, "Supabase"),
                        (DataSourceType::Oracle, "Oracle"),
                        (DataSourceType::DigitalOcean, "DigitalOcean"),
                    ] {
                        if ui.button(label).clicked() {
                            action_queue.push(world_view::UIAction::AddParameters { entity, source_type: dst });
                            modal_state.add_params_open = false;
                        }
                    }
                });
                
                ui.add_space(8.0);
                
                // Industrial / IoT
                ui.heading("🏭 Industrial / IoT");
                ui.horizontal_wrapped(|ui| {
                    for (dst, label) in [
                        (DataSourceType::OPCUA, "OPC-UA"),
                        (DataSourceType::Modbus, "Modbus"),
                        (DataSourceType::BACnet, "BACnet"),
                        (DataSourceType::CoAP, "CoAP"),
                        (DataSourceType::LwM2M, "LwM2M"),
                    ] {
                        if ui.button(label).clicked() {
                            action_queue.push(world_view::UIAction::AddParameters { entity, source_type: dst });
                            modal_state.add_params_open = false;
                        }
                    }
                });
                
                ui.add_space(8.0);
                
                // Healthcare
                ui.heading("🏥 Healthcare");
                ui.horizontal_wrapped(|ui| {
                    for (dst, label) in [
                        (DataSourceType::FHIR, "FHIR R4"),
                        (DataSourceType::HL7v2, "HL7 v2"),
                        (DataSourceType::HL7v3, "HL7 v3"),
                        (DataSourceType::DICOM, "DICOM"),
                        (DataSourceType::CDA, "CDA"),
                        (DataSourceType::OMOP, "OMOP CDM"),
                        (DataSourceType::OpenEHR, "openEHR"),
                        (DataSourceType::IHE, "IHE Profiles"),
                        (DataSourceType::X12, "X12 EDI"),
                        (DataSourceType::NCPDP, "NCPDP"),
                    ] {
                        if ui.button(label).clicked() {
                            action_queue.push(world_view::UIAction::AddParameters { entity, source_type: dst });
                            modal_state.add_params_open = false;
                        }
                    }
                });
                
                ui.add_space(8.0);
                
                // Specialty
                ui.heading("📁 Specialty");
                ui.horizontal_wrapped(|ui| {
                    for (dst, label) in [
                        (DataSourceType::LDAP, "LDAP"),
                        (DataSourceType::SFTP, "SFTP"),
                        (DataSourceType::FTP, "FTP"),
                        (DataSourceType::Email, "Email/IMAP"),
                        (DataSourceType::RSS, "RSS/Atom"),
                    ] {
                        if ui.button(label).clicked() {
                            action_queue.push(world_view::UIAction::AddParameters { entity, source_type: dst });
                            modal_state.add_params_open = false;
                        }
                    }
                });
            });
            
            ui.add_space(8.0);
            ui.separator();
            
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    modal_state.add_params_open = false;
                }
            });
        });
}

/// Show properties for a selected service with editable properties
fn show_service_properties_inline(
    ui: &mut egui::Ui, 
    service: explorer::ServiceType,
    soul_settings: &mut crate::soul::SoulServiceSettings,
    workspace_res: &mut eustress_common::services::Workspace,
    workspace_ui: &mut service_properties::WorkspaceService,
    players_service: &mut service_properties::PlayersService,
    lighting_service: &mut service_properties::LightingService,
    sound_service: &mut service_properties::SoundServiceService,
) {
    use explorer::ServiceType;
    
    // Service header
    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.label(egui::RichText::new(service.name()).strong());
    });
    
    ui.horizontal(|ui| {
        ui.label("Class:");
        ui.label(egui::RichText::new(service.class_name()).strong().color(egui::Color32::from_rgb(100, 149, 237)));
    });
    
    ui.add_space(8.0);
    ui.separator();
    
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            match service {
                ServiceType::Workspace => {
                    // Gravity - edit the Y component magnitude (positive value, applied as negative)
                    let mut gravity_magnitude = workspace_res.gravity.y.abs();
                    ui.horizontal(|ui| {
                        ui.label("Gravity:");
                        if ui.add(egui::DragValue::new(&mut gravity_magnitude).speed(0.1).suffix(" m/s²").range(0.0..=100.0)).changed() {
                            workspace_res.gravity.y = -gravity_magnitude;
                            workspace_ui.gravity = gravity_magnitude;
                        }
                    });
                    
                    // FallenPartsDestroyHeight
                    ui.horizontal(|ui| {
                        ui.label("FallenPartsDestroyHeight:");
                        if ui.add(egui::DragValue::new(&mut workspace_res.fall_height).speed(1.0).suffix(" m")).changed() {
                            workspace_ui.fallen_parts_destroy_height = workspace_res.fall_height;
                        }
                    });
                    
                    // StreamingEnabled
                    if ui.checkbox(&mut workspace_res.streaming_enabled, "StreamingEnabled").changed() {
                        workspace_ui.streaming_enabled = workspace_res.streaming_enabled;
                    }
                }
                ServiceType::Lighting => {
                    // Brightness
                    ui.horizontal(|ui| {
                        ui.label("Brightness:");
                        ui.add(egui::DragValue::new(&mut lighting_service.brightness).speed(0.01).range(0.0..=10.0));
                    });
                    
                    // ClockTime (0-24 hours)
                    ui.horizontal(|ui| {
                        ui.label("ClockTime:");
                        ui.add(egui::DragValue::new(&mut lighting_service.clock_time).speed(0.1).range(0.0..=24.0).suffix("h"));
                    });
                    
                    // GlobalShadows
                    ui.checkbox(&mut lighting_service.global_shadows, "GlobalShadows");
                    
                    // GeographicLatitude
                    ui.horizontal(|ui| {
                        ui.label("GeographicLatitude:");
                        ui.add(egui::DragValue::new(&mut lighting_service.geographic_latitude).speed(0.1).range(-90.0..=90.0).suffix("°"));
                    });
                    
                    // ExposureCompensation
                    ui.horizontal(|ui| {
                        ui.label("ExposureCompensation:");
                        ui.add(egui::DragValue::new(&mut lighting_service.exposure_compensation).speed(0.1).range(-5.0..=5.0));
                    });
                    
                    // ShadowSoftness
                    ui.horizontal(|ui| {
                        ui.label("ShadowSoftness:");
                        ui.add(egui::DragValue::new(&mut lighting_service.shadow_softness).speed(0.01).range(0.0..=1.0));
                    });
                    
                    // EnvironmentDiffuseScale
                    ui.horizontal(|ui| {
                        ui.label("EnvironmentDiffuseScale:");
                        ui.add(egui::DragValue::new(&mut lighting_service.environment_diffuse_scale).speed(0.01).range(0.0..=2.0));
                    });
                    
                    // EnvironmentSpecularScale
                    ui.horizontal(|ui| {
                        ui.label("EnvironmentSpecularScale:");
                        ui.add(egui::DragValue::new(&mut lighting_service.environment_specular_scale).speed(0.01).range(0.0..=2.0));
                    });
                }
                ServiceType::Players => {
                    // MaxPlayers
                    ui.horizontal(|ui| {
                        ui.label("MaxPlayers:");
                        ui.add(egui::DragValue::new(&mut players_service.max_players).speed(1.0).range(1..=100));
                    });
                    
                    // RespawnTime
                    ui.horizontal(|ui| {
                        ui.label("RespawnTime:");
                        ui.add(egui::DragValue::new(&mut players_service.respawn_time).speed(0.1).suffix("s").range(0.0..=60.0));
                    });
                    
                    // CharacterAutoLoads
                    ui.checkbox(&mut players_service.character_auto_loads, "CharacterAutoLoads");
                }
                ServiceType::SoundService => {
                    // DistanceFactor
                    ui.horizontal(|ui| {
                        ui.label("DistanceFactor:");
                        ui.add(egui::DragValue::new(&mut sound_service.distance_factor).speed(0.1).range(0.1..=100.0));
                    });
                    
                    // DopplerScale
                    ui.horizontal(|ui| {
                        ui.label("DopplerScale:");
                        ui.add(egui::DragValue::new(&mut sound_service.doppler_scale).speed(0.1).range(0.0..=10.0));
                    });
                    
                    // RolloffScale
                    ui.horizontal(|ui| {
                        ui.label("RolloffScale:");
                        ui.add(egui::DragValue::new(&mut sound_service.rolloff_scale).speed(0.1).range(0.0..=10.0));
                    });
                    
                    // RespectFilteringEnabled
                    ui.checkbox(&mut sound_service.respect_filtering_enabled, "RespectFilteringEnabled");
                }
                ServiceType::Teams => {
                    ui.weak("Container for Team objects.");
                    ui.add_space(4.0);
                    ui.weak("Add Team children to create player teams.");
                }
                ServiceType::SoulService => {
                    egui::CollapsingHeader::new("Soul Service Settings")
                        .default_open(true)
                        .show(ui, |ui| {
                            // Show current key source
                            if soul_settings.use_global_key {
                                ui.horizontal(|ui| {
                                    ui.label("[Global]");
                                    ui.label("Using global API key");
                                    if ui.small_button("Use custom key").clicked() {
                                        soul_settings.use_global_key = false;
                                    }
                                });
                            } else {
                                // Per-space API Key field
                                ui.horizontal(|ui| {
                                    ui.label("API Key:");
                                    let api_key_id = egui::Id::new("soul_service_api_key");
                                    
                                    // Use password field to hide the key
                                    let response = egui::TextEdit::singleline(&mut soul_settings.claude_api_key)
                                        .id(api_key_id)
                                        .password(true)
                                        .hint_text("sk-ant-...")
                                        .desired_width(150.0)
                                        .show(ui);
                                    
                                    // Show validation status
                                    if let Some(valid) = soul_settings.api_key_valid {
                                        if valid {
                                            ui.label(egui::RichText::new("[OK]").color(egui::Color32::GREEN));
                                        } else {
                                            ui.label(egui::RichText::new("[X]").color(egui::Color32::RED));
                                        }
                                    }
                                    
                                    // Clear validation when key changes
                                    if response.response.changed() {
                                        soul_settings.api_key_valid = None;
                                    }
                                });
                                
                                ui.horizontal(|ui| {
                                    if ui.small_button("Use global key").clicked() {
                                        soul_settings.use_global_key = true;
                                        soul_settings.api_key_valid = None;
                                    }
                                });
                            }
                            
                            // Info link to Claude API portal
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("ℹ").size(14.0).color(egui::Color32::from_rgb(100, 149, 237)));
                                if ui.link("Claude API").on_hover_text("Open Anthropic Console to get an API key").clicked() {
                                    let _ = open::that("https://console.anthropic.com/");
                                }
                                ui.label(" | ");
                                if ui.link("Soul Settings").on_hover_text("Open Soul Settings (File → Soul Settings)").clicked() {
                                    // This would need to set state.show_soul_settings_window = true
                                    // but we don't have access to state here, so just show a tooltip
                                }
                            });
                            
                            ui.add_space(8.0);
                            ui.weak("The API key is used to compile Soul scripts\ninto executable Rust code via Claude AI.");
                        });
                }
                _ => {
                    // Generic service info
                    egui::CollapsingHeader::new("📋 Service Info")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.label(format!("Type: {}", service.class_name()));
                            ui.label("Parent: Experience");
                            ui.add_space(4.0);
                            ui.weak("This service contains game data and scripts.");
                        });
                }
            }
            
        });
}

/// Keybindings settings window system
fn keybindings_window_system(
    mut contexts: EguiContexts,
    mut state: ResMut<StudioState>,
    mut keybindings: ResMut<crate::keybindings::KeyBindings>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    if !state.show_keybindings_window {
        return;
    }
    
    let Ok(ctx) = contexts.ctx_mut() else { return; };
    
    use crate::keybindings::Action;
    
    let mut close_window = false;
    
    egui::Window::new("⌨️ Keyboard Shortcuts")
        .default_width(500.0)
        .collapsible(false)
        .open(&mut state.show_keybindings_window)
        .show(ctx, |ui| {
            ui.heading("Customize Keyboard Shortcuts");
            ui.separator();
            
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Pre-fetch all binding strings to avoid borrow conflicts
                let bindings = vec![
                    ("Select Tool", keybindings.get_string(Action::SelectTool)),
                    ("Move Tool", keybindings.get_string(Action::MoveTool)),
                    ("Rotate Tool", keybindings.get_string(Action::RotateTool)),
                    ("Scale Tool", keybindings.get_string(Action::ScaleTool)),
                    ("Undo", keybindings.get_string(Action::Undo)),
                    ("Redo", keybindings.get_string(Action::Redo)),
                    ("Toggle Explorer", keybindings.get_string(Action::ToggleExplorer)),
                    ("Toggle Properties", keybindings.get_string(Action::ToggleProperties)),
                    ("Toggle Output", keybindings.get_string(Action::ToggleOutput)),
                ];
                
                egui::Grid::new("keybindings_grid")
                    .num_columns(2)
                    .spacing([40.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Action");
                        ui.label("Shortcut");
                        ui.end_row();
                        
                        // Tool shortcuts
                        ui.strong("🛠️ Tools");
                        ui.label("");
                        ui.end_row();
                        
                        for (label, binding) in &bindings[0..4] {
                            show_keybinding_row(ui, label, binding.clone());
                        }
                        
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                        
                        // Edit shortcuts
                        ui.strong("✏️ Edit");
                        ui.label("");
                        ui.end_row();
                        
                        for (label, binding) in &bindings[4..6] {
                            show_keybinding_row(ui, label, binding.clone());
                        }
                        
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                        
                        // View shortcuts
                        ui.strong("👁️ View");
                        ui.label("");
                        ui.end_row();
                        
                        for (label, binding) in &bindings[6..9] {
                            show_keybinding_row(ui, label, binding.clone());
                        }
                    });
            });
            
            ui.separator();
            
            ui.horizontal(|ui| {
                if ui.button("💾 Save to File").clicked() {
                    match keybindings.save() {
                        Ok(_) => notifications.success("Keyboard shortcuts saved!"),
                        Err(e) => notifications.error(format!("Failed to save: {}", e)),
                    }
                }
                
                if ui.button("🔄 Reset to Defaults").clicked() {
                    *keybindings = crate::keybindings::KeyBindings::default();
                    notifications.info("Keyboard shortcuts reset to defaults");
                }
                
                if ui.button("Close").clicked() {
                    close_window = true;
                }
            });
        });
    
    if close_window {
        state.show_keybindings_window = false;
    }
}

/// Soul Settings window system - manages global and per-space API keys
fn soul_settings_window_system(
    mut contexts: EguiContexts,
    mut state: ResMut<StudioState>,
    mut global_settings: ResMut<crate::soul::GlobalSoulSettings>,
    mut space_settings: ResMut<crate::soul::SoulServiceSettings>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    if !state.show_soul_settings_window {
        return;
    }
    
    let Ok(ctx) = contexts.ctx_mut() else { return; };
    
    let mut close_window = false;
    
    // Copy values to local state for checkbox editing
    let mut use_global_for_new_spaces = global_settings.use_global_for_new_spaces;
    let mut use_global_key = space_settings.use_global_key;
    
    // Center the window on screen
    let screen_rect = ctx.screen_rect();
    let window_size = egui::vec2(450.0, 400.0);
    let default_pos = egui::pos2(
        (screen_rect.width() - window_size.x) / 2.0,
        (screen_rect.height() - window_size.y) / 2.0,
    );
    
    egui::Window::new("Soul Settings")
        .default_width(450.0)
        .default_pos(default_pos)
        .collapsible(false)
        .open(&mut state.show_soul_settings_window)
        .show(ctx, |ui| {
            ui.heading("Claude API Configuration");
            ui.add_space(8.0);
            
            // ════════════════════════════════════════════════════════════════
            // Global Settings Section
            // ════════════════════════════════════════════════════════════════
            egui::CollapsingHeader::new("Global Settings")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label("These settings persist across all sessions and spaces.");
                    ui.add_space(4.0);
                    
                    // Global API Key
                    ui.horizontal(|ui| {
                        ui.label("Global API Key:");
                        let response = egui::TextEdit::singleline(&mut global_settings.global_api_key)
                            .password(true)
                            .hint_text("sk-ant-...")
                            .desired_width(200.0)
                            .show(ui);
                        
                        // Show validation status
                        if let Some(valid) = global_settings.api_key_valid {
                            if valid {
                                ui.label(egui::RichText::new("[OK]").color(egui::Color32::GREEN));
                            } else {
                                ui.label(egui::RichText::new("[X]").color(egui::Color32::RED));
                            }
                        }
                        
                        // Clear validation when key changes
                        if response.response.changed() {
                            global_settings.api_key_valid = None;
                        }
                    });
                    
                    // Auto-fill new spaces option
                    ui.add_space(4.0);
                    ui.checkbox(&mut use_global_for_new_spaces, "Use global key for new spaces");
                    
                    // Info link
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Get API Key:");
                        if ui.link("Anthropic Console").on_hover_text("Opens console.anthropic.com").clicked() {
                            let _ = open::that("https://console.anthropic.com/");
                        }
                    });
                });
            
            ui.add_space(8.0);
            
            // ════════════════════════════════════════════════════════════════
            // Current Space Settings Section
            // ════════════════════════════════════════════════════════════════
            egui::CollapsingHeader::new("Current Space Settings")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label("These settings are saved with the current space/scene.");
                    ui.add_space(4.0);
                    
                    // Use global key checkbox
                    ui.checkbox(&mut use_global_key, "Use global API key for this space");
                    
                    // Show per-space key input only if not using global
                    if !use_global_key {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Space API Key:");
                            let response = egui::TextEdit::singleline(&mut space_settings.claude_api_key)
                                .password(true)
                                .hint_text("sk-ant-...")
                                .desired_width(200.0)
                                .show(ui);
                            
                            // Show validation status
                            if let Some(valid) = space_settings.api_key_valid {
                                if valid {
                                    ui.label(egui::RichText::new("[OK]").color(egui::Color32::GREEN));
                                } else {
                                    ui.label(egui::RichText::new("[X]").color(egui::Color32::RED));
                                }
                            }
                            
                            // Clear validation when key changes
                            if response.response.changed() {
                                space_settings.api_key_valid = None;
                            }
                        });
                    } else {
                        ui.add_space(4.0);
                        if global_settings.has_api_key() {
                            ui.label(egui::RichText::new("[OK] Using global API key").color(egui::Color32::GREEN));
                        } else {
                            ui.label(egui::RichText::new("[!] No global API key set").color(egui::Color32::YELLOW));
                        }
                    }
                    
                    // Show effective key status
                    ui.add_space(8.0);
                    let effective_key = space_settings.effective_api_key(&global_settings);
                    if effective_key.is_empty() {
                        ui.label(egui::RichText::new("[!] No API key configured - Soul compilation disabled").color(egui::Color32::YELLOW));
                    } else {
                        let key_preview = if effective_key.len() > 12 {
                            format!("{}...{}", &effective_key[..8], &effective_key[effective_key.len()-4..])
                        } else {
                            "****".to_string()
                        };
                        ui.label(format!("Effective key: {}", key_preview));
                    }
                });
            
            ui.add_space(8.0);
            
            // ════════════════════════════════════════════════════════════════
            // Telemetry Settings Section
            // ════════════════════════════════════════════════════════════════
            egui::CollapsingHeader::new("Telemetry & Error Reporting")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label("Help improve Eustress by sending anonymous error reports.");
                    ui.add_space(4.0);
                    
                    let mut telemetry_enabled = space_settings.telemetry_enabled;
                    if ui.checkbox(&mut telemetry_enabled, "Enable anonymous error reporting").changed() {
                        space_settings.telemetry_enabled = telemetry_enabled;
                        
                        // Initialize or shutdown telemetry based on toggle
                        if telemetry_enabled {
                            let settings = crate::telemetry::TelemetrySettings {
                                enabled: true,
                                custom_dsn: None,
                            };
                            crate::telemetry::init_telemetry(&settings);
                            notifications.info("Telemetry enabled - thank you for helping improve Eustress!");
                        } else {
                            crate::telemetry::shutdown_telemetry();
                            notifications.info("Telemetry disabled");
                        }
                    }
                    
                    ui.add_space(4.0);
                    ui.weak("When enabled, Rune script errors are sent to help us fix bugs.");
                    ui.weak("No personal data, API keys, or script content is shared.");
                    
                    // Show current status
                    ui.add_space(4.0);
                    if crate::telemetry::is_telemetry_enabled() {
                        ui.label(egui::RichText::new("📊 Telemetry: Active").color(egui::Color32::GREEN));
                    } else {
                        ui.label(egui::RichText::new("📊 Telemetry: Disabled").color(egui::Color32::GRAY));
                    }
                });
            
            ui.add_space(16.0);
            ui.separator();
            
            // ════════════════════════════════════════════════════════════════
            // Action Buttons
            // ════════════════════════════════════════════════════════════════
            ui.horizontal(|ui| {
                let save_btn = egui::Button::new("Save & Close")
                    .fill(egui::Color32::from_rgb(59, 130, 246)); // Blue
                if ui.add(save_btn).clicked() {
                    match global_settings.save() {
                        Ok(_) => {
                            notifications.success("Global Soul settings saved!");
                            close_window = true;
                        }
                        Err(e) => notifications.error(format!("Failed to save: {}", e)),
                    }
                }
                
                if ui.button("Cancel").clicked() {
                    close_window = true;
                }
            });
            
            ui.add_space(8.0);
            ui.weak("Note: Space settings are saved when you save the scene.");
        });
    
    // Sync checkbox values back to resources
    global_settings.use_global_for_new_spaces = use_global_for_new_spaces;
    space_settings.use_global_key = use_global_key;
    
    if close_window {
        state.show_soul_settings_window = false;
    }
}

/// Publish dialog system
fn publish_dialog_system(
    mut contexts: EguiContexts,
    mut state: ResMut<StudioState>,
    mut publish_state: ResMut<publish::PublishState>,
    auth_state: Res<crate::auth::AuthState>,
    scene_file: Res<SceneFile>,
) {
    // Sync auth token to publish state
    if auth_state.is_logged_in() {
        publish_state.auth_token = auth_state.token.clone();
    } else {
        publish_state.auth_token = None;
    }
    
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let scene_path = scene_file.path.as_ref().map(|p| p.to_string_lossy());
    let scene_path_str = scene_path.as_deref();
    publish::show_publish_dialog(ctx, &mut state, &mut publish_state, &scene_file.name, scene_path_str);
}

/// Helper function to show a keybinding row
fn show_keybinding_row(ui: &mut egui::Ui, label: &str, binding: String) {
    ui.label(label);
    ui.label(binding);
    ui.end_row();
}

/// Handle window close request (X button) - show confirmation if unsaved changes
fn handle_window_close_request(
    mut close_events: EventReader<bevy::window::WindowCloseRequested>,
    mut state: ResMut<StudioState>,
    scene_file: Res<SceneFile>,
    mut commands: Commands,
    windows: Query<Entity, With<bevy::window::Window>>,
) {
    for event in close_events.read() {
        // Check if there are unsaved changes
        if scene_file.modified || state.has_unsaved_changes {
            // Show confirmation modal instead of closing
            state.show_exit_confirmation = true;
        } else {
            // No unsaved changes - close the window directly
            if let Ok(window_entity) = windows.get(event.window) {
                commands.entity(window_entity).despawn();
            }
        }
    }
}

/// Exit confirmation modal system - shows when user tries to exit with unsaved changes
fn exit_confirmation_system(
    mut contexts: EguiContexts,
    mut state: ResMut<StudioState>,
    mut commands: Commands,
    windows: Query<Entity, With<bevy::window::Window>>,
) {
    if !state.show_exit_confirmation {
        return;
    }
    
    let Ok(ctx) = contexts.ctx_mut() else { return };
    
    egui::Window::new("Unsaved Changes")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.label("You have unsaved changes. Would you like to save before exiting?");
                ui.add_space(20.0);
                
                ui.horizontal(|ui| {
                    if ui.button("Save and Exit").clicked() {
                        state.show_exit_confirmation = false;
                        // Set pending file action to save, then exit
                        state.pending_file_action = Some(FileEvent::SaveScene);
                        // Schedule exit after save completes
                        std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            std::process::exit(0);
                        });
                    }
                    
                    if ui.button("Don't Save").clicked() {
                        state.show_exit_confirmation = false;
                        // Close window without saving
                        for window_entity in windows.iter() {
                            commands.entity(window_entity).despawn();
                        }
                    }
                    
                    if ui.button("Cancel").clicked() {
                        state.show_exit_confirmation = false;
                    }
                });
                ui.add_space(10.0);
            });
        });
}

/// Parse Entity from debug string format "Entity(XvY)" or "0v1"
/// DEPRECATED: Selection now uses part_id strings, use find_entity_by_part_id instead
fn parse_entity_from_string(s: &str) -> Option<Entity> {
    // Remove "Entity(" prefix and ")" suffix if present
    let s = s.trim();
    let s = if s.starts_with("Entity(") && s.ends_with(')') {
        &s[7..s.len()-1]
    } else {
        s
    };
    
    // Parse "XvY" format
    if let Some(v_pos) = s.find('v') {
        let index_str = &s[..v_pos];
        let gen_str = &s[v_pos+1..];
        
        if let (Ok(index), Ok(generation)) = (index_str.parse::<u32>(), gen_str.parse::<u32>()) {
            // Entity::from_bits expects (generation << 32) | index
            let bits = ((generation as u64) << 32) | (index as u64);
            return Some(Entity::from_bits(bits));
        }
    }
    
    None
}

/// Find entity by part_id string (used by selection system)
/// 
/// The selection system stores IDs in two formats:
/// 1. PartEntity.part_id - the actual part ID string
/// 2. Entity index format: "indexVgeneration" e.g. "70v0"
/// 
/// This function checks both formats.
fn find_entity_by_part_id(world: &World, part_id: &str) -> Option<Entity> {
    // First, try to parse as entity index format (e.g., "70v0")
    if let Some((index_str, gen_str)) = part_id.split_once('v') {
        if let (Ok(index), Ok(generation)) = (index_str.parse::<u32>(), gen_str.parse::<u32>()) {
            let entity = Entity::from_bits(((generation as u64) << 32) | (index as u64));
            // Verify entity exists in world
            if world.get_entity(entity).is_ok() {
                return Some(entity);
            }
        }
    }
    
    // Second, try to match against PartEntity.part_id
    for entity in world.iter_entities() {
        if let Some(part_entity) = entity.get::<crate::rendering::PartEntity>() {
            if part_entity.part_id == part_id {
                return Some(entity.id());
            }
        }
    }
    
    // Third, try Entity debug format "{:?}" which is "70v0" style
    // This handles cases where selection was stored with format!("{:?}", entity)
    for entity in world.iter_entities() {
        let entity_debug = format!("{:?}", entity.id());
        if entity_debug == part_id {
            return Some(entity.id());
        }
        // Also check "indexVgeneration" format
        let entity_index_format = format!("{}v{}", entity.id().index(), entity.id().generation());
        if entity_index_format == part_id {
            return Some(entity.id());
        }
    }
    
    None
}

/// Helper function to collect an entity and all its children recursively
/// Used for Lock/Unlock/Anchor operations on Models and Folders
fn collect_entity_and_children(world: &World, entity: Entity, result: &mut Vec<Entity>) {
    // Add this entity
    result.push(entity);
    
    // Get children and recurse
    if let Ok(entity_ref) = world.get_entity(entity) {
        if let Some(children) = entity_ref.get::<Children>() {
            for child in children.iter() {
                collect_entity_and_children(world, child, result);
            }
        }
    }
}

/// System to update egui input state resource (runs before keyboard shortcuts)
fn update_egui_input_state(
    mut egui_ctx: EguiContexts,
    mut input_state: ResMut<EguiInputState>,
) {
    let Ok(ctx) = egui_ctx.ctx_mut() else {
        input_state.wants_keyboard = false;
        input_state.wants_pointer = false;
        return;
    };
    
    // Check if egui wants keyboard input (text fields, etc.)
    input_state.wants_keyboard = ctx.wants_keyboard_input();
    // Check if egui wants pointer input (dragging sliders, scrolling, etc.)
    input_state.wants_pointer = ctx.wants_pointer_input() || ctx.is_using_pointer();
}

/// Keyboard shortcuts system (exclusive - only uses World)
fn keyboard_shortcuts_exclusive(world: &mut World) {
    // SAFETY: We use UnsafeWorldCell to split borrows safely
    let world_cell = world.as_unsafe_world_cell();
    
    // Check if egui wants keyboard input - if so, skip all shortcuts except menu actions
    let egui_wants_keyboard = unsafe {
        world_cell.get_resource::<EguiInputState>()
            .map(|s| s.wants_keyboard)
            .unwrap_or(false)
    };
    
    // Get resources - SAFETY: We carefully manage borrows
    let (keys, keybindings, pending_menu_actions) = unsafe {
        let keys = world_cell.get_resource::<ButtonInput<KeyCode>>().unwrap();
        let keybindings = world_cell.get_resource::<crate::keybindings::KeyBindings>().unwrap();
        let mut pending = world_cell.get_resource_mut::<PendingMenuActions>().unwrap();
        let actions = std::mem::take(&mut pending.actions); // Take and clear
        drop(pending);
        (keys, keybindings, actions)
    };
    
    // Now process all keybindings using world_cell for safe access
    use crate::keybindings::Action;
    
    // If egui wants keyboard input, only process menu actions (not keyboard shortcuts)
    // This prevents shortcuts from triggering while typing in text fields or dragging sliders
    if egui_wants_keyboard && pending_menu_actions.is_empty() {
        return;
    }
    
    // Helper to check keybindings only when egui doesn't want keyboard
    let check_key = |action: Action| -> bool {
        !egui_wants_keyboard && keybindings.check(action, &keys)
    };
    
    // Helper to check menu actions (always allowed) OR keybindings (only when egui doesn't want keyboard)
    let check_key_or_menu = |action: Action| -> bool {
        pending_menu_actions.contains(&action) || check_key(action)
    };
    
    // Tool shortcuts (Alt+Z/X/V/C by default)
    if check_key(Action::SelectTool) {
        unsafe {
            let mut studio_state = world_cell.get_resource_mut::<StudioState>().unwrap();
            studio_state.current_tool = Tool::Select;
        }
    }
    if check_key(Action::MoveTool) {
        unsafe {
            let mut studio_state = world_cell.get_resource_mut::<StudioState>().unwrap();
            studio_state.current_tool = Tool::Move;
        }
    }
    if check_key(Action::RotateTool) {
        unsafe {
            let mut studio_state = world_cell.get_resource_mut::<StudioState>().unwrap();
            studio_state.current_tool = Tool::Rotate;
        }
    }
    if check_key(Action::ScaleTool) {
        unsafe {
            let mut studio_state = world_cell.get_resource_mut::<StudioState>().unwrap();
            studio_state.current_tool = Tool::Scale;
        }
    }
    
    // View shortcuts
    if check_key(Action::ToggleExplorer) {
        unsafe {
            let mut studio_state = world_cell.get_resource_mut::<StudioState>().unwrap();
            studio_state.show_explorer = !studio_state.show_explorer;
        }
    }
    if check_key(Action::ToggleProperties) {
        unsafe {
            let mut studio_state = world_cell.get_resource_mut::<StudioState>().unwrap();
            studio_state.show_properties = !studio_state.show_properties;
        }
    }
    if check_key(Action::ToggleOutput) {
        unsafe {
            let mut studio_state = world_cell.get_resource_mut::<StudioState>().unwrap();
            studio_state.show_output = !studio_state.show_output;
        }
    }
    
    // Undo (Ctrl+Z)
    if check_key_or_menu(Action::Undo) {
        unsafe {
            let mut undo_stack = world_cell.get_resource_mut::<crate::undo::UndoStack>().unwrap();
            if let Some(action) = undo_stack.undo() {
                let description = action.description();
                drop(undo_stack);
                
                // Apply the undo action
                let world_mut = world_cell.world_mut();
                crate::undo::apply_undo_action(&action, world_mut);
                
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.info(format!("↶ Undid: {}", description));
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("Nothing to undo");
            }
        }
    }
    
    // Redo (Ctrl+Y)
    if check_key_or_menu(Action::Redo) {
        unsafe {
            let mut undo_stack = world_cell.get_resource_mut::<crate::undo::UndoStack>().unwrap();
            if let Some(action) = undo_stack.redo() {
                let description = action.description();
                drop(undo_stack);
                
                // Apply the redo action
                let world_mut = world_cell.world_mut();
                crate::undo::apply_redo_action(&action, world_mut);
                
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.info(format!("↷ Redid: {}", description));
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("Nothing to redo");
            }
        }
    }
    
    // Delete selected entities (Delete key, Backspace, or from menu)
    let delete_pressed = check_key_or_menu(Action::Delete) || 
                         (!egui_wants_keyboard && keys.just_pressed(KeyCode::Backspace));
    
    if delete_pressed {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            
            if !selected.is_empty() {
                // Collect entities to delete and their data for undo
                let (entities_to_delete, undo_actions): (Vec<Entity>, Vec<crate::undo::Action>) = {
                    let world = world_cell.world();
                    let mut entities = Vec::new();
                    let mut actions = Vec::new();
                    
                    for part_id in selected.iter() {
                        if let Some(entity) = find_entity_by_part_id(world, part_id) {
                            entities.push(entity);
                            
                            // Capture entity data for undo
                            let instance = world.get::<Instance>(entity);
                            let transform = world.get::<Transform>(entity);
                            let base_part = world.get::<BasePart>(entity);
                            
                            if let (Some(inst), Some(tf)) = (instance, transform) {
                                let (rx, ry, rz) = tf.rotation.to_euler(EulerRot::XYZ);
                                let part_data = crate::parts::PartData {
                                    id: inst.id,
                                    name: inst.name.clone(),
                                    part_type: crate::parts::PartType::Cube,
                                    position: tf.translation.to_array(),
                                    rotation: [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()],
                                    size: base_part.map(|bp| bp.size.to_array()).unwrap_or([1.0, 1.0, 1.0]),
                                    color: base_part.map(|bp| [bp.color.to_srgba().red, bp.color.to_srgba().green, bp.color.to_srgba().blue, 1.0]).unwrap_or([0.5, 0.5, 0.5, 1.0]),
                                    material: crate::parts::Material::Plastic, // Default - actual material conversion not needed for undo
                                    anchored: base_part.map(|bp| bp.anchored).unwrap_or(false),
                                    transparency: base_part.map(|bp| bp.transparency).unwrap_or(0.0),
                                    can_collide: base_part.map(|bp| bp.can_collide).unwrap_or(true),
                                    parent: None,
                                    locked: false,
                                };
                                actions.push(crate::undo::Action::DeletePart { data: part_data });
                            }
                        }
                    }
                    (entities, actions)
                };
                
                if !entities_to_delete.is_empty() {
                    let count = entities_to_delete.len();
                    
                    // Record undo action (batch if multiple)
                    {
                        let mut undo_stack = world_cell.get_resource_mut::<crate::undo::UndoStack>().unwrap();
                        if undo_actions.len() == 1 {
                            undo_stack.push(undo_actions.into_iter().next().unwrap());
                        } else if !undo_actions.is_empty() {
                            undo_stack.push(crate::undo::Action::Batch { actions: undo_actions });
                        }
                    }
                    
                    // Despawn entities
                    let world_mut = world_cell.world_mut();
                    for entity in entities_to_delete {
                        world_mut.despawn(entity);
                    }
                    
                    // Clear selection
                    selection_manager.0.write().clear();
                    
                    let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                    notifications.success(format!("Deleted {} entity(ies)", count));
                }
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entities selected to delete");
            }
        }
    }
    
    // Select All (Ctrl+A or from menu)
    if check_key_or_menu(Action::SelectAll) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let world_mut = world_cell.world_mut();
            
            // Query all entities with Instance component (all selectable classes)
            let mut query = world_mut.query::<(Entity, &Instance, Option<&BasePart>)>();
            let mut selected_count = 0;
            
            // Collect all entity IDs
            let mut entity_ids: Vec<String> = Vec::new();
            for (entity, instance, base_part_opt) in query.iter(world_mut) {
                // Skip locked instances (only if they have BasePart)
                if let Some(base_part) = base_part_opt {
                    if base_part.locked {
                        continue;
                    }
                }
                
                // Skip abstract/service classes that shouldn't be selected
                match instance.class_name {
                    crate::classes::ClassName::Atmosphere | 
                    crate::classes::ClassName::Sun | 
                    crate::classes::ClassName::Moon | 
                    crate::classes::ClassName::Sky |
                    crate::classes::ClassName::Instance |
                    crate::classes::ClassName::PVInstance |
                    crate::classes::ClassName::BasePart => continue,
                    _ => {}
                }
                
                let entity_id = format!("{}v{}", entity.index(), entity.generation());
                entity_ids.push(entity_id);
                selected_count += 1;
            }
            
            // Select all entities
            if selected_count > 0 {
                let mut sm = selection_manager.0.write();
                sm.clear();
                for entity_id in entity_ids {
                    sm.add_to_selection(entity_id);
                }
                drop(sm);
                
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.success(format!("Selected {} entities", selected_count));
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entities to select");
            }
        }
    }
    
    // Focus camera on selection (F or from menu)
    if check_key_or_menu(Action::FocusSelection) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            
            if !selected.is_empty() {
                // Calculate bounding box of ALL selected entities
                let world = world_cell.world();
                let mut min_bounds = Vec3::splat(f32::INFINITY);
                let mut max_bounds = Vec3::splat(f32::NEG_INFINITY);
                let mut found_any = false;
                
                for part_id in &selected {
                    if let Some(entity) = find_entity_by_part_id(world, part_id) {
                        if let Ok(entity_ref) = world.get_entity(entity) {
                            if let Some(bp) = entity_ref.get::<BasePart>() {
                                let pos = bp.cframe.translation;
                                let half_size = bp.size * 0.5;
                                
                                // Expand bounding box
                                min_bounds = min_bounds.min(pos - half_size);
                                max_bounds = max_bounds.max(pos + half_size);
                                found_any = true;
                            }
                        }
                    }
                }
                
                if found_any {
                    // Calculate center and size of bounding box
                    let center = (min_bounds + max_bounds) * 0.5;
                    let bbox_size = max_bounds - min_bounds;
                    // Use the largest dimension for framing
                    let max_extent = bbox_size.x.max(bbox_size.y).max(bbox_size.z);
                    
                    // Update camera pivot and distance (keeps rotation unchanged!)
                    let world_mut = world_cell.world_mut();
                    let mut camera_query = world_mut.query::<&mut crate::camera_controller::EustressCamera>();
                    
                    if let Some(mut camera) = camera_query.iter_mut(world_mut).next() {
                        camera.pivot = center;
                        // Calculate distance to fit object in viewport with padding
                        let fov_factor = 1.0 / 0.577; // ~1.73 for 60° FOV
                        let padding_factor = 2.5; // Zoom out so object is comfortably framed
                        camera.distance = (max_extent * 0.5 * fov_factor * padding_factor).max(5.0);
                        
                        let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                        if selected.len() == 1 {
                            notifications.info("Focused on selection");
                        } else {
                            notifications.info(format!("Focused on {} selected entities", selected.len()));
                        }
                    } else {
                        println!("❌ FOCUS: No camera found");
                    }
                } else {
                    println!("⚠️ FOCUS: No BasePart components found in selection");
                }
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entities selected to focus on");
            }
        }
    }
    
    // Rotate Y 90° (Ctrl+R) - rotates around selection center
    if check_key(Action::RotateY90) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            
            if !selected.is_empty() {
                let world = world_cell.world();
                
                // First pass: collect entities and calculate center
                let mut entities_and_positions: Vec<(Entity, Vec3)> = Vec::new();
                let mut center = Vec3::ZERO;
                
                for part_id in &selected {
                    if let Some(entity) = find_entity_by_part_id(world, part_id) {
                        if let Ok(entity_ref) = world.get_entity(entity) {
                            if let Some(basepart) = entity_ref.get::<BasePart>() {
                                let pos = basepart.cframe.translation;
                                entities_and_positions.push((entity, pos));
                                center += pos;
                            }
                        }
                    }
                }
                
                if !entities_and_positions.is_empty() {
                    center /= entities_and_positions.len() as f32;
                    let rotation_quat = Quat::from_rotation_y(90.0_f32.to_radians());
                    
                    // Second pass: apply rotation around center
                    let world_mut = world_cell.world_mut();
                    let mut rotated_count = 0;
                    
                    for (entity, old_pos) in &entities_and_positions {
                        if let Ok(mut entity_mut) = world_mut.get_entity_mut(*entity) {
                            // Calculate new position and rotation
                            let (new_translation, new_rotation) = {
                                if let Some(basepart) = entity_mut.get::<BasePart>() {
                                    let offset = *old_pos - center;
                                    let rotated_offset = rotation_quat * offset;
                                    let new_trans = center + rotated_offset;
                                    let new_rot = rotation_quat * basepart.cframe.rotation;
                                    (new_trans, new_rot)
                                } else {
                                    continue;
                                }
                            };
                            
                            // Apply to BasePart
                            if let Some(mut basepart) = entity_mut.get_mut::<BasePart>() {
                                basepart.cframe.translation = new_translation;
                                basepart.cframe.rotation = new_rotation;
                                rotated_count += 1;
                            }
                            
                            // Update Transform to match
                            if let Some(mut transform) = entity_mut.get_mut::<Transform>() {
                                transform.translation = new_translation;
                                transform.rotation = new_rotation;
                            }
                        }
                    }
                    
                    if rotated_count > 0 {
                        let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                        notifications.success(format!("Rotated {} part(s) 90° around selection center (Y axis)", rotated_count));
                    }
                }
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entity selected to rotate");
            }
        }
    }
    
    // Tilt Z 90° (Ctrl+T) - tilts around selection center
    if check_key(Action::TiltZ90) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            
            if !selected.is_empty() {
                let world = world_cell.world();
                
                // First pass: collect entities and calculate center
                let mut entities_and_positions: Vec<(Entity, Vec3)> = Vec::new();
                let mut center = Vec3::ZERO;
                
                for part_id in &selected {
                    if let Some(entity) = find_entity_by_part_id(world, part_id) {
                        if let Ok(entity_ref) = world.get_entity(entity) {
                            if let Some(basepart) = entity_ref.get::<BasePart>() {
                                let pos = basepart.cframe.translation;
                                entities_and_positions.push((entity, pos));
                                center += pos;
                            }
                        }
                    }
                }
                
                if !entities_and_positions.is_empty() {
                    center /= entities_and_positions.len() as f32;
                    let rotation_quat = Quat::from_rotation_z(90.0_f32.to_radians());
                    
                    // Second pass: apply rotation around center
                    let world_mut = world_cell.world_mut();
                    let mut tilted_count = 0;
                    
                    for (entity, old_pos) in &entities_and_positions {
                        if let Ok(mut entity_mut) = world_mut.get_entity_mut(*entity) {
                            // Calculate new position and rotation
                            let (new_translation, new_rotation) = {
                                if let Some(basepart) = entity_mut.get::<BasePart>() {
                                    let offset = *old_pos - center;
                                    let rotated_offset = rotation_quat * offset;
                                    let new_trans = center + rotated_offset;
                                    let new_rot = rotation_quat * basepart.cframe.rotation;
                                    (new_trans, new_rot)
                                } else {
                                    continue;
                                }
                            };
                            
                            // Apply to BasePart
                            if let Some(mut basepart) = entity_mut.get_mut::<BasePart>() {
                                basepart.cframe.translation = new_translation;
                                basepart.cframe.rotation = new_rotation;
                                tilted_count += 1;
                            }
                            
                            // Update Transform to match
                            if let Some(mut transform) = entity_mut.get_mut::<Transform>() {
                                transform.translation = new_translation;
                                transform.rotation = new_rotation;
                            }
                        }
                    }
                    
                    if tilted_count > 0 {
                        let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                        notifications.success(format!("Tilted {} part(s) 90° around selection center (Z axis)", tilted_count));
                    }
                }
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entity selected to tilt");
            }
        }
    }
    
    // Snap modes (1, 2, 3 keys) - Space Grade Ready physics-based grid
    // Based on SI standard gravity: 9.80665 m/s²
    // Grid 1: 9.80665 / 5 = 1.96133m (primary grid unit)
    // Grid 2: 1.96133 / 5 = 0.392266m (fine detail)
    // Grid 3: Off (free movement)
    if check_key(Action::SnapMode1) {
        unsafe {
            let mut editor_settings = world_cell.get_resource_mut::<crate::editor_settings::EditorSettings>().unwrap();
            editor_settings.snap_enabled = true;
            editor_settings.snap_size = 1.96133; // 9.80665 / 5 = Space Grade grid unit
            
            let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
            notifications.info("Snap: 1.96133m grid (Space Grade)");
        }
    }
    if check_key(Action::SnapMode2) {
        unsafe {
            let mut editor_settings = world_cell.get_resource_mut::<crate::editor_settings::EditorSettings>().unwrap();
            editor_settings.snap_enabled = true;
            editor_settings.snap_size = 0.196133; // 1.96133 / 10 = fine detail
            
            let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
            notifications.info("Snap: 0.196133m grid (fine, 1/10)");
        }
    }
    if check_key(Action::SnapModeOff) {
        unsafe {
            let mut editor_settings = world_cell.get_resource_mut::<crate::editor_settings::EditorSettings>().unwrap();
            editor_settings.snap_enabled = false;
            
            let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
            notifications.info("Snap: Off (free movement)");
        }
    }
    
    // Window toggles
    if check_key(Action::ToggleCommandBar) {
        unsafe {
            let mut cmd_state = world_cell.get_resource_mut::<CommandBarState>().unwrap();
            cmd_state.show = !cmd_state.show;
        }
    }
    if check_key(Action::ToggleAssets) {
        unsafe {
            let mut asset_state = world_cell.get_resource_mut::<AssetManagerState>().unwrap();
            asset_state.show = !asset_state.show;
        }
    }
    if check_key(Action::ToggleCollaboration) {
        unsafe {
            let mut collab_state = world_cell.get_resource_mut::<CollaborationState>().unwrap();
            collab_state.show = !collab_state.show;
        }
    }
    
    // Copy selected entities (Ctrl+C or from menu)
    if check_key_or_menu(Action::Copy) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            info!("Copy: {} entities selected", selected.len());
            
            if !selected.is_empty() {
                let mut clipboard_entities = Vec::new();
                let world = world_cell.world();
                
                // Query and clone each selected entity's components
                for part_id in &selected {
                    if let Some(entity) = find_entity_by_part_id(world, part_id) {
                        if let Some(clip_entity) = copy_entity_to_clipboard(world, entity) {
                            info!("  Added to clipboard: {}", clip_entity.name);
                            clipboard_entities.push(clip_entity);
                        }
                    }
                }
                
                if !clipboard_entities.is_empty() {
                    let count = clipboard_entities.len();
                    let mut clipboard = world_cell.get_resource_mut::<crate::clipboard::Clipboard>().unwrap();
                    // Store the original entity IDs so we can check if they're still selected on paste
                    clipboard.copy_with_ids(clipboard_entities, selected.clone());
                    info!("📋 Copied {} entities to clipboard", count);
                    drop(clipboard);
                    
                    let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                    notifications.success(format!("Copied {} entity(ies)", count));
                } else {
                    info!("⚠️ No valid entities to copy");
                }
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entities selected to copy");
            }
        }
    }
    
    // Paste entities (Ctrl+V or from menu)
    // Behavior depends on whether original copied entities are still selected:
    // - If originals ARE selected: paste above the original (stacking behavior)
    // - If originals are NOT selected (deselected): paste at cursor position
    if check_key_or_menu(Action::Paste) {
        unsafe {
            let clipboard = world_cell.get_resource::<crate::clipboard::Clipboard>().unwrap();
            info!("🔍 Paste: Clipboard has {} entities", clipboard.entities.len());
            
            if !clipboard.is_empty() {
                let entities_to_paste = clipboard.paste();
                let copy_center = clipboard.copy_center;
                let paste_count = clipboard.paste_count;
                let copied_entity_ids = clipboard.copied_entity_ids.clone();
                let _ = clipboard; // Release borrow
                
                // Check if the original copied entities are still selected
                let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
                let current_selection = selection_manager.0.read().get_selected();
                let originals_selected = copied_entity_ids.iter().any(|id| current_selection.contains(id));
                drop(current_selection);
                
                let (paste_offset, paste_at_cursor) = if originals_selected {
                    // ORIGINALS SELECTED: Paste directly above the original (flush stacking)
                    // Calculate the actual height of the copied entities for flush placement
                    let mut min_y = f32::MAX;
                    let mut max_y = f32::MIN;
                    
                    for clip_entity in &entities_to_paste {
                        let pos = clip_entity.transform.translation;
                        // Get the size from the clipboard entity data
                        let size = match &clip_entity.data {
                            crate::clipboard::ClipboardEntityData::Part { basepart, .. } => basepart.size,
                            crate::clipboard::ClipboardEntityData::MeshPart { basepart, .. } => basepart.size,
                            _ => Vec3::ONE, // Default for non-parts
                        };
                        let half_height = size.y * 0.5;
                        min_y = min_y.min(pos.y - half_height);
                        max_y = max_y.max(pos.y + half_height);
                    }
                    
                    // Calculate the total height of the copied group
                    let group_height = if max_y > min_y { max_y - min_y } else { 1.0 };
                    
                    // Stack flush: offset by the group height times paste count
                    let y_offset = (paste_count + 1) as f32 * group_height;
                    info!("Paste: Originals selected - stacking flush at Y+{} (group height: {})", y_offset, group_height);
                    (Vec3::new(0.0, y_offset, 0.0), false)
                } else {
                    // ORIGINALS NOT SELECTED: Paste at cursor position on surface
                    let mut paste_position = Vec3::new(0.0, 1.0, 0.0); // Default position
                    
                    // Get snap settings
                    let (snap_enabled, snap_size) = {
                        let world_mut = world_cell.world_mut();
                        if let Some(settings) = world_mut.get_resource::<crate::editor_settings::EditorSettings>() {
                            (settings.snap_enabled, settings.snap_size)
                        } else {
                            (false, 1.0)
                        }
                    };
                    
                    // Calculate group bounding box size for proper surface placement
                    // Include actual part sizes, not just positions
                    let mut group_min = Vec3::splat(f32::MAX);
                    let mut group_max = Vec3::splat(f32::MIN);
                    for clip_entity in &entities_to_paste {
                        let pos = clip_entity.transform.translation;
                        // Get the size from the clipboard entity data
                        let size = match &clip_entity.data {
                            crate::clipboard::ClipboardEntityData::Part { basepart, .. } => basepart.size,
                            crate::clipboard::ClipboardEntityData::MeshPart { basepart, .. } => basepart.size,
                            _ => Vec3::ONE, // Default for non-parts
                        };
                        let half_size = size * 0.5;
                        group_min = group_min.min(pos - half_size);
                        group_max = group_max.max(pos + half_size);
                    }
                    let group_size = (group_max - group_min).max(Vec3::splat(0.5));
                    
                    // Try to get cursor position from window and raycast to surfaces
                    {
                        let world_mut = world_cell.world_mut();
                        
                        // Get cursor position
                        let mut cursor_pos_opt: Option<Vec2> = None;
                        let mut window_query = world_mut.query::<&bevy::window::Window>();
                        for window in window_query.iter(world_mut) {
                            if let Some(pos) = window.cursor_position() {
                                cursor_pos_opt = Some(pos);
                                break;
                            }
                        }
                        
                        if let Some(cursor_pos) = cursor_pos_opt {
                            // Find camera for raycasting
                            let mut camera_query = world_mut.query::<(&Camera, &GlobalTransform)>();
                            let mut ray_opt: Option<bevy::math::Ray3d> = None;
                            for (camera, camera_transform) in camera_query.iter(world_mut) {
                                if let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) {
                                    ray_opt = Some(ray);
                                    break;
                                }
                            }
                            
                            if let Some(ray) = ray_opt {
                                // Parse excluded IDs (originals) for raycasting - use string set for comparison
                                let excluded_ids: std::collections::HashSet<String> = copied_entity_ids.iter().cloned().collect();

                                // First, raycast against ALL parts in the scene using OBB intersection
                                // This works regardless of can_collide setting
                                let mut closest_hit: Option<(Vec3, Vec3, f32)> = None; // (hit_point, normal, distance)
                                
                                let mut parts_query = world_mut.query::<(Entity, &GlobalTransform, Option<&crate::classes::BasePart>)>();
                                for (entity, transform, basepart) in parts_query.iter(world_mut) {
                                    // Skip originals to avoid self-shadowing/camera intersection issues
                                    let entity_id = format!("{}v{}", entity.index(), entity.generation());
                                    if excluded_ids.contains(&entity_id) {
                                        continue;
                                    }

                                    let part_transform = transform.compute_transform();
                                    let part_pos = part_transform.translation;
                                    let part_rot = part_transform.rotation;
                                    let part_size = basepart.map(|bp| bp.size).unwrap_or(part_transform.scale);
                                    
                                    // Skip very small parts (likely not surfaces)
                                    if part_size.length_squared() < 0.01 {
                                        continue;
                                    }
                                    
                                    // Use OBB intersection - works on ALL parts regardless of collision
                                    if let Some(distance) = crate::select_tool::ray_intersects_part_rotated_distance(&ray, part_pos, part_rot, part_size) {
                                        let hit_point = ray.origin + *ray.direction * distance;
                                        
                                        // Calculate surface normal based on which face was hit
                                        let local_hit = part_rot.inverse() * (hit_point - part_pos);
                                        let half_size = part_size * 0.5;
                                        
                                        // Find closest face
                                        let mut best_normal = Vec3::Y;
                                        let mut best_dist = f32::MAX;
                                        let faces = [
                                            (Vec3::X, half_size.x - local_hit.x),
                                            (Vec3::NEG_X, half_size.x + local_hit.x),
                                            (Vec3::Y, half_size.y - local_hit.y),
                                            (Vec3::NEG_Y, half_size.y + local_hit.y),
                                            (Vec3::Z, half_size.z - local_hit.z),
                                            (Vec3::NEG_Z, half_size.z + local_hit.z),
                                        ];
                                        for (normal, dist) in faces {
                                            if dist.abs() < best_dist {
                                                best_dist = dist.abs();
                                                best_normal = normal;
                                            }
                                        }
                                        let world_normal = (part_rot * best_normal).normalize();
                                        
                                        if closest_hit.is_none() || distance < closest_hit.unwrap().2 {
                                            closest_hit = Some((hit_point, world_normal, distance));
                                        }
                                    }
                                }
                                
                                if let Some((hit_point, normal, _)) = closest_hit {
                                    // Place object ON the surface using the normal
                                    let half_height = group_size.y * 0.5;
                                    paste_position = hit_point + normal * (half_height + 0.01);
                                    info!("Paste: Surface hit at {:?}, normal {:?}", hit_point, normal);
                                } else {
                                    // Fallback to ground plane (Y=0)
                                    if let Some(ground_hit) = ray_plane_intersection_simple(&ray, Vec3::ZERO, Vec3::Y) {
                                        let half_height = group_size.y * 0.5;
                                        paste_position = ground_hit + Vec3::new(0.0, half_height + 0.01, 0.0);
                                        info!("Paste: Ground plane at {:?}", paste_position);
                                    } else {
                                        // Final fallback: place at a fixed distance along the ray
                                        let half_height = group_size.y * 0.5;
                                        paste_position = ray.origin + *ray.direction * 20.0 + Vec3::new(0.0, half_height, 0.0);
                                        info!("Paste: Ray fallback at {:?}", paste_position);
                                    }
                                }
                            }
                        } else {
                            // No cursor position - use camera forward direction
                            let world_mut = world_cell.world_mut();
                            let mut camera_query = world_mut.query::<(&Camera, &GlobalTransform)>();
                            for (_camera, camera_transform) in camera_query.iter(world_mut) {
                                let forward = camera_transform.forward();
                                let half_height = group_size.y * 0.5;
                                paste_position = camera_transform.translation() + *forward * 15.0 + Vec3::new(0.0, half_height, 0.0);
                                info!("Paste: Camera forward fallback at {:?}", paste_position);
                                break;
                            }
                        }
                    }
                    
                    // Apply grid snapping if enabled
                    if snap_enabled {
                        paste_position.x = (paste_position.x / snap_size).round() * snap_size;
                        paste_position.y = (paste_position.y / snap_size).round() * snap_size;
                        paste_position.z = (paste_position.z / snap_size).round() * snap_size;
                    }
                    
                    // Calculate offset from copy center to paste position
                    (paste_position - copy_center, true)
                };
                
                let mut pasted_count = 0;
                let mut pasted_entities: Vec<Entity> = Vec::new();
                
                // Paste all entities at the new position
                for clip_entity in &entities_to_paste {
                    let world_mut = world_cell.world_mut();
                    if let Some(pasted) = paste_clipboard_entity(world_mut, clip_entity, paste_offset, copy_center) {
                        pasted_entities.push(pasted);
                        pasted_count += 1;
                    }
                }
                
                if pasted_count > 0 {
                    // Increment paste counter for next paste
                    let mut clipboard = world_cell.get_resource_mut::<crate::clipboard::Clipboard>().unwrap();
                    clipboard.increment_paste_count();
                    drop(clipboard);
                    
                    // Select all pasted entities
                    let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
                    {
                        let mut selection = selection_manager.0.write();
                        selection.clear();
                        for entity in &pasted_entities {
                            let entity_str = format!("{}v{}", entity.index(), entity.generation());
                            selection.add_to_selection(entity_str);
                        }
                    }
                    
                    let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                    let msg = if paste_at_cursor {
                        format!("Pasted {} entity(ies) at cursor", pasted_count)
                    } else {
                        format!("Pasted {} entity(ies) above original", pasted_count)
                    };
                    notifications.success(msg);
                } else {
                    let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                    notifications.warning("Nothing to paste (invalid clipboard data)");
                }
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("Nothing to paste");
            }
        }
    }
    
    // Group - Create Model entity with children (Ctrl+G or from menu)
    if check_key_or_menu(Action::Group) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            
            if selected.len() >= 2 {
                let world = world_cell.world();
                
                // Calculate center point of all selected entities
                let mut center = Vec3::ZERO;
                let mut count = 0;
                let mut child_entities = Vec::new();
                
                for part_id in &selected {
                    if let Some(entity) = find_entity_by_part_id(world, part_id) {
                        if let Ok(entity_ref) = world.get_entity(entity) {
                            if let Some(bp) = entity_ref.get::<BasePart>() {
                                center += bp.cframe.translation;
                                count += 1;
                                child_entities.push(entity);
                            }
                        }
                    }
                }
                
                if count > 0 {
                    center /= count as f32;
                    
                    // Calculate bounding box size for the Model's BasePart
                    let mut min_bounds = Vec3::splat(f32::INFINITY);
                    let mut max_bounds = Vec3::splat(f32::NEG_INFINITY);
                    let world = world_cell.world();
                    for child in &child_entities {
                        if let Ok(entity_ref) = world.get_entity(*child) {
                            if let Some(bp) = entity_ref.get::<BasePart>() {
                                let pos = bp.cframe.translation;
                                let half_size = bp.size * 0.5;
                                min_bounds = min_bounds.min(pos - half_size);
                                max_bounds = max_bounds.max(pos + half_size);
                            }
                        }
                    }
                    let model_size = max_bounds - min_bounds;
                    
                    // Create Model entity - use origin (0,0,0) as the Model's position
                    // Children keep their WORLD positions unchanged
                    let world_mut = world_cell.world_mut();
                    let model_instance = Instance {
                        name: "Model".to_string(),
                        class_name: crate::classes::ClassName::Model,
                        archivable: true,
                        id: 0,
                    };
                    
                    // Model has no visual representation - just a logical container
                    // Position at origin so children don't need transform adjustment
                    let model_entity = world_mut.spawn((
                        model_instance,
                        Transform::IDENTITY,
                        GlobalTransform::default(),
                        Name::new("Model"),
                    )).id();
                    
                    // Add children to model - their world positions stay the same
                    // because the Model is at origin with identity transform
                    for child in &child_entities {
                        if let Ok(mut entity_mut) = world_mut.get_entity_mut(*child) {
                            entity_mut.insert(ChildOf(model_entity));
                        }
                    }
                    
                    // Select the model using correct ID format
                    let model_id = format!("{}v{}", model_entity.index(), model_entity.generation());
                    let sm = selection_manager.0.write();
                    sm.clear();
                    sm.select(model_id.clone());
                    drop(sm);
                    
                    let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                    notifications.success(format!("Grouped {} entities into Model", child_entities.len()));
                }
            } else if selected.len() == 1 {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("Select at least 2 entities to group");
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entities selected to group");
            }
        }
    }
    
    // Ungroup - Remove Model and unparent children (Ctrl+U or from menu)
    if check_key_or_menu(Action::Ungroup) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            
            if selected.len() == 1 {
                let world = world_cell.world();
                if let Some(entity) = find_entity_by_part_id(world, &selected[0]) {
                    // Check if it's a Model
                    if let Ok(entity_ref) = world.get_entity(entity) {
                        if let Some(instance) = entity_ref.get::<Instance>() {
                            if instance.class_name == crate::classes::ClassName::Model {
                                // Get children
                                let children: Vec<Entity> = entity_ref.get::<Children>()
                                    .map(|c| c.iter().collect())
                                    .unwrap_or_default();
                                
                                // Remove parent from all children
                                let world_mut = world_cell.world_mut();
                                for child in &children {
                                    if let Ok(mut child_mut) = world_mut.get_entity_mut(*child) {
                                        child_mut.remove::<ChildOf>();
                                    }
                                }
                                
                                // Despawn the model
                                world_mut.despawn(entity);
                                
                                // Select the ungrouped children
                                let sm = selection_manager.0.write();
                                sm.clear();
                                for child in &children {
                                    sm.add_to_selection(format!("{:?}", child));
                                }
                                drop(sm);
                                
                                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                                notifications.success(format!("Ungrouped {} entities from Model", children.len()));
                            } else {
                                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                                notifications.warning("Selected entity is not a Model");
                            }
                        }
                    }
                }
            } else if selected.len() > 1 {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("Select only one Model to ungroup");
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No Model selected to ungroup");
            }
        }
    }
    
    // Lock Selection - Set locked=true on selected parts and their children
    if check_key_or_menu(Action::LockSelection) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            
            if !selected.is_empty() {
                let world = world_cell.world();
                
                // Collect all entities to lock (including children of Models/Folders)
                let mut entities_to_lock: Vec<Entity> = Vec::new();
                for part_id in &selected {
                    if let Some(entity) = find_entity_by_part_id(world, part_id) {
                        collect_entity_and_children(world, entity, &mut entities_to_lock);
                    }
                }
                
                // Lock all collected entities
                let world_mut = world_cell.world_mut();
                let mut locked_count = 0;
                for entity in &entities_to_lock {
                    if let Ok(mut entity_mut) = world_mut.get_entity_mut(*entity) {
                        if let Some(mut base_part) = entity_mut.get_mut::<BasePart>() {
                            base_part.locked = true;
                            locked_count += 1;
                        }
                    }
                }
                
                if locked_count > 0 {
                    let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                    notifications.success(format!("🔒 Locked {} part(s)", locked_count));
                }
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entities selected to lock");
            }
        }
    }
    
    // Unlock Selection - Set locked=false on selected parts and their children
    if check_key_or_menu(Action::UnlockSelection) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            
            if !selected.is_empty() {
                let world = world_cell.world();
                
                // Collect all entities to unlock (including children of Models/Folders)
                let mut entities_to_unlock: Vec<Entity> = Vec::new();
                for part_id in &selected {
                    if let Some(entity) = find_entity_by_part_id(world, part_id) {
                        collect_entity_and_children(world, entity, &mut entities_to_unlock);
                    }
                }
                
                // Unlock all collected entities
                let world_mut = world_cell.world_mut();
                let mut unlocked_count = 0;
                for entity in &entities_to_unlock {
                    if let Ok(mut entity_mut) = world_mut.get_entity_mut(*entity) {
                        if let Some(mut base_part) = entity_mut.get_mut::<BasePart>() {
                            base_part.locked = false;
                            unlocked_count += 1;
                        }
                    }
                }
                
                if unlocked_count > 0 {
                    let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                    notifications.success(format!("🔓 Unlocked {} part(s)", unlocked_count));
                }
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entities selected to unlock");
            }
        }
    }
    
    // Toggle Anchor - Toggle anchored property on selected parts and their children
    if check_key_or_menu(Action::ToggleAnchor) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            
            if !selected.is_empty() {
                let world = world_cell.world();
                
                // Collect all entities to toggle (including children of Models/Folders)
                let mut entities_to_toggle: Vec<Entity> = Vec::new();
                for part_id in &selected {
                    if let Some(entity) = find_entity_by_part_id(world, part_id) {
                        collect_entity_and_children(world, entity, &mut entities_to_toggle);
                    }
                }
                
                // Determine target state from first entity with BasePart
                let target_anchored = {
                    let mut target = true; // Default to anchoring if no parts found
                    for entity in &entities_to_toggle {
                        if let Ok(entity_ref) = world.get_entity(*entity) {
                            if let Some(base_part) = entity_ref.get::<BasePart>() {
                                target = !base_part.anchored; // Toggle from first part's state
                                break;
                            }
                        }
                    }
                    target
                };
                
                // Apply target state to all collected entities
                let world_mut = world_cell.world_mut();
                let mut toggled_count = 0;
                for entity in &entities_to_toggle {
                    if let Ok(mut entity_mut) = world_mut.get_entity_mut(*entity) {
                        if let Some(mut base_part) = entity_mut.get_mut::<BasePart>() {
                            base_part.anchored = target_anchored;
                            toggled_count += 1;
                        }
                    }
                }
                
                if toggled_count > 0 {
                    let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                    let state = if target_anchored { "⚓ Anchored" } else { "🔗 Unanchored" };
                    notifications.success(format!("{} {} part(s)", state, toggled_count));
                }
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entities selected to anchor/unanchor");
            }
        }
    }
    
    // CSG Negate - Mark selected part for CSG subtraction
    if check_key_or_menu(Action::CSGNegate) {
        unsafe {
            let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
            notifications.info("CSG Negate: Not yet implemented - requires CSG library integration");
        }
    }
    
    // CSG Union - Combine selected parts into a UnionOperation
    if check_key_or_menu(Action::CSGUnion) {
        unsafe {
            let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
            notifications.info("CSG Union: Not yet implemented - requires CSG library integration");
        }
    }
    
    // CSG Intersect - Create intersection of selected parts
    if check_key_or_menu(Action::CSGIntersect) {
        unsafe {
            let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
            notifications.info("CSG Intersect: Not yet implemented - requires CSG library integration");
        }
    }
    
    // CSG Separate - Split UnionOperation back into parts
    if check_key_or_menu(Action::CSGSeparate) {
        unsafe {
            let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
            notifications.info("CSG Separate: Not yet implemented - requires CSG library integration");
        }
    }
    
    // Duplicate selected entities (Ctrl+D or from menu)
    if check_key_or_menu(Action::Duplicate) {
        unsafe {
            let selection_manager = world_cell.get_resource::<BevySelectionManager>().unwrap();
            let selected = selection_manager.0.read().get_selected();
            println!("🔍 Duplicate: {} entities selected", selected.len());
            
            if !selected.is_empty() {
                // 1. Resolve and Filter (READ PHASE)
                // We want to duplicate ROOTS of the selection, and their entire subtrees.
                // If a child is selected but its parent is also selected, we ignore the child (it's covered by parent).
                // If a child is selected and parent is NOT, it is a root.
                
                let mut nodes_to_duplicate = Vec::new(); // (original_entity, parent_entity, instance, basepart, part, name)
                
                {
                    let world = world_cell.world();
                    
                    // Resolve part_ids to Entities
                    let mut selected_entities = std::collections::HashSet::new();
                    let mut resolved_list = Vec::new();
                    for part_id in &selected {
                        if let Some(entity) = find_entity_by_part_id(world, part_id) {
                            selected_entities.insert(entity);
                            resolved_list.push(entity);
                        }
                    }
                    
                    // Filter to find ROOTS (selected entities whose parents are NOT selected)
                    let mut roots = Vec::new();
                    for &entity in &resolved_list {
                        let mut is_descendant_of_selection = false;
                        if let Ok(entity_ref) = world.get_entity(entity) {
                            if let Some(child_of) = entity_ref.get::<ChildOf>() {
                                let parent_entity = child_of.0;
                                // We only check immediate parent for "root-ness" in this context? 
                                // No, we must check if ANY ancestor is selected.
                                let mut current = parent_entity;
                                loop {
                                    if selected_entities.contains(&current) {
                                        is_descendant_of_selection = true;
                                        break;
                                    }
                                    if let Ok(curr_ref) = world.get_entity(current) {
                                        if let Some(p) = curr_ref.get::<ChildOf>() {
                                            current = p.0;
                                            continue;
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                        
                        if !is_descendant_of_selection {
                            roots.push(entity);
                        }
                    }
                    
                    // Now traverse descendants of ALL roots to gather everything that needs duplication
                    let mut queue = std::collections::VecDeque::new();
                    for root in roots {
                        queue.push_back(root);
                    }
                    
                    while let Some(entity) = queue.pop_front() {
                        if let Ok(entity_ref) = world.get_entity(entity) {
                            let instance = entity_ref.get::<Instance>().cloned();
                            let basepart = entity_ref.get::<BasePart>().cloned();
                            let part = entity_ref.get::<crate::classes::Part>().cloned();
                            let name = entity_ref.get::<Name>()
                                .map(|n| n.as_str().to_string())
                                .unwrap_or_else(|| "Entity".to_string());
                            let parent = entity_ref.get::<ChildOf>().map(|p| p.0);
                            
                            // Queue children
                            if let Some(children) = entity_ref.get::<Children>() {
                                for child in children {
                                    queue.push_back(*child);
                                }
                            }
                            
                            if let Some(inst) = instance {
                                nodes_to_duplicate.push((entity, parent, inst, basepart, part, name));
                            }
                        }
                    }
                }
                
                if !nodes_to_duplicate.is_empty() {
                    println!("📋 duplicating {} entities (recursive)", nodes_to_duplicate.len());
                    
                    // 2. Create Assets (RESOURCE MUT PHASE)
                    let mut spawn_data = Vec::new();
                    {
                        let (mut meshes, mut materials) = unsafe {
                            let meshes = world_cell.get_resource_mut::<Assets<Mesh>>().unwrap();
                            let materials = world_cell.get_resource_mut::<Assets<StandardMaterial>>().unwrap();
                            (meshes, materials)
                        };
                        
                        for (original, parent, instance, basepart_opt, part_opt, name) in nodes_to_duplicate {
                            if let (Some(basepart), Some(part)) = (basepart_opt, part_opt) {
                                // Create mesh
                                let mesh_handle = match part.shape {
                                    crate::classes::PartType::Block => meshes.add(Cuboid::from_size(basepart.size)),
                                    crate::classes::PartType::Ball => meshes.add(Sphere::new(basepart.size.x / 2.0).mesh().ico(5).unwrap()),
                                    crate::classes::PartType::Cylinder => meshes.add(Cylinder::new(basepart.size.x / 2.0, basepart.size.y)),
                                    crate::classes::PartType::Wedge => meshes.add(Cuboid::from_size(basepart.size)),
                                    crate::classes::PartType::CornerWedge => meshes.add(Cuboid::from_size(basepart.size)),
                                    crate::classes::PartType::Cone => meshes.add(Cylinder::new(basepart.size.x / 2.0, basepart.size.y)),
                                };
                                
                                // Create material
                                let (roughness, metallic, reflectance) = basepart.material.pbr_params();
                                let material_handle = materials.add(StandardMaterial {
                                    base_color: basepart.color,
                                    perceptual_roughness: roughness,
                                    metallic,
                                    reflectance,
                                    alpha_mode: if basepart.transparency > 0.0 {
                                        AlphaMode::Blend
                                    } else {
                                        AlphaMode::Opaque
                                    },
                                    ..default()
                                });
                                
                                spawn_data.push((original, parent, instance, basepart, part, name, mesh_handle, material_handle));
                            }
                        }
                    }
                    
                    // 3. Spawn and Hierarchy (WORLD MUT PHASE)
                    let mut old_to_new = std::collections::HashMap::new();
                    let mut new_roots = Vec::new();
                    let world_mut = unsafe { world_cell.world_mut() };
                    
                    // First pass: Spawn all entities
                    for (original, parent, instance, basepart, part, name, mesh_handle, material_handle) in &spawn_data {
                        let new_entity = world_mut.spawn((
                            Mesh3d(mesh_handle.clone()),
                            MeshMaterial3d(material_handle.clone()),
                            basepart.cframe,
                            instance.clone(),
                            basepart.clone(),
                            part.clone(),
                            Name::new(name.clone()),
                            crate::rendering::PartEntity {
                                part_id: String::new(), // Will be set below
                            },
                        )).id();
                        
                        // Update PartEntity ID
                        let entity_id = format!("{}v{}", new_entity.index(), new_entity.generation());
                        if let Ok(mut entity_mut) = world_mut.get_entity_mut(new_entity) {
                            if let Some(mut part_entity) = entity_mut.get_mut::<crate::rendering::PartEntity>() {
                                part_entity.part_id = entity_id.clone();
                            }
                        }
                        
                        old_to_new.insert(*original, new_entity);
                    }
                    
                    // Second pass: Reconstruct hierarchy
                    for (original, parent, _, _, _, _, _, _) in &spawn_data {
                        if let Some(&new_entity) = old_to_new.get(original) {
                            if let Some(original_parent) = parent {
                                // If parent was also duplicated, parent to new copy.
                                // Otherwise, parent to original parent (sibling).
                                let target_parent = if let Some(&new_parent) = old_to_new.get(original_parent) {
                                    new_parent
                                } else {
                                    *original_parent
                                };
                                
                                // Only add to new_roots if it's a sibling of the original selection (not a child of a new copy)
                                if !old_to_new.contains_key(original_parent) {
                                    new_roots.push(format!("{}v{}", new_entity.index(), new_entity.generation()));
                                }
                                
                                if let Ok(mut entity_mut) = world_mut.get_entity_mut(new_entity) {
                                    entity_mut.insert(ChildOf(target_parent));
                                }
                            } else {
                                // No parent originally -> No parent now. It's a root.
                                new_roots.push(format!("{}v{}", new_entity.index(), new_entity.generation()));
                            }
                        }
                    }
                    
                    // Select the duplicated ROOTS
                    let sm = selection_manager.0.write();
                    sm.clear();
                    for id in &new_roots {
                        sm.add_to_selection(id.clone());
                    }
                    drop(sm);
                    
                    let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                    notifications.success(format!("Duplicated {} entities", new_roots.len()));
                }
            } else {
                let mut notifications = world_cell.get_resource_mut::<crate::notifications::NotificationManager>().unwrap();
                notifications.warning("No entities selected to duplicate");
            }
        }
    }
}

/// Setup custom egui style for professional look and feel
/// Also marks EguiReady as true after successful initialization
fn setup_egui_style(mut contexts: EguiContexts, mut egui_ready: ResMut<EguiReady>) {
    let Ok(ctx) = contexts.ctx_mut() else { 
        warn!("setup_egui_style: Failed to get egui context on Startup");
        return; 
    };
    
    // Initialize Material Icons font
    egui_material_icons::initialize(ctx);
    
    // Mark egui as ready - this enables all other egui systems
    egui_ready.0 = true;
    info!("✅ Custom egui theme applied - Modern dark with blue accents + Material Icons");
    
    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::dark();
    
    // Deeper, richer dark theme - use same color to eliminate gaps
    visuals.window_fill = egui::Color32::from_rgb(35, 35, 38);      // Match ribbon color to hide gaps
    visuals.panel_fill = egui::Color32::from_rgb(35, 35, 38);       // Match ribbon color
    visuals.faint_bg_color = egui::Color32::from_rgb(50, 50, 50);   // Subtle button backgrounds
    visuals.extreme_bg_color = egui::Color32::from_rgb(55, 55, 55); // Text edit backgrounds - neutral grey
    
    // Selection colors - BLUE BACKGROUND with WHITE TEXT
    visuals.selection.bg_fill = egui::Color32::from_rgb(0, 122, 204);      // Vibrant blue (#007ACC)
    visuals.selection.stroke.color = egui::Color32::from_rgb(0, 142, 224); // Brighter blue border
    
    // CRITICAL: Force white text everywhere for maximum contrast
    visuals.override_text_color = Some(egui::Color32::WHITE);
    visuals.text_cursor.stroke.color = egui::Color32::WHITE;
    
    // Widget styling
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(50, 50, 50);
    visuals.widgets.noninteractive.weak_bg_fill = egui::Color32::from_rgb(45, 45, 45);
    visuals.widgets.noninteractive.bg_stroke.color = egui::Color32::from_rgb(60, 60, 60);
    
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(55, 55, 55);
    visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(50, 50, 50);
    visuals.widgets.inactive.bg_stroke.color = egui::Color32::from_rgb(70, 70, 70);
    
    // Hover state - steel blue for modern feel
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(70, 130, 180);   // Steel blue
    visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(60, 120, 170);
    visuals.widgets.hovered.bg_stroke.color = egui::Color32::from_rgb(100, 150, 200);
    
    // Active/pressed state - VIBRANT BLUE like selection
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0, 122, 204);     // Vibrant blue!
    visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(0, 112, 194);
    visuals.widgets.active.bg_stroke.color = egui::Color32::from_rgb(0, 142, 224);
    
    // Better text colors - PURE WHITE for selected/active states
    // fg_stroke controls both text and icon colors in egui (including checkbox checkmarks!)
    visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(200, 200, 200);
    visuals.widgets.noninteractive.fg_stroke.width = 1.5; // Need width for checkbox checkmarks
    visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(210, 210, 210);
    visuals.widgets.inactive.fg_stroke.width = 1.5;
    visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;  // Pure white on hover
    visuals.widgets.hovered.fg_stroke.width = 1.5;
    visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;   // Pure white when active!
    visuals.widgets.active.fg_stroke.width = 1.5;
    
    // Hyperlinks remain slightly blue for distinction
    visuals.hyperlink_color = egui::Color32::from_rgb(100, 150, 255);
    
    // Rounded corners for modern look
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
    
    // Remove the default clip rect margin that causes gaps at window edges
    visuals.clip_rect_margin = 0.0;
    
    style.visuals = visuals;
    
    // Better spacing for breathing room
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);  // Reduced vertical spacing
    style.spacing.window_margin = egui::Margin::same(10);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.indent = 20.0;
    style.spacing.menu_margin = egui::Margin::ZERO;
    
    // Larger, more readable fonts
    style.override_font_id = Some(egui::FontId::new(14.0, egui::FontFamily::Proportional));
    
    ctx.set_style(style);
    
    println!("✅ Custom egui theme applied - Modern dark with blue accents");
}

/// Handle menu action events from ribbon (exclusive - only uses World)
/// Menu buttons directly trigger actions instead of simulating keypresses
fn handle_menu_actions_exclusive(world: &mut World) {
    use crate::keybindings::Action;
    
    // Extract menu action events
    let mut menu_events = world.resource_mut::<Messages<MenuActionEvent>>();
    let events: Vec<_> = menu_events.drain().collect();
    drop(menu_events);
    
    if events.is_empty() {
        return;
    }
    
    // Process each menu action directly
    // This avoids keypress simulation which can conflict with camera controls
    for event in events {
        match event.action {
            Action::Copy | Action::Paste | Action::Duplicate | 
            Action::Delete | Action::SelectAll | Action::Group | 
            Action::Ungroup | Action::FocusSelection |
            Action::LockSelection | Action::UnlockSelection | Action::ToggleAnchor |
            Action::CSGNegate | Action::CSGUnion | Action::CSGIntersect | Action::CSGSeparate => {
                // Create a fake KeyState with just this action triggered
                // We'll inject it into the keyboard shortcuts handler's logic
                // by directly checking the action in the next frame
                
                // Store the action in a resource for keyboard_shortcuts_exclusive to pick up
                let mut pending_actions = world.resource_mut::<PendingMenuActions>();
                pending_actions.actions.push(event.action);
            }
            _ => continue, // Other actions not from menu
        }
    }
}

/// Resource to store menu actions that need to be processed
#[derive(Resource, Default)]
struct PendingMenuActions {
    actions: Vec<crate::keybindings::Action>,
}

/// Handle file save/load events (exclusive - only uses World)
fn handle_file_events_exclusive(world: &mut World) {
    use crate::serialization::scene::{save_scene, SceneMetadata};
    use crate::serialization::binary::{save_binary_scene, load_binary_scene_to_world};
    use std::path::Path;
    
    // Helper to detect binary format by extension
    fn is_binary_format(path: &Path) -> bool {
        path.extension()
            .map(|ext| ext == "eustressengine")
            .unwrap_or(false)
    }
    
    // Helper to save scene (auto-detect format)
    fn save_scene_auto(world: &mut World, path: &Path) -> std::result::Result<(), String> {
        if is_binary_format(path) {
            save_binary_scene(world, path).map_err(|e| e.to_string())
        } else {
            save_scene(world, path, None).map_err(|e| e.to_string())
        }
    }
    
    // Helper to load scene (auto-detect format)
    fn load_scene_auto(world: &mut World, path: &Path) -> std::result::Result<usize, String> {
        if is_binary_format(path) {
            // Load binary format - spawns entities directly into world
            load_binary_scene_to_world(world, path).map_err(|e| e.to_string())
        } else {
            // Load JSON/RON format
            crate::serialization::scene::load_scene_from_world(world, path)
                .map_err(|e| e.to_string())
        }
    }
    
    // Extract events - need to consume them
    let mut file_events = world.resource_mut::<Messages<FileEvent>>();
    let events: Vec<_> = file_events.drain().collect();
    drop(file_events);
    
    if events.is_empty() {
        return;
    }
    
    for event in events {
        match event {
            FileEvent::NewScene => {
                // TODO: Clear current scene
                let mut scene_file = world.resource_mut::<SceneFile>();
                scene_file.path = None;
                scene_file.modified = false;
                drop(scene_file);
                
                let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                notifications.success("New scene created");
            }
            FileEvent::SaveScene => {
                let path = {
                    let scene_file = world.resource::<SceneFile>();
                    scene_file.path.clone()
                };
                
                if let Some(path) = path {
                    // We have a known path - save directly without prompting
                    let format_name = if is_binary_format(&path) { "binary" } else { "JSON" };
                    match save_scene_auto(world, &path) {
                        Ok(_) => {
                            let mut scene_file = world.resource_mut::<SceneFile>();
                            scene_file.modified = false;
                            // Update name from path in case it changed
                            scene_file.name = path.file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| "Untitled".to_string());
                            drop(scene_file);
                            
                            let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                            notifications.success(format!("Saved ({format_name})"));
                            
                            // Log to output with clickable path
                            let mut output = world.resource_mut::<OutputConsole>();
                            output.info_with_path(
                                format!("💾 Scene saved ({format_name})"),
                                path.display().to_string()
                            );
                        }
                        Err(e) => {
                            let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                            notifications.error(format!("Save failed: {}", e));
                            
                            let mut output = world.resource_mut::<OutputConsole>();
                            output.error(format!("❌ Save failed: {}", e));
                        }
                    }
                } else {
                    // No path known - trigger Save As dialog
                    if let Some(path) = pick_save_file() {
                        let format_name = if is_binary_format(&path) { "binary" } else { "JSON" };
                        match save_scene_auto(world, &path) {
                            Ok(_) => {
                                let mut scene_file = world.resource_mut::<SceneFile>();
                                scene_file.path = Some(path.clone());
                                scene_file.modified = false;
                                // Update name from the new path
                                scene_file.name = path.file_stem()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_else(|| "Untitled".to_string());
                                drop(scene_file);
                                
                                let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                                notifications.success(format!("Saved ({format_name})"));
                                
                                // Log to output with clickable path
                                let mut output = world.resource_mut::<OutputConsole>();
                                output.info_with_path(
                                    format!("💾 Scene saved ({format_name})"),
                                    path.display().to_string()
                                );
                            }
                            Err(e) => {
                                let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                                notifications.error(format!("Save failed: {}", e));
                                
                                let mut output = world.resource_mut::<OutputConsole>();
                                output.error(format!("❌ Save failed: {}", e));
                            }
                        }
                    }
                }
            }
            FileEvent::SaveSceneAs => {
                // Always prompt for a new file location
                if let Some(path) = pick_save_file() {
                    let format_name = if is_binary_format(&path) { "binary" } else { "JSON" };
                    match save_scene_auto(world, &path) {
                        Ok(_) => {
                            let mut scene_file = world.resource_mut::<SceneFile>();
                            scene_file.path = Some(path.clone());
                            scene_file.modified = false;
                            // Update name from the new path - this becomes the new file name
                            scene_file.name = path.file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| "Untitled".to_string());
                            drop(scene_file);
                            
                            let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                            notifications.success(format!("Saved ({format_name})"));
                            
                            // Log to output with clickable path
                            let mut output = world.resource_mut::<OutputConsole>();
                            output.info_with_path(
                                format!("💾 Scene saved as ({format_name})"),
                                path.display().to_string()
                            );
                        }
                        Err(e) => {
                            let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                            notifications.error(format!("Save failed: {}", e));
                            
                            let mut output = world.resource_mut::<OutputConsole>();
                            output.error(format!("❌ Save failed: {}", e));
                        }
                    }
                }
            }
            FileEvent::OpenScene => {
                if let Some(path) = pick_open_file() {
                    let format_name = if is_binary_format(&path) { "binary" } else { "JSON" };
                    // Load scene using exclusive World access
                    match load_scene_auto(world, &path) {
                        Ok(entity_count) => {
                            let mut scene_file = world.resource_mut::<SceneFile>();
                            scene_file.path = Some(path.clone());
                            scene_file.modified = false;
                            // Update name from the opened file
                            scene_file.name = path.file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| "Untitled".to_string());
                            drop(scene_file);
                            
                            let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                            notifications.success(format!("Loaded ({format_name}): {} ({} entities)", path.display(), entity_count));
                        }
                        Err(e) => {
                            let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                            notifications.error(format!("Load failed: {}", e));
                        }
                    }
                } else {
                    // User cancelled file picker - just show cancelled message
                    let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                    notifications.warning("Scene loading cancelled");
                }
            }
            FileEvent::OpenRecent(path) => {
                let format_name = if is_binary_format(&path) { "binary" } else { "JSON" };
                // Load scene from recent files list
                match load_scene_auto(world, &path) {
                    Ok(entity_count) => {
                        let mut scene_file = world.resource_mut::<SceneFile>();
                        scene_file.path = Some(path.clone());
                        scene_file.modified = false;
                        scene_file.name = path.file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Untitled".to_string());
                        drop(scene_file);
                        
                        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                        notifications.success(format!("Loaded ({format_name}): {} ({} entities)", path.display(), entity_count));
                    }
                    Err(e) => {
                        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
                        notifications.error(format!("Load failed: {}", e));
                    }
                }
            }
            FileEvent::Publish => {
                // Show publish dialog for existing experience or prompt for new
                let mut state = world.resource_mut::<StudioState>();
                state.show_publish_dialog = true;
                state.publish_as_new = false;
            }
            FileEvent::PublishAs => {
                // Always show publish dialog as new experience
                let mut state = world.resource_mut::<StudioState>();
                state.show_publish_dialog = true;
                state.publish_as_new = true;
            }
        }
    }
}

// ============================================================================
// Clipboard Helper Functions
// ============================================================================

/// Copy an entity to clipboard format, including all its components
fn copy_entity_to_clipboard(world: &World, entity: Entity) -> Option<crate::clipboard::ClipboardEntity> {
    use crate::clipboard::{ClipboardEntity, ClipboardEntityData};
    
    let entity_ref = world.get_entity(entity).ok()?;
    
    // Get required components
    let instance = entity_ref.get::<Instance>()?.clone();
    let name = entity_ref.get::<Name>()
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| instance.name.clone());
    let transform = entity_ref.get::<Transform>().cloned().unwrap_or_default();
    
    // Determine entity type and create appropriate data
    let data = if let Some(part) = entity_ref.get::<crate::classes::Part>() {
        if let Some(basepart) = entity_ref.get::<BasePart>() {
            ClipboardEntityData::Part {
                basepart: basepart.clone(),
                part: part.clone(),
            }
        } else {
            ClipboardEntityData::Generic
        }
    } else if let Some(meshpart) = entity_ref.get::<crate::classes::MeshPart>() {
        if let Some(basepart) = entity_ref.get::<BasePart>() {
            ClipboardEntityData::MeshPart {
                basepart: basepart.clone(),
                meshpart: meshpart.clone(),
            }
        } else {
            ClipboardEntityData::Generic
        }
    } else if let Some(model) = entity_ref.get::<crate::classes::Model>() {
        ClipboardEntityData::Model {
            model: model.clone(),
        }
    } else if entity_ref.get::<crate::classes::Folder>().is_some() {
        ClipboardEntityData::Folder
    } else if let Some(light) = entity_ref.get::<crate::classes::EustressPointLight>() {
        ClipboardEntityData::PointLight {
            light: light.clone(),
        }
    } else if let Some(light) = entity_ref.get::<crate::classes::EustressSpotLight>() {
        ClipboardEntityData::SpotLight {
            light: light.clone(),
        }
    } else if let Some(light) = entity_ref.get::<crate::classes::EustressDirectionalLight>() {
        ClipboardEntityData::DirectionalLight {
            light: light.clone(),
        }
    } else if let Some(light) = entity_ref.get::<crate::classes::SurfaceLight>() {
        ClipboardEntityData::SurfaceLight {
            light: light.clone(),
        }
    } else if let Some(sound) = entity_ref.get::<crate::classes::Sound>() {
        ClipboardEntityData::Sound {
            sound: sound.clone(),
        }
    } else if let Some(attachment) = entity_ref.get::<crate::classes::Attachment>() {
        ClipboardEntityData::Attachment {
            attachment: attachment.clone(),
        }
    } else if let Some(emitter) = entity_ref.get::<crate::classes::ParticleEmitter>() {
        ClipboardEntityData::ParticleEmitter {
            emitter: emitter.clone(),
        }
    } else if let Some(beam) = entity_ref.get::<crate::classes::Beam>() {
        ClipboardEntityData::Beam {
            beam: beam.clone(),
        }
    } else if let Some(decal) = entity_ref.get::<crate::classes::Decal>() {
        ClipboardEntityData::Decal {
            decal: decal.clone(),
        }
    } else if let Some(mesh) = entity_ref.get::<crate::classes::SpecialMesh>() {
        ClipboardEntityData::SpecialMesh {
            mesh: mesh.clone(),
        }
    } else if let Some(gui) = entity_ref.get::<crate::classes::BillboardGui>() {
        ClipboardEntityData::BillboardGui {
            gui: gui.clone(),
        }
    } else if let Some(label) = entity_ref.get::<crate::classes::TextLabel>() {
        ClipboardEntityData::TextLabel {
            label: label.clone(),
        }
    } else {
        // Generic entity with just transform
        ClipboardEntityData::Generic
    };
    
    let mut clip_entity = ClipboardEntity::new(instance, name, transform, data);
    clip_entity.original_entity = Some(entity);
    
    // Copy children for containers (Models/Folders)
    if clip_entity.is_container() {
        if let Some(children) = entity_ref.get::<Children>() {
            for child in children.iter() {
                if let Some(child_clip) = copy_entity_to_clipboard(world, child) {
                    clip_entity.add_child(child_clip);
                }
            }
        }
    }
    
    Some(clip_entity)
}

/// Paste a clipboard entity into the world
fn paste_clipboard_entity(
    world: &mut World,
    clip_entity: &crate::clipboard::ClipboardEntity,
    offset: Vec3,
    _copy_center: Vec3,
) -> Option<Entity> {
    use crate::clipboard::ClipboardEntityData;
    
    // Calculate new position with offset
    let new_transform = Transform {
        translation: clip_entity.transform.translation + offset,
        rotation: clip_entity.transform.rotation,
        scale: clip_entity.transform.scale,
    };
    
    // Create new instance with unique ID
    let mut new_instance = clip_entity.instance.clone();
    new_instance.id = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() % u32::MAX as u128) as u32;
    
    let entity = match &clip_entity.data {
        ClipboardEntityData::Part { basepart, part } => {
            // Update basepart cframe with new position
            let mut new_basepart = basepart.clone();
            new_basepart.cframe = new_transform;
            
            // Spawn part with mesh and collider
            let size = new_basepart.size;
            let mesh = world.resource_scope(|_world, mut meshes: Mut<Assets<Mesh>>| {
                match part.shape {
                    crate::classes::PartType::Block => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                    crate::classes::PartType::Ball => meshes.add(bevy::math::primitives::Sphere::new(size.x / 2.0)),
                    crate::classes::PartType::Cylinder => meshes.add(bevy::math::primitives::Cylinder::new(size.x / 2.0, size.y)),
                    _ => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                }
            });
            
            let (roughness, metallic, reflectance) = new_basepart.material.pbr_params();
            let material = world.resource_scope(|_world, mut materials: Mut<Assets<StandardMaterial>>| {
                materials.add(StandardMaterial {
                    base_color: new_basepart.color,
                    perceptual_roughness: roughness,
                    metallic,
                    reflectance,
                    ..default()
                })
            });
            
            let collider = match part.shape {
                crate::classes::PartType::Ball => avian3d::prelude::Collider::sphere(size.x / 2.0),
                crate::classes::PartType::Cylinder => avian3d::prelude::Collider::cylinder(size.x / 2.0, size.y),
                _ => avian3d::prelude::Collider::cuboid(size.x, size.y, size.z),
            };
            
            let entity = world.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                new_transform,
                new_instance,
                new_basepart,
                part.clone(),
                collider,
                avian3d::prelude::RigidBody::Static,
                Name::new(clip_entity.name.clone()),
                crate::rendering::PartEntity { part_id: String::new() },
            )).id();
            Some(entity)
        }
        ClipboardEntityData::MeshPart { basepart, meshpart } => {
            let mut new_basepart = basepart.clone();
            new_basepart.cframe = new_transform;
            
            let size = new_basepart.size;
            let collider = avian3d::prelude::Collider::cuboid(size.x, size.y, size.z);
            
            let entity = world.spawn((
                new_instance,
                new_basepart,
                meshpart.clone(),
                new_transform,
                GlobalTransform::default(),
                collider,
                avian3d::prelude::RigidBody::Static,
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::Model { model } => {
            let entity = world.spawn((
                new_instance,
                model.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            
            // Paste children
            for child_clip in &clip_entity.children {
                if let Some(child_entity) = paste_clipboard_entity(world, child_clip, offset, _copy_center) {
                    if let Ok(mut child_mut) = world.get_entity_mut(child_entity) {
                        child_mut.insert(ChildOf(entity));
                    }
                }
            }
            
            Some(entity)
        }
        ClipboardEntityData::Folder => {
            let entity = world.spawn((
                new_instance,
                crate::classes::Folder::default(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            
            // Paste children
            for child_clip in &clip_entity.children {
                if let Some(child_entity) = paste_clipboard_entity(world, child_clip, offset, _copy_center) {
                    if let Ok(mut child_mut) = world.get_entity_mut(child_entity) {
                        child_mut.insert(ChildOf(entity));
                    }
                }
            }
            
            Some(entity)
        }
        ClipboardEntityData::PointLight { light } => {
            let entity = world.spawn((
                new_instance,
                light.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::SpotLight { light } => {
            let entity = world.spawn((
                new_instance,
                light.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::DirectionalLight { light } => {
            let entity = world.spawn((
                new_instance,
                light.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::SurfaceLight { light } => {
            let entity = world.spawn((
                new_instance,
                light.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::Sound { sound } => {
            let entity = world.spawn((
                new_instance,
                sound.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::Attachment { attachment } => {
            let entity = world.spawn((
                new_instance,
                attachment.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::ParticleEmitter { emitter } => {
            let entity = world.spawn((
                new_instance,
                emitter.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::Beam { beam } => {
            let entity = world.spawn((
                new_instance,
                beam.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::Decal { decal } => {
            let entity = world.spawn((
                new_instance,
                decal.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::SpecialMesh { mesh } => {
            let entity = world.spawn((
                new_instance,
                mesh.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::BillboardGui { gui } => {
            let entity = world.spawn((
                new_instance,
                gui.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::TextLabel { label } => {
            let entity = world.spawn((
                new_instance,
                label.clone(),
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
        ClipboardEntityData::Generic => {
            let entity = world.spawn((
                new_instance,
                new_transform,
                GlobalTransform::default(),
                Name::new(clip_entity.name.clone()),
            )).id();
            Some(entity)
        }
    };
    
    entity
}

/// Simple ray-plane intersection for paste positioning
fn ray_plane_intersection_simple(ray: &bevy::math::Ray3d, point_on_plane: Vec3, normal: Vec3) -> Option<Vec3> {
    let denom = ray.direction.dot(normal);
    
    if denom.abs() < 0.0001 {
        return None; // Ray is parallel to plane
    }
    
    let t = (point_on_plane - ray.origin).dot(normal) / denom;
    
    if t < 0.0 {
        return None; // Intersection is behind ray origin
    }
    
    Some(ray.origin + *ray.direction * t)
}

// ============================================================================
// Data Menu Windows System
// ============================================================================

/// System to render Data menu windows (Global Sources, Domains, Global Variables)
fn data_windows_system(
    mut contexts: EguiContexts,
    mut state: ResMut<StudioState>,
    registry: Option<ResMut<eustress_common::parameters::GlobalParametersRegistry>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    
    // Check for temp values set by buttons in Parameters Editor panel
    if ctx.data(|d| d.get_temp::<bool>(egui::Id::new("open_global_sources_window"))).unwrap_or(false) {
        state.show_global_sources_window = true;
        ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::new("open_global_sources_window")));
    }
    if ctx.data(|d| d.get_temp::<bool>(egui::Id::new("open_domains_window"))).unwrap_or(false) {
        state.show_domains_window = true;
        ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::new("open_domains_window")));
    }
    
    // Early return if registry doesn't exist
    let Some(mut registry) = registry else {
        // Close windows if registry is missing
        if state.show_global_sources_window || state.show_domains_window || state.show_global_variables_window {
            state.show_global_sources_window = false;
            state.show_domains_window = false;
            state.show_global_variables_window = false;
        }
        return;
    };
    
    // Global Sources Window
    if state.show_global_sources_window {
        let mut open = true;
        egui::Window::new("🌐 Global Data Sources")
            .open(&mut open)
            .default_width(500.0)
            .default_height(400.0)
            .resizable(true)
            .show(ctx, |ui| {
                render_global_sources_window(ui, ctx, &mut registry, &mut state.quick_add_source_type);
            });
        if !open {
            state.show_global_sources_window = false;
            state.quick_add_source_type = None;
        }
    }
    
    // Domains Window
    if state.show_domains_window {
        let mut open = true;
        egui::Window::new("📂 Domain Configurations")
            .open(&mut open)
            .default_width(500.0)
            .default_height(400.0)
            .resizable(true)
            .show(ctx, |ui| {
                render_domains_window(ui, ctx, &mut registry);
            });
        if !open {
            state.show_domains_window = false;
        }
    }
    
    // Global Variables Window
    if state.show_global_variables_window {
        let mut open = true;
        egui::Window::new("🔧 Global Variables")
            .open(&mut open)
            .default_width(450.0)
            .default_height(350.0)
            .resizable(true)
            .show(ctx, |ui| {
                render_global_variables_window(ui, ctx, &mut registry);
            });
        if !open {
            state.show_global_variables_window = false;
        }
    }
    
    // Sync Domain to Object Type Modal
    if state.show_sync_domain_modal {
        let mut open = true;
        egui::Window::new("🔄 Sync Domain to Object Type")
            .open(&mut open)
            .default_width(550.0)
            .default_height(600.0)
            .resizable(true)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                render_sync_domain_modal(ui, &mut state.sync_domain_config, &registry);
            });
        if !open {
            state.show_sync_domain_modal = false;
            state.sync_domain_config.reset();
        }
    }
}

/// Render the Global Sources management window
fn render_global_sources_window(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    registry: &mut eustress_common::parameters::GlobalParametersRegistry,
    quick_add_type: &mut Option<String>,
) {
    use eustress_common::parameters::{DataSourceType, AuthType, GlobalDataSource};
    
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
    
    ui.label("Define connection endpoints shared across all entities.");
    ui.add_space(8.0);
    
    // Check if we should auto-open the add form
    let should_show_add = quick_add_type.is_some() || ctx.data(|d| d.get_temp(egui::Id::new("show_add_source"))).unwrap_or(false);
    
    // Add new source button
    ui.horizontal(|ui| {
        if ui.button("➕ Add New Source").clicked() || quick_add_type.is_some() {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("show_add_source"), true));
        }
    });
    
    // Add source form
    if should_show_add {
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(egui::RichText::new("New Data Source").strong());
            ui.add_space(4.0);
            
            // Source ID
            let mut source_id: String = ctx.data(|d| d.get_temp(egui::Id::new("new_source_id"))).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label("Source ID:");
                ui.add_space(20.0);
                if ui.add(egui::TextEdit::singleline(&mut source_id).hint_text("my_api")).changed() {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_source_id"), source_id.clone()));
                }
            });
            
            // Display Name
            let mut source_name: String = ctx.data(|d| d.get_temp(egui::Id::new("new_source_name"))).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label("Display Name:");
                if ui.add(egui::TextEdit::singleline(&mut source_name).hint_text("My API Server")).changed() {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_source_name"), source_name.clone()));
                }
            });
            
            // Source Type - auto-select from quick_add_type
            let default_type = quick_add_type.as_ref()
                .and_then(|t| match t.as_str() {
                    "REST" => Some(DataSourceType::REST),
                    "GraphQL" => Some(DataSourceType::GraphQL),
                    "Firebase" => Some(DataSourceType::Firebase),
                    "Supabase" => Some(DataSourceType::Supabase),
                    "PostgreSQL" => Some(DataSourceType::PostgreSQL),
                    "CSV" => Some(DataSourceType::CSV),
                    "S3" => Some(DataSourceType::S3),
                    "AzureBlob" => Some(DataSourceType::AzureBlob),
                    "Oracle" => Some(DataSourceType::Oracle),
                    _ => None,
                })
                .unwrap_or(DataSourceType::REST);
            
            let current_type: DataSourceType = ctx.data(|d| d.get_temp(egui::Id::new("new_source_type"))).unwrap_or(default_type);
            
            // Clear quick_add_type after first use
            if quick_add_type.is_some() {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_source_type"), default_type));
                *quick_add_type = None;
            }
            
            ui.horizontal(|ui| {
                ui.label("Type:");
                ui.add_space(48.0);
                egui::ComboBox::from_id_salt("new_source_type_combo")
                    .selected_text(current_type.display_name())
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        // Group by category using the category() method
                        let mut current_category = "";
                        for source_type in DataSourceType::all_variants() {
                            if *source_type == DataSourceType::None {
                                continue;
                            }
                            let cat = source_type.category();
                            if cat != current_category {
                                if !current_category.is_empty() {
                                    ui.separator();
                                }
                                ui.label(egui::RichText::new(cat).small().weak());
                                current_category = cat;
                            }
                            if ui.selectable_label(current_type == *source_type, source_type.display_name()).clicked() {
                                ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_source_type"), *source_type));
                            }
                        }
                    });
            });
            
            // Base Endpoint
            let mut endpoint: String = ctx.data(|d| d.get_temp(egui::Id::new("new_source_endpoint"))).unwrap_or_default();
            let endpoint_hint = match current_type {
                DataSourceType::REST | DataSourceType::GraphQL => "https://api.example.com/v1",
                DataSourceType::Firebase => "https://project-id.firebaseio.com",
                DataSourceType::Supabase => "https://project-id.supabase.co",
                DataSourceType::PostgreSQL => "postgresql://user:pass@host:5432/db",
                DataSourceType::MySQL => "mysql://user:pass@host:3306/db",
                DataSourceType::CSV => "/path/to/data.csv or https://...",
                DataSourceType::FHIR => "https://fhir.server.com/r4",
                DataSourceType::WebSocket => "wss://ws.example.com",
                DataSourceType::MQTT => "mqtt://broker.example.com:1883",
                DataSourceType::S3 => "s3://bucket-name/path",
                DataSourceType::AzureBlob => "https://account.blob.core.windows.net/container",
                DataSourceType::Oracle => "https://objectstorage.region.oraclecloud.com",
                DataSourceType::GCS => "gs://bucket-name/path",
                _ => "https://...",
            };
            ui.horizontal(|ui| {
                ui.label("Endpoint:");
                ui.add_space(24.0);
                if ui.add(egui::TextEdit::singleline(&mut endpoint).desired_width(300.0).hint_text(endpoint_hint)).changed() {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_source_endpoint"), endpoint.clone()));
                }
            });
            
            // Auth Type
            let current_auth: AuthType = ctx.data(|d| d.get_temp(egui::Id::new("new_source_auth"))).unwrap_or(AuthType::None);
            ui.horizontal(|ui| {
                ui.label("Auth:");
                ui.add_space(52.0);
                egui::ComboBox::from_id_salt("new_source_auth_combo")
                    .selected_text(format!("{:?}", current_auth))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for auth in [AuthType::None, AuthType::Bearer, AuthType::APIKey, AuthType::Basic, AuthType::OAuth2] {
                            if ui.selectable_label(current_auth == auth, format!("{:?}", auth)).clicked() {
                                ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_source_auth"), auth));
                            }
                        }
                    });
            });
            
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("✓ Create Source").clicked() {
                    // Create the source
                    if !source_id.is_empty() && !endpoint.is_empty() {
                        let source = GlobalDataSource {
                            id: source_id.clone(),
                            name: if source_name.is_empty() { source_id.clone() } else { source_name },
                            source_type: current_type,
                            base_endpoint: endpoint,
                            auth_type: current_auth,
                            ..Default::default()
                        };
                        registry.register_source(source);
                        
                        // Clear form
                        ctx.data_mut(|d| {
                            d.remove::<String>(egui::Id::new("new_source_id"));
                            d.remove::<String>(egui::Id::new("new_source_name"));
                            d.remove::<String>(egui::Id::new("new_source_endpoint"));
                            d.insert_temp(egui::Id::new("show_add_source"), false);
                        });
                    }
                }
                if ui.button("Cancel").clicked() {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("show_add_source"), false));
                }
            });
        });
    }
    
    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    
    // List existing sources
    ui.label(egui::RichText::new("Configured Sources").strong());
    ui.add_space(4.0);
    
    let sources: Vec<_> = registry.sources.values().cloned().collect();
    if sources.is_empty() {
        ui.weak("No data sources configured yet.");
    } else {
        for source in sources {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    // Icon based on type
                    let icon = match source.source_type {
                        DataSourceType::REST | DataSourceType::GraphQL => "🌐",
                        DataSourceType::Firebase | DataSourceType::Supabase => "☁",
                        DataSourceType::PostgreSQL | DataSourceType::MySQL => "🗄",
                        DataSourceType::CSV | DataSourceType::JSON => "📄",
                        DataSourceType::FHIR => "🏥",
                        DataSourceType::WebSocket | DataSourceType::MQTT => "📡",
                        _ => "📦",
                    };
                    ui.label(egui::RichText::new(icon).size(18.0));
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&source.id).strong());
                        ui.weak(format!("{} - {}", source.source_type.display_name(), source.base_endpoint));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                            registry.sources.remove(&source.id);
                        }
                    });
                });
            });
        }
    }
    }); // End ScrollArea
}

/// Render the Domains configuration window
fn render_domains_window(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    registry: &mut eustress_common::parameters::GlobalParametersRegistry,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
    
    ui.label("Define how entity types connect to global sources.");
    ui.add_space(8.0);
    
    // Add new domain button
    if ui.button("➕ Add Domain").clicked() {
        ctx.data_mut(|d| {
            let show: bool = d.get_temp(egui::Id::new("show_add_domain")).unwrap_or(false);
            d.insert_temp(egui::Id::new("show_add_domain"), !show);
        });
    }
    
    // Add domain form
    let show_add: bool = ctx.data(|d| d.get_temp(egui::Id::new("show_add_domain"))).unwrap_or(false);
    if show_add {
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(egui::RichText::new("New Domain Configuration").strong());
            ui.add_space(4.0);
            
            let mut domain_id: String = ctx.data(|d| d.get_temp(egui::Id::new("new_domain_id"))).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label("Domain ID:");
                if ui.add(egui::TextEdit::singleline(&mut domain_id).hint_text("Patient, Sensor, Product...")).changed() {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_domain_id"), domain_id.clone()));
                }
            });
            
            // Select source
            let sources: Vec<String> = registry.sources.keys().cloned().collect();
            let current_source: String = ctx.data(|d| d.get_temp(egui::Id::new("new_domain_source"))).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label("Source:");
                egui::ComboBox::from_id_salt("new_domain_source_combo")
                    .selected_text(if current_source.is_empty() { "Select..." } else { &current_source })
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for source_id in &sources {
                            if ui.selectable_label(*source_id == current_source, source_id).clicked() {
                                ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_domain_source"), source_id.clone()));
                            }
                        }
                    });
            });
            
            let mut path_template: String = ctx.data(|d| d.get_temp(egui::Id::new("new_domain_path"))).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label("Path Template:");
                if ui.add(egui::TextEdit::singleline(&mut path_template).hint_text("/{resource_id}")).changed() {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_domain_path"), path_template.clone()));
                }
            });
            
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("✓ Create Domain").clicked() {
                    if !domain_id.is_empty() && !current_source.is_empty() {
                        let config = eustress_common::parameters::DomainConfig::new(domain_id.clone(), domain_id.clone())
                            .with_source(current_source)
                            .with_path_template(path_template);
                        registry.register_domain(config);
                        
                        ctx.data_mut(|d| {
                            d.remove::<String>(egui::Id::new("new_domain_id"));
                            d.remove::<String>(egui::Id::new("new_domain_source"));
                            d.remove::<String>(egui::Id::new("new_domain_path"));
                            d.insert_temp(egui::Id::new("show_add_domain"), false);
                        });
                    }
                }
                if ui.button("Cancel").clicked() {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("show_add_domain"), false));
                }
            });
        });
    }
    
    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    
    // List existing domains
    ui.label(egui::RichText::new("Configured Domains").strong());
    ui.add_space(4.0);
    
    let domains: Vec<_> = registry.domains.values().cloned().collect();
    if domains.is_empty() {
        ui.weak("No domains configured yet.");
    } else {
        for domain in domains {
            egui::CollapsingHeader::new(format!("📁 {}", domain.domain))
                .default_open(false)
                .show(ui, |ui| {
                    if let Some(ref source) = domain.global_source_id {
                        ui.horizontal(|ui| {
                            ui.label("Source:");
                            ui.label(egui::RichText::new(source).color(egui::Color32::from_rgb(100, 180, 255)));
                        });
                    }
                    if !domain.resource_path_template.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label("Path:");
                            ui.code(&domain.resource_path_template);
                        });
                    }
                    if ui.small_button("🗑 Delete").clicked() {
                        registry.domains.remove(&domain.domain);
                    }
                });
        }
    }
    }); // End ScrollArea
}

/// Render the Global Variables window
fn render_global_variables_window(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    registry: &mut eustress_common::parameters::GlobalParametersRegistry,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
    
    ui.label("Environment variables for template substitution in URLs and queries.");
    ui.weak("Use ${VAR_NAME} syntax in endpoints and paths.");
    ui.add_space(8.0);
    
    // Add new variable
    ui.horizontal(|ui| {
        let mut new_key: String = ctx.data(|d| d.get_temp(egui::Id::new("new_var_key"))).unwrap_or_default();
        let mut new_value: String = ctx.data(|d| d.get_temp(egui::Id::new("new_var_value"))).unwrap_or_default();
        
        ui.label("Key:");
        if ui.add(egui::TextEdit::singleline(&mut new_key).desired_width(100.0).hint_text("API_KEY")).changed() {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_var_key"), new_key.clone()));
        }
        
        ui.label("Value:");
        if ui.add(egui::TextEdit::singleline(&mut new_value).desired_width(200.0).hint_text("sk-...").password(true)).changed() {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("new_var_value"), new_value.clone()));
        }
        
        if ui.button("➕ Add").clicked() && !new_key.is_empty() {
            registry.set_variable(new_key.clone(), new_value);
            ctx.data_mut(|d| {
                d.remove::<String>(egui::Id::new("new_var_key"));
                d.remove::<String>(egui::Id::new("new_var_value"));
            });
        }
    });
    
    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);
    
    // List existing variables
    ui.label(egui::RichText::new("Configured Variables").strong());
    ui.add_space(4.0);
    
    let variables: Vec<_> = registry.global_variables.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    if variables.is_empty() {
        ui.weak("No variables configured yet.");
        ui.add_space(8.0);
        ui.label("Example usage:");
        ui.code("https://api.example.com?key=${API_KEY}");
    } else {
        for (key, value) in variables {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("${{{}}}", key)).code());
                ui.label("=");
                // Show masked value for sensitive keys
                let is_sensitive = key.to_lowercase().contains("key") 
                    || key.to_lowercase().contains("secret") 
                    || key.to_lowercase().contains("password")
                    || key.to_lowercase().contains("token");
                if is_sensitive {
                    ui.label("••••••••");
                } else {
                    ui.label(&value);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                        registry.global_variables.remove(&key);
                    }
                });
            });
        }
    }
    }); // End ScrollArea
}

/// Render the Sync Domain to Object Type modal
fn render_sync_domain_modal(
    ui: &mut egui::Ui,
    config: &mut SyncDomainModalState,
    registry: &eustress_common::parameters::GlobalParametersRegistry,
) {
    use eustress_common::classes::{SyncTargetClass, SpawnLayout, ColorMapping};
    
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
    
    ui.label("Configure how domain data maps to scene entities.");
    ui.add_space(8.0);
    
    // Domain info
    ui.horizontal(|ui| {
        ui.label("Domain:");
        ui.label(egui::RichText::new(&config.selected_domain).strong());
    });
    ui.add_space(12.0);
    
    // === Target Class Type ===
    ui.group(|ui| {
        ui.label(egui::RichText::new("Target Class Type").strong());
        ui.add_space(4.0);
        
        ui.horizontal(|ui| {
            ui.selectable_value(&mut config.target_class, SyncTargetClass::Part, "Part");
            ui.selectable_value(&mut config.target_class, SyncTargetClass::MeshPart, "MeshPart");
            ui.selectable_value(&mut config.target_class, SyncTargetClass::Model, "Model");
            ui.selectable_value(&mut config.target_class, SyncTargetClass::Folder, "Folder");
        });
    });
    ui.add_space(8.0);
    
    // === Spawn Layout ===
    ui.group(|ui| {
        ui.label(egui::RichText::new("Spawn Layout").strong());
        ui.add_space(4.0);
        
        ui.horizontal(|ui| {
            if ui.selectable_label(matches!(config.layout, SpawnLayout::Horizontal), "Horizontal").clicked() {
                config.layout = SpawnLayout::Horizontal;
            }
            if ui.selectable_label(matches!(config.layout, SpawnLayout::Vertical), "Vertical").clicked() {
                config.layout = SpawnLayout::Vertical;
            }
            if ui.selectable_label(matches!(config.layout, SpawnLayout::Depth), "Depth").clicked() {
                config.layout = SpawnLayout::Depth;
            }
            if ui.selectable_label(matches!(config.layout, SpawnLayout::Grid { .. }), "Grid").clicked() {
                config.layout = SpawnLayout::Grid { columns: 5 };
            }
            if ui.selectable_label(matches!(config.layout, SpawnLayout::Stacked), "Stacked").clicked() {
                config.layout = SpawnLayout::Stacked;
            }
        });
        
        // Grid columns if grid selected
        if let SpawnLayout::Grid { ref mut columns } = config.layout {
            ui.horizontal(|ui| {
                ui.label("Columns:");
                let mut cols = *columns as i32;
                if ui.add(egui::DragValue::new(&mut cols).range(1..=20)).changed() {
                    *columns = cols.max(1) as u32;
                }
            });
        }
        
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Spacing (X, Y, Z):");
            ui.add(egui::DragValue::new(&mut config.spacing[0]).speed(0.1).prefix("X: "));
            ui.add(egui::DragValue::new(&mut config.spacing[1]).speed(0.1).prefix("Y: "));
            ui.add(egui::DragValue::new(&mut config.spacing[2]).speed(0.1).prefix("Z: "));
        });
        
        ui.horizontal(|ui| {
            ui.label("Origin Offset:");
            ui.add(egui::DragValue::new(&mut config.origin_offset[0]).speed(0.1).prefix("X: "));
            ui.add(egui::DragValue::new(&mut config.origin_offset[1]).speed(0.1).prefix("Y: "));
            ui.add(egui::DragValue::new(&mut config.origin_offset[2]).speed(0.1).prefix("Z: "));
        });
    });
    ui.add_space(8.0);
    
    // === Default Appearance ===
    ui.group(|ui| {
        ui.label(egui::RichText::new("Default Appearance").strong());
        ui.add_space(4.0);
        
        ui.horizontal(|ui| {
            ui.label("Size (W, H, D):");
            ui.add(egui::DragValue::new(&mut config.default_size[0]).speed(0.1).range(0.1..=100.0).prefix("W: "));
            ui.add(egui::DragValue::new(&mut config.default_size[1]).speed(0.1).range(0.1..=100.0).prefix("H: "));
            ui.add(egui::DragValue::new(&mut config.default_size[2]).speed(0.1).range(0.1..=100.0).prefix("D: "));
        });
        
        ui.horizontal(|ui| {
            ui.label("Default Color:");
            let mut color = egui::Color32::from_rgba_unmultiplied(
                (config.default_color[0] * 255.0) as u8,
                (config.default_color[1] * 255.0) as u8,
                (config.default_color[2] * 255.0) as u8,
                (config.default_color[3] * 255.0) as u8,
            );
            if ui.color_edit_button_srgba(&mut color).changed() {
                config.default_color = [
                    color.r() as f32 / 255.0,
                    color.g() as f32 / 255.0,
                    color.b() as f32 / 255.0,
                    color.a() as f32 / 255.0,
                ];
            }
        });
    });
    ui.add_space(8.0);
    
    // === Field Mappings ===
    ui.group(|ui| {
        ui.label(egui::RichText::new("Field Mappings").strong());
        ui.add_space(4.0);
        
        // Name field
        ui.horizontal(|ui| {
            ui.label("Name from field:");
            if config.available_fields.is_empty() {
                ui.add(egui::TextEdit::singleline(&mut config.name_field).desired_width(150.0).hint_text("e.g., patient_name"));
            } else {
                egui::ComboBox::from_id_salt("name_field_combo")
                    .selected_text(if config.name_field.is_empty() { "Select field..." } else { &config.name_field })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.name_field, String::new(), "(none)");
                        for field in &config.available_fields {
                            ui.selectable_value(&mut config.name_field, field.clone(), field);
                        }
                    });
            }
        });
        
        // Color field
        ui.horizontal(|ui| {
            ui.label("Color from field:");
            if config.available_fields.is_empty() {
                ui.add(egui::TextEdit::singleline(&mut config.color_field).desired_width(150.0).hint_text("e.g., status"));
            } else {
                egui::ComboBox::from_id_salt("color_field_combo")
                    .selected_text(if config.color_field.is_empty() { "Select field..." } else { &config.color_field })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.color_field, String::new(), "(none)");
                        for field in &config.available_fields {
                            ui.selectable_value(&mut config.color_field, field.clone(), field);
                        }
                    });
            }
        });
        
        // Color mappings (conditional formatting)
        if !config.color_field.is_empty() {
            ui.add_space(4.0);
            ui.label("Color Mappings (value → color):");
            
            let mut to_remove: Option<usize> = None;
            for (i, mapping) in config.color_mappings.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("\"{}\" →", mapping.field_value));
                    let color = egui::Color32::from_rgba_unmultiplied(
                        (mapping.color[0] * 255.0) as u8,
                        (mapping.color[1] * 255.0) as u8,
                        (mapping.color[2] * 255.0) as u8,
                        (mapping.color[3] * 255.0) as u8,
                    );
                    ui.colored_label(color, "■■■");
                    if ui.small_button("🗑").clicked() {
                        to_remove = Some(i);
                    }
                });
            }
            if let Some(idx) = to_remove {
                config.color_mappings.remove(idx);
            }
            
            // Add new mapping
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut config.new_color_value).desired_width(80.0).hint_text("value"));
                ui.label("→");
                let mut color = egui::Color32::from_rgba_unmultiplied(
                    (config.new_color_rgba[0] * 255.0) as u8,
                    (config.new_color_rgba[1] * 255.0) as u8,
                    (config.new_color_rgba[2] * 255.0) as u8,
                    (config.new_color_rgba[3] * 255.0) as u8,
                );
                if ui.color_edit_button_srgba(&mut color).changed() {
                    config.new_color_rgba = [
                        color.r() as f32 / 255.0,
                        color.g() as f32 / 255.0,
                        color.b() as f32 / 255.0,
                        color.a() as f32 / 255.0,
                    ];
                }
                if ui.button("➕").clicked() && !config.new_color_value.is_empty() {
                    config.color_mappings.push(ColorMapping {
                        field_value: config.new_color_value.clone(),
                        color: config.new_color_rgba,
                    });
                    config.new_color_value.clear();
                }
            });
        }
    });
    ui.add_space(8.0);
    
    // === Billboard Labels ===
    ui.group(|ui| {
        ui.label(egui::RichText::new("Billboard Labels (MindSpace Text)").strong());
        ui.add_space(4.0);
        
        ui.checkbox(&mut config.show_billboard, "Show billboard labels");
        
        if config.show_billboard {
            ui.horizontal(|ui| {
                ui.label("Label from field:");
                if config.available_fields.is_empty() {
                    ui.add(egui::TextEdit::singleline(&mut config.billboard_field).desired_width(150.0).hint_text("e.g., display_name"));
                } else {
                    egui::ComboBox::from_id_salt("billboard_field_combo")
                        .selected_text(if config.billboard_field.is_empty() { "Select field..." } else { &config.billboard_field })
                        .show_ui(ui, |ui| {
                            for field in &config.available_fields {
                                ui.selectable_value(&mut config.billboard_field, field.clone(), field);
                            }
                        });
                }
            });
            
            ui.horizontal(|ui| {
                ui.label("Billboard Offset:");
                ui.add(egui::DragValue::new(&mut config.billboard_offset[0]).speed(0.1).prefix("X: "));
                ui.add(egui::DragValue::new(&mut config.billboard_offset[1]).speed(0.1).prefix("Y: "));
                ui.add(egui::DragValue::new(&mut config.billboard_offset[2]).speed(0.1).prefix("Z: "));
            });
            
            ui.horizontal(|ui| {
                ui.label("Text Alignment:");
                ui.selectable_value(&mut config.billboard_alignment, 0, "Left");
                ui.selectable_value(&mut config.billboard_alignment, 1, "Center");
                ui.selectable_value(&mut config.billboard_alignment, 2, "Right");
            });
        }
    });
    ui.add_space(16.0);
    
    // === Action Buttons ===
    ui.separator();
    ui.add_space(8.0);
    
    ui.horizontal(|ui| {
        if ui.button("🔄 Sync Now").clicked() {
            // TODO: Trigger sync - will be implemented in sync logic step
            info!("Sync Domain triggered for domain: {}", config.selected_domain);
        }
        
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Save Configuration").clicked() {
                // TODO: Save config to folder's sync_config
                info!("Save sync config for domain: {}", config.selected_domain);
            }
        });
    });
    
    }); // End ScrollArea
}
