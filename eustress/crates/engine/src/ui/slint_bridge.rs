// ============================================================================
// Slint Bridge — Thread-Safe State Transfer Between Bevy Thread and Slint Main Thread
// ============================================================================
//
// Architecture:
//   - Bevy sync systems write to SlintBridge (Arc<Mutex<BridgeState>>)
//   - Slint main thread reads BridgeState and applies changes via ui.set_*()
//   - Dirty flags ensure only changed properties trigger Slint updates
//   - SlintActionQueue (Slint→Bevy direction) is also part of the bridge
//
// Table of Contents:
//   - BridgeState: all UI state that flows from Bevy → Slint
//   - SlintBridge: Arc<Mutex<BridgeState>> wrapper as Bevy Resource
//   - SlintActionBridge: Arc<Mutex<Vec<SlintAction>>> for Slint → Bevy actions
//   - apply_bridge_to_slint(): called on Slint main thread to apply changes
// ============================================================================

use std::sync::{Arc, Mutex};
use bevy::prelude::*;

// ============================================================================
// Bevy → Slint: UI State Snapshot
// ============================================================================

/// All UI property state that flows from Bevy ECS to Slint UI.
/// Bevy sync systems write fields here; the Slint thread reads and applies them.
/// Uses Option<T> for each field — None means "no change", Some(v) means "set to v".
#[derive(Default)]
pub struct BridgeState {
    // ---- Toolbar / Tool Selection ----
    pub current_tool: Option<String>,
    pub transform_mode: Option<String>,
    pub play_state: Option<String>,

    // ---- Performance Metrics ----
    pub current_fps: Option<f32>,
    pub current_frame_time: Option<f32>,
    pub current_entity_count: Option<i32>,

    // ---- Scene / Tab State ----
    pub scene_tab_name: Option<String>,
    pub active_tab_type: Option<String>,
    pub center_active_tab: Option<i32>,
    pub has_unsaved_changes: Option<bool>,

    // ---- Panel Visibility ----
    pub show_explorer: Option<bool>,
    pub show_properties: Option<bool>,
    pub show_output: Option<bool>,
    pub show_terrain_editor: Option<bool>,
    pub show_network_panel: Option<bool>,
    pub show_mindspace_panel: Option<bool>,
    pub show_exit_confirmation: Option<bool>,
    pub show_help_icons: Option<bool>,
    pub help_opens_in_tab: Option<bool>,

    // ---- Terrain ----
    pub has_terrain: Option<bool>,
    pub terrain_edit_mode: Option<bool>,
    pub terrain_brush: Option<String>,
    pub terrain_size: Option<String>,
    pub terrain_chunk_count: Option<String>,

    // ---- Editor Settings ----
    pub grid_visible: Option<bool>,
    pub grid_size: Option<f32>,
    pub snap_enabled: Option<bool>,
    pub snap_size: Option<f32>,
    pub dark_theme: Option<bool>,

    // ---- Selection / Properties ----
    pub selected_count: Option<i32>,
    pub selected_class: Option<String>,
    /// Icon name or path — the Slint main thread loads the actual slint::Image
    pub selected_icon_name: Option<String>,

    // ---- Account ----
    pub account_name: Option<String>,
    pub account_status: Option<String>,

    // ---- Script Editor ----
    pub script_editor_content: Option<String>,
    pub script_line_numbers: Option<String>,
    pub script_scroll_to_top: Option<bool>,

    // ---- Publish Dialog ----
    pub show_publish_dialog: Option<bool>,
    pub publish_experience_name: Option<String>,
    pub publish_description: Option<String>,
    pub publish_genre: Option<String>,
    pub publish_target: Option<String>,
    pub publish_is_public: Option<bool>,
    pub publish_open_source: Option<bool>,
    pub publish_studio_editable: Option<bool>,
    pub publish_as_new: Option<bool>,
    pub publish_is_update: Option<bool>,

    // ---- Model Data (VecModel payloads serialized as Vec) ----
    /// Explorer tree nodes — serialized as Vec of node data
    pub explorer_nodes: Option<Vec<ExplorerNodeData>>,
    /// Entity properties — serialized as Vec of property data
    pub entity_properties: Option<Vec<PropertyData>>,
    /// Output log lines
    pub output_logs: Option<Vec<OutputLogData>>,
    /// Center tabs
    pub center_tabs: Option<Vec<CenterTabData>>,
    /// Asset manager nodes
    pub asset_nodes: Option<Vec<AssetNodeData>>,
    /// Universe asset count for the asset manager panel
    pub universe_asset_count: Option<i32>,
    /// Space asset count for the asset manager panel
    pub space_asset_count: Option<i32>,
    /// Script highlight lines
    pub script_highlight_lines: Option<Vec<HighlightLineData>>,

    // ---- Workshop Panel ----
    pub workshop_state: Option<String>,
    pub workshop_product_name: Option<String>,
    pub workshop_artifact_count: Option<i32>,
    pub workshop_estimated_cost: Option<String>,
    pub workshop_api_key_valid: Option<bool>,
    pub workshop_messages: Option<Vec<WorkshopMessageData>>,
    pub workshop_steps: Option<Vec<WorkshopStepData>>,

    // ---- Viewport Bounds (read by camera controller) ----
    pub viewport_x: Option<f32>,
    pub viewport_y: Option<f32>,
    pub viewport_width: Option<f32>,
    pub viewport_height: Option<f32>,
}

impl BridgeState {
    /// Drain all pending changes, returning the snapshot and resetting to None.
    /// Called by the Slint main thread after applying changes.
    pub fn take(&mut self) -> BridgeState {
        std::mem::take(self)
    }

    /// Returns true if any field has a pending change (Some value).
    /// Used by the rendering notifier for activity-based frame pacing.
    pub fn has_changes(&self) -> bool {
        self.current_tool.is_some()
            || self.transform_mode.is_some()
            || self.play_state.is_some()
            || self.current_fps.is_some()
            || self.current_frame_time.is_some()
            || self.current_entity_count.is_some()
            || self.scene_tab_name.is_some()
            || self.explorer_nodes.is_some()
            || self.entity_properties.is_some()
            || self.output_logs.is_some()
            || self.center_tabs.is_some()
            || self.asset_nodes.is_some()
            || self.workshop_messages.is_some()
            || self.workshop_steps.is_some()
            || self.has_unsaved_changes.is_some()
            || self.selected_count.is_some()
    }
}

// ============================================================================
// Serialized Model Data Types (thread-safe, no Slint types)
// ============================================================================

/// Explorer tree node data (thread-safe mirror of Slint ExplorerNode)
#[derive(Clone, Debug)]
pub struct ExplorerNodeData {
    pub name: String,
    pub icon: String,
    pub node_type: String,
    pub depth: i32,
    pub entity_id: u64,
    pub is_expanded: bool,
    pub has_children: bool,
    pub is_selected: bool,
    pub child_count: i32,
}

/// Property panel row data (thread-safe mirror of Slint PropertyData)
#[derive(Clone, Debug)]
pub struct PropertyData {
    pub name: String,
    pub value: String,
    pub property_type: String,
    pub is_header: bool,
    pub is_readonly: bool,
    pub category: String,
    pub section_collapsed: bool,
    pub x_value: String,
    pub y_value: String,
    pub z_value: String,
    pub description: String,
    pub learn_url: String,
}

/// Output log line data
#[derive(Clone, Debug)]
pub struct OutputLogData {
    pub text: String,
    pub level: String,
    pub timestamp: String,
}

/// Center tab data (thread-safe mirror of Slint CenterTab)
#[derive(Clone, Debug)]
pub struct CenterTabData {
    pub entity_id: i32,
    pub name: String,
    pub tab_type: String,
    pub dirty: bool,
    pub content: String,
    pub url: String,
    pub loading: bool,
}

/// Asset manager node data
#[derive(Clone, Debug)]
pub struct AssetNodeData {
    pub name: String,
    pub asset_type: String,
    pub path: String,
    pub depth: i32,
    pub size: String,
}

/// Script editor highlight line data
#[derive(Clone, Debug)]
pub struct HighlightLineData {
    pub spans_json: String,
}

/// Workshop chat message data
#[derive(Clone, Debug)]
pub struct WorkshopMessageData {
    pub role: String,
    pub content: String,
    pub message_type: String,
    pub timestamp: String,
}

/// Workshop pipeline step data
#[derive(Clone, Debug)]
pub struct WorkshopStepData {
    pub name: String,
    pub status: String,
    pub description: String,
}

// ============================================================================
// Bridge Resources (Bevy side)
// ============================================================================

/// Thread-safe bridge for Bevy → Slint state transfer.
/// Inserted as a Bevy Resource. Bevy sync systems lock and write.
/// Slint main thread locks, takes snapshot, and applies to UI.
#[derive(Resource, Clone)]
pub struct SlintBridge(pub Arc<Mutex<BridgeState>>);

impl SlintBridge {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(BridgeState::default())))
    }

    /// Lock the bridge and get mutable access to the state.
    /// Bevy sync systems call this to write UI updates.
    pub fn lock(&self) -> std::sync::MutexGuard<'_, BridgeState> {
        self.0.lock().expect("SlintBridge mutex poisoned")
    }
}

// ============================================================================
// Slint → Bevy: Action Queue (callbacks from UI interactions)
// ============================================================================

/// Thread-safe queue for Slint → Bevy actions.
/// Slint callbacks push actions here; Bevy drain system reads them.
#[derive(Resource, Clone)]
pub struct SlintActionBridge(pub Arc<Mutex<Vec<super::slint_ui::SlintAction>>>);

impl SlintActionBridge {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }

    /// Push an action from the Slint main thread.
    pub fn push(&self, action: super::slint_ui::SlintAction) {
        self.0.lock().expect("SlintActionBridge mutex poisoned").push(action);
    }

    /// Drain all pending actions (called by Bevy drain system).
    pub fn drain(&self) -> Vec<super::slint_ui::SlintAction> {
        let mut queue = self.0.lock().expect("SlintActionBridge mutex poisoned");
        std::mem::take(&mut *queue)
    }
}
