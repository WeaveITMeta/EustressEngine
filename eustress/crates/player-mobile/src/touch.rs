// =============================================================================
// Eustress Player Mobile - Touch Input System
// =============================================================================
// Table of Contents:
// 1. Plugin Definition
// 2. Touch State
// 3. Input Actions
// 4. Touch Processing
// =============================================================================

use bevy::prelude::*;
use bevy::input::touch::{TouchInput, TouchPhase};

// -----------------------------------------------------------------------------
// 1. Plugin Definition
// -----------------------------------------------------------------------------

/// Plugin for handling touch input on mobile devices.
pub struct TouchInputPlugin;

impl Plugin for TouchInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TouchState>()
            .init_resource::<VirtualJoystickState>()
            .add_event::<InputAction>()
            .add_systems(Update, (
                process_touch_input,
                update_virtual_joystick,
            ));
    }
}

// -----------------------------------------------------------------------------
// 2. Touch State
// -----------------------------------------------------------------------------

/// Tracks active touches and their purposes.
#[derive(Resource, Default)]
pub struct TouchState {
    /// Touch used for movement (left side of screen)
    pub move_touch: Option<u64>,
    /// Touch used for looking (right side of screen)
    pub look_touch: Option<u64>,
    /// Touch positions
    pub positions: std::collections::HashMap<u64, Vec2>,
    /// Touch start positions
    pub start_positions: std::collections::HashMap<u64, Vec2>,
}

/// Virtual joystick state for movement.
#[derive(Resource, Default)]
pub struct VirtualJoystickState {
    /// Current joystick offset from center (-1 to 1)
    pub offset: Vec2,
    /// Whether joystick is active
    pub active: bool,
    /// Center position of joystick
    pub center: Vec2,
}

// -----------------------------------------------------------------------------
// 3. Input Actions
// -----------------------------------------------------------------------------

/// Abstract input actions that work across all platforms.
#[derive(Event, Clone, Debug)]
pub enum InputAction {
    /// Movement direction (normalized)
    Move(Vec2),
    /// Look/camera rotation delta
    Look(Vec2),
    /// Jump action
    Jump,
    /// Interact with object
    Interact,
    /// Open menu
    Menu,
    /// Primary action (tap)
    Primary,
    /// Secondary action (hold)
    Secondary,
}

// -----------------------------------------------------------------------------
// 4. Touch Processing
// -----------------------------------------------------------------------------

/// Process raw touch input into game actions.
fn process_touch_input(
    mut touch_events: EventReader<TouchInput>,
    mut touch_state: ResMut<TouchState>,
    mut actions: EventWriter<InputAction>,
    windows: Query<&Window>,
) {
    let Ok(window) = windows.get_single() else { return };
    let screen_width = window.width();
    let screen_center_x = screen_width / 2.0;
    
    for event in touch_events.read() {
        match event.phase {
            TouchPhase::Started => {
                let pos = event.position;
                touch_state.positions.insert(event.id, pos);
                touch_state.start_positions.insert(event.id, pos);
                
                // Left side = movement, Right side = look
                if pos.x < screen_center_x {
                    if touch_state.move_touch.is_none() {
                        touch_state.move_touch = Some(event.id);
                    }
                } else {
                    if touch_state.look_touch.is_none() {
                        touch_state.look_touch = Some(event.id);
                    }
                }
            }
            TouchPhase::Moved => {
                if let Some(old_pos) = touch_state.positions.get(&event.id) {
                    let delta = event.position - *old_pos;
                    
                    // Look touch sends look actions
                    if touch_state.look_touch == Some(event.id) {
                        actions.send(InputAction::Look(delta * 0.1));
                    }
                }
                touch_state.positions.insert(event.id, event.position);
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                // Check for tap (short touch without much movement)
                if let Some(start_pos) = touch_state.start_positions.get(&event.id) {
                    let distance = event.position.distance(*start_pos);
                    if distance < 20.0 {
                        // Tap detected
                        if event.position.x >= screen_center_x {
                            actions.send(InputAction::Primary);
                        }
                    }
                }
                
                // Clear touch
                touch_state.positions.remove(&event.id);
                touch_state.start_positions.remove(&event.id);
                
                if touch_state.move_touch == Some(event.id) {
                    touch_state.move_touch = None;
                }
                if touch_state.look_touch == Some(event.id) {
                    touch_state.look_touch = None;
                }
            }
        }
    }
}

/// Update virtual joystick based on move touch.
fn update_virtual_joystick(
    touch_state: Res<TouchState>,
    mut joystick: ResMut<VirtualJoystickState>,
    mut actions: EventWriter<InputAction>,
) {
    if let Some(touch_id) = touch_state.move_touch {
        if let (Some(pos), Some(start)) = (
            touch_state.positions.get(&touch_id),
            touch_state.start_positions.get(&touch_id),
        ) {
            joystick.active = true;
            joystick.center = *start;
            
            // Calculate offset (clamped to max radius)
            let max_radius = 75.0;
            let delta = *pos - *start;
            let clamped = if delta.length() > max_radius {
                delta.normalize() * max_radius
            } else {
                delta
            };
            
            joystick.offset = clamped / max_radius;
            
            // Send move action if significant
            if joystick.offset.length() > 0.1 {
                actions.send(InputAction::Move(joystick.offset));
            }
        }
    } else {
        joystick.active = false;
        joystick.offset = Vec2::ZERO;
    }
}
