//! # Physics Integration
//!
//! Integrates Avian physics with network ownership.
//!
//! ## Ownership Model
//!
//! - **Server-owned**: Server runs full physics, clients interpolate
//! - **Client-owned**: Client runs physics (prediction), server validates
//!
//! ## Sync Strategy
//!
//! - Owned entities: Client sends velocity/position, server validates
//! - Remote entities: Server sends state, client interpolates
//!
//! ## Service-Driven Configuration
//!
//! Physics bounds come from `Workspace` service (not hardcoded):
//! - `Workspace.gravity` - Applied to physics
//! - `Workspace.max_entity_speed` - Anti-exploit speed limit
//! - `Workspace.max_acceleration` - Anti-exploit acceleration limit

use bevy::prelude::*;
use avian3d::prelude::*;

use crate::config::NetworkConfig;
use crate::ownership::NetworkOwner;
use crate::protocol::{NetworkTransform, NetworkVelocity};

// Import Workspace from eustress-common for service-driven config
use eustress_common::services::workspace::Workspace;

// ============================================================================
// Components
// ============================================================================

/// Marks an entity for physics ownership sync.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct PhysicsOwnership {
    /// Last synced velocity
    pub last_velocity: Vec3,
    /// Last synced angular velocity
    pub last_angular: Vec3,
    /// Tick of last sync
    pub last_tick: u64,
}

/// Marks physics as frozen (no simulation).
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct PhysicsFrozen;

// ============================================================================
// Systems
// ============================================================================

/// Sync network ownership to physics simulation.
///
/// - Server-owned: Full physics
/// - Client-owned (local): Full physics (prediction)
/// - Client-owned (remote): Kinematic (interpolated)
#[cfg(feature = "client")]
fn sync_ownership_to_physics(
    mut commands: Commands,
    local_client: Res<crate::client::LocalClient>,
    query: Query<(
        Entity,
        &NetworkOwner,
        &RigidBody,
        Option<&PhysicsFrozen>,
    ), Changed<NetworkOwner>>,
) {
    for (entity, owner, rb, frozen) in query.iter() {
        let new_rb = if frozen.is_some() {
            RigidBody::Static
        } else if owner.is_server_owned() || owner.is_owned_by(local_client.id) {
            // We simulate this entity
            RigidBody::Dynamic
        } else {
            // Remote client owns it - we just interpolate
            RigidBody::Kinematic
        };
        
        // Only update if changed (RigidBody is now immutable, must replace)
        if *rb != new_rb {
            commands.entity(entity).insert(new_rb);
        }
    }
}

/// Server-side: All dynamic entities run physics.
#[cfg(feature = "server")]
fn sync_ownership_to_physics_server(
    mut commands: Commands,
    query: Query<(
        Entity,
        &NetworkOwner,
        &RigidBody,
        Option<&PhysicsFrozen>,
    ), Changed<NetworkOwner>>,
) {
    for (entity, _owner, rb, frozen) in query.iter() {
        let new_rb = if frozen.is_some() {
            RigidBody::Static
        } else {
            // Server always simulates for validation
            RigidBody::Dynamic
        };
        
        // Only update if changed (RigidBody is now immutable, must replace)
        if *rb != new_rb {
            commands.entity(entity).insert(new_rb);
        }
    }
}

/// Sync Avian velocity to NetworkVelocity.
fn sync_velocity_to_network(
    mut query: Query<(
        &LinearVelocity,
        &AngularVelocity,
        &mut NetworkVelocity,
    ), Changed<LinearVelocity>>,
) {
    for (lin_vel, ang_vel, mut net_vel) in query.iter_mut() {
        net_vel.linear = lin_vel.0;
        net_vel.angular = ang_vel.0;
    }
}

/// Sync NetworkVelocity to Avian velocity (for remote entities).
fn sync_network_to_velocity(
    mut query: Query<(
        &NetworkVelocity,
        &mut LinearVelocity,
        &mut AngularVelocity,
        &NetworkOwner,
    ), Changed<NetworkVelocity>>,
    #[cfg(feature = "client")]
    local_client: Res<crate::client::LocalClient>,
) {
    for (net_vel, mut lin_vel, mut ang_vel, owner) in query.iter_mut() {
        // Only apply to entities we don't own
        #[cfg(feature = "client")]
        if owner.is_owned_by(local_client.id) {
            continue;
        }

        lin_vel.0 = net_vel.linear;
        ang_vel.0 = net_vel.angular;
    }
}

/// Validate physics state (anti-exploit).
/// 
/// Reads limits from `Workspace` service (service-driven, not hardcoded).
#[cfg(feature = "server")]
fn validate_physics_state(
    workspace: Option<Res<Workspace>>,
    config: Res<NetworkConfig>,
    mut query: Query<(
        Entity,
        &NetworkOwner,
        &LinearVelocity,
        &Transform,
        &mut PhysicsOwnership,
    )>,
    time: Res<Time>,
) {
    // Get limits from Workspace service, fallback to config
    let (max_speed, max_accel) = if let Some(ws) = workspace {
        (ws.max_entity_speed, ws.max_acceleration)
    } else {
        (config.anti_exploit.max_speed, config.anti_exploit.max_acceleration)
    };
    
    let dt = time.delta_secs();

    for (entity, owner, velocity, transform, mut phys_own) in query.iter_mut() {
        if owner.is_server_owned() {
            continue; // Server controls, no validation needed
        }

        let speed = velocity.0.length();
        let accel = (velocity.0 - phys_own.last_velocity).length() / dt.max(0.001);

        // Check speed
        if speed > max_speed {
            warn!(
                "Entity {:?} speed violation: {:.1} > {:.1} studs/s",
                entity, speed, max_speed
            );
            // Could trigger rollback or kick
        }

        // Check acceleration
        if accel > max_accel {
            warn!(
                "Entity {:?} acceleration violation: {:.1} > {:.1} studs/s²",
                entity, accel, max_accel
            );
        }

        // Update tracking
        phys_own.last_velocity = velocity.0;
    }
}

/// Sync Workspace gravity to Avian's Gravity resource.
/// 
/// This allows per-scene gravity configuration from the Workspace service.
fn sync_workspace_gravity(
    workspace: Option<Res<Workspace>>,
    mut gravity: ResMut<Gravity>,
) {
    if let Some(ws) = workspace {
        // Only update if changed to avoid unnecessary work
        if gravity.0 != ws.gravity {
            gravity.0 = ws.gravity;
            info!("Gravity synced from Workspace: {:?} studs/s²", ws.gravity);
        }
    }
}

/// Freeze physics for entities marked as frozen.
fn freeze_physics(
    mut query: Query<(&mut LinearVelocity, &mut AngularVelocity), Added<PhysicsFrozen>>,
) {
    for (mut lin, mut ang) in query.iter_mut() {
        lin.0 = Vec3::ZERO;
        ang.0 = Vec3::ZERO;
    }
}

/// Unfreeze physics when marker removed.
fn unfreeze_physics(
    mut commands: Commands,
    mut removed: RemovedComponents<PhysicsFrozen>,
    query: Query<&RigidBody>,
) {
    for entity in removed.read() {
        if let Ok(rb) = query.get(entity) {
            if *rb != RigidBody::Dynamic {
                commands.entity(entity).insert(RigidBody::Dynamic);
            }
        }
    }
}

// ============================================================================
// Physics Plugin
// ============================================================================

/// Network physics integration plugin.
pub struct NetworkPhysicsPlugin;

impl Plugin for NetworkPhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<PhysicsOwnership>()
            .register_type::<PhysicsFrozen>();

        // Skip adding Avian physics - it's already added by the engine/client
        // PhysicsPlugins is added elsewhere, we just add our network-specific systems

        // Configure physics timestep to match network tick
        app.insert_resource(Time::<Fixed>::from_hz(120.0));

        // Common systems
        app.add_systems(
            Update,
            (
                sync_workspace_gravity,
                sync_velocity_to_network,
                freeze_physics,
                unfreeze_physics,
            ),
        );

        // Server-specific
        #[cfg(feature = "server")]
        app.add_systems(
            Update,
            sync_ownership_to_physics_server,
        )
        .add_systems(
            FixedUpdate,
            validate_physics_state,
        );

        // Client-specific
        #[cfg(feature = "client")]
        app.add_systems(
            Update,
            (
                sync_ownership_to_physics,
                sync_network_to_velocity,
            ),
        );

        info!("Network physics plugin initialized (120Hz)");
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Spawn a networked physics entity.
pub fn spawn_networked_physics(
    commands: &mut Commands,
    transform: Transform,
    owner: NetworkOwner,
) -> Entity {
    commands
        .spawn((
            transform,
            RigidBody::Dynamic,
            Collider::sphere(0.5),
            LinearVelocity::default(),
            AngularVelocity::default(),
            NetworkTransform::from_transform(&transform),
            NetworkVelocity::default(),
            owner,
            PhysicsOwnership::default(),
            crate::replication::Replicated::default(),
        ))
        .id()
}

/// Make an existing entity networked physics.
pub fn add_networked_physics(
    commands: &mut Commands,
    entity: Entity,
    owner: NetworkOwner,
) {
    commands.entity(entity).insert((
        RigidBody::Dynamic,
        LinearVelocity::default(),
        AngularVelocity::default(),
        NetworkVelocity::default(),
        owner,
        PhysicsOwnership::default(),
        crate::replication::Replicated::default(),
    ));
}

