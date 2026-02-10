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
use parking_lot::RwLock;

// Slint software renderer imports
use slint::platform::software_renderer::PremultipliedRgbaColor;
use slint::{LogicalPosition, PhysicalSize};
use slint::platform::WindowEvent;

use crate::commands::{SelectionManager, TransformManager};
use super::file_dialogs::{SceneFile, FileEvent};
use super::spawn_events::SpawnEventsPlugin;
use super::menu_events::MenuActionEvent;
use super::world_view::WorldViewPlugin;

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
            software_renderer: Default::default(),
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
// Slint → Bevy Action Queue
// ============================================================================

/// Actions queued by Slint UI callbacks, drained by Bevy systems each frame.
/// Uses Arc<Mutex<>> because Slint callbacks capture a clone of the queue.
#[derive(Debug, Clone)]
pub enum SlintAction {
    // File operations
    NewScene,
    OpenScene,
    SaveScene,
    SaveSceneAs,
    Publish,
    
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
    
    // Explorer
    SelectEntity(i32),
    ExpandEntity(i32),
    CollapseEntity(i32),
    RenameEntity(i32, String),
    ReparentEntity(i32, i32),
    
    // Properties
    PropertyChanged(String, String),
    
    // Command bar
    ExecuteCommand(String),
    
    // Context menu
    InsertPart(String),
    ContextAction(String),
    
    // Terrain
    GenerateTerrain(String),
    ToggleTerrainEditMode,
    SetTerrainBrush(String),
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
    
    // Close
    CloseRequested,
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

/// Explorer expanded state
#[derive(Resource, Default)]
pub struct ExplorerExpanded {
    pub expanded: std::collections::HashSet<Entity>,
}

/// Explorer state
#[derive(Resource, Default)]
pub struct ExplorerState {
    pub selected: Option<Entity>,
    pub search_query: String,
    pub filter: String,
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
            .init_resource::<ExplorerExpanded>()
            .init_resource::<ExplorerState>()
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
            .add_systems(Update, handle_window_close_request)
            .add_systems(Update, handle_explorer_toggle)
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
            .init_resource::<ToolboxState>()
            .init_resource::<StudioDockState>()
            .init_resource::<ExplorerExpanded>()
            .init_resource::<ExplorerState>()
            .init_resource::<ExplorerCache>()
            .init_resource::<UIPerformance>()
            .init_resource::<SceneFile>()
            .init_resource::<crate::auth::AuthState>()
            .init_resource::<crate::soul::SoulServiceSettings>()
            .init_resource::<crate::commands::CommandHistory>()
            .init_resource::<SlintCursorState>()
            // Events
            .add_message::<FileEvent>()
            .add_message::<MenuActionEvent>()
            .add_message::<ExplorerToggleEvent>()
            .add_message::<crate::commands::UndoCommandEvent>()
            .add_message::<crate::commands::RedoCommandEvent>()
            // Plugins
            .add_plugins(SpawnEventsPlugin)
            .add_plugins(WorldViewPlugin)
            // Slint software renderer overlay systems
            .add_systems(Startup, setup_slint_overlay)
            .add_systems(Update, (
                forward_input_to_slint,
                forward_keyboard_to_slint,
                drain_slint_actions,
                sync_bevy_to_slint,
                render_slint_to_texture,
            ).chain())
            // Window resize handling
            .add_systems(Update, handle_window_resize)
            // Performance tracking
            .add_systems(Update, update_ui_performance)
            // Explorer sync (throttled internally)
            .add_systems(Update, sync_explorer_to_slint)
            // Properties sync (throttled internally)
            .add_systems(Update, sync_properties_to_slint)
            // UI systems
            .add_systems(Update, handle_window_close_request)
            .add_systems(Update, handle_explorer_toggle)
            .add_systems(Update, crate::auth::auth_poll_system)
            .add_systems(Startup, try_restore_auth_session);
    }
}

/// Initialize Slint software renderer and create overlay (exclusive startup system)
fn setup_slint_overlay(world: &mut World) {
    // Get window dimensions
    let (width, height, scale_factor) = {
        let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
        match windows.iter(world).next() {
            Some(w) => {
                let width = w.width() as u32;
                let height = w.height() as u32;
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
    ui.on_new_scene(move || q.push(SlintAction::NewScene));
    let q = queue.clone();
    ui.on_open_scene(move || q.push(SlintAction::OpenScene));
    let q = queue.clone();
    ui.on_save_scene(move || q.push(SlintAction::SaveScene));
    let q = queue.clone();
    ui.on_save_scene_as(move || q.push(SlintAction::SaveSceneAs));
    let q = queue.clone();
    ui.on_publish(move || q.push(SlintAction::Publish));
    
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
    
    // Explorer
    let q = queue.clone();
    ui.on_select_entity(move |id| q.push(SlintAction::SelectEntity(id)));
    let q = queue.clone();
    ui.on_expand_entity(move |id| q.push(SlintAction::ExpandEntity(id)));
    let q = queue.clone();
    ui.on_collapse_entity(move |id| q.push(SlintAction::CollapseEntity(id)));
    let q = queue.clone();
    ui.on_rename_entity(move |id, name| q.push(SlintAction::RenameEntity(id, name.to_string())));
    let q = queue.clone();
    ui.on_reparent_entity(move |child, parent| q.push(SlintAction::ReparentEntity(child, parent)));
    
    // Properties
    let q = queue.clone();
    ui.on_property_changed(move |key, val| q.push(SlintAction::PropertyChanged(key.to_string(), val.to_string())));
    
    // Command bar
    let q = queue.clone();
    ui.on_execute_command(move |cmd| q.push(SlintAction::ExecuteCommand(cmd.to_string())));
    
    // Toolbox part insertion
    let q = queue.clone();
    ui.on_insert_part(move |part_type| q.push(SlintAction::InsertPart(part_type.to_string())));
    
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
    let q = queue.clone();
    ui.on_import_heightmap(move || q.push(SlintAction::ImportHeightmap));
    let q = queue.clone();
    ui.on_export_heightmap(move || q.push(SlintAction::ExportHeightmap));
    
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
    
    // Store queue as Bevy resource
    world.insert_resource(queue);
    
    info!("✅ Slint StudioWindow configured with {} callbacks wired", 50);
    
    // Create Bevy texture for Slint to render into (matches official bevy-hosts-slint pattern).
    // Uses Rgba8Unorm to match Slint's PremultipliedRgbaColor output format.
    let size = Extent3d { width, height, depth_or_array_layers: 1 };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("SlintOverlay"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);
    
    let image_handle = world.resource_mut::<Assets<Image>>().add(image);
    
    // Create unlit material with alpha blending (matches official example)
    let material_handle = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
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
    world.insert_resource(SlintOverlayInitialized(true));
    
    info!("✅ Slint overlay setup complete ({}x{}, scale={})", width, height, scale_factor);
}

/// Resource holding the overlay texture handle
#[derive(Resource)]
pub struct SlintOverlayTexture(pub Handle<Image>);

/// Tracks cursor position for Slint input forwarding
#[derive(Resource, Default)]
struct SlintCursorState {
    position: Option<LogicalPosition>,
}

/// Frame counter for one-time debug logging
static RENDER_FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Renders the Slint UI to the Bevy texture each frame (from official bevy-hosts-slint)
fn render_slint_to_texture(
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    slint_scenes: Query<&SlintScene>,
    slint_context: Option<NonSend<SlintUiState>>,
    windows: Query<&Window>,
) {
    let Some(slint_context) = slint_context else { return };
    
    let frame = RENDER_FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    
    // Update Slint timers and animations every frame
    slint::platform::update_timers_and_animations();
    
    // Get scale factor from Bevy window
    let scale_factor = windows.single().map(|w| w.scale_factor()).unwrap_or(1.0);
    
    let adapter = &slint_context.adapter;
    
    let Some(scene) = slint_scenes.iter().next() else {
        if frame < 5 { warn!("render_slint_to_texture: no SlintScene entity"); }
        return;
    };
    let Some(image) = images.get_mut(&scene.image) else {
        if frame < 5 { warn!("render_slint_to_texture: image asset not found"); }
        return;
    };
    
    let requested_size = slint::PhysicalSize::new(
        image.texture_descriptor.size.width,
        image.texture_descriptor.size.height,
    );
    
    // If size or scale changed, notify Slint's layout engine
    if requested_size != adapter.size.get() || scale_factor != adapter.scale_factor.get() {
        adapter.resize(requested_size, scale_factor);
    }
    
    // Render Slint UI directly into the Bevy texture's CPU-side storage
    if let Some(data) = image.data.as_mut() {
        adapter.software_renderer.render(
            bytemuck::cast_slice_mut::<u8, PremultipliedRgbaColor>(data),
            image.texture_descriptor.size.width as usize,
        );
        
        // Debug: log at specific frames
        if frame < 3 || frame == 10 || frame == 100 {
            let non_zero_count = data.iter().filter(|&&b| b != 0).count();
            info!("🎨 render_slint frame {}: {}x{} non_zero={} has_content={}",
                frame,
                image.texture_descriptor.size.width,
                image.texture_descriptor.size.height,
                non_zero_count,
                non_zero_count > 0,
            );
        }
    } else if frame < 3 || frame == 10 || frame == 100 {
        warn!("render_slint frame {}: image.data is None!", frame);
    }
    
    // WORKAROUND: Force GPU texture re-upload by touching the material mutably.
    // See: https://github.com/bevyengine/bevy/issues/17350
    materials.get_mut(&scene.material);
}

/// Forwards Bevy mouse/keyboard input to Slint (from official bevy-hosts-slint)
fn forward_input_to_slint(
    mut mouse_button: MessageReader<MouseButtonInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cursor_state: ResMut<SlintCursorState>,
    slint_context: Option<NonSend<SlintUiState>>,
) {
    let Some(slint_context) = slint_context else { return };
    let adapter = &slint_context.adapter;
    
    let Some(window) = windows.iter().next() else { return };
    let scale_factor = adapter.scale_factor.get();
    
    // Forward cursor position to Slint (fullscreen overlay = direct mapping)
    if let Some(cursor_pos) = window.cursor_position() {
        let position = LogicalPosition::new(
            cursor_pos.x / scale_factor,
            cursor_pos.y / scale_factor,
        );
        cursor_state.position = Some(position);
        adapter.slint_window.dispatch_event(WindowEvent::PointerMoved { position });
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
    terrain_toggle: MessageWriter<'w, super::spawn_events::ToggleTerrainEditEvent>,
    terrain_brush: MessageWriter<'w, super::spawn_events::SetTerrainBrushEvent>,
}

/// Bundled mutable resources for drain_slint_actions
#[derive(bevy::ecs::system::SystemParam)]
struct DrainResources<'w> {
    state: Option<ResMut<'w, StudioState>>,
    output: Option<ResMut<'w, OutputConsole>>,
    explorer_expanded: Option<ResMut<'w, ExplorerExpanded>>,
    explorer_state: Option<ResMut<'w, ExplorerState>>,
    view_state: Option<ResMut<'w, super::ViewSelectorState>>,
    editor_settings: Option<ResMut<'w, crate::editor_settings::EditorSettings>>,
    auth_state: Option<ResMut<'w, crate::auth::AuthState>>,
}

/// Drains the SlintActionQueue each frame and dispatches to Bevy events/state.
/// This is the Slint→Bevy direction: UI button clicks become Bevy state changes and events.
fn drain_slint_actions(
    queue: Option<Res<SlintActionQueue>>,
    mut events: DrainEventWriters,
    mut res: DrainResources,
    mut instances: Query<(Entity, &mut eustress_common::classes::Instance)>,
    mut transforms: Query<&mut Transform>,
    mut base_parts: Query<&mut eustress_common::classes::BasePart>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(queue) = queue else { return };
    let actions = queue.drain();
    if actions.is_empty() { return; }
    
    for action in actions {
        match action {
            // File operations → FileEvent
            SlintAction::NewScene => { events.file_events.write(FileEvent::NewScene); }
            SlintAction::OpenScene => { events.file_events.write(FileEvent::OpenScene); }
            SlintAction::SaveScene => { events.file_events.write(FileEvent::SaveScene); }
            SlintAction::SaveSceneAs => { events.file_events.write(FileEvent::SaveSceneAs); }
            SlintAction::Publish => { events.file_events.write(FileEvent::Publish); }
            
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
                if let Some(ref mut out) = res.output {
                    out.info(format!("Opening script #{}", id));
                }
                // TODO: Open script editor for entity with this instance ID
            }
            
            // Terrain
            SlintAction::GenerateTerrain(_size) => {
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
            SlintAction::ImportHeightmap => {
                // Open file dialog for heightmap import
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Heightmap", &["png", "exr", "raw", "r16"])
                    .set_title("Import Heightmap")
                    .pick_file()
                {
                    if let Some(ref mut out) = res.output {
                        out.info(format!("Importing heightmap: {}", path.display()));
                    }
                    // TODO: Feed path into terrain system when heightmap loader is implemented
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
            
            // Viewport bounds changed — update Bevy camera/render target
            SlintAction::ViewportBoundsChanged(_x, _y, _w, _h) => {
                // Viewport bounds are read directly from Slint properties in the render system
            }
            
            // Explorer actions — map instance ID (i32) back to Bevy Entity
            SlintAction::SelectEntity(id) => {
                if let Some(ref mut es) = res.explorer_state {
                    // Find the Entity with this instance ID
                    let found = instances.iter()
                        .find(|(_, inst)| inst.id as i32 == id)
                        .map(|(e, _)| e);
                    es.selected = found;
                }
            }
            SlintAction::ExpandEntity(id) => {
                if let Some(ref mut ee) = res.explorer_expanded {
                    if let Some((entity, _)) = instances.iter().find(|(_, inst)| inst.id as i32 == id) {
                        ee.expanded.insert(entity);
                    }
                }
            }
            SlintAction::CollapseEntity(id) => {
                if let Some(ref mut ee) = res.explorer_expanded {
                    if let Some((entity, _)) = instances.iter().find(|(_, inst)| inst.id as i32 == id) {
                        ee.expanded.remove(&entity);
                    }
                }
            }
            SlintAction::RenameEntity(id, name) => {
                if let Some((_, mut inst)) = instances.iter_mut().find(|(_, inst)| inst.id as i32 == id) {
                    inst.name = name;
                }
            }
            SlintAction::ReparentEntity(child_id, parent_id) => {
                // Find child and parent entities by instance ID
                let child_entity = instances.iter()
                    .find(|(_, inst)| inst.id as i32 == child_id)
                    .map(|(e, _)| e);
                let parent_entity = instances.iter()
                    .find(|(_, inst)| inst.id as i32 == parent_id)
                    .map(|(e, _)| e);
                if let (Some(child), Some(parent)) = (child_entity, parent_entity) {
                    commands.entity(child).insert(ChildOf(parent));
                    if let Some(ref mut out) = res.output {
                        out.info(format!("Reparented entity {} under {}", child_id, parent_id));
                    }
                }
            }
            
            // Properties write-back — apply edits from Slint properties panel to ECS
            SlintAction::PropertyChanged(key, val) => {
                let selected = res.explorer_state.as_ref().and_then(|es| es.selected);
                if let Some(entity) = selected {
                    match key.as_str() {
                        // Instance fields
                        "Name" => {
                            if let Ok((_, mut inst)) = instances.get_mut(entity) {
                                inst.name = val.clone();
                            }
                        }
                        "Archivable" => {
                            if let Ok((_, mut inst)) = instances.get_mut(entity) {
                                inst.archivable = val == "true";
                            }
                        }
                        // Transform fields
                        "Position.X" | "Position.Y" | "Position.Z" => {
                            if let Ok(mut t) = transforms.get_mut(entity) {
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
                            if let Ok(mut t) = transforms.get_mut(entity) {
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
                            if let Ok(mut bp) = base_parts.get_mut(entity) {
                                if let Ok(v) = val.parse::<f32>() {
                                    bp.transparency = v;
                                }
                            }
                        }
                        "Anchored" => {
                            if let Ok(mut bp) = base_parts.get_mut(entity) {
                                bp.anchored = val == "true";
                            }
                        }
                        "CanCollide" => {
                            if let Ok(mut bp) = base_parts.get_mut(entity) {
                                bp.can_collide = val == "true";
                            }
                        }
                        _ => {
                            // Unhandled property — log for debugging
                            if let Some(ref mut out) = res.output {
                                out.info(format!("Property '{}' = '{}' (unhandled)", key, val));
                            }
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
            
            // Toolbox insertion — parts via SpawnPartEvent, others via direct spawn
            SlintAction::InsertPart(part_type_str) => {
                use eustress_common::classes::PartType;
                use crate::classes::*;
                
                // Generate unique instance ID
                let uid = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() % u32::MAX as u128) as u32;
                
                match part_type_str.as_str() {
                    // Primitive parts → SpawnPartEvent (handled by existing system)
                    "Part" | "Block" => { events.spawn_events.write(super::SpawnPartEvent { part_type: PartType::Block, position: Vec3::new(0.0, 5.0, 0.0) }); }
                    "SpherePart" | "Ball" => { events.spawn_events.write(super::SpawnPartEvent { part_type: PartType::Ball, position: Vec3::new(0.0, 5.0, 0.0) }); }
                    "CylinderPart" | "Cylinder" => { events.spawn_events.write(super::SpawnPartEvent { part_type: PartType::Cylinder, position: Vec3::new(0.0, 5.0, 0.0) }); }
                    "WedgePart" | "Wedge" => { events.spawn_events.write(super::SpawnPartEvent { part_type: PartType::Wedge, position: Vec3::new(0.0, 5.0, 0.0) }); }
                    "CornerWedgePart" | "CornerWedge" => { events.spawn_events.write(super::SpawnPartEvent { part_type: PartType::CornerWedge, position: Vec3::new(0.0, 5.0, 0.0) }); }
                    "Cone" => { events.spawn_events.write(super::SpawnPartEvent { part_type: PartType::Cone, position: Vec3::new(0.0, 5.0, 0.0) }); }
                    
                    // Model — empty container
                    "Model" => {
                        let inst = Instance { name: "Model".into(), class_name: ClassName::Model, archivable: true, id: uid, ..Default::default() };
                        crate::spawn::spawn_model(&mut commands, inst, Model::default());
                        if let Some(ref mut out) = res.output { out.info("Inserted Model".to_string()); }
                    }
                    
                    // Folder — organizational container
                    "Folder" => {
                        let inst = Instance { name: "Folder".into(), class_name: ClassName::Folder, archivable: true, id: uid, ..Default::default() };
                        crate::spawn::spawn_folder(&mut commands, inst);
                        if let Some(ref mut out) = res.output { out.info("Inserted Folder".to_string()); }
                    }
                    
                    // PointLight
                    "PointLight" => {
                        let inst = Instance { name: "PointLight".into(), class_name: ClassName::PointLight, archivable: true, id: uid, ..Default::default() };
                        let light = EustressPointLight::default();
                        crate::spawn::spawn_point_light(&mut commands, inst, light, Transform::from_xyz(0.0, 8.0, 0.0));
                        if let Some(ref mut out) = res.output { out.info("Inserted PointLight".to_string()); }
                    }
                    
                    // SpotLight
                    "SpotLight" => {
                        let inst = Instance { name: "SpotLight".into(), class_name: ClassName::SpotLight, archivable: true, id: uid, ..Default::default() };
                        let light = EustressSpotLight::default();
                        crate::spawn::spawn_spot_light(&mut commands, inst, light, Transform::from_xyz(0.0, 8.0, 0.0).looking_at(Vec3::ZERO, Vec3::Y));
                        if let Some(ref mut out) = res.output { out.info("Inserted SpotLight".to_string()); }
                    }
                    
                    // DirectionalLight
                    "DirectionalLight" => {
                        let inst = Instance { name: "DirectionalLight".into(), class_name: ClassName::DirectionalLight, archivable: true, id: uid, ..Default::default() };
                        let light = EustressDirectionalLight::default();
                        crate::spawn::spawn_directional_light(&mut commands, inst, light, Transform::from_xyz(0.0, 20.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y));
                        if let Some(ref mut out) = res.output { out.info("Inserted DirectionalLight".to_string()); }
                    }
                    
                    _ => {
                        if let Some(ref mut out) = res.output {
                            out.info(format!("Insert not yet supported: {}", part_type_str));
                        }
                    }
                }
            }
            
            // Context menu
            SlintAction::ContextAction(action) => {
                match action.as_str() {
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
            
            // Close
            SlintAction::CloseRequested => {
                if let Some(ref s) = res.state {
                    if s.has_unsaved_changes {
                        // Show exit confirmation (handled by Slint dialog)
                    } else {
                        events.exit_events.write(bevy::app::AppExit::Success);
                    }
                } else {
                    events.exit_events.write(bevy::app::AppExit::Success);
                }
            }
        }
    }
}

/// Pushes Bevy state to Slint properties each frame (Bevy→Slint direction).
/// Updates tool selection, play state, FPS, panel visibility, output logs, etc.
fn sync_bevy_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    state: Option<Res<StudioState>>,
    perf: Option<Res<UIPerformance>>,
    output: Option<Res<OutputConsole>>,
    time: Res<Time>,
) {
    let Some(slint_context) = slint_context else { return };
    let ui = &slint_context.window;
    
    // Sync StudioState → Slint properties
    if let Some(ref state) = state {
        // Tool
        let tool_str = match state.current_tool {
            Tool::Select => "select",
            Tool::Move => "move",
            Tool::Rotate => "rotate",
            Tool::Scale => "scale",
            Tool::Terrain => "terrain",
        };
        ui.set_current_tool(tool_str.into());
        
        // Transform mode
        let mode_str = match state.transform_mode {
            TransformMode::World => "world",
            TransformMode::Local => "local",
        };
        ui.set_transform_mode(mode_str.into());
        
        // Panel visibility
        ui.set_show_explorer(state.show_explorer);
        ui.set_show_properties(state.show_properties);
        ui.set_show_output(state.show_output);
        
        // Dialogs
        ui.set_show_exit_confirmation(state.show_exit_confirmation);
        ui.set_has_unsaved_changes(state.has_unsaved_changes);
        
        // Network
        ui.set_show_network_panel(state.show_network_panel);
        ui.set_show_terrain_editor(state.show_terrain_editor);
        ui.set_show_mindspace_panel(state.mindspace_panel_visible);
    }
    
    // Sync performance metrics → Slint
    if let Some(ref perf) = perf {
        ui.set_current_fps(perf.fps);
        ui.set_current_frame_time(perf.avg_frame_time_ms);
    }
    
    // Sync output console logs → Slint (throttled: only last 200 entries, every 10 frames)
    if let Some(ref perf) = perf {
        if perf.should_throttle(10) { return; }
    }
    if let Some(ref output) = output {
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

/// Handles window resize: updates Slint texture, overlay quad, and overlay camera
fn handle_window_resize(
    windows: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    slint_context: Option<NonSend<SlintUiState>>,
    slint_scenes: Query<&SlintScene>,
    mut overlay_quads: Query<&mut Mesh3d, With<SlintOverlaySprite>>,
    mut overlay_cameras: Query<(&mut Camera, &mut Projection), With<SlintOverlayCamera>>,
) {
    let Some(window) = windows.iter().next() else { return };
    let Some(slint_context) = slint_context else { return };
    
    let new_width = window.width() as u32;
    let new_height = window.height() as u32;
    if new_width == 0 || new_height == 0 { return; }
    
    let scale_factor = window.scale_factor();
    
    // Resize Slint adapter
    slint_context.adapter.resize(
        slint::PhysicalSize::new(new_width, new_height),
        scale_factor,
    );
    
    // Resize the Slint texture
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
    
    // Resize the overlay quad mesh
    for mut mesh3d in overlay_quads.iter_mut() {
        let new_quad = Rectangle::new(new_width as f32, new_height as f32);
        mesh3d.0 = meshes.add(new_quad);
    }
    
    // Update overlay camera projection
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
    
    info!("🔄 Window resized to {}x{} (scale={})", new_width, new_height, scale_factor);
}

/// Forwards Bevy keyboard events to Slint for text input and key handling
fn forward_keyboard_to_slint(
    mut key_events: MessageReader<bevy::input::keyboard::KeyboardInput>,
    slint_context: Option<NonSend<SlintUiState>>,
) {
    let Some(slint_context) = slint_context else { return };
    let adapter = &slint_context.adapter;
    
    for event in key_events.read() {
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

/// Syncs the ECS Instance hierarchy to the Slint explorer panel.
/// Builds a flat list of EntityNode structs with depth info for tree rendering.
/// Throttled to run every 30 frames to avoid per-frame overhead.
fn sync_explorer_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    perf: Option<Res<UIPerformance>>,
    explorer_expanded: Res<ExplorerExpanded>,
    explorer_state: Res<ExplorerState>,
    instances: Query<(Entity, &eustress_common::classes::Instance)>,
    children_query: Query<&Children>,
    child_of_query: Query<&ChildOf>,
) {
    // Throttle: only update every 30 frames
    if let Some(ref perf) = perf {
        if perf.should_throttle(30) { return; }
    }
    let Some(slint_context) = slint_context else { return };
    let ui = &slint_context.window;
    
    // Build set of all entities that have Instance components
    let instance_entities: std::collections::HashSet<Entity> = 
        instances.iter().map(|(e, _)| e).collect();
    
    // Find root entities (no ChildOf, or ChildOf points to non-Instance entity)
    let mut roots: Vec<Entity> = Vec::new();
    for (entity, _instance) in instances.iter() {
        match child_of_query.get(entity) {
            Ok(child_of) => {
                // If parent is not an Instance entity, treat as root
                if !instance_entities.contains(&child_of.0) {
                    roots.push(entity);
                }
            }
            Err(_) => {
                // No parent → root entity
                roots.push(entity);
            }
        }
    }
    
    // Sort roots by name for stable ordering
    roots.sort_by(|a, b| {
        let a_name = instances.get(*a).map(|(_, i)| i.name.as_str()).unwrap_or("");
        let b_name = instances.get(*b).map(|(_, i)| i.name.as_str()).unwrap_or("");
        a_name.cmp(b_name)
    });
    
    // Build flat tree via DFS
    let mut flat_nodes: Vec<EntityNode> = Vec::new();
    let mut stack: Vec<(Entity, i32)> = roots.into_iter().rev().map(|e| (e, 0)).collect();
    
    while let Some((entity, depth)) = stack.pop() {
        let Ok((_, instance)) = instances.get(entity) else { continue };
        
        // Check if this entity has Instance children
        let has_children = children_query.get(entity)
            .map(|children| children.iter().any(|c| instance_entities.contains(&c)))
            .unwrap_or(false);
        
        let entity_id = instance.id as i32;
        let is_expanded = explorer_expanded.expanded.contains(&entity);
        let is_selected = explorer_state.selected == Some(entity);
        
        // Map class name to icon character
        let icon = class_name_to_icon(&instance.class_name);
        
        flat_nodes.push(EntityNode {
            id: entity_id,
            name: instance.name.clone().into(),
            class_name: format!("{:?}", instance.class_name).into(),
            icon: icon.into(),
            depth,
            expandable: has_children,
            expanded: is_expanded,
            selected: is_selected,
            visible: true,
        });
        
        // If expanded, push children onto stack (reversed for correct order)
        if is_expanded && has_children {
            if let Ok(children) = children_query.get(entity) {
                let mut child_instances: Vec<Entity> = children.iter()
                    .filter(|c| instance_entities.contains(c))
                    .collect();
                // Sort children by name
                child_instances.sort_by(|a, b| {
                    let a_name = instances.get(*a).map(|(_, i)| i.name.as_str()).unwrap_or("");
                    let b_name = instances.get(*b).map(|(_, i)| i.name.as_str()).unwrap_or("");
                    a_name.cmp(b_name)
                });
                // Push in reverse so first child is processed first
                for child in child_instances.into_iter().rev() {
                    stack.push((child, depth + 1));
                }
            }
        }
    }
    
    // Push to Slint
    let model_rc = std::rc::Rc::new(slint::VecModel::from(flat_nodes));
    ui.set_explorer_entities(slint::ModelRc::from(model_rc));
}

/// Syncs the selected entity's properties to the Slint properties panel.
/// Reads Instance, BasePart, Transform, and other component properties via PropertyAccess.
/// Throttled to run every 15 frames.
fn sync_properties_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    perf: Option<Res<UIPerformance>>,
    explorer_state: Res<ExplorerState>,
    instances: Query<(Entity, &eustress_common::classes::Instance)>,
    transforms: Query<&Transform>,
    base_parts: Query<&eustress_common::classes::BasePart>,
) {
    // Throttle: only update every 15 frames
    if let Some(ref perf) = perf {
        if perf.should_throttle(15) { return; }
    }
    let Some(slint_context) = slint_context else { return };
    let ui = &slint_context.window;
    
    let Some(selected_entity) = explorer_state.selected else {
        // No selection — clear properties and update count
        ui.set_selected_count(0);
        ui.set_selected_class(slint::SharedString::default());
        let empty: Vec<PropertyData> = Vec::new();
        let model_rc = std::rc::Rc::new(slint::VecModel::from(empty));
        ui.set_entity_properties(slint::ModelRc::from(model_rc));
        return;
    };
    
    let Ok((_, instance)) = instances.get(selected_entity) else { return };
    
    ui.set_selected_count(1);
    ui.set_selected_class(format!("{:?}", instance.class_name).into());
    
    let mut props: Vec<PropertyData> = Vec::new();
    
    // Data properties from Instance
    use eustress_common::classes::PropertyAccess;
    
    props.push(PropertyData {
        name: "Name".into(),
        value: instance.name.clone().into(),
        property_type: "string".into(),
        category: "Data".into(),
        editable: true,
        options: slint::ModelRc::default(),
    });
    props.push(PropertyData {
        name: "ClassName".into(),
        value: format!("{:?}", instance.class_name).into(),
        property_type: "string".into(),
        category: "Data".into(),
        editable: false,
        options: slint::ModelRc::default(),
    });
    props.push(PropertyData {
        name: "Archivable".into(),
        value: instance.archivable.to_string().into(),
        property_type: "bool".into(),
        category: "Data".into(),
        editable: true,
        options: slint::ModelRc::default(),
    });
    
    // Transform properties
    if let Ok(transform) = transforms.get(selected_entity) {
        let (rx, ry, rz) = transform.rotation.to_euler(bevy::math::EulerRot::XYZ);
        props.push(PropertyData {
            name: "Position".into(),
            value: format!("{:.2}, {:.2}, {:.2}", transform.translation.x, transform.translation.y, transform.translation.z).into(),
            property_type: "vec3".into(),
            category: "Transform".into(),
            editable: true,
            options: slint::ModelRc::default(),
        });
        props.push(PropertyData {
            name: "Rotation".into(),
            value: format!("{:.1}, {:.1}, {:.1}", rx.to_degrees(), ry.to_degrees(), rz.to_degrees()).into(),
            property_type: "vec3".into(),
            category: "Transform".into(),
            editable: true,
            options: slint::ModelRc::default(),
        });
        props.push(PropertyData {
            name: "Scale".into(),
            value: format!("{:.2}, {:.2}, {:.2}", transform.scale.x, transform.scale.y, transform.scale.z).into(),
            property_type: "vec3".into(),
            category: "Transform".into(),
            editable: true,
            options: slint::ModelRc::default(),
        });
    }
    
    // BasePart properties (Size, Anchored, CanCollide, Transparency, etc.)
    if let Ok(base_part) = base_parts.get(selected_entity) {
        for prop_desc in base_part.list_properties() {
            if let Some(value) = base_part.get_property(&prop_desc.name) {
                let (val_str, prop_type) = property_value_to_display(&value);
                props.push(PropertyData {
                    name: prop_desc.name.clone().into(),
                    value: val_str.into(),
                    property_type: prop_type.into(),
                    category: "Appearance".into(),
                    editable: true,
                    options: slint::ModelRc::default(),
                });
            }
        }
    }
    
    // Push to Slint
    let model_rc = std::rc::Rc::new(slint::VecModel::from(props));
    ui.set_entity_properties(slint::ModelRc::from(model_rc));
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

/// Maps a ClassName enum to a single-character icon for the explorer tree
fn class_name_to_icon(class_name: &eustress_common::classes::ClassName) -> &'static str {
    use eustress_common::classes::ClassName;
    match class_name {
        ClassName::Part | ClassName::BasePart | ClassName::MeshPart => "■",
        ClassName::Model | ClassName::PVInstance => "▣",
        ClassName::Folder => "📁",
        ClassName::Humanoid => "🧑",
        ClassName::Camera => "📷",
        ClassName::PointLight | ClassName::SpotLight | ClassName::SurfaceLight | ClassName::DirectionalLight => "💡",
        ClassName::Sound => "🔊",
        ClassName::ParticleEmitter => "✨",
        ClassName::Beam => "⚡",
        ClassName::Terrain => "🏔",
        ClassName::Sky => "☁",
        ClassName::SoulScript => "📜",
        ClassName::Decal => "🖼",
        ClassName::Attachment => "📌",
        ClassName::WeldConstraint | ClassName::Motor6D => "🔗",
        ClassName::UnionOperation => "⊕",
        ClassName::BillboardGui | ClassName::SurfaceGui | ClassName::ScreenGui => "🖥",
        ClassName::TextLabel | ClassName::TextButton => "T",
        ClassName::Frame | ClassName::ScrollingFrame => "□",
        ClassName::ImageLabel | ClassName::ImageButton => "🖼",
        ClassName::Animator | ClassName::KeyframeSequence => "🎬",
        ClassName::SpecialMesh => "△",
        _ => "●",
    }
}
