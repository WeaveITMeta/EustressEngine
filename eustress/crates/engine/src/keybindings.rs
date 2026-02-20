use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Actions that can be bound to keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    // Tools
    SelectTool,
    MoveTool,
    RotateTool,
    ScaleTool,
    
    // Edit
    Undo,
    Redo,
    Copy,
    Paste,
    Duplicate,
    Delete,
    SelectAll,
    Group,
    Ungroup,
    LockSelection,
    UnlockSelection,
    ToggleAnchor,
    
    // View Panels
    ToggleExplorer,
    ToggleProperties,
    ToggleOutput,
    
    // Windows
    ToggleCommandBar,
    ToggleAssets,
    ToggleCollaboration,
    
    // Transform
    ToggleTransformSpace, // Toggle World/Local space
    
    // Camera
    FocusSelection, // Focus camera on selected part (F key)
    
    // Camera View Modes (Blender-style numpad)
    ViewPerspectiveToggle, // Toggle Perspective/Orthographic (Numpad 5)
    ViewTop,               // Top view (Numpad 8)
    ViewFront,             // Front view (Numpad 2)
    ViewSideLeft,          // Left side view (Numpad 4)
    ViewSideRight,         // Right side view (Numpad 6)
    
    // Snapping
    SnapMode1,      // 1 unit snapping (1 key)
    SnapMode2,      // 0.2 unit snapping (2 key)
    SnapModeOff,    // No snapping (3 key)
    
    // Quick Rotation
    RotateY90,      // Rotate 90° on Y axis (Ctrl+R)
    TiltZ90,        // Tilt 90° on Z axis (Ctrl+T)
    
    // Network
    StartServer,    // Start local server (F9)
    StopServer,     // Stop server
    ToggleNetworkPanel, // Toggle network panel (Ctrl+Shift+N)
    
    // CSG Operations
    CSGNegate,      // Negate selected part (CSG subtract)
    CSGUnion,       // Union selected parts
    CSGIntersect,   // Intersect selected parts
    CSGSeparate,    // Separate union into parts
}

impl Action {
    pub fn name(&self) -> &'static str {
        match self {
            Action::SelectTool => "Select Tool",
            Action::MoveTool => "Move Tool",
            Action::RotateTool => "Rotate Tool",
            Action::ScaleTool => "Scale Tool",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Copy => "Copy",
            Action::Paste => "Paste",
            Action::Duplicate => "Duplicate",
            Action::Delete => "Delete",
            Action::SelectAll => "Select All",
            Action::Group => "Group",
            Action::Ungroup => "Ungroup",
            Action::LockSelection => "Lock Selection",
            Action::UnlockSelection => "Unlock Selection",
            Action::ToggleAnchor => "Toggle Anchor",
            Action::ToggleExplorer => "Toggle Explorer",
            Action::ToggleProperties => "Toggle Properties",
            Action::ToggleOutput => "Toggle Output",
            Action::ToggleCommandBar => "Toggle Command Bar",
            Action::ToggleAssets => "Toggle Assets",
            Action::ToggleCollaboration => "Toggle Collaboration",
            Action::ToggleTransformSpace => "Toggle Transform Space",
            Action::FocusSelection => "Focus Selection",
            Action::ViewPerspectiveToggle => "Toggle Perspective/Ortho",
            Action::ViewTop => "Top View",
            Action::ViewFront => "Front View",
            Action::ViewSideLeft => "Left Side View",
            Action::ViewSideRight => "Right Side View",
            Action::SnapMode1 => "Snap Mode: 1.96133m (Space Grade)",
            Action::SnapMode2 => "Snap Mode: 0.392266m (Fine)",
            Action::SnapModeOff => "Snap Mode: Off",
            Action::RotateY90 => "Rotate 90° (Y Axis)",
            Action::TiltZ90 => "Tilt 90° (Z Axis)",
            Action::StartServer => "Start Server",
            Action::StopServer => "Stop Server",
            Action::ToggleNetworkPanel => "Toggle Network Panel",
            Action::CSGNegate => "CSG Negate",
            Action::CSGUnion => "CSG Union",
            Action::CSGIntersect => "CSG Intersect",
            Action::CSGSeparate => "CSG Separate",
        }
    }
}

/// Key combination with modifiers
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyBinding {
    pub key: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyBinding {
    pub fn new(key: KeyCode) -> Self {
        Self {
            key,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }
    
    pub fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }
    
    pub fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }
    
    pub fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }
    
    pub fn matches(&self, keys: &ButtonInput<KeyCode>) -> bool {
        // Check modifiers
        let ctrl_pressed = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let alt_pressed = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
        let shift_pressed = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        
        if self.ctrl != ctrl_pressed || self.alt != alt_pressed || self.shift != shift_pressed {
            return false;
        }
        
        // Check key
        keys.just_pressed(self.key)
    }
    
    pub fn to_string_rep(&self) -> String {
        let mut parts = Vec::new();
        
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        
        parts.push(format!("{:?}", self.key));
        
        parts.join("+")
    }
}

/// Resource for managing keybindings
#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct KeyBindings {
    bindings: HashMap<Action, KeyBinding>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        
        // Tool shortcuts (Alt-based to avoid text input conflicts)
        bindings.insert(Action::SelectTool, KeyBinding::new(KeyCode::KeyZ).with_alt());
        bindings.insert(Action::MoveTool, KeyBinding::new(KeyCode::KeyX).with_alt());
        bindings.insert(Action::ScaleTool, KeyBinding::new(KeyCode::KeyC).with_alt());
        bindings.insert(Action::RotateTool, KeyBinding::new(KeyCode::KeyV).with_alt());
        
        // Edit shortcuts
        bindings.insert(Action::Undo, KeyBinding::new(KeyCode::KeyZ).with_ctrl());
        bindings.insert(Action::Redo, KeyBinding::new(KeyCode::KeyY).with_ctrl());
        bindings.insert(Action::Copy, KeyBinding::new(KeyCode::KeyC).with_ctrl());
        bindings.insert(Action::Paste, KeyBinding::new(KeyCode::KeyV).with_ctrl());
        bindings.insert(Action::Duplicate, KeyBinding::new(KeyCode::KeyD).with_ctrl());
        bindings.insert(Action::Delete, KeyBinding::new(KeyCode::Delete));
        bindings.insert(Action::SelectAll, KeyBinding::new(KeyCode::KeyA).with_ctrl());
        bindings.insert(Action::Group, KeyBinding::new(KeyCode::KeyG).with_ctrl());
        bindings.insert(Action::Ungroup, KeyBinding::new(KeyCode::KeyU).with_ctrl());
        
        // View shortcuts
        bindings.insert(Action::ToggleExplorer, KeyBinding::new(KeyCode::Digit1).with_ctrl());
        bindings.insert(Action::ToggleProperties, KeyBinding::new(KeyCode::Digit2).with_ctrl());
        bindings.insert(Action::ToggleOutput, KeyBinding::new(KeyCode::Digit3).with_ctrl());
        
        // Window shortcuts
        bindings.insert(Action::ToggleCommandBar, KeyBinding::new(KeyCode::KeyK).with_ctrl());
        bindings.insert(Action::ToggleAssets, KeyBinding::new(KeyCode::KeyF).with_ctrl().with_shift()); // Changed from A to avoid conflict
        bindings.insert(Action::ToggleCollaboration, KeyBinding::new(KeyCode::KeyL).with_ctrl().with_shift()); // Changed from C to avoid conflict
        
        // Transform shortcuts
        bindings.insert(Action::ToggleTransformSpace, KeyBinding::new(KeyCode::KeyL).with_ctrl()); // Ctrl+L for World/Local space toggle
        
        // Camera shortcuts
        bindings.insert(Action::FocusSelection, KeyBinding::new(KeyCode::KeyF)); // F to focus on selection
        
        // Camera View Mode shortcuts (Blender-style numpad)
        bindings.insert(Action::ViewPerspectiveToggle, KeyBinding::new(KeyCode::Numpad5)); // Numpad 5 toggles perspective/ortho
        bindings.insert(Action::ViewTop, KeyBinding::new(KeyCode::Numpad8));               // Numpad 8 for top view
        bindings.insert(Action::ViewFront, KeyBinding::new(KeyCode::Numpad2));             // Numpad 2 for front view
        bindings.insert(Action::ViewSideLeft, KeyBinding::new(KeyCode::Numpad4));          // Numpad 4 for left side view
        bindings.insert(Action::ViewSideRight, KeyBinding::new(KeyCode::Numpad6));         // Numpad 6 for right side view
        
        // Snapping shortcuts
        bindings.insert(Action::SnapMode1, KeyBinding::new(KeyCode::Digit1));    // 1 for 1 unit snapping
        bindings.insert(Action::SnapMode2, KeyBinding::new(KeyCode::Digit2));    // 2 for 0.2 unit snapping
        bindings.insert(Action::SnapModeOff, KeyBinding::new(KeyCode::Digit3));  // 3 for no snapping
        
        // Quick rotation shortcuts
        bindings.insert(Action::RotateY90, KeyBinding::new(KeyCode::KeyR).with_ctrl()); // Ctrl+R to rotate 90° on Y
        bindings.insert(Action::TiltZ90, KeyBinding::new(KeyCode::KeyT).with_ctrl());   // Ctrl+T to tilt 90° on Z
        
        // Network shortcuts
        bindings.insert(Action::StartServer, KeyBinding::new(KeyCode::F9)); // F9 to start server
        bindings.insert(Action::ToggleNetworkPanel, KeyBinding::new(KeyCode::KeyN).with_ctrl().with_shift()); // Ctrl+Shift+N
        
        Self { bindings }
    }
}

impl KeyBindings {
    pub fn get(&self, action: Action) -> Option<&KeyBinding> {
        self.bindings.get(&action)
    }
    
    pub fn get_string(&self, action: Action) -> String {
        self.get(action)
            .map(|kb| kb.to_string_rep())
            .unwrap_or_else(|| "Not bound".to_string())
    }
    
    pub fn set(&mut self, action: Action, binding: KeyBinding) {
        self.bindings.insert(action, binding);
    }
    
    pub fn check(&self, action: Action, keys: &ButtonInput<KeyCode>) -> bool {
        self.get(action)
            .map(|binding| binding.matches(keys))
            .unwrap_or(false)
    }
    
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let serialized = ron::to_string(&self)?;
        std::fs::write("keybindings.ron", serialized)?;
        Ok(())
    }
    
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string("keybindings.ron")?;
        let bindings = ron::from_str(&contents)?;
        Ok(bindings)
    }
}

/// Plugin for keybindings system
pub struct KeyBindingsPlugin;

impl Plugin for KeyBindingsPlugin {
    fn build(&self, app: &mut App) {
        // Try to load saved bindings, otherwise use defaults
        let bindings = KeyBindings::load().unwrap_or_default();
        app.insert_resource(bindings)
            .add_systems(Update, (
                dispatch_keyboard_shortcuts,
                handle_menu_action_events,
            ).chain());
    }
}

// ============================================================================
// Keyboard Shortcut Dispatch System
// ============================================================================

/// Reads keyboard input each frame and dispatches tool changes + MenuActionEvents
fn dispatch_keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<KeyBindings>,
    mut studio_state: ResMut<crate::ui::StudioState>,
    mut menu_events: MessageWriter<crate::ui::MenuActionEvent>,
) {
    // Tool switching — directly update StudioState for instant response
    if bindings.check(Action::SelectTool, &keys) {
        studio_state.current_tool = crate::ui::Tool::Select;
        return; // Only one action per frame
    }
    if bindings.check(Action::MoveTool, &keys) {
        studio_state.current_tool = crate::ui::Tool::Move;
        return;
    }
    if bindings.check(Action::ScaleTool, &keys) {
        studio_state.current_tool = crate::ui::Tool::Scale;
        return;
    }
    if bindings.check(Action::RotateTool, &keys) {
        studio_state.current_tool = crate::ui::Tool::Rotate;
        return;
    }

    // All other actions → dispatch as MenuActionEvent
    let actions = [
        Action::Undo, Action::Redo,
        Action::Copy, Action::Paste, Action::Duplicate, Action::Delete,
        Action::SelectAll, Action::Group, Action::Ungroup,
        Action::LockSelection, Action::UnlockSelection, Action::ToggleAnchor,
        Action::ToggleExplorer, Action::ToggleProperties, Action::ToggleOutput,
        Action::ToggleCommandBar, Action::ToggleAssets, Action::ToggleCollaboration,
        Action::ToggleTransformSpace,
        Action::FocusSelection,
        Action::ViewPerspectiveToggle, Action::ViewTop, Action::ViewFront,
        Action::ViewSideLeft, Action::ViewSideRight,
        Action::SnapMode1, Action::SnapMode2, Action::SnapModeOff,
        Action::RotateY90, Action::TiltZ90,
        Action::StartServer, Action::ToggleNetworkPanel,
        Action::CSGNegate, Action::CSGUnion, Action::CSGIntersect, Action::CSGSeparate,
    ];

    for action in actions {
        if bindings.check(action, &keys) {
            menu_events.write(crate::ui::MenuActionEvent::new(action));
            return; // One action per frame to avoid conflicts
        }
    }
}

// ============================================================================
// MenuActionEvent Handler System
// ============================================================================

/// Processes MenuActionEvents dispatched by keyboard shortcuts or Slint UI.
/// Handles actions that modify StudioState or trigger editor behavior.
fn handle_menu_action_events(
    mut events: MessageReader<crate::ui::MenuActionEvent>,
    mut studio_state: ResMut<crate::ui::StudioState>,
    mut undo_events: MessageWriter<crate::commands::UndoCommandEvent>,
    mut redo_events: MessageWriter<crate::commands::RedoCommandEvent>,
) {
    for event in events.read() {
        match event.action {
            // Tool switching (also reachable via MenuActionEvent from Slint)
            Action::SelectTool => { studio_state.current_tool = crate::ui::Tool::Select; }
            Action::MoveTool   => { studio_state.current_tool = crate::ui::Tool::Move; }
            Action::ScaleTool  => { studio_state.current_tool = crate::ui::Tool::Scale; }
            Action::RotateTool => { studio_state.current_tool = crate::ui::Tool::Rotate; }

            // Undo/Redo
            Action::Undo => { undo_events.write(crate::commands::UndoCommandEvent); }
            Action::Redo => { redo_events.write(crate::commands::RedoCommandEvent); }

            // View panel toggles
            Action::ToggleExplorer   => { studio_state.show_explorer = !studio_state.show_explorer; }
            Action::ToggleProperties => { studio_state.show_properties = !studio_state.show_properties; }
            Action::ToggleOutput     => { studio_state.show_output = !studio_state.show_output; }

            // Paste
            Action::Paste => { studio_state.pending_paste = true; }

            // Command bar
            Action::ToggleCommandBar => { /* Handled by Slint UI directly */ }

            // Snapping
            Action::SnapMode1 | Action::SnapMode2 | Action::SnapModeOff => {
                // Snapping is handled by editor_settings; these events are consumed
                // by the editor_settings system if it listens for them.
            }

            // Other actions are consumed by their respective systems
            _ => {}
        }
    }
}
