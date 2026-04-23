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
    Cut,
    Paste,
    Duplicate,
    Delete,
    SelectAll,
    /// Add the direct children of the current selection (single level).
    SelectChildren,
    /// Recursively add every descendant of the current selection.
    SelectDescendants,
    /// Replace selection with the parent(s) of the current selection.
    SelectParent,
    /// Add siblings sharing the same parent.
    SelectSiblings,
    /// Flip selection to everything NOT currently selected.
    InvertSelection,
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
    
    // Nudge: `-` lifts by grid unit, `+` is the smart settle
    // (raycast-down flush, or pop-on-top when inside a container).
    NudgeUp,        // Move selection up by one grid unit (- key)
    NudgeDown,      // Smart settle: raycast down + flush OR pop on top (+ key)

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

    // Smart Build Modal Tools (activate via ModalToolRegistry)
    ToolPartSwap,   // Ctrl+Alt+P — swap two parts' positions
    ToolEdgeAlign,  // Ctrl+Alt+E — translate source to target's edge
    ToolModelReflect, // Ctrl+Alt+M — reflect selection across a plane
    ToolGapFill,    // Ctrl+Alt+G — fill the gap between two parts
    ToolResizeAlign,// Ctrl+Alt+A — resize source until its face meets target's
    ToolMaterialFlip,// Ctrl+Alt+F — flip texture UVs on selected parts

    // Array tools (Phase 1)
    ToolLinearArray, // Ctrl+Alt+L — N copies along a step vector
    ToolRadialArray, // Ctrl+Alt+R — N copies around a pivot axis
    ToolGridArray,   // Ctrl+Alt+K — Nx × Ny × Nz 3D pattern
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
            Action::Cut => "Cut",
            Action::Paste => "Paste",
            Action::Duplicate => "Duplicate",
            Action::Delete => "Delete",
            Action::SelectAll => "Select All",
            Action::SelectChildren => "Select Children",
            Action::SelectDescendants => "Select Descendants",
            Action::SelectParent => "Select Parent",
            Action::SelectSiblings => "Select Siblings",
            Action::InvertSelection => "Invert Selection",
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
            Action::SnapMode1 => "Snap Mode: 1m",
            Action::SnapMode2 => "Snap Mode: 0.2m",
            Action::SnapModeOff => "Snap Mode: Off",
            Action::NudgeUp => "Nudge Up",
            Action::NudgeDown => "Nudge Down",
            Action::RotateY90 => "Rotate 90° (Y Axis)",
            Action::TiltZ90 => "Tilt 90° (Z Axis)",
            Action::StartServer => "Start Server",
            Action::StopServer => "Stop Server",
            Action::ToggleNetworkPanel => "Toggle Network Panel",
            Action::CSGNegate => "CSG Negate",
            Action::CSGUnion => "CSG Union",
            Action::CSGIntersect => "CSG Intersect",
            Action::CSGSeparate => "CSG Separate",
            Action::ToolPartSwap => "Part Swap",
            Action::ToolEdgeAlign => "Edge Align",
            Action::ToolModelReflect => "Model Reflect",
            Action::ToolGapFill => "Gap Fill",
            Action::ToolResizeAlign => "Resize Align",
            Action::ToolMaterialFlip => "Material Flip",
            Action::ToolLinearArray => "Linear Array",
            Action::ToolRadialArray => "Radial Array",
            Action::ToolGridArray => "Grid Array",
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

    /// User-facing display string — strips the `Key` / `Digit` enum-variant
    /// prefixes produced by `{:?}` on KeyCode so users see "Ctrl+Z" instead
    /// of "Ctrl+KeyZ". Used by the ribbon to render subtitle shortcuts.
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl  { parts.push("Ctrl".to_string()); }
        if self.alt   { parts.push("Alt".to_string()); }
        if self.shift { parts.push("Shift".to_string()); }
        parts.push(key_display(&self.key));
        parts.join("+")
    }
}

/// Map a Bevy [`bevy::input::keyboard::KeyCode`] to a short, user-readable
/// string. Handles the common letter/digit/arrow/function cases; falls back
/// to `{:?}` for anything exotic (media keys, IMEs, etc.).
fn key_display(key: &bevy::input::keyboard::KeyCode) -> String {
    use bevy::input::keyboard::KeyCode;
    let raw = format!("{:?}", key);
    // KeyA..KeyZ → A..Z
    if let Some(rest) = raw.strip_prefix("Key") {
        if rest.len() == 1 {
            return rest.to_string();
        }
    }
    // Digit0..Digit9 → 0..9
    if let Some(rest) = raw.strip_prefix("Digit") {
        return rest.to_string();
    }
    // Function keys stay as F1..F24
    // Arrows → ↑ ↓ ← → for compactness
    match key {
        KeyCode::ArrowUp    => "↑".to_string(),
        KeyCode::ArrowDown  => "↓".to_string(),
        KeyCode::ArrowLeft  => "←".to_string(),
        KeyCode::ArrowRight => "→".to_string(),
        KeyCode::Space      => "Space".to_string(),
        KeyCode::Escape     => "Esc".to_string(),
        KeyCode::Enter      => "Enter".to_string(),
        KeyCode::Backspace  => "Backspace".to_string(),
        KeyCode::Delete     => "Del".to_string(),
        KeyCode::Tab        => "Tab".to_string(),
        _ => raw,
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
        bindings.insert(Action::Cut, KeyBinding::new(KeyCode::KeyX).with_ctrl());
        bindings.insert(Action::Paste, KeyBinding::new(KeyCode::KeyV).with_ctrl());
        bindings.insert(Action::Duplicate, KeyBinding::new(KeyCode::KeyD).with_ctrl());
        bindings.insert(Action::Delete, KeyBinding::new(KeyCode::Delete));
        bindings.insert(Action::SelectAll, KeyBinding::new(KeyCode::KeyA).with_ctrl());
        // Hierarchy selection (Maya / Blender parity):
        //   Ctrl+Shift+C → Select Children (one level)
        //   Ctrl+Shift+D → Select Descendants (recursive)
        //   Ctrl+Shift+U → Select Parent (up one level)
        //   Ctrl+Shift+S → Select Siblings
        //   Ctrl+I       → Invert Selection
        bindings.insert(Action::SelectChildren, KeyBinding::new(KeyCode::KeyC).with_ctrl().with_shift());
        bindings.insert(Action::SelectDescendants, KeyBinding::new(KeyCode::KeyD).with_ctrl().with_shift());
        bindings.insert(Action::SelectParent, KeyBinding::new(KeyCode::KeyU).with_ctrl().with_shift());
        bindings.insert(Action::SelectSiblings, KeyBinding::new(KeyCode::KeyS).with_ctrl().with_shift());
        bindings.insert(Action::InvertSelection, KeyBinding::new(KeyCode::KeyI).with_ctrl());
        bindings.insert(Action::Group, KeyBinding::new(KeyCode::KeyG).with_ctrl());
        bindings.insert(Action::Ungroup, KeyBinding::new(KeyCode::KeyU).with_ctrl());

        // Smart Build Tools — activate modal tools via the registry.
        // Ctrl+Alt for a distinct namespace from tool-switch shortcuts
        // (which are bare Alt+Letter for Select/Move/Scale/Rotate).
        bindings.insert(Action::ToolPartSwap, KeyBinding::new(KeyCode::KeyP).with_ctrl().with_alt());
        bindings.insert(Action::ToolEdgeAlign, KeyBinding::new(KeyCode::KeyE).with_ctrl().with_alt());
        bindings.insert(Action::ToolModelReflect, KeyBinding::new(KeyCode::KeyM).with_ctrl().with_alt());
        bindings.insert(Action::ToolGapFill, KeyBinding::new(KeyCode::KeyG).with_ctrl().with_alt());
        bindings.insert(Action::ToolResizeAlign, KeyBinding::new(KeyCode::KeyA).with_ctrl().with_alt());
        bindings.insert(Action::ToolMaterialFlip, KeyBinding::new(KeyCode::KeyF).with_ctrl().with_alt());
        bindings.insert(Action::ToolLinearArray, KeyBinding::new(KeyCode::KeyL).with_ctrl().with_alt());
        bindings.insert(Action::ToolRadialArray, KeyBinding::new(KeyCode::KeyR).with_ctrl().with_alt());
        bindings.insert(Action::ToolGridArray,   KeyBinding::new(KeyCode::KeyK).with_ctrl().with_alt());
        
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
        bindings.insert(Action::NudgeUp,   KeyBinding::new(KeyCode::Minus));    // - key
        bindings.insert(Action::NudgeDown, KeyBinding::new(KeyCode::Equal));    // +/= key
        
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
            .init_resource::<NudgeTimer>()
            .add_systems(Update, (
                dispatch_keyboard_shortcuts
                    .after(crate::ui::slint_ui::update_slint_ui_focus),
                handle_menu_action_events.after(dispatch_keyboard_shortcuts),
                handle_nudge_keys
                    .after(crate::ui::slint_ui::update_slint_ui_focus),
            ));
    }
}

// ============================================================================
// Keyboard Shortcut Dispatch System
// ============================================================================

/// Reads keyboard input each frame and dispatches tool changes + MenuActionEvents.
/// Uses Option<ResMut> to avoid silent skip from error handler when resources are missing.
fn dispatch_keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Option<Res<KeyBindings>>,
    studio_state: Option<ResMut<crate::ui::StudioState>>,
    mut menu_events: MessageWriter<crate::ui::MenuActionEvent>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
) {
    // Block keyboard shortcuts when a text input has focus or overlay modal is open
    // (typing in Properties, Settings dialog, Workshop chat, etc.)
    if let Some(ref focus) = ui_focus {
        if focus.text_input_focused {
            return;
        }
    }
    if crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some(mut studio_state) = studio_state else { return };
    let Some(bindings) = bindings else { return };

    // Tool switching — directly update StudioState for instant response
    if bindings.check(Action::SelectTool, &keys) {
        info!("⌨️ Shortcut: Select Tool (Alt+Z)");
        studio_state.current_tool = crate::ui::Tool::Select;
        return;
    }
    if bindings.check(Action::MoveTool, &keys) {
        info!("⌨️ Shortcut: Move Tool (Alt+X)");
        studio_state.current_tool = crate::ui::Tool::Move;
        return;
    }
    if bindings.check(Action::ScaleTool, &keys) {
        info!("⌨️ Shortcut: Scale Tool (Alt+C)");
        studio_state.current_tool = crate::ui::Tool::Scale;
        return;
    }
    if bindings.check(Action::RotateTool, &keys) {
        info!("⌨️ Shortcut: Rotate Tool (Alt+V)");
        studio_state.current_tool = crate::ui::Tool::Rotate;
        return;
    }

    // Delete key only — Backspace is reserved for text editing
    if keys.just_pressed(KeyCode::Delete) {
        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
        if !ctrl && !alt {
            info!("⌨️ Delete/Backspace key detected directly");
            menu_events.write(crate::ui::MenuActionEvent::new(Action::Delete));
            return;
        }
    }

    // All other actions → dispatch as MenuActionEvent
    let actions = [
        Action::Undo, Action::Redo,
        Action::Copy, Action::Cut, Action::Paste, Action::Duplicate, Action::Delete,
        Action::SelectAll,
        Action::SelectChildren, Action::SelectDescendants,
        Action::SelectParent, Action::SelectSiblings,
        Action::InvertSelection,
        Action::Group, Action::Ungroup,
        Action::LockSelection, Action::UnlockSelection, Action::ToggleAnchor,
        Action::ToggleExplorer, Action::ToggleProperties, Action::ToggleOutput,
        Action::ToggleCommandBar, Action::ToggleAssets, Action::ToggleCollaboration,
        Action::ToggleTransformSpace,
        Action::FocusSelection,
        Action::ViewPerspectiveToggle, Action::ViewTop, Action::ViewFront,
        Action::ViewSideLeft, Action::ViewSideRight,
        Action::SnapMode1, Action::SnapMode2, Action::SnapModeOff,
        Action::NudgeUp, Action::NudgeDown,
        Action::RotateY90, Action::TiltZ90,
        Action::StartServer, Action::ToggleNetworkPanel,
        Action::CSGNegate, Action::CSGUnion, Action::CSGIntersect, Action::CSGSeparate,
        Action::ToolPartSwap, Action::ToolEdgeAlign, Action::ToolModelReflect, Action::ToolGapFill,
        Action::ToolResizeAlign, Action::ToolMaterialFlip,
        Action::ToolLinearArray, Action::ToolRadialArray, Action::ToolGridArray,
    ];

    for action in actions {
        if bindings.check(action, &keys) {
            info!("⌨️ Shortcut: {:?}", action);
            menu_events.write(crate::ui::MenuActionEvent::new(action));
            return;
        }
    }
}

// ============================================================================
// MenuActionEvent Handler System
// ============================================================================

/// Processes MenuActionEvents dispatched by keyboard shortcuts or Slint UI.
/// Handles actions that modify StudioState or trigger editor behavior.
/// Uses Option wrappers to prevent silent skip from error handler.
fn handle_menu_action_events(
    mut events: MessageReader<crate::ui::MenuActionEvent>,
    mut commands: Commands,
    studio_state: Option<ResMut<crate::ui::StudioState>>,
    // Event writers bundled as tuple (keeps total param count ≤ 16)
    mut event_writers: (
        MessageWriter<crate::commands::UndoCommandEvent>,
        MessageWriter<crate::commands::RedoCommandEvent>,
        MessageWriter<crate::camera_controller::FrameSelectionEvent>,
        MessageWriter<crate::clipboard::CopyEvent>,
        MessageWriter<crate::clipboard::DuplicateEvent>,
        MessageWriter<crate::undo::UndoEvent>,
        MessageWriter<crate::undo::RedoEvent>,
    ),
    selection_manager: Option<Res<crate::selection_sync::SelectionSyncManager>>,
    entity_query: Query<(Entity, Option<&GlobalTransform>, Option<&eustress_common::classes::BasePart>),
        Or<(With<crate::rendering::PartEntity>, With<eustress_common::classes::Instance>)>>,
    instance_query: Query<&eustress_common::classes::Instance>,
    instance_file_query: Query<&crate::space::instance_loader::InstanceFile>,
    mut file_registry: Option<ResMut<crate::space::SpaceFileRegistry>>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    mut editor_settings: Option<ResMut<crate::editor_settings::EditorSettings>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    mut explorer_state: Option<ResMut<crate::ui::slint_ui::UnifiedExplorerState>>,
    // Hierarchy-selection event writers (bundled to stay under Bevy's
    // 16-param limit on systems).
    mut selection_events: (
        MessageWriter<crate::selection_sync::SelectChildrenEvent>,
        MessageWriter<crate::selection_sync::SelectDescendantsEvent>,
        MessageWriter<crate::selection_sync::SelectParentEvent>,
        MessageWriter<crate::selection_sync::SelectSiblingsEvent>,
        MessageWriter<crate::selection_sync::InvertSelectionEvent>,
    ),
    // Modal-tool activation events — fired by the Ctrl+Alt+<Letter>
    // shortcuts for Smart Build Tools.
    mut activate_modal_tool: MessageWriter<crate::modal_tool::ActivateModalToolEvent>,
) {
    let (
        ref mut undo_events,
        ref mut redo_events,
        ref mut frame_events,
        ref mut copy_events,
        ref mut duplicate_events,
        ref mut undo_action_events,
        ref mut redo_action_events,
    ) = event_writers;
    let Some(mut studio_state) = studio_state else { return };

    for event in events.read() {
        match event.action {
            // Tool switching (also reachable via MenuActionEvent from Slint)
            Action::SelectTool => { studio_state.current_tool = crate::ui::Tool::Select; }
            Action::MoveTool   => { studio_state.current_tool = crate::ui::Tool::Move; }
            Action::ScaleTool  => { studio_state.current_tool = crate::ui::Tool::Scale; }
            Action::RotateTool => { studio_state.current_tool = crate::ui::Tool::Rotate; }

            // Undo/Redo — fire both event types:
            // UndoCommandEvent → CommandHistory (selection undo)
            // UndoEvent → UndoStack (transform undo)
            Action::Undo => {
                undo_events.write(crate::commands::UndoCommandEvent);
                undo_action_events.write(crate::undo::UndoEvent);
            }
            Action::Redo => {
                redo_events.write(crate::commands::RedoCommandEvent);
                redo_action_events.write(crate::undo::RedoEvent);
            }

            // View panel toggles
            Action::ToggleExplorer   => { studio_state.show_explorer = !studio_state.show_explorer; }
            Action::ToggleProperties => { studio_state.show_properties = !studio_state.show_properties; }
            Action::ToggleOutput     => { studio_state.show_output = !studio_state.show_output; }

            // Copy / Paste
            Action::Copy => { copy_events.write(crate::clipboard::CopyEvent { is_cut: false }); }
            Action::Cut => { copy_events.write(crate::clipboard::CopyEvent { is_cut: true }); }
            Action::Paste => { studio_state.pending_paste = true; }

            // Command bar
            Action::ToggleCommandBar => { /* Handled by Slint UI directly */ }

            // Focus camera on selection (F key)
            // Reads from SelectionSyncManager directly so it works even on the same
            // frame an Explorer-click selection happens (no SelectionBox yet).
            Action::FocusSelection => {
                // Get the set of currently selected IDs
                let selected_ids: std::collections::HashSet<String> = selection_manager
                    .as_ref()
                    .map(|sm| sm.0.read().get_selected().into_iter().collect())
                    .unwrap_or_default();

                let mut min = Vec3::splat(f32::MAX);
                let mut max = Vec3::splat(f32::MIN);
                let mut has_selection = false;

                if !selected_ids.is_empty() {
                    for (entity, transform, base_part) in entity_query.iter() {
                        let id = format!("{}v{}", entity.index(), entity.generation());
                        if !selected_ids.contains(&id) { continue; }

                        let pos = transform.map(|t| t.translation()).unwrap_or(Vec3::ZERO);
                        let half_size = base_part
                            .map(|bp| bp.size * 0.5)
                            .unwrap_or(Vec3::splat(0.5));
                        min = min.min(pos - half_size);
                        max = max.max(pos + half_size);
                        has_selection = true;
                    }
                }

                if has_selection {
                    frame_events.write(crate::camera_controller::FrameSelectionEvent {
                        target_bounds: Some((min, max)),
                    });
                    info!("📷 Focus on selection: bounds ({:?} to {:?})", min, max);
                }
                // No selection → no-op. Framing the whole scene on an empty
                // selection used to fly the camera to an empty world and show
                // only sky — unintuitive.
            }

            // Snapping
            Action::SnapMode1 => {
                if let Some(ref mut es) = editor_settings {
                    es.snap_size = 1.0;
                    es.snap_enabled = true;
                }
            }
            Action::SnapMode2 => {
                if let Some(ref mut es) = editor_settings {
                    es.snap_size = 0.2;
                    es.snap_enabled = true;
                }
            }
            Action::SnapModeOff => {
                if let Some(ref mut es) = editor_settings {
                    es.snap_enabled = false;
                }
            }

            // Delete selected entities; respawn default camera at origin if Camera class deleted
            Action::Delete => {
                let sm_exists = selection_manager.is_some();
                let selected_ids: std::collections::HashSet<String> = selection_manager
                    .as_ref()
                    .map(|sm| sm.0.read().get_selected().into_iter().collect())
                    .unwrap_or_default();

                info!("🗑️ Delete action: sm_exists={}, selected_ids={:?}", sm_exists, selected_ids);

                if selected_ids.is_empty() {
                    info!("🗑️ Delete: nothing selected");
                } else {
                    let mut camera_deleted = false;
                    let mut trashed_paths: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();

                    for (entity, _, _) in entity_query.iter() {
                        let id = format!("{}v{}", entity.index(), entity.generation());
                        if !selected_ids.contains(&id) { continue; }
                        if instance_query.get(entity)
                            .map(|inst| inst.class_name == eustress_common::classes::ClassName::Camera)
                            .unwrap_or(false)
                        {
                            camera_deleted = true;
                        }
                        // Move TOML (or the whole folder, for folder-based
                        // entities) to .eustress/trash/ so Ctrl+Z can restore.
                        if let Ok(inst_file) = instance_file_query.get(entity) {
                            let toml_path = inst_file.toml_path.clone();
                            // Non-removable folders: the Workshop folder
                            // under SoulService is the engine's chat
                            // history root — deleting it would scramble
                            // session persistence + trap the Workshop
                            // panel in a "no Space bound" state. Skip
                            // silently (match OS convention for
                            // system-protected paths) and continue so
                            // the rest of the multi-select delete still
                            // works.
                            let is_workshop_root = toml_path
                                .components()
                                .rev()
                                .take(3)
                                .collect::<Vec<_>>()
                                .iter()
                                .rev()
                                .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
                                .collect::<Vec<_>>()
                                .ends_with(&["soulservice".to_string(), "workshop".to_string(), "_instance.toml".to_string()]);
                            if is_workshop_root {
                                info!("🔒 Skipping delete on protected Workshop folder");
                                continue;
                            }
                            // Folder-based entities live in `Foo/_instance.toml`;
                            // trashing only the TOML leaves an empty folder on disk
                            // AND orphans sibling files (Summary.md, child instances).
                            // Trash the containing folder instead.
                            let is_folder_instance = toml_path
                                .file_name()
                                .map(|n| n.to_string_lossy() == "_instance.toml")
                                .unwrap_or(false);
                            let source_path = if is_folder_instance {
                                toml_path.parent().unwrap_or(toml_path.as_path()).to_path_buf()
                            } else {
                                toml_path.clone()
                            };

                            if source_path.exists() {
                                // trash dir lives one level up from the part's
                                // containing folder (or Workspace root for flat files).
                                let trash_anchor = if is_folder_instance {
                                    source_path.parent()
                                } else {
                                    toml_path.parent()
                                };
                                let trash_dir = trash_anchor
                                    .and_then(|p| p.parent())
                                    .unwrap_or(trash_anchor.unwrap_or(std::path::Path::new(".")))
                                    .join(".eustress").join("trash");
                                let _ = std::fs::create_dir_all(&trash_dir);
                                let trash_name = source_path.file_name().unwrap_or_default();
                                let trash_path = trash_dir.join(trash_name);

                                // Tell file-watcher to ignore the impending delete
                                // so it doesn't try to despawn an already-gone entity.
                                if let Some(ref mut registry) = file_registry {
                                    registry.rename_in_progress.insert(toml_path.clone());
                                    registry.rename_in_progress.insert(source_path.clone());
                                }

                                match std::fs::rename(&source_path, &trash_path) {
                                    Ok(_) => {
                                        trashed_paths.push((toml_path.clone(), trash_path));
                                        info!("🗑️ Moved {:?} to trash", source_path.file_name().unwrap_or_default());
                                    }
                                    Err(_) => {
                                        // Fallback: hard-delete. For folders, remove recursively.
                                        if is_folder_instance {
                                            let _ = std::fs::remove_dir_all(&source_path);
                                        } else {
                                            let _ = std::fs::remove_file(&source_path);
                                        }
                                        info!("🗑️ Deleted {:?}", source_path.file_name().unwrap_or_default());
                                    }
                                }
                            }
                            if let Some(ref mut registry) = file_registry {
                                registry.unregister_file(&toml_path);
                            }
                        }
                        commands.entity(entity).despawn();
                        info!("🗑️ Deleted entity {:?} ({})", entity, id);
                    }

                    // Push to undo stack so Ctrl+Z can restore
                    if !trashed_paths.is_empty() {
                        undo_stack.push(crate::undo::Action::TrashEntities { paths: trashed_paths });
                    }

                    // Clear selection after delete
                    if let Some(ref sm) = selection_manager {
                        sm.0.write().clear();
                    }

                    // Force Explorer re-sync next frame so the deleted entities
                    // disappear from the tree without waiting for the 30-frame
                    // throttle — avoids the "deleted but still shown" UX.
                    if let Some(ref mut es) = explorer_state {
                        es.needs_immediate_sync = true;
                    }
                    // Respawn a default camera at origin so the viewport is never left without one
                    if camera_deleted {
                        use bevy::core_pipeline::tonemapping::Tonemapping;
                        use eustress_common::classes::{Instance, ClassName};
                        commands.spawn((
                            Camera3d::default(),
                            Tonemapping::Reinhard,
                            Transform::from_xyz(10.0, 8.0, 10.0)
                                .looking_at(Vec3::ZERO, Vec3::Y),
                            Projection::Perspective(PerspectiveProjection {
                                fov: 70.0_f32.to_radians(),
                                near: 0.1,
                                far: 10000.0,
                                ..default()
                            }),
                            Instance {
                                name: "Camera".to_string(),
                                class_name: ClassName::Camera,
                                archivable: true,
                                id: 0,
                                ..Default::default()
                            },
                            Name::new("Camera"),
                        ));
                        info!("📷 Camera deleted — respawned default camera at origin");
                    }
                }
            }

            // Select All (Ctrl+A) — select all unlocked BasePart entities
            // Also blocked when cursor is over UI panels (Properties text fields)
            Action::SelectAll => {
                if ui_focus.as_ref().map(|f| f.has_focus).unwrap_or(false) { continue; }
                if let Some(ref sel_mgr) = selection_manager {
                    let sm = sel_mgr.0.write();
                    sm.clear();
                    for (entity, _, bp) in entity_query.iter() {
                        // Only select entities that have BasePart (actual 3D parts)
                        let Some(bp) = bp else { continue; };
                        // Skip locked parts
                        if bp.locked { continue; }
                        // Skip adornments
                        if instance_query.get(entity)
                            .map(|i| i.class_name.is_adornment())
                            .unwrap_or(false) { continue; }
                        let id = format!("{}v{}", entity.index(), entity.generation());
                        sm.add_to_selection(id);
                    }
                }
            }

            // Duplicate (Ctrl+D) — copy + paste in place
            Action::Duplicate => {
                duplicate_events.write(crate::clipboard::DuplicateEvent);
            }

            // Hierarchy-selection commands — emit the corresponding
            // event; handler systems in selection_sync.rs do the work.
            Action::SelectChildren => {
                selection_events.0.write(crate::selection_sync::SelectChildrenEvent);
            }
            Action::SelectDescendants => {
                selection_events.1.write(crate::selection_sync::SelectDescendantsEvent);
            }
            Action::SelectParent => {
                selection_events.2.write(crate::selection_sync::SelectParentEvent);
            }
            Action::SelectSiblings => {
                selection_events.3.write(crate::selection_sync::SelectSiblingsEvent);
            }
            Action::InvertSelection => {
                selection_events.4.write(crate::selection_sync::InvertSelectionEvent);
            }

            // Smart Build Tools — activate via the modal-tool registry.
            // The tool_id strings match the factories registered in
            // `tools_smart::register_smart_tools`.
            Action::ToolPartSwap => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "part_swap_positions".to_string(),
                });
            }
            Action::ToolEdgeAlign => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "edge_align".to_string(),
                });
            }
            Action::ToolModelReflect => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "model_reflect".to_string(),
                });
            }
            Action::ToolGapFill => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "gap_fill".to_string(),
                });
            }
            Action::ToolResizeAlign => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "resize_align".to_string(),
                });
            }
            Action::ToolMaterialFlip => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "material_flip".to_string(),
                });
            }
            Action::ToolLinearArray => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "linear_array".to_string(),
                });
            }
            Action::ToolRadialArray => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "radial_array".to_string(),
                });
            }
            Action::ToolGridArray => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "grid_array".to_string(),
                });
            }

            // ── Boolean (CSG) ribbon group ─────────────────────────
            //
            // truck-shapeops 0.4 exports `or` (union) and `and`
            // (intersect) but NOT `not` (difference). For now, the
            // ribbon buttons log selection info + emit an Output-panel
            // entry so users get immediate feedback instead of a
            // silent no-op (the "buttons don't work" regression
            // reported 2026-04-23). Wiring the Bevy-mesh → truck
            // `Solid` → Bevy-mesh round-trip is tracked in
            // `eustress-cad::eval::boolean_*`; once that round-trip
            // exposes a `fn apply_csg_to_selection` helper these arms
            // call into it directly.
            //
            // `CSGSeparate` DOES work today: unparent all children of
            // any selected Model / CSG-union entity so each sub-body
            // becomes an independent selectable again. No mesh-edit
            // needed.
            Action::CSGUnion => {
                let n = selected_count(&selection_manager);
                if n < 2 {
                    warn!("🔨 CSG Union needs ≥2 selected bodies (have {}). Select both parts first.", n);
                } else {
                    info!("🔨 CSG Union: {} bodies selected — truck-shapeops wiring pending (v0.2). \
                           Use Model grouping as a non-destructive placeholder in the meantime.", n);
                }
            }
            Action::CSGNegate => {
                let n = selected_count(&selection_manager);
                if n < 2 {
                    warn!("🔨 CSG Subtract needs ≥2 selected bodies (first = target, others = cutters). Have {}.", n);
                } else {
                    warn!("🔨 CSG Subtract: truck-shapeops 0.4 doesn't export `not` — feature lands \
                           with the upcoming shapeops release. {} bodies selected.", n);
                }
            }
            Action::CSGIntersect => {
                let n = selected_count(&selection_manager);
                if n < 2 {
                    warn!("🔨 CSG Intersect needs ≥2 selected bodies (have {}). Select both parts first.", n);
                } else {
                    info!("🔨 CSG Intersect: {} bodies selected — truck-shapeops wiring pending (v0.2).", n);
                }
            }
            Action::CSGSeparate => {
                // Selection-level ungroup — strip `ChildOf` from
                // every child of a selected Model / Folder so each
                // sub-entity becomes an independent selectable
                // again. Mirror of the standard Ungroup (Ctrl+U)
                // but scoped to the Boolean-group metaphor.
                let selected_ids: std::collections::HashSet<String> = selection_manager
                    .as_ref()
                    .map(|sm| sm.0.read().get_selected().into_iter().collect())
                    .unwrap_or_default();
                let mut separated = 0u32;
                for (entity, _tf, _bp) in entity_query.iter() {
                    let id = format!("{}v{}", entity.index(), entity.generation());
                    if !selected_ids.contains(&id) { continue; }
                    // Only containers (Model / Folder) have children
                    // worth separating. Plain Parts get a no-op.
                    if !matches!(
                        instance_query.get(entity).map(|i| i.class_name),
                        Ok(eustress_common::classes::ClassName::Model)
                        | Ok(eustress_common::classes::ClassName::Folder),
                    ) { continue; }
                    commands.entity(entity).remove::<bevy::prelude::Children>();
                    separated += 1;
                }
                if separated > 0 {
                    info!("🔨 CSG Separate: detached children of {} container(s).", separated);
                } else {
                    warn!("🔨 CSG Separate: select a Model or Folder to separate its children.");
                }
            }

            // Other actions are consumed by their respective systems
            _ => {}
        }
    }
}

/// Cheap helper for the CSG action arms — returns how many entities
/// are currently selected via `SelectionSyncManager`. `0` when the
/// manager resource isn't available (e.g. during startup).
fn selected_count(
    sm: &Option<Res<crate::selection_sync::SelectionSyncManager>>,
) -> usize {
    sm.as_ref()
        .map(|m| m.0.read().get_selected().len())
        .unwrap_or(0)
}

// ============================================================================
// Nudge System — +/- keys move selection up/down by grid unit
// ============================================================================

/// State for nudge keys — tracks hold time and whether initial press was consumed.
#[derive(Resource, Default)]
struct NudgeTimer {
    up_held: bool,
    up_timer: f32,
    down_held: bool,
    down_timer: f32,
}

/// Initial delay before auto-repeat starts (seconds). ~OS-standard
/// keyboard-repeat latency — short enough that holding the key feels
/// responsive, long enough that a deliberate single tap stays a single
/// nudge.
const NUDGE_DELAY_SECS: f32 = 0.30;
/// Repeat interval once auto-repeat is active (seconds). 12 nudges/sec.
const NUDGE_REPEAT_SECS: f32 = 0.08;

/// Queries + resources needed by `handle_nudge_keys`. Bundled into a
/// `SystemParam` so the handler itself stays under Bevy's 16-param
/// limit — the Move-Down settle path needs the selection transforms,
/// all-part transforms (for AABB containment), and a `SpatialQuery`
/// for the downward raycast, which together outgrow the flat param
/// list.
#[derive(bevy::ecs::system::SystemParam)]
pub struct NudgeContext<'w, 's> {
    pub selected: Query<
        'w, 's,
        (
            Entity,
            &'static mut Transform,
            &'static GlobalTransform,
            Option<&'static crate::classes::BasePart>,
        ),
        With<crate::selection_box::Selected>,
    >,
    /// Every other part in the scene — used to detect "selection is
    /// inside another part's AABB" for the pop-on-top semantics.
    pub other_parts: Query<
        'w, 's,
        (Entity, &'static GlobalTransform, &'static crate::classes::BasePart),
        Without<crate::selection_box::Selected>,
    >,
    pub spatial: avian3d::prelude::SpatialQuery<'w, 's>,
}

fn handle_nudge_keys(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut timer: ResMut<NudgeTimer>,
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    mut ctx: NudgeContext,
) {
    // Block when text input focused or overlay has focus
    if ui_focus.as_ref().map(|f| f.text_input_focused).unwrap_or(false) { return; }
    if crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) { return; }

    let snap = settings.as_ref().map(|s| if s.snap_enabled { s.snap_size } else { 1.0 }).unwrap_or(1.0);

    // ── `-` = Move Up (simple snap-grid lift) ─────────────────────
    if keys.pressed(KeyCode::Minus) {
        if !timer.up_held {
            timer.up_held = true;
            timer.up_timer = 0.0;
            nudge_up(&mut ctx.selected, snap);
        } else {
            timer.up_timer += time.delta_secs();
            if timer.up_timer >= NUDGE_DELAY_SECS {
                timer.up_timer -= NUDGE_REPEAT_SECS;
                nudge_up(&mut ctx.selected, snap);
            }
        }
    } else {
        timer.up_held = false;
        timer.up_timer = 0.0;
    }

    // ── `+` = Settle Down (smart raycast + pop-on-top) ───────────
    if keys.pressed(KeyCode::Equal) {
        if !timer.down_held {
            timer.down_held = true;
            timer.down_timer = 0.0;
            settle_down(&mut ctx, snap);
        } else {
            timer.down_timer += time.delta_secs();
            if timer.down_timer >= NUDGE_DELAY_SECS {
                timer.down_timer -= NUDGE_REPEAT_SECS;
                settle_down(&mut ctx, snap);
            }
        }
    } else {
        timer.down_held = false;
        timer.down_timer = 0.0;
    }
}

/// Simple lift: every selected entity moves up by `snap` on +Y.
fn nudge_up(
    selected: &mut Query<
        (
            Entity,
            &mut Transform,
            &GlobalTransform,
            Option<&crate::classes::BasePart>,
        ),
        With<crate::selection_box::Selected>,
    >,
    snap: f32,
) {
    for (_, mut t, _, _) in selected.iter_mut() {
        t.translation.y += snap;
    }
}

/// Smart settle: for each selected entity, resolve its vertical
/// position via (in priority order):
///   1. **Pop-on-top**: if the selection's center is inside another
///      part's world-aligned AABB AND above that part's horizontal
///      midplane, snap the selection so its *bottom* sits flush with
///      the container's *top* face.
///   2. **Ground-flush raycast**: cast a ray from the selection's
///      current center straight down; snap the selection so its
///      bottom sits flush with the first hit point.
///   3. **Fallback**: if neither path finds a support surface, fall
///      back to a plain `-= snap` nudge so the key still feels
///      responsive on parts floating in empty space.
fn settle_down(ctx: &mut NudgeContext, snap: f32) {
    use bevy::math::Dir3;
    // Snapshot other parts once per key-fire so the inner AABB-
    // containment check doesn't re-borrow the query per selected entity.
    let containers: Vec<(Vec3, Vec3)> = ctx
        .other_parts
        .iter()
        .map(|(_, gt, bp)| {
            let center = gt.translation();
            let half = bp.size * 0.5;
            (center - half, center + half) // (aabb_min, aabb_max)
        })
        .collect();

    // Collect selected entity snapshots first — we can't hold a
    // mutable borrow of the query across its own iter_mut body when
    // we need to read the spatial-query resource too. Use the
    // *world-space* center via `GlobalTransform` so the AABB
    // containment check below compares apples-to-apples with
    // `containers` (which are world-space AABBs).
    let mut selected_snapshot: Vec<(Entity, Vec3, f32)> = Vec::new();
    for (entity, _tf, gt, bp) in ctx.selected.iter() {
        let center = gt.translation();
        let half_height = bp.map(|b| b.size.y * 0.5).unwrap_or(0.5);
        selected_snapshot.push((entity, center, half_height));
    }

    for (entity, center, half_height) in selected_snapshot {
        let mut target_y: Option<f32> = None;

        // 1. Pop-on-top — is the center inside any container's AABB
        //    AND above that container's horizontal midplane?
        for (min, max) in &containers {
            let inside = center.x >= min.x && center.x <= max.x
                && center.y >= min.y && center.y <= max.y
                && center.z >= min.z && center.z <= max.z;
            if !inside { continue; }
            let mid_y = (min.y + max.y) * 0.5;
            if center.y >= mid_y {
                let candidate = max.y + half_height;
                target_y = Some(
                    target_y.map(|y| y.max(candidate)).unwrap_or(candidate)
                );
            }
        }

        // 2. Ground-flush raycast — cast from center straight down.
        //    Only used when the pop-on-top path didn't fire.
        if target_y.is_none() {
            let Ok(down) = Dir3::new(Vec3::NEG_Y) else { continue };
            let hits = ctx.spatial.ray_hits(
                center,
                down,
                10_000.0,
                16, // enough to skip any colliders belonging to the
                    // selected entity itself before landing on a real
                    // support surface below it
                true,
                &avian3d::prelude::SpatialQueryFilter::default(),
            );
            // First hit that isn't the selected entity or a child of
            // it. `avian`'s ray_hits doesn't filter by entity directly
            // without a filter-mask setup; a simple identity check is
            // cheap at this scale.
            for hit in hits {
                if hit.entity == entity { continue; }
                let hit_y = center.y - hit.distance;
                target_y = Some(hit_y + half_height);
                break;
            }
        }

        // 3. Apply the resolved target, or plain nudge as fallback.
        //    Translate the world-space target into a local-Y delta so
        //    parented entities (Model children) resolve correctly.
        if let Ok((_, mut tf, gt, _)) = ctx.selected.get_mut(entity) {
            let cur_world_y = gt.translation().y;
            if let Some(new_y) = target_y {
                let delta = new_y - cur_world_y;
                tf.translation.y += delta;
            } else {
                tf.translation.y -= snap;
            }
        }
    }
}
