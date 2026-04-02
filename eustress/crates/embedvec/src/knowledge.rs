//! Knowledge Graph — Bidirectional Concept Relations
//!
//! ## Table of Contents
//! 1. KnowledgeGraph  — bidirectional HashMap concept graph
//! 2. RelationEdge    — typed lateral edge between two concept nodes
//! 3. ConceptNode     — named concept with tags and embedding hint
//!
//! ## Purpose
//! `OntologyTree` models strict parent→child class taxonomy.
//! `KnowledgeGraph` models arbitrary lateral relationships between concepts:
//! "fire" → ["heat", "light", "danger"], "rain" ↔ "water", etc.
//!
//! Inspired by Vortex `Atman::build_knowledge_graph()` which uses a plain
//! bidirectional `HashMap<String, Vec<String>>`. This extends that with
//! typed edges and optional embedding hints for richer traversal.
//!
//! ## Usage
//! ```rust
//! use eustress_embedvec::{KnowledgeGraph, RelationType};
//!
//! let mut kg = KnowledgeGraph::new();
//! kg.add_relations("fire", &["heat", "light", "danger"]);
//! kg.add_relation("fire", "water", RelationType::Opposes);
//!
//! let related = kg.related("fire");    // ["heat", "light", "danger", "water"]
//! let path = kg.shortest_path("fire", "cold"); // BFS path if connected
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

// ============================================================================
// 1. RelationType — typed edge label
// ============================================================================

/// Type of relationship between two concepts
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationType {
    /// Generic association (default)
    Related,
    /// A causes or produces B
    Causes,
    /// A is a component of B
    PartOf,
    /// A is a subtype of B (different from OntologyTree — for concept space)
    IsA,
    /// A opposes or contradicts B
    Opposes,
    /// A requires B to function
    Requires,
    /// A is similar to B (near-synonym)
    Similar,
    /// A precedes B in time or causation
    Precedes,
    /// Custom relation label
    Custom(String),
}

impl RelationType {
    /// Bidirectional inverse of this relation type
    pub fn inverse(&self) -> RelationType {
        match self {
            RelationType::Causes => RelationType::Related,
            RelationType::PartOf => RelationType::Related,
            RelationType::IsA => RelationType::Related,
            RelationType::Opposes => RelationType::Opposes,
            RelationType::Requires => RelationType::Related,
            RelationType::Similar => RelationType::Similar,
            RelationType::Precedes => RelationType::Related,
            RelationType::Related => RelationType::Related,
            RelationType::Custom(s) => RelationType::Custom(format!("inv:{}", s)),
        }
    }
}

// ============================================================================
// 2. RelationEdge — typed lateral edge
// ============================================================================

/// A directed typed edge between two concept nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationEdge {
    /// Target concept name
    pub target: String,
    /// Relation type
    pub relation: RelationType,
    /// Strength weight (0.0–1.0)
    pub weight: f32,
}

impl RelationEdge {
    /// Create a new edge with default weight 1.0
    pub fn new(target: impl Into<String>, relation: RelationType) -> Self {
        Self {
            target: target.into(),
            relation,
            weight: 1.0,
        }
    }

    /// Set edge weight
    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = weight.clamp(0.0, 1.0);
        self
    }
}

// ============================================================================
// 3. ConceptNode
// ============================================================================

/// A named concept node in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptNode {
    /// Concept name (unique key)
    pub name: String,
    /// Optional tags for filtering
    pub tags: Vec<String>,
    /// Optional short description
    pub description: Option<String>,
    /// Optional pre-computed embedding hint (for seeding vector search)
    pub embedding_hint: Option<Vec<f32>>,
}

impl ConceptNode {
    /// Create a minimal concept node
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tags: Vec::new(),
            description: None,
            embedding_hint: None,
        }
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Add a description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

// ============================================================================
// 1. KnowledgeGraph
// ============================================================================

/// Bidirectional concept relation graph.
///
/// Nodes are concept names (`String`). Edges are typed `RelationEdge`s.
/// All `add_relations` calls are automatically bidirectional with inverse types.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    /// Adjacency list: concept → outgoing edges
    edges: HashMap<String, Vec<RelationEdge>>,
    /// Optional concept metadata
    nodes: HashMap<String, ConceptNode>,
}

impl KnowledgeGraph {
    /// Create an empty knowledge graph
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a concept node with metadata
    pub fn add_node(&mut self, node: ConceptNode) {
        self.edges.entry(node.name.clone()).or_default();
        self.nodes.insert(node.name.clone(), node);
    }

    /// Add a single typed directed edge (and its inverse)
    pub fn add_relation(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        relation: RelationType,
    ) {
        let from = from.into();
        let to = to.into();
        let inverse = relation.inverse();

        self.edges
            .entry(from.clone())
            .or_default()
            .push(RelationEdge::new(to.clone(), relation));

        self.edges
            .entry(to)
            .or_default()
            .push(RelationEdge::new(from, inverse));
    }

    /// Add multiple `Related` edges from one concept to many (bidirectional).
    /// Mirrors Vortex `Atman::add_relations()`.
    pub fn add_relations(&mut self, concept: &str, related: &[&str]) {
        for &target in related {
            self.add_relation(concept, target, RelationType::Related);
        }
    }

    /// Get all outgoing edges for a concept
    pub fn edges_from(&self, concept: &str) -> &[RelationEdge] {
        self.edges.get(concept).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Get names of all concepts directly related to the given concept
    pub fn related(&self, concept: &str) -> Vec<&str> {
        self.edges_from(concept)
            .iter()
            .map(|e| e.target.as_str())
            .collect()
    }

    /// Get edges filtered by relation type
    pub fn related_by(&self, concept: &str, relation: &RelationType) -> Vec<&str> {
        self.edges_from(concept)
            .iter()
            .filter(|e| &e.relation == relation)
            .map(|e| e.target.as_str())
            .collect()
    }

    /// Check if two concepts are directly connected
    pub fn are_related(&self, a: &str, b: &str) -> bool {
        self.edges_from(a).iter().any(|e| e.target == b)
    }

    /// BFS shortest path between two concepts (returns node names or None)
    pub fn shortest_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        if from == to {
            return Some(vec![from.to_string()]);
        }

        let mut visited: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<Vec<&str>> = VecDeque::new();

        queue.push_back(vec![from]);
        visited.insert(from);

        while let Some(path) = queue.pop_front() {
            let current = *path.last().unwrap();

            for edge in self.edges_from(current) {
                let next = edge.target.as_str();
                if next == to {
                    let mut result: Vec<String> =
                        path.iter().map(|s| s.to_string()).collect();
                    result.push(to.to_string());
                    return Some(result);
                }
                if !visited.contains(next) {
                    visited.insert(next);
                    let mut new_path = path.clone();
                    new_path.push(next);
                    queue.push_back(new_path);
                }
            }
        }

        None
    }

    /// Get all concepts reachable from `start` within `max_hops`
    pub fn reachable(&self, start: &str, max_hops: usize) -> Vec<String> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut frontier: Vec<String> = vec![start.to_string()];
        visited.insert(start.to_string());

        for _ in 0..max_hops {
            let mut next_frontier = Vec::new();
            for concept in &frontier {
                for edge in self.edges_from(concept) {
                    if !visited.contains(&edge.target) {
                        visited.insert(edge.target.clone());
                        next_frontier.push(edge.target.clone());
                    }
                }
            }
            if next_frontier.is_empty() {
                break;
            }
            frontier = next_frontier;
        }

        visited.into_iter().filter(|s| s != start).collect()
    }

    /// Total number of concept nodes
    pub fn node_count(&self) -> usize {
        self.edges.len()
    }

    /// Total number of directed edges
    pub fn edge_count(&self) -> usize {
        self.edges.values().map(|v| v.len()).sum()
    }

    /// Get concept node metadata if registered
    pub fn get_node(&self, concept: &str) -> Option<&ConceptNode> {
        self.nodes.get(concept)
    }

    /// All concept names in the graph
    pub fn concepts(&self) -> impl Iterator<Item = &str> {
        self.edges.keys().map(String::as_str)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_relations_bidirectional() {
        let mut kg = KnowledgeGraph::new();
        kg.add_relations("fire", &["heat", "light"]);

        assert!(kg.are_related("fire", "heat"));
        assert!(kg.are_related("heat", "fire")); // bidirectional
        assert_eq!(kg.related("fire").len(), 2);
    }

    #[test]
    fn test_typed_relation() {
        let mut kg = KnowledgeGraph::new();
        kg.add_relation("rain", "drought", RelationType::Opposes);

        assert!(kg.are_related("rain", "drought"));
        assert!(kg.are_related("drought", "rain")); // Opposes is self-inverse

        let opposites = kg.related_by("rain", &RelationType::Opposes);
        assert!(opposites.contains(&"drought"));
    }

    #[test]
    fn test_shortest_path() {
        let mut kg = KnowledgeGraph::new();
        kg.add_relations("a", &["b"]);
        kg.add_relations("b", &["c"]);
        kg.add_relations("c", &["d"]);

        let path = kg.shortest_path("a", "d").unwrap();
        assert_eq!(path, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_shortest_path_not_found() {
        let mut kg = KnowledgeGraph::new();
        kg.add_relations("a", &["b"]);
        // "c" is isolated
        kg.add_node(ConceptNode::new("c"));

        assert!(kg.shortest_path("a", "c").is_none());
    }

    #[test]
    fn test_reachable() {
        let mut kg = KnowledgeGraph::new();
        kg.add_relations("root", &["a", "b"]);
        kg.add_relations("a", &["c"]);

        let reachable = kg.reachable("root", 2);
        assert!(reachable.contains(&"a".to_string()));
        assert!(reachable.contains(&"b".to_string()));
        assert!(reachable.contains(&"c".to_string())); // 2 hops
    }
}
