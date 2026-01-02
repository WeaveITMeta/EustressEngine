//! # Eustress Runtime
//!
//! Shared game runtime for Eustress Engine.
//! Used by both Studio (engine) and Player (client) for consistent game logic.
//!
//! ## Modules
//!
//! - [`character`]: Character controller reading from `Humanoid` properties
//! - [`replication`]: Sync between `BasePart` and network components
//! - [`physics`]: Physics integration with Avian, reading from `Workspace`
//! - [`ownership`]: Ownership resolution from scene rules
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      Eustress Runtime                           │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Character Controller                                           │
//! │  ├── Reads Humanoid.walk_speed, run_speed, jump_power           │
//! │  ├── Applies PlayerService speed multipliers                    │
//! │  └── Outputs to physics velocity                                │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Replication Sync                                               │
//! │  ├── BasePart.cframe <-> NetworkTransform                       │
//! │  ├── BasePart.assembly_linear_velocity <-> NetworkVelocity      │
//! │  └── Bidirectional based on ownership                           │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Physics Integration                                            │
//! │  ├── Reads Workspace.gravity, max_entity_speed                  │
//! │  ├── Applies to Avian RigidBodies                               │
//! │  └── Validates against anti-exploit bounds                      │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Ownership Resolution                                           │
//! │  ├── Reads NetworkOwnershipRule from scene                      │
//! │  ├── Resolves to NetworkOwner at runtime                        │
//! │  └── Handles inheritance and spawn ownership                    │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

pub mod character;
pub mod ownership;

#[cfg(feature = "networking")]
pub mod replication;

#[cfg(feature = "physics")]
pub mod physics;

use bevy::prelude::*;

// ============================================================================
// Runtime Plugin
// ============================================================================

/// Main runtime plugin - add to both Studio and Client apps.
///
/// # Example
/// ```rust,ignore
/// use bevy::prelude::*;
/// use eustress_runtime::EustressRuntimePlugin;
///
/// fn main() {
///     App::new()
///         .add_plugins(DefaultPlugins)
///         .add_plugins(EustressRuntimePlugin)
///         .run();
/// }
/// ```
pub struct EustressRuntimePlugin;

impl Plugin for EustressRuntimePlugin {
    fn build(&self, app: &mut App) {
        // Character controller
        app.add_plugins(character::CharacterPlugin);
        
        // Ownership resolution
        app.add_plugins(ownership::OwnershipPlugin);
        
        // Replication sync (if networking enabled)
        #[cfg(feature = "networking")]
        app.add_plugins(replication::ReplicationSyncPlugin);
        
        // Physics integration (if physics enabled)
        #[cfg(feature = "physics")]
        app.add_plugins(physics::RuntimePhysicsPlugin);
        
        info!("Eustress Runtime initialized");
    }
}

// ============================================================================
// Prelude
// ============================================================================

/// Convenient re-exports for common runtime types.
pub mod prelude {
    pub use super::EustressRuntimePlugin;
    pub use super::character::{CharacterPlugin, CharacterController, CharacterState};
    pub use super::ownership::{OwnershipPlugin, resolve_ownership};
    
    #[cfg(feature = "networking")]
    pub use super::replication::{ReplicationSyncPlugin, sync_basepart_to_network};
    
    #[cfg(feature = "physics")]
    pub use super::physics::{RuntimePhysicsPlugin, apply_workspace_gravity};
}
