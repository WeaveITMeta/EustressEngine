//! Slint UI Plugin - Software renderer overlay on Bevy window
//! Renders Slint UI to a texture and composites it over the Bevy 3D scene

#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

use bevy::prelude::*;
use bevy::input::{ButtonState, mouse::MouseButtonInput};
use bevy::render::render_resource::{TextureDescriptor, TextureUsages, TextureFormat, Extent3d, TextureDimension};
use bevy::window::PrimaryWindow;
use bevy::camera::ScalingMode;
use bevy::camera::visibility::RenderLayers;
use std::sync::{Arc, Mutex};
use std::rc::{Rc, Weak};
use std::cell::Cell;
use std::cell::RefCell;
use std::path::Path;
use parking_lot::RwLock;

// Slint software renderer imports
use slint::platform::software_renderer::PremultipliedRgbaColor;
use slint::{LogicalPosition, PhysicalSize};
use slint::platform::WindowEvent;

use crate::commands::{SelectionManager, TransformManager};
use crate::ui::highlight::{highlight_to_lines, language_for_ext, HighlightLine as ComputedHighlightLine};
use super::file_dialogs::{SceneFile, FileEvent, PublishRequest};
use super::file_icons::load_file_icon;
use super::spawn_events::SpawnEventsPlugin;
use super::menu_events::MenuActionEvent;
use super::{SlintUIFocus};
use super::world_view::{WorldViewPlugin, UIWorldSnapshot};

// Include Slint modules - this creates StudioWindow type
slint::include_modules!();

// ============================================================================
// Slint Software Renderer - BevyWindowAdapter (from official bevy-hosts-slint)
// ============================================================================

/// Window adapter that bridges Slint to Bevy using software rendering.
/// Renders to a pixel buffer that Bevy uploads to a GPU texture.
struct BevyWindowAdapter {
    /// Current physical size of the window in pixels
    size: Cell<slint::PhysicalSize>,
    /// Display scale factor (1.0 for standard, 2.0 for HiDPI)
    scale_factor: Cell<f32>,
    /// The Slint window instance that receives events
    slint_window: slint::Window,
    /// Software renderer that renders UI into a pixel buffer
    software_renderer: slint::platform::software_renderer::SoftwareRenderer,
}

impl slint::platform::WindowAdapter for BevyWindowAdapter {
    fn window(&self) -> &slint::Window {
        &self.slint_window
    }

    fn size(&self) -> slint::PhysicalSize {
        self.size.get()
    }

    fn renderer(&self) -> &dyn slint::platform::Renderer {
        &self.software_renderer
    }

    fn set_visible(&self, _visible: bool) -> Result<(), slint::PlatformError> {
        Ok(())
    }

    fn request_redraw(&self) {}
}

impl BevyWindowAdapter {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|self_weak: &Weak<Self>| Self {
            size: Cell::new(slint::PhysicalSize::new(1600, 900)),
            scale_factor: Cell::new(1.0),
            slint_window: slint::Window::new(self_weak.clone()),
            // ReusedBuffer: only repaint dirty regions instead of full buffer each frame
            software_renderer: slint::platform::software_renderer::SoftwareRenderer::new_with_repaint_buffer_type(
                slint::platform::software_renderer::RepaintBufferType::ReusedBuffer,
            ),
        })
    }

    fn resize(&self, new_size: PhysicalSize, scale_factor: f32) {
        self.size.set(new_size);
        self.scale_factor.set(scale_factor);
        self.slint_window.dispatch_event(WindowEvent::Resized {
            size: self.size.get().to_logical(scale_factor),
        });
        self.slint_window
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
    }
}

// Thread-local storage for window adapters created by the platform
thread_local! {
    static SLINT_WINDOWS: RefCell<Vec<Weak<BevyWindowAdapter>>> = RefCell::new(Vec::new());
}

/// Custom Slint platform for Bevy integration
struct SlintBevyPlatform {}

impl slint::platform::Platform for SlintBevyPlatform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        let adapter = BevyWindowAdapter::new();
        let scale_factor = adapter.scale_factor.get();
        adapter.slint_window.dispatch_event(WindowEvent::Resized {
            size: adapter.size.get().to_logical(scale_factor),
        });
        adapter
            .slint_window
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
        SLINT_WINDOWS.with(|windows| {
            windows.borrow_mut().push(Rc::downgrade(&adapter));
        });
        Ok(adapter)
    }
}

/// Non-Send resource holding Slint UI context (must stay on main thread)
pub struct SlintUiState {
    /// The Slint StudioWindow instance
    pub window: StudioWindow,
    /// Reference to the window adapter for rendering and input
    pub adapter: Rc<BevyWindowAdapter>,
}

/// Resource to track if Slint overlay has been initialized
#[derive(Resource, Default)]
pub struct SlintOverlayInitialized(pub bool);

/// Marker component for the UI overlay sprite
#[derive(Component)]
pub struct SlintOverlaySprite;

/// Marker component for the UI overlay camera
#[derive(Component)]
pub struct SlintOverlayCamera;

/// Component tracking the Slint texture and material for GPU re-upload workaround
#[derive(Component)]
struct SlintScene {
    image: Handle<Image>,
    material: Handle<StandardMaterial>,
}


// ============================================================================
// Bevy Resource Wrappers
// ============================================================================

/// Bevy resource wrapping SelectionManager for UI access
#[derive(Resource, Clone)]
pub struct BevySelectionManager(pub Arc<RwLock<SelectionManager>>);

/// Bevy resource wrapping TransformManager for UI access
#[derive(Resource, Clone)]
pub struct BevyTransformManager(pub Arc<RwLock<TransformManager>>);

// ============================================================================
// Tool and Mode Enums
// ============================================================================

/// Current tool selection
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
    Terrain,
}

/// Transform mode (local vs world space)
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransformMode {
    #[default]
    World,
    Local,
}

/// View mode
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    #[default]
    Perspective,
    Top,
    Front,
    Right,
    Orthographic,
}

/// Ribbon tab selection
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RibbonTab {
    #[default]
    Home,
    Model,
    Test,
    View,
    Plugins,
}

/// Secondary panel tab (Terrain/MindSpace)
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecondaryPanelTab {
    #[default]
    Terrain,
    MindSpace,
}

/// MindSpace mode
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum MindSpaceMode {
    #[default]
    Edit,
    Connect,
}

/// Tab entry for ribbon
#[derive(Clone, Debug)]
pub enum TabEntry {
    BuiltIn { name: String },
    Plugin { plugin_id: String, name: String },
}

/// Custom tab definition
#[derive(Clone, Debug, Default)]
pub struct CustomTab {
    pub name: String,
    pub items: Vec<String>,
}

/// Ribbon tab manager state
#[derive(Default, Clone, Debug)]
pub struct RibbonTabManagerState {
    pub show: bool,
    pub selected_tab: Option<usize>,
}

/// Sync domain modal state
#[derive(Default, Clone, Debug)]
pub struct SyncDomainModalState {
    pub domain_name: String,
    pub object_type: String,
}

// ============================================================================
// System Set Labels
// ============================================================================

/// Public system set for ordering against Slint drain systems from other plugins.
/// Use `.after(SlintSystems::Drain)` to guarantee your system runs after
/// `drain_slint_actions` has consumed all queued Slint→Bevy actions for the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum SlintSystems {
    /// drain_slint_actions: converts SlintActionQueue entries into Bevy state/events
    Drain,
}

// ============================================================================
// Slint → Bevy Action Queue
// ============================================================================

/// Actions queued by Slint UI callbacks, drained by Bevy systems each frame.
/// Uses Arc<Mutex<>> because Slint callbacks capture a clone of the queue.
#[derive(Debug, Clone)]
pub enum SlintAction {
    // File operations
    NewUniverse,
    NewScene,
    OpenScene,
    SaveScene,
    SaveSceneAs,
    OpenPublishDialog,
    OpenPublishAsDialog,
    Publish(PublishRequest),
    
    // Edit operations
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Delete,
    Duplicate,
    SelectAll,
    
    // Tool selection
    SelectTool(String),
    
    // Transform mode
    SetTransformMode(String),
    
    // Play controls
    PlaySolo,
    PlayWithCharacter,
    Pause,
    Stop,
    
    // View
    SetViewMode(String),
    FocusSelected,
    ToggleWireframe,
    ToggleGrid,
    ToggleSnap,
    SetSnapIncrement(f32),
    
    // Panel toggles (from Slint → Bevy state sync)
    ToggleCommandBar,
    ShowKeybindings,
    ShowSoulSettings,
    ShowSettings,
    ShowFind,
    
    // Explorer (unified: entities + files)
    SelectNode(i32, String),      // (id, node_type: "entity"|"file")
    ExpandNode(i32, String),      // (id, node_type)
    CollapseNode(i32, String),    // (id, node_type)
    OpenNode(i32, String),        // (id, node_type) — double-click to open
    RenameNode(i32, String, String), // (id, new_name, node_type)
    AddService,                   // (+) button — open add-service dialog
    ExpandAll,                    // Expand all tree nodes in explorer
    CollapseAll,                  // Collapse all tree nodes in explorer
    
    // Properties
    PropertyChanged(String, String),
    SectionToggle(String), // Toggle collapse/expand of a property section by category name
    
    // Command bar
    ExecuteCommand(String),
    
    // Context menu
    InsertPart(String),
    ContextAction(String),
    
    // Terrain
    GenerateTerrain(String),
    ToggleTerrainEditMode,
    SetTerrainBrush(String),
    BrushSizeChanged(f32),
    BrushStrengthChanged(f32),
    BrushFalloffChanged(String),
    ImportHeightmap,
    ExportHeightmap,
    
    // Network
    StartServer,
    StopServer,
    ConnectForge,
    DisconnectForge,
    AllocateForgeServer,
    SpawnSyntheticClients(i32),
    DisconnectAllClients,
    
    // Data
    OpenGlobalSources,
    OpenDomains,
    OpenGlobalVariables,
    
    // MindSpace
    ToggleMindspace,
    MindspaceAddLabel,
    MindspaceConnect,
    
    // Auth
    Login,
    Logout,
    
    // Scripts
    BuildScript(i32),
    OpenScript(i32),
    
    // Center tab management
    CloseCenterTab(i32),
    SelectCenterTab(i32),
    ScriptContentChanged(String),
    ReorderCenterTab(i32, i32), // (from_index, to_index)
    ToggleTabMode(i32),         // Toggle summary/code for tab at StudioState index (1-based)
    
    // Web browser
    OpenWebTab(String),         // URL
    WebNavigate(String),        // Navigate active web tab
    WebGoBack,
    WebGoForward,
    WebRefresh,
    
    // Simulation settings — Apply is automatic on Save
    SaveSimulationSettings,
    AddSimWatchpoint,
    RemoveSimWatchpoint(i32),
    AddSimOutputBinding,
    RemoveSimOutputBinding(i32),

    // Layout
    ApplyLayoutPreset(i32),
    SaveLayoutToFile,
    LoadLayoutFromFile,
    ResetLayoutToDefault,
    ToggleThemeEditor,
    ApplyThemeSettings(bool, bool, f32), // dark-mode, high-contrast, ui-scale
    DetachPanelToWindow(String),
    
    // Viewport
    ViewportBoundsChanged(f32, f32, f32, f32), // x, y, width, height
    
    // Asset Manager
    AssetSelect(i32),
    AssetExpand(i32),
    AssetImport(String),
    AssetSearch(String),
    AssetCategoryChanged(String),

    // Generic menu action (ribbon Insert dropdown, Model menu, etc.)
    MenuAction(String),
    
    // Close
    CloseRequested,
    // Force exit — skip unsaved check (used after exit confirmation dialog)
    ForceExit,

    // Help icons — open /learn documentation URL
    // Destination: tabbed viewer or system browser depending on help-opens-in-tab setting
    OpenLearnUrl(String),

    // Settings changed — sync bool settings that affect other panels
    ShowHelpIconsChanged(bool),
    HelpOpensInTabChanged(bool),
    
    // Workshop Panel (System 0: Ideation)
    WorkshopSendMessage(String),
    WorkshopApproveMcp(i32),
    WorkshopSkipMcp(i32),
    WorkshopEditMcp(i32),
    WorkshopOpenArtifact(String),
    WorkshopStartPipeline,
    WorkshopPausePipeline,
    WorkshopResumePipeline,
    WorkshopCancelPipeline,
    WorkshopOptimizeAndBuild,
}

/// Shared action queue between Slint callbacks and Bevy systems
#[derive(Resource, Clone)]
pub struct SlintActionQueue(pub Arc<Mutex<Vec<SlintAction>>>);

impl Default for SlintActionQueue {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }
}

impl SlintActionQueue {
    /// Push an action from a Slint callback
    pub fn push(&self, action: SlintAction) {
        if let Ok(mut queue) = self.0.lock() {
            queue.push(action);
        }
    }
    
    /// Drain all queued actions (called by Bevy system each frame)
    pub fn drain(&self) -> Vec<SlintAction> {
        if let Ok(mut queue) = self.0.lock() {
            queue.drain(..).collect()
        } else {
            Vec::new()
        }
    }
}

// ============================================================================
// UI State Resources
// ============================================================================

/// Global studio state - main UI state resource
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
    
    // Play mode controls
    pub play_solo_requested: bool,
    pub play_with_character_requested: bool,
    pub pause_requested: bool,
    pub stop_requested: bool,
    
    // Panel visibility
    pub mindspace_panel_visible: bool,
    pub secondary_panel_tab: SecondaryPanelTab,
    
    // Dialogs
    pub show_publish_dialog: bool,
    pub publish_as_new: bool,
    pub trigger_login: bool,
    
    // Paste mode
    pub pending_paste: bool,
    pub pending_file_action: Option<FileEvent>,
    
    // Network
    pub show_network_panel: bool,
    pub show_forge_connect_window: bool,
    pub show_stress_test_window: bool,
    pub synthetic_client_count: u32,
    pub synthetic_clients_changed: bool,
    
    // Data windows
    pub show_global_sources_window: bool,
    pub show_domains_window: bool,
    pub show_global_variables_window: bool,
    pub quick_add_source_type: Option<String>,
    
    // Sync domain modal
    pub show_sync_domain_modal: bool,
    pub sync_domain_config: SyncDomainModalState,
    
    // Ribbon
    pub ribbon_tab: RibbonTab,
    pub visible_tabs: Vec<TabEntry>,
    pub custom_tabs: Vec<CustomTab>,
    pub tab_manager: RibbonTabManagerState,
    
    // Browser
    pub browser_open_request: Option<(String, String)>,
    
    // Find/Settings
    pub show_find_dialog: bool,
    pub show_settings_window: bool,
    
    // Exit confirmation
    pub has_unsaved_changes: bool,
    pub show_exit_confirmation: bool,
    
    // MindSpace
    pub mindspace_mode: MindSpaceMode,
    pub mindspace_edit_buffer: String,
    pub mindspace_font: eustress_common::classes::Font,
    pub mindspace_font_size: f32,
    
    // Center tab management (Space1 + script/web tabs)
    pub center_tabs: Vec<CenterTabData>,
    pub active_center_tab: i32,          // 0 = Space1, 1+ = tab index
    pub tabs_dirty: bool,                // Set when CenterTabManager updates tabs
    pub pending_open_script: Option<(i32, String)>,
    pub pending_open_web: Option<String>, // URL to open in new web tab
    pub pending_close_tab: Option<i32>,
    pub pending_reorder: Option<(i32, i32)>, // (from, to)
    pub script_editor_content: String,
    pub script_content_dirty: bool,     // Set when user types; triggers line-number re-sync
    pub script_highlight_lines: Vec<HighlightLine>,
    
    // Web browser state for active web tab
    pub pending_web_navigate: Option<String>,
    pub pending_web_back: bool,
    pub pending_web_forward: bool,
    pub pending_web_refresh: bool,

    // Properties panel settings (from File > Settings)
    pub show_help_icons: bool,
    pub help_opens_in_tab: bool,
    
    // Properties panel — collapsed section categories
    pub collapsed_sections: std::collections::HashSet<String>,
    
    // Properties panel — hash of last pushed model to avoid flickering hover on redundant pushes
    pub last_properties_hash: u64,
    // Properties panel — last selected entity to detect selection changes
    pub last_selected_entity: Option<bevy::prelude::Entity>,
    // Properties panel — frame counter since last selection change (delays sync to avoid flicker during editing)
    pub frames_since_selection_change: u32,
    // Output console — last log count to avoid rebuilding model on every sync
    pub last_log_count: usize,
}

/// Data for a single center tab (script or web)
#[derive(Debug, Clone)]
pub struct CenterTabData {
    pub entity_id: i32,       // -1 for web tabs
    pub name: String,
    pub tab_type: String,     // "script" or "web"
    pub mode: String,         // SoulScript mode: "summary" | "code" | ""
    pub url: String,          // URL for web tabs
    pub dirty: bool,
    pub loading: bool,
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
                TabEntry::BuiltIn { name: "Home".to_string() },
                TabEntry::BuiltIn { name: "Model".to_string() },
                TabEntry::BuiltIn { name: "Test".to_string() },
                TabEntry::BuiltIn { name: "View".to_string() },
                TabEntry::BuiltIn { name: "Plugins".to_string() },
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
            center_tabs: Vec::new(),
            active_center_tab: 0,
            tabs_dirty: false,
            pending_open_script: None,
            pending_open_web: None,
            pending_close_tab: None,
            pending_reorder: None,
            script_editor_content: String::new(),
            script_content_dirty: false,
            script_highlight_lines: Vec::new(),
            pending_web_navigate: None,
            pending_web_back: false,
            pending_web_forward: false,
            pending_web_refresh: false,
            show_help_icons: true,
            help_opens_in_tab: false,
            collapsed_sections: std::collections::HashSet::new(),
            last_properties_hash: 0,
            last_selected_entity: None,
            frames_since_selection_change: 0,
            last_log_count: 0,
        }
    }
}

/// Output console for logs
#[derive(Resource, Default)]
pub struct OutputConsole {
    pub entries: Vec<LogEntry>,
    pub max_entries: usize,
    pub auto_scroll: bool,
    pub filter_level: LogLevel,
}

impl OutputConsole {
    pub fn info(&mut self, msg: impl Into<String>) {
        self.push(LogLevel::Info, msg.into());
    }
    
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.push(LogLevel::Warn, msg.into());
    }
    
    pub fn warning(&mut self, msg: impl Into<String>) {
        self.push(LogLevel::Warn, msg.into());
    }
    
    pub fn error(&mut self, msg: impl Into<String>) {
        self.push(LogLevel::Error, msg.into());
    }
    
    pub fn debug(&mut self, msg: impl Into<String>) {
        self.push(LogLevel::Debug, msg.into());
    }
    
    fn push(&mut self, level: LogLevel, message: String) {
        let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
        self.entries.push(LogEntry { level, message, timestamp });
        
        // Trim old entries
        let max = if self.max_entries > 0 { self.max_entries } else { 1000 };
        while self.entries.len() > max {
            self.entries.remove(0);
        }
    }
    
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Log entry
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub timestamp: String,
}

/// Log level
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LogLevel {
    #[default]
    Info,
    Warn,
    Error,
    Debug,
}

/// Command bar state
#[derive(Resource, Default)]
pub struct CommandBarState {
    pub input: String,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub is_focused: bool,
    pub show: bool,
}

/// Collaboration state
#[derive(Resource, Default)]
pub struct CollaborationState {
    pub connected: bool,
    pub users: Vec<CollaborationUser>,
    pub room_id: Option<String>,
}

/// Collaboration user
#[derive(Clone, Debug)]
pub struct CollaborationUser {
    pub id: String,
    pub name: String,
    pub color: bevy::color::Color,
    pub cursor_position: Option<Vec3>,
}

/// Toolbox state
#[derive(Resource, Default)]
pub struct ToolboxState {
    pub expanded_categories: std::collections::HashSet<String>,
    pub search_query: String,
}

/// Studio dock state
#[derive(Resource, Default)]
pub struct StudioDockState {
    pub left_width: f32,
    pub right_width: f32,
    pub bottom_height: f32,
}

/// Unified explorer state combining ECS entities and filesystem
#[derive(Resource)]
pub struct UnifiedExplorerState {
    /// Currently selected item (entity or file)
    pub selected: SelectedItem,
    /// Set of expanded entity IDs
    pub expanded_entities: std::collections::HashSet<Entity>,
    /// Set of expanded directory paths
    pub expanded_dirs: std::collections::HashSet<std::path::PathBuf>,
    /// Search query for filtering
    pub search_query: String,
    /// Space root directory — the filesystem scope for the Explorer.
    /// Defaults to the user's Documents folder. All file browsing is
    /// relative to this path so the Explorer doesn't show the entire OS.
    pub project_root: std::path::PathBuf,
    /// Cached filesystem tree
    pub file_cache: FileTreeCache,
    /// Whether cache needs refresh
    pub dirty: bool,
    /// Reverse lookup: hash ID → file path (populated during sync)
    pub file_path_cache: std::collections::HashMap<i32, std::path::PathBuf>,
    /// Stable i32 → Entity lookup. Bevy entity.to_bits() is u64 and cannot be
    /// safely truncated to i32 (Slint's int). We assign sequential positive IDs
    /// (starting at 1) each sync and store the mapping here for OpenNode/SelectNode.
    pub entity_id_cache: std::collections::HashMap<i32, Entity>,
    /// Counter for assigning the next sequential entity node ID.
    next_entity_node_id: i32,
    /// Set of service names that are currently expanded.
    /// Services start expanded by default (populated on first sync).
    pub expanded_services: std::collections::HashSet<String>,
    /// When true, the next sync_unified_explorer_to_slint call bypasses the
    /// 30-frame throttle so selection/expand changes feel instant.
    pub needs_immediate_sync: bool,
    /// Hash of last pushed tree model to avoid flickering on redundant pushes
    pub last_tree_hash: u64,
}

fn default_space_root() -> std::path::PathBuf {
    crate::space::default_space_root()
}

impl Default for UnifiedExplorerState {
    fn default() -> Self {
        Self {
            selected: SelectedItem::None,
            expanded_entities: std::collections::HashSet::new(),
            entity_id_cache: std::collections::HashMap::new(),
            next_entity_node_id: 1,
            expanded_services: ["Workspace"].iter().map(|s| s.to_string()).collect(),
            expanded_dirs: std::collections::HashSet::new(),
            search_query: String::new(),
            project_root: default_space_root(),
            file_cache: FileTreeCache::default(),
            dirty: true,
            file_path_cache: std::collections::HashMap::new(),
            needs_immediate_sync: false,
            last_tree_hash: 0,
        }
    }
}

/// Selected item in unified explorer
#[derive(Debug, Clone, PartialEq)]
pub enum SelectedItem {
    Entity(Entity),
    File(std::path::PathBuf),
    /// Service header node (Workspace, Lighting, etc.) — selected by name
    Service(String),
    None,
}

/// Cached directory tree for efficient Slint sync
pub struct FileTreeCache {
    pub nodes: Vec<FileNodeData>,
    pub last_scan: std::time::Instant,
}

/// File node data for caching
pub struct FileNodeData {
    pub path: std::path::PathBuf,
    pub name: String,
    pub is_directory: bool,
    pub extension: String,
    pub size: u64,
    pub modified: std::time::SystemTime,
}

impl Default for FileTreeCache {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            last_scan: std::time::Instant::now(),
        }
    }
}

/// Explorer toggle event
#[derive(bevy::ecs::message::Message)]
pub struct ExplorerToggleEvent {
    pub entity: Entity,
}

/// Explorer cache
#[derive(Resource, Default)]
pub struct ExplorerCache {
    pub entities: Vec<Entity>,
    pub dirty: bool,
}

// ============================================================================
// Stub functions for compatibility
// ============================================================================

/// Capture bevy logs (stub - Slint handles this differently)
pub fn capture_bevy_logs(_console: ResMut<OutputConsole>) {}

/// Push to log buffer
pub fn push_to_log_buffer(_msg: &str) {}

/// Parse and push log
pub fn parse_and_push_log(_msg: &str) {}

/// Handle explorer toggle
pub fn handle_explorer_toggle(
    mut events: MessageReader<ExplorerToggleEvent>,
    mut state: ResMut<UnifiedExplorerState>,
) {
    for event in events.read() {
        if state.expanded_entities.contains(&event.entity) {
            state.expanded_entities.remove(&event.entity);
        } else {
            state.expanded_entities.insert(event.entity);
        }
    }
}

/// Handle window close request
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

// ============================================================================
// Performance Tracking
// ============================================================================

/// Resource to track UI performance metrics
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
            frame_times: Vec::with_capacity(60),
            fps: 60.0,
            avg_frame_time_ms: 16.67,
            ui_budget_ms: 8.0,
            last_ui_time_ms: 0.0,
            skip_heavy_updates: false,
            frame_counter: 0,
        }
    }
}

impl UIPerformance {
    pub fn update(&mut self, delta_secs: f32) {
        let frame_time_ms = delta_secs * 1000.0;
        self.frame_times.push(frame_time_ms);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }
        if !self.frame_times.is_empty() {
            self.avg_frame_time_ms = self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
            self.fps = 1000.0 / self.avg_frame_time_ms;
        }
        self.skip_heavy_updates = self.last_ui_time_ms > self.ui_budget_ms;
        self.frame_counter += 1;
    }
    
    pub fn should_throttle(&self, interval: u64) -> bool {
        self.frame_counter % interval != 0
    }
    
    pub fn record_ui_time(&mut self, time_ms: f32) {
        self.last_ui_time_ms = time_ms;
    }
}

// ============================================================================
// StudioUiPlugin (Legacy - use SlintUiPlugin instead)
// ============================================================================

/// Main Studio UI Plugin - Slint-only version
pub struct StudioUiPlugin {
    pub selection_manager: Arc<RwLock<SelectionManager>>,
    pub transform_manager: Arc<RwLock<TransformManager>>,
}

impl Plugin for StudioUiPlugin {
    fn build(&self, app: &mut App) {
        info!("StudioUiPlugin: Initializing Slint-only UI");
        
        app
            // Manager resources
            .insert_resource(BevySelectionManager(self.selection_manager.clone()))
            .insert_resource(BevyTransformManager(self.transform_manager.clone()))
            // UI state resources
            .init_resource::<StudioState>()
            .init_resource::<OutputConsole>()
            .init_resource::<CommandBarState>()
            .init_resource::<CollaborationState>()
            .init_resource::<ToolboxState>()
            .init_resource::<StudioDockState>()
            .init_resource::<UnifiedExplorerState>()
            .init_resource::<ExplorerCache>()
            .init_resource::<UIPerformance>()
            .init_resource::<SceneFile>()
            .init_resource::<crate::auth::AuthState>()
            .init_resource::<crate::soul::SoulServiceSettings>()
            .init_resource::<crate::commands::CommandHistory>()
            // Events
            .add_message::<FileEvent>()
            .add_message::<MenuActionEvent>()
            .add_message::<ExplorerToggleEvent>()
            .add_message::<crate::commands::UndoCommandEvent>()
            .add_message::<crate::commands::RedoCommandEvent>()
            // Plugins
            .add_plugins(SpawnEventsPlugin)
            .add_plugins(WorldViewPlugin)
            .add_plugins(super::floating_windows::FloatingWindowsPlugin)
            // Systems
            .init_resource::<super::file_event_handler::PendingFileActions>()
            .add_systems(Update, handle_window_close_request)
            .add_systems(Update, handle_explorer_toggle)
            .add_systems(Update, (
                super::file_event_handler::drain_file_events,
                super::file_event_handler::execute_file_actions,
            ).chain())
            .add_systems(Update, crate::auth::auth_poll_system)
            .add_systems(Startup, try_restore_auth_session);
    }
}

// ============================================================================
// Slint Software Renderer Implementation
// ============================================================================

/// Alias for SlintUiPlugin (simpler plugin that doesn't require managers)
pub struct SlintUiPlugin;

impl Plugin for SlintUiPlugin {
    fn build(&self, app: &mut App) {
        info!("SlintUiPlugin: Initializing Slint software renderer overlay");
        
        // CRITICAL: Set the Slint platform BEFORE creating any Slint components
        slint::platform::set_platform(Box::new(SlintBevyPlatform {})).unwrap();
        info!("✅ Slint platform set");
        
        app
            // UI state resources
            .init_resource::<StudioState>()
            .init_resource::<OutputConsole>()
            .init_resource::<CommandBarState>()
            .init_resource::<CollaborationState>()
            .insert_resource(BrushState::new())
            .init_resource::<ToolboxState>()
            .init_resource::<StudioDockState>()
            .init_resource::<UnifiedExplorerState>()
            .init_resource::<ExplorerCache>()
            .init_resource::<UIPerformance>()
            .init_resource::<SlintUIFocus>()
            .init_resource::<SceneFile>()
            .init_resource::<crate::auth::AuthState>()
            .init_resource::<crate::soul::SoulServiceSettings>()
            .init_resource::<crate::commands::CommandHistory>()
            .init_resource::<SlintCursorState>()
            .init_resource::<super::ViewportBounds>()
            .init_resource::<LastWindowSize>()
            .init_resource::<super::center_tabs::CenterTabManager>()
            .init_resource::<AssetManagerState>()
            // Events
            .add_message::<FileEvent>()
            .add_message::<MenuActionEvent>()
            .add_message::<ExplorerToggleEvent>()
            .add_message::<crate::commands::UndoCommandEvent>()
            .add_message::<crate::commands::RedoCommandEvent>()
            // Plugins
            .add_plugins(SpawnEventsPlugin)
            .add_plugins(WorldViewPlugin)
            .add_plugins(super::webview::WebViewPlugin)
            .add_plugins(super::monaco_bridge::MonacoBridgePlugin)
            // Slint software renderer overlay systems
            .add_systems(Startup, setup_slint_overlay)
            .add_systems(Update, forward_input_to_slint)
            .add_systems(Update, forward_keyboard_to_slint)
            .add_systems(Update, update_slint_ui_focus)
            .add_systems(Update, drain_slint_actions.in_set(SlintSystems::Drain))
            .add_systems(Update, sync_bevy_to_slint.after(SlintSystems::Drain))
            .add_systems(Update, render_slint_to_texture.after(sync_bevy_to_slint))
            // Window resize handling
            .add_systems(Update, handle_window_resize)
            // Performance tracking
            .add_systems(Update, update_ui_performance)
            // Bridge: viewport click → SelectionManager → UnifiedExplorerState (runs first)
            .add_systems(Update, sync_viewport_selection_to_explorer)
            // Unified explorer sync: entities + filesystem (throttled internally)
            .add_systems(Update, sync_unified_explorer_to_slint.after(sync_viewport_selection_to_explorer))
            // Properties sync (throttled internally)
            .add_systems(Update, sync_properties_to_slint.after(sync_viewport_selection_to_explorer))
            // Asset Manager sync: scan Universe + Space directories (throttled internally)
            .add_systems(Update, sync_asset_manager_to_slint)
            // Workshop Panel sync: IdeationPipeline → Slint (throttled internally)
            .add_systems(Update, sync_workshop_to_slint.after(SlintSystems::Drain))
            // Center tab sync: drain_slint_actions → CenterTabManager → StudioState → Slint
            .add_systems(Update, sync_tab_manager_to_studio_state
                .after(SlintSystems::Drain)
                .before(sync_center_tabs_to_slint))
            .add_systems(Update, sync_center_tabs_to_slint.after(SlintSystems::Drain))
            .init_resource::<super::file_event_handler::PendingFileActions>()
            // UI systems
            .add_systems(Update, handle_window_close_request)
            .add_systems(Update, handle_explorer_toggle)
            .add_systems(Update, (
                super::file_event_handler::drain_file_events,
                super::file_event_handler::execute_file_actions,
            ).chain())
            .add_systems(Update, crate::auth::auth_poll_system)
            .add_systems(Startup, try_restore_auth_session);
    }
}

/// Initialize Slint software renderer and create overlay (exclusive startup system)
fn setup_slint_overlay(world: &mut World) {
    // Get window dimensions in PHYSICAL pixels (must match framebuffer size)
    let (width, height, scale_factor) = {
        let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
        match windows.iter(world).next() {
            Some(w) => {
                let width = w.physical_width();
                let height = w.physical_height();
                if width == 0 || height == 0 {
                    warn!("Window has zero size, skipping Slint setup");
                    return;
                }
                (width, height, w.scale_factor())
            }
            None => {
                warn!("No primary window found for Slint overlay setup");
                return;
            }
        }
    };
    
    info!("🎨 Setting up Slint software renderer overlay ({}x{})", width, height);
    
    // Initialize Slint timers before creating component
    slint::platform::update_timers_and_animations();
    
    // Create the StudioWindow Slint component
    let ui = match StudioWindow::new() {
        Ok(ui) => {
            info!("✅ Slint StudioWindow created successfully");
            ui
        }
        Err(e) => {
            error!("❌ Failed to create Slint window: {}", e);
            return;
        }
    };
    
    ui.window().show().expect("Failed to show Slint window");
    
    // Retrieve the adapter from thread-local storage
    let adapter = SLINT_WINDOWS
        .with(|windows| windows.borrow().first().and_then(|w| w.upgrade()))
        .expect("Slint window adapter should be created when StudioWindow is initialized");
    
    // Notify Slint the window is active
    adapter.slint_window.dispatch_event(WindowEvent::WindowActiveChanged(true));
    adapter.resize(slint::PhysicalSize::new(width, height), scale_factor);
    
    // Set initial UI state
    ui.set_dark_theme(true);
    ui.set_show_explorer(true);
    ui.set_show_properties(true);
    ui.set_show_output(true);
    ui.set_show_toolbox(true);
    
    // ========================================================================
    // Wire Slint callbacks → SlintActionQueue
    // Each callback captures a clone of the Arc<Mutex<Vec<SlintAction>>> queue.
    // The drain_slint_actions system reads these each frame.
    // ========================================================================
    let queue = SlintActionQueue::default();
    
    // File operations
    let q = queue.clone();
    ui.on_new_universe(move || q.push(SlintAction::NewUniverse));
    let q = queue.clone();
    ui.on_new_scene(move || q.push(SlintAction::NewScene));
    let q = queue.clone();
    ui.on_open_scene(move || q.push(SlintAction::OpenScene));
    let q = queue.clone();
    ui.on_save_scene(move || q.push(SlintAction::SaveScene));
    let q = queue.clone();
    ui.on_save_scene_as(move || q.push(SlintAction::SaveSceneAs));
    let q = queue.clone();
    ui.on_open_publish_dialog(move || q.push(SlintAction::OpenPublishDialog));
    let q = queue.clone();
    ui.on_publish_as(move || q.push(SlintAction::OpenPublishAsDialog));
    let q = queue.clone();
    ui.on_publish(move |experience_name, description, genre, is_public, open_source, studio_editable, as_new| {
        q.push(SlintAction::Publish(PublishRequest {
            experience_name: experience_name.to_string(),
            description: description.to_string(),
            genre: genre.to_string(),
            is_public,
            open_source,
            studio_editable,
            as_new,
        }))
    });
    
    // Edit operations
    let q = queue.clone();
    ui.on_undo(move || q.push(SlintAction::Undo));
    let q = queue.clone();
    ui.on_redo(move || q.push(SlintAction::Redo));
    let q = queue.clone();
    ui.on_copy(move || q.push(SlintAction::Copy));
    let q = queue.clone();
    ui.on_cut(move || q.push(SlintAction::Cut));
    let q = queue.clone();
    ui.on_paste(move || q.push(SlintAction::Paste));
    let q = queue.clone();
    ui.on_delete_selected(move || q.push(SlintAction::Delete));
    let q = queue.clone();
    ui.on_duplicate(move || q.push(SlintAction::Duplicate));
    let q = queue.clone();
    ui.on_select_all(move || q.push(SlintAction::SelectAll));
    
    // Tool selection
    let q = queue.clone();
    ui.on_select_tool(move |tool| q.push(SlintAction::SelectTool(tool.to_string())));
    
    // Transform mode
    let q = queue.clone();
    ui.on_set_transform_mode(move |mode| q.push(SlintAction::SetTransformMode(mode.to_string())));
    let q = queue.clone();
    ui.on_toggle_snap(move || q.push(SlintAction::ToggleSnap));
    let q = queue.clone();
    ui.on_set_snap_increment(move |val| q.push(SlintAction::SetSnapIncrement(val)));
    
    // View
    let q = queue.clone();
    ui.on_set_view_mode(move |mode| q.push(SlintAction::SetViewMode(mode.to_string())));
    let q = queue.clone();
    ui.on_focus_selected(move || q.push(SlintAction::FocusSelected));
    let q = queue.clone();
    ui.on_toggle_wireframe(move || q.push(SlintAction::ToggleWireframe));
    let q = queue.clone();
    ui.on_toggle_grid(move || q.push(SlintAction::ToggleGrid));
    
    // Play controls
    let q = queue.clone();
    ui.on_play_solo(move || q.push(SlintAction::PlaySolo));
    let q = queue.clone();
    ui.on_play_with_character(move || q.push(SlintAction::PlayWithCharacter));
    let q = queue.clone();
    ui.on_pause(move || q.push(SlintAction::Pause));
    let q = queue.clone();
    ui.on_stop(move || q.push(SlintAction::Stop));

    // Simulation settings — Apply is automatic on Save (no separate Apply callback)
    let q = queue.clone();
    ui.on_save_simulation_settings(move || q.push(SlintAction::SaveSimulationSettings));
    let q = queue.clone();
    ui.on_add_sim_watchpoint(move || q.push(SlintAction::AddSimWatchpoint));
    let q = queue.clone();
    ui.on_remove_sim_watchpoint(move |i| q.push(SlintAction::RemoveSimWatchpoint(i)));
    let q = queue.clone();
    ui.on_add_sim_output_binding(move || q.push(SlintAction::AddSimOutputBinding));
    let q = queue.clone();
    ui.on_remove_sim_output_binding(move |i| q.push(SlintAction::RemoveSimOutputBinding(i)));

    // Explorer (unified: entities + files)
    let q = queue.clone();
    ui.on_select_node(move |id, node_type| q.push(SlintAction::SelectNode(id, node_type.to_string())));
    let q = queue.clone();
    ui.on_expand_node(move |id, node_type| q.push(SlintAction::ExpandNode(id, node_type.to_string())));
    let q = queue.clone();
    ui.on_collapse_node(move |id, node_type| q.push(SlintAction::CollapseNode(id, node_type.to_string())));
    let q = queue.clone();
    ui.on_open_node(move |id, node_type| q.push(SlintAction::OpenNode(id, node_type.to_string())));
    let q = queue.clone();
    ui.on_rename_node(move |id, name, node_type| q.push(SlintAction::RenameNode(id, name.to_string(), node_type.to_string())));
    let q = queue.clone();
    ui.on_add_service(move || q.push(SlintAction::AddService));
    let q = queue.clone();
    ui.on_expand_all(move || q.push(SlintAction::ExpandAll));
    let q = queue.clone();
    ui.on_collapse_all(move || q.push(SlintAction::CollapseAll));
    
    // Properties
    let q = queue.clone();
    ui.on_property_changed(move |key, val| q.push(SlintAction::PropertyChanged(key.to_string(), val.to_string())));
    let q = queue.clone();
    ui.on_section_toggle(move |category| q.push(SlintAction::SectionToggle(category.to_string())));

    // Help icon — open /learn URL (tabbed viewer or system browser per settings)
    let q = queue.clone();
    ui.on_open_learn_url(move |url| q.push(SlintAction::OpenLearnUrl(url.to_string())));

    // Help icon settings changed from Settings dialog — push action so StudioState updates
    let q = queue.clone();
    ui.on_help_icons_changed(move |val| q.push(SlintAction::ShowHelpIconsChanged(val)));
    let q = queue.clone();
    ui.on_help_opens_in_tab_changed(move |val| q.push(SlintAction::HelpOpensInTabChanged(val)));
    
    // Command bar
    let q = queue.clone();
    ui.on_execute_command(move |cmd| q.push(SlintAction::ExecuteCommand(cmd.to_string())));
    
    // Toolbox part insertion
    let q = queue.clone();
    ui.on_insert_part(move |part_type| q.push(SlintAction::InsertPart(part_type.to_string())));

    // Ribbon menu actions (Insert dropdown, Model menu, etc.)
    let q = queue.clone();
    ui.on_menu_action(move |action| q.push(SlintAction::MenuAction(action.to_string())));
    
    // Context menu
    let q = queue.clone();
    ui.on_context_action(move |action| q.push(SlintAction::ContextAction(action.to_string())));
    
    // Terrain
    let q = queue.clone();
    ui.on_generate_terrain(move |size| q.push(SlintAction::GenerateTerrain(size.to_string())));
    let q = queue.clone();
    ui.on_toggle_terrain_edit_mode(move || q.push(SlintAction::ToggleTerrainEditMode));
    let q = queue.clone();
    ui.on_set_terrain_brush(move |brush| q.push(SlintAction::SetTerrainBrush(brush.to_string())));
    // TODO: Uncomment after Slint regenerates bindings for brush settings callbacks
    // let q = queue.clone();
    // ui.on_brush_size_changed(move |size| q.push(SlintAction::BrushSizeChanged(size)));
    // let q = queue.clone();
    // ui.on_brush_strength_changed(move |strength| q.push(SlintAction::BrushStrengthChanged(strength)));
    // let q = queue.clone();
    // ui.on_brush_falloff_changed(move |falloff| q.push(SlintAction::BrushFalloffChanged(falloff.to_string())));
    let q = queue.clone();
    ui.on_import_heightmap(move || q.push(SlintAction::ImportHeightmap));
    let q = queue.clone();
    ui.on_export_heightmap(move || q.push(SlintAction::ExportHeightmap));
    
    // Asset Manager
    let q = queue.clone();
    ui.on_asset_select(move |id| q.push(SlintAction::AssetSelect(id)));
    let q = queue.clone();
    ui.on_asset_expand(move |id| q.push(SlintAction::AssetExpand(id)));
    let q = queue.clone();
    ui.on_asset_import(move |kind| q.push(SlintAction::AssetImport(kind.to_string())));
    let q = queue.clone();
    ui.on_asset_search(move |text| q.push(SlintAction::AssetSearch(text.to_string())));
    let q = queue.clone();
    ui.on_asset_category_changed(move |cat| q.push(SlintAction::AssetCategoryChanged(cat.to_string())));

    // Network
    let q = queue.clone();
    ui.on_start_server(move || q.push(SlintAction::StartServer));
    let q = queue.clone();
    ui.on_stop_server(move || q.push(SlintAction::StopServer));
    let q = queue.clone();
    ui.on_connect_forge(move || q.push(SlintAction::ConnectForge));
    let q = queue.clone();
    ui.on_disconnect_forge(move || q.push(SlintAction::DisconnectForge));
    let q = queue.clone();
    ui.on_allocate_forge_server(move || q.push(SlintAction::AllocateForgeServer));
    let q = queue.clone();
    ui.on_spawn_synthetic_clients(move |count| q.push(SlintAction::SpawnSyntheticClients(count)));
    let q = queue.clone();
    ui.on_disconnect_all_clients(move || q.push(SlintAction::DisconnectAllClients));
    
    // Data
    let q = queue.clone();
    ui.on_open_global_sources(move || q.push(SlintAction::OpenGlobalSources));
    let q = queue.clone();
    ui.on_open_domains(move || q.push(SlintAction::OpenDomains));
    let q = queue.clone();
    ui.on_open_global_variables(move || q.push(SlintAction::OpenGlobalVariables));
    
    // MindSpace
    let q = queue.clone();
    ui.on_toggle_mindspace(move || q.push(SlintAction::ToggleMindspace));
    let q = queue.clone();
    ui.on_mindspace_add_label(move || q.push(SlintAction::MindspaceAddLabel));
    let q = queue.clone();
    ui.on_mindspace_connect(move || q.push(SlintAction::MindspaceConnect));
    
    // Auth
    let q = queue.clone();
    ui.on_login(move || q.push(SlintAction::Login));
    let q = queue.clone();
    ui.on_logout(move || q.push(SlintAction::Logout));
    
    // Scripts
    let q = queue.clone();
    ui.on_build_script(move |id| q.push(SlintAction::BuildScript(id)));
    let q = queue.clone();
    ui.on_open_script(move |id| q.push(SlintAction::OpenScript(id)));
    
    // Center tab management
    let q = queue.clone();
    ui.on_close_center_tab(move |idx| q.push(SlintAction::CloseCenterTab(idx)));
    let q = queue.clone();
    ui.on_select_center_tab(move |idx| q.push(SlintAction::SelectCenterTab(idx)));
    let q = queue.clone();
    ui.on_script_content_changed(move |text| q.push(SlintAction::ScriptContentChanged(text.to_string())));
    let q = queue.clone();
    ui.on_reorder_center_tab(move |from, to| q.push(SlintAction::ReorderCenterTab(from, to)));
    let q = queue.clone();
    ui.on_toggle_mode(move |idx| q.push(SlintAction::ToggleTabMode(idx)));
    
    // Web browser
    let q = queue.clone();
    ui.on_open_web_tab(move |url| q.push(SlintAction::OpenWebTab(url.to_string())));
    let q = queue.clone();
    ui.on_web_navigate(move |url| q.push(SlintAction::WebNavigate(url.to_string())));
    let q = queue.clone();
    ui.on_web_go_back(move || q.push(SlintAction::WebGoBack));
    let q = queue.clone();
    ui.on_web_go_forward(move || q.push(SlintAction::WebGoForward));
    let q = queue.clone();
    ui.on_web_refresh(move || q.push(SlintAction::WebRefresh));
    
    // Settings
    let q = queue.clone();
    ui.on_open_settings(move || q.push(SlintAction::ShowSettings));
    let q = queue.clone();
    ui.on_open_find(move || q.push(SlintAction::ShowFind));
    
    // Layout
    let q = queue.clone();
    ui.on_apply_layout_preset(move |preset| q.push(SlintAction::ApplyLayoutPreset(preset)));
    let q = queue.clone();
    ui.on_save_layout_to_file(move || q.push(SlintAction::SaveLayoutToFile));
    let q = queue.clone();
    ui.on_load_layout_from_file(move || q.push(SlintAction::LoadLayoutFromFile));
    let q = queue.clone();
    ui.on_reset_layout_to_default(move || q.push(SlintAction::ResetLayoutToDefault));
    let q = queue.clone();
    ui.on_toggle_theme_editor(move || q.push(SlintAction::ToggleThemeEditor));
    let q = queue.clone();
    ui.on_apply_theme_settings(move |dark, hc, scale| q.push(SlintAction::ApplyThemeSettings(dark, hc, scale)));
    let q = queue.clone();
    ui.on_detach_panel_to_window(move |panel| q.push(SlintAction::DetachPanelToWindow(panel.to_string())));
    
    // Viewport bounds
    let q = queue.clone();
    ui.on_viewport_bounds_changed(move |x, y, w, h| q.push(SlintAction::ViewportBoundsChanged(x, y, w, h)));
    
    // Close
    let q = queue.clone();
    ui.on_close_requested(move || q.push(SlintAction::CloseRequested));
    
    // Force exit (after exit confirmation dialog)
    let q = queue.clone();
    ui.on_force_exit(move || q.push(SlintAction::ForceExit));
    
    // Workshop Panel (System 0: Ideation)
    let q = queue.clone();
    ui.on_workshop_send_message(move |text| q.push(SlintAction::WorkshopSendMessage(text.to_string())));
    let q = queue.clone();
    ui.on_workshop_approve_mcp(move |id| q.push(SlintAction::WorkshopApproveMcp(id)));
    let q = queue.clone();
    ui.on_workshop_skip_mcp(move |id| q.push(SlintAction::WorkshopSkipMcp(id)));
    let q = queue.clone();
    ui.on_workshop_edit_mcp(move |id| q.push(SlintAction::WorkshopEditMcp(id)));
    let q = queue.clone();
    ui.on_workshop_open_artifact(move |path| q.push(SlintAction::WorkshopOpenArtifact(path.to_string())));
    let q = queue.clone();
    ui.on_workshop_start_pipeline(move || q.push(SlintAction::WorkshopStartPipeline));
    let q = queue.clone();
    ui.on_workshop_pause_pipeline(move || q.push(SlintAction::WorkshopPausePipeline));
    let q = queue.clone();
    ui.on_workshop_resume_pipeline(move || q.push(SlintAction::WorkshopResumePipeline));
    let q = queue.clone();
    ui.on_workshop_cancel_pipeline(move || q.push(SlintAction::WorkshopCancelPipeline));
    let q = queue.clone();
    ui.on_workshop_optimize_and_build(move || q.push(SlintAction::WorkshopOptimizeAndBuild));
    
    // Store queue as Bevy resource
    world.insert_resource(queue);
    
    info!("✅ Slint StudioWindow configured with {} callbacks wired", 60);
    
    // Create Bevy texture for Slint to render into.
    // Uses Rgba8UnormSrgb for correct sRGB gamma (prevents washed-out colors).
    // Slint's SoftwareRenderer outputs PremultipliedRgbaColor in sRGB space.
    // CRITICAL: asset_usage must include MAIN_WORLD so Bevy keeps the CPU-side
    // data buffer alive every frame. Without it image.data is None after the first
    // GPU upload and Slint has nowhere to write its rendered pixels.
    let size = Extent3d { width, height, depth_or_array_layers: 1 };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("SlintOverlay"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        asset_usage: bevy::asset::RenderAssetUsages::MAIN_WORLD
            | bevy::asset::RenderAssetUsages::RENDER_WORLD,
        ..default()
    };
    image.resize(size);
    
    let image_handle = world.resource_mut::<Assets<Image>>().add(image);
    
    // Create unlit material with premultiplied alpha blending.
    // Slint outputs premultiplied RGBA, so we must use PremultipliedAlpha
    // (not Blend, which assumes straight alpha and causes double-blending/washout).
    let material_handle = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Premultiplied,
        cull_mode: None,
        ..default()
    });
    
    // Create fullscreen quad mesh
    let quad_mesh = world.resource_mut::<Assets<Mesh>>().add(Rectangle::new(width as f32, height as f32));
    
    // Track the scene for the render system's materials.get_mut() workaround
    world.spawn(SlintScene { image: image_handle.clone(), material: material_handle.clone() });
    
    // Use RenderLayers to isolate the overlay from the main 3D scene
    let overlay_layer = RenderLayers::layer(31);
    
    // Spawn overlay camera: orthographic Camera3d (NOT Camera2d — Camera2d uses a separate
    // 2D pipeline that doesn't render Mesh3d/MeshMaterial3d entities).
    // Camera3d with orthographic projection renders on top of the main scene.
    // SkyboxAttached prevents SharedLightingPlugin from attaching a skybox to this camera,
    // which would paint over the entire 3D scene since this camera renders at order=100.
    world.spawn((
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            near: -1.0,
            far: 10.0,
            scaling_mode: ScalingMode::Fixed {
                width: width as f32,
                height: height as f32,
            },
            ..OrthographicProjection::default_3d()
        }),
        Camera {
            order: 100,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        overlay_layer.clone(),
        SlintOverlayCamera,
        eustress_common::plugins::lighting_plugin::SkyboxAttached,
        Name::new("Slint Overlay Camera"),
    ));
    
    // Spawn fullscreen quad with the Slint texture material
    world.spawn((
        Mesh3d(quad_mesh),
        MeshMaterial3d(material_handle),
        Transform::from_xyz(0.0, 0.0, 0.0),
        overlay_layer,
        SlintOverlaySprite,
        Name::new("Slint Overlay Quad"),
    ));
    
    // Store Slint state as NonSend resource (requires World access)
    world.insert_non_send_resource(SlintUiState {
        window: ui,
        adapter,
    });
    
    world.insert_resource(SlintOverlayTexture(image_handle));
    world.insert_resource(SlintStagingBuffer {
        pixels: vec![PremultipliedRgbaColor::default(); (width * height) as usize],
        width: width as usize,
        height: height as usize,
    });
    world.insert_resource(SlintOverlayInitialized(true));

    // Set the window title-bar icon via WinitWindows (available here at Startup)
    set_window_icon(world);

    info!("✅ Slint overlay setup complete ({}x{}, scale={})", width, height, scale_factor);
}

/// Sets the window icon using the WINIT_WINDOWS thread-local (Bevy 0.18 stores
/// WinitWindows in a thread-local, not as a NonSend ECS resource).
/// 
/// **macOS Note:** This function does nothing on macOS because winit's `set_window_icon()`
/// is not supported on that platform. On macOS, the application icon must be bundled in
/// the `.app` package as `icon.icns` and referenced in `Info.plist`. The build script
/// generates `assets/icon.icns` from the SVG, which should be copied to
/// `YourApp.app/Contents/Resources/icon.icns` when packaging for distribution.
fn set_window_icon(world: &mut World) {
    // macOS doesn't support runtime window icon setting via winit
    #[cfg(target_os = "macos")]
    {
        info!("ℹ️  set_window_icon: skipped on macOS (icon must be bundled in .app package)");
        return;
    }
    
    #[cfg(not(target_os = "macos"))]
    {
        use bevy::window::PrimaryWindow;
        use bevy::winit::WINIT_WINDOWS;
        use winit::window::Icon;

        // Resolve icon.png path (256x256 RGBA generated by build.rs from SVG)
        let candidates = [
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("assets/icon.png"))),
            Some(std::path::PathBuf::from("crates/engine/assets/icon.png")),
            Some(std::path::PathBuf::from("assets/icon.png")),
        ];
        let Some(icon_path) = candidates.into_iter().flatten().find(|p| p.exists()) else {
            warn!("set_window_icon: icon.png not found");
            return;
        };

        let icon = match image::open(&icon_path) {
            Ok(img) => {
                let img = img.to_rgba8();
                let (w, h) = img.dimensions();
                match Icon::from_rgba(img.into_raw(), w, h) {
                    Ok(i) => i,
                    Err(e) => { warn!("set_window_icon: Icon::from_rgba failed: {}", e); return; }
                }
            }
            Err(e) => { warn!("set_window_icon: failed to open {:?}: {}", icon_path, e); return; }
        };

        // Get the primary window entity
        let mut q = world.query_filtered::<Entity, With<PrimaryWindow>>();
        let Some(entity) = q.iter(world).next() else {
            warn!("set_window_icon: no PrimaryWindow entity");
            return;
        };

        // Access WinitWindows via the Bevy 0.18 thread-local (not a NonSend ECS resource)
        WINIT_WINDOWS.with_borrow(|winit_windows| {
            let Some(window_id) = winit_windows.entity_to_winit.get(&entity) else {
                warn!("set_window_icon: entity not in WINIT_WINDOWS.entity_to_winit");
                return;
            };
            let Some(winit_window) = winit_windows.windows.get(window_id) else {
                warn!("set_window_icon: window_id not in WINIT_WINDOWS.windows");
                return;
            };
            winit_window.set_window_icon(Some(icon.clone()));
            info!("✅ set_window_icon: icon set from {:?}", icon_path);
        });
    }
}

/// Resource holding the overlay texture handle
#[derive(Resource)]
pub struct SlintOverlayTexture(pub Handle<Image>);

/// Staging buffer for Slint software renderer output.
/// Slint renders into this buffer every frame. Only when dirty regions exist
/// do we copy the affected rows into the Bevy Image and trigger a GPU re-upload.
/// This avoids calling images.get_mut() every frame (which marks the entire
/// texture asset as modified and forces a full ~8MB GPU upload even when
/// nothing changed).
#[derive(Resource)]
struct SlintStagingBuffer {
    /// Pixel data — same layout as the Bevy texture (width * height * 4 bytes RGBA)
    pixels: Vec<PremultipliedRgbaColor>,
    /// Current buffer dimensions
    width: usize,
    height: usize,
}

/// Tracks cursor position for Slint input forwarding
#[derive(Resource, Default)]
struct SlintCursorState {
    position: Option<LogicalPosition>,
}

/// Frame counter for one-time debug logging
static RENDER_FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Renders the Slint UI to a staging buffer, then copies only dirty regions
/// into the Bevy texture for GPU upload.
///
/// **Performance**: Slint renders into `SlintStagingBuffer` (no Bevy asset mutation).
/// Only when dirty pixels exist do we call `images.get_mut()` and copy the affected
/// rows. This avoids marking the ~8MB texture asset as modified every frame, which
/// previously forced a full GPU re-upload even when nothing changed.
fn render_slint_to_texture(
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    slint_scenes: Query<&SlintScene>,
    slint_context: Option<NonSend<SlintUiState>>,
    mut staging: ResMut<SlintStagingBuffer>,
) {
    let Some(slint_context) = slint_context else { return };
    
    let frame = RENDER_FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    
    // Update Slint timers and animations every frame (needed for animations/transitions)
    slint::platform::update_timers_and_animations();
    
    let adapter = &slint_context.adapter;
    
    let Some(scene) = slint_scenes.iter().next() else {
        if frame < 5 { warn!("render_slint_to_texture: no SlintScene entity"); }
        return;
    };
    
    // Read texture dimensions WITHOUT mutating the image asset.
    // images.get() does NOT mark the asset as changed — no GPU re-upload triggered.
    let (tex_width, tex_height) = {
        let Some(image) = images.get(&scene.image) else {
            if frame < 5 { warn!("render_slint_to_texture: image asset not found"); }
            return;
        };
        (image.texture_descriptor.size.width as usize,
         image.texture_descriptor.size.height as usize)
    };
    
    // Ensure staging buffer matches texture dimensions
    let needed = tex_width * tex_height;
    if staging.pixels.len() != needed || staging.width != tex_width || staging.height != tex_height {
        staging.pixels.resize(needed, PremultipliedRgbaColor::default());
        staging.width = tex_width;
        staging.height = tex_height;
    }
    
    // Render Slint UI into the staging buffer (NOT into image.data).
    // ReusedBuffer mode: Slint only repaints dirty regions within the buffer.
    let dirty_region = adapter.software_renderer.render(
        &mut staging.pixels,
        tex_width,
    );
    
    // Only touch the Bevy image when Slint actually repainted something.
    // This is the critical optimization — when nothing is dirty (e.g. static UI,
    // no hover, no animation), we skip the ~8MB GPU texture upload entirely.
    let dirty_size = dirty_region.bounding_box_size();
    if dirty_size.width == 0 || dirty_size.height == 0 {
        return;
    }
    
    // Dirty region exists — copy affected rows from staging into image.data
    let dirty_origin = dirty_region.bounding_box_origin();
    let dirty_x = dirty_origin.x as usize;
    let dirty_y = dirty_origin.y as usize;
    let dirty_w = dirty_size.width as usize;
    let dirty_h = dirty_size.height as usize;
    
    let Some(image) = images.get_mut(&scene.image) else { return };
    if let Some(data) = image.data.as_mut() {
        let expected_bytes = tex_width * tex_height * 4;
        if data.len() != expected_bytes {
            // Buffer and descriptor are out of sync (resize in flight) — skip
            return;
        }
        
        // Copy only the dirty rectangle's rows from staging → image.data.
        // Each pixel is 4 bytes (PremultipliedRgbaColor = RGBA u8×4).
        let staging_bytes: &[u8] = bytemuck::cast_slice(&staging.pixels);
        let row_bytes = tex_width * 4;
        let dirty_row_start = dirty_x * 4;
        let dirty_row_len = dirty_w * 4;
        
        for row in dirty_y..(dirty_y + dirty_h).min(tex_height) {
            let offset = row * row_bytes + dirty_row_start;
            let end = offset + dirty_row_len;
            if end <= data.len() && end <= staging_bytes.len() {
                data[offset..end].copy_from_slice(&staging_bytes[offset..end]);
            }
        }
    }
    
    // WORKAROUND: Force GPU texture re-upload by touching the material mutably.
    // See: https://github.com/bevyengine/bevy/issues/17350
    materials.get_mut(&scene.material);
}

/// Forwards Bevy mouse/keyboard input to Slint (from official bevy-hosts-slint)
fn forward_input_to_slint(
    mut mouse_button: MessageReader<MouseButtonInput>,
    mut mouse_wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cursor_state: ResMut<SlintCursorState>,
    slint_context: Option<NonSend<SlintUiState>>,
) {
    let Some(slint_context) = slint_context else { return };
    let adapter = &slint_context.adapter;

    let Some(window) = windows.iter().next() else { return };
    let scale_factor = adapter.scale_factor.get();

    // Forward cursor position. When cursor leaves the window:
    // - Only send PointerExited when NO button is held (avoids cancelling scroll drag).
    // - When a button is held (dragging), keep the last known position so Slint
    //   continues tracking the drag until the button is released.
    let any_button_held = mouse_buttons.pressed(MouseButton::Left)
        || mouse_buttons.pressed(MouseButton::Right)
        || mouse_buttons.pressed(MouseButton::Middle);

    if let Some(cursor_pos) = window.cursor_position() {
        let position = LogicalPosition::new(
            cursor_pos.x / scale_factor,
            cursor_pos.y / scale_factor,
        );
        // Only dispatch PointerMoved when position actually changes.
        // Sending it every frame causes Slint to re-evaluate hover states and
        // mark dirty regions continuously, triggering unnecessary software renderer repaints.
        let moved = match cursor_state.position {
            Some(prev) => (prev.x - position.x).abs() > 0.1 || (prev.y - position.y).abs() > 0.1,
            None => true,
        };
        cursor_state.position = Some(position);
        if moved {
            adapter.slint_window.dispatch_event(WindowEvent::PointerMoved { position });
        }
    } else if any_button_held {
        // Cursor is outside the window but a button is pressed — keep last position
        // so Slint doesn't cancel the active drag/scroll operation.
        if let Some(position) = cursor_state.position {
            adapter.slint_window.dispatch_event(WindowEvent::PointerMoved { position });
        }
    } else if cursor_state.position.is_some() {
        cursor_state.position = None;
        adapter.slint_window.dispatch_event(WindowEvent::PointerExited);
    }

    // Forward mouse button events
    for event in mouse_button.read() {
        if let Some(position) = cursor_state.position {
            let button = match event.button {
                MouseButton::Left => slint::platform::PointerEventButton::Left,
                MouseButton::Right => slint::platform::PointerEventButton::Right,
                MouseButton::Middle => slint::platform::PointerEventButton::Middle,
                _ => slint::platform::PointerEventButton::Other,
            };
            match event.state {
                ButtonState::Pressed => {
                    adapter.slint_window.dispatch_event(
                        WindowEvent::PointerPressed { button, position },
                    );
                }
                ButtonState::Released => {
                    adapter.slint_window.dispatch_event(
                        WindowEvent::PointerReleased { button, position },
                    );
                }
            }
        }
    }

    // Forward scroll wheel events with the actual cursor position so Slint routes
    // them to the correct widget (toolbox, explorer, etc.) and not the 3D viewport.
    // The camera_controller consumes scroll only when the cursor is inside the
    // viewport bounds, so both can receive the event independently.
    for event in mouse_wheel.read() {
        if let Some(position) = cursor_state.position {
            use bevy::input::mouse::MouseScrollUnit;
            // Convert line-based units to logical pixels (1 line ≈ 20 logical px)
            let (dx, dy) = match event.unit {
                MouseScrollUnit::Line  => (event.x * 20.0, event.y * 20.0),
                MouseScrollUnit::Pixel => (event.x, event.y),
            };
            adapter.slint_window.dispatch_event(WindowEvent::PointerScrolled {
                position,
                delta_x: dx,
                delta_y: dy,
            });
        }
    }
}

/// Updates `SlintUIFocus.has_focus` every frame based on cursor position vs viewport bounds.
/// When the cursor is outside the 3D viewport area (i.e. over Explorer, Properties, Ribbon,
/// Output, or Toolbox panels), `has_focus` is set to `true` to block 3D tool input.
///
/// This prevents phantom clicks: clicking a tool button in the ribbon no longer also fires
/// a raycast into the 3D scene that could deselect objects or start an unintended drag.
fn update_slint_ui_focus(
    windows: Query<&Window, With<PrimaryWindow>>,
    viewport_bounds: Option<Res<super::ViewportBounds>>,
    mut ui_focus: ResMut<super::SlintUIFocus>,
) {
    let Some(vb) = viewport_bounds.as_deref() else {
        // No viewport bounds yet — assume UI doesn't have focus
        ui_focus.has_focus = false;
        return;
    };
    
    // If viewport bounds are zero, layout hasn't been computed yet
    if vb.width <= 0.0 || vb.height <= 0.0 {
        ui_focus.has_focus = false;
        return;
    }
    
    let Ok(window) = windows.single() else {
        ui_focus.has_focus = false;
        return;
    };
    
    let Some(cursor_pos) = window.cursor_position() else {
        // Cursor outside the window — clear focus
        ui_focus.has_focus = false;
        ui_focus.last_ui_position = None;
        return;
    };
    
    // Check if cursor is inside the 3D viewport bounds (physical pixels)
    let in_viewport = cursor_pos.x >= vb.x
        && cursor_pos.x <= vb.x + vb.width
        && cursor_pos.y >= vb.y
        && cursor_pos.y <= vb.y + vb.height;
    
    // has_focus = true means "UI has focus" (cursor is over a panel, NOT the viewport)
    ui_focus.has_focus = !in_viewport;
    ui_focus.last_ui_position = if !in_viewport { Some(cursor_pos) } else { None };
}

/// Try to restore auth session on startup
fn try_restore_auth_session(mut auth_state: ResMut<crate::auth::AuthState>) {
    auth_state.try_restore_session();
}

// ============================================================================
// Slint ↔ Bevy Sync Systems
// ============================================================================

/// Bundled event writers for drain_slint_actions (keeps system under 16-param limit)
#[derive(bevy::ecs::system::SystemParam)]
struct DrainEventWriters<'w> {
    file_events: MessageWriter<'w, FileEvent>,
    menu_events: MessageWriter<'w, MenuActionEvent>,
    undo_events: MessageWriter<'w, crate::commands::UndoCommandEvent>,
    redo_events: MessageWriter<'w, crate::commands::RedoCommandEvent>,
    exit_events: MessageWriter<'w, bevy::app::AppExit>,
    spawn_events: MessageWriter<'w, super::SpawnPartEvent>,
    terrain_spawn: MessageWriter<'w, super::spawn_events::SpawnTerrainEvent>,
    terrain_toggle: MessageWriter<'w, super::spawn_events::ToggleTerrainEditEvent>,
    terrain_brush: MessageWriter<'w, super::spawn_events::SetTerrainBrushEvent>,
    terrain_import: MessageWriter<'w, super::spawn_events::ImportTerrainEvent>,
    // Workshop Panel (System 0: Ideation)
    workshop_send: MessageWriter<'w, crate::workshop::WorkshopSendMessageEvent>,
    workshop_approve: MessageWriter<'w, crate::workshop::WorkshopApproveMcpEvent>,
    workshop_skip: MessageWriter<'w, crate::workshop::WorkshopSkipMcpEvent>,
    workshop_edit: MessageWriter<'w, crate::workshop::WorkshopEditMcpEvent>,
    workshop_open_artifact: MessageWriter<'w, crate::workshop::WorkshopOpenArtifactEvent>,
    workshop_optimize: MessageWriter<'w, crate::workshop::OptimizeAndBuildEvent>,
}

/// Terrain brush settings state
#[derive(Resource, Default)]
pub struct BrushState {
    pub size: f32,
    pub strength: f32,
    pub falloff: String,
}

impl BrushState {
    pub fn new() -> Self {
        Self {
            size: 10.0,
            strength: 0.5,
            falloff: "smooth".to_string(),
        }
    }
}

/// Bundled mutable resources for drain_slint_actions
#[derive(bevy::ecs::system::SystemParam)]
struct DrainResources<'w> {
    state: Option<ResMut<'w, StudioState>>,
    output: Option<ResMut<'w, OutputConsole>>,
    explorer_state: Option<ResMut<'w, UnifiedExplorerState>>,
    space_root: Option<Res<'w, crate::space::SpaceRoot>>,
    view_state: Option<ResMut<'w, super::ViewSelectorState>>,
    editor_settings: Option<ResMut<'w, crate::editor_settings::EditorSettings>>,
    auth_state: Option<ResMut<'w, crate::auth::AuthState>>,
    viewport_bounds: Option<ResMut<'w, super::ViewportBounds>>,
    tab_manager: Option<ResMut<'w, super::center_tabs::CenterTabManager>>,
    file_registry: Option<ResMut<'w, crate::space::SpaceFileRegistry>>,
    /// Shared selection state — updated on Explorer node clicks so F-to-focus works
    selection_manager: Option<Res<'w, BevySelectionManager>>,
    /// Workshop pipeline resource for cancel/pause/resume actions
    workshop_pipeline: Option<ResMut<'w, crate::workshop::IdeationPipeline>>,
    /// Material registry for resolving material names on spawned parts
    material_registry: Option<ResMut<'w, crate::space::material_loader::MaterialRegistry>>,
    /// Primitive mesh handle cache — avoids per-entity asset_server.load() for same GLB
    mesh_cache: Option<ResMut<'w, crate::space::instance_loader::PrimitiveMeshCache>>,
    /// Asset Manager panel state (expand/collapse, search, category filter)
    asset_manager_state: Option<ResMut<'w, AssetManagerState>>,
    /// Terrain brush settings (size, strength, falloff)
    brush_state: Option<ResMut<'w, BrushState>>,
    /// Standard materials for spawning instances
    materials: ResMut<'w, Assets<StandardMaterial>>,
    /// Task 12: Iggy change queue — emit SceneDeltas on property write-back (iggy-streaming feature only).
    #[cfg(feature = "iggy-streaming")]
    iggy_queue: Option<Res<'w, eustress_common::iggy_queue::IggyChangeQueue>>,
}

fn default_publish_name(space_root: Option<&Path>) -> String {
    space_root
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Untitled".to_string())
}

fn publish_target_from_sync_manifest(sync_manifest: &eustress_common::SyncManifest) -> String {
    let provider = match sync_manifest.remote.provider.trim() {
        "cloudflare_r2" => "Cloudflare R2",
        "" => "Cloudflare bucket",
        other => other,
    };

    match sync_manifest.remote.bucket.as_deref().map(str::trim) {
        Some(bucket) if !bucket.is_empty() => format!("{} bucket ({})", provider, bucket),
        _ if provider == "Cloudflare bucket" => provider.to_string(),
        _ => format!("{} bucket", provider),
    }
}

fn populate_publish_dialog(ui: &StudioWindow, space_root: Option<&Path>, as_new: bool) {
    let mut request = PublishRequest {
        experience_name: default_publish_name(space_root),
        as_new,
        ..PublishRequest::default()
    };
    let mut publish_target = "Cloudflare bucket".to_string();
    let mut is_update = false;

    if let Some(space_root) = space_root {
        let project_dir = space_root.join(".eustress");
        let publish_path = project_dir.join("publish.toml");
        let sync_path = project_dir.join("sync.toml");

        if publish_path.exists() {
            if let Ok(publish_manifest) = eustress_common::load_toml_file::<eustress_common::PublishManifest>(&publish_path) {
                if !publish_manifest.listing.name.trim().is_empty() {
                    request.experience_name = publish_manifest.listing.name;
                }
                request.description = publish_manifest.listing.description.unwrap_or_default();
                if !publish_manifest.listing.genre.trim().is_empty() {
                    request.genre = publish_manifest.listing.genre;
                }
                request.is_public = publish_manifest.visibility.is_public;
                request.open_source = publish_manifest.visibility.open_source;
                request.studio_editable = publish_manifest.visibility.studio_editable;
                is_update = publish_manifest.publish.latest_release_id.is_some()
                    || publish_manifest.publish.last_published.is_some()
                    || !publish_manifest.releases.is_empty();
            }
        }

        if sync_path.exists() {
            if let Ok(sync_manifest) = eustress_common::load_toml_file::<eustress_common::SyncManifest>(&sync_path) {
                publish_target = publish_target_from_sync_manifest(&sync_manifest);
                if !as_new {
                    is_update = is_update
                        || sync_manifest.remote.experience_id.is_some()
                        || sync_manifest.remote.project_id.is_some();
                }
            }
        }
    }

    ui.set_publish_experience_name(request.experience_name.into());
    ui.set_publish_description(request.description.into());
    ui.set_publish_genre(request.genre.into());
    ui.set_publish_is_public(request.is_public);
    ui.set_publish_open_source(request.open_source);
    ui.set_publish_studio_editable(request.studio_editable);
    ui.set_publish_as_new(as_new);
    ui.set_publish_is_update(!as_new && is_update);
    ui.set_publish_target(publish_target.into());
    ui.set_show_publish_dialog(true);
}

/// Custom SystemParam bundle to group entity queries and stay under 16-parameter limit
#[derive(bevy::ecs::system::SystemParam)]
struct DrainActionQueries<'w, 's> {
    instances: Query<'w, 's, (Entity, &'static mut eustress_common::classes::Instance)>,
    transforms: Query<'w, 's, &'static mut Transform>,
    base_parts: Query<'w, 's, &'static mut eustress_common::classes::BasePart>,
    instance_files: Query<'w, 's, &'static crate::space::instance_loader::InstanceFile>,
    loaded_from_file: Query<'w, 's, (Entity, &'static crate::space::LoadedFromFile)>,
    service_components: Query<'w, 's, &'static mut crate::space::service_loader::ServiceComponent>,
    terrain_roots: Query<'w, 's, Entity, With<eustress_common::terrain::TerrainRoot>>,
    terrain_chunks: Query<'w, 's, Entity, With<eustress_common::terrain::Chunk>>,
    camera_query: Query<'w, 's, (&'static Camera, &'static GlobalTransform)>,
}

/// Drains the SlintActionQueue each frame and dispatches to Bevy events/state.
/// This is the Slint→Bevy direction: UI button clicks become Bevy state changes and events.
fn drain_slint_actions(
    queue: Option<Res<SlintActionQueue>>,
    slint_context: Option<NonSend<SlintUiState>>,
    mut events: DrainEventWriters,
    mut res: DrainResources,
    mut queries: DrainActionQueries,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    let Some(queue) = queue else { return };
    let actions = queue.drain();
    if actions.is_empty() { return; }
    let ui = slint_context.as_ref().map(|context| &context.window);
    
    for action in actions {
        match action {
            // File operations → FileEvent
            SlintAction::NewUniverse => { events.file_events.write(FileEvent::NewUniverse); }
            SlintAction::NewScene => { events.file_events.write(FileEvent::NewScene); }
            SlintAction::OpenScene => { events.file_events.write(FileEvent::OpenScene); }
            SlintAction::SaveScene => { events.file_events.write(FileEvent::SaveScene); }
            SlintAction::SaveSceneAs => { events.file_events.write(FileEvent::SaveSceneAs); }
            SlintAction::OpenPublishDialog => {
                if let Some(ui) = ui {
                    let current_space_root = res.space_root.as_ref().map(|root| root.0.as_path());
                    populate_publish_dialog(ui, current_space_root, false);
                }
            }
            SlintAction::OpenPublishAsDialog => {
                if let Some(ui) = ui {
                    let current_space_root = res.space_root.as_ref().map(|root| root.0.as_path());
                    populate_publish_dialog(ui, current_space_root, true);
                }
            }
            SlintAction::Publish(request) => { events.file_events.write(FileEvent::Publish(request)); }
            
            // Edit operations → MenuActionEvent
            SlintAction::Undo => { events.undo_events.write(crate::commands::UndoCommandEvent); }
            SlintAction::Redo => { events.redo_events.write(crate::commands::RedoCommandEvent); }
            SlintAction::Copy => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Copy)); }
            SlintAction::Cut => {
                events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Copy));
                events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Delete));
            }
            SlintAction::Paste => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Paste)); }
            SlintAction::Delete => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Delete)); }
            SlintAction::Duplicate => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Duplicate)); }
            SlintAction::SelectAll => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::SelectAll)); }
            
            // Tool selection → StudioState
            SlintAction::SelectTool(tool) => {
                if let Some(ref mut s) = res.state {
                    s.current_tool = match tool.as_str() {
                        "move" => Tool::Move,
                        "rotate" => Tool::Rotate,
                        "scale" => Tool::Scale,
                        "terrain" => Tool::Terrain,
                        _ => Tool::Select,
                    };
                    if let Some(ref mut out) = res.output {
                        out.info(format!("Tool: {}", tool));
                    }
                }
            }
            
            // Transform mode → StudioState
            SlintAction::SetTransformMode(mode) => {
                if let Some(ref mut s) = res.state {
                    s.transform_mode = match mode.as_str() {
                        "local" => TransformMode::Local,
                        _ => TransformMode::World,
                    };
                }
            }
            
            // Play controls → StudioState flags (consumed by play_mode.rs)
            SlintAction::PlaySolo => {
                if let Some(ref mut s) = res.state {
                    s.play_solo_requested = true;
                }
            }
            SlintAction::PlayWithCharacter => {
                if let Some(ref mut s) = res.state {
                    s.play_with_character_requested = true;
                }
            }
            SlintAction::Pause => {
                if let Some(ref mut s) = res.state {
                    s.pause_requested = true;
                }
            }
            SlintAction::Stop => {
                if let Some(ref mut s) = res.state {
                    s.stop_requested = true;
                }
            }

            // Simulation settings — save to simulation.toml (also applies)
            SlintAction::SaveSimulationSettings => {
                if let Some(ref mut out) = res.output {
                    out.info("Simulation settings saved to simulation.toml.".to_string());
                }
            }
            SlintAction::AddSimWatchpoint => {
                if let Some(ref mut out) = res.output {
                    out.info("Watchpoint added.".to_string());
                }
            }
            SlintAction::RemoveSimWatchpoint(idx) => {
                if let Some(ref mut out) = res.output {
                    out.info(format!("Watchpoint {} removed.", idx));
                }
            }
            SlintAction::AddSimOutputBinding => {
                if let Some(ref mut out) = res.output {
                    out.info("Output binding added.".to_string());
                }
            }
            SlintAction::RemoveSimOutputBinding(idx) => {
                if let Some(ref mut out) = res.output {
                    out.info(format!("Output binding {} removed.", idx));
                }
            }

            // View
            SlintAction::FocusSelected => {
                events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::FocusSelection));
            }
            SlintAction::SetViewMode(_mode) => {
                // View mode changes handled by camera controller
            }
            SlintAction::ToggleWireframe => {
                if let Some(ref mut vs) = res.view_state {
                    vs.wireframe = !vs.wireframe;
                }
            }
            SlintAction::ToggleGrid => {
                if let Some(ref mut vs) = res.view_state {
                    vs.grid = !vs.grid;
                }
            }
            SlintAction::ToggleSnap => {
                if let Some(ref mut es) = res.editor_settings {
                    es.snap_enabled = !es.snap_enabled;
                    if let Some(ref mut out) = res.output {
                        out.info(format!("Snap: {}", if es.snap_enabled { "ON" } else { "OFF" }));
                    }
                }
            }
            SlintAction::SetSnapIncrement(val) => {
                if let Some(ref mut es) = res.editor_settings {
                    es.snap_size = val;
                    if let Some(ref mut out) = res.output {
                        out.info(format!("Snap increment: {:.2}", val));
                    }
                }
            }
            
            // Panel toggles → StudioState
            SlintAction::ToggleCommandBar => {
                if let Some(ref mut s) = res.state {
                    // Toggled directly in Slint via show-command-bar binding
                }
            }
            SlintAction::ShowKeybindings => {
                if let Some(ref mut s) = res.state {
                    s.show_keybindings_window = true;
                }
            }
            SlintAction::ShowSoulSettings => {
                if let Some(ref mut s) = res.state {
                    s.show_soul_settings_window = true;
                }
            }
            SlintAction::ShowSettings => {
                if let Some(ref mut s) = res.state {
                    s.show_settings_window = true;
                }
            }
            SlintAction::ShowFind => {
                if let Some(ref mut s) = res.state {
                    s.show_find_dialog = true;
                }
            }
            
            // Network → StudioState
            SlintAction::StartServer => {
                events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::StartServer));
            }
            SlintAction::StopServer => {
                events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::StopServer));
            }
            SlintAction::ConnectForge => {
                if let Some(ref mut s) = res.state {
                    s.show_forge_connect_window = true;
                }
            }
            SlintAction::DisconnectForge => {}
            SlintAction::AllocateForgeServer => {}
            SlintAction::SpawnSyntheticClients(count) => {
                if let Some(ref mut s) = res.state {
                    s.synthetic_client_count = count as u32;
                    s.synthetic_clients_changed = true;
                }
            }
            SlintAction::DisconnectAllClients => {}
            
            // Data → StudioState
            SlintAction::OpenGlobalSources => {
                if let Some(ref mut s) = res.state {
                    s.show_global_sources_window = true;
                }
            }
            SlintAction::OpenDomains => {
                if let Some(ref mut s) = res.state {
                    s.show_domains_window = true;
                }
            }
            SlintAction::OpenGlobalVariables => {
                if let Some(ref mut s) = res.state {
                    s.show_global_variables_window = true;
                }
            }
            
            // MindSpace
            SlintAction::ToggleMindspace => {
                if let Some(ref mut s) = res.state {
                    s.mindspace_panel_visible = !s.mindspace_panel_visible;
                }
            }
            SlintAction::MindspaceAddLabel => {
                // TODO: Add label node to MindSpace graph
            }
            SlintAction::MindspaceConnect => {
                // TODO: Connect selected MindSpace nodes
            }
            
            // Auth
            SlintAction::Login => {
                if let Some(ref mut s) = res.state {
                    s.trigger_login = true;
                }
            }
            SlintAction::Logout => {
                if let Some(ref mut auth) = res.auth_state {
                    auth.logout();
                    if let Some(ref mut out) = res.output {
                        out.info("Logged out".to_string());
                    }
                }
            }
            
            // Scripts
            SlintAction::BuildScript(id) => {
                if let Some(ref mut out) = res.output {
                    out.info(format!("Building script #{}", id));
                }
                // TODO: Trigger Soul script compilation for entity with this instance ID
            }
            SlintAction::OpenScript(id) => {
                // Open a Soul Script in a new center tab (or focus existing)
                let script_name = queries.instances.iter()
                    .find(|(_, inst)| inst.id as i32 == id)
                    .map(|(_, inst)| inst.name.clone())
                    .unwrap_or_else(|| format!("Script #{}", id));
                if let Some(ref mut out) = res.output {
                    out.info(format!("Opening script: {}", script_name));
                }
                // Tab opening is handled by the Slint-side sync system
                // We store the request in StudioState for the sync system to pick up
                if let Some(ref mut s) = res.state {
                    s.pending_open_script = Some((id, script_name));
                }
            }
            
            // Center tab management
            // All tab actions update BOTH CenterTabManager (source of truth)
            // AND StudioState so the two stay in sync.
            SlintAction::CloseCenterTab(idx) => {
                // Slint sends 0-based index into the non-scene tabs list.
                // CenterTabManager uses Scene at index 0, so mgr_idx = idx + 1.
                if let Some(ref mut mgr) = res.tab_manager {
                    let mgr_idx = (idx as usize) + 1;
                    mgr.close_tab(mgr_idx);
                } else if let Some(ref mut s) = res.state {
                    s.pending_close_tab = Some(idx);
                }
            }
            SlintAction::SelectCenterTab(idx) => {
                // idx from Slint: 0 = Scene, 1+ = other tabs
                // CenterTabManager uses the same indexing.
                if let Some(ref mut mgr) = res.tab_manager {
                    mgr.select_tab(idx as usize);
                }
                if let Some(ref mut s) = res.state {
                    s.active_center_tab = idx;
                }
            }
            SlintAction::ScriptContentChanged(text) => {
                // Update CenterTabManager content first (before moving text)
                if let Some(ref mut mgr) = res.tab_manager {
                    let idx = mgr.active_tab;
                    if let Some(active) = mgr.tabs.get_mut(idx) {
                        active.content = text.clone();
                    }
                }
                // Store updated script content and mark dirty so
                // sync_center_tabs_to_slint pushes fresh line numbers this frame.
                if let Some(ref mut s) = res.state {
                    s.script_editor_content = text;
                    s.script_content_dirty = true;
                }
            }
            SlintAction::ReorderCenterTab(from, to) => {
                // Slint sends 0-based indices into the non-scene tabs list.
                // CenterTabManager: Scene at 0, so mgr indices = from+1, to+1.
                if let Some(ref mut mgr) = res.tab_manager {
                    mgr.reorder_tab((from as usize) + 1, (to as usize) + 1);
                } else if let Some(ref mut s) = res.state {
                    s.pending_reorder = Some((from, to));
                }
            }
            SlintAction::ToggleTabMode(studio_idx) => {
                // studio_idx is 1-based (StudioState.center_tabs index).
                // CenterTabManager has Scene at 0, so mgr index = studio_idx.
                if let Some(ref mut mgr) = res.tab_manager {
                    let mgr_idx = studio_idx as usize;
                    mgr.toggle_mode(mgr_idx);
                }
            }
            
            // Web browser
            SlintAction::OpenWebTab(url) => {
                if let Some(ref mut s) = res.state {
                    s.pending_open_web = Some(url.clone());
                }
                if let Some(ref mut out) = res.output {
                    out.info(format!("Opening web tab: {}", url));
                }
            }
            SlintAction::WebNavigate(url) => {
                // Normalize: prepend https:// if no scheme is present
                let url = if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("about:") {
                    url
                } else {
                    format!("https://{}", url)
                };
                if let Some(ref mut s) = res.state {
                    // Always update the URL bar state so Slint shows the correct URL
                    s.pending_web_navigate = Some(url.clone());
                    // Update the active tab's URL immediately for the URL bar display
                    if s.active_center_tab > 0 {
                        let idx = (s.active_center_tab - 1) as usize;
                        if let Some(tab) = s.center_tabs.get_mut(idx) {
                            if tab.tab_type == "web" {
                                tab.url = url.clone();
                                tab.name = url.clone();
                            }
                        }
                    }
                    s.tabs_dirty = true;
                }
                // Without the webview feature, open in system default browser
                #[cfg(not(feature = "webview"))]
                {
                    if let Err(e) = open::that(&url) {
                        warn!("Failed to open URL in system browser: {}", e);
                    }
                }
            }
            SlintAction::WebGoBack => {
                if let Some(ref mut s) = res.state {
                    s.pending_web_back = true;
                }
            }
            SlintAction::WebGoForward => {
                if let Some(ref mut s) = res.state {
                    s.pending_web_forward = true;
                }
            }
            SlintAction::WebRefresh => {
                if let Some(ref mut s) = res.state {
                    s.pending_web_refresh = true;
                }
            }
            
            // Terrain
            SlintAction::GenerateTerrain(size) => {
                // Spawn terrain with size-appropriate config and switch to Terrain tab
                use eustress_common::terrain::TerrainConfig;
                let config = match size.as_str() {
                    "small" => TerrainConfig::small(),
                    "large" => TerrainConfig::large(),
                    _ => TerrainConfig::default(), // "medium"
                };
                events.terrain_spawn.write(super::spawn_events::SpawnTerrainEvent { config });
                if let Some(ref mut s) = res.state {
                    s.show_terrain_editor = true;
                }
            }
            SlintAction::ToggleTerrainEditMode => {
                events.terrain_toggle.write(super::spawn_events::ToggleTerrainEditEvent);
            }
            SlintAction::SetTerrainBrush(brush) => {
                use eustress_common::terrain::BrushMode;
                let mode = match brush.to_lowercase().as_str() {
                    "raise" => Some(BrushMode::Raise),
                    "lower" => Some(BrushMode::Lower),
                    "smooth" => Some(BrushMode::Smooth),
                    "flatten" => Some(BrushMode::Flatten),
                    "paint" | "painttexture" => Some(BrushMode::PaintTexture),
                    "voxeladd" => Some(BrushMode::VoxelAdd),
                    "voxelremove" => Some(BrushMode::VoxelRemove),
                    "voxelsmooth" => Some(BrushMode::VoxelSmooth),
                    "region" => Some(BrushMode::Region),
                    "fill" => Some(BrushMode::Fill),
                    _ => None,
                };
                if let Some(m) = mode {
                    events.terrain_brush.write(super::spawn_events::SetTerrainBrushEvent { mode: m });
                }
            }
            SlintAction::BrushSizeChanged(size) => {
                // Update brush size in terrain state
                if let Some(ref mut brush_state) = res.brush_state {
                    brush_state.size = size;
                }
            }
            SlintAction::BrushStrengthChanged(strength) => {
                // Update brush strength in terrain state
                if let Some(ref mut brush_state) = res.brush_state {
                    brush_state.strength = strength;
                }
            }
            SlintAction::BrushFalloffChanged(falloff) => {
                // Update brush falloff in terrain state
                if let Some(ref mut brush_state) = res.brush_state {
                    brush_state.falloff = falloff;
                }
            }
            SlintAction::ImportHeightmap => {
                // Open file dialog for heightmap import
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Heightmap", &["png", "r16", "raw", "hgt", "asc", "tif", "tiff"])
                    .set_title("Import Heightmap")
                    .pick_file()
                {
                    if let Some(ref mut out) = res.output {
                        out.info(format!("Importing heightmap: {}", path.display()));
                    }
                    events.terrain_import.write(super::spawn_events::ImportTerrainEvent {
                        path: path.to_string_lossy().to_string(),
                    });
                    if let Some(ref mut s) = res.state {
                        s.show_terrain_editor = true;
                    }
                }
            }
            SlintAction::ExportHeightmap => {
                // Open file dialog for heightmap export
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Heightmap PNG", &["png"])
                    .set_title("Export Heightmap")
                    .save_file()
                {
                    if let Some(ref mut out) = res.output {
                        out.info(format!("Exporting heightmap: {}", path.display()));
                    }
                    // TODO: Export terrain data when heightmap exporter is implemented
                }
            }
            
            // Layout
            SlintAction::ApplyLayoutPreset(preset) => {
                // Apply preset layout configurations
                if let Some(ref mut s) = res.state {
                    match preset {
                        0 => { // Default
                            s.show_explorer = true;
                            s.show_properties = true;
                            s.show_output = true;
                        }
                        1 => { // Minimal — hide side panels
                            s.show_explorer = false;
                            s.show_properties = false;
                            s.show_output = false;
                        }
                        2 => { // Code — explorer + output, no properties
                            s.show_explorer = true;
                            s.show_properties = false;
                            s.show_output = true;
                        }
                        3 => { // Build — all panels visible
                            s.show_explorer = true;
                            s.show_properties = true;
                            s.show_output = true;
                        }
                        _ => {}
                    }
                }
            }
            SlintAction::SaveLayoutToFile => {
                if let Some(ref es) = res.editor_settings {
                    if let Err(e) = es.save() {
                        if let Some(ref mut out) = res.output {
                            out.info(format!("Failed to save layout: {}", e));
                        }
                    } else if let Some(ref mut out) = res.output {
                        out.info("Layout saved".to_string());
                    }
                }
            }
            SlintAction::LoadLayoutFromFile => {
                // Reload editor settings from disk
                let loaded = crate::editor_settings::EditorSettings::load();
                if let Some(ref mut es) = res.editor_settings {
                    **es = loaded;
                    if let Some(ref mut out) = res.output {
                        out.info("Layout loaded".to_string());
                    }
                }
            }
            SlintAction::ResetLayoutToDefault => {
                if let Some(ref mut s) = res.state {
                    s.show_explorer = true;
                    s.show_properties = true;
                    s.show_output = true;
                }
            }
            SlintAction::ToggleThemeEditor => {
                if let Some(ref mut out) = res.output {
                    out.info("Theme editor toggled".to_string());
                }
                // Theme editor visibility is managed by Slint-side state
            }
            SlintAction::ApplyThemeSettings(dark_mode, _high_contrast, _ui_scale) => {
                // Push dark_theme to Slint via the sync_bevy_to_slint system
                // The Slint UI reads dark-theme property each frame
                // For now we log — the Slint property is set directly by the callback
                if let Some(ref mut out) = res.output {
                    out.info(format!("Theme: dark={}", dark_mode));
                }
            }
            SlintAction::DetachPanelToWindow(_panel) => {
                // TODO: Detach panel to separate OS window
            }
            
            // Viewport bounds changed — update Bevy camera viewport clipping
            SlintAction::ViewportBoundsChanged(x, y, w, h) => {
                // Store in ViewportBounds resource for camera controller to use
                if let Some(ref mut vb) = res.viewport_bounds {
                    vb.x = x;
                    vb.y = y;
                    vb.width = w;
                    vb.height = h;
                }
            }
            
            // Explorer actions — unified node handling (entities + files)
            SlintAction::SelectNode(id, node_type) => {
                if let Some(ref mut es) = res.explorer_state {
                    if node_type == "entity" {
                        if id < 0 {
                            // Negative IDs are service header nodes from make_service_node().
                            // Reconstruct the service name by matching against known service name lengths.
                            // Service names are stored in class_name on the TreeNode; we recover them
                            // by scanning service_components for entities whose name matches the ID.
                            let service_name = queries.service_components.iter()
                                .find_map(|sc| {
                                    // ServiceComponent.class_name matches the service display name
                                    if -(sc.class_name.len() as i32) == id {
                                        Some(sc.class_name.clone())
                                    } else {
                                        None
                                    }
                                });
                            // Fallback: look up known service names by their negative ID
                            let name = service_name.or_else(|| {
                                let known = ["Workspace","Lighting","Players","StarterGui","StarterPack",
                                    "StarterPlayer","ReplicatedStorage","ServerStorage",
                                    "ServerScriptService","SoulService","SoundService","Teams","Chat"];
                                known.iter().find(|n| -(n.len() as i32) == id).map(|n| n.to_string())
                            });
                            es.selected = name.map(SelectedItem::Service).unwrap_or(SelectedItem::None);
                            // Clear entity selection — service nodes are not focusable parts
                            if let Some(ref sel_mgr) = res.selection_manager {
                                sel_mgr.0.write().clear();
                            }
                        } else {
                            // Positive ID — look up Entity from stable sequential ID cache
                            if let Some(entity) = es.entity_id_cache.get(&id).copied() {
                                // Accept entity if it has Instance component OR if it's a terrain entity
                                let is_valid = queries.instances.get(entity).is_ok() 
                                    || queries.terrain_roots.get(entity).is_ok()
                                    || queries.terrain_chunks.get(entity).is_ok();
                                
                                if is_valid {
                                    es.selected = SelectedItem::Entity(entity);
                                    
                                    // Also update BevySelectionManager so FocusSelection (F key)
                                    // can read the correct entity bounds from SelectionSyncManager.
                                    if let Some(ref sel_mgr) = res.selection_manager {
                                        let id_str = format!("{}v{}", entity.index(), entity.generation());
                                        sel_mgr.0.write().select(id_str);
                                    }
                                } else {
                                    es.selected = SelectedItem::None;
                                }
                            } else {
                                es.selected = SelectedItem::None;
                            }
                        }
                    } else {
                        // File node — look up path by hash ID from file_path_cache
                        if let Some(path) = es.file_path_cache.get(&id).cloned() {
                            es.selected = SelectedItem::File(path);
                        } else {
                            es.selected = SelectedItem::None;
                        }
                    }
                    es.needs_immediate_sync = true;
                }
            }
            SlintAction::ExpandNode(id, node_type) => {
                if let Some(ref mut es) = res.explorer_state {
                    if node_type == "entity" {
                        if id < 0 {
                            // Negative ID — service header node. Recover name from known list.
                            let known = ["Workspace","Lighting","Players","StarterGui","StarterPack",
                                "StarterPlayer","ReplicatedStorage","ServerStorage",
                                "ServerScriptService","SoulService","SoundService","Teams","Chat"];
                            if let Some(name) = known.iter().find(|n| -(n.len() as i32) == id) {
                                es.expanded_services.insert(name.to_string());
                            }
                        } else {
                            if let Some(entity) = es.entity_id_cache.get(&id).copied() {
                                es.expanded_entities.insert(entity);
                            }
                        }
                    } else {
                        // File node — expand directory by hash ID lookup
                        if let Some(path) = es.file_path_cache.get(&id).cloned() {
                            es.expanded_dirs.insert(path);
                        }
                        es.dirty = true;
                    }
                    es.needs_immediate_sync = true;
                }
            }
            SlintAction::CollapseNode(id, node_type) => {
                if let Some(ref mut es) = res.explorer_state {
                    if node_type == "entity" {
                        if id < 0 {
                            // Negative ID — service header node.
                            let known = ["Workspace","Lighting","Players","StarterGui","StarterPack",
                                "StarterPlayer","ReplicatedStorage","ServerStorage",
                                "ServerScriptService","SoulService","SoundService","Teams","Chat"];
                            if let Some(name) = known.iter().find(|n| -(n.len() as i32) == id) {
                                es.expanded_services.remove(*name);
                            }
                        } else {
                            if let Some(entity) = es.entity_id_cache.get(&id).copied() {
                                es.expanded_entities.remove(&entity);
                            }
                        }
                    } else {
                        // File node — collapse directory by hash ID lookup
                        if let Some(path) = es.file_path_cache.get(&id).cloned() {
                            es.expanded_dirs.remove(&path);
                        }
                        es.dirty = true;
                    }
                    es.needs_immediate_sync = true;
                }
            }
            SlintAction::OpenNode(id, node_type) => {
                if node_type == "entity" {
                    // Double-click entity — open script if SoulScript.
                    // Look up Entity from the stable sequential ID cache.
                    let entity_opt = res.explorer_state.as_ref()
                        .and_then(|es| es.entity_id_cache.get(&id).copied());
                    if let Some(entity) = entity_opt {
                        if let Ok((_, inst)) = queries.instances.get(entity) {
                            if inst.class_name == eustress_common::classes::ClassName::SoulScript {
                                // Open SoulScript in tabbed viewer
                                // Get the file path from LoadedFromFile component if available
                                if let Ok((_, loaded)) = queries.loaded_from_file.get(entity) {
                                    if let Some(ref mut mgr) = res.tab_manager {
                                        let idx = mgr.open_file(&loaded.path);
                                        if let Some(ref mut out) = res.output {
                                            out.info(format!("Opened SoulScript: {} (tab {})", inst.name, idx));
                                        }
                                    }
                                } else {
                                    // Fallback: use pending_open_script for scripts without file path
                                    if let Some(ref mut s) = res.state {
                                        s.pending_open_script = Some((id, inst.name.clone()));
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // Double-click file — open in appropriate tab via CenterTabManager
                    let file_path = res.explorer_state.as_ref()
                        .and_then(|es| es.file_path_cache.get(&id).cloned());
                    if let Some(path) = file_path {
                        if path.is_file() {
                            if let Some(ref mut mgr) = res.tab_manager {
                                let idx = mgr.open_file(&path);
                                if let Some(ref mut out) = res.output {
                                    out.info(format!("Opened: {} (tab {})", path.display(), idx));
                                }
                            }
                        } else if path.is_dir() {
                            // Double-click directory — toggle expand
                            if let Some(ref mut es) = res.explorer_state {
                                if es.expanded_dirs.contains(&path) {
                                    es.expanded_dirs.remove(&path);
                                } else {
                                    es.expanded_dirs.insert(path);
                                }
                                es.dirty = true;
                            }
                        }
                    }
                }
            }
            SlintAction::RenameNode(id, name, node_type) => {
                if node_type == "entity" {
                    // Look up Entity from stable sequential ID cache
                    let entity_opt = res.explorer_state.as_ref()
                        .and_then(|es| es.entity_id_cache.get(&id).copied());
                    if let Some(entity) = entity_opt {
                        if let Ok((_, mut inst)) = queries.instances.get_mut(entity) {
                            inst.name = name;
                        }
                    }
                } else {
                    // File rename — filesystem operation
                    let file_path = res.explorer_state.as_ref()
                        .and_then(|es| es.file_path_cache.get(&id).cloned());
                    if let Some(old_path) = file_path {
                        let new_path = old_path.with_file_name(&name);
                        match std::fs::rename(&old_path, &new_path) {
                            Ok(_) => {
                                if let Some(ref mut out) = res.output {
                                    out.info(format!("Renamed: {} → {}", old_path.display(), new_path.display()));
                                }
                                if let Some(ref mut es) = res.explorer_state {
                                    es.dirty = true;
                                }
                            }
                            Err(e) => {
                                if let Some(ref mut out) = res.output {
                                    out.error(format!("Rename failed: {}", e));
                                }
                            }
                        }
                    }
                }
            }
            
            SlintAction::AddService => {
                // (+) button — log intent; actual service creation requires user to pick
                // a name/type via command bar or future dialog. For now, print available services.
                if let Some(ref mut out) = res.output {
                    out.info("Add Service: use the command bar to create a new service (e.g. 'add service MyService')".to_string());
                }
            }
            
            SlintAction::ExpandAll => {
                // Expand all expandable nodes in the explorer tree
                if let Some(ref mut es) = res.explorer_state {
                    // Expand all ECS services
                    for svc in ["Workspace", "Lighting", "Players", "StarterGui", "StarterPack", "StarterPlayer", "ServerStorage", "SoulService", "SoundService", "Teams", "Chat", "LocalizationService", "TestService"] {
                        es.expanded_services.insert(svc.to_string());
                    }
                    // Mark explorer dirty so tree rebuilds with all expanded
                    es.dirty = true;
                }
            }
            
            SlintAction::CollapseAll => {
                // Collapse all nodes in the explorer tree
                if let Some(ref mut es) = res.explorer_state {
                    es.expanded_services.clear();
                    es.expanded_dirs.clear();
                    es.dirty = true;
                }
            }
            
            // Properties section collapse/expand toggle
            SlintAction::SectionToggle(category) => {
                if let Some(ref mut s) = res.state {
                    if !s.collapsed_sections.remove(&category) {
                        s.collapsed_sections.insert(category);
                    }
                    // Force properties re-sync next frame so section-collapsed flags update
                    s.show_properties = true;
                }
            }
            
            // Properties write-back — apply edits from Slint properties panel to ECS
            SlintAction::PropertyChanged(key, raw_val) => {
                // Decode rotation step protocol: "step:axis:+1:x,y,z" or "step:axis:-1:x,y,z"
                // Emitted by RotationVec3Row +/- buttons to avoid Slint float-to-string conversion.
                let val: String = if raw_val.starts_with("step:") {
                    // Format: "step:<axis>:<delta>:<x>,<y>,<z>"
                    // axis = x|y|z, delta = +1|-1, x/y/z are current degree strings
                    let parts: Vec<&str> = raw_val.splitn(4, ':').collect();
                    if parts.len() == 4 {
                        let axis = parts[1];
                        let delta: f32 = parts[2].parse().unwrap_or(0.0);
                        let coords: Vec<f32> = parts[3].split(',')
                            .map(|s| s.trim().parse::<f32>().unwrap_or(0.0))
                            .collect();
                        if coords.len() == 3 {
                            let (mut cx, mut cy, mut cz) = (coords[0], coords[1], coords[2]);
                            match axis { "x" => cx += delta, "y" => cy += delta, "z" => cz += delta, _ => {} }
                            format!("{:.2}, {:.2}, {:.2}", cx, cy, cz)
                        } else { raw_val.clone() }
                    } else { raw_val.clone() }
                } else {
                    raw_val.clone()
                };
                let selected_entity = res.explorer_state.as_ref().and_then(|es| {
                    match &es.selected {
                        SelectedItem::Entity(e) => Some(*e),
                        _ => None,
                    }
                });
                if let Some(entity) = selected_entity {
                    match key.as_str() {
                        // Instance fields
                        "Name" => {
                            if let Ok((_, mut inst)) = queries.instances.get_mut(entity) {
                                inst.name = val.clone();
                            }
                        }
                        "Archivable" => {
                            if let Ok((_, mut inst)) = queries.instances.get_mut(entity) {
                                inst.archivable = val == "true";
                            }
                        }
                        // Transform fields
                        "Position.X" | "Position.Y" | "Position.Z" => {
                            if let Ok(mut t) = queries.transforms.get_mut(entity) {
                                if let Ok(v) = val.parse::<f32>() {
                                    match key.as_str() {
                                        "Position.X" => t.translation.x = v,
                                        "Position.Y" => t.translation.y = v,
                                        "Position.Z" => t.translation.z = v,
                                        _ => {}
                                    }
                                }
                            }
                        }
                        "Scale.X" | "Scale.Y" | "Scale.Z" => {
                            if let Ok(mut t) = queries.transforms.get_mut(entity) {
                                if let Ok(v) = val.parse::<f32>() {
                                    match key.as_str() {
                                        "Scale.X" => t.scale.x = v,
                                        "Scale.Y" => t.scale.y = v,
                                        "Scale.Z" => t.scale.z = v,
                                        _ => {}
                                    }
                                }
                            }
                        }
                        // BasePart fields
                        "Transparency" => {
                            if let Ok(mut bp) = queries.base_parts.get_mut(entity) {
                                if let Ok(v) = val.parse::<f32>() {
                                    bp.transparency = v;
                                }
                            }
                        }
                        "Anchored" => {
                            if let Ok(mut bp) = queries.base_parts.get_mut(entity) {
                                bp.anchored = val == "true";
                            }
                        }
                        "CanCollide" => {
                            if let Ok(mut bp) = queries.base_parts.get_mut(entity) {
                                bp.can_collide = val == "true";
                            }
                        }
                        "Locked" => {
                            if let Ok(mut bp) = queries.base_parts.get_mut(entity) {
                                bp.locked = val == "true";
                            }
                        }
                        _ => {
                            // TODO: UI class ECS component live mutation via PropertyAccess
                            // This requires ui_queries ParamSet which is not available in drain_slint_actions
                            // For now, just log unhandled properties
                            if let Some(ref mut out) = res.output {
                                out.info(format!("Property '{}' = '{}' (unhandled)", key, val));
                            }
                        }
                    }
                    
                    // ══════════════════════════════════════════════════════════
                    // File-System-First: Write property changes back to TOML
                    // Supports ALL properties dynamically
                    // ══════════════════════════════════════════════════════════
                    if let Ok(instance_file) = queries.instance_files.get(entity) {
                        match crate::space::instance_loader::load_instance_definition(&instance_file.toml_path) {
                            Ok(mut def) => {
                                let changed = update_toml_property(&mut def, &key, &val);
                                
                                if changed {
                                    def.metadata.last_modified = chrono::Utc::now().to_rfc3339();
                                    
                                    if let Err(e) = crate::space::instance_loader::write_instance_definition(
                                        &instance_file.toml_path,
                                        &def,
                                    ) {
                                        if let Some(ref mut out) = res.output {
                                            out.error(format!("Failed to write TOML: {}", e));
                                        }
                                    } else {
                                        if let Some(ref mut out) = res.output {
                                            out.info(format!("💾 Saved {} to {:?}", key, instance_file.toml_path.file_name().unwrap_or_default()));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                if let Some(ref mut out) = res.output {
                                    out.error(format!("Failed to load TOML for write-back: {}", e));
                                }
                            }
                        }
                    }
                    
                    // ══════════════════════════════════════════════════════════
                    // Service property write-back: Update ServiceComponent dynamically
                    // Any property can be edited - no hardcoding
                    // ══════════════════════════════════════════════════════════
                    if let Ok(mut service) = queries.service_components.get_mut(entity) {
                        let mut changed = false;
                        
                        // Handle special service fields
                        match key.as_str() {
                            "Icon" => {
                                service.icon = val.clone();
                                changed = true;
                            }
                            "Description" => {
                                service.description = val.clone();
                                changed = true;
                            }
                            _ => {
                                // Dynamic property - parse value based on existing type or infer
                                if let Some(existing) = service.properties.get(&key) {
                                    // Update based on existing type
                                    let new_value = match existing {
                                        crate::space::service_loader::PropertyValue::Bool(_) => {
                                            Some(crate::space::service_loader::PropertyValue::Bool(val == "true"))
                                        }
                                        crate::space::service_loader::PropertyValue::Int(_) => {
                                            val.parse::<i64>().ok().map(crate::space::service_loader::PropertyValue::Int)
                                        }
                                        crate::space::service_loader::PropertyValue::Float(_) => {
                                            val.parse::<f64>().ok().map(crate::space::service_loader::PropertyValue::Float)
                                        }
                                        crate::space::service_loader::PropertyValue::String(_) => {
                                            Some(crate::space::service_loader::PropertyValue::String(val.clone()))
                                        }
                                        crate::space::service_loader::PropertyValue::Vec3(_) => {
                                            parse_vec3_value(&val).map(|(x, y, z)| {
                                                crate::space::service_loader::PropertyValue::Vec3([x as f64, y as f64, z as f64])
                                            })
                                        }
                                        crate::space::service_loader::PropertyValue::Vec4(_) => {
                                            parse_color4_value(&val).map(|(r, g, b, a)| {
                                                crate::space::service_loader::PropertyValue::Vec4([r as f64, g as f64, b as f64, a as f64])
                                            })
                                        }
                                    };
                                    
                                    if let Some(new_val) = new_value {
                                        service.properties.insert(key.clone(), new_val);
                                        changed = true;
                                    }
                                }
                            }
                        }
                        
                        // Save to _service.toml file
                        if changed {
                            if let Err(e) = crate::space::service_loader::save_service_to_file(&service) {
                                if let Some(ref mut out) = res.output {
                                    out.error(format!("Failed to save service: {}", e));
                                }
                            } else if let Some(ref mut out) = res.output {
                                out.info(format!("💾 Saved {} to {:?}", key, service.toml_path.file_name().unwrap_or_default()));
                            }
                        }
                    }

                    // ══════════════════════════════════════════════════════════
                    // Task 12: Iggy delta emission — stream property change to
                    // IggyChangeQueue so TOML materializers and remote agents
                    // observe every write-back in real time.
                    //
                    // DeltaKind selection:
                    //   Position.* / Scale.* / Rotation.* → TransformChanged
                    //   Name → Renamed
                    //   everything else → PartPropertiesChanged
                    // ══════════════════════════════════════════════════════════
                    #[cfg(feature = "iggy-streaming")]
                    if let Some(ref iggy) = res.iggy_queue {
                        use eustress_common::iggy_delta::{
                            DeltaKind, NamePayload, PartPayload, SceneDelta, TransformPayload,
                        };

                        let entity_id = entity.to_bits();
                        let seq = iggy.next_seq();
                        let ts  = iggy.now_ms();

                        let delta = match key.as_str() {
                            "Position.X" | "Position.Y" | "Position.Z"
                            | "Scale.X" | "Scale.Y" | "Scale.Z"
                            | "Rotation.X" | "Rotation.Y" | "Rotation.Z" => {
                                // Read live transform from ECS for a consistent snapshot.
                                let t = queries.transforms.get(entity).ok();
                                let pos = t.map(|t| [t.translation.x, t.translation.y, t.translation.z])
                                    .unwrap_or([0.0, 0.0, 0.0]);
                                let rot = t.map(|t| [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w])
                                    .unwrap_or([0.0, 0.0, 0.0, 1.0]);
                                let scl = t.map(|t| [t.scale.x, t.scale.y, t.scale.z])
                                    .unwrap_or([1.0, 1.0, 1.0]);
                                Some(SceneDelta::transform(
                                    entity_id, seq, ts,
                                    TransformPayload { position: pos, rotation: rot, scale: scl },
                                ))
                            }
                            "Name" => {
                                Some(SceneDelta {
                                    entity: entity_id,
                                    kind: DeltaKind::Renamed,
                                    seq,
                                    timestamp_ms: ts,
                                    transform: None,
                                    part: None,
                                    name: Some(NamePayload { name: val.clone() }),
                                    new_parent: None,
                                })
                            }
                            "Transparency" => {
                                let v = val.parse::<f32>().ok();
                                Some(SceneDelta {
                                    entity: entity_id,
                                    kind: DeltaKind::PartPropertiesChanged,
                                    seq,
                                    timestamp_ms: ts,
                                    transform: None,
                                    part: Some(PartPayload {
                                        color: None, material: None, size: None,
                                        name: None, anchored: None, can_collide: None,
                                        transparency: v, reflectance: None,
                                    }),
                                    name: None,
                                    new_parent: None,
                                })
                            }
                            "Anchored" => {
                                Some(SceneDelta {
                                    entity: entity_id,
                                    kind: DeltaKind::PartPropertiesChanged,
                                    seq,
                                    timestamp_ms: ts,
                                    transform: None,
                                    part: Some(PartPayload {
                                        color: None, material: None, size: None,
                                        name: None, anchored: Some(val == "true"),
                                        can_collide: None, transparency: None, reflectance: None,
                                    }),
                                    name: None,
                                    new_parent: None,
                                })
                            }
                            "CanCollide" => {
                                Some(SceneDelta {
                                    entity: entity_id,
                                    kind: DeltaKind::PartPropertiesChanged,
                                    seq,
                                    timestamp_ms: ts,
                                    transform: None,
                                    part: Some(PartPayload {
                                        color: None, material: None, size: None,
                                        name: None, anchored: None,
                                        can_collide: Some(val == "true"),
                                        transparency: None, reflectance: None,
                                    }),
                                    name: None,
                                    new_parent: None,
                                })
                            }
                            _ => {
                                // Remaining property — emit PartPropertiesChanged as a presence
                                // signal so materializers know the entity was touched this frame.
                                Some(SceneDelta::lifecycle(
                                    entity_id,
                                    DeltaKind::PartPropertiesChanged,
                                    seq,
                                    ts,
                                ))
                            }
                        };

                        if let Some(d) = delta {
                            iggy.send_delta(d);
                        }
                    }
                }
            }
            
            // Command bar
            SlintAction::ExecuteCommand(cmd) => {
                if let Some(ref mut out) = res.output {
                    out.info(format!("> {}", cmd));
                }
            }

            // Help icons — open /learn documentation URL
            SlintAction::OpenLearnUrl(url) => {
                let help_opens_in_tab = res.state.as_ref().map(|s| s.help_opens_in_tab).unwrap_or(false);
                if help_opens_in_tab {
                    // Open in the tabbed viewer (same as OpenWebTab)
                    if let Some(ref mut s) = res.state { s.pending_open_web = Some(url.clone()); }
                } else {
                    // Open in system browser using cross-platform std::process::Command
                    let full_url = if url.starts_with("http") {
                        url.clone()
                    } else {
                        format!("https://eustress.dev{}", url)
                    };
                    #[cfg(target_os = "windows")]
                    let result = std::process::Command::new("cmd")
                        .args(["/c", "start", "", &full_url])
                        .spawn();
                    #[cfg(target_os = "macos")]
                    let result = std::process::Command::new("open").arg(&full_url).spawn();
                    #[cfg(target_os = "linux")]
                    let result = std::process::Command::new("xdg-open").arg(&full_url).spawn();
                    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
                    let result: Result<_, std::io::Error> = Err(std::io::Error::new(std::io::ErrorKind::Other, "unsupported"));
                    if let Err(e) = result {
                        if let Some(ref mut out) = res.output {
                            out.error(format!("Failed to open browser: {}", e));
                        }
                    }
                }
            }

            // Settings toggles that affect other panels
            SlintAction::ShowHelpIconsChanged(val) => {
                if let Some(ref mut s) = res.state { s.show_help_icons = val; }
            }
            SlintAction::HelpOpensInTabChanged(val) => {
                if let Some(ref mut s) = res.state { s.help_opens_in_tab = val; }
            }
            
            // Workshop Panel (System 0: Ideation) — dispatch to Bevy messages
            SlintAction::WorkshopSendMessage(text) => {
                events.workshop_send.write(crate::workshop::WorkshopSendMessageEvent { content: text });
            }
            SlintAction::WorkshopApproveMcp(id) => {
                events.workshop_approve.write(crate::workshop::WorkshopApproveMcpEvent { message_id: id as u32 });
            }
            SlintAction::WorkshopSkipMcp(id) => {
                events.workshop_skip.write(crate::workshop::WorkshopSkipMcpEvent { message_id: id as u32 });
            }
            SlintAction::WorkshopEditMcp(id) => {
                events.workshop_edit.write(crate::workshop::WorkshopEditMcpEvent { message_id: id as u32 });
            }
            SlintAction::WorkshopOpenArtifact(path) => {
                events.workshop_open_artifact.write(crate::workshop::WorkshopOpenArtifactEvent { path });
            }
            SlintAction::WorkshopStartPipeline => {
                // Start pipeline is implicit — happens when user sends first message
                info!("Workshop: Start pipeline requested");
            }
            SlintAction::WorkshopPausePipeline => {
                if let Some(ref mut pipeline) = res.workshop_pipeline {
                    if pipeline.state == crate::workshop::IdeationState::Conversing {
                        pipeline.state = crate::workshop::IdeationState::Paused;
                    }
                }
            }
            SlintAction::WorkshopResumePipeline => {
                if let Some(ref mut pipeline) = res.workshop_pipeline {
                    if pipeline.state == crate::workshop::IdeationState::Paused {
                        pipeline.state = crate::workshop::IdeationState::Conversing;
                    }
                }
            }
            SlintAction::WorkshopCancelPipeline => {
                if let Some(ref mut pipeline) = res.workshop_pipeline {
                    let _ = crate::workshop::persistence::save_session(pipeline);
                    pipeline.reset();
                    info!("Workshop: Pipeline cancelled, session reset");
                }
            }
            SlintAction::WorkshopOptimizeAndBuild => {
                let opt_event = res.workshop_pipeline.as_ref().and_then(|p| {
                    if p.state == crate::workshop::IdeationState::Complete {
                        p.output_dir.as_ref().map(|dir| {
                            crate::workshop::OptimizeAndBuildEvent {
                                product_name: p.product_name.clone(),
                                brief_path: dir.join("ideation_brief.toml"),
                                output_dir: dir.clone(),
                            }
                        })
                    } else {
                        None
                    }
                });
                if let Some(evt) = opt_event {
                    events.workshop_optimize.write(evt);
                    info!("Workshop: Optimize & Build fired for Systems 1-8");
                }
            }
            
            // Toolbox insertion - file-system-first: create .glb.toml → spawn inline
            SlintAction::InsertPart(part_type_str) => {
                // ── Model / Folder: create directory with _instance.toml ──
                if part_type_str == "Model" || part_type_str == "Folder" {
                    let space_root = crate::space::default_space_root();
                    let workspace_dir = space_root.join("Workspace");
                    let _ = std::fs::create_dir_all(&workspace_dir);

                    // Generate unique directory name
                    let base = part_type_str.clone();
                    let dir_name = {
                        let test = workspace_dir.join(&base);
                        if !test.exists() { base.clone() } else {
                            (1..1000).map(|i| format!("{}{}", base, i))
                                .find(|c| !workspace_dir.join(c).exists())
                                .unwrap_or_else(|| format!("{}_{}", base, chrono::Utc::now().timestamp()))
                        }
                    };
                    let dir_path = workspace_dir.join(&dir_name);
                    if let Err(e) = std::fs::create_dir_all(&dir_path) {
                        error!("Failed to create {} directory: {}", part_type_str, e);
                    } else {
                        let instance_toml = format!(
                            "[metadata]\nclass_name = \"{}\"\narchivable = true\n",
                            part_type_str
                        );
                        if let Err(e) = std::fs::write(dir_path.join("_instance.toml"), &instance_toml) {
                            error!("Failed to write _instance.toml: {}", e);
                        } else {
                            let class_enum = if part_type_str == "Model" {
                                eustress_common::classes::ClassName::Model
                            } else {
                                eustress_common::classes::ClassName::Folder
                            };
                            let entity = commands.spawn((
                                eustress_common::classes::Instance {
                                    name: dir_name.clone(),
                                    class_name: class_enum,
                                    archivable: true,
                                    id: 0,
                                    ai: false,
                                },
                                crate::space::file_loader::LoadedFromFile {
                                    path: dir_path.clone(),
                                    file_type: crate::space::FileType::Directory,
                                    service: "Workspace".to_string(),
                                },
                                Name::new(dir_name.clone()),
                                Transform::default(),
                                Visibility::default(),
                            )).id();
                            if let Some(ref mut registry) = res.file_registry {
                                registry.register(
                                    dir_path.clone(),
                                    entity,
                                    crate::space::FileMetadata {
                                        path: dir_path.clone(),
                                        file_type: crate::space::FileType::Directory,
                                        service: "Workspace".to_string(),
                                        name: dir_name.clone(),
                                        size: 0,
                                        modified: std::time::SystemTime::now(),
                                        children: Vec::new(),
                                    },
                                );
                            }
                            if let Some(ref mut out) = res.output {
                                out.info(format!("Created {} '{}'", part_type_str, dir_name));
                            }
                            info!("📁 Created {} '{}' at {:?}", part_type_str, dir_name, dir_path);
                        }
                    }
                    continue;
                }

                // ── Mesh primitives: create .glb.toml file ──
                let mesh_id = match part_type_str.as_str() {
                    "Part" | "Block" => "block",
                    "SpherePart" | "Ball" => "ball",
                    "CylinderPart" | "Cylinder" => "cylinder",
                    "WedgePart" | "Wedge" => "wedge",
                    "CornerWedgePart" | "CornerWedge" => "corner_wedge",
                    "Cone" => "cone",
                    _ => "block",
                };

                // Compute spawn position: 10 units in front of the camera, min Y = 0.5
                let spawn_pos: [f32; 3] = if let Some((_, cam_transform)) = queries.camera_query.iter().find(|(c, _)| c.order == 0) {
                    let forward = cam_transform.forward();
                    let cam_pos = cam_transform.translation();
                    let pos = cam_pos + forward * 10.0;
                    [pos.x, pos.y.max(0.5), pos.z]
                } else {
                    [0.0, 0.5, 0.0]
                };

                // Determine correct parent entity:
                // 1. If a Folder/Model entity is selected in explorer → parent to that
                // 2. Otherwise → parent to the Workspace service root entity
                let parent_entity: Option<Entity> = if let Some(ref es) = res.explorer_state {
                    match &es.selected {
                        SelectedItem::Entity(e) => {
                            // Only parent to Folder or Model — not to parts/scripts/services
                            queries.instances.get(*e).ok().and_then(|(ent, inst)| {
                                match inst.class_name {
                                    eustress_common::classes::ClassName::Folder
                                    | eustress_common::classes::ClassName::Model => Some(ent),
                                    _ => None,
                                }
                            })
                        }
                        _ => None,
                    }
                } else {
                    None
                };

                // Fall back to Workspace service root if no folder selected:
                // find the entity whose LoadedFromFile has service == "Workspace" and is a _service.toml
                let parent_entity = parent_entity.or_else(|| {
                    queries.loaded_from_file.iter().find_map(|(lff_entity, lff)| {
                        if lff.service == "Workspace"
                            && lff.path.file_name()
                                .map(|n| n.to_string_lossy().as_ref() == "_service.toml")
                                .unwrap_or(false)
                        {
                            Some(lff_entity)
                        } else {
                            None
                        }
                    })
                });

                // Space root path (uses dynamic default)
                let space_root = crate::space::default_space_root();

                // Step 1: Create .glb.toml instance file on disk in the correct directory.
                // If parented to a folder entity, write the file inside that folder's directory.
                let write_dir = parent_entity
                    .and_then(|pe| queries.loaded_from_file.get(pe).ok())
                    .map(|(_, lff)| {
                        // For Directory entries, path IS the directory; for _service.toml, use parent dir
                        if lff.path.is_dir() {
                            lff.path.clone()
                        } else {
                            lff.path.parent().unwrap_or(&space_root.join("Workspace")).to_path_buf()
                        }
                    })
                    .unwrap_or_else(|| space_root.join("Workspace"));

                match crate::toolbox::insert_mesh_instance_at(
                    &write_dir,
                    mesh_id,
                    spawn_pos,
                    None,
                ) {
                    Ok(toml_path) => {
                        // Step 2: Load the instance definition from the file we just wrote
                        match crate::space::load_instance_definition(&toml_path) {
                            Ok(instance) => {
                                // Step 3: Spawn entity inline
                                let mut default_mat_reg = crate::space::material_loader::MaterialRegistry::default();
                                let mat_reg_ref = res.material_registry.as_deref_mut().unwrap_or(&mut default_mat_reg);
                                let mut default_mesh_cache = crate::space::instance_loader::PrimitiveMeshCache::default();
                                let mesh_cache_ref = res.mesh_cache.as_deref_mut().unwrap_or(&mut default_mesh_cache);
                                let entity = crate::space::instance_loader::spawn_instance(
                                    &mut commands,
                                    &asset_server,
                                    &mut res.materials,
                                    mat_reg_ref,
                                    mesh_cache_ref,
                                    toml_path.clone(),
                                    instance,
                                );

                                // Step 4: Parent to selected folder / Workspace root
                                if let Some(parent) = parent_entity {
                                    commands.entity(entity).insert(ChildOf(parent));
                                }

                                // Step 5: Register in SpaceFileRegistry
                                if let Some(ref mut registry) = res.file_registry {
                                    let name = toml_path.file_stem()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("Unknown")
                                        .trim_end_matches(".glb")
                                        .to_string();
                                    registry.register(
                                        toml_path.clone(),
                                        entity,
                                        crate::space::FileMetadata {
                                            path: toml_path.clone(),
                                            file_type: crate::space::FileType::Toml,
                                            service: "Workspace".to_string(),
                                            name,
                                            size: 0,
                                            modified: std::time::SystemTime::now(),
                                            children: Vec::new(),
                                        },
                                    );
                                }

                                if let Some(ref mut out) = res.output {
                                    out.info(format!("Inserted {} at {:?}", part_type_str, toml_path));
                                }
                            }
                            Err(e) => {
                                error!("Failed to load instance definition: {}", e);
                                if let Some(ref mut out) = res.output {
                                    out.error(format!("Failed to load instance: {}", e));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to create instance file: {}", e);
                        if let Some(ref mut out) = res.output {
                            out.error(format!("Failed to insert part: {}", e));
                        }
                    }
                }
            }
            
            // Context menu
            SlintAction::ContextAction(action) => {
                match action.as_str() {
                    // File-specific actions
                    "open" => {
                        // Open file in appropriate editor (handled by OpenNode action)
                        if let Some(ref es) = res.explorer_state {
                            if let SelectedItem::File(ref path) = es.selected {
                                info!("Opening file: {}", path.display());
                                // File opening is handled by the OpenNode action
                            }
                        }
                    }
                    "open-with" => {
                        info!("Open With... dialog not yet implemented");
                    }
                    "show-in-explorer" => {
                        if let Some(ref es) = res.explorer_state {
                            if let SelectedItem::File(ref path) = es.selected {
                                #[cfg(target_os = "windows")]
                                {
                                    use std::process::Command;
                                    let _ = Command::new("explorer")
                                        .args(["/select,", &path.to_string_lossy()])
                                        .spawn();
                                }
                                #[cfg(target_os = "macos")]
                                {
                                    use std::process::Command;
                                    let _ = Command::new("open")
                                        .args(["-R", &path.to_string_lossy()])
                                        .spawn();
                                }
                                #[cfg(target_os = "linux")]
                                {
                                    use std::process::Command;
                                    if let Some(parent) = path.parent() {
                                        let _ = Command::new("xdg-open")
                                            .arg(parent)
                                            .spawn();
                                    }
                                }
                            }
                        }
                    }
                    "copy-path" => {
                        if let Some(ref es) = res.explorer_state {
                            if let SelectedItem::File(ref path) = es.selected {
                                #[cfg(feature = "clipboard")]
                                {
                                    use arboard::Clipboard;
                                    if let Ok(mut clipboard) = Clipboard::new() {
                                        let _ = clipboard.set_text(path.to_string_lossy().to_string());
                                        info!("Copied path to clipboard: {}", path.display());
                                    }
                                }
                                #[cfg(not(feature = "clipboard"))]
                                info!("Clipboard feature not enabled");
                            }
                        }
                    }
                    "copy-relative-path" => {
                        if let Some(ref es) = res.explorer_state {
                            if let SelectedItem::File(ref path) = es.selected {
                                let relative = path.strip_prefix(&es.project_root)
                                    .unwrap_or(path);
                                #[cfg(feature = "clipboard")]
                                {
                                    use arboard::Clipboard;
                                    if let Ok(mut clipboard) = Clipboard::new() {
                                        let _ = clipboard.set_text(relative.to_string_lossy().to_string());
                                        info!("Copied relative path to clipboard: {}", relative.display());
                                    }
                                }
                                #[cfg(not(feature = "clipboard"))]
                                info!("Clipboard feature not enabled");
                            }
                        }
                    }
                    "properties" => {
                        if let Some(ref es) = res.explorer_state {
                            if let SelectedItem::File(ref path) = es.selected {
                                info!("File properties for: {}", path.display());
                                // TODO: Show file properties dialog with size, modified date, etc.
                            }
                        }
                    }
                    
                    // Entity/file common actions
                    "cut" => {
                        events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Copy));
                        events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Delete));
                    }
                    "copy" => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Copy)); }
                    "paste" => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Paste)); }
                    "delete" => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Delete)); }
                    "duplicate" => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Duplicate)); }
                    "select_all" => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::SelectAll)); }
                    "group" => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Group)); }
                    "ungroup" => { events.menu_events.write(MenuActionEvent::new(crate::keybindings::Action::Ungroup)); }
                    "rename" => {
                        // TODO: Trigger inline rename in explorer panel
                    }
                    "insert" => {
                        // TODO: Open insert submenu or switch to toolbox tab
                    }
                    _ => {}
                }
            }
            
            // Ribbon menu actions — route insert:* to InsertPart or log unsupported
            SlintAction::MenuAction(action) => {
                let action = action.as_str();

                // Model / Folder inserts → create directory with _instance.toml
                if action == "insert:model" || action == "insert:folder" {
                    let part_type_str = if action == "insert:model" { "Model" } else { "Folder" };
                    let space_root = crate::space::default_space_root();
                    let workspace_dir = space_root.join("Workspace");
                    let _ = std::fs::create_dir_all(&workspace_dir);
                    let base = part_type_str.to_string();
                    let dir_name = {
                        let test = workspace_dir.join(&base);
                        if !test.exists() { base.clone() } else {
                            (1..1000).map(|i| format!("{}{}", base, i))
                                .find(|c| !workspace_dir.join(c).exists())
                                .unwrap_or_else(|| format!("{}_{}", base, chrono::Utc::now().timestamp()))
                        }
                    };
                    let dir_path = workspace_dir.join(&dir_name);
                    if let Err(e) = std::fs::create_dir_all(&dir_path) {
                        error!("Failed to create {} directory: {}", part_type_str, e);
                    } else {
                        let instance_toml = format!(
                            "[metadata]\nclass_name = \"{}\"\narchivable = true\n",
                            part_type_str
                        );
                        if let Err(e) = std::fs::write(dir_path.join("_instance.toml"), &instance_toml) {
                            error!("Failed to write _instance.toml: {}", e);
                        } else {
                            let class_enum = if part_type_str == "Model" {
                                eustress_common::classes::ClassName::Model
                            } else {
                                eustress_common::classes::ClassName::Folder
                            };
                            let entity = commands.spawn((
                                eustress_common::classes::Instance {
                                    name: dir_name.clone(),
                                    class_name: class_enum,
                                    archivable: true,
                                    id: 0,
                                    ai: false,
                                },
                                crate::space::file_loader::LoadedFromFile {
                                    path: dir_path.clone(),
                                    file_type: crate::space::FileType::Directory,
                                    service: "Workspace".to_string(),
                                },
                                Name::new(dir_name.clone()),
                                Transform::default(),
                                Visibility::default(),
                            )).id();
                            if let Some(ref mut registry) = res.file_registry {
                                registry.register(dir_path.clone(), entity, crate::space::FileMetadata {
                                    path: dir_path.clone(),
                                    file_type: crate::space::FileType::Directory,
                                    service: "Workspace".to_string(),
                                    name: dir_name.clone(),
                                    size: 0,
                                    modified: std::time::SystemTime::now(),
                                    children: Vec::new(),
                                });
                            }
                            if let Some(ref mut out) = res.output {
                                out.info(format!("Created {} '{}'", part_type_str, dir_name));
                            }
                            info!("📁 Created {} '{}' at {:?}", part_type_str, dir_name, dir_path);
                        }
                    }
                    continue;
                }

                // Primitive / structure inserts → reuse the full InsertPart pipeline
                let mesh_id: Option<&str> = match action {
                    "insert:part"            => Some("block"),
                    "insert:sphere"          => Some("ball"),
                    "insert:cylinder"        => Some("cylinder"),
                    "insert:wedge"           => Some("wedge"),
                    "insert:cone"            => Some("cone"),
                    "insert:corner_wedge"    => Some("corner_wedge"),
                    "insert:spawnlocation"   => Some("block"),
                    "insert:seat"            => Some("block"),
                    "insert:vehicleseat"     => Some("block"),
                    "insert:unionoperation"  => Some("block"),
                    _ => None,
                };

                if let Some(mid) = mesh_id {
                    // Camera-forward spawn position (same logic as InsertPart)
                    let spawn_pos: [f32; 3] = if let Some((_, cam_transform)) = queries.camera_query.iter().find(|(c, _)| c.order == 0) {
                        let forward = cam_transform.forward();
                        let cam_pos  = cam_transform.translation();
                        let pos = cam_pos + forward * 10.0;
                        [pos.x, pos.y.max(0.5), pos.z]
                    } else {
                        [0.0, 0.5, 0.0]
                    };

                    // Parent entity: selected Folder/Model or Workspace service root
                    let parent_entity: Option<Entity> = if let Some(ref es) = res.explorer_state {
                        match &es.selected {
                            SelectedItem::Entity(e) => {
                                queries.instances.get(*e).ok().and_then(|(ent, inst)| {
                                    match inst.class_name {
                                        eustress_common::classes::ClassName::Folder
                                        | eustress_common::classes::ClassName::Model => Some(ent),
                                        _ => None,
                                    }
                                })
                            }
                            _ => None,
                        }
                    } else { None };

                    let parent_entity = parent_entity.or_else(|| {
                        queries.loaded_from_file.iter().find_map(|(lff_entity, lff)| {
                            if lff.service == "Workspace"
                                && lff.path.file_name()
                                    .map(|n| n.to_string_lossy().as_ref() == "_service.toml")
                                    .unwrap_or(false)
                            {
                                Some(lff_entity)
                            } else {
                                None
                            }
                        })
                    });

                    let space_root = crate::space::default_space_root();
                    let write_dir = parent_entity
                        .and_then(|pe| queries.loaded_from_file.get(pe).ok())
                        .map(|(_, lff)| {
                            if lff.path.is_dir() { lff.path.clone() }
                            else { lff.path.parent().unwrap_or(&space_root.join("Workspace")).to_path_buf() }
                        })
                        .unwrap_or_else(|| space_root.join("Workspace"));

                    match crate::toolbox::insert_mesh_instance_at(&write_dir, mid, spawn_pos, None) {
                        Ok(toml_path) => {
                            match crate::space::load_instance_definition(&toml_path) {
                                Ok(instance) => {
                                    let mut default_mat_reg2 = crate::space::material_loader::MaterialRegistry::default();
                                    let mat_reg_ref2 = res.material_registry.as_deref_mut().unwrap_or(&mut default_mat_reg2);
                                    let mut default_mesh_cache2 = crate::space::instance_loader::PrimitiveMeshCache::default();
                                    let mesh_cache_ref2 = res.mesh_cache.as_deref_mut().unwrap_or(&mut default_mesh_cache2);
                                    let entity = crate::space::instance_loader::spawn_instance(
                                        &mut commands,
                                        &asset_server,
                                        &mut res.materials,
                                        mat_reg_ref2,
                                        mesh_cache_ref2,
                                        toml_path.clone(),
                                        instance,
                                    );
                                    if let Some(parent) = parent_entity {
                                        commands.entity(entity).insert(ChildOf(parent));
                                    }
                                    if let Some(ref mut registry) = res.file_registry {
                                        let name = toml_path.file_stem()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("Unknown")
                                            .trim_end_matches(".glb")
                                            .to_string();
                                        registry.register(
                                            toml_path.clone(),
                                            entity,
                                            crate::space::FileMetadata {
                                                path: toml_path.clone(),
                                                file_type: crate::space::FileType::Toml,
                                                service: "Workspace".to_string(),
                                                name,
                                                size: 0,
                                                modified: std::time::SystemTime::now(),
                                                children: Vec::new(),
                                            },
                                        );
                                    }
                                    if let Some(ref mut out) = res.output {
                                        out.info(format!("Inserted {} via {:?}", mid, toml_path));
                                    }
                                }
                                Err(e) => {
                                    error!("MenuAction insert load error: {}", e);
                                    if let Some(ref mut out) = res.output {
                                        out.error(format!("Insert failed: {}", e));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("MenuAction insert file error: {}", e);
                            if let Some(ref mut out) = res.output {
                                out.error(format!("Insert failed: {}", e));
                            }
                        }
                    }
                } else {
                    // UI element inserts — map action string → (class_name, file_extension, service)
                    let ui_insert: Option<(&str, &str, &str)> = match action {
                        "insert:screengui"    => Some(("ScreenGui",      "screengui",      "StarterGui")),
                        "insert:billboardgui" => Some(("BillboardGui",   "billboardgui",   "StarterGui")),
                        "insert:surfacegui"   => Some(("SurfaceGui",     "surfacegui",     "StarterGui")),
                        "insert:frame"        => Some(("Frame",          "frame",          "StarterGui")),
                        "insert:scrollingframe" => Some(("ScrollingFrame", "scrollingframe", "StarterGui")),
                        "insert:textlabel"    => Some(("TextLabel",      "textlabel",      "StarterGui")),
                        "insert:imagelabel"   => Some(("ImageLabel",     "imagelabel",     "StarterGui")),
                        "insert:textbutton"   => Some(("TextButton",     "textbutton",     "StarterGui")),
                        "insert:imagebutton"  => Some(("ImageButton",    "imagebutton",    "StarterGui")),
                        "insert:textbox"      => Some(("TextBox",        "textbox",        "StarterGui")),
                        "insert:viewportframe" => Some(("ViewportFrame", "viewportframe",  "StarterGui")),
                        "insert:videoframe"   => Some(("VideoFrame",     "videoframe",     "StarterGui")),
                        "insert:documentframe" => Some(("DocumentFrame", "documentframe",  "StarterGui")),
                        "insert:webframe"     => Some(("WebFrame",       "webframe",       "StarterGui")),
                        _ => None,
                    };

                    if let Some((class_name_str, file_ext, service)) = ui_insert {
                        // Check if a ScreenGui (or other GUI container) is selected — parent UI children there
                        let selected_gui_entity: Option<Entity> = if let Some(ref es) = res.explorer_state {
                            match &es.selected {
                                SelectedItem::Entity(e) => {
                                    queries.instances.get(*e).ok().and_then(|(ent, inst)| {
                                        match inst.class_name {
                                            eustress_common::classes::ClassName::ScreenGui
                                            | eustress_common::classes::ClassName::BillboardGui
                                            | eustress_common::classes::ClassName::SurfaceGui
                                            | eustress_common::classes::ClassName::Frame
                                            | eustress_common::classes::ClassName::ScrollingFrame => Some(ent),
                                            _ => None,
                                        }
                                    })
                                }
                                _ => None,
                            }
                        } else { None };

                        // Find the service entity (StarterGui root) as fallback
                        let service_entity = queries.loaded_from_file.iter().find_map(|(e, lff)| {
                            if lff.service == service
                                && lff.path.file_name()
                                    .map(|n| n.to_string_lossy().as_ref() == "_service.toml")
                                    .unwrap_or(false)
                            {
                                Some(e)
                            } else {
                                None
                            }
                        });

                        // Prefer selected GUI container, fall back to service root
                        let parent_entity = selected_gui_entity.or(service_entity);

                        let space_root = crate::space::default_space_root();
                        let write_dir = parent_entity
                            .and_then(|pe| queries.loaded_from_file.get(pe).ok())
                            .map(|(_, lff)| {
                                if lff.path.is_dir() { lff.path.clone() }
                                else { lff.path.parent().unwrap_or(&space_root.join(service)).to_path_buf() }
                            })
                            .unwrap_or_else(|| space_root.join(service));

                        // Generate unique file name
                        let base_name = class_name_str;
                        let instance_name = (0..1000u32).find_map(|i| {
                            let candidate = if i == 0 {
                                base_name.to_string()
                            } else {
                                format!("{}{}", base_name, i)
                            };
                            let path = write_dir.join(format!("{}.{}.toml", candidate, file_ext));
                            if !path.exists() { Some(candidate) } else { None }
                        }).unwrap_or_else(|| format!("{}_new", base_name));

                        // Create GUI TOML with proper [instance]/[gui]/[text] format
                        let gui_def = crate::space::gui_loader::create_default_gui_toml(
                            class_name_str,
                            &instance_name,
                        );

                        let _ = std::fs::create_dir_all(&write_dir);
                        let toml_path = write_dir.join(format!("{}.{}.toml", instance_name, file_ext));

                        // Write GUI TOML to disk, then load + spawn with Bevy UI components
                        match crate::space::gui_loader::write_gui_toml(&toml_path, &gui_def) {
                            Ok(()) => {
                                let entity = crate::space::gui_loader::spawn_gui_element(
                                    &mut commands,
                                    &toml_path,
                                    &gui_def,
                                );
                                if let Some(parent) = parent_entity {
                                    commands.entity(entity).insert(ChildOf(parent));
                                }
                                if let Some(ref mut registry) = res.file_registry {
                                    registry.register(
                                        toml_path.clone(),
                                        entity,
                                        crate::space::FileMetadata {
                                            path: toml_path.clone(),
                                            file_type: crate::space::FileType::GuiElement,
                                            service: service.to_string(),
                                            name: instance_name.clone(),
                                            size: 0,
                                            modified: std::time::SystemTime::now(),
                                            children: Vec::new(),
                                        },
                                    );
                                }
                                if let Some(ref mut out) = res.output {
                                    out.info(format!("Inserted {} as {:?}", class_name_str, toml_path));
                                }
                            }
                            Err(e) => {
                                error!("UI insert write error for {}: {}", class_name_str, e);
                                if let Some(ref mut out) = res.output {
                                    out.error(format!("Insert {} failed: {}", class_name_str, e));
                                }
                            }
                        }
                    } else if action == "terrain:clear" {
                        // Clear terrain: delete Workspace/Terrain directory and despawn all terrain entities
                        let space_root = crate::space::default_space_root();
                        let terrain_dir = space_root.join("Workspace").join("Terrain");
                        
                        // Delete terrain directory and all its contents
                        if terrain_dir.exists() {
                            match std::fs::remove_dir_all(&terrain_dir) {
                                Ok(()) => {
                                    if let Some(ref mut out) = res.output {
                                        out.info("Deleted Workspace/Terrain directory");
                                    }
                                    info!("🗑️ Deleted terrain directory: {:?}", terrain_dir);
                                }
                                Err(e) => {
                                    error!("Failed to delete terrain directory: {}", e);
                                    if let Some(ref mut out) = res.output {
                                        out.error(format!("Failed to delete terrain files: {}", e));
                                    }
                                }
                            }
                        }
                        
                        // Despawn all terrain entities (TerrainRoot and Chunks)
                        let terrain_count = queries.terrain_roots.iter().count();
                        let chunk_count = queries.terrain_chunks.iter().count();
                        
                        for entity in queries.terrain_roots.iter() {
                            commands.entity(entity).despawn();
                        }
                        
                        for entity in queries.terrain_chunks.iter() {
                            commands.entity(entity).despawn();
                        }
                        
                        if let Some(ref mut out) = res.output {
                            out.info(format!("Cleared {} terrain entities", terrain_count + chunk_count));
                        }
                        info!("🗑️ Despawned {} terrain roots and {} chunks", terrain_count, chunk_count);
                        
                        // Mark explorer as dirty to refresh the tree
                        if let Some(ref mut es) = res.explorer_state {
                            es.dirty = true;
                        }
                    } else {
                        // Other menu actions (model:negate, edit:lock, etc.) — log unhandled
                        if let Some(ref mut out) = res.output {
                            if action.starts_with("insert:") {
                                out.info(format!("TODO: insert {}", &action["insert:".len()..]));
                            }
                        }
                    }
                }
            }

            // Asset Manager actions
            SlintAction::AssetSelect(id) => {
                if let Some(mut am) = res.asset_manager_state.as_mut() {
                    am.selected = Some(id);
                    am.dirty = true;
                }
            }
            SlintAction::AssetExpand(id) => {
                if let Some(mut am) = res.asset_manager_state.as_mut() {
                    if am.expanded.contains(&id) {
                        am.expanded.remove(&id);
                    } else {
                        am.expanded.insert(id);
                    }
                    am.dirty = true;
                }
            }
            SlintAction::AssetImport(_kind) => {
                info!("Asset import not yet implemented");
            }
            SlintAction::AssetSearch(text) => {
                if let Some(mut am) = res.asset_manager_state.as_mut() {
                    am.search = text;
                    am.dirty = true;
                }
            }
            SlintAction::AssetCategoryChanged(cat) => {
                if let Some(mut am) = res.asset_manager_state.as_mut() {
                    am.category = cat;
                    am.dirty = true;
                }
            }

            // Close
            SlintAction::CloseRequested => {
                if let Some(ref mut s) = res.state {
                    if s.has_unsaved_changes {
                        s.show_exit_confirmation = true;
                    } else {
                        events.exit_events.write(bevy::app::AppExit::Success);
                    }
                } else {
                    events.exit_events.write(bevy::app::AppExit::Success);
                }
            }
            SlintAction::ForceExit => {
                events.exit_events.write(bevy::app::AppExit::Success);
            }
        }
    }
}

/// Pushes Bevy state to Slint properties each frame (Bevy→Slint direction).
/// Updates tool selection, play state, FPS, panel visibility, output logs, etc.
fn sync_bevy_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    state: Option<ResMut<StudioState>>,
    perf: Option<Res<UIPerformance>>,
    output: Option<Res<OutputConsole>>,
    editor_settings: Option<Res<crate::editor_settings::EditorSettings>>,
    auth_state: Option<Res<crate::auth::AuthState>>,
    mut viewport_bounds: Option<ResMut<super::ViewportBounds>>,
    snapshot: Option<Res<UIWorldSnapshot>>,
    // Direct entity count query as fallback when snapshot is empty
    instance_query: Query<Entity, With<eustress_common::classes::Instance>>,
    play_mode_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
    // Terrain state sync
    terrain_roots: Query<Entity, With<eustress_common::terrain::TerrainRoot>>,
    terrain_config: Query<&eustress_common::terrain::TerrainConfig, With<eustress_common::terrain::TerrainRoot>>,
    terrain_chunks: Query<Entity, With<eustress_common::terrain::Chunk>>,
    terrain_mode: Option<Res<eustress_common::terrain::TerrainMode>>,
    terrain_brush: Option<Res<eustress_common::terrain::TerrainBrush>>,
) {
    let Some(slint_context) = slint_context else { return };
    let ui = &slint_context.window;
    
    // ── Per-frame: viewport bounds (needed by camera every frame) ──
    if let Some(ref mut vb) = viewport_bounds {
        let scale = slint_context.adapter.scale_factor.get();
        let vw = ui.get_viewport_width() * scale;
        let vh = ui.get_viewport_height() * scale;
        let vx = ui.get_viewport_x() * scale;
        let vy = ui.get_viewport_y() * scale;
        if vw > 0.0 && vh > 0.0 {
            vb.x = vx;
            vb.y = vy;
            vb.width = vw;
            vb.height = vh;
        }
    }
    
    // ── Per-frame: FPS / frame time ──
    // Only update when value changes by >= 1.0 to avoid marking Slint dirty every frame.
    // FPS fluctuates constantly; displaying sub-integer precision is unnecessary noise.
    if let Some(ref perf) = perf {
        let current_fps = ui.get_current_fps();
        let current_frame_time = ui.get_current_frame_time();
        let new_fps = perf.fps.round();
        let new_frame_time = (perf.avg_frame_time_ms * 10.0).round() / 10.0;
        if (new_fps - current_fps).abs() >= 1.0 {
            ui.set_current_fps(new_fps);
        }
        if (new_frame_time - current_frame_time).abs() >= 0.5 {
            ui.set_current_frame_time(new_frame_time);
        }
    }
    
    // ── Per-frame: play mode state (only update when changed) ──
    if let Some(ref pms) = play_mode_state {
        let play_state_str = match pms.get() {
            crate::play_mode::PlayModeState::Playing => "playing",
            crate::play_mode::PlayModeState::Paused  => "paused",
            crate::play_mode::PlayModeState::Editing => "stopped",
        };
        let current_play_state: String = ui.get_play_state().into();
        if current_play_state != play_state_str {
            ui.set_play_state(play_state_str.into());
        }
    }

    // ── Pre-throttle: tool and transform mode must sync every frame for responsiveness ──
    if let Some(ref state) = state {
        let tool_str = match state.current_tool {
            Tool::Select => "select",
            Tool::Move => "move",
            Tool::Rotate => "rotate",
            Tool::Scale => "scale",
            Tool::Terrain => "terrain",
        };
        let current_tool: String = ui.get_current_tool().into();
        if current_tool != tool_str {
            ui.set_current_tool(tool_str.into());
        }
        
        let mode_str = match state.transform_mode {
            TransformMode::World => "world",
            TransformMode::Local => "local",
        };
        let current_mode: String = ui.get_transform_mode().into();
        if current_mode != mode_str {
            ui.set_transform_mode(mode_str.into());
        }
    }

    // ── Throttled (every 10 frames): everything that rarely changes ──
    // Avoids marking Slint dirty every frame for stable state, which forces
    // the software renderer to repaint the full overlay every frame.
    if let Some(ref perf) = perf {
        if perf.should_throttle(10) { return; }
    }

    // Entity count
    let entity_count = if let Some(ref snapshot) = snapshot {
        let count = snapshot.entities.len();
        if count > 0 { count } else { instance_query.iter().count() }
    } else {
        instance_query.iter().count()
    };
    
    // Get mutable reference to state
    let Some(mut state) = state else { return };
    
    // Boolean flags - only set when changed
    if ui.get_show_exit_confirmation() != state.show_exit_confirmation {
        ui.set_show_exit_confirmation(state.show_exit_confirmation);
    }
    if ui.get_has_unsaved_changes() != state.has_unsaved_changes {
        ui.set_has_unsaved_changes(state.has_unsaved_changes);
    }
    if ui.get_show_network_panel() != state.show_network_panel {
        ui.set_show_network_panel(state.show_network_panel);
    }
    if ui.get_show_terrain_editor() != state.show_terrain_editor {
        ui.set_show_terrain_editor(state.show_terrain_editor);
    }
    let has_terrain = !terrain_roots.is_empty();
    if ui.get_has_terrain() != has_terrain {
        ui.set_has_terrain(has_terrain);
    }
    // Terrain mode
    if let Some(ref tm) = terrain_mode {
        let terrain_edit = **tm == eustress_common::terrain::TerrainMode::Editor;
        if ui.get_terrain_edit_mode() != terrain_edit {
            ui.set_terrain_edit_mode(terrain_edit);
        }
    }
    // Terrain brush
    if let Some(ref tb) = terrain_brush {
        let brush_str = match tb.mode {
            eustress_common::terrain::BrushMode::Raise => "raise",
            eustress_common::terrain::BrushMode::Lower => "lower",
            eustress_common::terrain::BrushMode::Smooth => "smooth",
            eustress_common::terrain::BrushMode::Flatten => "flatten",
            eustress_common::terrain::BrushMode::PaintTexture => "paint",
            eustress_common::terrain::BrushMode::VoxelAdd => "voxeladd",
            eustress_common::terrain::BrushMode::VoxelRemove => "voxelremove",
            eustress_common::terrain::BrushMode::VoxelSmooth => "voxelsmooth",
            eustress_common::terrain::BrushMode::Region => "region",
            eustress_common::terrain::BrushMode::Fill => "fill",
        };
        let current_brush: String = ui.get_terrain_brush().into();
        if current_brush != brush_str {
            ui.set_terrain_brush(brush_str.into());
        }
    }
    // Terrain config - only sync when values change
    if let Ok(config) = terrain_config.single() {
        let (total_w, _total_d) = config.total_size();
        let size_str = format!("{:.0}", total_w);
        let current_size: String = ui.get_terrain_size().into();
        if current_size != size_str {
            ui.set_terrain_size(size_str.into());
            ui.set_terrain_chunk_size(format!("{:.0}", config.chunk_size).into());
            ui.set_terrain_resolution(format!("{}", config.chunk_resolution).into());
            ui.set_terrain_height_scale(format!("{:.1}", config.height_scale).into());
            ui.set_terrain_lod_levels(format!("{}", config.lod_levels).into());
            ui.set_terrain_material("Default".into());
        }
    }
    let chunk_count_str = format!("{}", terrain_chunks.iter().count());
    let current_chunk_count: String = ui.get_terrain_chunk_count().into();
    if current_chunk_count != chunk_count_str {
        ui.set_terrain_chunk_count(chunk_count_str.into());
    }
    // Mindspace panel
    if ui.get_show_mindspace_panel() != state.mindspace_panel_visible {
        ui.set_show_mindspace_panel(state.mindspace_panel_visible);
    }
    // Help icon settings
    if ui.get_show_help_icons() != state.show_help_icons {
        ui.set_show_help_icons(state.show_help_icons);
    }
    if ui.get_help_opens_in_tab() != state.help_opens_in_tab {
        ui.set_help_opens_in_tab(state.help_opens_in_tab);
    }
    
    // EditorSettings - only sync when changed
    if let Some(ref es) = editor_settings {
        if ui.get_snap_enabled() != es.snap_enabled {
            ui.set_snap_enabled(es.snap_enabled);
        }
        if (ui.get_snap_size() - es.snap_size).abs() > 0.001 {
            ui.set_snap_size(es.snap_size);
        }
        if ui.get_grid_visible() != es.show_grid {
            ui.set_grid_visible(es.show_grid);
        }
        if (ui.get_grid_size() - es.grid_size).abs() > 0.001 {
            ui.set_grid_size(es.grid_size);
        }
    }

    // Account state - only sync when changed
    let (account_name, account_status, sync_status) = if let Some(ref auth) = auth_state {
        let name = auth.user
            .as_ref()
            .map(|user| user.username.clone())
            .unwrap_or_else(|| "Guest".to_string());
        let status = if auth.is_offline() {
            "Offline"
        } else if auth.is_logged_in() {
            "Online"
        } else {
            "Logged out"
        };
        let sync = if auth.is_offline() {
            "Offline local"
        } else if auth.can_publish() {
            "Cloud sync ready"
        } else {
            "Local-first"
        };
        (name, status, sync)
    } else {
        ("Guest".to_string(), "Logged out", "Local-first")
    };
    let current_account: String = ui.get_account_name().into();
    if current_account != account_name {
        ui.set_account_name(account_name.into());
        ui.set_account_status(account_status.into());
        ui.set_sync_status(sync_status.into());
    }
    
    // Sync entity count - only when changed
    let current_entity_count = ui.get_current_entity_count();
    if entity_count as i32 != current_entity_count {
        ui.set_current_entity_count(entity_count as i32);
    }
    
    // Sync output console logs → Slint (last 200 entries)
    // Only rebuild the model when log count changes to avoid flickering
    if let Some(ref output) = output {
        let new_log_count = output.entries.len();
        if new_log_count != state.last_log_count {
            state.last_log_count = new_log_count;
            let log_model: Vec<LogData> = output.entries.iter().enumerate().map(|(i, entry)| {
                LogData {
                    id: i as i32,
                    level: match entry.level {
                        LogLevel::Info => "info".into(),
                        LogLevel::Warn => "warning".into(),
                        LogLevel::Error => "error".into(),
                        LogLevel::Debug => "debug".into(),
                    },
                    timestamp: entry.timestamp.clone().into(),
                    message: entry.message.clone().into(),
                    source: slint::SharedString::default(),
                }
            }).collect();
            let model_rc = std::rc::Rc::new(slint::VecModel::from(log_model));
            ui.set_output_logs(slint::ModelRc::from(model_rc));
        }
    }
    
    // Workshop Panel sync is handled by the dedicated sync_workshop_to_slint system
    // (runs on its own schedule to avoid adding workshop as a parameter here)
}

/// Syncs IdeationPipeline state to the Workshop Panel Slint properties.
/// Only runs when the pipeline resource has actually changed (Bevy change detection).
fn sync_workshop_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    pipeline: Option<Res<crate::workshop::IdeationPipeline>>,
    global_settings: Option<Res<crate::soul::GlobalSoulSettings>>,
    space_settings: Option<Res<crate::soul::SoulServiceSettings>>,
) {
    let Some(slint_context) = slint_context else { return };
    let Some(pipeline) = pipeline else { return };
    // Only sync when the pipeline resource was actually mutated this frame
    if !pipeline.is_changed() { return; }
    let ui = &slint_context.window;
    
    // Push pipeline state string
    ui.set_workshop_pipeline_state(pipeline.state_string().into());
    ui.set_workshop_product_name(pipeline.product_name.clone().into());
    ui.set_workshop_total_artifacts(pipeline.artifacts.len() as i32);
    ui.set_workshop_estimated_cost(pipeline.format_cost().into());
    
    // Check API key validity
    let api_key_valid = match (&global_settings, &space_settings) {
        (Some(global), Some(space)) => !space.effective_api_key(global).is_empty(),
        _ => false,
    };
    ui.set_workshop_api_key_valid(api_key_valid);
    
    // Push chat messages to Slint model
    let messages: Vec<ChatMessage> = pipeline.messages.iter().map(|msg| {
        ChatMessage {
            id: msg.id as i32,
            role: msg.role.to_slint_string().into(),
            content: msg.content.clone().into(),
            timestamp: msg.timestamp.clone().into(),
            mcp_endpoint: msg.mcp_endpoint.clone().unwrap_or_default().into(),
            mcp_method: msg.mcp_method.clone().unwrap_or_default().into(),
            mcp_status: msg.mcp_status.as_ref()
                .map(|s| s.to_slint_string().to_string())
                .unwrap_or_default()
                .into(),
            artifact_path: msg.artifact_path.as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
                .into(),
            artifact_type: msg.artifact_type.as_ref()
                .map(|t| t.to_slint_string().to_string())
                .unwrap_or_default()
                .into(),
        }
    }).collect();
    let msg_model = std::rc::Rc::new(slint::VecModel::from(messages));
    ui.set_workshop_messages(slint::ModelRc::from(msg_model));
    
    // Push pipeline steps to Slint model
    let steps: Vec<PipelineStepData> = pipeline.steps.iter().map(|step| {
        PipelineStepData {
            index: step.index as i32,
            label: step.label.clone().into(),
            status: step.status.to_slint_string().into(),
            artifact_count: step.artifact_count as i32,
        }
    }).collect();
    let steps_model = std::rc::Rc::new(slint::VecModel::from(steps));
    ui.set_workshop_pipeline_steps(slint::ModelRc::from(steps_model));
}

/// Tracks last known window size to detect resize (Changed<Window> is unreliable)
#[derive(Resource, Default)]
struct LastWindowSize {
    width: u32,
    height: u32,
    scale_factor: f32,
}

/// Handles window resize: updates Slint texture, overlay quad, and overlay camera.
/// All dimensions use PHYSICAL pixels to match the actual framebuffer size.
/// Runs every frame and compares against last known size (Changed<Window> is unreliable).
fn handle_window_resize(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut last_size: ResMut<LastWindowSize>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    slint_context: Option<NonSend<SlintUiState>>,
    slint_scenes: Query<&SlintScene>,
    mut overlay_quads: Query<&mut Mesh3d, With<SlintOverlaySprite>>,
    mut overlay_cameras: Query<(&mut Camera, &mut Projection), With<SlintOverlayCamera>>,
    mut staging: ResMut<SlintStagingBuffer>,
) {
    let Some(window) = windows.iter().next() else { return };
    let Some(slint_context) = slint_context else { return };
    
    // Use PHYSICAL pixels — must match framebuffer, texture, and Slint PhysicalSize
    let new_width = window.physical_width();
    let new_height = window.physical_height();
    let scale_factor = window.scale_factor();
    if new_width == 0 || new_height == 0 { return; }
    
    // Skip if nothing changed
    if new_width == last_size.width && new_height == last_size.height && scale_factor == last_size.scale_factor {
        return;
    }
    last_size.width = new_width;
    last_size.height = new_height;
    last_size.scale_factor = scale_factor;
    
    // Resize Slint adapter (PhysicalSize = physical pixels)
    slint_context.adapter.resize(
        slint::PhysicalSize::new(new_width, new_height),
        scale_factor,
    );
    
    // Resize the staging buffer to match new dimensions
    let new_pixel_count = (new_width * new_height) as usize;
    staging.pixels.resize(new_pixel_count, PremultipliedRgbaColor::default());
    staging.pixels.fill(PremultipliedRgbaColor::default());
    staging.width = new_width as usize;
    staging.height = new_height as usize;
    
    // Resize the Slint texture to match physical framebuffer
    if let Some(scene) = slint_scenes.iter().next() {
        if let Some(image) = images.get_mut(&scene.image) {
            let new_size = Extent3d {
                width: new_width,
                height: new_height,
                depth_or_array_layers: 1,
            };
            image.texture_descriptor.size = new_size;
            image.resize(new_size);
        }
    }
    
    // Resize the overlay quad mesh (physical pixels for orthographic projection)
    for mut mesh3d in overlay_quads.iter_mut() {
        let new_quad = Rectangle::new(new_width as f32, new_height as f32);
        mesh3d.0 = meshes.add(new_quad);
    }
    
    // Update overlay camera projection (physical pixels to match quad and texture)
    for (mut _camera, mut projection) in overlay_cameras.iter_mut() {
        *projection = Projection::from(OrthographicProjection {
            near: -1.0,
            far: 10.0,
            scaling_mode: ScalingMode::Fixed {
                width: new_width as f32,
                height: new_height as f32,
            },
            ..OrthographicProjection::default_3d()
        });
    }
    
    info!("🔄 Window resized to {}x{} physical (scale={})", new_width, new_height, scale_factor);
}

/// Forwards Bevy keyboard events to Slint for text input and key handling.
/// Skips modifier-only keys and Alt+key combos (those are engine shortcuts, not text input).
fn forward_keyboard_to_slint(
    mut key_events: MessageReader<bevy::input::keyboard::KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    slint_context: Option<NonSend<SlintUiState>>,
) {
    let Some(slint_context) = slint_context else { return };
    let adapter = &slint_context.adapter;
    
    // Check if Alt is held — if so, skip forwarding to Slint (engine shortcuts take priority)
    let alt_held = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    
    for event in key_events.read() {
        // Skip bare modifier keys — Slint doesn't need them for text input
        match &event.logical_key {
            bevy::input::keyboard::Key::Alt
            | bevy::input::keyboard::Key::Control
            | bevy::input::keyboard::Key::Shift
            | bevy::input::keyboard::Key::Super => continue,
            _ => {}
        }
        
        // Skip Alt+key combos — these are engine shortcuts (Alt+Z/X/C/V for tools)
        if alt_held { continue; }
        
        let text = convert_key_to_slint_text(&event.logical_key);
        if text.is_empty() { continue; }
        
        match event.state {
            ButtonState::Pressed => {
                adapter.slint_window.dispatch_event(
                    WindowEvent::KeyPressed { text: text.clone() },
                );
            }
            ButtonState::Released => {
                adapter.slint_window.dispatch_event(
                    WindowEvent::KeyReleased { text },
                );
            }
        }
    }
}

/// Convert Bevy logical key to Slint key text representation.
/// Uses slint::platform::Key enum which converts to SharedString via Into.
fn convert_key_to_slint_text(key: &bevy::input::keyboard::Key) -> slint::SharedString {
    use bevy::input::keyboard::Key as BevyKey;
    use slint::platform::Key as SlintKey;
    match key {
        BevyKey::Character(c) => c.as_str().into(),
        BevyKey::Space => " ".into(),
        BevyKey::Enter => SlintKey::Return.into(),
        BevyKey::Tab => SlintKey::Tab.into(),
        BevyKey::Escape => SlintKey::Escape.into(),
        BevyKey::Backspace => SlintKey::Backspace.into(),
        BevyKey::Delete => SlintKey::Delete.into(),
        BevyKey::ArrowUp => SlintKey::UpArrow.into(),
        BevyKey::ArrowDown => SlintKey::DownArrow.into(),
        BevyKey::ArrowLeft => SlintKey::LeftArrow.into(),
        BevyKey::ArrowRight => SlintKey::RightArrow.into(),
        BevyKey::Home => SlintKey::Home.into(),
        BevyKey::End => SlintKey::End.into(),
        BevyKey::PageUp => SlintKey::PageUp.into(),
        BevyKey::PageDown => SlintKey::PageDown.into(),
        BevyKey::Shift => SlintKey::Shift.into(),
        BevyKey::Control => SlintKey::Control.into(),
        BevyKey::Alt => SlintKey::Alt.into(),
        _ => slint::SharedString::default(),
    }
}

/// Updates UIPerformance metrics each frame
fn update_ui_performance(
    mut perf: ResMut<UIPerformance>,
    time: Res<Time>,
) {
    perf.update(time.delta_secs());
}

/// Bridges viewport click selection → UnifiedExplorerState.
///
/// Reads the current selection from BevySelectionManager (updated by
/// part_selection_system on viewport clicks) and writes the matching ECS
/// entity into UnifiedExplorerState.selected.  This ensures that:
///   - The Explorer tree highlights the clicked entity
///   - sync_properties_to_slint reads the correct entity for the Properties panel
///
/// Runs every frame (no throttle) so selection feels instant.
fn sync_viewport_selection_to_explorer(
    selection_manager: Option<Res<BevySelectionManager>>,
    mut explorer_state: ResMut<UnifiedExplorerState>,
    instances: Query<(Entity, &eustress_common::classes::Instance)>,
) {
    let Some(sel_mgr) = selection_manager else { return };
    let selected_ids = sel_mgr.0.read().get_selected();

    // Compute the new SelectedItem from the SelectionManager state
    let new_selected = if selected_ids.is_empty() {
        SelectedItem::None
    } else {
        // Try to find the first selected entity by matching the id string
        // format "index v generation" (e.g. "42v0") produced by entity_to_id_string()
        let first_id = &selected_ids[0];
        let found = instances.iter().find(|(entity, _)| {
            let id = format!("{}v{}", entity.index(), entity.generation());
            &id == first_id
        });
        match found {
            Some((entity, _)) => SelectedItem::Entity(entity),
            None => SelectedItem::None,
        }
    };

    // Only write if changed to avoid triggering unnecessary change detection
    // on the two downstream throttled systems.
    let changed = match (&explorer_state.selected, &new_selected) {
        (SelectedItem::Entity(a), SelectedItem::Entity(b)) => a != b,
        (SelectedItem::None, SelectedItem::None) => false,
        _ => true,
    };
    if changed {
        explorer_state.selected = new_selected;
        explorer_state.needs_immediate_sync = true;
    }
}

/// Syncs both ECS entities and filesystem to a single unified tree in Slint.
/// Builds a flat list of TreeNode structs: entities first (services + children),
/// then filesystem nodes (project root directories/files).
/// Throttled to run every 30 frames unless needs_immediate_sync is set
/// (selection/expand change), in which case it runs on the next frame.
fn sync_unified_explorer_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    perf: Option<Res<UIPerformance>>,
    mut explorer_state: ResMut<UnifiedExplorerState>,
    instances: Query<(Entity, &eustress_common::classes::Instance)>,
    children_query: Query<&Children>,
    child_of_query: Query<&ChildOf>,
    service_components: Query<&crate::space::service_loader::ServiceComponent>,
    loaded_from_file: Query<&crate::space::LoadedFromFile>,
    // Terrain entities for Explorer tree (TerrainRoot + Chunks)
    terrain_roots: Query<(Entity, &eustress_common::terrain::TerrainConfig), With<eustress_common::terrain::TerrainRoot>>,
    terrain_chunks: Query<(Entity, &eustress_common::terrain::Chunk)>,
) {
    // Bypass throttle when a user interaction (select/expand/collapse) demands
    // an immediate refresh; otherwise throttle to every 30 frames.
    if explorer_state.needs_immediate_sync {
        explorer_state.needs_immediate_sync = false;
    } else if let Some(ref perf) = perf {
        if perf.should_throttle(30) { return; }
    }
    let Some(slint_context) = slint_context else { return };
    let ui = &slint_context.window;
    
    use eustress_common::classes::ClassName;
    use super::file_icons;
    
    let mut tree_nodes: Vec<TreeNode> = Vec::new();
    
    // ================================================================
    // Part 1: Build ECS entity nodes (services + children)
    // ================================================================
    
    // Reset entity ID cache each sync — Slint int is 32-bit so we cannot
    // truncate entity.to_bits() (u64). Instead we assign sequential IDs
    // starting at 1 and store the Entity lookup here.
    explorer_state.entity_id_cache.clear();
    explorer_state.next_entity_node_id = 1;
    
    // Build set of all entities that have Instance components
    let instance_entities: std::collections::HashSet<Entity> = 
        instances.iter().map(|(e, _)| e).collect();
    
    // Build a parent -> children lookup map from ChildOf components
    // This is more reliable than querying Children, which may not be populated yet
    let mut children_of_parent: std::collections::HashMap<Entity, Vec<Entity>> = std::collections::HashMap::new();
    for (entity, _) in instances.iter() {
        if let Ok(child_of) = child_of_query.get(entity) {
            children_of_parent.entry(child_of.0).or_default().push(entity);
        }
    }
    
    // Find root entities (no ChildOf, or ChildOf points to non-Instance entity)
    // Filter out adornment entities (meta = true) - they are hidden from Explorer
    let mut roots: Vec<Entity> = Vec::new();
    for (entity, instance) in instances.iter() {
        // Skip adornment classes (meta entities hidden from Explorer)
        if instance.class_name.is_adornment() {
            continue;
        }
        match child_of_query.get(entity) {
            Ok(child_of) => {
                if !instance_entities.contains(&child_of.0) {
                    roots.push(entity);
                }
            }
            Err(_) => {
                roots.push(entity);
            }
        }
    }
    
    roots.sort_by(|a, b| {
        let a_name = instances.get(*a).map(|(_, i)| i.name.as_str()).unwrap_or("");
        let b_name = instances.get(*b).map(|(_, i)| i.name.as_str()).unwrap_or("");
        
        // Pin Camera at the top
        match (a_name, b_name) {
            ("Camera", "Camera") => std::cmp::Ordering::Equal,
            ("Camera", _) => std::cmp::Ordering::Less,
            (_, "Camera") => std::cmp::Ordering::Greater,
            _ => a_name.cmp(b_name),
        }
    });
    
    // Classify root entities into service buckets
    // Primary: use LoadedFromFile.service field (filesystem-based classification)
    // Fallback: use ClassName for entities without LoadedFromFile
    let is_lighting_child = |cn: &ClassName| matches!(cn,
        ClassName::Sky | ClassName::Atmosphere | ClassName::Star | ClassName::Moon | ClassName::Clouds
    );
    
    let is_ui_child = |cn: &ClassName| matches!(cn,
        ClassName::ScreenGui | ClassName::Frame | ClassName::TextLabel | ClassName::TextButton |
        ClassName::ImageLabel | ClassName::ImageButton | ClassName::ScrollingFrame |
        ClassName::TextBox | ClassName::ViewportFrame | ClassName::BillboardGui | ClassName::SurfaceGui
    );
    
    let is_script = |cn: &ClassName| matches!(cn,
        ClassName::SoulScript
    );
    
    let mut workspace_roots: Vec<Entity> = Vec::new();
    let mut lighting_roots: Vec<Entity> = Vec::new();
    let mut starter_gui_roots: Vec<Entity> = Vec::new();
    let mut soul_service_roots: Vec<Entity> = Vec::new();
    // Dynamic services: entities whose LoadedFromFile.service is not one of the
    // hardcoded names above. Keyed by service name for Explorer rendering.
    let mut dynamic_service_roots: std::collections::HashMap<String, Vec<Entity>> = std::collections::HashMap::new();
    
    // Set of hardcoded service names — entities in these are handled by the
    // dedicated buckets above, everything else goes to dynamic_service_roots.
    let hardcoded_services: std::collections::HashSet<&str> = [
        "Workspace", "Lighting", "StarterGui", "SoulService",
        "Players", "StarterPack", "StarterPlayer", "ReplicatedStorage",
        "ServerStorage", "ServerScriptService", "SoundService", "Teams", "Chat",
        // Also skip StarterCharacterScripts and StarterPlayerScripts if present
        "StarterCharacterScripts", "StarterPlayerScripts",
    ].into_iter().collect();
    
    // Find service entities and populate their children buckets directly
    // This is more reliable than using roots, since children of services have ChildOf
    // pointing to the service entity, so they're not in roots.
    for (entity, _) in instances.iter() {
        if let Ok(service) = service_components.get(entity) {
            // Get children of this service entity from the children_of_parent map
            if let Some(children) = children_of_parent.get(&entity) {
                for child in children {
                    // Skip adornments
                    if let Ok((_, inst)) = instances.get(*child) {
                        if inst.class_name.is_adornment() {
                            continue;
                        }
                    }
                    match service.class_name.as_str() {
                        "Workspace" => workspace_roots.push(*child),
                        "Lighting" => lighting_roots.push(*child),
                        "StarterGui" => starter_gui_roots.push(*child),
                        "SoulService" => soul_service_roots.push(*child),
                        other if !hardcoded_services.contains(other) => {
                            dynamic_service_roots.entry(other.to_string()).or_default().push(*child);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    
    // Also add root entities that don't have a parent (fallback for entities without ChildOf)
    for entity in &roots {
        if let Ok((_, instance)) = instances.get(*entity) {
            // Skip service header entities
            if service_components.get(*entity).is_ok() {
                continue;
            }
            
            // Skip if already added (has ChildOf to a service)
            if child_of_query.get(*entity).is_ok() {
                continue;
            }
            
            // Primary classification: use LoadedFromFile.service field
            if let Ok(loaded) = loaded_from_file.get(*entity) {
                match loaded.service.as_str() {
                    "Workspace" => workspace_roots.push(*entity),
                    "Lighting" => lighting_roots.push(*entity),
                    "StarterGui" => starter_gui_roots.push(*entity),
                    "SoulService" => soul_service_roots.push(*entity),
                    other if !hardcoded_services.contains(other) => {
                        dynamic_service_roots.entry(other.to_string()).or_default().push(*entity);
                    }
                    _ => {}
                }
            } else {
                // Fallback: classify by ClassName
                if is_lighting_child(&instance.class_name) {
                    lighting_roots.push(*entity);
                } else if is_ui_child(&instance.class_name) {
                    starter_gui_roots.push(*entity);
                } else if is_script(&instance.class_name) {
                    soul_service_roots.push(*entity);
                } else {
                    workspace_roots.push(*entity);
                }
            }
        }
    }
    
    // Snapshot borrow-free copies of what the DFS closure needs from explorer_state
    let expanded_entities = explorer_state.expanded_entities.clone();
    let selected_item = explorer_state.selected.clone();

    // DFS helper — builds TreeNodes for a list of root entities.
    // Takes entity_id_cache and next_id by mutable reference so it can register IDs
    // without a ResMut borrow conflict with the immutable borrows above.
    #[allow(clippy::too_many_arguments)]
    let mut build_entity_nodes = |root_list: &[Entity],
                                   base_depth: i32,
                                   entity_id_cache: &mut std::collections::HashMap<i32, Entity>,
                                   next_id: &mut i32| -> Vec<TreeNode> {
        let mut nodes: Vec<TreeNode> = Vec::new();
        let mut stack: Vec<(Entity, i32)> = root_list.iter().rev().map(|e| (*e, base_depth)).collect();

        while let Some((entity, depth)) = stack.pop() {
            let Ok((_, instance)) = instances.get(entity) else { continue };
            
            // Skip adornment classes (meta entities hidden from Explorer)
            if instance.class_name.is_adornment() {
                continue;
            }

            // Use children_of_parent map (built from ChildOf components) instead of Children query
            // This is more reliable since Children may not be populated yet after ChildOf insertion
            let has_children = children_of_parent.get(&entity)
                .map(|children| children.iter().any(|c| {
                    instance_entities.contains(c) && 
                    instances.get(*c).map(|(_, i)| !i.class_name.is_adornment()).unwrap_or(false)
                }))
                .unwrap_or(false);

            // Assign a stable sequential i32 ID (avoids u64 truncation to i32)
            let entity_id = *next_id;
            entity_id_cache.insert(entity_id, entity);
            *next_id += 1;

            let is_expanded = expanded_entities.contains(&entity);
            let is_selected = matches!(&selected_item, SelectedItem::Entity(e) if *e == entity);

            // Use ServiceComponent icon if available, otherwise fall back to class icon
            let icon = if let Ok(service) = service_components.get(entity) {
                load_service_icon(&service.icon)
            } else {
                load_class_icon(&instance.class_name)
            };

            nodes.push(TreeNode {
                id: entity_id,
                name: instance.name.clone().into(),
                icon,
                depth,
                expandable: has_children,
                expanded: is_expanded,
                selected: is_selected,
                visible: true,
                node_type: "entity".into(),
                class_name: format!("{:?}", instance.class_name).into(),
                path: slint::SharedString::default(),
                is_directory: false,
                extension: slint::SharedString::default(),
                size: slint::SharedString::default(),
                modified: false,
            });

            if is_expanded && has_children {
                if let Some(children) = children_of_parent.get(&entity) {
                    let mut child_instances: Vec<Entity> = children.iter()
                        .filter(|c| {
                            instance_entities.contains(c) &&
                            instances.get(**c).map(|(_, i)| !i.class_name.is_adornment()).unwrap_or(false)
                        })
                        .copied()
                        .collect();
                    child_instances.sort_by(|a, b| {
                        let a_name = instances.get(*a).map(|(_, i)| i.name.as_str()).unwrap_or("");
                        let b_name = instances.get(*b).map(|(_, i)| i.name.as_str()).unwrap_or("");
                        match (a_name, b_name) {
                            ("Camera", "Camera") => std::cmp::Ordering::Equal,
                            ("Camera", _) => std::cmp::Ordering::Less,
                            (_, "Camera") => std::cmp::Ordering::Greater,
                            _ => a_name.cmp(b_name),
                        }
                    });
                    for child in child_instances.into_iter().rev() {
                        stack.push((child, depth + 1));
                    }
                }
            }
        }
        nodes
    };

    // Temporarily take ownership of entity_id_cache and next_id counter
    // to avoid ResMut borrow conflicts inside the closure.
    let mut entity_id_cache = std::mem::take(&mut explorer_state.entity_id_cache);
    let mut next_id = explorer_state.next_entity_node_id;

    // Helper: is a service currently expanded? (clone to avoid borrow conflicts)
    let expanded_services = explorer_state.expanded_services.clone();
    let svc_expanded = |name: &str| expanded_services.contains(name);

    // Service: Workspace (depth 0) + children (depth 1+)
    let has_terrain = !terrain_roots.is_empty();
    let ws_has = !workspace_roots.is_empty() || has_terrain;
    tree_nodes.push(make_service_node("Workspace", "workspace", 0, ws_has, svc_expanded("Workspace"), &explorer_state));
    if svc_expanded("Workspace") {
        tree_nodes.extend(build_entity_nodes(&workspace_roots, 1, &mut entity_id_cache, &mut next_id));
        
        // Inject Terrain entities (TerrainRoot + Chunks) into Workspace
        for (terrain_entity, terrain_config) in terrain_roots.iter() {
            let terrain_node_id = next_id;
            entity_id_cache.insert(terrain_node_id, terrain_entity);
            next_id += 1;
            
            // Collect chunks that belong to this terrain root
            let terrain_chunk_entities: Vec<(Entity, &eustress_common::terrain::Chunk)> = 
                if let Ok(children) = children_query.get(terrain_entity) {
                    children.iter()
                        .filter_map(|c| terrain_chunks.get(c).ok())
                        .collect()
                } else {
                    Vec::new()
                };
            
            let has_chunks = !terrain_chunk_entities.is_empty();
            let is_terrain_expanded = expanded_entities.contains(&terrain_entity);
            let is_terrain_selected = matches!(&selected_item, SelectedItem::Entity(e) if *e == terrain_entity);
            
            let (total_w, _) = terrain_config.total_size();
            
            tree_nodes.push(TreeNode {
                id: terrain_node_id,
                name: format!("Terrain ({:.0}m)", total_w).into(),
                icon: load_service_icon("terrain"),
                depth: 1,
                expandable: has_chunks,
                expanded: is_terrain_expanded,
                selected: is_terrain_selected,
                visible: true,
                node_type: "entity".into(),
                class_name: "Terrain".into(),
                path: slint::SharedString::default(),
                is_directory: false,
                extension: slint::SharedString::default(),
                size: slint::SharedString::default(),
                modified: false,
            });
            
            // Add chunk children if terrain is expanded
            if is_terrain_expanded {
                let mut sorted_chunks = terrain_chunk_entities;
                sorted_chunks.sort_by(|(_, a), (_, b)| {
                    a.position.x.cmp(&b.position.x)
                        .then(a.position.y.cmp(&b.position.y))
                });
                
                for (chunk_entity, chunk) in &sorted_chunks {
                    let chunk_node_id = next_id;
                    entity_id_cache.insert(chunk_node_id, *chunk_entity);
                    next_id += 1;
                    
                    let is_chunk_selected = matches!(&selected_item, SelectedItem::Entity(e) if *e == *chunk_entity);
                    
                    tree_nodes.push(TreeNode {
                        id: chunk_node_id,
                        name: format!("Chunk ({}, {})", chunk.position.x, chunk.position.y).into(),
                        icon: load_service_icon("terrain"),
                        depth: 2,
                        expandable: false,
                        expanded: false,
                        selected: is_chunk_selected,
                        visible: true,
                        node_type: "entity".into(),
                        class_name: "Chunk".into(),
                        path: slint::SharedString::default(),
                        is_directory: false,
                        extension: slint::SharedString::default(),
                        size: format!("LOD {}", chunk.lod).into(),
                        modified: chunk.dirty,
                    });
                }
            }
        }
    }

    // ================================================================
    // Terrain Folder (file-system-first) — appears under Workspace
    // Shows Workspace/Terrain/ directory with _terrain.toml and chunks/
    // ================================================================
    {
        let space_root = &crate::space::default_space_root();
        let terrain_dir = space_root.join("Workspace").join("Terrain");
        
        if terrain_dir.exists() {
            // Terrain folder node (depth 1, child of Workspace)
            let terrain_folder_id = next_id;
            next_id += 1;
            
            // Register in file_path_cache for expand/collapse actions
            explorer_state.file_path_cache.insert(terrain_folder_id, terrain_dir.clone());
            
            let terrain_expanded = explorer_state.expanded_dirs.contains(&terrain_dir);
            let terrain_selected = matches!(&explorer_state.selected, SelectedItem::File(p) if p == &terrain_dir);
            
            // Count total files in terrain dir for display
            let mut file_count = 0;
            if let Ok(entries) = std::fs::read_dir(&terrain_dir) {
                file_count = entries.flatten().filter(|e| e.path().is_file()).count();
            }
            
            let folder_icon = {
                let icon_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(load_file_icon("folder"));
                slint::Image::load_from_path(&icon_path).unwrap_or_default()
            };
            tree_nodes.push(TreeNode {
                id: terrain_folder_id,
                name: format!("Terrain ({} files)", file_count).into(),
                icon: folder_icon,
                depth: 1,
                expandable: true,
                expanded: terrain_expanded,
                selected: terrain_selected,
                visible: true,
                node_type: "file".into(),
                class_name: slint::SharedString::default(),
                path: terrain_dir.to_string_lossy().to_string().into(),
                is_directory: true,
                extension: slint::SharedString::default(),
                size: slint::SharedString::default(),
                modified: false,
            });
            
            // If expanded, show contents
            if terrain_expanded {
                // Collect all entries (files and subdirs)
                let mut entries: Vec<(String, std::path::PathBuf, bool)> = Vec::new();
                if let Ok(read_dir) = std::fs::read_dir(&terrain_dir) {
                    for entry in read_dir.flatten() {
                        let path = entry.path();
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                        let is_dir = path.is_dir();
                        entries.push((name, path, is_dir));
                    }
                }
                
                // Sort: directories first, then files, alphabetically
                entries.sort_by(|a, b| {
                    match (a.2, b.2) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => a.0.cmp(&b.0),
                    }
                });
                
                // Add each entry as a TreeNode
                for (name, path, is_dir) in &entries {
                    let entry_id = next_id;
                    next_id += 1;
                    
                    // Register in file_path_cache
                    explorer_state.file_path_cache.insert(entry_id, path.clone());
                    
                    let entry_selected = matches!(&explorer_state.selected, SelectedItem::File(p) if p == path);
                    let entry_expanded = *is_dir && explorer_state.expanded_dirs.contains(path);
                    
                    let extension = if !is_dir {
                        path.extension().and_then(|e| e.to_str()).unwrap_or("")
                    } else {
                        ""
                    };
                    
                    let size = if !is_dir {
                        path.metadata().map(|m| {
                            let bytes = m.len();
                            if bytes < 1024 { format!("{} B", bytes) }
                            else if bytes < 1024 * 1024 { format!("{:.1} KB", bytes as f64 / 1024.0) }
                            else { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) }
                        }).unwrap_or_default()
                    } else {
                        // Count files in subdirectory
                        std::fs::read_dir(path)
                            .map(|rd| format!("{} items", rd.flatten().count()))
                            .unwrap_or_default()
                    };
                    
                    let entry_icon = {
                        let icon_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(load_file_icon(if *is_dir { "folder" } else { extension }));
                        slint::Image::load_from_path(&icon_path).unwrap_or_default()
                    };
                    tree_nodes.push(TreeNode {
                        id: entry_id,
                        name: name.clone().into(),
                        icon: entry_icon,
                        depth: 2,
                        expandable: *is_dir,
                        expanded: entry_expanded,
                        selected: entry_selected,
                        visible: true,
                        node_type: "file".into(),
                        class_name: slint::SharedString::default(),
                        path: path.to_string_lossy().to_string().into(),
                        is_directory: *is_dir,
                        extension: extension.into(),
                        size: size.into(),
                        modified: false,
                    });
                    
                    // If subdirectory is expanded, show its contents (depth 3)
                    if *is_dir && entry_expanded {
                        if let Ok(sub_entries) = std::fs::read_dir(path) {
                            let mut sub_files: Vec<(String, std::path::PathBuf)> = sub_entries
                                .flatten()
                                .filter(|e| e.path().is_file())
                                .map(|e| {
                                    let p = e.path();
                                    let n = p.file_name().and_then(|f| f.to_str()).unwrap_or("").to_string();
                                    (n, p)
                                })
                                .collect();
                            sub_files.sort_by(|a, b| a.0.cmp(&b.0));
                            
                            for (sub_name, sub_path) in &sub_files {
                                let sub_id = next_id;
                                next_id += 1;
                                
                                // Register in file_path_cache
                                explorer_state.file_path_cache.insert(sub_id, sub_path.clone());
                                
                                let sub_selected = matches!(&explorer_state.selected, SelectedItem::File(p) if p == sub_path);
                                let sub_ext = sub_path.extension().and_then(|e| e.to_str()).unwrap_or("");
                                let sub_size = sub_path.metadata().map(|m| {
                                    let bytes = m.len();
                                    if bytes < 1024 { format!("{} B", bytes) }
                                    else if bytes < 1024 * 1024 { format!("{:.1} KB", bytes as f64 / 1024.0) }
                                    else { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) }
                                }).unwrap_or_default();
                                
                                let sub_icon = {
                                    let icon_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(load_file_icon(sub_ext));
                                    slint::Image::load_from_path(&icon_path).unwrap_or_default()
                                };
                                tree_nodes.push(TreeNode {
                                    id: sub_id,
                                    name: sub_name.clone().into(),
                                    icon: sub_icon,
                                    depth: 3,
                                    expandable: false,
                                    expanded: false,
                                    selected: sub_selected,
                                    visible: true,
                                    node_type: "file".into(),
                                    class_name: slint::SharedString::default(),
                                    path: sub_path.to_string_lossy().to_string().into(),
                                    is_directory: false,
                                    extension: sub_ext.into(),
                                    size: sub_size.into(),
                                    modified: false,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Service: Lighting (depth 0) + children (depth 1+)
    let lt_has = !lighting_roots.is_empty();
    tree_nodes.push(make_service_node("Lighting", "lighting", 0, lt_has, svc_expanded("Lighting"), &explorer_state));
    if svc_expanded("Lighting") {
        tree_nodes.extend(build_entity_nodes(&lighting_roots, 1, &mut entity_id_cache, &mut next_id));
    }

    // Service: Players (depth 0) - runtime only, no children in editor
    tree_nodes.push(make_service_node("Players", "players", 0, false, false, &explorer_state));

    // Service: StarterGui (depth 0) + UI children (depth 1+)
    let sg_has = !starter_gui_roots.is_empty();
    tree_nodes.push(make_service_node("StarterGui", "startergui", 0, sg_has, svc_expanded("StarterGui"), &explorer_state));
    if svc_expanded("StarterGui") {
        tree_nodes.extend(build_entity_nodes(&starter_gui_roots, 1, &mut entity_id_cache, &mut next_id));
    }

    // Service: StarterPack (depth 0) - no editor children
    tree_nodes.push(make_service_node("StarterPack", "starterpack", 0, false, false, &explorer_state));

    // Service: StarterPlayer (depth 0) - no editor children
    tree_nodes.push(make_service_node("StarterPlayer", "starterplayer", 0, false, false, &explorer_state));

    // Service: ReplicatedStorage (depth 0) - no editor children
    tree_nodes.push(make_service_node("ReplicatedStorage", "replicatedstorage", 0, false, false, &explorer_state));

    // Service: ServerStorage (depth 0) - no editor children
    tree_nodes.push(make_service_node("ServerStorage", "serverstorage", 0, false, false, &explorer_state));

    // Service: ServerScriptService (depth 0) - no editor children
    tree_nodes.push(make_service_node("ServerScriptService", "serverscriptservice", 0, false, false, &explorer_state));

    // Service: SoulService (depth 0) + script children (depth 1+)
    let ss_has = !soul_service_roots.is_empty();
    tree_nodes.push(make_service_node("SoulService", "soulservice", 0, ss_has, svc_expanded("SoulService"), &explorer_state));
    if svc_expanded("SoulService") {
        tree_nodes.extend(build_entity_nodes(&soul_service_roots, 1, &mut entity_id_cache, &mut next_id));
    }

    // Service: SoundService (depth 0) - no editor children
    tree_nodes.push(make_service_node("SoundService", "soundservice", 0, false, false, &explorer_state));

    // Service: Teams (depth 0) - no editor children
    tree_nodes.push(make_service_node("Teams", "teams", 0, false, false, &explorer_state));

    // Service: Chat (depth 0) - no editor children
    tree_nodes.push(make_service_node("Chat", "chat", 0, false, false, &explorer_state));

    // ================================================================
    // Dynamic services: discovered from _service.toml files on disk.
    // Any service folder not in the hardcoded list above gets rendered here.
    // ================================================================
    {
        let space_root = &crate::space::default_space_root();
        if let Ok(read_dir) = std::fs::read_dir(space_root) {
            let mut dynamic_names: Vec<String> = Vec::new();
            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_dir() { continue; }
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
                // Skip hardcoded services — they are already rendered above
                if hardcoded_services.contains(name) { continue; }
                // Must have _service.toml to be a valid service
                if !path.join("_service.toml").exists() { continue; }
                dynamic_names.push(name.to_string());
            }
            dynamic_names.sort();

            for svc_name in &dynamic_names {
                // Load icon from _service.toml if available
                let svc_toml = space_root.join(svc_name).join("_service.toml");
                let icon_name = std::fs::read_to_string(&svc_toml)
                    .ok()
                    .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
                    .and_then(|v| v.get("service").and_then(|s| s.get("icon")).and_then(|i| i.as_str().map(|s| s.to_string())))
                    .unwrap_or_else(|| svc_name.to_lowercase());

                let children = dynamic_service_roots.get(svc_name).cloned().unwrap_or_default();
                let has = !children.is_empty();
                tree_nodes.push(make_service_node(svc_name, &icon_name, 0, has, svc_expanded(svc_name), &explorer_state));
                if svc_expanded(svc_name) && has {
                    tree_nodes.extend(build_entity_nodes(&children, 1, &mut entity_id_cache, &mut next_id));
                }
            }
        }
    }

    // Write entity_id_cache and counter back to explorer_state
    explorer_state.entity_id_cache = entity_id_cache;
    explorer_state.next_entity_node_id = next_id;

    // ================================================================
    // Part 2: Filesystem nodes DISABLED — Explorer shows entity items only.
    // File browsing will be handled by a separate Asset Manager panel.
    // ================================================================

    // ================================================================
    // Part 3: Filter by search query
    // ================================================================

    if !explorer_state.search_query.is_empty() {
        let query = explorer_state.search_query.to_lowercase();
        for node in tree_nodes.iter_mut() {
            let name_lower: String = node.name.to_string().to_lowercase();
            node.visible = name_lower.contains(&query);
        }
    }
    
    // Hash-based change detection: only push to Slint when the model data actually
    // changes. Re-pushing an identical model destroys and recreates all `for` loop
    // items in Slint, which resets hover state and causes visible flickering.
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for node in &tree_nodes {
        node.id.hash(&mut hasher);
        node.name.hash(&mut hasher);
        node.depth.hash(&mut hasher);
        node.expanded.hash(&mut hasher);
        node.selected.hash(&mut hasher);
        node.expandable.hash(&mut hasher);
        node.visible.hash(&mut hasher);
        node.node_type.hash(&mut hasher);
    }
    let new_hash = hasher.finish();
    
    if new_hash != explorer_state.last_tree_hash {
        explorer_state.last_tree_hash = new_hash;
        let model = std::rc::Rc::new(slint::VecModel::from(tree_nodes));
        ui.set_tree_nodes(slint::ModelRc::from(model));
    }
}

/// Create a service header node (Workspace, Lighting, Players, etc.)
/// `has_children` — whether this service has any child entities (controls arrow visibility)
/// `is_expanded`  — whether the service is currently expanded (controls arrow direction)
fn make_service_node(
    name: &str,
    icon_name: &str,
    depth: i32,
    has_children: bool,
    is_expanded: bool,
    state: &UnifiedExplorerState,
) -> TreeNode {
    let is_selected = matches!(&state.selected, SelectedItem::Service(s) if s == name);
    TreeNode {
        id: -(name.len() as i32), // Negative IDs for services
        name: name.into(),
        icon: load_service_icon(icon_name),
        depth,
        expandable: has_children,
        expanded: is_expanded,
        selected: is_selected,
        visible: true,
        node_type: "entity".into(),
        class_name: name.into(),
        path: slint::SharedString::default(),
        is_directory: false,
        extension: slint::SharedString::default(),
        size: slint::SharedString::default(),
        modified: false,
    }
}

/// Load icon for a service by name (from assets/icons/{name}.svg)
fn load_service_icon(name: &str) -> slint::Image {
    let icon_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("icons")
        .join(format!("{}.svg", name));
    slint::Image::load_from_path(&icon_path).unwrap_or_default()
}

/// Recursively build filesystem TreeNodes from a directory.
/// Populates `path_cache` with hash_id → PathBuf for reverse lookup in actions.
fn build_file_tree_nodes(
    nodes: &mut Vec<TreeNode>,
    dir: &std::path::Path,
    depth: i32,
    expanded_dirs: &std::collections::HashSet<std::path::PathBuf>,
    selected: &SelectedItem,
    path_cache: &mut std::collections::HashMap<i32, std::path::PathBuf>,
) {
    use super::file_icons;
    
    // Read directory entries, skip errors
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        
        // Skip hidden files/dirs (starting with .) and common noise
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        
        if path.is_dir() {
            dirs.push(path);
        } else {
            files.push(path);
        }
    }
    
    // Sort: directories first (alphabetical), then files (alphabetical)
    dirs.sort_by(|a, b| {
        let a_name = a.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let b_name = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
        a_name.to_lowercase().cmp(&b_name.to_lowercase())
    });
    files.sort_by(|a, b| {
        let a_name = a.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let b_name = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
        a_name.to_lowercase().cmp(&b_name.to_lowercase())
    });
    
    // Emit directory nodes
    for dir_path in &dirs {
        let dir_name = file_icons::get_dir_name(dir_path);
        let is_expanded = expanded_dirs.contains(dir_path);
        let is_selected = matches!(selected, SelectedItem::File(p) if p == dir_path);
        let path_hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            dir_path.hash(&mut hasher);
            (hasher.finish() & 0x7FFFFFFF) as i32
        };
        
        // Populate reverse lookup cache
        path_cache.insert(path_hash, dir_path.clone());
        
        let folder_icon_name = if is_expanded {
            match dir_name.to_lowercase().as_str() {
                "src" | "source" => "folder-src-open",
                "assets" | "resources" | "res" => "folder-assets-open",
                "docs" | "documentation" | "doc" => "folder-docs-open",
                "test" | "tests" | "__tests__" | "spec" | "specs" => "folder-test-open",
                "config" | "configs" | "configuration" | ".config" => "folder-config-open",
                "dist" | "build" | "out" | "output" => "folder-dist-open",
                "scripts" | "script" => "folder-scripts-open",
                "lib" | "libs" | "library" | "libraries" => "folder-lib-open",
                "target" | "bin" | "obj" => "folder-target-open",
                ".git" => "folder-git-open",
                ".github" | ".gitlab" => "folder-github-open",
                ".vscode" | ".idea" | ".vs" => "folder-vscode-open",
                "images" | "imgs" | "img" | "pictures" | "pics" => "folder-images-open",
                _ => "folder-open",
            }
        } else {
            match dir_name.to_lowercase().as_str() {
                "src" | "source" => "folder-src",
                "assets" | "resources" | "res" => "folder-assets",
                "docs" | "documentation" | "doc" => "folder-docs",
                "test" | "tests" | "__tests__" | "spec" | "specs" => "folder-test",
                "config" | "configs" | "configuration" | ".config" => "folder-config",
                "dist" | "build" | "out" | "output" => "folder-dist",
                "scripts" | "script" => "folder-scripts",
                "lib" | "libs" | "library" | "libraries" => "folder-lib",
                "target" | "bin" | "obj" => "folder-target",
                ".git" => "folder-git",
                ".github" | ".gitlab" => "folder-github",
                ".vscode" | ".idea" | ".vs" => "folder-vscode",
                "images" | "imgs" | "img" | "pictures" | "pics" => "folder-images",
                _ => "folder",
            }
        };
        let icon_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("icons")
            .join("folders")
            .join(format!("{}.svg", folder_icon_name));
        let icon = slint::Image::load_from_path(&icon_path).unwrap_or_default();
        
        nodes.push(TreeNode {
            id: path_hash,
            name: dir_name.clone().into(),
            icon,
            depth,
            expandable: true,
            expanded: is_expanded,
            selected: is_selected,
            visible: true,
            node_type: "file".into(),
            class_name: slint::SharedString::default(),
            path: dir_path.to_string_lossy().to_string().into(),
            is_directory: true,
            extension: slint::SharedString::default(),
            size: slint::SharedString::default(),
            modified: false,
        });
        
        // Recurse into expanded directories
        if is_expanded {
            build_file_tree_nodes(nodes, dir_path, depth + 1, expanded_dirs, selected, path_cache);
        }
    }
    
    // Emit file nodes
    for file_path in &files {
        let file_name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let ext = file_icons::get_extension(file_path);
        let is_selected = matches!(selected, SelectedItem::File(p) if p == file_path);
        let file_size = std::fs::metadata(file_path)
            .map(|m| file_icons::format_file_size(m.len()))
            .unwrap_or_default();
        let path_hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            file_path.hash(&mut hasher);
            (hasher.finish() & 0x7FFFFFFF) as i32
        };
        
        // Populate reverse lookup cache
        path_cache.insert(path_hash, file_path.clone());
        
        let file_icon_name = match ext.to_lowercase().as_str() {
            "rs" | "ron" => "rust",
            "lua" => "lua",
            "js" | "mjs" | "cjs" => "javascript",
            "ts" | "mts" | "cts" => "typescript",
            "py" => "python",
            "json" | "jsonc" => "json",
            "toml" => "toml",
            "yaml" | "yml" => "yaml",
            "xml" | "xsl" | "xsd" => "xml",
            "html" | "htm" => "html",
            "css" => "css",
            "md" | "markdown" => "markdown",
            "svg" => "svg",
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "tga" | "tiff" | "tif" => "image",
            "mp4" | "webm" | "mov" | "avi" | "mkv" | "flv" | "wmv" => "video",
            "wav" | "ogg" | "mp3" | "flac" | "aac" | "m4a" | "opus" => "audio",
            "pdf" => "pdf",
            "glb" | "gltf" => "model",
            "hgt" | "tif" | "tiff" | "geotiff" => "image",
            _ => "file",
        };
        let icon_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("icons")
            .join("filetypes")
            .join(format!("{}.svg", file_icon_name));
        let icon = slint::Image::load_from_path(&icon_path).unwrap_or_default();
        
        nodes.push(TreeNode {
            id: path_hash,
            name: file_name.into(),
            icon,
            depth,
            expandable: false,
            expanded: false,
            selected: is_selected,
            visible: true,
            node_type: "file".into(),
            class_name: slint::SharedString::default(),
            path: file_path.to_string_lossy().to_string().into(),
            is_directory: false,
            extension: ext.into(),
            size: file_size.into(),
            modified: false,
        });
    }
}

/// Syncs the selected entity's properties to the Slint properties panel.
/// Builds a flat list with category headers interleaved for section grouping.
/// Throttled to run every 15 frames.
fn sync_properties_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    perf: Option<Res<UIPerformance>>,
    mut studio_state: ResMut<StudioState>,
    explorer_state: Res<UnifiedExplorerState>,
    instances: Query<(Entity, &eustress_common::classes::Instance)>,
    transforms: Query<&Transform>,
    base_parts: Query<&eustress_common::classes::BasePart>,
    instance_files: Query<&crate::space::instance_loader::InstanceFile>,
    service_components: Query<&crate::space::service_loader::ServiceComponent>,
    material_props: Query<&eustress_common::realism::materials::properties::MaterialProperties>,
    thermo_states: Query<&eustress_common::realism::particles::components::ThermodynamicState>,
    echem_states: Query<&eustress_common::realism::particles::components::ElectrochemicalState>,
    // UI class components collapsed into a ParamSet to stay within Bevy's 16-param system limit
    mut ui_queries: ParamSet<(
        Query<&eustress_common::classes::TextLabel>,
        Query<&eustress_common::classes::TextButton>,
        Query<&eustress_common::classes::TextBox>,
        Query<&eustress_common::classes::Frame>,
        Query<&eustress_common::classes::ImageLabel>,
        Query<&eustress_common::classes::ImageButton>,
        Query<&eustress_common::classes::ScrollingFrame>,
    )>,
) {
    let Some(slint_context) = slint_context else { return };
    let ui = &slint_context.window;
    
    // Skip sync entirely if user is actively editing an input field
    // This prevents overwriting user input while they're typing
    if ui.get_any_input_has_focus() {
        return;
    }
    
    // Detect selection changes to trigger immediate sync
    let current_selected = match &explorer_state.selected {
        SelectedItem::Entity(e) => Some(*e),
        _ => None,
    };
    
    let selection_changed = current_selected != studio_state.last_selected_entity;
    if selection_changed {
        studio_state.last_selected_entity = current_selected;
        studio_state.frames_since_selection_change = 0;
        studio_state.last_properties_hash = 0; // Force rebuild on selection change
    } else {
        studio_state.frames_since_selection_change += 1;
    }
    
    // Only sync properties:
    // 1. Immediately on selection change (frames_since_selection_change == 0)
    // 2. After a long delay (300 frames = ~5 seconds at 60fps) to catch external changes
    // This prevents flickering during editing while still allowing property updates
    let should_sync = studio_state.frames_since_selection_change == 0 
        || studio_state.frames_since_selection_change == 300;
    
    if !should_sync {
        return;
    }
    
    let selected_entity = match &explorer_state.selected {
        SelectedItem::Entity(e) => *e,
        SelectedItem::File(path) => {
            // File selected — show filesystem properties
            build_file_properties(ui, path);
            return;
        }
        SelectedItem::Service(service_name) => {
            // Service header selected — show its properties from ServiceComponent
            build_service_properties(ui, service_name, &service_components);
            return;
        }
        SelectedItem::None => {
            // No selection — clear properties and update count
            ui.set_selected_count(0);
            ui.set_selected_class(slint::SharedString::default());
            ui.set_selected_icon(slint::Image::default());
            let empty: Vec<PropertyData> = Vec::new();
            let model_rc = std::rc::Rc::new(slint::VecModel::from(empty));
            ui.set_entity_properties(slint::ModelRc::from(model_rc));
            return;
        }
    };
    
    let Ok((_, instance)) = instances.get(selected_entity) else { return };

    ui.set_selected_count(1);
    ui.set_selected_class(format!("{:?}", instance.class_name).into());
    ui.set_selected_icon(load_class_icon(&instance.class_name));
    
    // Collect raw properties with categories into buckets
    // category -> Vec<(name, value, type, editable)>
    let mut categorized: std::collections::BTreeMap<String, Vec<(String, String, String, bool)>> = std::collections::BTreeMap::new();
    
    // Helper to add a property to a category bucket
    let mut add_prop = |cat: &str, name: &str, value: String, prop_type: &str, editable: bool| {
        categorized.entry(cat.to_string())
            .or_default()
            .push((name.to_string(), value, prop_type.to_string(), editable));
    };
    
    // ══════════════════════════════════════════════════════════════════════════
    // FILE-SYSTEM-FIRST: Read ALL properties directly from TOML file
    // This ensures Properties panel shows exactly what's in the TOML
    // ══════════════════════════════════════════════════════════════════════════
    if let Ok(instance_file) = instance_files.get(selected_entity) {
        if let Ok(toml_def) = crate::space::instance_loader::load_instance_definition(&instance_file.toml_path) {
            // -- Metadata section --
            add_prop("Metadata", "ClassName", toml_def.metadata.class_name.clone(), "string", false);
            add_prop("Metadata", "Name", instance_file.name.clone(), "string", true);
            add_prop("Metadata", "Archivable", toml_def.metadata.archivable.to_string(), "bool", true);
            if !toml_def.metadata.created.is_empty() {
                add_prop("Metadata", "Created", toml_def.metadata.created.clone(), "string", false);
            }
            if !toml_def.metadata.last_modified.is_empty() {
                add_prop("Metadata", "LastModified", toml_def.metadata.last_modified.clone(), "string", false);
            }
            
            // -- Asset section --
            if let Some(ref asset) = toml_def.asset {
                add_prop("Asset", "Mesh", asset.mesh.clone(), "string", true);
                add_prop("Asset", "Scene", asset.scene.clone(), "string", true);
            }
            
            // -- Transform section --
            add_prop("Transform", "Position", format!("{:.3}, {:.3}, {:.3}", 
                toml_def.transform.position[0], toml_def.transform.position[1], toml_def.transform.position[2]), "vec3", true);
            // Convert stored quaternion [x, y, z, w] → Euler degrees for display
            let rot_quat = bevy::math::Quat::from_xyzw(
                toml_def.transform.rotation[0],
                toml_def.transform.rotation[1],
                toml_def.transform.rotation[2],
                toml_def.transform.rotation[3],
            );
            let (rx, ry, rz) = rot_quat.to_euler(bevy::math::EulerRot::XYZ);
            add_prop("Transform", "Rotation", format!("{:.2}, {:.2}, {:.2}",
                rx.to_degrees(), ry.to_degrees(), rz.to_degrees()), "rotation", true);
            add_prop("Transform", "Scale", format!("{:.3}, {:.3}, {:.3}", 
                toml_def.transform.scale[0], toml_def.transform.scale[1], toml_def.transform.scale[2]), "vec3", true);
            
            // ── UI class properties (TextLabel, TextButton, Frame, etc.) ────────
            // If the entity carries a UI ECS component, emit all its properties
            // via PropertyAccess.  Skip the BasePart Appearance/Physics sections
            // since UI classes do not have color/anchored/can_collide semantics.
            use eustress_common::classes::{ClassName, PropertyAccess};
            let is_ui_class = matches!(instance.class_name,
                ClassName::TextLabel | ClassName::TextButton | ClassName::TextBox |
                ClassName::Frame     | ClassName::ImageLabel | ClassName::ImageButton |
                ClassName::ScrollingFrame
            );
            if is_ui_class {
                // Emit all properties from the live ECS component so edits are
                // always round-tripped through the component, not re-read from disk.
                // Use the ParamSet accessors sequentially (only one is accessed at a time).
                let ui_props: Option<Vec<eustress_common::classes::PropertyDescriptor>> =
                    if let Ok(c) = ui_queries.p0().get(selected_entity)     { Some(c.list_properties()) }
                    else if let Ok(c) = ui_queries.p1().get(selected_entity)  { Some(c.list_properties()) }
                    else if let Ok(c) = ui_queries.p2().get(selected_entity)  { Some(c.list_properties()) }
                    else if let Ok(c) = ui_queries.p3().get(selected_entity)  { Some(c.list_properties()) }
                    else if let Ok(c) = ui_queries.p4().get(selected_entity)  { Some(c.list_properties()) }
                    else if let Ok(c) = ui_queries.p5().get(selected_entity)  { Some(c.list_properties()) }
                    else if let Ok(c) = ui_queries.p6().get(selected_entity)  { Some(c.list_properties()) }
                    else { None };

                if let Some(descriptors) = ui_props {
                    for desc in &descriptors {
                        let val_opt =
                            ui_queries.p0().get(selected_entity).ok().and_then(|c| c.get_property(&desc.name))
                            .or_else(|| ui_queries.p1().get(selected_entity).ok().and_then(|c| c.get_property(&desc.name)))
                            .or_else(|| ui_queries.p2().get(selected_entity).ok().and_then(|c| c.get_property(&desc.name)))
                            .or_else(|| ui_queries.p3().get(selected_entity).ok().and_then(|c| c.get_property(&desc.name)))
                            .or_else(|| ui_queries.p4().get(selected_entity).ok().and_then(|c| c.get_property(&desc.name)))
                            .or_else(|| ui_queries.p5().get(selected_entity).ok().and_then(|c| c.get_property(&desc.name)))
                            .or_else(|| ui_queries.p6().get(selected_entity).ok().and_then(|c| c.get_property(&desc.name)));
                        if let Some(val) = val_opt {
                            let (val_str, prop_type) = property_value_to_display(&val);
                            add_prop(&desc.category, &desc.name, val_str, prop_type, !desc.read_only);
                        }
                    }
                }
            } else {
            // -- Properties section (BasePart / non-UI) --
            add_prop("Appearance", "Color", format!("{}, {}, {}", 
                (toml_def.properties.color[0] * 255.0).round() as u8,
                (toml_def.properties.color[1] * 255.0).round() as u8,
                (toml_def.properties.color[2] * 255.0).round() as u8), "color", true);
            add_prop("Appearance", "Transparency", format!("{:.3}", toml_def.properties.transparency), "float", true);
            add_prop("Appearance", "Reflectance", format!("{:.3}", toml_def.properties.reflectance), "float", true);
            add_prop("Appearance", "CastShadow", toml_def.properties.cast_shadow.to_string(), "bool", true);
            add_prop("Appearance", "Material", toml_def.properties.material.clone(), "string", true);
            
            // -- Physics section --
            add_prop("Physics", "Anchored", toml_def.properties.anchored.to_string(), "bool", true);
            add_prop("Physics", "CanCollide", toml_def.properties.can_collide.to_string(), "bool", true);
            add_prop("Physics", "Locked", toml_def.properties.locked.to_string(), "bool", true);
            } // end non-UI branch
            
            // -- Material section (realism, optional) --
            if let Some(ref mat) = toml_def.material {
                add_prop("Material", "Name", mat.name.clone(), "string", true);
                add_prop("Material", "YoungModulus", format!("{:.2e}", mat.young_modulus), "float", true);
                add_prop("Material", "PoissonRatio", format!("{:.4}", mat.poisson_ratio), "float", true);
                add_prop("Material", "YieldStrength", format!("{:.2e}", mat.yield_strength), "float", true);
                add_prop("Material", "UltimateStrength", format!("{:.2e}", mat.ultimate_strength), "float", true);
                add_prop("Material", "FractureToughness", format!("{:.2e}", mat.fracture_toughness), "float", true);
                add_prop("Material", "Hardness", format!("{:.1}", mat.hardness), "float", true);
                add_prop("Material", "ThermalConductivity", format!("{:.3}", mat.thermal_conductivity), "float", true);
                add_prop("Material", "SpecificHeat", format!("{:.1}", mat.specific_heat), "float", true);
                add_prop("Material", "ThermalExpansion", format!("{:.2e}", mat.thermal_expansion), "float", true);
                add_prop("Material", "MeltingPoint", format!("{:.1}", mat.melting_point), "float", true);
                add_prop("Material", "Density", format!("{:.1}", mat.density), "float", true);
                add_prop("Material", "FrictionStatic", format!("{:.3}", mat.friction_static), "float", true);
                add_prop("Material", "FrictionKinetic", format!("{:.3}", mat.friction_kinetic), "float", true);
                add_prop("Material", "Restitution", format!("{:.3}", mat.restitution), "float", true);
                // Custom properties
                for (key, val) in &mat.custom {
                    let val_str = match val {
                        toml::Value::Float(f) => format!("{:.4}", f),
                        toml::Value::Integer(i) => i.to_string(),
                        toml::Value::String(s) => s.clone(),
                        toml::Value::Boolean(b) => b.to_string(),
                        _ => format!("{:?}", val),
                    };
                    add_prop("Material", key, val_str, "string", true);
                }
            }
            
            // -- Thermodynamic section (realism, optional) --
            if let Some(ref thermo) = toml_def.thermodynamic {
                add_prop("Thermodynamic", "Temperature", format!("{:.2}", thermo.temperature), "float", true);
                add_prop("Thermodynamic", "Pressure", format!("{:.1}", thermo.pressure), "float", true);
                add_prop("Thermodynamic", "Volume", format!("{:.6}", thermo.volume), "float", true);
                add_prop("Thermodynamic", "InternalEnergy", format!("{:.2}", thermo.internal_energy), "float", true);
                add_prop("Thermodynamic", "Entropy", format!("{:.4}", thermo.entropy), "float", true);
                add_prop("Thermodynamic", "Enthalpy", format!("{:.2}", thermo.enthalpy), "float", true);
                add_prop("Thermodynamic", "Moles", format!("{:.4}", thermo.moles), "float", true);
            }
            
            // -- Electrochemical section (realism, optional) --
            if let Some(ref echem) = toml_def.electrochemical {
                add_prop("Electrochemical", "Voltage", format!("{:.4}", echem.voltage), "float", true);
                add_prop("Electrochemical", "TerminalVoltage", format!("{:.4}", echem.terminal_voltage), "float", true);
                add_prop("Electrochemical", "CapacityAh", format!("{:.2}", echem.capacity_ah), "float", true);
                add_prop("Electrochemical", "SOC", format!("{:.4}", echem.soc), "float", true);
                add_prop("Electrochemical", "Current", format!("{:.4}", echem.current), "float", true);
                add_prop("Electrochemical", "InternalResistance", format!("{:.6}", echem.internal_resistance), "float", true);
                add_prop("Electrochemical", "IonicConductivity", format!("{:.6}", echem.ionic_conductivity), "float", true);
                add_prop("Electrochemical", "CycleCount", echem.cycle_count.to_string(), "int", true);
                add_prop("Electrochemical", "CRate", format!("{:.3}", echem.c_rate), "float", true);
                add_prop("Electrochemical", "CapacityRetention", format!("{:.4}", echem.capacity_retention), "float", true);
                add_prop("Electrochemical", "HeatGeneration", format!("{:.6}", echem.heat_generation), "float", true);
                add_prop("Electrochemical", "DendriteRisk", format!("{:.4}", echem.dendrite_risk), "float", true);
            }
        } else {
            // Fallback: show basic instance info if TOML load fails
            add_prop("Data", "Name", instance.name.clone(), "string", true);
            add_prop("Data", "ClassName", format!("{:?}", instance.class_name), "string", false);
            add_prop("Data", "Archivable", instance.archivable.to_string(), "bool", true);
            add_prop("Data", "Error", "Failed to load TOML file".to_string(), "string", false);
        }
    } else if let Ok(service) = service_components.get(selected_entity) {
        // ══════════════════════════════════════════════════════════════════════════
        // SERVICE ENTITY: Display ALL properties dynamically from ServiceComponent
        // No hardcoding - any property in _service.toml is displayed
        // ══════════════════════════════════════════════════════════════════════════
        add_prop("Service", "ClassName", service.class_name.clone(), "string", false);
        add_prop("Service", "Icon", service.icon.clone(), "string", true);
        if !service.description.is_empty() {
            add_prop("Service", "Description", service.description.clone(), "string", true);
        }
        add_prop("Service", "TomlPath", service.toml_path.display().to_string(), "string", false);
        
        // Display ALL dynamic properties from the TOML file
        // Properties are sorted alphabetically for consistent display
        let mut prop_names: Vec<&String> = service.properties.keys().collect();
        prop_names.sort();
        
        for prop_name in prop_names {
            if let Some(prop_value) = service.properties.get(prop_name) {
                add_prop(
                    "Properties",
                    prop_name,
                    prop_value.to_display_string(),
                    prop_value.type_name(),
                    true, // All dynamic properties are editable
                );
            }
        }
    } else {
        // No InstanceFile or ServiceComponent — show ECS-based properties (legacy/programmatic entities)
        add_prop("Data", "Name", instance.name.clone(), "string", true);
        add_prop("Data", "ClassName", format!("{:?}", instance.class_name), "string", false);
        add_prop("Data", "Archivable", instance.archivable.to_string(), "bool", true);
        
        // Transform from Bevy
        if let Ok(transform) = transforms.get(selected_entity) {
            let (rx, ry, rz) = transform.rotation.to_euler(bevy::math::EulerRot::XYZ);
            add_prop("Transform", "Position",
                format!("{:.2}, {:.2}, {:.2}", transform.translation.x, transform.translation.y, transform.translation.z),
                "vec3", true);
            add_prop("Transform", "Rotation",
                format!("{:.1}, {:.1}, {:.1}", rx.to_degrees(), ry.to_degrees(), rz.to_degrees()),
                "rotation", true);
            add_prop("Transform", "Scale",
                format!("{:.2}, {:.2}, {:.2}", transform.scale.x, transform.scale.y, transform.scale.z),
                "vec3", true);
        }
        
        // BasePart properties
        use eustress_common::classes::PropertyAccess;
        let transform_props = ["Position", "Orientation", "Size", "Rotation", "Scale"];
        if let Ok(base_part) = base_parts.get(selected_entity) {
            for prop_desc in base_part.list_properties() {
                if transform_props.contains(&prop_desc.name.as_str()) { continue; }
                if let Some(value) = base_part.get_property(&prop_desc.name) {
                    let (val_str, prop_type) = property_value_to_display(&value);
                    add_prop(&prop_desc.category, &prop_desc.name, val_str, prop_type, !prop_desc.read_only);
                }
            }
        }
    }
    
    // Build flat list with category headers interleaved
    // Sort categories alphabetically (A-Z)
    let mut ordered_categories: Vec<String> = categorized.keys().cloned().collect();
    ordered_categories.sort();
    
    let mut flat_props: Vec<PropertyData> = Vec::new();
    for cat in &ordered_categories {
        // Insert category header
        let is_collapsed = studio_state.collapsed_sections.contains(cat);
        flat_props.push(PropertyData {
            name: slint::SharedString::default(),
            value: slint::SharedString::default(),
            property_type: slint::SharedString::default(),
            category: cat.as_str().into(),
            editable: false,
            options: slint::ModelRc::default(),
            is_header: true,
            section_collapsed: is_collapsed,
            x_value: slint::SharedString::default(),
            y_value: slint::SharedString::default(),
            z_value: slint::SharedString::default(),
            description: slint::SharedString::default(),
            learn_url: slint::SharedString::default(),
        });
        
        // Insert properties in this category (sorted A-Z by name)
        if let Some(entries) = categorized.get(cat.as_str()) {
            let mut sorted_entries = entries.clone();
            sorted_entries.sort_by(|a, b| a.0.cmp(&b.0));
            
            for (name, value, prop_type, editable) in sorted_entries {
                // Parse Vec3/rotation values into x, y, z components
                let (x_val, y_val, z_val) = if prop_type == "vec3" || prop_type == "rotation" {
                    parse_vec3_string(&value)
                } else {
                    (String::new(), String::new(), String::new())
                };
                
                flat_props.push(PropertyData {
                    name: name.as_str().into(),
                    value: value.as_str().into(),
                    property_type: prop_type.as_str().into(),
                    category: cat.as_str().into(),
                    editable,
                    options: slint::ModelRc::default(),
                    is_header: false,
                    section_collapsed: is_collapsed,
                    x_value: x_val.into(),
                    y_value: y_val.into(),
                    z_value: z_val.into(),
                    description: slint::SharedString::default(),
                    learn_url: slint::SharedString::default(),
                });
            }
        }
    }
    
    // Hash-based change detection: only push to Slint when the model data actually
    // changes. Re-pushing an identical model destroys and recreates all `for` loop
    // items in Slint, which resets hover state and causes visible flickering.
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for prop in &flat_props {
        prop.name.hash(&mut hasher);
        prop.value.hash(&mut hasher);
        prop.category.hash(&mut hasher);
        prop.property_type.hash(&mut hasher);
        prop.is_header.hash(&mut hasher);
        prop.section_collapsed.hash(&mut hasher);
        prop.x_value.hash(&mut hasher);
        prop.y_value.hash(&mut hasher);
        prop.z_value.hash(&mut hasher);
    }
    let new_hash = hasher.finish();
    
    if new_hash != studio_state.last_properties_hash {
        studio_state.last_properties_hash = new_hash;
        let model_rc = std::rc::Rc::new(slint::VecModel::from(flat_props));
        ui.set_entity_properties(slint::ModelRc::from(model_rc));
    }
}

/// Converts a property name + string value into a typed PropertyValue.
/// Uses the property name to infer the expected type, mirroring how
/// PropertyAccess::set_property dispatches on the name.
fn property_string_to_value(name: &str, val: &str) -> Option<eustress_common::classes::PropertyValue> {
    use eustress_common::classes::PropertyValue;
    // Helper: parse "r, g, b" → Color3
    fn parse_c3(s: &str) -> Option<PropertyValue> {
        let p: Vec<f32> = s.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if p.len() >= 3 { Some(PropertyValue::Color3([p[0], p[1], p[2]])) } else { None }
    }
    // Helper: parse "x, y" → Vector2
    fn parse_v2(s: &str) -> Option<PropertyValue> {
        let p: Vec<f32> = s.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if p.len() >= 2 { Some(PropertyValue::Vector2([p[0], p[1]])) } else { None }
    }
    match name {
        // Boolean properties
        "Visible" | "Active" | "AutoButtonColor" | "RichText" | "TextScaled" |
        "TextWrapped" | "ClipsDescendants" | "ScrollingEnabled" =>
            Some(PropertyValue::Bool(val == "true")),
        // Float properties
        "FontSize" | "LineHeight" | "TextTransparency" | "TextStrokeTransparency" |
        "BackgroundTransparency" | "ImageTransparency" | "Rotation" =>
            val.trim().parse::<f32>().ok().map(PropertyValue::Float),
        // Int properties
        "BorderSizePixel" | "ZIndex" | "LayoutOrder" | "ScrollBarThickness" =>
            val.trim().parse::<i32>().ok().map(PropertyValue::Int),
        // String properties
        "Text" | "Font" | "Image" | "ScaleType" | "AutomaticSize" =>
            Some(PropertyValue::String(val.to_string())),
        // Enum properties
        "TextXAlignment" | "TextYAlignment" | "BorderMode" =>
            Some(PropertyValue::Enum(val.to_string())),
        // Color3 properties
        "TextColor3" | "TextStrokeColor3" | "BackgroundColor3" | "BorderColor3" |
        "ImageColor3" => parse_c3(val),
        // Vector2 properties
        "AnchorPoint" | "PositionScale" | "PositionOffset" | "SizeScale" | "SizeOffset" =>
            parse_v2(val),
        _ => None,
    }
}

/// Converts a PropertyValue to a display string and type identifier
fn property_value_to_display(value: &eustress_common::classes::PropertyValue) -> (String, &'static str) {
    use eustress_common::classes::PropertyValue;
    match value {
        PropertyValue::String(s) => (s.clone(), "string"),
        PropertyValue::Float(f) => (format!("{:.3}", f), "float"),
        PropertyValue::Int(i) => (i.to_string(), "int"),
        PropertyValue::Bool(b) => (b.to_string(), "bool"),
        PropertyValue::Vector3(v) => (format!("{:.2}, {:.2}, {:.2}", v.x, v.y, v.z), "vec3"),
        PropertyValue::Color(c) => {
            let srgba = c.to_srgba();
            (format!("#{:02x}{:02x}{:02x}", (srgba.red * 255.0) as u8, (srgba.green * 255.0) as u8, (srgba.blue * 255.0) as u8), "color")
        }
        PropertyValue::Color3(c) => (format!("{:.2}, {:.2}, {:.2}", c[0], c[1], c[2]), "color"),
        PropertyValue::Transform(t) => (format!("({:.1}, {:.1}, {:.1})", t.translation.x, t.translation.y, t.translation.z), "string"),
        PropertyValue::Material(m) => (format!("{:?}", m), "enum"),
        PropertyValue::Enum(e) => (e.clone(), "enum"),
        PropertyValue::Vector2(v) => (format!("{:.2}, {:.2}", v[0], v[1]), "string"),
    }
}

/// Updates a property in the InstanceDefinition based on property name
/// Returns true if a property was changed
fn update_toml_property(
    def: &mut crate::space::instance_loader::InstanceDefinition,
    key: &str,
    val: &str,
) -> bool {
    match key {
        // Metadata (Name is derived from filename, not stored in TOML)
        "Name" => { false } // Name changes require file rename, not TOML edit
        "Archivable" => { def.metadata.archivable = val == "true"; true }
        
        // Asset
        "Mesh" => {
            def.asset.get_or_insert_with(|| crate::space::instance_loader::AssetReference {
                mesh: String::new(), scene: "Scene0".to_string(),
            }).mesh = val.to_string(); true
        }
        "Scene" => {
            def.asset.get_or_insert_with(|| crate::space::instance_loader::AssetReference {
                mesh: String::new(), scene: "Scene0".to_string(),
            }).scene = val.to_string(); true
        }
        
        // Transform
        "Position" => {
            if let Some((x, y, z)) = parse_vec3_value(val) {
                def.transform.position = [x, y, z]; true
            } else { false }
        }
        "Scale" => {
            if let Some((x, y, z)) = parse_vec3_value(val) {
                def.transform.scale = [x, y, z]; true
            } else { false }
        }
        "Rotation" => {
            // Accept Euler degrees Vec3 "x, y, z" and convert to quaternion [x, y, z, w]
            if let Some((ex, ey, ez)) = parse_vec3_value(val) {
                let q = bevy::math::Quat::from_euler(
                    bevy::math::EulerRot::XYZ,
                    ex.to_radians(),
                    ey.to_radians(),
                    ez.to_radians(),
                );
                def.transform.rotation = [q.x, q.y, q.z, q.w]; true
            } else { false }
        }
        
        // Appearance (properties section)
        "Color" => {
            // Accept 0-255 integer RGB input from Properties panel, convert to 0.0-1.0 internal
            let parts: Vec<f32> = val.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            if parts.len() >= 3 {
                // Heuristic: if any value > 1.0, treat all as 0-255 integers
                let is_u8 = parts.iter().any(|&v| v > 1.0);
                if is_u8 {
                    def.properties.color = [
                        parts[0] / 255.0, parts[1] / 255.0, parts[2] / 255.0,
                        parts.get(3).map(|&a| a / 255.0).unwrap_or(1.0),
                    ]; true
                } else {
                    def.properties.color = [parts[0], parts[1], parts[2], parts.get(3).copied().unwrap_or(1.0)]; true
                }
            } else { false }
        }
        "Transparency" => { if let Ok(v) = val.parse() { def.properties.transparency = v; true } else { false } }
        "Reflectance" => { if let Ok(v) = val.parse() { def.properties.reflectance = v; true } else { false } }
        "CastShadow" => { def.properties.cast_shadow = val == "true"; true }
        "Material" => { def.properties.material = val.to_string(); true }
        
        // Physics (properties section)
        "Anchored" => { def.properties.anchored = val == "true"; true }
        "CanCollide" => { def.properties.can_collide = val == "true"; true }
        "Locked" => { def.properties.locked = val == "true"; true }
        
        // Material section (realism)
        k if k.starts_with("Material.") || is_material_prop(k) => {
            let mat = def.material.get_or_insert_with(Default::default);
            update_material_property(mat, k, val)
        }
        
        // Thermodynamic section (realism)
        k if k.starts_with("Thermodynamic.") || is_thermo_prop(k) => {
            let thermo = def.thermodynamic.get_or_insert_with(Default::default);
            update_thermo_property(thermo, k, val)
        }
        
        // Electrochemical section (realism)
        k if k.starts_with("Electrochemical.") || is_echem_prop(k) => {
            let echem = def.electrochemical.get_or_insert_with(Default::default);
            update_echem_property(echem, k, val)
        }
        
        // UI class properties — routed to [ui] section
        k if is_ui_prop(k) => {
            let ui = def.ui.get_or_insert_with(Default::default);
            update_ui_property(ui, k, val)
        }
        
        _ => false
    }
}

fn is_material_prop(k: &str) -> bool {
    matches!(k, "YoungModulus" | "PoissonRatio" | "YieldStrength" | "UltimateStrength" | 
        "FractureToughness" | "Hardness" | "ThermalConductivity" | "SpecificHeat" | 
        "ThermalExpansion" | "MeltingPoint" | "Density" | "FrictionStatic" | 
        "FrictionKinetic" | "Restitution")
}

fn is_thermo_prop(k: &str) -> bool {
    matches!(k, "Temperature" | "Pressure" | "Volume" | "InternalEnergy" | "Entropy" | "Enthalpy" | "Moles")
}

fn is_echem_prop(k: &str) -> bool {
    matches!(k, "Voltage" | "TerminalVoltage" | "CapacityAh" | "SOC" | "Current" | 
        "InternalResistance" | "IonicConductivity" | "CycleCount" | "CRate" | 
        "CapacityRetention" | "HeatGeneration" | "DendriteRisk")
}

fn update_material_property(mat: &mut crate::space::instance_loader::TomlMaterialProperties, key: &str, val: &str) -> bool {
    let k = key.strip_prefix("Material.").unwrap_or(key);
    match k {
        "Name" => { mat.name = val.to_string(); true }
        "YoungModulus" | "young_modulus" => { if let Ok(v) = val.parse() { mat.young_modulus = v; true } else { false } }
        "PoissonRatio" | "poisson_ratio" => { if let Ok(v) = val.parse() { mat.poisson_ratio = v; true } else { false } }
        "YieldStrength" | "yield_strength" => { if let Ok(v) = val.parse() { mat.yield_strength = v; true } else { false } }
        "UltimateStrength" | "ultimate_strength" => { if let Ok(v) = val.parse() { mat.ultimate_strength = v; true } else { false } }
        "FractureToughness" | "fracture_toughness" => { if let Ok(v) = val.parse() { mat.fracture_toughness = v; true } else { false } }
        "Hardness" | "hardness" => { if let Ok(v) = val.parse() { mat.hardness = v; true } else { false } }
        "ThermalConductivity" | "thermal_conductivity" => { if let Ok(v) = val.parse() { mat.thermal_conductivity = v; true } else { false } }
        "SpecificHeat" | "specific_heat" => { if let Ok(v) = val.parse() { mat.specific_heat = v; true } else { false } }
        "ThermalExpansion" | "thermal_expansion" => { if let Ok(v) = val.parse() { mat.thermal_expansion = v; true } else { false } }
        "MeltingPoint" | "melting_point" => { if let Ok(v) = val.parse() { mat.melting_point = v; true } else { false } }
        "Density" | "density" => { if let Ok(v) = val.parse() { mat.density = v; true } else { false } }
        "FrictionStatic" | "friction_static" => { if let Ok(v) = val.parse() { mat.friction_static = v; true } else { false } }
        "FrictionKinetic" | "friction_kinetic" => { if let Ok(v) = val.parse() { mat.friction_kinetic = v; true } else { false } }
        "Restitution" | "restitution" => { if let Ok(v) = val.parse() { mat.restitution = v; true } else { false } }
        // Custom properties
        _ => {
            if let Ok(f) = val.parse::<f64>() {
                mat.custom.insert(k.to_string(), toml::Value::Float(f));
            } else {
                mat.custom.insert(k.to_string(), toml::Value::String(val.to_string()));
            }
            true
        }
    }
}

fn update_thermo_property(thermo: &mut crate::space::instance_loader::TomlThermodynamicState, key: &str, val: &str) -> bool {
    let k = key.strip_prefix("Thermodynamic.").unwrap_or(key);
    match k {
        "Temperature" | "temperature" => { if let Ok(v) = val.parse() { thermo.temperature = v; true } else { false } }
        "Pressure" | "pressure" => { if let Ok(v) = val.parse() { thermo.pressure = v; true } else { false } }
        "Volume" | "volume" => { if let Ok(v) = val.parse() { thermo.volume = v; true } else { false } }
        "InternalEnergy" | "internal_energy" => { if let Ok(v) = val.parse() { thermo.internal_energy = v; true } else { false } }
        "Entropy" | "entropy" => { if let Ok(v) = val.parse() { thermo.entropy = v; true } else { false } }
        "Enthalpy" | "enthalpy" => { if let Ok(v) = val.parse() { thermo.enthalpy = v; true } else { false } }
        "Moles" | "moles" => { if let Ok(v) = val.parse() { thermo.moles = v; true } else { false } }
        _ => false
    }
}

fn update_echem_property(echem: &mut crate::space::instance_loader::TomlElectrochemicalState, key: &str, val: &str) -> bool {
    let k = key.strip_prefix("Electrochemical.").unwrap_or(key);
    match k {
        "Voltage" | "voltage" => { if let Ok(v) = val.parse() { echem.voltage = v; true } else { false } }
        "TerminalVoltage" | "terminal_voltage" => { if let Ok(v) = val.parse() { echem.terminal_voltage = v; true } else { false } }
        "CapacityAh" | "capacity_ah" => { if let Ok(v) = val.parse() { echem.capacity_ah = v; true } else { false } }
        "SOC" | "soc" => { if let Ok(v) = val.parse() { echem.soc = v; true } else { false } }
        "Current" | "current" => { if let Ok(v) = val.parse() { echem.current = v; true } else { false } }
        "InternalResistance" | "internal_resistance" => { if let Ok(v) = val.parse() { echem.internal_resistance = v; true } else { false } }
        "IonicConductivity" | "ionic_conductivity" => { if let Ok(v) = val.parse() { echem.ionic_conductivity = v; true } else { false } }
        "CycleCount" | "cycle_count" => { if let Ok(v) = val.parse() { echem.cycle_count = v; true } else { false } }
        "CRate" | "c_rate" => { if let Ok(v) = val.parse() { echem.c_rate = v; true } else { false } }
        "CapacityRetention" | "capacity_retention" => { if let Ok(v) = val.parse() { echem.capacity_retention = v; true } else { false } }
        "HeatGeneration" | "heat_generation" => { if let Ok(v) = val.parse() { echem.heat_generation = v; true } else { false } }
        "DendriteRisk" | "dendrite_risk" => { if let Ok(v) = val.parse() { echem.dendrite_risk = v; true } else { false } }
        _ => false
    }
}

/// Returns true if the property key belongs to a UI class [ui] section
fn is_ui_prop(k: &str) -> bool {
    matches!(k,
        "Text" | "RichText" | "TextScaled" | "TextWrapped" | "Font" |
        "FontSize" | "LineHeight" |
        "TextColor3" | "TextTransparency" | "TextStrokeColor3" | "TextStrokeTransparency" |
        "TextXAlignment" | "TextYAlignment" |
        "BackgroundColor3" | "BackgroundTransparency" | "BorderColor3" | "BorderSizePixel" |
        "BorderMode" | "ClipsDescendants" | "ZIndex" | "LayoutOrder" | "Rotation" |
        "AnchorPoint" | "PositionScale" | "PositionOffset" | "SizeScale" | "SizeOffset" |
        "Visible" | "Active" | "AutoButtonColor" |
        "Image" | "ImageColor3" | "ImageTransparency" | "ScaleType" |
        "ScrollingEnabled" | "ScrollBarThickness" | "AutomaticSize"
    )
}

/// Write a single UI property value into a UiInstanceProperties struct for TOML persistence.
/// Returns true if the key was recognised and written.
fn update_ui_property(
    ui: &mut crate::space::instance_loader::UiInstanceProperties,
    key: &str,
    val: &str,
) -> bool {
    /// Parse "r, g, b" → [f32; 3]
    fn parse_color3(s: &str) -> Option<[f32; 3]> {
        let p: Vec<f32> = s.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if p.len() >= 3 { Some([p[0], p[1], p[2]]) } else { None }
    }
    /// Parse "x, y" → [f32; 2]
    fn parse_vec2(s: &str) -> Option<[f32; 2]> {
        let p: Vec<f32> = s.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if p.len() >= 2 { Some([p[0], p[1]]) } else { None }
    }

    match key {
        "Text"                   => { ui.text = val.to_string(); true }
        "RichText"               => { ui.rich_text = val == "true"; true }
        "TextScaled"             => { ui.text_scaled = val == "true"; true }
        "TextWrapped"            => { ui.text_wrapped = val == "true"; true }
        "Font"                   => { ui.font = val.to_string(); true }
        "FontSize"               => { if let Ok(v) = val.parse::<f32>() { ui.font_size = v.max(1.0); true } else { false } }
        "LineHeight"             => { if let Ok(v) = val.parse::<f32>() { ui.line_height = v; true } else { false } }
        "TextColor3"             => { if let Some(c) = parse_color3(val) { ui.text_color3 = c; true } else { false } }
        "TextTransparency"       => { if let Ok(v) = val.parse::<f32>() { ui.text_transparency = v.clamp(0.0,1.0); true } else { false } }
        "TextStrokeColor3"       => { if let Some(c) = parse_color3(val) { ui.text_stroke_color3 = c; true } else { false } }
        "TextStrokeTransparency" => { if let Ok(v) = val.parse::<f32>() { ui.text_stroke_transparency = v.clamp(0.0,1.0); true } else { false } }
        "TextXAlignment"         => { ui.text_x_alignment = val.to_string(); true }
        "TextYAlignment"         => { ui.text_y_alignment = val.to_string(); true }
        "BackgroundColor3"       => { if let Some(c) = parse_color3(val) { ui.background_color3 = c; true } else { false } }
        "BackgroundTransparency" => { if let Ok(v) = val.parse::<f32>() { ui.background_transparency = v.clamp(0.0,1.0); true } else { false } }
        "BorderColor3"           => { if let Some(c) = parse_color3(val) { ui.border_color3 = c; true } else { false } }
        "BorderSizePixel"        => { if let Ok(v) = val.parse::<i32>() { ui.border_size_pixel = v.max(0); true } else { false } }
        "BorderMode"             => { ui.border_mode = val.to_string(); true }
        "ClipsDescendants"       => { ui.clips_descendants = val == "true"; true }
        "ZIndex"                 => { if let Ok(v) = val.parse::<i32>() { ui.z_index = v; true } else { false } }
        "LayoutOrder"            => { if let Ok(v) = val.parse::<i32>() { ui.layout_order = v; true } else { false } }
        "Rotation"               => { if let Ok(v) = val.parse::<f32>() { ui.rotation = v; true } else { false } }
        "AnchorPoint"            => { if let Some(v) = parse_vec2(val) { ui.anchor_point = v; true } else { false } }
        "PositionScale"          => { if let Some(v) = parse_vec2(val) { ui.position_scale = v; true } else { false } }
        "PositionOffset"         => { if let Some(v) = parse_vec2(val) { ui.position_offset = v; true } else { false } }
        "SizeScale"              => { if let Some(v) = parse_vec2(val) { ui.size_scale = v; true } else { false } }
        "SizeOffset"             => { if let Some(v) = parse_vec2(val) { ui.size_offset = v; true } else { false } }
        "Visible"                => { ui.visible = val == "true"; true }
        "Active"                 => { ui.active = val == "true"; true }
        "AutoButtonColor"        => { ui.auto_button_color = val == "true"; true }
        "Image"                  => { ui.image = val.to_string(); true }
        "ImageColor3"            => { if let Some(c) = parse_color3(val) { ui.image_color3 = c; true } else { false } }
        "ImageTransparency"      => { if let Ok(v) = val.parse::<f32>() { ui.image_transparency = v.clamp(0.0,1.0); true } else { false } }
        "ScaleType"              => { ui.scale_type = val.to_string(); true }
        "ScrollingEnabled"       => { ui.scrolling_enabled = val == "true"; true }
        "ScrollBarThickness"     => { if let Ok(v) = val.parse::<i32>() { ui.scroll_bar_thickness = v.max(0); true } else { false } }
        "AutomaticSize"          => { ui.automatic_size = val.to_string(); true }
        _ => false
    }
}

/// Parses a Vec3 string "x, y, z" into f32 tuple for TOML write-back
fn parse_vec3_value(value: &str) -> Option<(f32, f32, f32)> {
    let parts: Vec<&str> = value.split(',').map(|s| s.trim()).collect();
    if parts.len() == 3 {
        let x = parts[0].parse::<f32>().ok()?;
        let y = parts[1].parse::<f32>().ok()?;
        let z = parts[2].parse::<f32>().ok()?;
        Some((x, y, z))
    } else {
        None
    }
}

/// Parses a Color4 string "r, g, b, a" into f32 tuple for TOML write-back
fn parse_color4_value(value: &str) -> Option<(f32, f32, f32, f32)> {
    let parts: Vec<&str> = value.split(',').map(|s| s.trim()).collect();
    if parts.len() == 4 {
        let r = parts[0].parse::<f32>().ok()?;
        let g = parts[1].parse::<f32>().ok()?;
        let b = parts[2].parse::<f32>().ok()?;
        let a = parts[3].parse::<f32>().ok()?;
        Some((r, g, b, a))
    } else if parts.len() == 3 {
        // RGB without alpha - default alpha to 1.0
        let r = parts[0].parse::<f32>().ok()?;
        let g = parts[1].parse::<f32>().ok()?;
        let b = parts[2].parse::<f32>().ok()?;
        Some((r, g, b, 1.0))
    } else {
        None
    }
}

/// Parses a Vec3 string "x, y, z" into individual components
fn parse_vec3_string(value: &str) -> (String, String, String) {
    let parts: Vec<&str> = value.split(',').map(|s| s.trim()).collect();
    match parts.as_slice() {
        [x, y, z] => (x.to_string(), y.to_string(), z.to_string()),
        [x, y] => (x.to_string(), y.to_string(), "0".to_string()),
        [x] => (x.to_string(), "0".to_string(), "0".to_string()),
        _ => ("0".to_string(), "0".to_string(), "0".to_string()),
    }
}

/// Builds filesystem properties for a selected file and pushes them to the Slint Properties panel.
/// Shows: Name, Path, Extension, Size, Type, Modified, Created, Read-Only.
fn build_file_properties(ui: &StudioWindow, path: &std::path::Path) {
    let file_name = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string();
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    let is_dir = path.is_dir();
    let type_label = if is_dir { "Directory" } else { "File" };

    ui.set_selected_count(1);
    ui.set_selected_class(if is_dir { "Directory".into() } else { ext.as_str().into() });

    let mut props: Vec<PropertyData> = Vec::new();

    // Helper: create a property entry
    let make_prop = |name: &str, value: &str, prop_type: &str, category: &str, is_hdr: bool| -> PropertyData {
        PropertyData {
            name: name.into(),
            value: value.into(),
            property_type: prop_type.into(),
            editable: false,
            category: category.into(),
            options: slint::ModelRc::default(),
            is_header: is_hdr,
            section_collapsed: false,
            x_value: slint::SharedString::default(),
            y_value: slint::SharedString::default(),
            z_value: slint::SharedString::default(),
            description: slint::SharedString::default(),
            learn_url: slint::SharedString::default(),
        }
    };

    // Category header: File Info
    props.push(make_prop("", "", "", "File Info", true));
    props.push(make_prop("Name", &file_name, "string", "File Info", false));
    props.push(make_prop("Type", type_label, "string", "File Info", false));

    // Extension (files only)
    if !is_dir && !ext.is_empty() {
        props.push(make_prop("Extension", &format!(".{}", ext), "string", "File Info", false));
    }

    // Path
    props.push(make_prop("Path", &path.to_string_lossy(), "string", "File Info", false));

    // Metadata-dependent properties
    if let Ok(metadata) = std::fs::metadata(path) {
        // Size
        let size_str = if is_dir {
            std::fs::read_dir(path)
                .map(|entries| {
                    let count = entries.filter_map(|e| e.ok()).count();
                    format!("{} items", count)
                })
                .unwrap_or_else(|_| "Unknown".to_string())
        } else {
            super::file_icons::format_file_size(metadata.len())
        };

        // Category header: Details
        props.push(make_prop("", "", "", "Details", true));
        props.push(make_prop("Size", &size_str, "string", "Details", false));
        props.push(make_prop("Read Only", &metadata.permissions().readonly().to_string(), "bool", "Details", false));

        // Modified time
        if let Ok(modified) = metadata.modified() {
            let datetime: chrono::DateTime<chrono::Local> = modified.into();
            props.push(make_prop("Modified", &datetime.format("%Y-%m-%d %H:%M:%S").to_string(), "string", "Details", false));
        }

        // Created time
        if let Ok(created) = metadata.created() {
            let datetime: chrono::DateTime<chrono::Local> = created.into();
            props.push(make_prop("Created", &datetime.format("%Y-%m-%d %H:%M:%S").to_string(), "string", "Details", false));
        }
    }

    let model_rc = std::rc::Rc::new(slint::VecModel::from(props));
    ui.set_entity_properties(slint::ModelRc::from(model_rc));
}

/// Load service properties from TOML definition file
fn load_service_properties_from_toml(service_name: &str) -> Option<toml::Value> {
    let toml_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("service_properties")
        .join(format!("{}.toml", service_name));
    
    if let Ok(content) = std::fs::read_to_string(&toml_path) {
        toml::from_str(&content).ok()
    } else {
        None
    }
}

/// Builds service properties for a selected service header and pushes them to the Properties panel.
/// Reads properties from TOML definition files (Roblox-style) with fallback to ServiceComponent.
fn build_service_properties(
    ui: &StudioWindow,
    service_name: &str,
    service_components: &Query<&crate::space::service_loader::ServiceComponent>,
) {
    use crate::space::service_loader::PropertyValue;
    
    ui.set_selected_count(1);
    ui.set_selected_class(service_name.into());
    
    // Set the correct icon for the service — look up ServiceComponent.icon first,
    // then fall back to deriving from the service name (lowercase).
    let icon_name: String = service_components
        .iter()
        .find(|sc| sc.class_name == service_name)
        .map(|sc| sc.icon.clone())
        .unwrap_or_else(|| service_name.to_lowercase());
    ui.set_selected_icon(load_service_icon(&icon_name));
    
    let make_prop = |name: &str, value: &str, prop_type: &str, category: &str, is_hdr: bool| -> PropertyData {
        PropertyData {
            name: name.into(),
            value: value.into(),
            property_type: prop_type.into(),
            editable: true,
            category: category.into(),
            options: slint::ModelRc::default(),
            is_header: is_hdr,
            section_collapsed: false,
            x_value: slint::SharedString::default(),
            y_value: slint::SharedString::default(),
            z_value: slint::SharedString::default(),
            description: slint::SharedString::default(),
            learn_url: slint::SharedString::default(),
        }
    };
    
    let mut props: Vec<PropertyData> = Vec::new();
    
    // Try to load properties from TOML definition file first (Roblox-style)
    if let Some(toml_data) = load_service_properties_from_toml(service_name) {
        // Parse TOML sections and build properties
        if let Some(table) = toml_data.as_table() {
            for (section_name, section_value) in table.iter() {
                if let Some(section_table) = section_value.as_table() {
                    // Add section header
                    props.push(make_prop("", "", "", section_name, true));
                    
                    // Add properties in this section
                    for (prop_name, prop_value) in section_table.iter() {
                        if let Some(prop_table) = prop_value.as_table() {
                            let value_str = prop_table.get("value")
                                .and_then(|v| match v {
                                    toml::Value::String(s) => Some(s.clone()),
                                    toml::Value::Integer(i) => Some(i.to_string()),
                                    toml::Value::Float(f) => Some(f.to_string()),
                                    toml::Value::Boolean(b) => Some(b.to_string()),
                                    toml::Value::Array(arr) => Some(format!("{:?}", arr)),
                                    _ => None,
                                })
                                .unwrap_or_default();
                            
                            let type_str = prop_table.get("type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("string");
                            
                            let readonly = prop_table.get("readonly")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            
                            let description_str = prop_table.get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            let mut prop_data = make_prop(prop_name, &value_str, type_str, section_name, false);
                            prop_data.editable = !readonly;
                            prop_data.description = description_str.as_str().into();
                            props.push(prop_data);
                        }
                    }
                }
            }
        }
    } else {
        // Fallback: use ServiceComponent properties
        let sc = service_components.iter().find(|sc| sc.class_name == service_name);
        
        // Service header
        props.push(make_prop("", "", "", "Service", true));
        props.push(make_prop("ClassName", service_name, "string", "Service", false));
    
    if let Some(sc) = sc {
        if !sc.description.is_empty() {
            props.push(make_prop("Description", &sc.description, "string", "Service", false));
        }
        props.push(make_prop("CanHaveChildren", &sc.can_have_children.to_string(), "bool", "Service", false));
        
        // Lighting-specific typed property sections
        if sc.class_name == "Lighting" {
            props.push(make_prop("", "", "", "Lighting", true));
            if let Some(v) = sc.properties.get("ambient") {
                props.push(make_prop("Ambient", &v.to_display_string(), "color", "Lighting", false));
            }
            if let Some(v) = sc.properties.get("outdoor_ambient") {
                props.push(make_prop("OutdoorAmbient", &v.to_display_string(), "color", "Lighting", false));
            }
            if let Some(v) = sc.properties.get("brightness") {
                props.push(make_prop("Brightness", &v.to_display_string(), "float", "Lighting", false));
            }
            if let Some(v) = sc.properties.get("global_shadows") {
                props.push(make_prop("GlobalShadows", &v.to_display_string(), "bool", "Lighting", false));
            }
            props.push(make_prop("", "", "", "Time", true));
            if let Some(v) = sc.properties.get("clock_time") {
                props.push(make_prop("ClockTime", &v.to_display_string(), "float", "Time", false));
            }
            if let Some(v) = sc.properties.get("geographic_latitude") {
                props.push(make_prop("GeographicLatitude", &v.to_display_string(), "float", "Time", false));
            }
        } else {
            // Generic dynamic properties for all other services
            if !sc.properties.is_empty() {
                props.push(make_prop("", "", "", "Properties", true));
                let mut sorted_keys: Vec<&String> = sc.properties.keys().collect();
                sorted_keys.sort();
                for key in sorted_keys {
                    if let Some(val) = sc.properties.get(key) {
                        let type_str = val.type_name();
                        let val_str = val.to_display_string();
                        // Format key from snake_case to TitleCase for display
                        let display_key = key.split('_')
                            .map(|w| {
                                let mut c = w.chars();
                                match c.next() {
                                    None => String::new(),
                                    Some(f) => f.to_uppercase().to_string() + c.as_str(),
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        props.push(make_prop(&display_key, &val_str, type_str, "Properties", false));
                    }
                }
            }
        }
        
        // TOML path for reference
        if sc.toml_path != std::path::PathBuf::new() {
            props.push(make_prop("", "", "", "File", true));
            props.push(make_prop("TOML Path", &sc.toml_path.to_string_lossy(), "string", "File", false));
        }
    }
    } // Close fallback block
    
    let model_rc = std::rc::Rc::new(slint::VecModel::from(props));
    ui.set_entity_properties(slint::ModelRc::from(model_rc));
}

/// Bridges CenterTabManager → StudioState.center_tabs when the tab manager is dirty.
/// This allows files opened via the unified explorer (OpenNode) to appear in the Slint tab bar.
fn sync_tab_manager_to_studio_state(
    mut tab_manager: Option<ResMut<super::center_tabs::CenterTabManager>>,
    mut state: Option<ResMut<StudioState>>,
) {
    let Some(ref mut mgr) = tab_manager else { return };
    if !mgr.dirty { return; }
    let Some(ref mut state) = state else { return };
    
    // Rebuild StudioState.center_tabs from CenterTabManager (skip Scene tab at index 0)
    state.center_tabs = mgr.tabs.iter().skip(1).map(|tab| {
        CenterTabData {
            entity_id: tab.entity.map(|e| e.index().index() as i32).unwrap_or(-1),
            name: tab.name.clone(),
            tab_type: tab.tab_type.type_string().to_string(),
            mode: tab.tab_type.mode_string().to_string(),
            url: tab.url.clone().unwrap_or_default(),
            dirty: tab.dirty,
            loading: tab.loading,
        }
    }).collect();
    
    // Sync active tab index (CenterTabManager is 0-indexed with Scene at 0,
    // StudioState uses 0 for Scene and 1+ for other tabs)
    state.active_center_tab = mgr.active_tab as i32;
    
    // Sync active tab's content to script_editor_content for TextEdit display.
    // This covers both "script" and "code" tab types (Soul scripts + .md/.rs/.toml etc.)
    if let Some(active_tab) = mgr.active() {
        let tab_type_str = active_tab.tab_type.type_string();
        if tab_type_str == "script" || tab_type_str == "code" {
            state.script_editor_content = active_tab.content.clone();
        }
    }
    
    // Mark tabs as dirty so sync_center_tabs_to_slint will push to Slint
    state.tabs_dirty = true;
    
    mgr.dirty = false;
}

/// Syncs center tab state (Space1 + script/web tabs) from StudioState to Slint.
/// Processes pending open/close/reorder tab requests each frame.
fn sync_center_tabs_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    mut state: Option<ResMut<StudioState>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    let Some(slint_context) = slint_context else { return };
    let Some(ref mut state) = state else { return };
    let ui = &slint_context.window;

    let scene_tab_name = space_root
        .as_ref()
        .and_then(|root| root.0.file_name().map(|name| name.to_string_lossy().to_string()))
        .unwrap_or_else(|| "Space".to_string());
    let current_scene_name: String = ui.get_scene_tab_name().into();
    if current_scene_name != scene_tab_name {
        ui.set_scene_tab_name(scene_tab_name.into());
    }
    
    // Check if tabs were updated by CenterTabManager sync
    let mut tabs_changed = state.tabs_dirty;
    if state.tabs_dirty {
        state.tabs_dirty = false;
    }
    
    // Process pending open script request
    if let Some((entity_id, name)) = state.pending_open_script.take() {
        if let Some(idx) = state.center_tabs.iter().position(|t| t.entity_id == entity_id && t.tab_type == "script") {
            state.active_center_tab = (idx as i32) + 1;
        } else {
            state.center_tabs.push(CenterTabData {
                entity_id,
                name,
                tab_type: "script".to_string(),
                mode: "code".to_string(),
                url: String::new(),
                dirty: false,
                loading: false,
            });
            state.active_center_tab = state.center_tabs.len() as i32;
        }
        tabs_changed = true;
    }
    
    // Process pending open web tab request
    if let Some(url) = state.pending_open_web.take() {
        let title = if url == "about:blank" { "New Tab".to_string() } else { url.clone() };
        state.center_tabs.push(CenterTabData {
            entity_id: -1,
            name: title,
            tab_type: "web".to_string(),
            mode: String::new(),
            url: url.clone(),
            dirty: false,
            loading: false, // loading state only meaningful when webview feature is active
        });
        state.active_center_tab = state.center_tabs.len() as i32;
        tabs_changed = true;
    }
    
    // Process pending close tab request
    if let Some(idx) = state.pending_close_tab.take() {
        let tab_idx = idx as usize;
        if tab_idx < state.center_tabs.len() {
            state.center_tabs.remove(tab_idx);
            if state.active_center_tab > state.center_tabs.len() as i32 {
                state.active_center_tab = state.center_tabs.len() as i32;
            }
            if state.active_center_tab < 0 {
                state.active_center_tab = 0;
            }
            tabs_changed = true;
        }
    }
    
    // Process pending tab reorder
    if let Some((from, to)) = state.pending_reorder.take() {
        let from_idx = from as usize;
        let to_idx = to as usize;
        let len = state.center_tabs.len();
        if from_idx < len && to_idx <= len && from_idx != to_idx {
            let tab = state.center_tabs.remove(from_idx);
            let insert_at = if to_idx > from_idx { to_idx - 1 } else { to_idx };
            let insert_at = insert_at.min(state.center_tabs.len());
            state.center_tabs.insert(insert_at, tab);
            // Update active tab to follow the moved tab if it was active
            if state.active_center_tab == (from as i32) + 1 {
                state.active_center_tab = (insert_at as i32) + 1;
            }
            tabs_changed = true;
        }
    }
    
    // Update active-tab-type property - only when changed
    let tab_type = if state.active_center_tab <= 0 {
        "scene"
    } else {
        let idx = (state.active_center_tab - 1) as usize;
        state.center_tabs.get(idx).map(|t| t.tab_type.as_str()).unwrap_or("scene")
    };
    let current_tab_type: String = ui.get_active_tab_type().into();
    if current_tab_type != tab_type {
        ui.set_active_tab_type(tab_type.into());
    }
    
    // Push tab data to Slint when changed
    if tabs_changed {
        let slint_tabs: Vec<CenterTab> = state.center_tabs.iter().map(|t| {
            CenterTab {
                entity_id: t.entity_id,
                name: t.name.as_str().into(),
                tab_type: t.tab_type.as_str().into(),
                mode: t.mode.as_str().into(),
                dirty: t.dirty,
                content: slint::SharedString::default(),
                url: t.url.as_str().into(),
                loading: t.loading,
                favicon: slint::Image::default(),
                can_go_back: false,
                can_go_forward: false,
            }
        }).collect();
        
        let model_rc = std::rc::Rc::new(slint::VecModel::from(slint_tabs));
        ui.set_center_tabs(slint::ModelRc::from(model_rc));
        ui.set_center_active_tab(state.active_center_tab);
    }

    // Sync editor content + line numbers to Slint when a script or code tab is active.
    // Triggers on tab switch (tabs_changed) OR on every keystroke (script_content_dirty).
    let script_dirty = state.script_content_dirty;
    // Copy tab_type to an owned String before the mutable borrow of state below
    let tab_type: String = tab_type.to_owned();
    if script_dirty { state.script_content_dirty = false; }
    if (tab_type == "script" || tab_type == "code") && (tabs_changed || script_dirty) {
        let language = if state.active_center_tab > 0 {
            let idx = (state.active_center_tab - 1) as usize;
            if let Some(tab) = state.center_tabs.get(idx) {
                let lower_name = tab.name.to_lowercase();
                if tab.mode == "summary" || lower_name.ends_with(".md") || lower_name.ends_with(".markdown") {
                    "Markdown".to_string()
                } else if let Some(ext) = std::path::Path::new(&tab.name).extension().and_then(|ext| ext.to_str()) {
                    language_for_ext(ext).to_string()
                } else {
                    "Rust".to_string()
                }
            } else {
                "Rust".to_string()
            }
        } else {
            "Rust".to_string()
        };

        state.script_highlight_lines = highlight_to_lines(&state.script_editor_content, &language)
            .into_iter()
            .map(|line: ComputedHighlightLine| HighlightLine {
                text: line.text.into(),
                r: line.r,
                g: line.g,
                b: line.b,
                bold: line.bold,
            })
            .collect();

        ui.set_script_editor_content(state.script_editor_content.as_str().into());
        let nums = build_line_numbers_text(&state.script_editor_content);
        ui.set_script_line_numbers(nums.into());
        let highlight_model = std::rc::Rc::new(slint::VecModel::from(state.script_highlight_lines.clone()));
        ui.set_script_highlight_lines(slint::ModelRc::from(highlight_model));
        // On tab switch (not on every keystroke): reset editor scroll to line 1
        if tabs_changed {
            ui.set_script_scroll_to_top(true);
        }
    } else {
        state.script_highlight_lines.clear();
        let highlight_model = std::rc::Rc::new(slint::VecModel::from(Vec::<HighlightLine>::new()));
        ui.set_script_highlight_lines(slint::ModelRc::from(highlight_model));
    }

    // Always sync web browser properties when a web tab is active (loading can change each frame)
    if tab_type == "web" {
        let idx = (state.active_center_tab - 1) as usize;
        if let Some(tab) = state.center_tabs.get(idx) {
            ui.set_web_url_bar(tab.url.as_str().into());
            ui.set_web_loading(tab.loading);
            ui.set_web_has_url(tab.url != "about:blank" && !tab.url.is_empty());
            ui.set_web_secure(tab.url.starts_with("https://"));
        }
    }
}

/// Maps a ClassName enum to an SVG icon filename for the explorer tree
fn class_name_to_icon_filename(class_name: &eustress_common::classes::ClassName) -> &'static str {
    use eustress_common::classes::ClassName;
    match class_name {
        ClassName::Part | ClassName::BasePart => "part",
        ClassName::Model | ClassName::PVInstance => "model",
        ClassName::Folder => "folder",
        ClassName::Humanoid => "humanoid",
        ClassName::Camera => "camera",
        ClassName::PointLight => "pointlight",
        ClassName::SpotLight => "spotlight",
        ClassName::SurfaceLight => "surfacelight",
        ClassName::DirectionalLight => "directionallight",
        ClassName::Sound => "sound",
        ClassName::ParticleEmitter => "particleemitter",
        ClassName::Beam => "beam",
        ClassName::Terrain => "terrain",
        ClassName::Sky => "sky",
        ClassName::Atmosphere => "atmosphere",
        ClassName::Star => "sun",
        ClassName::Moon => "moon",
        ClassName::Clouds => "sky",
        ClassName::SoulScript => "soulservice",
        ClassName::Decal => "decal",
        ClassName::Attachment => "attachment",
        ClassName::WeldConstraint => "weldconstraint",
        ClassName::Motor6D => "motor6d",
        ClassName::UnionOperation => "unionoperation",
        ClassName::ScreenGui => "screengui",
        ClassName::BillboardGui => "billboardgui",
        ClassName::SurfaceGui => "surfacegui",
        ClassName::TextLabel => "textlabel",
        ClassName::TextButton => "textbutton",
        ClassName::TextBox => "textbox",
        ClassName::Frame => "frame",
        ClassName::ScrollingFrame => "scrollingframe",
        ClassName::ImageLabel => "imagelabel",
        ClassName::ImageButton => "imagebutton",
        ClassName::ViewportFrame => "viewportframe",
        ClassName::Animator => "animator",
        ClassName::KeyframeSequence => "keyframesequence",
        ClassName::SpecialMesh => "specialmesh",
        // Services and environment classes
        ClassName::Lighting => "lighting",
        ClassName::Workspace => "workspace",
        ClassName::SpawnLocation | ClassName::Seat | ClassName::VehicleSeat => "spawnlocation",
        ClassName::Team => "teams",
        // Orbital / world classes — fallback to instance icon (no dedicated SVG)
        ClassName::SolarSystem | ClassName::CelestialBody | ClassName::RegionChunk => "instance",
        ClassName::ChunkedWorld => "instance",
        // Media UI — fallback
        ClassName::VideoFrame | ClassName::DocumentFrame | ClassName::WebFrame => "instance",
        _ => "instance",
    }
}

/// Build a newline-separated string of line numbers for the script editor gutter.
/// e.g. content with 3 lines → "1\n2\n3"
fn build_line_numbers_text(content: &str) -> String {
    let count = content.chars().filter(|&c| c == '\n').count() + 1;
    (1..=count).map(|n| n.to_string()).collect::<Vec<_>>().join("\n")
}

// ============================================================================
// Asset Manager — state + sync system
// ============================================================================

/// Persistent state for the Asset Manager panel
#[derive(Resource, Default)]
pub struct AssetManagerState {
    /// Which category filter is active ("All", "Meshes", "Textures", etc.)
    pub category: String,
    /// Search query typed by the user
    pub search: String,
    /// Expanded section IDs (negative IDs for section headers)
    pub expanded: std::collections::HashSet<i32>,
    /// Currently selected node ID
    pub selected: Option<i32>,
    /// Frame counter for throttling
    pub frame: u64,
    /// Whether an immediate re-sync is needed (expand/collapse/search change)
    pub dirty: bool,
    /// Hash of last pushed model to avoid flickering on redundant pushes
    pub last_hash: u64,
}

/// Classify a file extension into an asset category string
fn asset_category_for_extension(ext: &str) -> &'static str {
    match ext {
        "glb" | "gltf" | "obj" | "fbx" | "stl" => "Meshes",
        "png" | "jpg" | "jpeg" | "bmp" | "tga" | "tiff" | "webp" | "hdr" | "exr" => "Textures",
        "ogg" | "mp3" | "wav" | "flac" => "Audio",
        "mat.toml" => "Materials",
        "soul" | "rune" | "lua" | "wasm" => "Scripts",
        "ttf" | "otf" | "woff" | "woff2" => "Fonts",
        _ => "Other",
    }
}

/// Get the compound extension for category matching (e.g. "mat.toml" from "Foo.mat.toml")
fn compound_ext(path: &std::path::Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // Check for compound extensions like .mat.toml, .part.toml, .glb.toml
    if name.ends_with(".mat.toml") { return "mat.toml".to_string(); }
    if name.ends_with(".part.toml") { return "part.toml".to_string(); }
    if name.ends_with(".glb.toml") { return "glb.toml".to_string(); }
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string()
}

/// Format file size as human-readable string
fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 { return format!("{} B", bytes); }
    if bytes < 1024 * 1024 { return format!("{:.1} KB", bytes as f64 / 1024.0); }
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// Load an icon for asset type
fn asset_icon(asset_type: &str) -> slint::Image {
    let icon_name = match asset_type {
        "section" => "ui/package",
        "folder" => "ui/group",
        "Meshes" | "mesh" => "ui/package",
        "Textures" | "texture" => "ui/image",
        "Audio" | "audio" => "ui/audio",
        "Materials" | "material" => "ui/palette",
        "Scripts" | "script" => "ui/new-file",
        "Fonts" | "font" => "ui/new-file",
        _ => "ui/new-file",
    };
    let icon_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("icons")
        .join(format!("{}.svg", icon_name));
    slint::Image::load_from_path(&icon_path).unwrap_or_default()
}

/// Scan a directory recursively and collect asset files as (relative_path, full_path, size)
fn scan_asset_dir(dir: &std::path::Path, base: &std::path::Path) -> Vec<(String, std::path::PathBuf, u64)> {
    let mut results = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return results };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            // Recurse into subdirectories
            results.extend(scan_asset_dir(&path, base));
        } else {
            let size = path.metadata().map(|m| m.len()).unwrap_or(0);
            let relative = path.strip_prefix(base)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| path.file_name().unwrap_or_default().to_string_lossy().to_string());
            results.push((relative, path, size));
        }
    }
    results
}

/// Sync the Asset Manager tree to Slint.
/// Builds a hierarchy:
///   ▸ Universe Assets
///     ▸ Engine Parts (assets/parts/)
///     ▸ Custom Meshes (assets/meshes/)
///   ▸ Space Assets
///     (all non-service assets in current Space)
fn sync_asset_manager_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    mut state: ResMut<AssetManagerState>,
    space_root_res: Option<Res<crate::space::SpaceRoot>>,
) {
    state.frame += 1;
    // Throttle: sync every 60 frames or when dirty
    if !state.dirty && state.frame % 60 != 1 { return; }
    state.dirty = false;

    let Some(ctx) = slint_context.as_ref() else { return };
    let ui = &ctx.window;

    let space_root = space_root_res.map(|r| r.0.clone())
        .unwrap_or_else(crate::space::default_space_root);

    let universe_root = crate::space::universe_root_for_path(&space_root);
    let category = &state.category;
    let search = state.search.to_lowercase();

    let mut asset_nodes: Vec<(i32, String, slint::Image, String, i32, bool, bool, bool, String, String)> = Vec::new();
    let mut next_id: i32 = 1;
    let mut universe_count: i32 = 0;
    let mut space_count: i32 = 0;

    // Section IDs (negative to avoid collision with file IDs)
    let universe_section_id: i32 = -1;
    let engine_parts_id: i32 = -2;
    let custom_meshes_id: i32 = -3;
    let space_section_id: i32 = -4;

    // Check expansion state
    let universe_expanded = state.expanded.contains(&universe_section_id);
    let engine_parts_expanded = state.expanded.contains(&engine_parts_id);
    let custom_meshes_expanded = state.expanded.contains(&custom_meshes_id);
    let space_expanded = state.expanded.contains(&space_section_id);
    let selected = state.selected;

    // Helper: push a node tuple
    let mut push_node = |id: i32, name: &str, icon: slint::Image, asset_type: &str, depth: i32, expandable: bool, expanded: bool, size: &str, path: &str| {
        let is_selected = selected == Some(id);
        asset_nodes.push((id, name.to_string(), icon, asset_type.to_string(), depth, expandable, expanded, is_selected, size.to_string(), path.to_string()));
    };

    // ── Universe Assets section ─────────────────────────────────────────
    if let Some(ref uni_root) = universe_root {
        let parts_dir = uni_root.join("assets").join("parts");
        let meshes_dir = uni_root.join("assets").join("meshes");

        let parts_files = scan_asset_dir(&parts_dir, &parts_dir);
        let mesh_files = scan_asset_dir(&meshes_dir, &meshes_dir);
        let uni_total = parts_files.len() + mesh_files.len();
        universe_count = uni_total as i32;

        push_node(universe_section_id, &format!("Universe Assets ({})", uni_total),
            asset_icon("section"), "section", 0, true, universe_expanded, "", "");

        if universe_expanded {
            // Engine Parts subfolder
            let has_parts = !parts_files.is_empty();
            push_node(engine_parts_id, &format!("Engine Parts ({})", parts_files.len()),
                asset_icon("folder"), "folder", 1, has_parts, engine_parts_expanded, "", "");

            if engine_parts_expanded {
                for (rel, full_path, size) in &parts_files {
                    let ext = compound_ext(full_path);
                    let cat = asset_category_for_extension(&ext);
                    let fname = full_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    // Filter by category
                    if category != "All" && cat != category.as_str() { continue; }
                    // Filter by search
                    if !search.is_empty() && !fname.to_lowercase().contains(&search) { continue; }

                    let id = next_id; next_id += 1;
                    push_node(id, &fname, asset_icon(cat), cat, 2, false, false,
                        &format_file_size(*size), &full_path.to_string_lossy());
                }
            }

            // Custom Meshes subfolder
            let has_meshes = !mesh_files.is_empty();
            push_node(custom_meshes_id, &format!("Custom Meshes ({})", mesh_files.len()),
                asset_icon("folder"), "folder", 1, has_meshes, custom_meshes_expanded, "", "");

            if custom_meshes_expanded {
                for (rel, full_path, size) in &mesh_files {
                    let ext = compound_ext(full_path);
                    let cat = asset_category_for_extension(&ext);
                    let fname = full_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if category != "All" && cat != category.as_str() { continue; }
                    if !search.is_empty() && !fname.to_lowercase().contains(&search) { continue; }

                    let id = next_id; next_id += 1;
                    push_node(id, &fname, asset_icon(cat), cat, 2, false, false,
                        &format_file_size(*size), &full_path.to_string_lossy());
                }
            }
        }
    }

    // ── Space Assets section ────────────────────────────────────────────
    // Scan current Space for non-marker, non-service TOML and media files
    {
        let space_files = scan_asset_dir(&space_root, &space_root);
        // Filter to actual asset files (not _service.toml, _instance.toml, space.toml, etc.)
        let space_assets: Vec<_> = space_files.iter().filter(|(rel, path, _)| {
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Skip marker files and config
            if fname.starts_with('_') || fname == "space.toml" || fname == "simulation.toml"
                || fname == ".gitignore" || rel.starts_with(".eustress")
                || rel.starts_with("src") { return false; }
            let ext = compound_ext(path);
            let cat = asset_category_for_extension(&ext);
            // Category filter
            if category != "All" && cat != category.as_str() { return false; }
            // Search filter
            if !search.is_empty() && !fname.to_lowercase().contains(&search) { return false; }
            // Only show recognized asset types
            cat != "Other"
        }).collect();

        space_count = space_assets.len() as i32;
        push_node(space_section_id, &format!("Space Assets ({})", space_assets.len()),
            asset_icon("section"), "section", 0, true, space_expanded, "", "");

        if space_expanded {
            for (rel, full_path, size) in &space_assets {
                let ext = compound_ext(full_path);
                let cat = asset_category_for_extension(&ext);
                let fname = full_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                // Show relative path as name for better context
                let display_name = if rel.contains('/') {
                    rel.clone()
                } else {
                    fname
                };

                let id = next_id; next_id += 1;
                push_node(id, &display_name, asset_icon(cat), cat, 1, false, false,
                    &format_file_size(*size), &full_path.to_string_lossy());
            }
        }
    }

    // Convert to Slint model
    let slint_nodes: Vec<AssetNode> = asset_nodes.into_iter().map(|(id, name, icon, asset_type, depth, expandable, expanded, selected, size, path)| {
        AssetNode {
            id,
            name: name.into(),
            icon,
            asset_type: asset_type.into(),
            depth,
            expandable,
            expanded,
            selected,
            size: size.into(),
            path: path.into(),
        }
    }).collect();

    // Hash-based change detection: only push to Slint when the model data actually
    // changes. Re-pushing an identical model destroys and recreates all `for` loop
    // items in Slint, which resets hover state and causes visible flickering.
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for node in &slint_nodes {
        node.id.hash(&mut hasher);
        node.name.hash(&mut hasher);
        node.depth.hash(&mut hasher);
        node.expandable.hash(&mut hasher);
        node.expanded.hash(&mut hasher);
        node.selected.hash(&mut hasher);
        node.asset_type.hash(&mut hasher);
    }
    let new_hash = hasher.finish();
    
    if new_hash != state.last_hash {
        state.last_hash = new_hash;
        let model = std::rc::Rc::new(slint::VecModel::from(slint_nodes));
        ui.set_asset_nodes(slint::ModelRc::from(model));
        ui.set_universe_asset_count(universe_count);
        ui.set_space_asset_count(space_count);
    }
}

/// Load an SVG icon as a slint::Image from the assets/icons directory
fn load_class_icon(class_name: &eustress_common::classes::ClassName) -> slint::Image {
    let filename = class_name_to_icon_filename(class_name);
    let icon_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("icons")
        .join(format!("{}.svg", filename));
    slint::Image::load_from_path(&icon_path).unwrap_or_default()
}
