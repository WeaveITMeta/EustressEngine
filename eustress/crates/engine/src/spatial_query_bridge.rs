//! # Spatial Query Bridge
//!
//! Unified raycasting and spatial query API for scripting runtimes (Rune + Luau).
//! Wraps Avian 0.6 `SpatialQuery` into a thread-safe, script-accessible layer.
//!
//! ## Table of Contents
//!
//! 1. **RaycastParams** — Filter parameters for raycasts (inspired by Roblox RaycastParams)
//! 2. **RaycastResult** — Single raycast hit result (inspired by Roblox RaycastResult)
//! 3. **ShapecastResult** — Shapecast hit result
//! 4. **ScriptSpatialQuery** — Bevy Resource: thread-safe bridge between scripts and Avian
//! 5. **Plugin** — Bevy plugin that syncs Avian SpatialQuery results to the bridge
//! 6. **Standalone Functions** — Direct raycast/shapecast for use from either runtime

use bevy::prelude::*;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

// ============================================================================
// 1. RaycastParams — Filtering (Roblox-inspired + Avian features)
// ============================================================================

/// Controls which entities a raycast/shapecast considers.
/// Mirrors Roblox `RaycastParams` semantics with Avian extensions.
///
/// ## Roblox Equivalent
/// ```lua
/// local params = RaycastParams.new()
/// params.FilterType = Enum.RaycastFilterType.Exclude
/// params.FilterDescendantsInstances = { workspace.Baseplate }
/// params.IgnoreWater = true
/// ```
///
/// ## Rune Equivalent
/// ```rune
/// let params = RaycastParams::new();
/// params.add_exclude("Baseplate");
/// params.ignore_water = true;
/// ```
#[derive(Debug, Clone, Default)]
pub struct RaycastParams {
    /// Filter mode: true = exclude listed entities, false = include only listed entities
    pub exclude_mode: bool,
    /// Entity names to filter (matched against Name component)
    pub filter_names: Vec<String>,
    /// Entity IDs to filter (raw Bevy Entity bits)
    pub filter_entity_ids: Vec<u64>,
    /// Collision groups to consider (0 = all groups)
    pub collision_group: u32,
    /// Whether to ignore water/liquid volumes
    pub ignore_water: bool,
    /// Whether to respect `can_collide = false` (true = skip non-collidable)
    pub respect_can_collide: bool,
    /// Maximum distance for the ray (studs/meters)
    pub max_distance: f32,
}

impl RaycastParams {
    /// Create default params: exclude mode, no filters, 1000m max distance
    pub fn new() -> Self {
        Self {
            exclude_mode: true,
            filter_names: Vec::new(),
            filter_entity_ids: Vec::new(),
            collision_group: 0,
            ignore_water: false,
            respect_can_collide: true,
            max_distance: 1000.0,
        }
    }

    /// Add an entity name to the filter list
    pub fn add_filter_name(&mut self, name: String) {
        self.filter_names.push(name);
    }

    /// Add an entity ID to the filter list
    pub fn add_filter_id(&mut self, entity_id: u64) {
        self.filter_entity_ids.push(entity_id);
    }
}

// ============================================================================
// 2. RaycastResult — Single hit (Roblox-inspired)
// ============================================================================

/// Result of a single raycast hit.
/// Mirrors Roblox `RaycastResult` with Avian extensions.
///
/// ## Roblox Fields
/// - `Instance` → `entity_id` + `entity_name`
/// - `Position` → `position`
/// - `Normal` → `normal`
/// - `Distance` → `distance`
/// - `Material` → `material`
#[derive(Debug, Clone)]
pub struct RaycastResult {
    /// Bevy Entity bits of the hit entity
    pub entity_id: u64,
    /// Name component of the hit entity (empty if unnamed)
    pub entity_name: String,
    /// World-space position of the hit point
    pub position: [f32; 3],
    /// World-space surface normal at the hit point
    pub normal: [f32; 3],
    /// Distance from ray origin to hit point
    pub distance: f32,
    /// Material name of the hit surface (if available)
    pub material: String,
}

// ============================================================================
// 3. ShapecastResult — Sweep hit
// ============================================================================

/// Result of a shapecast (sweep test).
/// Extends RaycastResult with shape-specific hit data.
#[derive(Debug, Clone)]
pub struct ShapecastResult {
    /// Bevy Entity bits of the hit entity
    pub entity_id: u64,
    /// Name component of the hit entity
    pub entity_name: String,
    /// World-space point on the cast shape at first contact
    pub point1: [f32; 3],
    /// World-space point on the hit collider at first contact
    pub point2: [f32; 3],
    /// Normal pointing from the hit collider toward the cast shape
    pub normal1: [f32; 3],
    /// Normal pointing from the cast shape toward the hit collider
    pub normal2: [f32; 3],
    /// Distance the shape traveled before hitting
    pub distance: f32,
}

// ============================================================================
// 4. ScriptSpatialQuery — Thread-safe bridge resource
// ============================================================================

/// Bevy Resource that holds a thread-safe snapshot of entity metadata
/// needed to resolve raycast results (Entity → name, material, etc.).
/// Scripts read from this; the sync system writes to it each frame.
///
/// Also contains a request/response queue so scripts can submit raycasts
/// and receive results within the same frame (processed by a Bevy system).
#[derive(Resource, Clone)]
pub struct ScriptSpatialQuery {
    /// Entity metadata: Bevy Entity bits → (name, material_name, can_collide)
    pub entity_metadata: Arc<RwLock<HashMap<u64, EntityMetadata>>>,
    /// Pending raycast requests from scripts, processed each frame
    pub raycast_requests: Arc<RwLock<Vec<RaycastRequest>>>,
    /// Completed raycast results, keyed by request_id
    pub raycast_results: Arc<RwLock<HashMap<u64, Option<RaycastResult>>>>,
    /// Pending raycast-all requests from scripts
    pub raycast_all_requests: Arc<RwLock<Vec<RaycastAllRequest>>>,
    /// Completed raycast-all results, keyed by request_id
    pub raycast_all_results: Arc<RwLock<HashMap<u64, Vec<RaycastResult>>>>,
    /// Monotonically increasing request ID counter
    pub next_request_id: Arc<RwLock<u64>>,
}

/// A raycast request submitted by a script for processing by the Bevy system.
#[derive(Debug, Clone)]
pub struct RaycastRequest {
    /// Unique ID for correlating request → result
    pub request_id: u64,
    /// Ray origin in world space
    pub origin: [f32; 3],
    /// Ray direction (will be normalized)
    pub direction: [f32; 3],
    /// Filter parameters
    pub params: RaycastParams,
}

/// A raycast-all request submitted by a script for processing by the Bevy system.
#[derive(Debug, Clone)]
pub struct RaycastAllRequest {
    /// Unique ID for correlating request → result
    pub request_id: u64,
    /// Ray origin in world space
    pub origin: [f32; 3],
    /// Ray direction (will be normalized)
    pub direction: [f32; 3],
    /// Filter parameters
    pub params: RaycastParams,
    /// Maximum number of hits to return
    pub max_hits: u32,
}

/// Cached metadata for a single entity, synced from ECS each frame.
#[derive(Debug, Clone, Default)]
pub struct EntityMetadata {
    pub name: String,
    pub material: String,
    pub can_collide: bool,
    pub is_water: bool,
}

impl Default for ScriptSpatialQuery {
    fn default() -> Self {
        Self {
            entity_metadata: Arc::new(RwLock::new(HashMap::new())),
            raycast_requests: Arc::new(RwLock::new(Vec::new())),
            raycast_results: Arc::new(RwLock::new(HashMap::new())),
            raycast_all_requests: Arc::new(RwLock::new(Vec::new())),
            raycast_all_results: Arc::new(RwLock::new(HashMap::new())),
            next_request_id: Arc::new(RwLock::new(1)),
        }
    }
}

impl ScriptSpatialQuery {
    /// Look up entity metadata by Bevy Entity bits
    pub fn get_metadata(&self, entity_bits: u64) -> Option<EntityMetadata> {
        self.entity_metadata.read().ok()
            .and_then(|map| map.get(&entity_bits).cloned())
    }

    /// Allocate a new unique request ID
    pub fn next_id(&self) -> u64 {
        let mut id = self.next_request_id.write().unwrap();
        let current = *id;
        *id += 1;
        current
    }

    /// Submit a single raycast request. Returns the request_id for polling the result.
    pub fn submit_raycast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        params: RaycastParams,
    ) -> u64 {
        let request_id = self.next_id();
        if let Ok(mut requests) = self.raycast_requests.write() {
            requests.push(RaycastRequest {
                request_id,
                origin,
                direction,
                params,
            });
        }
        request_id
    }

    /// Submit a raycast-all request. Returns the request_id for polling the result.
    pub fn submit_raycast_all(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        params: RaycastParams,
        max_hits: u32,
    ) -> u64 {
        let request_id = self.next_id();
        if let Ok(mut requests) = self.raycast_all_requests.write() {
            requests.push(RaycastAllRequest {
                request_id,
                origin,
                direction,
                params,
                max_hits,
            });
        }
        request_id
    }

    /// Poll for a single raycast result.
    /// Returns `Some(Some(result))` if hit, `Some(None)` if processed but no hit,
    /// `None` if the request hasn't been processed yet.
    pub fn poll_raycast(&self, request_id: u64) -> Option<Option<RaycastResult>> {
        let Ok(mut map) = self.raycast_results.write() else { return None };
        map.remove(&request_id)
            .map(|result| result)
    }

    /// Poll for a raycast-all result. Returns Some(results) if processed, None if pending.
    pub fn poll_raycast_all(&self, request_id: u64) -> Option<Vec<RaycastResult>> {
        self.raycast_all_results.write().ok()
            .and_then(|mut map| map.remove(&request_id))
    }
}

// ============================================================================
// 5. Core Raycast Execution — Called by both Rune and Luau
// ============================================================================

/// Perform a raycast using Avian's SpatialQuery system parameter.
/// This is the shared implementation called by both scripting runtimes.
///
/// Returns `None` if no hit, or the closest `RaycastResult`.
pub fn execute_raycast(
    spatial_query: &avian3d::prelude::SpatialQuery,
    origin: Vec3,
    direction: Vec3,
    params: &RaycastParams,
    bridge: &ScriptSpatialQuery,
    name_query: &Query<(Entity, Option<&Name>, Option<&eustress_common::classes::BasePart>)>,
) -> Option<RaycastResult> {
    use avian3d::prelude::SpatialQueryFilter;

    let Ok(dir) = Dir3::new(direction) else { return None };

    // Build excluded/included entity list from params
    let mut excluded: Vec<Entity> = Vec::new();
    let mut included: Vec<Entity> = Vec::new();

    // Resolve names → entities
    if !params.filter_names.is_empty() {
        for (entity, name_opt, _bp_opt) in name_query.iter() {
            if let Some(name) = name_opt {
                if params.filter_names.iter().any(|filter_name| name.as_str() == filter_name.as_str()) {
                    if params.exclude_mode {
                        excluded.push(entity);
                    } else {
                        included.push(entity);
                    }
                }
            }
        }
    }

    // Resolve raw entity IDs
    for &bits in &params.filter_entity_ids {
        let entity = Entity::from_bits(bits);
        if params.exclude_mode {
            excluded.push(entity);
        } else {
            included.push(entity);
        }
    }

    // If respect_can_collide, exclude entities where can_collide = false
    if params.respect_can_collide {
        for (entity, _name_opt, bp_opt) in name_query.iter() {
            if let Some(base_part) = bp_opt {
                if !base_part.can_collide {
                    excluded.push(entity);
                }
            }
        }
    }

    let mut filter = SpatialQueryFilter::default();
    if !excluded.is_empty() {
        filter = filter.with_excluded_entities(excluded);
    }

    // Perform the raycast — get up to 10 hits, sorted by distance
    let hits = spatial_query.ray_hits(
        origin,
        dir,
        params.max_distance,
        10,
        true, // compute normals
        &filter,
    );

    // Find the closest valid hit
    for hit in hits.iter() {
        let entity_bits = hit.entity.to_bits();

        // Apply include filter (if not in exclude mode and include list is non-empty)
        if !params.exclude_mode && !included.is_empty() {
            if !included.contains(&hit.entity) {
                continue;
            }
        }

        // Resolve metadata
        let metadata = bridge.get_metadata(entity_bits)
            .unwrap_or_default();

        // Apply water filter
        if params.ignore_water && metadata.is_water {
            continue;
        }

        let hit_point = origin + direction * hit.distance;
        let normal = hit.normal.normalize();

        return Some(RaycastResult {
            entity_id: entity_bits,
            entity_name: metadata.name,
            position: hit_point.into(),
            normal: [normal.x, normal.y, normal.z],
            distance: hit.distance,
            material: metadata.material,
        });
    }

    None
}

/// Perform a raycast that returns ALL hits (up to `max_hits`), sorted by distance.
pub fn execute_raycast_all(
    spatial_query: &avian3d::prelude::SpatialQuery,
    origin: Vec3,
    direction: Vec3,
    params: &RaycastParams,
    max_hits: u32,
    bridge: &ScriptSpatialQuery,
    name_query: &Query<(Entity, Option<&Name>, Option<&eustress_common::classes::BasePart>)>,
) -> Vec<RaycastResult> {
    use avian3d::prelude::SpatialQueryFilter;

    let Ok(dir) = Dir3::new(direction) else { return Vec::new() };

    let mut excluded: Vec<Entity> = Vec::new();

    // Resolve names → exclude entities
    if !params.filter_names.is_empty() && params.exclude_mode {
        for (entity, name_opt, _bp) in name_query.iter() {
            if let Some(name) = name_opt {
                if params.filter_names.iter().any(|n| name.as_str() == n.as_str()) {
                    excluded.push(entity);
                }
            }
        }
    }

    for &bits in &params.filter_entity_ids {
        if params.exclude_mode {
            excluded.push(Entity::from_bits(bits));
        }
    }

    if params.respect_can_collide {
        for (entity, _name, bp_opt) in name_query.iter() {
            if let Some(bp) = bp_opt {
                if !bp.can_collide {
                    excluded.push(entity);
                }
            }
        }
    }

    let mut filter = SpatialQueryFilter::default();
    if !excluded.is_empty() {
        filter = filter.with_excluded_entities(excluded);
    }

    let hits = spatial_query.ray_hits(
        origin,
        dir,
        params.max_distance,
        max_hits,
        true,
        &filter,
    );

    let mut results = Vec::with_capacity(hits.len());
    for hit in hits.iter() {
        let entity_bits = hit.entity.to_bits();
        let metadata = bridge.get_metadata(entity_bits).unwrap_or_default();
        if params.ignore_water && metadata.is_water {
            continue;
        }
        let hit_point = origin + direction * hit.distance;
        let normal = hit.normal.normalize();
        results.push(RaycastResult {
            entity_id: entity_bits,
            entity_name: metadata.name,
            position: hit_point.into(),
            normal: [normal.x, normal.y, normal.z],
            distance: hit.distance,
            material: metadata.material,
        });
    }

    // Sort by distance (Avian may not guarantee ordering)
    results.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(max_hits as usize);
    results
}

// ============================================================================
// 6. Metadata Sync System — Runs each frame to keep bridge up to date
// ============================================================================

/// Sync entity metadata from ECS into the ScriptSpatialQuery bridge.
/// This runs each frame so scripts always see current entity names/materials.
fn sync_entity_metadata(
    bridge: Res<ScriptSpatialQuery>,
    query: Query<(Entity, Option<&Name>, Option<&eustress_common::classes::BasePart>), Or<(With<Name>, With<eustress_common::classes::BasePart>)>>,
) {
    let Ok(mut map) = bridge.entity_metadata.write() else { return };
    map.clear();

    for (entity, name_opt, bp_opt) in query.iter() {
        let name = name_opt.map(|n| n.to_string()).unwrap_or_default();
        let (material, can_collide) = if let Some(bp) = bp_opt {
            (format!("{:?}", bp.material), bp.can_collide)
        } else {
            (String::new(), true)
        };

        map.insert(entity.to_bits(), EntityMetadata {
            name,
            material,
            can_collide,
            is_water: false, // TODO: detect water volumes
        });
    }
}

// ============================================================================
// 7. Process Script Raycast Requests — Drains queues, executes via Avian
// ============================================================================

/// Bevy system that drains pending raycast requests and writes results back.
/// Runs each frame after metadata sync so scripts get results next frame.
fn process_script_raycast_requests(
    bridge: Res<ScriptSpatialQuery>,
    spatial_query: avian3d::prelude::SpatialQuery,
    name_query: Query<(Entity, Option<&Name>, Option<&eustress_common::classes::BasePart>)>,
) {
    // Process single-raycast requests
    let requests: Vec<RaycastRequest> = {
        let Ok(mut queue) = bridge.raycast_requests.write() else { return };
        std::mem::take(&mut *queue)
    };

    if !requests.is_empty() {
        let Ok(mut results_map) = bridge.raycast_results.write() else { return };
        for request in requests {
            let origin = Vec3::from(request.origin);
            let direction = Vec3::from(request.direction);
            let result = execute_raycast(
                &spatial_query,
                origin,
                direction,
                &request.params,
                &bridge,
                &name_query,
            );
            results_map.insert(request.request_id, result);
        }
    }

    // Process raycast-all requests
    let all_requests: Vec<RaycastAllRequest> = {
        let Ok(mut queue) = bridge.raycast_all_requests.write() else { return };
        std::mem::take(&mut *queue)
    };

    if !all_requests.is_empty() {
        let Ok(mut results_map) = bridge.raycast_all_results.write() else { return };
        for request in all_requests {
            let origin = Vec3::from(request.origin);
            let direction = Vec3::from(request.direction);
            let results = execute_raycast_all(
                &spatial_query,
                origin,
                direction,
                &request.params,
                request.max_hits,
                &bridge,
                &name_query,
            );
            results_map.insert(request.request_id, results);
        }
    }
}

// ============================================================================
// 8. Plugin
// ============================================================================

/// Bevy plugin that registers the ScriptSpatialQuery resource, metadata sync,
/// and raycast request processing system.
pub struct SpatialQueryBridgePlugin;

impl Plugin for SpatialQueryBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptSpatialQuery>()
            .add_systems(PostUpdate, (
                sync_entity_metadata,
                process_script_raycast_requests.after(sync_entity_metadata),
            ));
    }
}
