//! # Stage 5: Knowledge Web — Graph Modeling, Weaving & Traversal
//!
//! Exposes the `KnowledgeGraph` and `MemoryStore` to Rune scripts for building,
//! linking, and querying semantic knowledge structures around ECS entities.
//!
//! ## Table of Contents
//! 1. Result types     — KnowledgeNode, KnowledgeEdge, SubGraph
//! 2. KnowledgeBridge  — thread-local access to graph + index + ontology
//! 3. Rune functions   — model / weave / traverse
//! 4. Module registration
//!
//! ## Functions
//!
//! | Function              | Purpose                                                       |
//! |-----------------------|---------------------------------------------------------------|
//! | `model(entity_bits)`  | Build a local knowledge subgraph around an entity             |
//! | `weave(from, to, rel)`| Create a typed semantic edge between two concept names        |
//! | `traverse(start, depth)` | BFS walk from a start concept, returning connected subgraph |
//!
//! ## Backing
//! - `KnowledgeGraph` — bidirectional typed concept graph (eustress-embedvec)
//! - `OntologyTree`   — hierarchical class taxonomy for ontology position
//! - `EmbedvecIndex`  — vector similarity for seeding concept embeddings
//!
//! ## Thread-Local Bridge
//!
//! ```rust,ignore
//! use eustress_functions::knowledge::{KnowledgeBridge, set_knowledge_bridge};
//!
//! set_knowledge_bridge(KnowledgeBridge::new(graph, ontology_tree));
//! vm.execute(...)?;
//! take_knowledge_bridge();
//! ```
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::knowledge;
//!
//! pub fn build_concept_web(entity_bits) {
//!     let subgraph = knowledge::model(entity_bits);
//!     for node in subgraph.nodes {
//!         knowledge::weave(node.name, "physical_object", "IsA");
//!     }
//!     let result = knowledge::traverse("physical_object", 2);
//!     eustress::log_info(&format!("Connected concepts: {}", result.node_count));
//! }
//! ```

use std::cell::RefCell;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};

use eustress_embedvec::{
    EmbedvecIndex, KnowledgeGraph, OntologyTree, RelationType,
};

// ============================================================================
// 1. Result Types — returned to Rune scripts
// ============================================================================

/// A single concept node returned from a knowledge query.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct KnowledgeNode {
    /// Concept name (unique key in the graph)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub name: String,
    /// Ontology class path if this concept maps to a class (e.g. "Entity/Spatial/Prop")
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub ontology_path: String,
    /// Number of outgoing edges from this node
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub edge_count: i64,
    /// Depth at which this node was reached during traversal (0 = start)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub depth: i64,
}

/// A typed edge between two concepts.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct KnowledgeEdge {
    /// Source concept name
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub from: String,
    /// Target concept name
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub to: String,
    /// Relation type label (e.g. "Related", "Causes", "IsA", "Opposes")
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub relation: String,
    /// Edge strength weight (0.0–1.0)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub weight: f64,
}

/// A subgraph result — a set of nodes and edges reachable from a query.
#[derive(Debug)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct SubGraph {
    /// All concept nodes in this subgraph
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub nodes: rune::runtime::Vec,
    /// All edges in this subgraph
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub edges: rune::runtime::Vec,
    /// Total node count (for quick checks without iterating)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub node_count: i64,
    /// Total edge count
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub edge_count: i64,
}

#[cfg(feature = "rune-dsl")]
impl SubGraph {
    fn empty() -> Self {
        Self {
            nodes: rune::runtime::Vec::new(),
            edges: rune::runtime::Vec::new(),
            node_count: 0,
            edge_count: 0,
        }
    }
}

// ============================================================================
// 2. KnowledgeBridge — thread-local access
// ============================================================================

/// Bridge providing Rune access to the knowledge graph infrastructure.
pub struct KnowledgeBridge {
    /// Mutable knowledge graph (model/weave write here)
    pub graph: Arc<RwLock<KnowledgeGraph>>,
    /// Ontology tree for resolving class paths on modeled entities
    pub ontology_tree: Arc<RwLock<OntologyTree>>,
    /// Optional vector index for seeding entity concept names from metadata
    pub index: Option<Arc<RwLock<EmbedvecIndex>>>,
}

impl KnowledgeBridge {
    /// Minimal constructor — ontology tree required, index optional.
    pub fn new(
        graph: Arc<RwLock<KnowledgeGraph>>,
        ontology_tree: Arc<RwLock<OntologyTree>>,
    ) -> Self {
        Self {
            graph,
            ontology_tree,
            index: None,
        }
    }

    /// Attach a vector index (enables embedding-seeded concept lookup).
    pub fn with_index(mut self, index: Arc<RwLock<EmbedvecIndex>>) -> Self {
        self.index = Some(index);
        self
    }
}

thread_local! {
    static KNOWLEDGE_BRIDGE: RefCell<Option<KnowledgeBridge>> = RefCell::new(None);
}

/// Install the knowledge bridge before Rune execution.
pub fn set_knowledge_bridge(bridge: KnowledgeBridge) {
    KNOWLEDGE_BRIDGE.with(|cell| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Remove and return the knowledge bridge after Rune execution.
pub fn take_knowledge_bridge() -> Option<KnowledgeBridge> {
    KNOWLEDGE_BRIDGE.with(|cell| cell.borrow_mut().take())
}

/// Read-only access to the bridge.
fn with_bridge<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&KnowledgeBridge) -> R,
{
    KNOWLEDGE_BRIDGE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Knowledge] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

/// Mutable access to the bridge (for weave writes).
fn with_bridge_mut<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&KnowledgeBridge) -> R,
{
    KNOWLEDGE_BRIDGE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Knowledge] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

// ============================================================================
// 3. Rune Functions
// ============================================================================

/// Build a local knowledge subgraph around an entity.
///
/// Resolves the entity's concept name from the embedvec index metadata,
/// then expands one hop in the knowledge graph to collect directly related
/// concepts and their ontology positions.
///
/// # Arguments
/// * `entity_bits` — Bevy `Entity::to_bits()` as u64
///
/// # Returns
/// A `SubGraph` with the entity's concept node plus all first-hop neighbors.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn model(entity_bits: u64) -> SubGraph {
    with_bridge(SubGraph::empty(), |bridge| {
        // Resolve concept name from entity bits via the vector index metadata
        let concept_name = if let Some(idx_lock) = &bridge.index {
            match idx_lock.read() {
                Ok(idx) => {
                    let entity = bevy::prelude::Entity::try_from_bits(entity_bits);
                    let name = entity
                        .and_then(|e| idx.get_metadata(e))
                        .and_then(|meta| meta.name.clone())
                        .unwrap_or_else(|| format!("entity:{}", entity_bits));
                    name
                }
                Err(_) => format!("entity:{}", entity_bits),
            }
        } else {
            format!("entity:{}", entity_bits)
        };

        let graph = match bridge.graph.read() {
            Ok(g) => g,
            Err(_) => {
                warn!("[Knowledge] model: failed to read graph lock");
                return SubGraph::empty();
            }
        };

        let tree = match bridge.ontology_tree.read() {
            Ok(t) => t,
            Err(_) => {
                warn!("[Knowledge] model: failed to read ontology tree lock");
                return SubGraph::empty();
            }
        };

        let edges_from_concept = graph.edges_from(&concept_name);

        // Build the root node
        let root_ontology = tree
            .get_by_name(&concept_name)
            .and_then(|node| tree.path_for(node.id))
            .unwrap_or_default();

        let root_node = KnowledgeNode {
            name: concept_name.clone(),
            ontology_path: root_ontology,
            edge_count: edges_from_concept.len() as i64,
            depth: 0,
        };

        let mut nodes = rune::runtime::Vec::new();
        let mut edges = rune::runtime::Vec::new();

        if let Ok(v) = rune::to_value(root_node) {
            let _ = nodes.push(v);
        }

        // Add first-hop neighbors and edges
        for edge in edges_from_concept {
            let neighbor_edges = graph.edges_from(&edge.target);
            let neighbor_ontology = tree
                .get_by_name(&edge.target)
                .and_then(|node| tree.path_for(node.id))
                .unwrap_or_default();

            let neighbor_node = KnowledgeNode {
                name: edge.target.clone(),
                ontology_path: neighbor_ontology,
                edge_count: neighbor_edges.len() as i64,
                depth: 1,
            };

            let ke = KnowledgeEdge {
                from: concept_name.clone(),
                to: edge.target.clone(),
                relation: relation_type_label(&edge.relation),
                weight: edge.weight as f64,
            };

            if let Ok(v) = rune::to_value(neighbor_node) {
                let _ = nodes.push(v);
            }
            if let Ok(v) = rune::to_value(ke) {
                let _ = edges.push(v);
            }
        }

        let node_count = nodes.len() as i64;
        let edge_count = edges.len() as i64;

        info!(
            "[Knowledge] model(entity={}) → concept='{}', {} nodes, {} edges",
            entity_bits, concept_name, node_count, edge_count
        );

        SubGraph { nodes, edges, node_count, edge_count }
    })
}

/// Create a typed semantic edge between two named concepts in the knowledge graph.
///
/// Edges are bidirectional — `weave("fire", "water", "Opposes")` also adds
/// the inverse edge from "water" back to "fire". Concepts are auto-created
/// if they don't already exist.
///
/// # Arguments
/// * `from`     — Source concept name
/// * `to`       — Target concept name  
/// * `relation` — Relation type: "Related", "Causes", "PartOf", "IsA",
///                "Opposes", "Requires", "Similar", "Precedes", or any custom string
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn weave(from: &str, to: &str, relation: &str) {
    with_bridge_mut((), |bridge| {
        let mut graph = match bridge.graph.write() {
            Ok(g) => g,
            Err(_) => {
                warn!("[Knowledge] weave: failed to acquire graph write lock");
                return;
            }
        };

        let rel = parse_relation_type(relation);

        graph.add_relation(from, to, rel);

        info!("[Knowledge] weave({}, {}, {})", from, to, relation);
    });
}

/// Walk the knowledge graph from a start concept up to `depth` hops.
///
/// Uses BFS to collect all reachable concepts within the given depth limit.
/// Returns a `SubGraph` with every node and edge encountered.
///
/// # Arguments
/// * `start` — Concept name to start traversal from
/// * `depth` — Maximum number of hops (1 = direct neighbors only, max clamped to 8)
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn traverse(start: &str, depth: i64) -> SubGraph {
    let max_depth = depth.clamp(1, 8) as usize;

    with_bridge(SubGraph::empty(), |bridge| {
        let graph = match bridge.graph.read() {
            Ok(g) => g,
            Err(_) => {
                warn!("[Knowledge] traverse: failed to read graph lock");
                return SubGraph::empty();
            }
        };

        let tree = match bridge.ontology_tree.read() {
            Ok(t) => t,
            Err(_) => {
                warn!("[Knowledge] traverse: failed to read ontology tree lock");
                return SubGraph::empty();
            }
        };

        // BFS traversal
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut queue: std::collections::VecDeque<(String, usize)> = std::collections::VecDeque::new();
        let mut nodes = rune::runtime::Vec::new();
        let mut edges = rune::runtime::Vec::new();

        queue.push_back((start.to_string(), 0));
        visited.insert(start.to_string());

        while let Some((concept, current_depth)) = queue.pop_front() {
            let concept_edges = graph.edges_from(&concept);
            let ontology_path = tree
                .get_by_name(&concept)
                .and_then(|node| tree.path_for(node.id))
                .unwrap_or_default();

            let node = KnowledgeNode {
                name: concept.clone(),
                ontology_path,
                edge_count: concept_edges.len() as i64,
                depth: current_depth as i64,
            };

            if let Ok(v) = rune::to_value(node) {
                let _ = nodes.push(v);
            }

            if current_depth < max_depth {
                for edge in concept_edges {
                    let ke = KnowledgeEdge {
                        from: concept.clone(),
                        to: edge.target.clone(),
                        relation: relation_type_label(&edge.relation),
                        weight: edge.weight as f64,
                    };
                    if let Ok(v) = rune::to_value(ke) {
                        let _ = edges.push(v);
                    }

                    if visited.insert(edge.target.clone()) {
                        queue.push_back((edge.target.clone(), current_depth + 1));
                    }
                }
            }
        }

        let node_count = nodes.len() as i64;
        let edge_count = edges.len() as i64;

        info!(
            "[Knowledge] traverse('{}', depth={}) → {} nodes, {} edges",
            start, max_depth, node_count, edge_count
        );

        SubGraph { nodes, edges, node_count, edge_count }
    })
}

// ============================================================================
// Helpers
// ============================================================================

/// Convert a `RelationType` to its display label string.
fn relation_type_label(rel: &RelationType) -> String {
    match rel {
        RelationType::Related => "Related".to_string(),
        RelationType::Causes => "Causes".to_string(),
        RelationType::PartOf => "PartOf".to_string(),
        RelationType::IsA => "IsA".to_string(),
        RelationType::Opposes => "Opposes".to_string(),
        RelationType::Requires => "Requires".to_string(),
        RelationType::Similar => "Similar".to_string(),
        RelationType::Precedes => "Precedes".to_string(),
        RelationType::Custom(s) => s.clone(),
    }
}

/// Parse a string label into a `RelationType`.
fn parse_relation_type(s: &str) -> RelationType {
    match s {
        "Causes" => RelationType::Causes,
        "PartOf" => RelationType::PartOf,
        "IsA" => RelationType::IsA,
        "Opposes" => RelationType::Opposes,
        "Requires" => RelationType::Requires,
        "Similar" => RelationType::Similar,
        "Precedes" => RelationType::Precedes,
        "Related" => RelationType::Related,
        other => RelationType::Custom(other.to_string()),
    }
}

// ============================================================================
// 4. Module Registration
// ============================================================================

/// Create the `knowledge` Rune module with all three functions registered.
#[cfg(feature = "rune-dsl")]
pub fn create_knowledge_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "knowledge"])?;

    // Return types exposed to Rune
    module.ty::<KnowledgeNode>()?;
    module.ty::<KnowledgeEdge>()?;
    module.ty::<SubGraph>()?;

    // Core functions
    module.function_meta(model)?;
    module.function_meta(weave)?;
    module.function_meta(traverse)?;

    Ok(module)
}
