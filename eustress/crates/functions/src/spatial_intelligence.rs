//! # Stage 10: Spatial Intelligence — Graph, Link, Query & Resolve
//!
//! Builds spatial navigation graphs, performs radius queries, and resolves
//! an entity's full spatial context (position, region, neighbors, ontology path).
//!
//! ## Table of Contents
//! 1. SpatialNode / SpatialEdge / SpatialContext — result types
//! 2. SpatialBridge — thread-local R-tree-like spatial index + graph
//! 3. Rune functions — graph / link / spatial_query / resolve
//! 4. Module registration
//!
//! ## Functions
//!
//! | Function                       | Purpose                                                     |
//! |--------------------------------|-------------------------------------------------------------|
//! | `graph(region)`                | Build a navigation graph for a named world region           |
//! | `link(a_bits, b_bits, weight)` | Add a weighted edge to the spatial graph                    |
//! | `spatial_query(origin_bits, r)`| Find all entities within radius r (Euclidean)               |
//! | `resolve(entity_bits)`         | Get entity's full spatial context                           |
//!
//! ## Backing
//! `SpatialBridge` holds a flat position map (entity → [x,y,z]) populated by a
//! Bevy system before Rune execution. `spatial_query` does an O(n) scan which is
//! adequate for script use; production code should use eustress-geo's R-tree.
//!
//! ## Bridge population (Bevy system)
//!
//! ```rust,ignore
//! use eustress_functions::spatial_intelligence::{SpatialBridge, SpatialEntry, set_spatial_bridge};
//!
//! fn populate_spatial_bridge(query: Query<(Entity, &Transform, Option<&Name>)>) {
//!     let mut bridge = SpatialBridge::new();
//!     for (entity, transform, name) in &query {
//!         bridge.insert(SpatialEntry {
//!             entity_bits: entity.to_bits(),
//!             position: [
//!                 transform.translation.x as f64,
//!                 transform.translation.y as f64,
//!                 transform.translation.z as f64,
//!             ],
//!             region: "Workspace".to_string(),
//!             name: name.map(|n| n.to_string()).unwrap_or_default(),
//!             ontology_path: String::new(),
//!         });
//!     }
//!     set_spatial_bridge(bridge);
//! }
//! ```
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::spatial_intelligence;
//!
//! pub fn find_nearby(origin_bits, radius) {
//!     let nearby = spatial_intelligence::spatial_query(origin_bits, radius);
//!     for entry in nearby {
//!         let ctx = spatial_intelligence::resolve(entry.entity_bits);
//!         eustress::log_info(&format!("{} @ ({:.1},{:.1},{:.1})", ctx.name, ctx.x, ctx.y, ctx.z));
//!     }
//! }
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use tracing::{info, warn};

// ============================================================================
// 1. Result Types
// ============================================================================

/// A node in the spatial navigation graph.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct SpatialNode {
    /// Entity bits (Bevy Entity::to_bits())
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub entity_bits: u64,
    /// Entity display name
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub name: String,
    /// World region this node belongs to
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub region: String,
    /// X position
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub x: f64,
    /// Y position
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub y: f64,
    /// Z position
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub z: f64,
    /// Ontology class path (e.g. "Entity/Spatial/Prop/Tree")
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub ontology_path: String,
}

/// A weighted edge in the spatial graph.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct SpatialEdge {
    /// Source entity bits
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub from: u64,
    /// Target entity bits
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub to: u64,
    /// Edge weight (e.g. 1/distance or user-defined cost)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub weight: f64,
    /// Euclidean distance between the two nodes
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub distance: f64,
}

/// Full spatial context for an entity (from `resolve()`).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct SpatialContext {
    /// Entity bits
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub entity_bits: u64,
    /// Entity name
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub name: String,
    /// World region
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub region: String,
    /// X coordinate
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub x: f64,
    /// Y coordinate
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub y: f64,
    /// Z coordinate
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub z: f64,
    /// Ontology class path
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub ontology_path: String,
    /// Number of neighbors within default radius (50 units)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub neighbor_count: i64,
    /// Nearest neighbor entity bits (0 if none)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub nearest_bits: u64,
    /// Distance to nearest neighbor (f64::MAX if none)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub nearest_distance: f64,
}

impl SpatialContext {
    fn unknown(entity_bits: u64) -> Self {
        Self {
            entity_bits,
            name: format!("entity:{}", entity_bits),
            region: String::new(),
            x: 0.0,
            y: 0.0,
            z: 0.0,
            ontology_path: String::new(),
            neighbor_count: 0,
            nearest_bits: 0,
            nearest_distance: f64::MAX,
        }
    }
}

/// A region navigation graph from `graph()`.
#[derive(Debug)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct NavGraph {
    /// Region name
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub region: String,
    /// All nodes in the region
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub nodes: rune::runtime::Vec,
    /// All auto-linked edges (nearest-neighbor within default_link_radius)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub edges: rune::runtime::Vec,
    /// Number of nodes
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub node_count: i64,
    /// Number of edges
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub edge_count: i64,
}

// ============================================================================
// 2. SpatialBridge
// ============================================================================

/// One entity's position and metadata in the spatial bridge.
#[derive(Debug, Clone)]
pub struct SpatialEntry {
    /// Bevy Entity::to_bits()
    pub entity_bits: u64,
    /// World-space position [x, y, z]
    pub position: [f64; 3],
    /// Owning region/service name (e.g. "Workspace", "Terrain")
    pub region: String,
    /// Entity display name
    pub name: String,
    /// Ontology class path (optional — can be filled by ontology system)
    pub ontology_path: String,
}

/// Bridge holding spatial position data for all entities + a persistent graph.
pub struct SpatialBridge {
    /// All entities by bits
    pub entries: HashMap<u64, SpatialEntry>,
    /// Persistent graph edges (entity bits → Vec<(target bits, weight)>)
    pub graph_edges: HashMap<u64, Vec<(u64, f64)>>,
    /// Default link radius used by `graph()`
    pub default_link_radius: f64,
}

impl SpatialBridge {
    /// Create a new empty bridge.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            graph_edges: HashMap::new(),
            default_link_radius: 50.0,
        }
    }

    /// Insert a spatial entry.
    pub fn insert(&mut self, entry: SpatialEntry) {
        self.entries.insert(entry.entity_bits, entry);
    }

    /// Euclidean distance between two entity positions.
    fn distance(&self, a_bits: u64, b_bits: u64) -> Option<f64> {
        let a = self.entries.get(&a_bits)?;
        let b = self.entries.get(&b_bits)?;
        let dx = a.position[0] - b.position[0];
        let dy = a.position[1] - b.position[1];
        let dz = a.position[2] - b.position[2];
        Some((dx * dx + dy * dy + dz * dz).sqrt())
    }
}

impl Default for SpatialBridge {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static SPATIAL_BRIDGE: RefCell<Option<SpatialBridge>> = RefCell::new(None);
}

/// Install the spatial bridge before Rune execution.
pub fn set_spatial_bridge(bridge: SpatialBridge) {
    SPATIAL_BRIDGE.with(|cell| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Remove and return the bridge after Rune execution.
pub fn take_spatial_bridge() -> Option<SpatialBridge> {
    SPATIAL_BRIDGE.with(|cell| cell.borrow_mut().take())
}

fn with_bridge<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&SpatialBridge) -> R,
{
    SPATIAL_BRIDGE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Spatial] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

fn with_bridge_mut<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&mut SpatialBridge) -> R,
{
    SPATIAL_BRIDGE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Spatial] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

// ============================================================================
// 3. Rune Functions
// ============================================================================

/// Build a spatial navigation graph for a named world region.
///
/// Collects all entities belonging to `region` and auto-links adjacent
/// nodes within the bridge's `default_link_radius`. Returns a `NavGraph`
/// with all nodes and auto-generated edges.
///
/// # Arguments
/// * `region` — Region name (e.g. "Workspace", "Terrain", "" for all)
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn graph(region: &str) -> NavGraph {
    with_bridge(
        NavGraph {
            region: region.to_string(),
            nodes: rune::runtime::Vec::new(),
            edges: rune::runtime::Vec::new(),
            node_count: 0,
            edge_count: 0,
        },
        |bridge| {
            // Filter to entries in the requested region (empty = all)
            let region_entries: Vec<&SpatialEntry> = bridge
                .entries
                .values()
                .filter(|e| region.is_empty() || e.region == region)
                .collect();

            let mut nodes = rune::runtime::Vec::new();
            let mut edges = rune::runtime::Vec::new();

            // Build nodes
            for entry in &region_entries {
                let node = SpatialNode {
                    entity_bits: entry.entity_bits,
                    name: entry.name.clone(),
                    region: entry.region.clone(),
                    x: entry.position[0],
                    y: entry.position[1],
                    z: entry.position[2],
                    ontology_path: entry.ontology_path.clone(),
                };
                if let Ok(v) = rune::to_value(node) {
                    let _ = nodes.push(v);
                }
            }

            // Auto-link within radius (O(n²) — fine for script use)
            let radius_sq = bridge.default_link_radius * bridge.default_link_radius;
            for i in 0..region_entries.len() {
                for j in (i + 1)..region_entries.len() {
                    let a = region_entries[i];
                    let b = region_entries[j];
                    let dx = a.position[0] - b.position[0];
                    let dy = a.position[1] - b.position[1];
                    let dz = a.position[2] - b.position[2];
                    let dist_sq = dx * dx + dy * dy + dz * dz;

                    if dist_sq <= radius_sq {
                        let dist = dist_sq.sqrt();
                        let edge = SpatialEdge {
                            from: a.entity_bits,
                            to: b.entity_bits,
                            weight: 1.0 / (dist + 1e-9),
                            distance: dist,
                        };
                        if let Ok(v) = rune::to_value(edge) {
                            let _ = edges.push(v);
                        }
                    }
                }
            }

            let node_count = nodes.len() as i64;
            let edge_count = edges.len() as i64;

            info!(
                "[Spatial] graph('{}') → {} nodes, {} edges",
                region, node_count, edge_count
            );

            NavGraph {
                region: region.to_string(),
                nodes,
                edges,
                node_count,
                edge_count,
            }
        },
    )
}

/// Add a weighted edge between two entities in the persistent spatial graph.
///
/// Edges persist in the bridge for the duration of the script execution.
/// They can be queried via `resolve()` (neighbor count) or iterated manually.
///
/// # Arguments
/// * `a_bits` — Source entity bits
/// * `b_bits` — Target entity bits
/// * `weight` — Edge weight (use 1.0 for unweighted, or 1/distance for proximity)
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn link(a_bits: u64, b_bits: u64, weight: f64) {
    with_bridge_mut((), |bridge| {
        // Compute actual distance for annotation
        let dist = bridge.distance(a_bits, b_bits).unwrap_or(0.0);

        bridge
            .graph_edges
            .entry(a_bits)
            .or_default()
            .push((b_bits, weight));
        bridge
            .graph_edges
            .entry(b_bits)
            .or_default()
            .push((a_bits, weight));

        info!(
            "[Spatial] link({}, {}, weight={:.4}) dist={:.2}",
            a_bits, b_bits, weight, dist
        );
    });
}

/// Find all entities within a spatial radius of an origin entity.
///
/// Performs an O(n) scan of all entries in the bridge. Returns a list of
/// `SpatialNode` values sorted by distance (nearest first).
///
/// # Arguments
/// * `origin_bits` — Origin entity bits
/// * `radius`      — Search radius in world units
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn spatial_query(origin_bits: u64, radius: f64) -> rune::runtime::Vec {
    with_bridge(rune::runtime::Vec::new(), |bridge| {
        let Some(origin) = bridge.entries.get(&origin_bits) else {
            warn!("[Spatial] spatial_query: origin entity {} not in bridge", origin_bits);
            return rune::runtime::Vec::new();
        };

        let ox = origin.position[0];
        let oy = origin.position[1];
        let oz = origin.position[2];
        let radius_sq = radius * radius;

        // Collect (distance, entry) within radius, excluding self
        let mut hits: Vec<(f64, &SpatialEntry)> = bridge
            .entries
            .values()
            .filter(|e| e.entity_bits != origin_bits)
            .filter_map(|e| {
                let dx = e.position[0] - ox;
                let dy = e.position[1] - oy;
                let dz = e.position[2] - oz;
                let dist_sq = dx * dx + dy * dy + dz * dz;
                if dist_sq <= radius_sq {
                    Some((dist_sq.sqrt(), e))
                } else {
                    None
                }
            })
            .collect();

        hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut result = rune::runtime::Vec::new();
        for (_, entry) in &hits {
            let node = SpatialNode {
                entity_bits: entry.entity_bits,
                name: entry.name.clone(),
                region: entry.region.clone(),
                x: entry.position[0],
                y: entry.position[1],
                z: entry.position[2],
                ontology_path: entry.ontology_path.clone(),
            };
            if let Ok(v) = rune::to_value(node) {
                let _ = result.push(v);
            }
        }

        info!(
            "[Spatial] spatial_query(origin={}, radius={}) → {} entities",
            origin_bits, radius, hits.len()
        );

        result
    })
}

/// Get an entity's full spatial context — position, region, neighbors, ontology.
///
/// Resolves the nearest neighbor, neighbor count within default radius (50 units),
/// and all stored metadata for the entity.
///
/// # Arguments
/// * `entity_bits` — Bevy `Entity::to_bits()` as u64
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn resolve(entity_bits: u64) -> SpatialContext {
    with_bridge(SpatialContext::unknown(entity_bits), |bridge| {
        let Some(entry) = bridge.entries.get(&entity_bits) else {
            warn!("[Spatial] resolve: entity {} not in bridge", entity_bits);
            return SpatialContext::unknown(entity_bits);
        };

        let ex = entry.position[0];
        let ey = entry.position[1];
        let ez = entry.position[2];
        let default_radius_sq = bridge.default_link_radius * bridge.default_link_radius;

        let mut neighbor_count: i64 = 0;
        let mut nearest_bits: u64 = 0;
        let mut nearest_distance = f64::MAX;

        for other in bridge.entries.values() {
            if other.entity_bits == entity_bits {
                continue;
            }
            let dx = other.position[0] - ex;
            let dy = other.position[1] - ey;
            let dz = other.position[2] - ez;
            let dist_sq = dx * dx + dy * dy + dz * dz;

            if dist_sq <= default_radius_sq {
                neighbor_count += 1;
            }

            if dist_sq < nearest_distance * nearest_distance {
                nearest_distance = dist_sq.sqrt();
                nearest_bits = other.entity_bits;
            }
        }

        info!(
            "[Spatial] resolve(entity={}) region='{}' pos=({:.1},{:.1},{:.1}) neighbors={}",
            entity_bits, entry.region, ex, ey, ez, neighbor_count
        );

        SpatialContext {
            entity_bits,
            name: entry.name.clone(),
            region: entry.region.clone(),
            x: ex,
            y: ey,
            z: ez,
            ontology_path: entry.ontology_path.clone(),
            neighbor_count,
            nearest_bits,
            nearest_distance,
        }
    })
}

// ============================================================================
// 4. Module Registration
// ============================================================================

/// Create the `spatial_intelligence` Rune module.
#[cfg(feature = "rune-dsl")]
pub fn create_spatial_intelligence_module() -> Result<rune::Module, rune::ContextError> {
    let mut module =
        rune::Module::with_crate_item("eustress", ["functions", "spatial_intelligence"])?;

    module.ty::<SpatialNode>()?;
    module.ty::<SpatialEdge>()?;
    module.ty::<SpatialContext>()?;
    module.ty::<NavGraph>()?;

    module.function_meta(graph)?;
    module.function_meta(link)?;
    module.function_meta(spatial_query)?;
    module.function_meta(resolve)?;

    Ok(module)
}
