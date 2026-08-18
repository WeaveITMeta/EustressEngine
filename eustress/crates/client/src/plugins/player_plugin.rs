//! PlayerService Plugin - Client-side player systems
//! 
//! Uses shared types from eustress_common::services::player
//! Implements client-specific systems:
//! - Character spawning with physics (Avian3D)
//! - Procedural animation (walk, run, jump)
//! - Camera following with smooth interpolation
//! - Input handling (WASD + mouse)
//!
//! ## Animation System
//! 
//! Inspired by AAA games like Uncharted 4 and GTA V:
//! - Procedural limb animation based on velocity
//! - State machine for animation transitions
//! - Hip bob and body lean during movement
//! - Smooth blending between states

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::anti_alias::smaa::Smaa;
use bevy::post_process::bloom::Bloom;
use bevy::render::view::Msaa;
use avian3d::prelude::*;


// Import shared types from common
#[allow(unused_imports)]
pub use eustress_common::services::player::{
    Player, Character, CharacterRoot, CharacterHead,
    PlayerService, PlayerCamera, CameraMode,
    find_spawn_position, get_spawn_position_or_default,
    find_spawn_position_by_team_id, get_spawn_position_by_team_id_or_default,
};
pub use eustress_common::classes::SpawnLocation;
pub use eustress_common::services::{TeamService, TeamMember, TeamColor};
#[allow(unused_imports)]
pub use eustress_common::services::animation::{
    AnimationService, AnimationStateMachine, AnimationState,
    LocomotionController, ProceduralAnimation, FootIK,
    CharacterAnimationBundle,
};

// Re-export character controller components
#[allow(unused_imports)]
pub use super::character_controller::{
    CharacterPhysics, MovementIntent, CharacterBody, CharacterLimb, CharacterFacing,
};

// Import shared character markers from common (for 1:1 parity with Play Mode)
pub use eustress_common::plugins::character_plugin::{
    PlayModeCharacter, PlayModeCamera,
};

// Import skinned character system
use eustress_common::plugins::skinned_character::{
    spawn_skinned_character, CharacterModel, CharacterGender,
    CharacterAnimationPaths, SkinnedCharacter,
};
use eustress_common::services::player::{BiologicalSex, PlayerProfile};

// ============================================================================
// Character Type Configuration
// ============================================================================


// ============================================================================
// Plugin
// ============================================================================

pub struct PlayerServicePlugin;

impl Plugin for PlayerServicePlugin {
    fn build(&self, app: &mut App) {
        // The character itself — spawning, physics, input, camera, facing —
        // now belongs to `eustress_common::avatar::AvatarRuntimePlugin`, which
        // both shells add. Nothing character-related may be registered here.
        //
        // What was removed and why:
        //  * `SharedCharacterPlugin` — shared systems by convention while
        //    leaving entity construction free, which is how Studio came to
        //    spawn a Female/XBot and the Client a Male/YBot from "the same"
        //    code. It is now `pub(crate)` inside the runtime.
        //  * `spawn_local_player` — a second, structurally different character
        //    (its own capsule dimensions, its own camera with a different FOV
        //    and tonemapper). Replaced by the `SpawnAvatar` message.
        //  * `CharacterSystemConfig` — the `use_skinned_characters` flag whose
        //    two branches produced entirely different entities on the two
        //    sides.
        //  * `update_first_person_mode` — keyed on `CharacterBody`, which no
        //    live path ever inserted. Dead. First-person body hiding is a
        //    P5 item on the bound rig.
        //
        // `PlayerService` DOES stay here. It is a genuine service (spawn
        // position, cursor state, local-player handle) and is independent of
        // how the character is implemented — but it was only ever
        // `init_resource`'d as a side effect of `SharedCharacterPlugin`.
        // Removing that plugin therefore took `PlayerService` with it, and
        // `PauseMenuPlugin`'s three systems failed param validation and
        // panicked the schedule at startup.
        app.init_resource::<PlayerService>().init_resource::<AnimationService>();
    }
}

// ============================================================================
// Startup Systems
// ============================================================================





// ============================================================================
// Update Systems (LEGACY - now provided by SharedCharacterPlugin)
// These are kept for reference but marked dead_code since SharedCharacterPlugin
// provides identical systems for 1:1 parity with Play Mode.
// ============================================================================
















