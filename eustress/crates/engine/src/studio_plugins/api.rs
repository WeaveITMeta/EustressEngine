//! # Plugin API
//!
//! The API that plugins use to interact with Eustress Engine Studio.
//! This provides a safe, sandboxed interface for plugins to:
//! - Add UI elements (panels, menus, overlays)
//! - Access and modify the scene
//! - Respond to events
//! - Store persistent data
//! - Control simulation timing and speed
//! - Schedule events for discrete event simulation (DES)
//! - Query entities in bulk

#[allow(dead_code)]

use bevy::prelude::*;
use bevy_egui::egui;
use std::collections::HashMap;
use std::path::PathBuf;

// ============================================================================
// Simulation Clock
// ============================================================================

/// Simulation mode for the clock
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum SimMode {
    /// Real-time simulation (1 second = 1 second)
    #[default]
    Realtime,
    /// Fast-forward simulation (speed multiplier applies)
    Fast,
    /// Step-by-step simulation (advance manually)
    Step,
    /// Event-driven simulation (skip to next scheduled event)
    EventDriven,
}

/// Global simulation clock resource
/// 
/// Provides unified time tracking for simulations with variable speed,
/// pause control, and different simulation modes.
/// 
/// # Example
/// ```rust
/// // In a plugin's on_update:
/// fn on_update(&mut self, api: &mut PluginApi, clock: &SimClock) {
///     if clock.paused {
///         return; // Skip updates when paused
///     }
///     
///     // Use scaled delta for physics-consistent movement
///     let movement = self.velocity * clock.scaled_delta();
///     
///     // Fast-forward: speed=100.0 simulates 24h in ~14 minutes
///     if clock.speed > 1.0 {
///         self.process_batch_events(clock.scaled_delta());
///     }
/// }
/// ```
#[derive(Resource, Debug, Clone, Reflect)]
#[reflect(Resource)]
pub struct SimClock {
    /// Current simulation time in seconds (can exceed real time)
    pub current: f64,
    
    /// Raw frame delta time in seconds (unscaled, typically ~0.016 for 60fps)
    pub delta: f32,
    
    /// Simulation speed multiplier (1.0 = realtime, 100.0 = 100x speed)
    pub speed: f32,
    
    /// Whether simulation is paused
    pub paused: bool,
    
    /// Current simulation mode
    pub mode: SimMode,
    
    /// Total elapsed real time since simulation started
    pub real_elapsed: f64,
    
    /// Total elapsed simulation time (affected by speed/pause)
    pub sim_elapsed: f64,
    
    /// Frame count since simulation started
    pub frame_count: u64,
    
    /// Fixed timestep for physics (if using fixed update)
    pub fixed_timestep: f32,
    
    /// Accumulated time for fixed timestep processing
    pub accumulator: f32,
}

impl Default for SimClock {
    fn default() -> Self {
        Self {
            current: 0.0,
            delta: 1.0 / 60.0,
            speed: 1.0,
            paused: false,
            mode: SimMode::Realtime,
            real_elapsed: 0.0,
            sim_elapsed: 0.0,
            frame_count: 0,
            fixed_timestep: 1.0 / 60.0,
            accumulator: 0.0,
        }
    }
}

impl SimClock {
    /// Create a new simulation clock
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Get the scaled delta time (delta * speed, or 0 if paused)
    pub fn scaled_delta(&self) -> f32 {
        if self.paused {
            0.0
        } else {
            self.delta * self.speed
        }
    }
    
    /// Get the scaled delta as f64 for high-precision calculations
    pub fn scaled_delta_f64(&self) -> f64 {
        if self.paused {
            0.0
        } else {
            self.delta as f64 * self.speed as f64
        }
    }
    
    /// Advance the clock by one frame
    pub fn tick(&mut self, real_delta: f32) {
        self.delta = real_delta;
        self.real_elapsed += real_delta as f64;
        self.frame_count += 1;
        
        if !self.paused {
            let sim_delta = self.scaled_delta_f64();
            self.current += sim_delta;
            self.sim_elapsed += sim_delta;
            self.accumulator += self.scaled_delta();
        }
    }
    
    /// Process fixed timestep updates, returns number of steps to process
    pub fn consume_fixed_steps(&mut self) -> u32 {
        let mut steps = 0;
        while self.accumulator >= self.fixed_timestep {
            self.accumulator -= self.fixed_timestep;
            steps += 1;
        }
        steps
    }
    
    /// Set simulation speed (clamped to 0.0 - 10000.0)
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed.clamp(0.0, 10000.0);
    }
    
    /// Pause the simulation
    pub fn pause(&mut self) {
        self.paused = true;
    }
    
    /// Resume the simulation
    pub fn resume(&mut self) {
        self.paused = false;
    }
    
    /// Toggle pause state
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }
    
    /// Reset the clock to initial state
    pub fn reset(&mut self) {
        self.current = 0.0;
        self.real_elapsed = 0.0;
        self.sim_elapsed = 0.0;
        self.frame_count = 0;
        self.accumulator = 0.0;
    }
    
    /// Step forward by a specific amount (for Step mode)
    pub fn step_by(&mut self, seconds: f64) {
        if self.mode == SimMode::Step {
            self.current += seconds;
            self.sim_elapsed += seconds;
        }
    }
    
    /// Get simulation time as hours:minutes:seconds string
    pub fn time_string(&self) -> String {
        let total_secs = self.current as u64;
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }
    
    /// Get simulation time as days:hours:minutes string (for long simulations)
    pub fn time_string_long(&self) -> String {
        let total_secs = self.current as u64;
        let days = total_secs / 86400;
        let hours = (total_secs % 86400) / 3600;
        let minutes = (total_secs % 3600) / 60;
        if days > 0 {
            format!("{}d {:02}:{:02}", days, hours, minutes)
        } else {
            format!("{:02}:{:02}", hours, minutes)
        }
    }
}

// ============================================================================
// Scheduled Events
// ============================================================================

/// A scheduled event for discrete event simulation
#[derive(Debug, Clone)]
pub struct ScheduledEvent {
    /// Unique event ID
    pub id: String,
    /// Scheduled simulation time
    pub scheduled_time: f64,
    /// Event type/name
    pub event_type: String,
    /// Event payload
    pub payload: HashMap<String, PropertyValue>,
    /// Whether this event repeats
    pub repeating: bool,
    /// Repeat interval (if repeating)
    pub repeat_interval: f64,
}

impl ScheduledEvent {
    /// Create a new one-shot scheduled event
    pub fn new(id: impl Into<String>, time: f64, event_type: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            scheduled_time: time,
            event_type: event_type.into(),
            payload: HashMap::new(),
            repeating: false,
            repeat_interval: 0.0,
        }
    }
    
    /// Create a repeating scheduled event
    pub fn repeating(id: impl Into<String>, start_time: f64, interval: f64, event_type: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            scheduled_time: start_time,
            event_type: event_type.into(),
            payload: HashMap::new(),
            repeating: true,
            repeat_interval: interval,
        }
    }
    
    /// Add a payload value
    pub fn with_payload(mut self, key: impl Into<String>, value: PropertyValue) -> Self {
        self.payload.insert(key.into(), value);
        self
    }
}

// ============================================================================
// Plugin Info
// ============================================================================

/// Metadata about a plugin
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Unique identifier (e.g., "mindspace", "terrain-tools")
    pub id: String,
    /// Display name
    pub name: String,
    /// Semantic version
    pub version: String,
    /// Author name or organization
    pub author: String,
    /// Short description
    pub description: String,
    /// Optional icon path
    pub icon: Option<PathBuf>,
    /// Plugin category
    pub category: PluginCategory,
    /// Required permissions
    pub permissions: Vec<PluginPermission>,
}

impl Default for PluginInfo {
    fn default() -> Self {
        Self {
            id: "unknown".to_string(),
            name: "Unknown Plugin".to_string(),
            version: "0.0.0".to_string(),
            author: "Unknown".to_string(),
            description: "No description".to_string(),
            icon: None,
            category: PluginCategory::Utility,
            permissions: vec![],
        }
    }
}

/// Plugin categories for organization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PluginCategory {
    /// Building and modeling tools
    Building,
    /// Scripting and automation
    Scripting,
    /// Visual effects and rendering
    Graphics,
    /// Audio and sound
    Audio,
    /// Collaboration and sharing
    Collaboration,
    /// Mind mapping and organization
    MindMapping,
    /// General utilities
    #[default]
    Utility,
    /// Official Eustress plugins
    Official,
}

/// Permissions that plugins can request
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginPermission {
    /// Read scene data
    ReadScene,
    /// Modify scene data
    WriteScene,
    /// Access file system
    FileSystem,
    /// Network access
    Network,
    /// Access clipboard
    Clipboard,
    /// Run background tasks
    BackgroundTasks,
    /// Access user preferences
    UserPreferences,
}

// ============================================================================
// Plugin Trait
// ============================================================================

/// Trait that all Studio plugins must implement
pub trait StudioPlugin: Send + Sync {
    /// Get plugin metadata
    fn info(&self) -> PluginInfo;
    
    /// Called when the plugin is enabled
    fn on_enable(&mut self, api: &mut PluginApi);
    
    /// Called when the plugin is disabled
    fn on_disable(&mut self, _api: &mut PluginApi) {}
    
    /// Called every frame while enabled
    /// 
    /// # Arguments
    /// * `api` - Plugin API for interacting with the editor
    /// * `clock` - Simulation clock with timing information
    /// 
    /// Use `clock.scaled_delta()` for simulation-speed-aware updates.
    /// Use `clock.delta` for real-time updates (UI, etc.).
    fn on_update(&mut self, _api: &mut PluginApi, _clock: &SimClock) {}
    
    /// Called to render plugin UI (panels, overlays)
    fn on_ui(&mut self, _ctx: &egui::Context, _api: &mut PluginApi) {}
    
    /// Called when a menu item is clicked
    fn on_menu_action(&mut self, _action_id: &str, _api: &mut PluginApi) {}
    
    /// Called when selection changes
    fn on_selection_changed(&mut self, _selected: &[Entity], _api: &mut PluginApi) {}
    
    /// Called when a scheduled event is ready to be processed
    /// 
    /// This is the core hook for discrete event simulation (DES).
    /// Events scheduled via `api.schedule_event()` will be dispatched here
    /// when their scheduled time is reached.
    /// 
    /// # Arguments
    /// * `event` - The scheduled event with its payload
    /// * `api` - Plugin API for responding to the event
    /// * `clock` - Current simulation clock state
    fn on_scheduled_event(&mut self, _event: &ScheduledEvent, _api: &mut PluginApi, _clock: &SimClock) {}
}

// ============================================================================
// Plugin API
// ============================================================================

/// The API provided to plugins for interacting with the editor
pub struct PluginApi {
    /// Registered panels
    pub panels: Vec<PluginPanel>,
    /// Registered menu items
    pub menu_items: Vec<PluginMenuItem>,
    /// Registered toolbar items
    pub toolbar_items: Vec<PluginToolbarItem>,
    /// Plugin-specific storage
    pub storage: HashMap<String, String>,
    /// Currently selected entities
    pub selected_entities: Vec<Entity>,
    /// Pending actions to execute
    pub pending_actions: Vec<PluginAction>,
    /// Notifications to show
    pub notifications: Vec<PluginNotification>,
    /// OS information
    pub os_info: OsInfo,
    
    // === Tab Registration ===
    /// Pending tab registrations (synced to TabRegistry)
    pub pending_tabs: Vec<PendingTabRegistration>,
    /// Pending section additions
    pub pending_sections: Vec<PendingSectionRegistration>,
    /// Pending button additions
    pub pending_buttons: Vec<PendingButtonRegistration>,
    
    // === Simulation Control ===
    /// Scheduled events for discrete event simulation
    pub scheduled_events: Vec<ScheduledEvent>,
    /// Events ready to be dispatched (time has passed)
    pub ready_events: Vec<ScheduledEvent>,
    /// Cached entity query results
    pub cached_queries: HashMap<String, Vec<(Entity, HashMap<String, PropertyValue>)>>,
    /// Simulation state snapshot requests
    pub snapshot_requests: Vec<SnapshotRequest>,
}

/// Pending tab registration from a plugin
#[derive(Debug, Clone)]
pub struct PendingTabRegistration {
    pub tab_id: String,
    pub label: String,
    pub icon: Option<String>,
    pub priority: i32,
    pub owner_plugin: String,
}

/// Pending section registration
#[derive(Debug, Clone)]
pub struct PendingSectionRegistration {
    pub tab_id: String,
    pub section_id: String,
    pub label: String,
}

/// Pending button registration
#[derive(Debug, Clone)]
pub struct PendingButtonRegistration {
    pub tab_id: String,
    pub section_id: String,
    pub button_id: String,
    pub label: String,
    pub icon: Option<String>,
    pub tooltip: String,
    pub action_id: String,
    pub size: TabButtonSize,
}

/// Button size for tab buttons
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabButtonSize {
    /// Small button (icon only, inline)
    Small,
    /// Normal button (icon + label below)
    #[default]
    Normal,
    /// Large button (big icon + label)
    Large,
}

impl Default for PluginApi {
    fn default() -> Self {
        Self {
            panels: Vec::new(),
            menu_items: Vec::new(),
            toolbar_items: Vec::new(),
            storage: HashMap::new(),
            selected_entities: Vec::new(),
            pending_actions: Vec::new(),
            notifications: Vec::new(),
            pending_tabs: Vec::new(),
            pending_sections: Vec::new(),
            pending_buttons: Vec::new(),
            os_info: OsInfo::detect(),
            scheduled_events: Vec::new(),
            ready_events: Vec::new(),
            cached_queries: HashMap::new(),
            snapshot_requests: Vec::new(),
        }
    }
}

impl PluginApi {
    /// Create a new plugin API instance
    pub fn new() -> Self {
        Self::default()
    }
    
    // ========================================================================
    // UI Registration
    // ========================================================================
    
    /// Register a panel that can be shown in the UI
    pub fn register_panel(&mut self, panel: PluginPanel) {
        info!("🔌 Registered panel: {}", panel.title);
        self.panels.push(panel);
    }
    
    /// Register a menu item
    pub fn register_menu_item(&mut self, item: PluginMenuItem) {
        info!("🔌 Registered menu item: {} -> {}", item.menu_path, item.label);
        self.menu_items.push(item);
    }
    
    /// Register a toolbar item
    pub fn register_toolbar_item(&mut self, item: PluginToolbarItem) {
        info!("🔌 Registered toolbar item: {}", item.tooltip);
        self.toolbar_items.push(item);
    }
    
    // ========================================================================
    // Tab Registration (Ribbon Bar)
    // ========================================================================
    
    /// Register a custom tab in the ribbon bar
    /// 
    /// # Example
    /// ```rust
    /// api.register_tab("mindspace-tab", "MindSpace", Some("🧠"), 100, "mindspace-v2");
    /// ```
    pub fn register_tab(
        &mut self,
        tab_id: impl Into<String>,
        label: impl Into<String>,
        icon: Option<impl Into<String>>,
        priority: i32,
        owner_plugin: impl Into<String>,
    ) {
        let tab_id = tab_id.into();
        let label_str = label.into();
        info!("🔌 Registering tab: {} ({})", label_str, tab_id);
        self.pending_tabs.push(PendingTabRegistration {
            tab_id,
            label: label_str,
            icon: icon.map(|i| i.into()),
            priority,
            owner_plugin: owner_plugin.into(),
        });
    }
    
    /// Add a section to a registered tab
    /// If tab_id is empty (""), the section will be added to the default "Plugins" tab
    /// 
    /// # Example
    /// ```rust
    /// // Add to specific tab
    /// api.add_tab_section("mindspace-tab", "labels", "Labels");
    /// 
    /// // Add to default Plugins tab
    /// api.add_tab_section("", "my-section", "My Section");
    /// ```
    pub fn add_tab_section(
        &mut self,
        tab_id: impl Into<String>,
        section_id: impl Into<String>,
        label: impl Into<String>,
    ) {
        self.pending_sections.push(PendingSectionRegistration {
            tab_id: tab_id.into(),
            section_id: section_id.into(),
            label: label.into(),
        });
    }
    
    /// Add a button to a tab section
    /// If tab_id is empty (""), the button will be added to the default "Plugins" tab
    /// 
    /// # Example
    /// ```rust
    /// // Add to specific tab
    /// api.add_tab_button(
    ///     "mindspace-tab",
    ///     "labels",
    ///     "add-label",
    ///     "Add",
    ///     Some("➕"),
    ///     "Add label to selected entity",
    ///     "mindspace:add-label",
    ///     TabButtonSize::Small,
    /// );
    /// 
    /// // Add to default Plugins tab
    /// api.add_tab_button(
    ///     "",  // Empty = default Plugins tab
    ///     "my-section",
    ///     "my-button",
    ///     "Click",
    ///     Some("🔧"),
    ///     "Does something",
    ///     "my-plugin:action",
    ///     TabButtonSize::Small,
    /// );
    /// ```
    pub fn add_tab_button(
        &mut self,
        tab_id: impl Into<String>,
        section_id: impl Into<String>,
        button_id: impl Into<String>,
        label: impl Into<String>,
        icon: Option<impl Into<String>>,
        tooltip: impl Into<String>,
        action_id: impl Into<String>,
        size: TabButtonSize,
    ) {
        self.pending_buttons.push(PendingButtonRegistration {
            tab_id: tab_id.into(),
            section_id: section_id.into(),
            button_id: button_id.into(),
            label: label.into(),
            icon: icon.map(|i| i.into()),
            tooltip: tooltip.into(),
            action_id: action_id.into(),
            size,
        });
    }
    
    // ========================================================================
    // Scene Access
    // ========================================================================
    
    /// Get currently selected entities
    pub fn get_selection(&self) -> &[Entity] {
        &self.selected_entities
    }
    
    /// Request to select entities
    pub fn select_entities(&mut self, entities: Vec<Entity>) {
        self.pending_actions.push(PluginAction::Select(entities));
    }
    
    /// Request to spawn an entity
    pub fn spawn_entity(&mut self, spawn_request: SpawnRequest) {
        self.pending_actions.push(PluginAction::Spawn(spawn_request));
    }
    
    /// Request to modify an entity's property
    pub fn set_property(&mut self, entity: Entity, property: String, value: PropertyValue) {
        self.pending_actions.push(PluginAction::SetProperty { entity, property, value });
    }
    
    /// Request to delete an entity (general-purpose removal)
    pub fn delete_entity(&mut self, entity: Entity) {
        self.pending_actions.push(PluginAction::Delete(vec![entity]));
    }
    
    /// Request to delete multiple entities
    pub fn delete_entities(&mut self, entities: Vec<Entity>) {
        self.pending_actions.push(PluginAction::Delete(entities));
    }
    
    // ========================================================================
    // Storage
    // ========================================================================
    
    /// Store a value (persisted per-plugin)
    pub fn store(&mut self, key: &str, value: &str) {
        self.storage.insert(key.to_string(), value.to_string());
    }
    
    /// Retrieve a stored value
    pub fn retrieve(&self, key: &str) -> Option<&String> {
        self.storage.get(key)
    }
    
    // ========================================================================
    // Notifications
    // ========================================================================
    
    /// Show an info notification
    pub fn notify_info(&mut self, message: &str) {
        self.notifications.push(PluginNotification {
            level: NotificationLevel::Info,
            message: message.to_string(),
        });
    }
    
    /// Show a success notification
    pub fn notify_success(&mut self, message: &str) {
        self.notifications.push(PluginNotification {
            level: NotificationLevel::Success,
            message: message.to_string(),
        });
    }
    
    /// Show a warning notification
    pub fn notify_warning(&mut self, message: &str) {
        self.notifications.push(PluginNotification {
            level: NotificationLevel::Warning,
            message: message.to_string(),
        });
    }
    
    /// Show an error notification
    pub fn notify_error(&mut self, message: &str) {
        self.notifications.push(PluginNotification {
            level: NotificationLevel::Error,
            message: message.to_string(),
        });
    }
    
    // ========================================================================
    // Billboard / Label Operations (MindSpace)
    // ========================================================================
    
    /// Request spawn of BillboardGui + TextLabel hierarchy on an entity
    pub fn request_spawn_billboard_label(
        &mut self,
        parent_entity: Entity,
        text: &str,
        font_size: f32,
        color: [f32; 4],
    ) {
        self.pending_actions.push(PluginAction::SpawnBillboardLabel {
            parent_entity,
            text: text.to_string(),
            font_size,
            color,
        });
    }
    
    /// Request update of billboard text on an entity
    pub fn request_update_billboard_text(&mut self, entity: Entity, text: &str) {
        self.pending_actions.push(PluginAction::UpdateBillboardText {
            entity,
            text: text.to_string(),
        });
    }
    
    /// Request removal of billboard from an entity
    pub fn request_remove_billboard(&mut self, entity: Entity) {
        self.pending_actions.push(PluginAction::RemoveBillboard { entity });
    }
    
    // ========================================================================
    // OS Information
    // ========================================================================
    
    /// Get operating system information
    pub fn get_os(&self) -> &OsInfo {
        &self.os_info
    }
    
    /// Get the user's documents directory (with Redox support)
    pub fn get_documents_dir(&self) -> Option<PathBuf> {
        self.os_info.documents_dir()
    }
    
    /// Get the plugin data directory (with Redox support)
    pub fn get_plugin_data_dir(&self, plugin_id: &str) -> PathBuf {
        let base = self.os_info.data_dir()
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("Eustress").join("Plugins").join(plugin_id)
    }
    
    // ========================================================================
    // Simulation Control
    // ========================================================================
    
    /// Request to set simulation speed
    pub fn set_simulation_speed(&mut self, speed: f32) {
        self.pending_actions.push(PluginAction::SetSimSpeed(speed));
    }
    
    /// Request to pause simulation
    pub fn pause_simulation(&mut self) {
        self.pending_actions.push(PluginAction::PauseSim);
    }
    
    /// Request to resume simulation
    pub fn resume_simulation(&mut self) {
        self.pending_actions.push(PluginAction::ResumeSim);
    }
    
    /// Request to reset simulation clock
    pub fn reset_simulation(&mut self) {
        self.pending_actions.push(PluginAction::ResetSim);
    }
    
    /// Set simulation mode
    pub fn set_simulation_mode(&mut self, mode: SimMode) {
        self.pending_actions.push(PluginAction::SetSimMode(mode));
    }
    
    /// Step simulation forward by specified seconds (only works in Step mode)
    pub fn step_simulation(&mut self, seconds: f64) {
        self.pending_actions.push(PluginAction::StepSim(seconds));
    }
    
    // ========================================================================
    // Event Scheduling (Discrete Event Simulation)
    // ========================================================================
    
    /// Schedule an event to occur at a specific simulation time
    /// 
    /// # Arguments
    /// * `delay_sec` - Delay from current simulation time in seconds
    /// * `event_id` - Unique identifier for this event
    /// * `event_type` - Type/name of the event
    /// * `payload` - Data to pass with the event
    /// 
    /// # Example
    /// ```rust
    /// api.schedule_event(300.0, "patient_001_arrival", "patient_arrival", payload);
    /// ```
    pub fn schedule_event(
        &mut self, 
        delay_sec: f64, 
        event_id: impl Into<String>,
        event_type: impl Into<String>,
        payload: HashMap<String, PropertyValue>,
    ) {
        self.scheduled_events.push(ScheduledEvent {
            id: event_id.into(),
            scheduled_time: delay_sec, // Will be converted to absolute time by manager
            event_type: event_type.into(),
            payload,
            repeating: false,
            repeat_interval: 0.0,
        });
    }
    
    /// Schedule a repeating event
    /// 
    /// # Arguments
    /// * `start_delay` - Initial delay from current time
    /// * `interval` - Time between repetitions
    /// * `event_id` - Unique identifier
    /// * `event_type` - Type/name of the event
    pub fn schedule_repeating_event(
        &mut self,
        start_delay: f64,
        interval: f64,
        event_id: impl Into<String>,
        event_type: impl Into<String>,
    ) {
        self.scheduled_events.push(ScheduledEvent {
            id: event_id.into(),
            scheduled_time: start_delay,
            event_type: event_type.into(),
            payload: HashMap::new(),
            repeating: true,
            repeat_interval: interval,
        });
    }
    
    /// Cancel a scheduled event by ID
    pub fn cancel_event(&mut self, event_id: &str) {
        self.scheduled_events.retain(|e| e.id != event_id);
        self.pending_actions.push(PluginAction::CancelEvent(event_id.to_string()));
    }
    
    /// Get events that are ready to be processed (called by manager)
    pub fn take_ready_events(&mut self) -> Vec<ScheduledEvent> {
        std::mem::take(&mut self.ready_events)
    }
    
    // ========================================================================
    // Bulk Entity Queries
    // ========================================================================
    
    /// Request a bulk entity query by component filter
    /// 
    /// Results will be available in `cached_queries` after the next frame.
    /// 
    /// # Arguments
    /// * `query_id` - Unique ID for this query (used to retrieve results)
    /// * `component_filter` - List of component names to filter by (e.g., ["Part", "Anchored"])
    /// 
    /// # Example
    /// ```rust
    /// api.request_entity_query("beds", vec!["Bed".to_string()]);
    /// // Next frame:
    /// if let Some(beds) = api.get_query_results("beds") {
    ///     for (entity, props) in beds {
    ///         // Process each bed
    ///     }
    /// }
    /// ```
    pub fn request_entity_query(&mut self, query_id: impl Into<String>, component_filter: Vec<String>) {
        self.pending_actions.push(PluginAction::QueryEntities {
            query_id: query_id.into(),
            component_filter,
        });
    }
    
    /// Get cached query results
    pub fn get_query_results(&self, query_id: &str) -> Option<&Vec<(Entity, HashMap<String, PropertyValue>)>> {
        self.cached_queries.get(query_id)
    }
    
    /// Clear cached query results
    pub fn clear_query_cache(&mut self) {
        self.cached_queries.clear();
    }
    
    // ========================================================================
    // Timeline / Recording
    // ========================================================================
    
    /// Request a simulation state snapshot for recording/undo
    pub fn request_snapshot(&mut self, label: impl Into<String>) {
        self.snapshot_requests.push(SnapshotRequest {
            label: label.into(),
            timestamp: 0.0, // Will be filled by manager
            include_entities: None,
        });
    }
    
    /// Request a snapshot of specific entities only
    pub fn request_partial_snapshot(&mut self, label: impl Into<String>, entities: Vec<Entity>) {
        self.snapshot_requests.push(SnapshotRequest {
            label: label.into(),
            timestamp: 0.0,
            include_entities: Some(entities),
        });
    }
    
    /// Take pending snapshot requests (called by manager)
    pub fn take_snapshot_requests(&mut self) -> Vec<SnapshotRequest> {
        std::mem::take(&mut self.snapshot_requests)
    }
}

// ============================================================================
// UI Elements
// ============================================================================

/// Where a plugin panel should be located in the UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelLocation {
    /// Floating window (default)
    #[default]
    Floating,
    /// Left side panel (after Explorer/Toolbox/Assets)
    LeftSide,
    /// Right side panel (after Properties/History)
    RightSide,
    /// Bottom panel (with Output)
    Bottom,
    /// Bottom-left panel (split with left side panels)
    BottomLeft,
    /// Bottom-right panel (split with right side panels)
    BottomRight,
}

/// A panel that can be docked in the UI
#[derive(Clone)]
pub struct PluginPanel {
    /// Unique ID for this panel
    pub id: String,
    /// Display title
    pub title: String,
    /// Whether the panel is currently visible
    pub visible: bool,
    /// Minimum size [width, height]
    pub min_size: [f32; 2],
    /// Maximum size [width, height]
    pub max_size: [f32; 2],
    /// Whether to add to View menu automatically
    pub add_to_view_menu: bool,
    /// Where the panel should be located
    pub location: PanelLocation,
}

impl Default for PluginPanel {
    fn default() -> Self {
        Self {
            id: "panel".to_string(),
            title: "Panel".to_string(),
            visible: false,
            min_size: [200.0, 150.0],
            max_size: [500.0, 800.0],
            add_to_view_menu: true,
            location: PanelLocation::Floating,
        }
    }
}

/// A menu item added by a plugin
#[derive(Clone)]
pub struct PluginMenuItem {
    /// Menu path (e.g., "Plugin/MindSpace/Toggle Panel")
    pub menu_path: String,
    /// Display label
    pub label: String,
    /// Action ID (sent to on_menu_action)
    pub action_id: String,
    /// Optional keyboard shortcut
    pub shortcut: Option<String>,
    /// Whether item is enabled
    pub enabled: bool,
    /// Whether item is checkable
    pub checkable: bool,
    /// Current checked state
    pub checked: bool,
}

impl Default for PluginMenuItem {
    fn default() -> Self {
        Self {
            menu_path: "Plugin".to_string(),
            label: "Menu Item".to_string(),
            action_id: "action".to_string(),
            shortcut: None,
            enabled: true,
            checkable: false,
            checked: false,
        }
    }
}

/// A toolbar item added by a plugin
#[derive(Clone)]
pub struct PluginToolbarItem {
    /// Unique ID
    pub id: String,
    /// Icon (emoji or path)
    pub icon: String,
    /// Tooltip text
    pub tooltip: String,
    /// Action ID
    pub action_id: String,
    /// Whether item is a toggle
    pub toggle: bool,
    /// Current toggle state
    pub toggled: bool,
}

// ============================================================================
// Actions
// ============================================================================

/// Actions that plugins can request
#[derive(Debug, Clone)]
pub enum PluginAction {
    /// Select entities
    Select(Vec<Entity>),
    /// Spawn a new entity
    Spawn(SpawnRequest),
    /// Set a property on an entity
    SetProperty {
        entity: Entity,
        property: String,
        value: PropertyValue,
    },
    /// Delete entities
    Delete(Vec<Entity>),
    /// Open a file dialog
    OpenFileDialog {
        title: String,
        filters: Vec<(String, Vec<String>)>,
        callback_id: String,
    },
    /// Save a file dialog
    SaveFileDialog {
        title: String,
        default_name: String,
        filters: Vec<(String, Vec<String>)>,
        callback_id: String,
    },
    
    // === Simulation Control Actions ===
    /// Set simulation speed multiplier
    SetSimSpeed(f32),
    /// Pause simulation
    PauseSim,
    /// Resume simulation
    ResumeSim,
    /// Reset simulation clock
    ResetSim,
    /// Set simulation mode
    SetSimMode(SimMode),
    /// Step simulation forward (Step mode only)
    StepSim(f64),
    /// Cancel a scheduled event
    CancelEvent(String),
    /// Request bulk entity query
    QueryEntities {
        query_id: String,
        component_filter: Vec<String>,
    },
    /// Request state snapshot
    RequestSnapshot(SnapshotRequest),
    /// Spawn BillboardGui > TextLabel hierarchy for MindSpace labels
    SpawnBillboardLabel {
        parent_entity: Entity,
        text: String,
        font_size: f32,
        color: [f32; 4],
    },
    /// Spawn ScreenGui with UI hierarchy for on-screen plugin UI
    /// Structure: ScreenGui > Frame > [TextLabel, TextBox, TextButton...]
    SpawnScreenGui {
        /// Unique ID for this screen GUI (for later reference/removal)
        gui_id: String,
        /// Display order (higher = on top)
        display_order: i32,
        /// Child elements to spawn
        elements: Vec<ScreenGuiElement>,
    },
    /// Remove a ScreenGui by ID
    RemoveScreenGui {
        gui_id: String,
    },
    /// Update text on an existing BillboardGui's TextLabel
    UpdateBillboardText {
        entity: Entity,
        text: String,
    },
    /// Remove BillboardGui from an entity
    RemoveBillboard {
        entity: Entity,
    },
}

/// Element types for ScreenGui hierarchy
#[derive(Debug, Clone)]
pub enum ScreenGuiElement {
    /// Frame container
    Frame {
        id: String,
        position_offset: [f32; 2],
        size_offset: [f32; 2],
        background_color: [f32; 3],
        background_transparency: f32,
        border_color: [f32; 3],
        border_size: i32,
        children: Vec<ScreenGuiElement>,
    },
    /// Text label (non-interactive)
    TextLabel {
        id: String,
        text: String,
        font_size: f32,
        text_color: [f32; 3],
        position_offset: [f32; 2],
        size_offset: [f32; 2],
        background_transparency: f32,
    },
    /// Text input box
    TextBox {
        id: String,
        placeholder: String,
        font_size: f32,
        position_offset: [f32; 2],
        size_offset: [f32; 2],
        text_color: [f32; 3],
        background_color: [f32; 3],
        border_color: [f32; 3],
    },
    /// Clickable text button
    TextButton {
        id: String,
        text: String,
        font_size: f32,
        position_offset: [f32; 2],
        size_offset: [f32; 2],
        text_color: [f32; 3],
        background_color: [f32; 3],
        /// Action ID to trigger when clicked
        action_id: String,
    },
}

/// Request to spawn an entity
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub name: String,
    pub class_name: String,
    pub position: Vec3,
    pub size: Option<Vec3>,
    pub color: Option<[f32; 4]>,
    pub parent: Option<Entity>,
}

/// Property value types
#[derive(Debug, Clone)]
pub enum PropertyValue {
    String(String),
    Float(f32),
    Int(i32),
    Bool(bool),
    Vec3([f32; 3]),
    Color([f32; 4]),
    Float64(f64),
    Entity(Entity),
}

/// Request for a simulation state snapshot
#[derive(Debug, Clone)]
pub struct SnapshotRequest {
    /// Label for this snapshot (e.g., "before_surgery", "initial_state")
    pub label: String,
    /// Simulation timestamp when snapshot was requested
    pub timestamp: f64,
    /// If Some, only snapshot these entities; if None, snapshot all
    pub include_entities: Option<Vec<Entity>>,
}

/// A recorded simulation snapshot for timeline/undo
#[derive(Debug, Clone)]
pub struct SimSnapshot {
    /// Unique snapshot ID
    pub id: u64,
    /// Label for this snapshot
    pub label: String,
    /// Simulation time when snapshot was taken
    pub sim_time: f64,
    /// Real time when snapshot was taken
    pub real_time: f64,
    /// Entity states (serialized)
    pub entity_states: HashMap<Entity, HashMap<String, PropertyValue>>,
}

// ============================================================================
// Notifications
// ============================================================================

/// Notification from a plugin
#[derive(Debug, Clone)]
pub struct PluginNotification {
    pub level: NotificationLevel,
    pub message: String,
}

/// Notification severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

// ============================================================================
// OS Information
// ============================================================================

/// Operating system information
#[derive(Debug, Clone)]
pub struct OsInfo {
    /// OS name (Windows, macOS, Linux, Redox)
    pub name: String,
    /// OS family (windows, unix)
    pub family: String,
    /// Architecture (x86_64, aarch64)
    pub arch: String,
    /// Is this a 64-bit system
    pub is_64bit: bool,
}

impl OsInfo {
    /// Detect current OS
    pub fn detect() -> Self {
        Self {
            name: std::env::consts::OS.to_string(),
            family: std::env::consts::FAMILY.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            is_64bit: cfg!(target_pointer_width = "64"),
        }
    }
    
    /// Check if running on Windows
    pub fn is_windows(&self) -> bool {
        self.name == "windows"
    }
    
    /// Check if running on macOS
    pub fn is_macos(&self) -> bool {
        self.name == "macos"
    }
    
    /// Check if running on Linux
    pub fn is_linux(&self) -> bool {
        self.name == "linux"
    }
    
    /// Check if running on Redox
    pub fn is_redox(&self) -> bool {
        self.name == "redox"
    }
    
    /// Get system fonts directory
    pub fn fonts_dir(&self) -> Option<PathBuf> {
        if self.is_windows() {
            Some(PathBuf::from("C:\\Windows\\Fonts"))
        } else if self.is_macos() {
            Some(PathBuf::from("/Library/Fonts"))
        } else if self.is_linux() {
            Some(PathBuf::from("/usr/share/fonts"))
        } else if self.is_redox() {
            // Redox OS fonts directory
            Some(PathBuf::from("/ui/fonts"))
        } else {
            None
        }
    }
    
    /// Get user home directory
    pub fn home_dir(&self) -> Option<PathBuf> {
        if self.is_windows() {
            std::env::var("USERPROFILE").ok().map(PathBuf::from)
        } else if self.is_redox() {
            // Redox uses $HOME like Unix
            std::env::var("HOME").ok().map(PathBuf::from)
                .or_else(|| Some(PathBuf::from("/home/user")))
        } else {
            // Unix-like (macOS, Linux)
            std::env::var("HOME").ok().map(PathBuf::from)
        }
    }
    
    /// Get documents directory (with Redox support)
    pub fn documents_dir(&self) -> Option<PathBuf> {
        if self.is_redox() {
            // Redox doesn't have XDG, use home/Documents
            self.home_dir().map(|h| h.join("Documents"))
        } else {
            dirs::document_dir()
        }
    }
    
    /// Get data directory for applications (with Redox support)
    pub fn data_dir(&self) -> Option<PathBuf> {
        if self.is_redox() {
            // Redox: use home/.local/share like XDG
            self.home_dir().map(|h| h.join(".local").join("share"))
        } else {
            dirs::data_local_dir()
        }
    }
}

// ============================================================================
// System Fonts
// ============================================================================

/// System fonts available across platforms
pub const SYSTEM_FONTS: &[&str] = &[
    "Arial",
    "Helvetica",
    "Times New Roman",
    "Georgia",
    "Verdana",
    "Tahoma",
    "Trebuchet MS",
    "Courier New",
    "Consolas",
    "Monaco",
    "Segoe UI",
    "San Francisco",
];

/// Get a list of available system fonts
pub fn get_available_fonts() -> Vec<String> {
    // For now, return the standard cross-platform fonts
    // In the future, this could query the actual system fonts
    SYSTEM_FONTS.iter().map(|s| s.to_string()).collect()
}
