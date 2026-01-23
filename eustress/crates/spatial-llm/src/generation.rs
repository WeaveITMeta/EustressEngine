//! Generation - Environment and behavior generation
//!
//! ## Table of Contents
//! 1. EnvironmentGenerator - Generate environments from prompts
//! 2. BehaviorGenerator - Generate NPC behaviors
//! 3. GeneratedContent - Output types

use crate::context::SpatialEntity;
use crate::error::{Result, SpatialLlmError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Generated content from LLM
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeneratedContent {
    /// Generated entities
    pub entities: Vec<SpatialEntity>,
    /// Generation metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Default for GeneratedContent {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            metadata: HashMap::new(),
        }
    }
}

impl GeneratedContent {
    /// Create empty content
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an entity
    pub fn add_entity(&mut self, entity: SpatialEntity) {
        self.entities.push(entity);
    }

    /// Get entity count
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

/// Environment generator
pub struct EnvironmentGenerator {
    /// Whether to validate generated positions
    validate_positions: bool,
    /// Maximum entities per generation
    max_entities: usize,
}

impl Default for EnvironmentGenerator {
    fn default() -> Self {
        Self {
            validate_positions: true,
            max_entities: 1000,
        }
    }
}

impl EnvironmentGenerator {
    /// Create a new environment generator
    pub fn new() -> Self {
        Self::default()
    }

    /// Set max entities
    pub fn with_max_entities(mut self, max: usize) -> Self {
        self.max_entities = max;
        self
    }

    /// Parse LLM response into generated content
    pub fn parse_response(&self, response: &str) -> Result<GeneratedContent> {
        // Find JSON array in response
        let json_start = response.find('[')
            .ok_or_else(|| SpatialLlmError::Generation("No JSON array found in response".to_string()))?;
        let json_end = response.rfind(']')
            .ok_or_else(|| SpatialLlmError::Generation("No closing bracket found".to_string()))?;

        let json_str = &response[json_start..=json_end];

        // Parse as array of entity definitions
        let entities: Vec<EntityDefinition> = serde_json::from_str(json_str)?;

        let mut content = GeneratedContent::new();

        for (i, def) in entities.into_iter().take(self.max_entities).enumerate() {
            let entity = SpatialEntity {
                id: format!("gen_{}", i),
                name: def.name,
                class: def.class,
                position: def.position.unwrap_or([0.0, 0.0, 0.0]),
                rotation: def.rotation.unwrap_or([0.0, 0.0, 0.0]),
                scale: def.scale.unwrap_or([1.0, 1.0, 1.0]),
                properties: def.properties.unwrap_or_default(),
                tags: def.tags.unwrap_or_default(),
                parent_id: None,
                children: Vec::new(),
            };

            content.add_entity(entity);
        }

        Ok(content)
    }
}

/// Entity definition from LLM response
#[derive(Clone, Debug, Serialize, Deserialize)]
struct EntityDefinition {
    class: String,
    name: Option<String>,
    position: Option<[f64; 3]>,
    rotation: Option<[f64; 3]>,
    scale: Option<[f64; 3]>,
    properties: Option<HashMap<String, serde_json::Value>>,
    tags: Option<Vec<String>>,
}

/// Behavior generator for NPCs
pub struct BehaviorGenerator {
    /// Default behavior patterns
    default_behaviors: HashMap<String, serde_json::Value>,
}

impl Default for BehaviorGenerator {
    fn default() -> Self {
        Self {
            default_behaviors: HashMap::new(),
        }
    }
}

impl BehaviorGenerator {
    /// Create a new behavior generator
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a default behavior pattern
    pub fn with_default_behavior(mut self, name: impl Into<String>, behavior: serde_json::Value) -> Self {
        self.default_behaviors.insert(name.into(), behavior);
        self
    }

    /// Parse behavior response from LLM
    pub fn parse_response(&self, response: &str) -> Result<serde_json::Value> {
        // Find JSON object in response
        let json_start = response.find('{')
            .ok_or_else(|| SpatialLlmError::Generation("No JSON object found in response".to_string()))?;
        let json_end = response.rfind('}')
            .ok_or_else(|| SpatialLlmError::Generation("No closing brace found".to_string()))?;

        let json_str = &response[json_start..=json_end];
        let behavior: serde_json::Value = serde_json::from_str(json_str)?;

        Ok(behavior)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_environment_response() {
        let generator = EnvironmentGenerator::new();

        let response = r#"Here are some trees for your forest:
[
  {"class": "Tree", "name": "Oak", "position": [10.0, 0.0, 5.0], "tags": ["vegetation"]},
  {"class": "Tree", "name": "Pine", "position": [15.0, 0.0, 8.0]}
]
"#;

        let content = generator.parse_response(response).unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content.entities[0].class, "Tree");
    }
}
