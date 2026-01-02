//! # Input Service
//! 
//! Input handling and action mapping (like Eustress's UserInputService).
//! 
//! ## Classes
//! - `InputService`: Global input state
//! - `InputAction`: Mapped input action
//! - `InputBinding`: Key/button binding

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// InputService Resource
// ============================================================================

/// InputService - global input state and settings
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct InputService {
    /// Is keyboard enabled
    pub keyboard_enabled: bool,
    /// Is mouse enabled
    pub mouse_enabled: bool,
    /// Is gamepad enabled
    pub gamepad_enabled: bool,
    /// Is touch enabled
    pub touch_enabled: bool,
    /// Mouse sensitivity
    pub mouse_sensitivity: f32,
    /// Gamepad deadzone
    pub gamepad_deadzone: f32,
    /// Is mouse locked to window
    pub mouse_locked: bool,
    /// Current mouse position (screen coords)
    pub mouse_position: Vec2,
    /// Mouse delta this frame
    pub mouse_delta: Vec2,
    /// Is any modal UI open (blocks game input)
    pub modal_open: bool,
}

impl Default for InputService {
    fn default() -> Self {
        Self {
            keyboard_enabled: true,
            mouse_enabled: true,
            gamepad_enabled: true,
            touch_enabled: false,
            mouse_sensitivity: 1.0,
            gamepad_deadzone: 0.1,
            mouse_locked: false,
            mouse_position: Vec2::ZERO,
            mouse_delta: Vec2::ZERO,
            modal_open: false,
        }
    }
}

// ============================================================================
// Input Actions
// ============================================================================

/// InputAction - a named input action with bindings
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct InputAction {
    /// Action name
    pub name: String,
    /// Primary binding
    pub primary: Option<InputBinding>,
    /// Secondary binding
    pub secondary: Option<InputBinding>,
    /// Is action currently pressed
    #[serde(skip)]
    pub pressed: bool,
    /// Was action just pressed this frame
    #[serde(skip)]
    pub just_pressed: bool,
    /// Was action just released this frame
    #[serde(skip)]
    pub just_released: bool,
    /// Axis value (-1 to 1 for analog)
    #[serde(skip)]
    pub value: f32,
}

impl InputAction {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            primary: None,
            secondary: None,
            pressed: false,
            just_pressed: false,
            just_released: false,
            value: 0.0,
        }
    }
    
    pub fn with_key(mut self, key: KeyCode) -> Self {
        self.primary = Some(InputBinding::Key(key));
        self
    }
    
    pub fn with_mouse(mut self, button: MouseButton) -> Self {
        self.primary = Some(InputBinding::Mouse(button));
        self
    }
}

/// Input binding types
#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum InputBinding {
    Key(KeyCode),
    Mouse(MouseButton),
    GamepadButton(GamepadButtonType),
    GamepadAxis(GamepadAxisType),
}

/// Mouse button enum (simplified)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

/// Gamepad button types
#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum GamepadButtonType {
    South,      // A / Cross
    East,       // B / Circle
    West,       // X / Square
    North,      // Y / Triangle
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    Select,
    Start,
    LeftStick,
    RightStick,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

/// Gamepad axis types
#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum GamepadAxisType {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftTrigger,
    RightTrigger,
}

// ============================================================================
// Input Action Map Resource
// ============================================================================

/// InputActionMap - collection of input actions
#[derive(Resource, Reflect, Clone, Debug, Default)]
#[reflect(Resource)]
pub struct InputActionMap {
    /// All registered actions
    pub actions: Vec<InputAction>,
}

impl InputActionMap {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn add(&mut self, action: InputAction) -> &mut Self {
        self.actions.push(action);
        self
    }
    
    pub fn get(&self, name: &str) -> Option<&InputAction> {
        self.actions.iter().find(|a| a.name == name)
    }
    
    pub fn get_mut(&mut self, name: &str) -> Option<&mut InputAction> {
        self.actions.iter_mut().find(|a| a.name == name)
    }
    
    pub fn is_pressed(&self, name: &str) -> bool {
        self.get(name).map(|a| a.pressed).unwrap_or(false)
    }
    
    pub fn just_pressed(&self, name: &str) -> bool {
        self.get(name).map(|a| a.just_pressed).unwrap_or(false)
    }
}

// ============================================================================
// Default Actions
// ============================================================================

/// Create default game input actions
pub fn default_input_actions() -> InputActionMap {
    let mut map = InputActionMap::new();
    
    map.add(InputAction::new("MoveForward").with_key(KeyCode::KeyW));
    map.add(InputAction::new("MoveBack").with_key(KeyCode::KeyS));
    map.add(InputAction::new("MoveLeft").with_key(KeyCode::KeyA));
    map.add(InputAction::new("MoveRight").with_key(KeyCode::KeyD));
    map.add(InputAction::new("Jump").with_key(KeyCode::Space));
    map.add(InputAction::new("Sprint").with_key(KeyCode::ShiftLeft));
    map.add(InputAction::new("Crouch").with_key(KeyCode::ControlLeft));
    map.add(InputAction::new("Interact").with_key(KeyCode::KeyE));
    map.add(InputAction::new("Attack").with_mouse(MouseButton::Left));
    map.add(InputAction::new("AltAttack").with_mouse(MouseButton::Right));
    map.add(InputAction::new("Reload").with_key(KeyCode::KeyR));
    map.add(InputAction::new("Inventory").with_key(KeyCode::Tab));
    map.add(InputAction::new("Pause").with_key(KeyCode::Escape));
    
    map
}
