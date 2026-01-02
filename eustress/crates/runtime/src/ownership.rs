//! # Ownership Resolution
//!
//! Resolves `NetworkOwnershipRule` from scene to runtime `NetworkOwner`.
//! Handles inheritance, spawn ownership, and archivable constraints.

use bevy::prelude::*;
use eustress_common::scene::NetworkOwnershipRule;
use eustress_common::classes::Instance;

// ============================================================================
// Ownership Plugin
// ============================================================================

/// Plugin for ownership resolution.
pub struct OwnershipPlugin;

impl Plugin for OwnershipPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, resolve_pending_ownership);
    }
}

// ============================================================================
// Components
// ============================================================================

/// Marker for entities pending ownership resolution.
#[derive(Component, Debug, Clone)]
pub struct PendingOwnership {
    /// The rule to resolve
    pub rule: NetworkOwnershipRule,
    /// Client ID of the spawner (for SpawnOwner rule)
    pub spawning_client: Option<u64>,
}

/// Resolved network owner (simplified version for runtime crate).
/// The full NetworkOwner is in eustress-networking.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub struct ResolvedOwner {
    /// Client ID (0 = server)
    pub client_id: u64,
    /// Is this server-owned?
    pub is_server: bool,
}

impl ResolvedOwner {
    pub fn server() -> Self {
        Self { client_id: 0, is_server: true }
    }
    
    pub fn client(id: u64) -> Self {
        Self { client_id: id, is_server: false }
    }
}

// ============================================================================
// Resolution Functions
// ============================================================================

/// Resolve a NetworkOwnershipRule to a ResolvedOwner.
///
/// # Arguments
/// - `rule`: The ownership rule from the scene
/// - `spawning_client`: Client ID that spawned this entity (for SpawnOwner)
/// - `parent_owner`: Parent entity's resolved owner (for Inherit)
/// - `archivable`: Whether the entity is archivable
///
/// # Returns
/// - `Some(ResolvedOwner)` for network-replicated entities
/// - `None` for LocalOnly entities
pub fn resolve_ownership(
    rule: NetworkOwnershipRule,
    spawning_client: Option<u64>,
    parent_owner: Option<ResolvedOwner>,
    archivable: bool,
) -> Option<ResolvedOwner> {
    // Non-archivable entities are always server-owned (prevents exploit saves)
    if !archivable {
        return Some(ResolvedOwner::server());
    }
    
    match rule {
        NetworkOwnershipRule::ServerOnly => {
            Some(ResolvedOwner::server())
        }
        
        NetworkOwnershipRule::ClientClaimable => {
            // Start as server-owned, can be claimed later
            Some(ResolvedOwner::server())
        }
        
        NetworkOwnershipRule::SpawnOwner => {
            match spawning_client {
                Some(client_id) => Some(ResolvedOwner::client(client_id)),
                None => Some(ResolvedOwner::server()), // Server spawned
            }
        }
        
        NetworkOwnershipRule::Inherit => {
            // Use parent's owner, or server if no parent
            Some(parent_owner.unwrap_or_else(ResolvedOwner::server))
        }
        
        NetworkOwnershipRule::LocalOnly => {
            // Not replicated
            None
        }
    }
}

/// Check if an ownership rule allows client claiming.
pub fn is_claimable(rule: NetworkOwnershipRule) -> bool {
    matches!(rule, NetworkOwnershipRule::ClientClaimable)
}

/// Check if an ownership rule requires server ownership.
pub fn is_server_only(rule: NetworkOwnershipRule) -> bool {
    matches!(rule, NetworkOwnershipRule::ServerOnly)
}

// ============================================================================
// Systems
// ============================================================================

/// System to resolve pending ownership markers.
fn resolve_pending_ownership(
    mut commands: Commands,
    query: Query<(Entity, &PendingOwnership, Option<&Instance>, Option<&ChildOf>)>,
    parent_query: Query<&ResolvedOwner>,
) {
    for (entity, pending, instance, parent) in query.iter() {
        // Get archivable status
        let archivable = instance.map(|i| i.archivable).unwrap_or(true);
        
        // Get parent's owner if inheriting
        // ChildOf is a relationship component - parent entity is accessed via .parent()
        let parent_owner = parent
            .and_then(|p| parent_query.get(p.parent()).ok())
            .copied();
        
        // Resolve ownership
        if let Some(owner) = resolve_ownership(
            pending.rule,
            pending.spawning_client,
            parent_owner,
            archivable,
        ) {
            commands.entity(entity)
                .insert(owner)
                .remove::<PendingOwnership>();
        } else {
            // LocalOnly - just remove the pending marker
            commands.entity(entity)
                .remove::<PendingOwnership>();
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Mark an entity for ownership resolution.
pub fn mark_for_resolution(
    commands: &mut Commands,
    entity: Entity,
    rule: NetworkOwnershipRule,
    spawning_client: Option<u64>,
) {
    commands.entity(entity).insert(PendingOwnership {
        rule,
        spawning_client,
    });
}
