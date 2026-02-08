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
use std::sync::Arc;
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
                render_slint_to_texture,
            ).chain())
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
    
    info!("✅ Slint StudioWindow configured");
    
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
