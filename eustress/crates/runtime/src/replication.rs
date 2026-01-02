//! # Replication Sync
//!
//! Bidirectional sync between `BasePart` properties and network components.
//! Direction depends on ownership: owners write, non-owners read.
//!
//! ## Sync Mappings
//!
//! | BasePart Property | Network Component |
//! |-------------------|-------------------|
//! | `cframe` | `NetworkTransform.position/rotation` |
//! | `assembly_linear_velocity` | `NetworkVelocity.linear` |
//! | `assembly_angular_velocity` | `NetworkVelocity.angular` |

use bevy::prelude::*;
use eustress_common::classes::BasePart;
use eustress_networking::prelude::*;

// ============================================================================
// Replication Sync Plugin
// ============================================================================

/// Plugin for BasePart <-> Network sync.
pub struct ReplicationSyncPlugin;

impl Plugin for ReplicationSyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            sync_basepart_to_network,
            sync_network_to_basepart,
        ));
    }
}

// ============================================================================
// Sync Systems
// ============================================================================

/// Sync BasePart -> NetworkTransform/NetworkVelocity (for owned entities).
///
/// Runs when BasePart changes and entity is locally owned.
pub fn sync_basepart_to_network(
    mut query: Query<
        (&BasePart, &mut NetworkTransform, &mut NetworkVelocity, &NetworkOwner),
        Changed<BasePart>,
    >,
    local_client: Option<Res<LocalClient>>,
) {
    let local_id = local_client.map(|c| c.id).unwrap_or(0);
    
    for (basepart, mut net_transform, mut net_velocity, owner) in query.iter_mut() {
        // Only sync if we own this entity
        if !owner.is_owned_by(local_id) {
            continue;
        }
        
        // Sync transform
        net_transform.position = basepart.cframe.translation;
        net_transform.rotation = basepart.cframe.rotation;
        net_transform.scale = basepart.size;
        
        // Sync velocity
        net_velocity.linear = basepart.assembly_linear_velocity;
        net_velocity.angular = basepart.assembly_angular_velocity;
    }
}

/// Sync NetworkTransform/NetworkVelocity -> BasePart (for remote entities).
///
/// Runs when network components change and entity is NOT locally owned.
pub fn sync_network_to_basepart(
    mut query: Query<
        (&mut BasePart, &NetworkTransform, &NetworkVelocity, &NetworkOwner),
        Or<(Changed<NetworkTransform>, Changed<NetworkVelocity>)>,
    >,
    local_client: Option<Res<LocalClient>>,
) {
    let local_id = local_client.map(|c| c.id).unwrap_or(0);
    
    for (mut basepart, net_transform, net_velocity, owner) in query.iter_mut() {
        // Only sync if we DON'T own this entity (receiving updates)
        if owner.is_owned_by(local_id) {
            continue;
        }
        
        // Sync transform
        basepart.cframe.translation = net_transform.position;
        basepart.cframe.rotation = net_transform.rotation;
        // Note: size is typically not synced dynamically
        
        // Sync velocity
        basepart.assembly_linear_velocity = net_velocity.linear;
        basepart.assembly_angular_velocity = net_velocity.angular;
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Initialize network components from a BasePart.
pub fn init_network_from_basepart(basepart: &BasePart) -> (NetworkTransform, NetworkVelocity) {
    let transform = NetworkTransform {
        position: basepart.cframe.translation,
        rotation: basepart.cframe.rotation,
        scale: basepart.size,
    };
    
    let velocity = NetworkVelocity {
        linear: basepart.assembly_linear_velocity,
        angular: basepart.assembly_angular_velocity,
    };
    
    (transform, velocity)
}

/// Check if a BasePart has changed enough to warrant a network update.
pub fn should_replicate(
    basepart: &BasePart,
    net_transform: &NetworkTransform,
    threshold: f32,
) -> bool {
    let pos_diff = basepart.cframe.translation.distance_squared(net_transform.position);
    let rot_diff = basepart.cframe.rotation.angle_between(net_transform.rotation);
    
    pos_diff > threshold * threshold || rot_diff > threshold
}
