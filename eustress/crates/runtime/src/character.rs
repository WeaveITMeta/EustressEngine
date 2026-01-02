//! # Character Controller
//!
//! Reads movement properties from `Humanoid` component and applies physics.
//! Integrates with `PlayerService` for per-player speed multipliers.
//!
//! ## Design
//!
//! - **Property-driven**: All speeds come from `Humanoid`, not constants
//! - **Service-aware**: Applies `PlayerService` multipliers for boosts/debuffs
//! - **Network-ready**: Outputs to physics; networking syncs the result

use bevy::prelude::*;
use eustress_common::classes::Humanoid;
use eustress_common::services::player::{PlayerService, Character, CharacterRoot};

// ============================================================================
// Character State
// ============================================================================

/// Runtime state for a character controller.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct CharacterController {
    /// Current movement input (normalized direction)
    pub input_direction: Vec3,
    
    /// Is the character sprinting?
    pub sprinting: bool,
    
    /// Is the character jumping this frame?
    pub jump_requested: bool,
    
    /// Is the character on the ground?
    pub grounded: bool,
    
    /// Current velocity (output)
    pub velocity: Vec3,
    
    /// Network client ID (for multiplier lookup)
    pub client_id: Option<u64>,
}

/// Character movement state for animations/effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum CharacterState {
    #[default]
    Idle,
    Walking,
    Running,
    Jumping,
    Falling,
    Dead,
}

// ============================================================================
// Character Plugin
// ============================================================================

/// Plugin for character movement.
pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<CharacterController>()
            .register_type::<CharacterState>()
            .add_systems(Update, (
                update_character_movement,
                update_character_state,
            ).chain());
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Update character movement based on Humanoid properties.
///
/// Reads:
/// - `Humanoid.walk_speed`, `run_speed`, `jump_power`
/// - `PlayerService` speed multipliers
/// - `CharacterController.input_direction`, `sprinting`
///
/// Writes:
/// - `CharacterController.velocity`
fn update_character_movement(
    player_service: Option<Res<PlayerService>>,
    mut query: Query<(&Humanoid, &mut CharacterController)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    
    for (humanoid, mut controller) in query.iter_mut() {
        // Skip if movement disabled
        if !humanoid.can_move {
            controller.velocity = Vec3::ZERO;
            continue;
        }
        
        // Get base speed from Humanoid
        let base_speed = humanoid.effective_speed(controller.sprinting);
        
        // Apply per-player multiplier if available
        let speed = if let (Some(service), Some(client_id)) = (player_service.as_ref(), controller.client_id) {
            base_speed * service.get_speed_multiplier(client_id)
        } else {
            base_speed
        };
        
        // Calculate horizontal velocity
        let horizontal = controller.input_direction.normalize_or_zero() * speed;
        
        // Handle jumping
        let vertical = if controller.jump_requested && controller.grounded && humanoid.can_jump {
            controller.jump_requested = false;
            humanoid.jump_power
        } else if !controller.grounded {
            // Preserve existing vertical velocity (gravity applied elsewhere)
            controller.velocity.y
        } else {
            0.0
        };
        
        // Combine into final velocity
        controller.velocity = Vec3::new(horizontal.x, vertical, horizontal.z);
    }
}

/// Update character state for animations.
fn update_character_state(
    query: Query<(&CharacterController, &Humanoid), Changed<CharacterController>>,
) {
    for (controller, humanoid) in query.iter() {
        let state = if !humanoid.is_alive() {
            CharacterState::Dead
        } else if !controller.grounded && controller.velocity.y > 0.0 {
            CharacterState::Jumping
        } else if !controller.grounded {
            CharacterState::Falling
        } else if controller.velocity.length() > 0.1 {
            if controller.sprinting {
                CharacterState::Running
            } else {
                CharacterState::Walking
            }
        } else {
            CharacterState::Idle
        };
        
        // Could emit events or update a CharacterState component here
        let _ = state; // Suppress unused warning for now
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Spawn a character with controller.
pub fn spawn_character(
    commands: &mut Commands,
    humanoid: Humanoid,
    position: Vec3,
    client_id: Option<u64>,
) -> Entity {
    commands.spawn((
        humanoid,
        CharacterController {
            client_id,
            ..default()
        },
        CharacterRoot,
        Transform::from_translation(position),
    )).id()
}

/// Apply input to a character controller.
pub fn apply_input(
    controller: &mut CharacterController,
    direction: Vec3,
    sprinting: bool,
    jump: bool,
) {
    controller.input_direction = direction;
    controller.sprinting = sprinting;
    if jump {
        controller.jump_requested = true;
    }
}
