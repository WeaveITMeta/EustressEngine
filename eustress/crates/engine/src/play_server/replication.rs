// ============================================================================
// Play Server - Entity Replication System
// ============================================================================

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Network ID component - assigned to replicated entities
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkId(pub u64);

impl NetworkId {
    /// Generate a new unique network ID
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for NetworkId {
    fn default() -> Self {
        Self::new()
    }
}

/// Marker component for entities that should be replicated
#[derive(Component, Debug, Clone, Default)]
pub struct ReplicatedEntity {
    /// Owner session ID (None = server-owned)
    pub owner: Option<u64>,
    /// Replication priority (higher = more frequent updates)
    pub priority: ReplicationPriority,
    /// Last replicated transform (for delta detection)
    pub last_transform: Option<Transform>,
    /// Last replicated tick
    pub last_tick: u64,
}

/// Replication priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplicationPriority {
    /// Low priority - static objects, infrequent updates
    Low,
    /// Normal priority - most game objects
    #[default]
    Normal,
    /// High priority - player characters, important objects
    High,
    /// Critical - always replicate every tick
    Critical,
}

impl ReplicationPriority {
    /// Get update interval in ticks
    pub fn interval(&self) -> u32 {
        match self {
            Self::Low => 30,      // ~2 times per second at 60 tick
            Self::Normal => 6,    // ~10 times per second
            Self::High => 2,      // ~30 times per second
            Self::Critical => 1,  // Every tick
        }
    }
}

/// Replicated component data (serializable subset of components)
#[derive(Component, Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplicatedComponents {
    /// BasePart properties
    pub base_part: Option<ReplicatedBasePart>,
    /// Humanoid properties
    pub humanoid: Option<ReplicatedHumanoid>,
    /// Physics state
    pub physics: Option<ReplicatedPhysics>,
    /// Custom properties (key-value)
    pub custom: HashMap<String, Vec<u8>>,
}

/// Replicated BasePart data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicatedBasePart {
    pub size: [f32; 3],
    pub color: [f32; 4],
    pub material: u32,
    pub transparency: f32,
    pub anchored: bool,
    pub can_collide: bool,
}

/// Replicated Humanoid data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicatedHumanoid {
    pub health: f32,
    pub max_health: f32,
    pub walk_speed: f32,
    pub jump_power: f32,
    pub state: u32, // HumanoidState as u32
}

/// Replicated physics state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicatedPhysics {
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub sleeping: bool,
}

/// Replication plugin
pub struct ReplicationPlugin;

impl Plugin for ReplicationPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<ReplicationState>()
            .init_resource::<NetworkIdRegistry>()
            .add_systems(Update, (
                assign_network_ids,
                track_transform_changes,
                collect_replication_updates,
            ).chain());
        
        info!("📡 Replication Plugin initialized");
    }
}

/// Global replication state
#[derive(Resource, Default)]
pub struct ReplicationState {
    /// Current server tick
    pub tick: u64,
    /// Pending spawn messages
    pub pending_spawns: Vec<PendingSpawn>,
    /// Pending despawn messages
    pub pending_despawns: Vec<u64>,
    /// Pending updates
    pub pending_updates: Vec<PendingUpdate>,
}

/// Pending entity spawn
#[derive(Debug, Clone)]
pub struct PendingSpawn {
    pub network_id: u64,
    pub class_name: String,
    pub name: String,
    pub parent_id: Option<u64>,
    pub transform: Transform,
    pub components: ReplicatedComponents,
}

/// Pending entity update
#[derive(Debug, Clone)]
pub struct PendingUpdate {
    pub network_id: u64,
    pub transform: Transform,
    pub components: Option<ReplicatedComponents>,
}

/// Registry mapping NetworkId to Entity
#[derive(Resource, Default)]
pub struct NetworkIdRegistry {
    /// NetworkId -> Entity mapping
    pub id_to_entity: HashMap<u64, Entity>,
    /// Entity -> NetworkId mapping
    pub entity_to_id: HashMap<Entity, u64>,
}

impl NetworkIdRegistry {
    /// Register an entity with a network ID
    pub fn register(&mut self, entity: Entity, network_id: NetworkId) {
        self.id_to_entity.insert(network_id.0, entity);
        self.entity_to_id.insert(entity, network_id.0);
    }
    
    /// Unregister an entity
    pub fn unregister(&mut self, entity: Entity) {
        if let Some(id) = self.entity_to_id.remove(&entity) {
            self.id_to_entity.remove(&id);
        }
    }
    
    /// Get entity by network ID
    pub fn get_entity(&self, network_id: u64) -> Option<Entity> {
        self.id_to_entity.get(&network_id).copied()
    }
    
    /// Get network ID by entity
    pub fn get_network_id(&self, entity: Entity) -> Option<u64> {
        self.entity_to_id.get(&entity).copied()
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Assign network IDs to new replicated entities
fn assign_network_ids(
    mut commands: Commands,
    query: Query<Entity, (With<ReplicatedEntity>, Without<NetworkId>)>,
    mut registry: ResMut<NetworkIdRegistry>,
) {
    for entity in query.iter() {
        let network_id = NetworkId::new();
        commands.entity(entity).insert(network_id);
        registry.register(entity, network_id);
        
        info!("📡 Assigned NetworkId {} to entity {:?}", network_id.0, entity);
    }
}

/// Track transform changes for delta replication
fn track_transform_changes(
    mut query: Query<(&NetworkId, &Transform, &mut ReplicatedEntity), Changed<Transform>>,
    replication_state: Res<ReplicationState>,
) {
    for (network_id, transform, mut replicated) in query.iter_mut() {
        // Check if change is significant enough to replicate
        if let Some(last) = replicated.last_transform {
            let pos_delta = (transform.translation - last.translation).length();
            let rot_delta = transform.rotation.angle_between(last.rotation);
            
            // Skip tiny changes
            if pos_delta < 0.001 && rot_delta < 0.001 {
                continue;
            }
        }
        
        replicated.last_transform = Some(*transform);
        replicated.last_tick = replication_state.tick;
    }
}

/// Collect entities that need replication updates
fn collect_replication_updates(
    query: Query<(
        &NetworkId,
        &Transform,
        &ReplicatedEntity,
        Option<&Name>,
        Option<&eustress_common::classes::Instance>,
    )>,
    mut replication_state: ResMut<ReplicationState>,
) {
    replication_state.tick += 1;
    let current_tick = replication_state.tick;
    
    replication_state.pending_updates.clear();
    
    for (network_id, transform, replicated, name, instance) in query.iter() {
        // Check if this entity should update this tick based on priority
        let interval = replicated.priority.interval() as u64;
        if current_tick - replicated.last_tick < interval {
            continue;
        }
        
        // Check if transform actually changed
        if let Some(last) = replicated.last_transform {
            if *transform == last {
                continue;
            }
        }
        
        replication_state.pending_updates.push(PendingUpdate {
            network_id: network_id.0,
            transform: *transform,
            components: None, // TODO: Collect changed components
        });
    }
}

/// Spawn a replicated entity on the client
pub fn spawn_replicated_entity(
    commands: &mut Commands,
    registry: &mut NetworkIdRegistry,
    spawn: &PendingSpawn,
) -> Entity {
    let entity = commands.spawn((
        NetworkId(spawn.network_id),
        ReplicatedEntity {
            owner: None,
            priority: ReplicationPriority::Normal,
            last_transform: Some(spawn.transform),
            last_tick: 0,
        },
        spawn.transform,
        Name::new(spawn.name.clone()),
    )).id();
    
    registry.register(entity, NetworkId(spawn.network_id));
    
    info!("📡 Spawned replicated entity {} ({})", spawn.name, spawn.network_id);
    
    entity
}

/// Despawn a replicated entity
pub fn despawn_replicated_entity(
    commands: &mut Commands,
    registry: &mut NetworkIdRegistry,
    network_id: u64,
) {
    if let Some(entity) = registry.get_entity(network_id) {
        commands.entity(entity).despawn();
        registry.unregister(entity);
        
        info!("📡 Despawned replicated entity {}", network_id);
    }
}

/// Apply a replication update to an entity
pub fn apply_replication_update(
    entity: Entity,
    transform: &mut Transform,
    update: &PendingUpdate,
) {
    *transform = update.transform;
    
    // TODO: Apply component updates
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_network_id_generation() {
        let id1 = NetworkId::new();
        let id2 = NetworkId::new();
        assert_ne!(id1.0, id2.0);
    }
    
    #[test]
    fn test_priority_intervals() {
        assert!(ReplicationPriority::Critical.interval() < ReplicationPriority::High.interval());
        assert!(ReplicationPriority::High.interval() < ReplicationPriority::Normal.interval());
        assert!(ReplicationPriority::Normal.interval() < ReplicationPriority::Low.interval());
    }
}
