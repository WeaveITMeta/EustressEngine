//! Query Processing - Natural language spatial queries
//!
//! ## Table of Contents
//! 1. QueryProcessor - Process spatial queries
//! 2. SpatialQuery - Query types
//! 3. QueryResult - Query results

use crate::context::SpatialEntity;
use crate::error::Result;
use serde::{Deserialize, Serialize};

/// Types of spatial queries
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SpatialQuery {
    /// Find entities near a point
    FindNear {
        x: f64,
        y: f64,
        z: f64,
        radius: f64,
    },
    /// Find entities by class
    FindByClass {
        class: String,
    },
    /// Find entities by tag
    FindByTag {
        tag: String,
    },
    /// Find k nearest neighbors
    FindKNearest {
        x: f64,
        y: f64,
        z: f64,
        k: usize,
    },
    /// Natural language query
    NaturalLanguage {
        query: String,
    },
}

/// Result from a spatial query
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryResult {
    /// Query that was executed
    pub query: String,
    /// Matched entity IDs
    pub entity_ids: Vec<String>,
    /// Natural language answer (if applicable)
    pub answer: Option<String>,
    /// Additional metadata
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

impl QueryResult {
    /// Create a new query result
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            entity_ids: Vec::new(),
            answer: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Add entity IDs
    pub fn with_entities(mut self, ids: Vec<String>) -> Self {
        self.entity_ids = ids;
        self
    }

    /// Add answer
    pub fn with_answer(mut self, answer: impl Into<String>) -> Self {
        self.answer = Some(answer.into());
        self
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entity_ids.is_empty() && self.answer.is_none()
    }
}

/// Query processor for spatial queries
pub struct QueryProcessor {
    /// Whether to use LLM for natural language queries
    use_llm: bool,
}

impl Default for QueryProcessor {
    fn default() -> Self {
        Self { use_llm: true }
    }
}

impl QueryProcessor {
    /// Create a new query processor
    pub fn new() -> Self {
        Self::default()
    }

    /// Disable LLM for natural language queries
    pub fn without_llm(mut self) -> Self {
        self.use_llm = false;
        self
    }

    /// Parse a natural language query into a structured query
    pub fn parse_query(&self, query: &str) -> SpatialQuery {
        let lower = query.to_lowercase();

        // Simple keyword-based parsing
        if lower.contains("near") || lower.contains("close to") {
            // Try to extract coordinates
            // For now, default to origin
            SpatialQuery::FindNear {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                radius: 50.0,
            }
        } else if lower.contains("find all") && lower.contains("trees") {
            SpatialQuery::FindByClass {
                class: "Tree".to_string(),
            }
        } else if lower.contains("tagged") || lower.contains("with tag") {
            // Extract tag from query
            SpatialQuery::FindByTag {
                tag: "unknown".to_string(),
            }
        } else {
            // Fall back to natural language
            SpatialQuery::NaturalLanguage {
                query: query.to_string(),
            }
        }
    }

    /// Execute a structured query against entities
    pub fn execute_local(
        &self,
        query: &SpatialQuery,
        entities: &[SpatialEntity],
    ) -> QueryResult {
        match query {
            SpatialQuery::FindNear { x, y, z, radius } => {
                let ids: Vec<String> = entities
                    .iter()
                    .filter(|e| e.distance_to_point(*x, *y, *z) <= *radius)
                    .map(|e| e.id.clone())
                    .collect();

                QueryResult::new(format!("Find near ({}, {}, {}) radius {}", x, y, z, radius))
                    .with_entities(ids)
            }
            SpatialQuery::FindByClass { class } => {
                let ids: Vec<String> = entities
                    .iter()
                    .filter(|e| e.class == *class)
                    .map(|e| e.id.clone())
                    .collect();

                QueryResult::new(format!("Find by class: {}", class))
                    .with_entities(ids)
            }
            SpatialQuery::FindByTag { tag } => {
                let ids: Vec<String> = entities
                    .iter()
                    .filter(|e| e.tags.contains(tag))
                    .map(|e| e.id.clone())
                    .collect();

                QueryResult::new(format!("Find by tag: {}", tag))
                    .with_entities(ids)
            }
            SpatialQuery::FindKNearest { x, y, z, k } => {
                let mut with_dist: Vec<_> = entities
                    .iter()
                    .map(|e| (e, e.distance_to_point(*x, *y, *z)))
                    .collect();

                with_dist.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

                let ids: Vec<String> = with_dist
                    .into_iter()
                    .take(*k)
                    .map(|(e, _)| e.id.clone())
                    .collect();

                QueryResult::new(format!("Find {} nearest to ({}, {}, {})", k, x, y, z))
                    .with_entities(ids)
            }
            SpatialQuery::NaturalLanguage { query } => {
                // For local execution, just return empty result
                // LLM would be needed for actual NL processing
                QueryResult::new(query.clone())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_parsing() {
        let processor = QueryProcessor::new();

        let query = processor.parse_query("find entities near the player");
        assert!(matches!(query, SpatialQuery::FindNear { .. }));
    }

    #[test]
    fn test_local_execution() {
        let processor = QueryProcessor::new();

        let entities = vec![
            SpatialEntity::new("e1", "Tree").with_position(0.0, 0.0, 0.0),
            SpatialEntity::new("e2", "Rock").with_position(100.0, 0.0, 0.0),
        ];

        let query = SpatialQuery::FindNear {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            radius: 10.0,
        };

        let result = processor.execute_local(&query, &entities);
        assert_eq!(result.entity_ids.len(), 1);
        assert_eq!(result.entity_ids[0], "e1");
    }
}
