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
#[derive(Debug, Clone)]
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

/// `Default` MUST delegate to [`RaycastParams::new`], never be derived.
///
/// A derived `Default` gives `max_distance: 0.0` — a zero-length ray that can
/// never hit anything. `workspace_raycast(origin, direction, None)` (the common
/// no-filter call, from both Rune and Luau) resolves its params through
/// `unwrap_or_default()`, so a derived default silently makes every unfiltered
/// raycast in the engine return nothing.
impl Default for RaycastParams {
    fn default() -> Self {
        Self::new()
    }
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
/// # Why raycasts answer with one frame of latency
///
/// Avian's [`SpatialQuery`](avian3d::prelude::SpatialQuery) is a pure
/// `SystemParam` over ECS queries — it cannot be captured into an `Arc` and
/// handed to a Rune/Luau native function, and the script VM never holds
/// `&World`. So a script's raycast is a REQUEST: it is queued here, executed
/// by [`process_script_raycast_requests`] later the same frame, and read back
/// by the script on its next call.
///
/// The original code submitted a request and polled the *same* `request_id`
/// on the very next line — which could never have been filled yet, so
/// `workspace_raycast` returned `None` 100% of the time. Requests and results
/// are now keyed by **call slot** (the Nth raycast a script performs in a
/// frame; the driver resets the counter each frame), so the Nth call reads the
/// answer to the Nth call of the previous frame. For the overwhelmingly common
/// shape — a fixed number of raycasts per `on_update` — that is exact, just
/// one frame stale. `staleness_guard_m` rejects an answer whose recorded
/// origin has moved implausibly far, so a script that branches into a
/// different number of raycasts gets `None` rather than a wrong hit.
#[derive(Resource, Clone)]
pub struct ScriptSpatialQuery {
    /// Entity metadata: Bevy Entity bits → (name, material_name, can_collide)
    pub entity_metadata: Arc<RwLock<HashMap<u64, EntityMetadata>>>,
    /// Pending raycast requests from scripts, keyed by call slot
    pub raycast_requests: Arc<RwLock<HashMap<u32, RaycastRequest>>>,
    /// Most recent raycast results, keyed by call slot
    pub raycast_results: Arc<RwLock<HashMap<u32, (RaycastRequest, Option<RaycastResult>)>>>,
    /// Pending raycast-all requests from scripts, keyed by call slot
    pub raycast_all_requests: Arc<RwLock<HashMap<u32, RaycastAllRequest>>>,
    /// Most recent raycast-all results, keyed by call slot
    pub raycast_all_results: Arc<RwLock<HashMap<u32, (RaycastAllRequest, Vec<RaycastResult>)>>>,
    /// How far (metres) a cached result's recorded origin may be from the
    /// current request's origin before the cached answer is discarded.
    pub staleness_guard_m: f32,
}

/// A raycast request submitted by a script for processing by the Bevy system.
#[derive(Debug, Clone)]
pub struct RaycastRequest {
    /// Which call in the script's per-frame raycast sequence this is
    pub slot: u32,
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
    /// Which call in the script's per-frame raycast-all sequence this is
    pub slot: u32,
    /// Ray origin in world space
    pub origin: [f32; 3],
    /// Ray direction (will be normalized)
    pub direction: [f32; 3],
    /// Filter parameters
    pub params: RaycastParams,
    /// Maximum number of hits to return
    pub max_hits: u32,
}

/// Distance in metres between two positions.
fn origin_drift(a: [f32; 3], b: [f32; 3]) -> f32 {
    Vec3::from(a).distance(Vec3::from(b))
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
            raycast_requests: Arc::new(RwLock::new(HashMap::new())),
            raycast_results: Arc::new(RwLock::new(HashMap::new())),
            raycast_all_requests: Arc::new(RwLock::new(HashMap::new())),
            raycast_all_results: Arc::new(RwLock::new(HashMap::new())),
            staleness_guard_m: 2.0,
        }
    }
}

impl ScriptSpatialQuery {
    /// Look up entity metadata by Bevy Entity bits
    pub fn get_metadata(&self, entity_bits: u64) -> Option<EntityMetadata> {
        self.entity_metadata.read().ok()
            .and_then(|map| map.get(&entity_bits).cloned())
    }

    /// Queue a raycast for this frame and return the answer to the same call
    /// slot from the previous frame (see the type-level docs for why the
    /// result is one frame old).
    ///
    /// `slot` is the ordinal of this raycast within the script's frame — the
    /// play-mode driver resets the counter each frame.
    pub fn raycast(
        &self,
        slot: u32,
        origin: [f32; 3],
        direction: [f32; 3],
        params: RaycastParams,
    ) -> Option<RaycastResult> {
        let request = RaycastRequest { slot, origin, direction, params };

        let cached = self
            .raycast_results
            .read()
            .ok()
            .and_then(|map| map.get(&slot).cloned())
            .and_then(|(prev, result)| {
                // Reject an answer that plainly belongs to a different query
                // (a branchy script shifting its slot ordering).
                if origin_drift(prev.origin, origin) > self.staleness_guard_m {
                    None
                } else {
                    result
                }
            });

        if let Ok(mut requests) = self.raycast_requests.write() {
            requests.insert(slot, request);
        }
        cached
    }

    /// Queue a raycast-all for this frame and return the previous frame's
    /// answer for the same call slot. Same latency contract as [`Self::raycast`].
    pub fn raycast_all(
        &self,
        slot: u32,
        origin: [f32; 3],
        direction: [f32; 3],
        params: RaycastParams,
        max_hits: u32,
    ) -> Vec<RaycastResult> {
        let request = RaycastAllRequest { slot, origin, direction, params, max_hits };

        let cached = self
            .raycast_all_results
            .read()
            .ok()
            .and_then(|map| map.get(&slot).cloned())
            .and_then(|(prev, results)| {
                if origin_drift(prev.origin, origin) > self.staleness_guard_m {
                    None
                } else {
                    Some(results)
                }
            })
            .unwrap_or_default();

        if let Ok(mut requests) = self.raycast_all_requests.write() {
            requests.insert(slot, request);
        }
        cached
    }

    /// Drop every queued request and cached answer. Called when play mode
    /// stops so the next session starts clean.
    pub fn clear_raycast_state(&self) {
        if let Ok(mut m) = self.raycast_requests.write() { m.clear(); }
        if let Ok(mut m) = self.raycast_results.write() { m.clear(); }
        if let Ok(mut m) = self.raycast_all_requests.write() { m.clear(); }
        if let Ok(mut m) = self.raycast_all_results.write() { m.clear(); }
    }
}

/// Per-thread counter handing out raycast call slots. Reset once per frame by
/// the play-mode driver via [`reset_raycast_slots`].
thread_local! {
    static RAYCAST_SLOT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static RAYCAST_ALL_SLOT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Reset the per-frame raycast call-slot counters. Call before running script
/// callbacks for a frame (and before a one-shot command-bar run).
pub fn reset_raycast_slots() {
    RAYCAST_SLOT.with(|c| c.set(0));
    RAYCAST_ALL_SLOT.with(|c| c.set(0));
}

/// Take the next raycast call slot for this frame.
pub fn next_raycast_slot() -> u32 {
    RAYCAST_SLOT.with(|c| {
        let n = c.get();
        c.set(n.wrapping_add(1));
        n
    })
}

/// Take the next raycast-all call slot for this frame.
pub fn next_raycast_all_slot() -> u32 {
    RAYCAST_ALL_SLOT.with(|c| {
        let n = c.get();
        c.set(n.wrapping_add(1));
        n
    })
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

/// Sync entity metadata from ECS into the ScriptSpatialQuery bridge —
/// INCREMENTALLY. Scripts still always see current names/materials, but the
/// map is only touched for entities that actually changed.
///
/// The previous version cleared and fully rebuilt the map from an UNGATED
/// full-world query every frame: at 131K entities that was ~262K heap
/// string allocations (`Name::to_string` + `format!("{:?}", material)`) per
/// frame — measured as a dominant slice of the 34 ms PostUpdate bucket on
/// the Mountain Ascension benchmark. Same data, same freshness (changes
/// land the frame they happen), a fraction of the cost: a static world now
/// pays one empty change-query check.
fn sync_entity_metadata(
    bridge: Res<ScriptSpatialQuery>,
    changed: Query<
        (Entity, Option<&Name>, Option<&eustress_common::classes::BasePart>),
        (
            Or<(With<Name>, With<eustress_common::classes::BasePart>)>,
            Or<(
                Changed<Name>,
                Changed<eustress_common::classes::BasePart>,
                Added<Name>,
                Added<eustress_common::classes::BasePart>,
            )>,
        ),
    >,
    mut removed_names: RemovedComponents<Name>,
    mut removed_parts: RemovedComponents<eustress_common::classes::BasePart>,
    still_relevant: Query<
        (Entity, Option<&Name>, Option<&eustress_common::classes::BasePart>),
        Or<(With<Name>, With<eustress_common::classes::BasePart>)>,
    >,
) {
    // Fast path: nothing changed, nothing removed — don't even take the lock.
    if changed.is_empty() && removed_names.is_empty() && removed_parts.is_empty() {
        return;
    }
    let Ok(mut map) = bridge.entity_metadata.write() else { return };

    for (entity, name_opt, bp_opt) in changed.iter() {
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

    // Removals: drop the entry when the entity no longer qualifies at all;
    // if it still has the OTHER component, refresh it instead.
    for entity in removed_names.read().chain(removed_parts.read()) {
        match still_relevant.get(entity) {
            Ok((entity, name_opt, bp_opt)) => {
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
                    is_water: false,
                });
            }
            Err(_) => {
                map.remove(&entity.to_bits());
            }
        }
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
    // Process single-raycast requests. Results REPLACE the previous map so a
    // slot the script stopped using doesn't linger and leak.
    let requests: Vec<RaycastRequest> = {
        let Ok(mut queue) = bridge.raycast_requests.write() else { return };
        std::mem::take(&mut *queue).into_values().collect()
    };

    if !requests.is_empty() {
        let mut fresh: HashMap<u32, (RaycastRequest, Option<RaycastResult>)> =
            HashMap::with_capacity(requests.len());
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
            fresh.insert(request.slot, (request, result));
        }
        if let Ok(mut results_map) = bridge.raycast_results.write() {
            *results_map = fresh;
        }
    }

    // Process raycast-all requests
    let all_requests: Vec<RaycastAllRequest> = {
        let Ok(mut queue) = bridge.raycast_all_requests.write() else { return };
        std::mem::take(&mut *queue).into_values().collect()
    };

    if !all_requests.is_empty() {
        let mut fresh: HashMap<u32, (RaycastAllRequest, Vec<RaycastResult>)> =
            HashMap::with_capacity(all_requests.len());
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
            fresh.insert(request.slot, (request, results));
        }
        if let Ok(mut results_map) = bridge.raycast_all_results.write() {
            *results_map = fresh;
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
