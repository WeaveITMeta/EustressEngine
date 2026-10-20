//! # Physics Plugin (Client)
//! 
//! Registers PhysicsService. Actual physics is handled by Avian3D.

use bevy::prelude::*;
use eustress_common::services::physics::*;

#[allow(dead_code)]
pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resource
            .init_resource::<PhysicsService>()
            .register_type::<PhysicsService>()
            
            // Components
            .register_type::<CollisionGroup>()
            .register_type::<PhysicsMaterial>()
            .register_type::<Constraint>()
            .register_type::<BodyVelocity>()
            .register_type::<BodyForce>();
        // No gravity system here. Gravity is owned by `Workspace.gravity` and
        // written to Avian only by `sync_workspace_gravity_to_avian`. This
        // plugin used to copy a separate PhysicsService gravity into Avian,
        // a second writer for the same resource.
    }
}
