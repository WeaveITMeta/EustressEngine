use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::space::SpaceFileRegistry;

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
    
    // Nudge (+ / -)
    NudgeUp,        // Move selection up by one grid unit (= / + key)
    NudgeDown,      // Move selection down by one grid unit (- key)

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
            Action::Cut => "Cut",
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
        bindings.insert(Action::Cut, KeyBinding::new(KeyCode::KeyX).with_ctrl());
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
        bindings.insert(Action::NudgeUp, KeyBinding::new(KeyCode::Equal));      // + (=/+ key)
        bindings.insert(Action::NudgeDown, KeyBinding::new(KeyCode::Minus));    // - key
        
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
                dispatch_keyboard_shortcuts,
                handle_menu_action_events.after(dispatch_keyboard_shortcuts),
                handle_nudge_keys,
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
        Action::SelectAll, Action::Group, Action::Ungroup,
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
                } else {
                    // No selection or ID mismatch — frame entire scene
                    frame_events.write(crate::camera_controller::FrameSelectionEvent {
                        target_bounds: None,
                    });
                    info!("📷 Focus on scene (no selection)");
                }
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
                        // Move TOML to .eustress/trash/ (recoverable via undo)
                        if let Ok(inst_file) = instance_file_query.get(entity) {
                            let toml_path = inst_file.toml_path.clone();
                            if toml_path.exists() {
                                let trash_dir = toml_path.parent()
                                    .and_then(|p| p.parent())
                                    .unwrap_or(toml_path.parent().unwrap_or(std::path::Path::new(".")))
                                    .join(".eustress").join("trash");
                                let _ = std::fs::create_dir_all(&trash_dir);
                                let trash_path = trash_dir.join(toml_path.file_name().unwrap_or_default());
                                match std::fs::rename(&toml_path, &trash_path) {
                                    Ok(_) => {
                                        trashed_paths.push((toml_path.clone(), trash_path));
                                        info!("🗑️ Moved {:?} to trash", toml_path.file_name().unwrap_or_default());
                                    }
                                    Err(_) => {
                                        let _ = std::fs::remove_file(&toml_path);
                                        info!("🗑️ Deleted {:?}", toml_path.file_name().unwrap_or_default());
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
            Action::SelectAll => {
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

            // Other actions are consumed by their respective systems
            _ => {}
        }
    }
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

/// Initial delay before auto-repeat starts (seconds)
const NUDGE_DELAY_SECS: f32 = 2.0;
/// Repeat interval once auto-repeat is active (seconds)
const NUDGE_REPEAT_SECS: f32 = 1.0;

fn handle_nudge_keys(
    mut key_events: EventReader<bevy::input::keyboard::KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut timer: ResMut<NudgeTimer>,
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    mut selected: Query<&mut Transform, With<crate::selection_box::Selected>>,
) {
    // Block when text input focused or overlay has focus
    if ui_focus.as_ref().map(|f| f.text_input_focused).unwrap_or(false) { return; }
    if crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) { return; }

    let snap = settings.as_ref().map(|s| if s.snap_enabled { s.snap_size } else { 1.0 }).unwrap_or(1.0);

    // Consume raw key events to detect true initial press vs OS repeat
    for event in key_events.read() {
        if event.state != bevy::input::ButtonState::Pressed { continue; }
        match event.key_code {
            KeyCode::Equal => {
                if !timer.up_held {
                    // True initial press — nudge once
                    timer.up_held = true;
                    timer.up_timer = 0.0;
                    for mut t in selected.iter_mut() {
                        t.translation.y += snap;
                    }
                }
                // Ignore OS key repeats (up_held is already true)
            }
            KeyCode::Minus => {
                if !timer.down_held {
                    timer.down_held = true;
                    timer.down_timer = 0.0;
                    for mut t in selected.iter_mut() {
                        t.translation.y -= snap;
                    }
                }
            }
            _ => {}
        }
    }

    // Auto-repeat while physically held (using ButtonInput, not events)
    if keys.pressed(KeyCode::Equal) && timer.up_held {
        timer.up_timer += time.delta_secs();
        if timer.up_timer >= NUDGE_DELAY_SECS {
            timer.up_timer -= NUDGE_REPEAT_SECS;
            for mut t in selected.iter_mut() {
                t.translation.y += snap;
            }
        }
    }
    if !keys.pressed(KeyCode::Equal) {
        timer.up_held = false;
        timer.up_timer = 0.0;
    }

    if keys.pressed(KeyCode::Minus) && timer.down_held {
        timer.down_timer += time.delta_secs();
        if timer.down_timer >= NUDGE_DELAY_SECS {
            timer.down_timer -= NUDGE_REPEAT_SECS;
            for mut t in selected.iter_mut() {
                t.translation.y -= snap;
            }
        }
    }
    if !keys.pressed(KeyCode::Minus) {
        timer.down_held = false;
        timer.down_timer = 0.0;
    }
}
