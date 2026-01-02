//! # Input Plugin (Client)
//! 
//! Registers InputService and handles input action mapping.

use bevy::prelude::*;
use eustress_common::services::input::*;

#[allow(dead_code)]
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<InputService>()
            .register_type::<InputService>()
            .insert_resource(default_input_actions())
            .register_type::<InputActionMap>()
            
            // Systems
            .add_systems(Update, (
                update_mouse_state,
                update_input_actions,
            ));
    }
}

/// Update mouse position in InputService
#[allow(dead_code)]
fn update_mouse_state(
    mut input_service: ResMut<InputService>,
    windows: Query<&Window>,
) {
    if let Ok(window) = windows.single() {
        if let Some(pos) = window.cursor_position() {
            let delta = pos - input_service.mouse_position;
            input_service.mouse_position = pos;
            input_service.mouse_delta = delta;
        }
    }
}

/// Update input action states from Bevy input
#[allow(dead_code)]
fn update_input_actions(
    mut action_map: ResMut<InputActionMap>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<bevy::input::mouse::MouseButton>>,
) {
    for action in action_map.actions.iter_mut() {
        let was_pressed = action.pressed;
        
        // Check primary binding
        action.pressed = match &action.primary {
            Some(InputBinding::Key(key)) => keyboard.pressed(*key),
            Some(InputBinding::Mouse(btn)) => {
                let bevy_btn = match btn {
                    eustress_common::services::input::MouseButton::Left => bevy::input::mouse::MouseButton::Left,
                    eustress_common::services::input::MouseButton::Right => bevy::input::mouse::MouseButton::Right,
                    eustress_common::services::input::MouseButton::Middle => bevy::input::mouse::MouseButton::Middle,
                    _ => bevy::input::mouse::MouseButton::Left,
                };
                mouse.pressed(bevy_btn)
            }
            _ => false,
        };
        
        // Check secondary binding if primary not pressed
        if !action.pressed {
            action.pressed = match &action.secondary {
                Some(InputBinding::Key(key)) => keyboard.pressed(*key),
                Some(InputBinding::Mouse(btn)) => {
                    let bevy_btn = match btn {
                        eustress_common::services::input::MouseButton::Left => bevy::input::mouse::MouseButton::Left,
                        eustress_common::services::input::MouseButton::Right => bevy::input::mouse::MouseButton::Right,
                        eustress_common::services::input::MouseButton::Middle => bevy::input::mouse::MouseButton::Middle,
                        _ => bevy::input::mouse::MouseButton::Left,
                    };
                    mouse.pressed(bevy_btn)
                }
                _ => false,
            };
        }
        
        action.just_pressed = action.pressed && !was_pressed;
        action.just_released = !action.pressed && was_pressed;
        action.value = if action.pressed { 1.0 } else { 0.0 };
    }
}
