//! # Replication System
//!
//! Handles entity replication with AOI (Area of Interest) filtering.
//!
//! ## Features
//!
//! - **Spatial hashing** for O(1) neighbor queries
//! - **Distance-based culling** with hysteresis
//! - **Delta compression** (only send changes)
//! - **Priority-based updates** (closer = more frequent)

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::config::ReplicationConfig;
use crate::protocol::{EntityDelta, EntityState, NetworkTransform, NetworkVelocity};

// ============================================================================
// Components
// ============================================================================

/// Marks an entity for network replication.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct Replicated {
    /// Network ID (assigned by server)
    pub net_id: u64,
    /// Last replicated tick
    pub last_tick: u64,
    /// Priority (higher = more frequent updates)
    pub priority: u8,
}

impl Replicated {
    /// Create with network ID
    pub fn new(net_id: u64) -> Self {
        Self {
            net_id,
            last_tick: 0,
            priority: 128, // Default priority
        }
    }

    /// Set priority (0-255)
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }
}

/// Replication group for batch updates.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct ReplicationGroup {
    /// Group ID
    pub group_id: u64,
    /// All entities in this group
    pub members: Vec<Entity>,
}

/// Filter for which clients receive this entity.
#[derive(Component, Debug, Clone, Default)]
pub struct ReplicationFilter {
    /// Whitelist of client IDs (empty = all)
    pub include: HashSet<u64>,
    /// Blacklist of client IDs
    pub exclude: HashSet<u64>,
    /// Custom distance override (0 = use default)
    pub custom_distance: f32,
}

impl ReplicationFilter {
    /// Allow all clients
    pub fn all() -> Self {
        Self::default()
    }

    /// Only specific clients
    pub fn only(clients: impl IntoIterator<Item = u64>) -> Self {
        Self {
            include: clients.into_iter().collect(),
            ..Default::default()
        }
    }

    /// Exclude specific clients
    pub fn except(clients: impl IntoIterator<Item = u64>) -> Self {
        Self {
            exclude: clients.into_iter().collect(),
            ..Default::default()
        }
    }

    /// Check if client should receive
    pub fn should_replicate_to(&self, client_id: u64) -> bool {
        if !self.include.is_empty() && !self.include.contains(&client_id) {
            return false;
        }
        !self.exclude.contains(&client_id)
    }
}

// ============================================================================
// Spatial Hashing
// ============================================================================

/// Spatial hash grid for efficient AOI queries.
#[derive(Resource, Debug, Default)]
pub struct SpatialGrid {
    /// Cell size in studs
    pub cell_size: f32,
    /// Map of cell coords -> entities
    pub cells: HashMap<IVec3, Vec<Entity>>,
    /// Reverse map: entity -> cell
    pub entity_cells: HashMap<Entity, IVec3>,
}

impl SpatialGrid {
    /// Create with cell size
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size,
            cells: HashMap::new(),
            entity_cells: HashMap::new(),
        }
    }

    /// Get cell coordinates for a position
    pub fn cell_coords(&self, pos: Vec3) -> IVec3 {
        IVec3::new(
            (pos.x / self.cell_size).floor() as i32,
            (pos.y / self.cell_size).floor() as i32,
            (pos.z / self.cell_size).floor() as i32,
        )
    }

    /// Insert or update entity position
    pub fn update(&mut self, entity: Entity, pos: Vec3) {
        let new_cell = self.cell_coords(pos);

        // Remove from old cell if moved
        if let Some(old_cell) = self.entity_cells.get(&entity) {
            if *old_cell != new_cell {
                if let Some(entities) = self.cells.get_mut(old_cell) {
                    entities.retain(|e| *e != entity);
                }
            }
        }

        // Add to new cell
        self.cells.entry(new_cell).or_default().push(entity);
        self.entity_cells.insert(entity, new_cell);
    }

    /// Remove entity
    pub fn remove(&mut self, entity: Entity) {
        if let Some(cell) = self.entity_cells.remove(&entity) {
            if let Some(entities) = self.cells.get_mut(&cell) {
                entities.retain(|e| *e != entity);
            }
        }
    }

    /// Query entities within radius of position
    pub fn query_radius(&self, pos: Vec3, radius: f32) -> Vec<Entity> {
        let mut result = Vec::new();
        let center = self.cell_coords(pos);
        let cell_radius = (radius / self.cell_size).ceil() as i32;

        for x in -cell_radius..=cell_radius {
            for y in -cell_radius..=cell_radius {
                for z in -cell_radius..=cell_radius {
                    let cell = center + IVec3::new(x, y, z);
                    if let Some(entities) = self.cells.get(&cell) {
                        result.extend(entities.iter().copied());
                    }
                }
            }
        }

        result
    }
}

// ============================================================================
// Interest Management
// ============================================================================

/// Per-client interest set (entities they should receive).
#[derive(Debug, Default)]
pub struct ClientInterest {
    /// Entities currently in interest
    pub entities: HashSet<Entity>,
    /// Entities pending add (entered AOI)
    pub pending_add: HashSet<Entity>,
    /// Entities pending remove (left AOI)
    pub pending_remove: HashSet<Entity>,
    /// Last known position
    pub position: Vec3,
}

/// Manages interest sets for all clients.
#[derive(Resource, Debug, Default)]
pub struct InterestManager {
    /// Per-client interest
    pub clients: HashMap<u64, ClientInterest>,
    /// AOI radius
    pub aoi_radius: f32,
    /// Hysteresis buffer
    pub hysteresis: f32,
}

impl InterestManager {
    /// Create with config
    pub fn new(config: &ReplicationConfig) -> Self {
        Self {
            clients: HashMap::new(),
            aoi_radius: config.aoi_radius,
            hysteresis: config.aoi_hysteresis,
        }
    }

    /// Update client position and recalculate interest
    pub fn update_client(
        &mut self,
        client_id: u64,
        position: Vec3,
        grid: &SpatialGrid,
        transforms: &Query<&Transform>,
    ) {
        let interest = self.clients.entry(client_id).or_default();
        interest.position = position;

        // Query nearby entities
        let nearby: HashSet<Entity> = grid
            .query_radius(position, self.aoi_radius + self.hysteresis)
            .into_iter()
            .filter(|e| {
                transforms
                    .get(*e)
                    .map(|t| t.translation.distance(position) <= self.aoi_radius)
                    .unwrap_or(false)
            })
            .collect();

        // Find entities that entered AOI
        for entity in nearby.difference(&interest.entities) {
            interest.pending_add.insert(*entity);
        }

        // Find entities that left AOI (with hysteresis)
        for entity in interest.entities.difference(&nearby) {
            if let Ok(transform) = transforms.get(*entity) {
                let distance = transform.translation.distance(position);
                if distance > self.aoi_radius + self.hysteresis {
                    interest.pending_remove.insert(*entity);
                }
            }
        }

        // Apply changes
        for entity in interest.pending_add.drain() {
            interest.entities.insert(entity);
        }
        for entity in interest.pending_remove.drain() {
            interest.entities.remove(&entity);
        }
    }

    /// Get entities in client's interest
    pub fn get_interest(&self, client_id: u64) -> impl Iterator<Item = Entity> + '_ {
        self.clients
            .get(&client_id)
            .map(|i| i.entities.iter().copied())
            .into_iter()
            .flatten()
    }

    /// Remove client
    pub fn remove_client(&mut self, client_id: u64) {
        self.clients.remove(&client_id);
    }
}

// ============================================================================
// Delta Tracking
// ============================================================================

/// Tracks last sent state for delta compression.
#[derive(Resource, Debug, Default)]
pub struct DeltaTracker {
    /// Per-entity last sent state (net_id -> state)
    pub last_sent: HashMap<u64, LastSentState>,
}

/// Last sent state for an entity.
#[derive(Debug, Clone)]
pub struct LastSentState {
    pub tick: u64,
    pub transform: NetworkTransform,
    pub velocity: NetworkVelocity,
}

impl DeltaTracker {
    /// Check if entity needs update
    pub fn needs_update(
        &self,
        net_id: u64,
        transform: &NetworkTransform,
        velocity: &NetworkVelocity,
        threshold: f32,
    ) -> bool {
        match self.last_sent.get(&net_id) {
            Some(last) => {
                transform.differs_from(&last.transform, threshold)
                    || velocity.linear.distance_squared(last.velocity.linear) > threshold * threshold
            }
            None => true, // Never sent
        }
    }

    /// Compute delta
    pub fn compute_delta(
        &self,
        net_id: u64,
        transform: &NetworkTransform,
        velocity: &NetworkVelocity,
        threshold: f32,
    ) -> Option<EntityDelta> {
        let last = self.last_sent.get(&net_id);

        let transform_changed = last
            .map(|l| transform.differs_from(&l.transform, threshold))
            .unwrap_or(true);

        let velocity_changed = last
            .map(|l| velocity.linear.distance_squared(l.velocity.linear) > threshold * threshold)
            .unwrap_or(true);

        if !transform_changed && !velocity_changed {
            return None;
        }

        let mut changed = 0u32;
        let mut delta_transform = None;
        let mut delta_velocity = None;

        if transform_changed {
            changed |= EntityDelta::TRANSFORM_CHANGED;
            delta_transform = Some(transform.clone());
        }

        if velocity_changed {
            changed |= EntityDelta::VELOCITY_CHANGED;
            delta_velocity = Some(velocity.clone());
        }

        Some(EntityDelta {
            net_id,
            changed,
            transform: delta_transform,
            velocity: delta_velocity,
            components: None,
        })
    }

    /// Update last sent state
    pub fn update(
        &mut self,
        net_id: u64,
        tick: u64,
        transform: NetworkTransform,
        velocity: NetworkVelocity,
    ) {
        self.last_sent.insert(
            net_id,
            LastSentState {
                tick,
                transform,
                velocity,
            },
        );
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Update spatial grid with entity positions.
pub fn update_spatial_grid(
    mut grid: ResMut<SpatialGrid>,
    query: Query<(Entity, &Transform), (With<Replicated>, Changed<Transform>)>,
    mut removed: RemovedComponents<Replicated>,
) {
    // Update moved entities
    for (entity, transform) in query.iter() {
        grid.update(entity, transform.translation);
    }

    // Remove despawned entities
    for entity in removed.read() {
        grid.remove(entity);
    }
}

/// Initialize spatial grid from config.
/// Uses NetworkConfig.replication since ReplicationConfig is nested.
pub fn init_spatial_grid(mut commands: Commands, config: Res<crate::config::NetworkConfig>) {
    let replication = &config.replication;
    commands.insert_resource(SpatialGrid::new(replication.grid_cell_size));
    commands.insert_resource(InterestManager::new(replication));
    commands.insert_resource(DeltaTracker::default());
}

// ============================================================================
// Component Sync Systems
// ============================================================================

use crate::protocol::{
    NetworkDomainScope, NetworkAttributes, NetworkTags,
    NetworkDocument, NetworkImageAsset, NetworkVideoAsset,
};

/// Sync Parameters component to NetworkDomainScope for replicated entities.
/// Only updates when Parameters changes.
pub fn sync_parameters_to_network(
    mut commands: Commands,
    query: Query<
        (Entity, &eustress_common::parameters::Parameters, Option<&NetworkDomainScope>),
        (With<Replicated>, Changed<eustress_common::parameters::Parameters>),
    >,
) {
    for (entity, params, existing) in query.iter() {
        let network = NetworkDomainScope::from_parameters(params);
        
        // Only update if changed
        if existing.map(|e| e.differs_from(&network)).unwrap_or(true) {
            commands.entity(entity).insert(network);
        }
    }
}

/// Sync Attributes component to NetworkAttributes for replicated entities.
pub fn sync_attributes_to_network(
    mut commands: Commands,
    query: Query<
        (Entity, &eustress_common::attributes::Attributes, Option<&NetworkAttributes>),
        (With<Replicated>, Changed<eustress_common::attributes::Attributes>),
    >,
) {
    for (entity, attrs, existing) in query.iter() {
        let network = NetworkAttributes::from_attributes(attrs);
        
        if existing.map(|e| e.differs_from(&network)).unwrap_or(true) {
            commands.entity(entity).insert(network);
        }
    }
}

/// Sync Tags component to NetworkTags for replicated entities.
pub fn sync_tags_to_network(
    mut commands: Commands,
    query: Query<
        (Entity, &eustress_common::attributes::Tags, Option<&NetworkTags>),
        (With<Replicated>, Changed<eustress_common::attributes::Tags>),
    >,
) {
    for (entity, tags, existing) in query.iter() {
        let network = NetworkTags::from_tags(tags);
        
        if existing.map(|e| e.differs_from(&network)).unwrap_or(true) {
            commands.entity(entity).insert(network);
        }
    }
}

/// Sync Document component to NetworkDocument for replicated entities.
pub fn sync_document_to_network(
    mut commands: Commands,
    query: Query<
        (Entity, &eustress_common::classes::Document, Option<&NetworkDocument>),
        (With<Replicated>, Changed<eustress_common::classes::Document>),
    >,
) {
    for (entity, doc, existing) in query.iter() {
        let network = NetworkDocument::from_document(doc);
        
        if existing.map(|e| e.differs_from(&network)).unwrap_or(true) {
            commands.entity(entity).insert(network);
        }
    }
}

/// Sync ImageAsset component to NetworkImageAsset for replicated entities.
pub fn sync_image_asset_to_network(
    mut commands: Commands,
    query: Query<
        (Entity, &eustress_common::classes::ImageAsset, Option<&NetworkImageAsset>),
        (With<Replicated>, Changed<eustress_common::classes::ImageAsset>),
    >,
) {
    for (entity, img, existing) in query.iter() {
        let network = NetworkImageAsset::from_image_asset(img);
        
        if existing.map(|e| e.differs_from(&network)).unwrap_or(true) {
            commands.entity(entity).insert(network);
        }
    }
}

/// Sync VideoAsset component to NetworkVideoAsset for replicated entities.
pub fn sync_video_asset_to_network(
    mut commands: Commands,
    query: Query<
        (Entity, &eustress_common::classes::VideoAsset, Option<&NetworkVideoAsset>),
        (With<Replicated>, Changed<eustress_common::classes::VideoAsset>),
    >,
) {
    for (entity, vid, existing) in query.iter() {
        let network = NetworkVideoAsset::from_video_asset(vid);
        
        if existing.map(|e| e.differs_from(&network)).unwrap_or(true) {
            commands.entity(entity).insert(network);
        }
    }
}

/// Initialize network components for newly replicated entities.
/// Runs once when Replicated is added to ensure all network components exist.
pub fn init_network_components_for_replicated(
    mut commands: Commands,
    query: Query<
        (
            Entity,
            Option<&eustress_common::parameters::Parameters>,
            Option<&eustress_common::attributes::Attributes>,
            Option<&eustress_common::attributes::Tags>,
            Option<&eustress_common::classes::Document>,
            Option<&eustress_common::classes::ImageAsset>,
            Option<&eustress_common::classes::VideoAsset>,
        ),
        Added<Replicated>,
    >,
) {
    for (entity, params, attrs, tags, doc, img, vid) in query.iter() {
        let mut entity_commands = commands.entity(entity);
        
        // Initialize NetworkDomainScope if Parameters exists
        if let Some(params) = params {
            entity_commands.insert(NetworkDomainScope::from_parameters(params));
        }
        
        // Initialize NetworkAttributes if Attributes exists
        if let Some(attrs) = attrs {
            entity_commands.insert(NetworkAttributes::from_attributes(attrs));
        }
        
        // Initialize NetworkTags if Tags exists
        if let Some(tags) = tags {
            entity_commands.insert(NetworkTags::from_tags(tags));
        }
        
        // Initialize NetworkDocument if Document exists
        if let Some(doc) = doc {
            entity_commands.insert(NetworkDocument::from_document(doc));
        }
        
        // Initialize NetworkImageAsset if ImageAsset exists
        if let Some(img) = img {
            entity_commands.insert(NetworkImageAsset::from_image_asset(img));
        }
        
        // Initialize NetworkVideoAsset if VideoAsset exists
        if let Some(vid) = vid {
            entity_commands.insert(NetworkVideoAsset::from_video_asset(vid));
        }
    }
}

// ============================================================================
// Replication Plugin
// ============================================================================

/// Plugin for replication management.
pub struct ReplicationPlugin;

impl Plugin for ReplicationPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Replicated>()
            .register_type::<ReplicationGroup>()
            .register_type::<NetworkDomainScope>()
            .register_type::<NetworkAttributes>()
            .register_type::<NetworkTags>()
            .register_type::<NetworkDocument>()
            .register_type::<NetworkImageAsset>()
            .register_type::<NetworkVideoAsset>()
            .add_systems(Startup, init_spatial_grid)
            .add_systems(PostUpdate, (
                update_spatial_grid,
                init_network_components_for_replicated,
                sync_parameters_to_network,
                sync_attributes_to_network,
                sync_tags_to_network,
                sync_document_to_network,
                sync_image_asset_to_network,
                sync_video_asset_to_network,
            ));
    }
}

