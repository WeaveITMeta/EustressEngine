//! # Stage 2: Proximity — KNN & Spatial Search
//!
//! Exposes embedvec's vector search capabilities to Rune scripts for finding
//! nearby entities, searching by class, and composing multi-step queries.
//!
//! - `nearest(entity_name, k)` — Find k nearest entities globally
//! - `nearest_class(entity_name, class_path, k)` — Find k nearest within a class
//! - `compose(query_text, k)` — Text-to-vector semantic search
//!
//! ## Thread-Local Bridge
//!
//! Proximity functions access the `EmbedvecResource` and `OntologyIndex` through
//! `ProximityBridge`, installed per-thread before Rune execution.
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::proximity;
//!
//! pub fn find_nearby_trees(entity_name) {
//!     let neighbors = proximity::nearest_class(entity_name, "Entity/Spatial/Prop/Vegetation", 5);
//!     for n in neighbors {
//!         eustress::log_info(&format!("{} at distance {}", n.name, n.distance));
//!     }
//! }
//! ```

use std::cell::RefCell;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};

use eustress_embedvec::{EmbedvecIndex, OntologyIndex, PropertyEmbedder};
use bevy::prelude::Entity;

// ============================================================================
// Neighbor Result — Returned to Rune scripts
// ============================================================================

/// A search result representing a nearby entity.
///
/// Uses standard `String` — Rune's `#[rune(get)]` handles conversion.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct Neighbor {
    /// Entity name from embedding metadata
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub name: String,
    /// Cosine similarity score (0.0 to 1.0, higher = more similar)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub score: f64,
    /// Euclidean distance (lower = closer)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub distance: f64,
    /// Entity class path in ontology (e.g., "Entity/Spatial/Prop/Tree")
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub class_path: String,
}

// ============================================================================
// Proximity Bridge — Thread-local access to embedvec
// ============================================================================

/// Bridge providing Rune access to vector search infrastructure.
pub struct ProximityBridge {
    /// The in-memory vector index
    pub index: Arc<RwLock<EmbedvecIndex>>,
    /// The ontology-aware index (optional — enables class-scoped search)
    pub ontology_index: Option<Arc<RwLock<OntologyIndex>>>,
    /// Property embedder for text-to-vector conversion
    pub embedder: Arc<dyn PropertyEmbedder + Send + Sync>,
}

thread_local! {
    static PROXIMITY_BRIDGE: RefCell<Option<ProximityBridge>> = RefCell::new(None);
}

/// Install the proximity bridge before Rune execution.
pub fn set_proximity_bridge(bridge: ProximityBridge) {
    PROXIMITY_BRIDGE.with(|cell| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Clear the proximity bridge after Rune execution.
pub fn take_proximity_bridge() -> Option<ProximityBridge> {
    PROXIMITY_BRIDGE.with(|cell| cell.borrow_mut().take())
}

/// Read-only access to the proximity bridge.
fn with_proximity_bridge<F, R>(fallback: R, callback: F) -> R
where
    F: FnOnce(&ProximityBridge) -> R,
{
    PROXIMITY_BRIDGE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bridge) => callback(bridge),
            None => {
                warn!("[Eustress Functions] Proximity bridge not available — returning fallback");
                fallback
            }
        }
    })
}

// ============================================================================
// Rune Functions
// ============================================================================

/// Find the k nearest entities to a given entity (global search).
///
/// Uses the entity's stored embedding to query the full index via
/// `EmbedvecIndex::find_similar()`. Returns an empty vec if the entity
/// has no embedding or the bridge is unavailable.
///
/// # Arguments
/// * `entity_bits` — Entity ID as u64 (from Bevy Entity::to_bits())
/// * `k` — Number of nearest neighbors to return
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn nearest(entity_bits: u64, k: i64) -> rune::runtime::Vec {
    let k = k.max(1) as usize;

    with_proximity_bridge(rune::runtime::Vec::new(), |bridge| {
        let entity = match Entity::try_from_bits(entity_bits) {
            Some(entity) => entity,
            None => {
                warn!("[Proximity] Invalid entity bits: {}", entity_bits);
                return rune::runtime::Vec::new();
            }
        };

        let index = match bridge.index.read() {
            Ok(guard) => guard,
            Err(_) => {
                warn!("[Proximity] Failed to acquire index read lock");
                return rune::runtime::Vec::new();
            }
        };

        let results = match index.find_similar(entity, k) {
            Ok(r) => r,
            Err(error) => {
                warn!("[Proximity] nearest({}, {}) failed: {}", entity_bits, k, error);
                return rune::runtime::Vec::new();
            }
        };

        info!(
            "[Proximity] nearest(entity={}, k={}) → {} results",
            entity_bits, k, results.len()
        );

        search_results_to_rune_vec(&results, "")
    })
}

/// Find the k nearest entities within a specific ontology class.
///
/// Scopes the search to entities classified under `class_path` and its
/// descendants in the ontology tree. Uses `OntologyIndex::search_class()`.
///
/// # Arguments
/// * `entity_bits` — Entity ID as u64 (from Bevy Entity::to_bits())
/// * `class_path` — Ontology class path (e.g., "Entity/Spatial/Prop/Vegetation")
/// * `k` — Number of nearest neighbors to return
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn nearest_class(entity_bits: u64, class_path: &str, k: i64) -> rune::runtime::Vec {
    let k = k.max(1) as usize;

    with_proximity_bridge(rune::runtime::Vec::new(), |bridge| {
        let entity = match Entity::try_from_bits(entity_bits) {
            Some(e) => e,
            None => {
                warn!("[Proximity] Invalid entity bits: {}", entity_bits);
                return rune::runtime::Vec::new();
            }
        };

        let ontology = match &bridge.ontology_index {
            Some(oi) => oi,
            None => {
                warn!("[Proximity] nearest_class requires ontology_index — not available");
                return rune::runtime::Vec::new();
            }
        };

        let ont_guard = match ontology.read() {
            Ok(guard) => guard,
            Err(_) => {
                warn!("[Proximity] Failed to acquire ontology read lock");
                return rune::runtime::Vec::new();
            }
        };

        let index_guard = match bridge.index.read() {
            Ok(guard) => guard,
            Err(_) => {
                warn!("[Proximity] Failed to acquire index read lock");
                return rune::runtime::Vec::new();
            }
        };

        // Get the entity's embedding from the main index
        let query_embedding = match index_guard.get_embedding(entity) {
            Some(embedding) => embedding.to_vec(),
            None => {
                warn!(
                    "[Proximity] Entity {} has no embedding — cannot search",
                    entity_bits
                );
                return rune::runtime::Vec::new();
            }
        };

        // Search within the ontology class (include descendants)
        let results = match ont_guard.search_class(class_path, &query_embedding, k, true) {
            Ok(r) => r,
            Err(error) => {
                warn!("[Proximity] nearest_class search failed: {}", error);
                return rune::runtime::Vec::new();
            }
        };

        info!(
            "[Proximity] nearest_class(entity={}, {}, {}) \u{2192} {} results",
            entity_bits, class_path, k, results.len()
        );

        search_results_to_rune_vec(&results, class_path)
    })
}

/// Text-to-vector semantic search across all indexed entities.
///
/// Embeds the query text using the configured PropertyEmbedder, then
/// searches the full index for the k most similar entities.
///
/// # Arguments
/// * `query_text` — Natural language query (e.g., "find trees near the player")
/// * `k` — Number of results to return
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn compose(query_text: &str, k: i64) -> rune::runtime::Vec {
    let k = k.max(1) as usize;

    with_proximity_bridge(rune::runtime::Vec::new(), |bridge| {
        // Embed the query text using the embedder
        let query_embedding = match bridge.embedder.embed_query(query_text) {
            Ok(embedding) => embedding,
            Err(error) => {
                warn!("[Proximity] compose embedding failed: {}", error);
                return rune::runtime::Vec::new();
            }
        };

        // Search using the raw embedding against the index
        let index = match bridge.index.read() {
            Ok(guard) => guard,
            Err(_) => {
                warn!("[Proximity] Failed to acquire index read lock");
                return rune::runtime::Vec::new();
            }
        };

        let results = match index.search(&query_embedding, k) {
            Ok(r) => r,
            Err(error) => {
                warn!("[Proximity] compose search failed: {}", error);
                return rune::runtime::Vec::new();
            }
        };

        info!(
            "[Proximity] compose(\"{}\", {}) \u{2192} {} results",
            query_text,
            k,
            results.len()
        );

        search_results_to_rune_vec(&results, "")
    })
}

/// Helper: Convert embedvec SearchResult vec to Rune Vec of Neighbor.
#[cfg(feature = "rune-dsl")]
fn search_results_to_rune_vec(
    results: &[eustress_embedvec::SearchResult],
    default_class: &str,
) -> rune::runtime::Vec {
    let mut output = rune::runtime::Vec::new();
    for result in results {
        // Extract name from EmbeddingMetadata.name (Option<String>)
        let name_str = result
            .metadata
            .name
            .as_deref()
            .unwrap_or("unknown");
        // Extract class_path from metadata properties if available
        let class_str = result
            .metadata
            .properties
            .get("class_path")
            .and_then(|v| v.as_str())
            .unwrap_or(default_class);

        let neighbor = Neighbor {
            name: name_str.to_string(),
            score: result.score as f64,
            distance: (1.0 - result.score as f64).max(0.0),
            class_path: class_str.to_string(),
        };
        if let Ok(value) = rune::to_value(neighbor) {
            let _ = output.push(value);
        }
    }
    output
}

// ============================================================================
// Rune Module Registration
// ============================================================================

/// Create the `proximity` Rune module.
#[cfg(feature = "rune-dsl")]
pub fn create_proximity_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "proximity"])?;

    // Neighbor type
    module.ty::<Neighbor>()?;

    // Core proximity functions
    module.function_meta(nearest)?;
    module.function_meta(nearest_class)?;
    module.function_meta(compose)?;

    Ok(module)
}
