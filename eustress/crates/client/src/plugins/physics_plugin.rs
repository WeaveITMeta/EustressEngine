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
            .register_type::<BodyForce>()
            
            // Systems
            .add_systems(Update, sync_physics_settings);
    }
}

/// Sync PhysicsService settings with Avian3D (if needed)
#[allow(dead_code)]
fn sync_physics_settings(
    physics: Res<PhysicsService>,
    mut gravity: ResMut<avian3d::prelude::Gravity>,
) {
    if physics.is_changed() {
        *gravity = avian3d::prelude::Gravity(physics.gravity);
    }
}
