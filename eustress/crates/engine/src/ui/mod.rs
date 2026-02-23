//! Eustress Engine UI Module - Slint Exclusive
//! All UI is handled by Slint declarative UI framework
//! egui has been completely removed

#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

use bevy::prelude::*;
use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use parking_lot::RwLock;
use bevy::log::{info, warn};

use crate::commands::{SelectionManager, TransformManager};

// ============================================================================
// Slint UI Integration (Primary UI System)
// ============================================================================

// Slint UI with software renderer
pub mod slint_ui;
pub mod slint_native;
pub mod floating_windows;

// Core modules that don't depend on egui
mod file_dialogs;
mod spawn_events;
mod menu_events;
mod world_view;
pub mod webview;
pub mod file_icons;
pub mod center_tabs;
pub mod monaco_bridge;

// Re-exports
pub use file_dialogs::{SceneFile, FileEvent, pick_open_file, pick_save_file};
pub use script_editor::OpenScriptEvent;
pub use spawn_events::{
    SpawnPartEvent, PastePartEvent, SpawnEventsPlugin,
    SpawnTerrainEvent, ToggleTerrainEditEvent, SetTerrainBrushEvent,
    ImportTerrainEvent, ExportTerrainEvent,
};
pub use menu_events::MenuActionEvent;
pub use world_view::{UIWorldSnapshot, UIActionQueue, UIAction, WorldViewPlugin};

// ============================================================================
// Bevy Resource Wrappers
// ============================================================================

#[derive(Resource, Clone)]
pub struct BevySelectionManager(pub Arc<RwLock<SelectionManager>>);

#[derive(Resource, Clone)]
pub struct BevyTransformManager(pub Arc<RwLock<TransformManager>>);

// ============================================================================
// Tool and Mode Enums
// ============================================================================

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
    Terrain,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransformMode {
    #[default]
    World,
    Local,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    #[default]
    Perspective,
    Top,
    Front,
    Right,
    Orthographic,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RibbonTab {
    #[default]
    Home,
    Model,
    Test,
    View,
    Plugins,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecondaryPanelTab {
    #[default]
    Terrain,
    MindSpace,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MindSpaceMode {
    #[default]
    Edit,
    Connect,
}

#[derive(Clone, Debug)]
pub enum TabEntry {
    BuiltIn { name: String },
    Plugin { plugin_id: String, name: String },
}

#[derive(Clone, Debug, Default)]
pub struct CustomTab {
    pub name: String,
    pub items: Vec<String>,
}

#[derive(Default, Clone, Debug)]
pub struct RibbonTabManagerState {
    pub show: bool,
    pub selected_tab: Option<usize>,
}

#[derive(Default, Clone, Debug)]
pub struct SyncDomainModalState {
    pub domain_name: String,
    pub object_type: String,
}

// ============================================================================
// Service Types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum ServiceType {
    Workspace,
    Players,
    Lighting,
    SoulService,
    ServerStorage,
    StarterGui,
    StarterPack,
    StarterPlayer,
    SoundService,
    Teams,
    Chat,
    LocalizationService,
    TestService,
}

impl ServiceType {
    pub fn all() -> &'static [ServiceType] {
        &[
            ServiceType::Workspace, ServiceType::Players, ServiceType::Lighting,
            ServiceType::SoulService, ServiceType::ServerStorage, ServiceType::StarterGui,
            ServiceType::StarterPack, ServiceType::StarterPlayer, ServiceType::SoundService,
            ServiceType::Teams, ServiceType::Chat, ServiceType::LocalizationService,
            ServiceType::TestService,
        ]
    }
    
    pub fn name(&self) -> &'static str {
        match self {
            ServiceType::Workspace => "Workspace",
            ServiceType::Players => "Players",
            ServiceType::Lighting => "Lighting",
            ServiceType::SoulService => "SoulService",
            ServiceType::ServerStorage => "ServerStorage",
            ServiceType::StarterGui => "StarterGui",
            ServiceType::StarterPack => "StarterPack",
            ServiceType::StarterPlayer => "StarterPlayer",
            ServiceType::SoundService => "SoundService",
            ServiceType::Teams => "Teams",
            ServiceType::Chat => "Chat",
            ServiceType::LocalizationService => "LocalizationService",
            ServiceType::TestService => "TestService",
        }
    }
    
    pub fn class_name(&self) -> &'static str { self.name() }
}

#[derive(Component, Debug, Clone, Reflect)]
pub struct ServiceOwner(pub ServiceType);

// ============================================================================
// UI State Resources
// ============================================================================

/// Viewport bounds reported by Slint layout (in physical pixels from top-left)
/// Used by camera controller to clip 3D rendering to the viewport area
#[derive(Resource, Default, Clone, Copy)]
pub struct ViewportBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

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
    pub play_solo_requested: bool,
    pub play_with_character_requested: bool,
    pub pause_requested: bool,
    pub stop_requested: bool,
    pub mindspace_panel_visible: bool,
    pub secondary_panel_tab: SecondaryPanelTab,
    pub show_publish_dialog: bool,
    pub publish_as_new: bool,
    pub trigger_login: bool,
    pub pending_paste: bool,
    pub pending_file_action: Option<FileEvent>,
    pub show_network_panel: bool,
    pub show_forge_connect_window: bool,
    pub show_stress_test_window: bool,
    pub synthetic_client_count: u32,
    pub synthetic_clients_changed: bool,
    pub show_global_sources_window: bool,
    pub show_domains_window: bool,
    pub show_global_variables_window: bool,
    pub quick_add_source_type: Option<String>,
    pub show_sync_domain_modal: bool,
    pub sync_domain_config: SyncDomainModalState,
    pub ribbon_tab: RibbonTab,
    pub visible_tabs: Vec<TabEntry>,
    pub custom_tabs: Vec<CustomTab>,
    pub tab_manager: RibbonTabManagerState,
    pub browser_open_request: Option<(String, String)>,
    pub show_find_dialog: bool,
    pub show_settings_window: bool,
    pub has_unsaved_changes: bool,
    pub show_exit_confirmation: bool,
    pub mindspace_mode: MindSpaceMode,
    pub mindspace_edit_buffer: String,
    pub mindspace_font: eustress_common::classes::Font,
    pub mindspace_font_size: f32,
    pub mindspace_editing_entity: Option<Entity>,
    pub mindspace_link_source: Option<Entity>,
    pub mindspace_last_selected: Option<String>,
    
    // Center tab management (Space1 + script/web tabs)
    pub center_tabs: Vec<slint_ui::CenterTabData>,
    pub active_center_tab: i32,
    pub pending_open_script: Option<(i32, String)>,
    pub pending_open_web: Option<String>,
    pub pending_close_tab: Option<i32>,
    pub pending_reorder: Option<(i32, i32)>,
    pub script_editor_content: String,
    
    // Web browser state for active web tab
    pub pending_web_navigate: Option<String>,
    pub pending_web_back: bool,
    pub pending_web_forward: bool,
    pub pending_web_refresh: bool,
}

impl Default for StudioState {
    fn default() -> Self {
        Self {
            show_explorer: true,
            show_properties: true,
            show_output: true,
            show_keybindings_window: false,
            show_terrain_editor: false,
            show_soul_settings_window: false,
            current_tool: Tool::Select,
            transform_mode: TransformMode::World,
            play_solo_requested: false,
            play_with_character_requested: false,
            pause_requested: false,
            stop_requested: false,
            mindspace_panel_visible: false,
            secondary_panel_tab: SecondaryPanelTab::Terrain,
            show_publish_dialog: false,
            publish_as_new: false,
            trigger_login: false,
            pending_paste: false,
            pending_file_action: None,
            show_network_panel: false,
            show_forge_connect_window: false,
            show_stress_test_window: false,
            synthetic_client_count: 0,
            synthetic_clients_changed: false,
            show_global_sources_window: false,
            show_domains_window: false,
            show_global_variables_window: false,
            quick_add_source_type: None,
            show_sync_domain_modal: false,
            sync_domain_config: SyncDomainModalState::default(),
            ribbon_tab: RibbonTab::Home,
            visible_tabs: vec![
                TabEntry::BuiltIn { name: "Home".into() },
                TabEntry::BuiltIn { name: "Model".into() },
                TabEntry::BuiltIn { name: "Test".into() },
                TabEntry::BuiltIn { name: "View".into() },
                TabEntry::BuiltIn { name: "Plugins".into() },
            ],
            custom_tabs: Vec::new(),
            tab_manager: RibbonTabManagerState::default(),
            browser_open_request: None,
            show_find_dialog: false,
            show_settings_window: false,
            has_unsaved_changes: false,
            show_exit_confirmation: false,
            mindspace_mode: MindSpaceMode::Edit,
            mindspace_edit_buffer: String::new(),
            mindspace_font: eustress_common::classes::Font::default(),
            mindspace_font_size: 14.0,
            mindspace_editing_entity: None,
            mindspace_link_source: None,
            mindspace_last_selected: None,
            center_tabs: Vec::new(),
            active_center_tab: 0,
            pending_open_script: None,
            pending_open_web: None,
            pending_close_tab: None,
            pending_reorder: None,
            script_editor_content: String::new(),
            pending_web_navigate: None,
            pending_web_back: false,
            pending_web_forward: false,
            pending_web_refresh: false,
        }
    }
}

#[derive(Resource)]
pub struct OutputConsole {
    pub entries: Vec<LogEntry>,
    pub max_entries: usize,
    pub auto_scroll: bool,
    pub filter_level: LogLevel,
}

impl Default for OutputConsole {
    fn default() -> Self {
        Self { entries: Vec::new(), max_entries: 1000, auto_scroll: true, filter_level: LogLevel::Info }
    }
}

impl OutputConsole {
    pub fn info(&mut self, msg: impl Into<String>) { self.push(LogLevel::Info, msg.into()); }
    pub fn warn(&mut self, msg: impl Into<String>) { self.push(LogLevel::Warn, msg.into()); }
    pub fn warning(&mut self, msg: impl Into<String>) { self.push(LogLevel::Warn, msg.into()); }
    pub fn error(&mut self, msg: impl Into<String>) { self.push(LogLevel::Error, msg.into()); }
    pub fn debug(&mut self, msg: impl Into<String>) { self.push(LogLevel::Debug, msg.into()); }
    fn push(&mut self, level: LogLevel, message: String) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        self.entries.push(LogEntry { level, message, timestamp });
        while self.entries.len() > self.max_entries { self.entries.remove(0); }
    }
    pub fn clear(&mut self) { self.entries.clear(); }
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub timestamp: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LogLevel { #[default] Info, Warn, Warning, Error, Debug, System }

#[derive(Resource, Default)]
pub struct CommandBarState {
    pub input: String,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub is_focused: bool,
    pub show: bool,
}

#[derive(Resource, Default)]
pub struct CollaborationState {
    pub connected: bool,
    pub users: Vec<CollaborationUser>,
    pub room_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CollaborationUser {
    pub id: String,
    pub name: String,
    pub color: bevy::color::Color,
    pub cursor_position: Option<Vec3>,
}

#[derive(Resource, Default)]
pub struct ToolboxState {
    pub expanded_categories: HashSet<String>,
    pub search_query: String,
}

#[derive(Resource, Default)]
pub struct StudioDockState {
    pub left_width: f32,
    pub right_width: f32,
    pub bottom_height: f32,
}

#[derive(Resource, Default)]
pub struct ExplorerExpanded {
    pub expanded: HashSet<Entity>,
    pub selected_service: Option<ServiceType>,
}

impl ExplorerExpanded {
    pub fn toggle(&mut self, entity: Entity) {
        if self.expanded.contains(&entity) {
            self.expanded.remove(&entity);
        } else {
            self.expanded.insert(entity);
        }
    }
    
    pub fn select_service(&mut self, service: ServiceType) {
        self.selected_service = Some(service);
    }
    
    pub fn deselect_service(&mut self) {
        self.selected_service = None;
    }
    
    pub fn toggle_service(&mut self, service: ServiceType) {
        if self.selected_service == Some(service) {
            self.selected_service = None;
        } else {
            self.selected_service = Some(service);
        }
    }
}

#[derive(Resource, Default)]
pub struct ExplorerState {
    pub selected: Option<Entity>,
    pub search_query: String,
    pub filter: String,
}

// ExplorerToggleEvent is defined in slint_ui module
pub use slint_ui::ExplorerToggleEvent;

#[derive(Resource, Default)]
pub struct ExplorerCache {
    pub entities: Vec<Entity>,
    pub dirty: bool,
}

#[derive(Resource, Default)]
pub struct ExplorerDragSelect {
    pub active: bool,
    pub start: Option<Vec2>,
}

#[derive(Resource)]
pub struct UIPerformance {
    pub frame_times: Vec<f32>,
    pub fps: f32,
    pub avg_frame_time_ms: f32,
    pub ui_budget_ms: f32,
    pub last_ui_time_ms: f32,
    pub skip_heavy_updates: bool,
    pub frame_counter: u64,
}

impl Default for UIPerformance {
    fn default() -> Self {
        Self {
            frame_times: Vec::with_capacity(60), fps: 60.0, avg_frame_time_ms: 16.67,
            ui_budget_ms: 8.0, last_ui_time_ms: 0.0, skip_heavy_updates: false, frame_counter: 0,
        }
    }
}

#[derive(Resource, Default)]
pub struct ViewSelectorState {
    pub view_mode: ViewMode,
    pub wireframe: bool,
    pub grid: bool,
}

#[derive(Resource, Default)]
pub struct AssetManagerState {
    pub search_query: String,
    pub selected_asset: Option<String>,
}

#[derive(Resource, Default)]
pub struct DynamicPropertiesPanel {
    pub properties: HashMap<String, String>,
}

#[derive(Resource, Default)]
pub struct AttributesPanelState {
    pub show: bool,
}

#[derive(Resource, Default)]
pub struct PendingMenuActions {
    pub actions: Vec<String>,
}

#[derive(Resource, Default)]
pub struct AdvancedSearchState {
    pub query: String,
    pub results: Vec<Entity>,
}

#[derive(Resource, Default)]
pub struct DockingLayout {
    pub left_visible: bool,
    pub right_visible: bool,
    pub bottom_visible: bool,
}

#[derive(Resource, Default)]
pub struct DockDragState {
    pub dragging: bool,
}

// ============================================================================
// Events
// ============================================================================

#[derive(Event, Message, Clone, Debug)]
pub struct InsertObjectEvent {
    pub class_name: String,
    pub parent: Option<Entity>,
}

// ============================================================================
// Stub Modules (for compatibility with other code)
// All egui-dependent modules have been disabled - these are minimal stubs
// ============================================================================

pub mod explorer {
    pub use super::{ServiceType, ServiceOwner, ExplorerExpanded, ExplorerState, ExplorerToggleEvent, ExplorerCache};
    pub struct ExplorerPanel;
}

pub mod context_menu {
    use bevy::prelude::*;
    pub use super::InsertObjectEvent;
    
    #[derive(Resource, Default)]
    pub struct ContextMenuState { pub open: bool, pub position: (f32, f32) }
    
    pub struct ContextMenuPlugin;
    impl Plugin for ContextMenuPlugin { fn build(&self, _app: &mut App) {} }
}

pub mod service_properties {
    use bevy::prelude::*;
    
    // Service resources that were previously initialized by the egui-based plugin
    #[derive(Resource, Clone, Default)]
    pub struct WorkspaceService {
        pub name: String,
        pub gravity: f32,
        pub fallen_parts_destroy_height: f32,
        pub air_density: f32,
    }
    
    #[derive(Resource, Clone, Default)]
    pub struct LightingService { pub name: String }
    
    #[derive(Resource, Clone, Default)]
    pub struct PlayersService { pub name: String, pub max_players: u32 }
    
    #[derive(Resource, Clone, Default)]
    pub struct ReplicatedStorageService { pub name: String }
    
    #[derive(Resource, Clone, Default)]
    pub struct ServerStorageService { pub name: String }
    
    #[derive(Resource, Clone, Default)]
    pub struct ServerScriptServiceService { pub name: String }
    
    #[derive(Resource, Clone, Default)]
    pub struct StarterGuiService { pub name: String }
    
    #[derive(Resource, Clone, Default)]
    pub struct StarterPackService { pub name: String }
    
    #[derive(Resource, Clone, Default)]
    pub struct StarterPlayerService { pub name: String }
    
    #[derive(Resource, Clone, Default)]
    pub struct SoundServiceService { pub name: String }
    
    #[derive(Resource, Clone, Default)]
    pub struct TeamsService { pub name: String }
    
    #[derive(Resource, Clone, Default)]
    pub struct ChatService { pub name: String }
    
    pub struct ServicePropertiesPlugin;
    impl Plugin for ServicePropertiesPlugin {
        fn build(&self, app: &mut App) {
            app
                .init_resource::<WorkspaceService>()
                .init_resource::<LightingService>()
                .init_resource::<PlayersService>()
                .init_resource::<ReplicatedStorageService>()
                .init_resource::<ServerStorageService>()
                .init_resource::<ServerScriptServiceService>()
                .init_resource::<StarterGuiService>()
                .init_resource::<StarterPackService>()
                .init_resource::<StarterPlayerService>()
                .init_resource::<SoundServiceService>()
                .init_resource::<TeamsService>()
                .init_resource::<ChatService>();
        }
    }
    pub fn render_service_properties() {}
}

pub mod docking {
    use bevy::prelude::*;
    pub use super::{DockingLayout, DockDragState};
    
    #[derive(Clone, Copy, Debug)] pub enum DockZone { Left, Right, Bottom, Center }
    #[derive(Clone, Copy, Debug)] pub enum DockArea { Explorer, Properties, Output }
    #[derive(Clone, Copy, Debug)] pub enum PanelId { Explorer, Properties, Output, Toolbox }
    
    pub struct DockingPlugin;
    impl Plugin for DockingPlugin { fn build(&self, _app: &mut App) {} }
}

pub mod notifications {
    use bevy::prelude::*;
    
    pub struct NotificationsPlugin;
    impl Plugin for NotificationsPlugin { fn build(&self, _app: &mut App) {} }
}

pub mod command_bar {
    pub use super::CommandBarState;
    pub struct CommandBarPanel;
    
    impl CommandBarPanel {
        pub fn cache_rune_script(_state: &mut CommandBarState, _script: String) {
            // Stub - command bar is now handled by Slint
        }
        
        pub fn cache_rune_script_with_source(_state: &mut CommandBarState, _script: String, _source: String) {
            // Stub - command bar is now handled by Slint
        }
    }
}

pub mod script_editor {
    use bevy::prelude::*;
    use std::collections::HashMap;
    
    #[derive(Resource, Default)]
    pub struct ScriptEditorState {
        pub open_scripts: Vec<Entity>,
        pub active_script: Option<Entity>,
        pub edit_buffers: HashMap<Entity, String>,
    }
    
    impl ScriptEditorState {
        pub fn open_parameters_editor(&mut self, _entity: Entity, _name: &str) {
            // Parameters editor is now handled by Slint
        }
    }
    
    #[derive(Event, bevy::ecs::message::Message)]
    pub struct OpenScriptEvent { pub entity: Entity }
    
    pub struct ScriptEditorPlugin;
    impl Plugin for ScriptEditorPlugin { fn build(&self, _app: &mut App) {} }
    
    pub fn render_tab_bar() {}
    pub fn render_script_editor() {}
    
    #[derive(Resource, Default)] pub struct BrowserState { pub url: String }
    #[derive(Resource, Default)] pub struct BrowserTabState { pub tabs: Vec<String> }
    #[derive(Event, Message)] pub struct OpenBrowserEvent { pub url: String }
    pub fn render_browser_controls() {}
    pub fn render_browser_content() {}
    #[derive(Resource, Default)] pub struct BrowserBookmarks { pub bookmarks: Vec<Bookmark> }
    #[derive(Clone, Debug, Default)] pub struct Bookmark { pub name: String, pub url: String }
    #[derive(Clone, Debug, Default)] pub struct BookmarkFolder { pub name: String }
    #[derive(Clone, Debug, Default)] pub struct HistoryEntry { pub url: String }
}

pub mod icons {
    pub const ICON_SIZE: f32 = 16.0;
    pub fn draw_class_icon() {}
    pub fn draw_service_icon() {}
    pub fn draw_brush_icon() {}
    pub fn draw_material_icon() {}
    pub fn draw_brush_shape_icon() {}
}

pub mod class_icons {
    use crate::classes::ClassName;
    pub fn class_color(_: &ClassName) -> [f32; 3] { [1.0, 1.0, 1.0] }
    pub fn class_category(_: &ClassName) -> &'static str { "Other" }
    pub fn class_label(_: &ClassName) -> &'static str { "Unknown" }
    pub fn class_label_compact(_: &ClassName) -> &'static str { "?" }
    pub fn class_filter_options() -> Vec<&'static str> { vec![] }
    pub fn matches_filter(_: &ClassName, _: &str) -> bool { true }
    pub fn class_icon(_: &ClassName) -> &'static str { "?" }
    pub fn class_tooltip(_: &ClassName) -> &'static str { "" }
}

pub mod ribbon {
    pub struct RibbonPanel;
}

pub mod view_selector {
    use bevy::prelude::*;
    pub use super::{ViewMode, ViewSelectorState};
    pub struct ViewSelectorWidget;
    pub fn handle_view_mode_changes() {}
    pub fn apply_wireframe_mode() {}
    pub fn handle_view_mode_shortcuts() {}
}

pub mod output {
    pub use super::{OutputConsole, LogLevel, LogEntry};
    pub struct OutputPanel;
    pub fn capture_bevy_logs() {}
    pub fn push_to_log_buffer(_: &str) {}
    pub fn parse_and_push_log(_: &str) {}
}

pub mod asset_manager {
    pub use super::AssetManagerState;
    pub struct AssetManagerPanel;
}

pub mod collaboration {
    pub use super::{CollaborationState, CollaborationUser};
    pub struct CollaborationPanel;
    pub fn update_collaboration_cursors() {}
}

pub mod toolbox {
    pub use super::ToolboxState;
    pub struct ToolboxPanel;
}

pub mod dock {
    pub use super::StudioDockState;
    #[derive(Clone, Debug)] pub enum Tab { Explorer, Properties, Output }
    #[derive(Clone, Debug)] pub enum LeftTab { Explorer, Toolbox }
    #[derive(Clone, Debug)] pub enum RightTab { Properties, History }
}

pub mod dynamic_properties {
    use bevy::prelude::*;
    pub use super::DynamicPropertiesPanel;
    pub struct DynamicPropertiesPlugin;
    impl Plugin for DynamicPropertiesPlugin { fn build(&self, _app: &mut App) {} }
}

pub mod selection_sync {
    use bevy::prelude::*;
    #[derive(Resource, Default)] pub struct SelectionSyncState { pub synced: bool }
    pub struct SelectionSyncPlugin;
    impl Plugin for SelectionSyncPlugin { fn build(&self, _app: &mut App) {} }
    pub fn sync_selection_to_properties() {}
}

pub mod attributes_ui {
    pub use super::AttributesPanelState;
    #[derive(Default)] pub struct AddAttributeState { pub name: String }
    #[derive(Default)] pub struct AddTagState { pub tag: String }
    #[derive(Default)] pub struct ParametersEditorState { pub editing: bool }
    pub fn render_attributes_panel() {}
}

pub mod history_panel {
    pub struct HistoryPanel;
}

pub mod property_widgets {
    pub fn render_property_widget() {}
}

pub mod publish {
    use bevy::prelude::*;
    #[derive(Resource, Default)]
    pub struct PublishState { pub publishing: bool }
}

pub mod soul_panel {
    use bevy::prelude::*;
    #[derive(Resource, Default)] pub struct SoulPanelState { pub visible: bool }
    pub struct SoulPanelPlugin;
    impl Plugin for SoulPanelPlugin { fn build(&self, _app: &mut App) {} }
    #[derive(Clone, Debug)] pub struct SoulScriptEntry { pub entity: Entity }
    #[derive(Clone, Debug, Default)] pub enum ScriptBuildStatus { #[default] None, Building, Success, Failed }
}

pub mod ai_generation {
    use bevy::prelude::*;
    pub struct AIGenerationPanel;
    pub struct AIGenerationUIPlugin;
    impl Plugin for AIGenerationUIPlugin { fn build(&self, _app: &mut App) {} }
    pub fn show_generation_queue() {}
}

pub mod explorer_search {
    use bevy::prelude::*;
    pub use super::AdvancedSearchState;
    #[derive(Clone, Debug, Default)] pub struct SearchQuery { pub text: String }
    #[derive(Clone, Debug)] pub struct SearchCriterion { pub field: String }
    #[derive(Clone, Debug)] pub struct SearchResult { pub entity: Entity }
    #[derive(Clone, Debug, Default)] pub enum CompareOp { #[default] Equals, Contains }
    #[derive(Default)] pub struct ExplorerSearchEngine;
    #[derive(Clone, Debug, Default)] pub enum FilterBuilderStep { #[default] Start }
    #[derive(Default)] pub struct SearchPresets;
    pub fn get_searchable_properties() -> Vec<PropertyInfo> { vec![] }
    #[derive(Clone, Debug)] pub struct PropertyInfo { pub name: String }
}

pub mod explorer_search_ui {
    pub fn show_advanced_search_panel() {}
    pub fn show_search_results() {}
    pub fn show_syntax_help() {}
}

pub mod cef_browser {
    use bevy::prelude::*;
    pub struct CefBrowserPlugin;
    impl Plugin for CefBrowserPlugin { fn build(&self, _app: &mut App) {} }
}

// ============================================================================
// Stub Functions
// ============================================================================

pub fn capture_bevy_logs(_console: ResMut<OutputConsole>) {}
pub fn push_to_log_buffer(_msg: &str) {}
pub fn parse_and_push_log(_msg: &str) {}

pub fn handle_explorer_toggle(
    mut events: MessageReader<ExplorerToggleEvent>,
    mut expanded: ResMut<ExplorerExpanded>,
) {
    for event in events.read() {
        if expanded.expanded.contains(&event.entity) {
            expanded.expanded.remove(&event.entity);
        } else {
            expanded.expanded.insert(event.entity);
        }
    }
}

pub fn handle_window_close_request(
    state: Option<ResMut<StudioState>>,
    mut exit_events: MessageWriter<bevy::app::AppExit>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut close_events: MessageReader<bevy::window::WindowCloseRequested>,
) {
    let Some(mut state) = state else { return };
    // Handle Alt+F4
    if keyboard.just_pressed(KeyCode::F4) && keyboard.pressed(KeyCode::AltLeft) {
        if state.has_unsaved_changes {
            state.show_exit_confirmation = true;
        } else {
            exit_events.write(bevy::app::AppExit::Success);
        }
    }
    
    // Handle window X button click
    for _event in close_events.read() {
        if state.has_unsaved_changes {
            state.show_exit_confirmation = true;
        } else {
            exit_events.write(bevy::app::AppExit::Success);
        }
    }
}

pub fn handle_view_mode_changes() {}
pub fn apply_wireframe_mode() {}
pub fn handle_view_mode_shortcuts() {}
pub fn update_collaboration_cursors() {}
pub fn sync_selection_to_properties() {}

// ============================================================================
// StudioUiPlugin (Legacy - use SlintUiPlugin instead)
// ============================================================================

pub struct StudioUiPlugin {
    pub selection_manager: Arc<RwLock<SelectionManager>>,
    pub transform_manager: Arc<RwLock<TransformManager>>,
}

impl Plugin for StudioUiPlugin {
    fn build(&self, app: &mut App) {
        // Initialize StudioState resource (required by select_tool and other systems)
        app.init_resource::<StudioState>();
        warn!("StudioUiPlugin is deprecated - use SlintUiPlugin");
    }
}

fn try_restore_auth_session(mut auth_state: ResMut<crate::auth::AuthState>) {
    auth_state.try_restore_session();
}
