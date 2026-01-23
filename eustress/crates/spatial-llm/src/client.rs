//! Spatial LLM Client - Main interface for spatial AI operations
//!
//! ## Table of Contents
//! 1. SpatialLlm - Main client struct
//! 2. SpatialLlmConfig - Configuration

use crate::context::{SceneGraph, SpatialContext, SpatialEntity};
use crate::error::{Result, SpatialLlmError};
use crate::indexing::SpatialIndex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for the Spatial LLM client
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpatialLlmConfig {
    /// API endpoint for remote LLM
    pub api_endpoint: Option<String>,
    /// API key for authentication
    pub api_key: Option<String>,
    /// Model name (e.g., "gpt-4", "claude-3")
    pub model: String,
    /// Maximum tokens for generation
    pub max_tokens: u32,
    /// Temperature for generation
    pub temperature: f32,
    /// Default focus radius for context
    pub default_focus_radius: f64,
    /// Maximum entities to include in context
    pub max_context_entities: usize,
}

impl Default for SpatialLlmConfig {
    fn default() -> Self {
        Self {
            api_endpoint: None,
            api_key: None,
            model: "gpt-4".to_string(),
            max_tokens: 2048,
            temperature: 0.7,
            default_focus_radius: 100.0,
            max_context_entities: 100,
        }
    }
}

impl SpatialLlmConfig {
    /// Create config from environment variables
    pub fn from_env() -> Result<Self> {
        let mut config = Self::default();

        if let Ok(endpoint) = std::env::var("SPATIAL_LLM_ENDPOINT") {
            config.api_endpoint = Some(endpoint);
        }

        if let Ok(key) = std::env::var("SPATIAL_LLM_API_KEY") {
            config.api_key = Some(key);
        }

        if let Ok(model) = std::env::var("SPATIAL_LLM_MODEL") {
            config.model = model;
        }

        Ok(config)
    }

    /// Set API endpoint
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.api_endpoint = Some(endpoint.into());
        self
    }

    /// Set API key
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Set model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

/// Main Spatial LLM client
pub struct SpatialLlm {
    /// Configuration
    config: SpatialLlmConfig,
    /// HTTP client for API calls
    client: reqwest::Client,
    /// Spatial index for fast queries
    index: Arc<RwLock<SpatialIndex>>,
    /// Current scene graph
    scene_graph: Arc<RwLock<SceneGraph>>,
}

impl SpatialLlm {
    /// Create a new Spatial LLM client
    pub async fn new(config: SpatialLlmConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| SpatialLlmError::Api(e.to_string()))?;

        Ok(Self {
            config,
            client,
            index: Arc::new(RwLock::new(SpatialIndex::default())),
            scene_graph: Arc::new(RwLock::new(SceneGraph::new())),
        })
    }

    /// Get the configuration
    pub fn config(&self) -> &SpatialLlmConfig {
        &self.config
    }

    /// Add an entity to the spatial context
    pub async fn add_entity(&self, entity: SpatialEntity) {
        self.index.write().await.insert(&entity);
        self.scene_graph.write().await.add_entity(entity);
    }

    /// Remove an entity from the spatial context
    pub async fn remove_entity(&self, id: &str) {
        self.index.write().await.remove(id);
    }

    /// Build a spatial context around a focus point
    pub async fn build_context(&self, x: f64, y: f64, z: f64, radius: f64) -> SpatialContext {
        let scene_graph = self.scene_graph.read().await;

        let mut context = SpatialContext::new().with_focus(x, y, z, radius);
        context.scene_graph = scene_graph.clone();

        context
    }

    /// Query the spatial world with natural language
    pub async fn query_spatial(&self, query: &str) -> Result<String> {
        // Build context around origin (could be parameterized)
        let context = self.build_context(0.0, 0.0, 0.0, self.config.default_focus_radius).await;

        // Build prompt
        let prompt = format!(
            "You are a spatial reasoning AI assistant. Given the following 3D scene:\n\n{}\n\nAnswer this question: {}",
            context.to_prompt_text(),
            query
        );

        // Call LLM API
        self.call_llm(&prompt).await
    }

    /// Query with a specific focus point
    pub async fn query_at(&self, query: &str, x: f64, y: f64, z: f64, radius: f64) -> Result<String> {
        let context = self.build_context(x, y, z, radius).await;

        let prompt = format!(
            "You are a spatial reasoning AI assistant. Given the following 3D scene:\n\n{}\n\nAnswer this question: {}",
            context.to_prompt_text(),
            query
        );

        self.call_llm(&prompt).await
    }

    /// Find entities near a point
    pub async fn find_near(&self, x: f64, y: f64, z: f64, radius: f64) -> Vec<String> {
        self.index
            .read()
            .await
            .find_within_radius(x, y, z, radius)
            .into_iter()
            .map(|e| e.id.clone())
            .collect()
    }

    /// Find k nearest entities to a point
    pub async fn find_k_nearest(&self, x: f64, y: f64, z: f64, k: usize) -> Vec<(String, f64)> {
        self.index
            .read()
            .await
            .find_k_nearest(x, y, z, k)
            .into_iter()
            .map(|(e, d)| (e.id.clone(), d))
            .collect()
    }

    /// Call the LLM API
    async fn call_llm(&self, prompt: &str) -> Result<String> {
        let endpoint = self.config.api_endpoint.as_ref()
            .ok_or_else(|| SpatialLlmError::Config("No API endpoint configured".to_string()))?;

        let api_key = self.config.api_key.as_ref()
            .ok_or_else(|| SpatialLlmError::Config("No API key configured".to_string()))?;

        // OpenAI-compatible API format
        let request_body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "max_tokens": self.config.max_tokens,
            "temperature": self.config.temperature
        });

        let response = self.client
            .post(endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(SpatialLlmError::Api(format!("API error: {}", error_text)));
        }

        let response_json: serde_json::Value = response.json().await?;

        // Extract content from OpenAI-style response
        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(content)
    }

    /// Get entity count in the index
    pub async fn entity_count(&self) -> usize {
        self.index.read().await.len()
    }

    /// Clear all entities
    pub async fn clear(&self) {
        self.index.write().await.clear();
        *self.scene_graph.write().await = SceneGraph::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spatial_llm_basic() {
        let config = SpatialLlmConfig::default();
        let llm = SpatialLlm::new(config).await.unwrap();

        llm.add_entity(SpatialEntity::new("e1", "Tree").with_position(10.0, 0.0, 5.0)).await;
        llm.add_entity(SpatialEntity::new("e2", "Rock").with_position(0.0, 0.0, 0.0)).await;

        assert_eq!(llm.entity_count().await, 2);

        let nearby = llm.find_near(0.0, 0.0, 0.0, 5.0).await;
        assert_eq!(nearby.len(), 1);
        assert_eq!(nearby[0], "e2");
    }
}
