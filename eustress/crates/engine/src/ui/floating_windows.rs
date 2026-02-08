// ============================================================================
// Floating Windows & Multi-Monitor Support
// Enables panels to be detached into separate OS windows
// ============================================================================

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::window::{Window, WindowRef};
use std::collections::HashMap;

// ============================================================================
// Resources
// ============================================================================

/// Tracks all floating panel windows
#[derive(Resource, Default)]
pub struct FloatingWindowManager {
    /// Map of panel ID to window entity
    pub windows: HashMap<String, Entity>,
    /// Map of panel ID to camera entity (targets the floating window)
    pub cameras: HashMap<String, Entity>,
    /// Map of panel ID to UI root entity (so we can despawn it)
    pub ui_roots: HashMap<String, Entity>,
    /// Pending window creation requests
    pub pending_creates: Vec<FloatingWindowRequest>,
    /// Pending window close requests
    pub pending_closes: Vec<String>,
}

/// Request to create a floating window
#[derive(Clone)]
pub struct FloatingWindowRequest {
    pub panel_id: String,
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub position: Option<(f32, f32)>,
}

/// Component marking a floating panel window
#[derive(Component)]
pub struct FloatingPanelWindow {
    pub panel_id: String,
}

/// Component marking the camera that renders into a floating panel window
#[derive(Component)]
pub struct FloatingPanelCamera {
    pub panel_id: String,
}

/// Component marking the UI root attached to a floating panel camera
#[derive(Component)]
pub struct FloatingPanelUiRoot {
    pub panel_id: String,
}

// ============================================================================
// Events
// ============================================================================

/// Event to request detaching a panel to a floating window
#[derive(Event, Message, Debug, Clone)]
pub struct DetachPanelEvent {
    pub panel_id: String,
    pub title: String,
}

/// Event to request docking a floating panel back to main window
#[derive(Event, Message, Debug, Clone)]
pub struct DockPanelEvent {
    pub panel_id: String,
    pub zone: DockZone,
}

/// Dock zones for panel placement
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockZone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

// ============================================================================
// Plugin
// ============================================================================

pub struct FloatingWindowsPlugin;

impl Plugin for FloatingWindowsPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<FloatingWindowManager>()
            .add_message::<DetachPanelEvent>()
            .add_message::<DockPanelEvent>()
            .init_resource::<DockingSuggestions>()
            .add_systems(Update, debug_detach_shortcuts)
            .add_systems(Update, handle_detach_panel_events)
            .add_systems(Update, handle_dock_panel_events)
            .add_systems(Update, process_pending_window_creates)
            .add_systems(Update, process_pending_window_closes);
    }
}

// ============================================================================
// Systems
// ============================================================================

fn debug_detach_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut detach_events: MessageWriter<DetachPanelEvent>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if !ctrl {
        return;
    }

    // Ctrl+F12 => detach Explorer
    // Ctrl+Shift+F12 => detach Properties
    if keyboard.just_pressed(KeyCode::F12) {
        let (panel_id, title) = if shift {
            ("properties".to_string(), "Properties".to_string())
        } else {
            ("explorer".to_string(), "Explorer".to_string())
        };

        detach_events.write(DetachPanelEvent { panel_id, title });
    }
}

/// Handle requests to detach panels into floating windows
fn handle_detach_panel_events(
    mut events: MessageReader<DetachPanelEvent>,
    mut manager: ResMut<FloatingWindowManager>,
) {
    for event in events.read() {
        // Don't create duplicate windows
        if manager.windows.contains_key(&event.panel_id) {
            info!("🪟 Panel {} already has a floating window", event.panel_id);
            continue;
        }
        
        info!("🪟 Queuing floating window for panel: {}", event.panel_id);
        
        manager.pending_creates.push(FloatingWindowRequest {
            panel_id: event.panel_id.clone(),
            title: event.title.clone(),
            width: 400.0,
            height: 600.0,
            position: None, // Let OS decide
        });
    }
}

/// Handle requests to dock floating panels back to main window
fn handle_dock_panel_events(
    mut events: MessageReader<DockPanelEvent>,
    mut manager: ResMut<FloatingWindowManager>,
) {
    for event in events.read() {
        if manager.windows.contains_key(&event.panel_id) {
            info!("📌 Queuing dock for panel: {} to {:?}", event.panel_id, event.zone);
            manager.pending_closes.push(event.panel_id.clone());
        }
    }
}

/// Process pending window creation requests
fn process_pending_window_creates(
    mut commands: Commands,
    mut manager: ResMut<FloatingWindowManager>,
) {
    let pending = std::mem::take(&mut manager.pending_creates);
    
    for request in pending {
        info!("🪟 Creating floating window for: {}", request.panel_id);
        
        // Spawn a new window
        let window_entity = commands.spawn((
            Window {
                title: format!("Eustress - {}", request.title),
                resolution: bevy::window::WindowResolution::new(request.width as u32, request.height as u32),
                resizable: true,
                decorations: true,
                transparent: false,
                ..default()
            },
            FloatingPanelWindow {
                panel_id: request.panel_id.clone(),
            },
        )).id();

        // Spawn a camera that targets this specific window.
        // Note: RenderTarget is a field on the Camera component, not a standalone component.
        let camera_entity = commands
            .spawn((
                Camera2d,
                Camera {
                    order: 0,
                    ..default()
                },
                RenderTarget::Window(WindowRef::Entity(window_entity)),
                FloatingPanelCamera {
                    panel_id: request.panel_id.clone(),
                },
            ))
            .id();

        // Spawn a tiny Bevy UI label in that window so it isn't blank.
        // (Actual panel content routing from Slint will come next.)
        let node = Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        };
        let ui_root = commands
            .spawn((node, UiTargetCamera(camera_entity), FloatingPanelUiRoot {
                panel_id: request.panel_id.clone(),
            }))
            .with_child((Text::new(format!("Floating Panel: {}", request.title)), TextShadow::default()))
            .id();
        
        let panel_id = request.panel_id.clone();
        manager.windows.insert(panel_id.clone(), window_entity);
        manager.cameras.insert(panel_id.clone(), camera_entity);
        manager.ui_roots.insert(panel_id, ui_root);
    }
}

/// Process pending window close requests
fn process_pending_window_closes(
    mut commands: Commands,
    mut manager: ResMut<FloatingWindowManager>,
) {
    let pending = std::mem::take(&mut manager.pending_closes);
    
    for panel_id in pending {
        if let Some(ui_root) = manager.ui_roots.remove(&panel_id) {
            commands.entity(ui_root).despawn();
        }
        if let Some(camera_entity) = manager.cameras.remove(&panel_id) {
            commands.entity(camera_entity).despawn();
        }
        if let Some(window_entity) = manager.windows.remove(&panel_id) {
            info!("📌 Closing floating window for: {}", panel_id);
            commands.entity(window_entity).despawn();
        }
    }
}

/// Sync floating window state with UI
fn sync_floating_window_state(
    manager: Res<FloatingWindowManager>,
    windows: Query<(Entity, &FloatingPanelWindow, &Window)>,
    mut removed: RemovedComponents<Window>,
) {
    // Handle windows that were closed by the user (X button)
    // This would need additional logic to sync back to Slint UI
}

// ============================================================================
// Context-Aware Docking Suggestions
// ============================================================================

/// Suggests optimal dock zones based on panel type and current layout
#[derive(Resource, Default)]
pub struct DockingSuggestions {
    /// Current suggestions based on drag state
    pub suggestions: Vec<DockSuggestion>,
    /// Whether suggestions are active (during drag)
    pub active: bool,
}

#[derive(Clone)]
pub struct DockSuggestion {
    pub zone: DockZone,
    pub score: f32, // 0.0 to 1.0, higher = better match
    pub reason: String,
}

/// Panel type classification for smart docking
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelType {
    Explorer,      // File/entity tree - prefers left
    Properties,    // Property editor - prefers right
    Output,        // Console/logs - prefers bottom
    Toolbox,       // Tools palette - prefers left
    Assets,        // Asset browser - prefers left or bottom
    ScriptEditor,  // Code editor - prefers center or right
    History,       // Undo history - prefers right
    Preview,       // 3D preview - prefers center
    Inspector,     // Debug inspector - prefers right
}

impl PanelType {
    /// Get preferred dock zones for this panel type (ordered by preference)
    pub fn preferred_zones(&self) -> Vec<(DockZone, f32)> {
        match self {
            PanelType::Explorer => vec![
                (DockZone::Left, 1.0),
                (DockZone::Right, 0.3),
            ],
            PanelType::Properties => vec![
                (DockZone::Right, 1.0),
                (DockZone::Left, 0.4),
            ],
            PanelType::Output => vec![
                (DockZone::Bottom, 1.0),
                (DockZone::Top, 0.2),
            ],
            PanelType::Toolbox => vec![
                (DockZone::Left, 0.9),
                (DockZone::Right, 0.5),
            ],
            PanelType::Assets => vec![
                (DockZone::Left, 0.7),
                (DockZone::Bottom, 0.8),
            ],
            PanelType::ScriptEditor => vec![
                (DockZone::Center, 1.0),
                (DockZone::Right, 0.6),
            ],
            PanelType::History => vec![
                (DockZone::Right, 0.8),
                (DockZone::Left, 0.4),
            ],
            PanelType::Preview => vec![
                (DockZone::Center, 1.0),
            ],
            PanelType::Inspector => vec![
                (DockZone::Right, 0.9),
                (DockZone::Bottom, 0.5),
            ],
        }
    }
    
    /// Classify a panel ID to a panel type
    pub fn from_panel_id(panel_id: &str) -> Option<Self> {
        match panel_id.to_lowercase().as_str() {
            "explorer" => Some(PanelType::Explorer),
            "properties" => Some(PanelType::Properties),
            "output" | "console" => Some(PanelType::Output),
            "toolbox" | "tools" => Some(PanelType::Toolbox),
            "assets" | "asset_manager" => Some(PanelType::Assets),
            "script_editor" | "code" => Some(PanelType::ScriptEditor),
            "history" => Some(PanelType::History),
            "preview" | "viewport" => Some(PanelType::Preview),
            "inspector" | "debug" => Some(PanelType::Inspector),
            _ => None,
        }
    }
}

/// Generate docking suggestions based on panel type and current layout
pub fn generate_docking_suggestions(
    panel_id: &str,
    current_layout: &LayoutState,
) -> Vec<DockSuggestion> {
    let mut suggestions = Vec::new();
    
    // Get panel type
    let panel_type = PanelType::from_panel_id(panel_id);
    
    if let Some(ptype) = panel_type {
        // Add type-based preferences
        for (zone, base_score) in ptype.preferred_zones() {
            let mut score = base_score;
            let mut reason = format!("{:?} panels prefer {:?}", ptype, zone);
            
            // Adjust score based on current layout
            match zone {
                DockZone::Left => {
                    if current_layout.left_panel_count >= 3 {
                        score *= 0.5; // Penalize overcrowded zones
                        reason = format!("{} (left panel crowded)", reason);
                    }
                }
                DockZone::Right => {
                    if current_layout.right_panel_count >= 3 {
                        score *= 0.5;
                        reason = format!("{} (right panel crowded)", reason);
                    }
                }
                DockZone::Bottom => {
                    if current_layout.bottom_panel_count >= 2 {
                        score *= 0.6;
                        reason = format!("{} (bottom panel crowded)", reason);
                    }
                }
                _ => {}
            }
            
            // Boost score if zone is empty
            let zone_empty = match zone {
                DockZone::Left => current_layout.left_panel_count == 0,
                DockZone::Right => current_layout.right_panel_count == 0,
                DockZone::Bottom => current_layout.bottom_panel_count == 0,
                DockZone::Top => current_layout.top_panel_count == 0,
                DockZone::Center => false,
            };
            
            if zone_empty {
                score = (score * 1.2).min(1.0);
                reason = format!("{} (zone empty)", reason);
            }
            
            suggestions.push(DockSuggestion {
                zone,
                score,
                reason,
            });
        }
    } else {
        // Unknown panel type - suggest all zones with equal weight
        for zone in [DockZone::Left, DockZone::Right, DockZone::Bottom, DockZone::Center] {
            suggestions.push(DockSuggestion {
                zone,
                score: 0.5,
                reason: "Unknown panel type".to_string(),
            });
        }
    }
    
    // Sort by score descending
    suggestions.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    
    suggestions
}

/// Current layout state for suggestion calculations
#[derive(Default)]
pub struct LayoutState {
    pub left_panel_count: usize,
    pub right_panel_count: usize,
    pub top_panel_count: usize,
    pub bottom_panel_count: usize,
}

// ============================================================================
// Integration with Slint UI
// ============================================================================

/// System to sync docking suggestions to Slint UI
pub fn update_docking_suggestions_ui(
    suggestions: Res<DockingSuggestions>,
    // Would need Slint UI state to update
) {
    if suggestions.active {
        // Update Slint UI with current suggestions
        // This would set properties on the DockZoneIndicator components
    }
}
