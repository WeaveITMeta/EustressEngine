// =============================================================================
// Eustress Player Mobile - Mobile UI Components
// =============================================================================
// Table of Contents:
// 1. Plugin Definition
// 2. UI Components
// 3. Virtual Joystick
// 4. Action Buttons
// =============================================================================

use bevy::prelude::*;
use crate::touch::VirtualJoystickState;

// -----------------------------------------------------------------------------
// 1. Plugin Definition
// -----------------------------------------------------------------------------

/// Plugin for mobile-specific UI elements.
pub struct MobileUiPlugin;

impl Plugin for MobileUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_mobile_ui)
            .add_systems(Update, (
                update_joystick_visual,
                handle_button_interactions,
            ));
    }
}

// -----------------------------------------------------------------------------
// 2. UI Components
// -----------------------------------------------------------------------------

/// Marker for the virtual joystick base.
#[derive(Component)]
pub struct JoystickBase;

/// Marker for the virtual joystick knob.
#[derive(Component)]
pub struct JoystickKnob;

/// Marker for action buttons.
#[derive(Component)]
pub struct ActionButton {
    pub action: ButtonAction,
}

/// Button action types.
#[derive(Clone, Copy, Debug)]
pub enum ButtonAction {
    Jump,
    Interact,
    Menu,
    Inventory,
}

// -----------------------------------------------------------------------------
// 3. Setup Mobile UI
// -----------------------------------------------------------------------------

/// Create mobile UI elements.
fn setup_mobile_ui(mut commands: Commands) {
    // Root UI node
    commands.spawn(Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    }).with_children(|parent| {
        // Virtual Joystick (bottom-left)
        spawn_virtual_joystick(parent);
        
        // Action Buttons (bottom-right)
        spawn_action_buttons(parent);
        
        // Menu Button (top-right)
        spawn_menu_button(parent);
    });
}

/// Spawn virtual joystick UI.
fn spawn_virtual_joystick(parent: &mut ChildBuilder) {
    // Joystick container
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(30.0),
            bottom: Val::Px(30.0),
            width: Val::Px(150.0),
            height: Val::Px(150.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)),
        BorderRadius::all(Val::Px(75.0)),
        JoystickBase,
    )).with_children(|joystick| {
        // Joystick knob
        joystick.spawn((
            Node {
                width: Val::Px(60.0),
                height: Val::Px(60.0),
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.4)),
            BorderRadius::all(Val::Px(30.0)),
            JoystickKnob,
        ));
    });
}

/// Spawn action buttons.
fn spawn_action_buttons(parent: &mut ChildBuilder) {
    // Button container (bottom-right)
    parent.spawn(Node {
        position_type: PositionType::Absolute,
        right: Val::Px(30.0),
        bottom: Val::Px(30.0),
        width: Val::Px(180.0),
        height: Val::Px(180.0),
        flex_direction: FlexDirection::Column,
        justify_content: JustifyContent::SpaceBetween,
        ..default()
    }).with_children(|container| {
        // Top row (Interact)
        container.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        }).with_children(|row| {
            spawn_action_button(row, ButtonAction::Interact, "E");
        });
        
        // Bottom row (Jump)
        container.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        }).with_children(|row| {
            spawn_action_button(row, ButtonAction::Jump, "⬆");
        });
    });
}

/// Spawn a single action button.
fn spawn_action_button(parent: &mut ChildBuilder, action: ButtonAction, label: &str) {
    parent.spawn((
        Button,
        Node {
            width: Val::Px(70.0),
            height: Val::Px(70.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.2)),
        BorderRadius::all(Val::Px(35.0)),
        ActionButton { action },
    )).with_children(|btn| {
        btn.spawn((
            Text::new(label),
            TextFont::from_font_size(28.0),
            TextColor(Color::WHITE),
        ));
    });
}

/// Spawn menu button (top-right).
fn spawn_menu_button(parent: &mut ChildBuilder) {
    parent.spawn((
        Button,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(20.0),
            top: Val::Px(20.0),
            width: Val::Px(50.0),
            height: Val::Px(50.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        BorderRadius::all(Val::Px(8.0)),
        ActionButton { action: ButtonAction::Menu },
    )).with_children(|btn| {
        btn.spawn((
            Text::new("☰"),
            TextFont::from_font_size(24.0),
            TextColor(Color::WHITE),
        ));
    });
}

// -----------------------------------------------------------------------------
// 4. Update Systems
// -----------------------------------------------------------------------------

/// Update joystick knob position based on touch input.
fn update_joystick_visual(
    joystick_state: Res<VirtualJoystickState>,
    mut knob_query: Query<&mut Style, With<JoystickKnob>>,
) {
    for mut style in knob_query.iter_mut() {
        if joystick_state.active {
            // Move knob based on offset (max 45px from center)
            let offset = joystick_state.offset * 45.0;
            style.left = Val::Px(45.0 + offset.x - 30.0);
            style.top = Val::Px(45.0 - offset.y - 30.0);
        } else {
            // Center the knob
            style.left = Val::Auto;
            style.top = Val::Auto;
        }
    }
}

/// Handle button press interactions.
fn handle_button_interactions(
    interaction_query: Query<(&Interaction, &ActionButton), Changed<Interaction>>,
    mut actions: EventWriter<crate::touch::InputAction>,
) {
    for (interaction, button) in interaction_query.iter() {
        if *interaction == Interaction::Pressed {
            match button.action {
                ButtonAction::Jump => {
                    actions.send(crate::touch::InputAction::Jump);
                }
                ButtonAction::Interact => {
                    actions.send(crate::touch::InputAction::Interact);
                }
                ButtonAction::Menu => {
                    actions.send(crate::touch::InputAction::Menu);
                }
                ButtonAction::Inventory => {
                    // Handle inventory
                }
            }
        }
    }
}
