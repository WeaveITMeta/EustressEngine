//! # Physics Plugin
//! 
//! Registers PhysicsService and constraint classes.

use bevy::prelude::*;
use eustress_common::classes::*;
use eustress_common::services::physics::*;

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resource
            .init_resource::<PhysicsService>()
            .register_type::<PhysicsService>()
            
            // Constraints
            .register_type::<Attachment>()
            .register_type::<WeldConstraint>()
            .register_type::<Motor6D>()
            
            // Physics components
            .register_type::<CollisionGroup>()
            .register_type::<PhysicsMaterial>()
            .register_type::<Constraint>()
            .register_type::<BodyVelocity>()
            .register_type::<BodyForce>();
    }
}
