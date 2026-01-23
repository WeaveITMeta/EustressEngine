//! EmbedvecExportTarget - Routes MCP entity data to the vector store
//!
//! ## Table of Contents
//! 1. EmbedvecExportTarget - ExportTarget implementation for embedvec
//! 2. OntologyAwareExportTarget - Exports with ontology-aware indexing
//! 3. TrainingRecord - Training data format for spatial LLM

use crate::error::McpResult;
use crate::protocol::EepExportRecord;
use crate::router::ExportTarget;
use eustress_embedvec::{
    EmbeddingMetadata, OntologyIndex, OntologyTree, PropertyEmbedder, SimpleHashEmbedder,
    SpatialContextEmbedder, SpatialFeatures,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Export target that writes entity embeddings to a vector store
/// 
/// This bridges the MCP entity export pipeline with the embedvec vector database,
/// enabling semantic search over world entities and training data collection
/// for spatial LLM models.
pub struct EmbedvecExportTarget {
    name: String,
    /// Dimension of embeddings
    dimension: usize,
    /// Collected training records (for batch export)
    training_records: Arc<RwLock<Vec<TrainingRecord>>>,
    /// Output directory for training data
    output_dir: Option<PathBuf>,
    /// Whether to auto-flush on each export
    auto_flush: bool,
    /// Flush threshold (number of records before auto-flush)
    flush_threshold: usize,
    /// Embedder for generating vectors
    embedder: Arc<dyn PropertyEmbedder + Send + Sync>,
    /// Spatial embedder for position-aware embeddings
    spatial_embedder: Arc<SpatialContextEmbedder>,
}

/// Training record for spatial LLM with embedding
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TrainingRecord {
    /// Record ID
    pub id: String,
    /// Entity ID
    pub entity_id: String,
    /// Entity class (ontology path)
    pub entity_class: String,
    /// Entity name
    pub entity_name: Option<String>,
    /// Spatial position [x, y, z]
    pub position: Option<[f64; 3]>,
    /// Spatial rotation [x, y, z] (euler)
    pub rotation: Option<[f64; 3]>,
    /// Spatial scale [x, y, z]
    pub scale: Option<[f64; 3]>,
    /// Entity properties (class-specific data)
    pub properties: HashMap<String, serde_json::Value>,
    /// Tags (categorization labels)
    pub tags: Vec<String>,
    /// Attributes (key-value metadata)
    pub attributes: HashMap<String, serde_json::Value>,
    /// Parameters (domain→key→value hierarchical config)
    pub parameters: HashMap<String, HashMap<String, serde_json::Value>>,
    /// Hierarchy path (scene graph)
    pub hierarchy: Vec<String>,
    /// Generated embedding vector
    pub embedding: Vec<f32>,
    /// Timestamp
    pub timestamp: i64,
}

impl EmbedvecExportTarget {
    /// Create a new embedvec export target
    pub fn new(name: impl Into<String>, dimension: usize) -> Self {
        Self {
            name: name.into(),
            dimension,
            training_records: Arc::new(RwLock::new(Vec::new())),
            output_dir: None,
            auto_flush: false,
            flush_threshold: 1000,
            embedder: Arc::new(SimpleHashEmbedder::new(dimension)),
            spatial_embedder: Arc::new(SpatialContextEmbedder::new(dimension)),
        }
    }

    /// Create with custom embedder
    pub fn with_embedder<E: PropertyEmbedder + Send + Sync + 'static>(
        mut self,
        embedder: E,
    ) -> Self {
        self.embedder = Arc::new(embedder);
        self
    }

    /// Generate embedding from EEP record
    fn generate_embedding(&self, record: &EepExportRecord) -> Option<Vec<f32>> {
        // Combine all entity data for embedding
        let mut combined_props = record.entity.properties.clone();

        // Add class and tags
        combined_props.insert("_class".to_string(), serde_json::json!(record.entity.class));
        combined_props.insert("_tags".to_string(), serde_json::json!(record.entity.tags));

        if let Some(name) = &record.entity.name {
            combined_props.insert("_name".to_string(), serde_json::json!(name));
        }

        // Add attributes (prefixed to avoid collision)
        for (key, value) in &record.entity.attributes {
            combined_props.insert(format!("_attr_{}", key), value.clone());
        }

        // Add flattened parameters (domain.key format)
        for (domain, params) in &record.entity.parameters {
            for (key, value) in params {
                combined_props.insert(format!("_param_{}_{}", domain, key), value.clone());
            }
        }

        // Generate base embedding from combined properties
        let base_embedding = self.embedder.embed_properties(&combined_props).ok()?;

        // If we have spatial data, blend with spatial embedding
        if let Some(transform) = &record.entity.transform {
            if let Some(spatial_emb) = self.generate_spatial_embedding(transform) {
                // Blend: 70% property, 30% spatial
                let blended: Vec<f32> = base_embedding
                    .iter()
                    .zip(spatial_emb.iter())
                    .map(|(p, s)| p * 0.7 + s * 0.3)
                    .collect();
                return Some(blended);
            }
        }

        Some(base_embedding)
    }

    /// Generate spatial embedding from transform
    fn generate_spatial_embedding(
        &self,
        transform: &HashMap<String, serde_json::Value>,
    ) -> Option<Vec<f32>> {
        let position = transform.get("position").and_then(|p| {
            if let serde_json::Value::Array(arr) = p {
                if arr.len() >= 3 {
                    Some([
                        arr[0].as_f64().unwrap_or(0.0) as f32,
                        arr[1].as_f64().unwrap_or(0.0) as f32,
                        arr[2].as_f64().unwrap_or(0.0) as f32,
                    ])
                } else {
                    None
                }
            } else {
                None
            }
        })?;

        let features = SpatialFeatures {
            position,
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            distance_to_origin: (position[0].powi(2) + position[1].powi(2) + position[2].powi(2)).sqrt(),
            height: position[1],
            position_bucket: [
                (position[0] / 10.0).floor() as i32,
                (position[1] / 10.0).floor() as i32,
                (position[2] / 10.0).floor() as i32,
            ],
            neighbor_count: 0,
            avg_neighbor_distance: 0.0,
            nearest_neighbor_distance: f32::MAX,
            parent_class: None,
            hierarchy_depth: 0,
            tags: Vec::new(),
        };

        self.spatial_embedder.embed_spatial(&features).ok()
    }

    /// Set output directory for training data
    pub fn with_output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.output_dir = Some(dir.into());
        self
    }

    /// Enable auto-flush
    pub fn with_auto_flush(mut self, threshold: usize) -> Self {
        self.auto_flush = true;
        self.flush_threshold = threshold;
        self
    }

    /// Get the number of pending training records
    pub async fn pending_count(&self) -> usize {
        self.training_records.read().await.len()
    }

    /// Flush training records to disk
    pub async fn flush(&self) -> McpResult<usize> {
        let mut records = self.training_records.write().await;
        let count = records.len();

        if count == 0 {
            return Ok(0);
        }

        if let Some(dir) = &self.output_dir {
            // Ensure directory exists
            tokio::fs::create_dir_all(dir).await
                .map_err(|e| crate::error::McpError::Io(e))?;

            // Write records to JSONL file
            let filename = format!(
                "training_data_{}.jsonl",
                chrono::Utc::now().format("%Y%m%d_%H%M%S")
            );
            let path = dir.join(filename);

            let mut content = String::new();
            for record in records.iter() {
                let json = serde_json::to_string(record)
                    .map_err(|e| crate::error::McpError::Serialization(e))?;
                content.push_str(&json);
                content.push('\n');
            }

            tokio::fs::write(&path, content).await
                .map_err(|e| crate::error::McpError::Io(e))?;

            tracing::info!(
                path = %path.display(),
                count = count,
                "Flushed training records to disk"
            );
        }

        records.clear();
        Ok(count)
    }

    /// Convert EEP record to training record with embedding
    fn to_training_record(&self, record: &EepExportRecord, embedding: Vec<f32>) -> TrainingRecord {
        // Extract position from transform
        let position = record.entity.transform.as_ref().and_then(|t| {
            t.get("position").and_then(|p| {
                if let serde_json::Value::Array(arr) = p {
                    if arr.len() >= 3 {
                        Some([
                            arr[0].as_f64().unwrap_or(0.0),
                            arr[1].as_f64().unwrap_or(0.0),
                            arr[2].as_f64().unwrap_or(0.0),
                        ])
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        });

        // Extract rotation from transform
        let rotation = record.entity.transform.as_ref().and_then(|t| {
            t.get("rotation").and_then(|r| {
                if let serde_json::Value::Array(arr) = r {
                    if arr.len() >= 3 {
                        Some([
                            arr[0].as_f64().unwrap_or(0.0),
                            arr[1].as_f64().unwrap_or(0.0),
                            arr[2].as_f64().unwrap_or(0.0),
                        ])
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        });

        // Extract scale from transform
        let scale = record.entity.transform.as_ref().and_then(|t| {
            t.get("scale").and_then(|s| {
                if let serde_json::Value::Array(arr) = s {
                    if arr.len() >= 3 {
                        Some([
                            arr[0].as_f64().unwrap_or(1.0),
                            arr[1].as_f64().unwrap_or(1.0),
                            arr[2].as_f64().unwrap_or(1.0),
                        ])
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        });

        TrainingRecord {
            id: uuid::Uuid::new_v4().to_string(),
            entity_id: record.entity.id.clone(),
            entity_class: record.entity.class.clone(),
            entity_name: record.entity.name.clone(),
            position,
            rotation,
            scale,
            properties: record.entity.properties.clone(),
            tags: record.entity.tags.clone(),
            attributes: record.entity.attributes.clone(),
            parameters: record.entity.parameters.clone(),
            hierarchy: record.hierarchy.iter().map(|h| h.name.clone()).collect(),
            embedding,
            timestamp: record.timestamp.timestamp(),
        }
    }
}

// ============================================================================
// Ontology-Aware Export Target
// ============================================================================

/// Export target that writes to OntologyIndex for hierarchical AI retrieval
pub struct OntologyAwareExportTarget {
    name: String,
    /// Ontology index for hierarchical storage
    index: Arc<RwLock<OntologyIndex>>,
    /// Embedder for generating vectors
    embedder: Arc<dyn PropertyEmbedder + Send + Sync>,
    /// Spatial embedder
    spatial_embedder: Arc<SpatialContextEmbedder>,
    /// Stats
    exported_count: Arc<RwLock<u64>>,
}

impl OntologyAwareExportTarget {
    /// Create a new ontology-aware export target
    pub fn new(name: impl Into<String>, dimension: usize) -> Self {
        Self {
            name: name.into(),
            index: Arc::new(RwLock::new(OntologyIndex::with_eustress_base(dimension))),
            embedder: Arc::new(SimpleHashEmbedder::new(dimension)),
            spatial_embedder: Arc::new(SpatialContextEmbedder::new(dimension)),
            exported_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Create with custom ontology
    pub fn with_ontology(name: impl Into<String>, ontology: OntologyTree, dimension: usize) -> Self {
        Self {
            name: name.into(),
            index: Arc::new(RwLock::new(OntologyIndex::new(ontology, dimension))),
            embedder: Arc::new(SimpleHashEmbedder::new(dimension)),
            spatial_embedder: Arc::new(SpatialContextEmbedder::new(dimension)),
            exported_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Get the ontology index for querying
    pub fn index(&self) -> Arc<RwLock<OntologyIndex>> {
        Arc::clone(&self.index)
    }

    /// Get export count
    pub async fn exported_count(&self) -> u64 {
        *self.exported_count.read().await
    }

    /// Map entity class to ontology path
    fn map_class_to_ontology_path(&self, class: &str) -> String {
        // Try to find in ontology, otherwise default to Entity/Data
        // This is a simple heuristic - could be made smarter
        match class.to_lowercase().as_str() {
            "player" => "Entity/Spatial/Actor/Character/Player".to_string(),
            "npc" | "character" => "Entity/Spatial/Actor/Character/NPC".to_string(),
            "tree" | "bush" | "plant" | "flower" => "Entity/Spatial/Prop/Vegetation".to_string(),
            "building" | "house" | "structure" => "Entity/Spatial/Prop/Structure".to_string(),
            "light" | "lamp" | "torch" => "Entity/Spatial/Light".to_string(),
            "trigger" | "zone" | "area" => "Entity/Spatial/Volume".to_string(),
            _ if class.contains("Actor") => "Entity/Spatial/Actor".to_string(),
            _ if class.contains("Prop") => "Entity/Spatial/Prop".to_string(),
            _ => format!("Entity/Spatial/Prop"), // Default to Prop
        }
    }

    /// Generate embedding from EEP record (includes tags, attributes, parameters)
    fn generate_embedding(&self, record: &EepExportRecord) -> Option<Vec<f32>> {
        let mut combined_props = record.entity.properties.clone();

        // Add class and tags
        combined_props.insert("_class".to_string(), serde_json::json!(record.entity.class));
        combined_props.insert("_tags".to_string(), serde_json::json!(record.entity.tags));

        if let Some(name) = &record.entity.name {
            combined_props.insert("_name".to_string(), serde_json::json!(name));
        }

        // Add attributes (prefixed)
        for (key, value) in &record.entity.attributes {
            combined_props.insert(format!("_attr_{}", key), value.clone());
        }

        // Add flattened parameters (domain.key format)
        for (domain, params) in &record.entity.parameters {
            for (key, value) in params {
                combined_props.insert(format!("_param_{}_{}", domain, key), value.clone());
            }
        }

        let base_embedding = self.embedder.embed_properties(&combined_props).ok()?;

        if let Some(transform) = &record.entity.transform {
            if let Some(spatial_emb) = self.generate_spatial_embedding(transform) {
                let blended: Vec<f32> = base_embedding
                    .iter()
                    .zip(spatial_emb.iter())
                    .map(|(p, s)| p * 0.7 + s * 0.3)
                    .collect();
                return Some(blended);
            }
        }

        Some(base_embedding)
    }

    /// Generate spatial embedding
    fn generate_spatial_embedding(
        &self,
        transform: &HashMap<String, serde_json::Value>,
    ) -> Option<Vec<f32>> {
        let position = transform.get("position").and_then(|p| {
            if let serde_json::Value::Array(arr) = p {
                if arr.len() >= 3 {
                    Some([
                        arr[0].as_f64().unwrap_or(0.0) as f32,
                        arr[1].as_f64().unwrap_or(0.0) as f32,
                        arr[2].as_f64().unwrap_or(0.0) as f32,
                    ])
                } else {
                    None
                }
            } else {
                None
            }
        })?;

        let features = SpatialFeatures {
            position,
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            distance_to_origin: (position[0].powi(2) + position[1].powi(2) + position[2].powi(2)).sqrt(),
            height: position[1],
            position_bucket: [
                (position[0] / 10.0).floor() as i32,
                (position[1] / 10.0).floor() as i32,
                (position[2] / 10.0).floor() as i32,
            ],
            neighbor_count: 0,
            avg_neighbor_distance: 0.0,
            nearest_neighbor_distance: f32::MAX,
            parent_class: None,
            hierarchy_depth: 0,
            tags: Vec::new(),
        };

        self.spatial_embedder.embed_spatial(&features).ok()
    }
}

#[async_trait::async_trait]
impl ExportTarget for OntologyAwareExportTarget {
    fn name(&self) -> &str {
        &self.name
    }

    async fn export(&self, record: &EepExportRecord) -> McpResult<()> {
        // Only process records with AI consent
        if !record.consent.ai_training {
            return Ok(());
        }

        // Generate embedding
        let embedding = match self.generate_embedding(record) {
            Some(emb) => emb,
            None => {
                tracing::warn!(
                    entity_id = %record.entity.id,
                    "Failed to generate embedding"
                );
                return Ok(());
            }
        };

        // Map class to ontology path
        let ontology_path = self.map_class_to_ontology_path(&record.entity.class);

        // Create metadata with all entity data
        let mut metadata = EmbeddingMetadata::new();
        if let Some(name) = &record.entity.name {
            metadata = EmbeddingMetadata::with_name(name);
        }
        metadata.entity_class = Some(record.entity.class.clone());

        // Include properties
        metadata.properties = record.entity.properties.clone();

        // Include tags
        for tag in &record.entity.tags {
            metadata.tags.push(tag.clone());
        }

        // Include attributes (prefixed in properties)
        for (key, value) in &record.entity.attributes {
            metadata.properties.insert(format!("_attr_{}", key), value.clone());
        }

        // Include parameters (flattened in properties)
        for (domain, params) in &record.entity.parameters {
            for (key, value) in params {
                metadata.properties.insert(format!("_param_{}_{}", domain, key), value.clone());
            }
        }

        // Parse entity ID as UUID or generate new one
        let instance_id = uuid::Uuid::parse_str(&record.entity.id)
            .unwrap_or_else(|_| uuid::Uuid::new_v4());

        // Insert into ontology index
        // Note: We use a placeholder Entity here since we don't have the actual Bevy Entity
        // In a real integration, this would come from the ECS
        let placeholder_entity = bevy::prelude::Entity::from_bits(instance_id.as_u128() as u64);

        let mut index = self.index.write().await;
        if let Err(e) = index.insert(
            &ontology_path,
            placeholder_entity,
            instance_id,
            embedding,
            metadata,
        ) {
            tracing::warn!(
                entity_id = %record.entity.id,
                ontology_path = %ontology_path,
                error = %e,
                "Failed to insert into ontology index, using fallback path"
            );
            // Try with a fallback path
            let _ = index.insert(
                "Entity/Spatial/Prop",
                placeholder_entity,
                instance_id,
                self.generate_embedding(record).unwrap_or_default(),
                EmbeddingMetadata::with_name(&record.entity.id),
            );
        }

        // Update stats
        *self.exported_count.write().await += 1;

        tracing::debug!(
            entity_id = %record.entity.id,
            ontology_path = %ontology_path,
            "Exported entity to ontology index"
        );

        Ok(())
    }

    async fn health_check(&self) -> bool {
        true
    }
}

#[async_trait::async_trait]
impl ExportTarget for EmbedvecExportTarget {
    fn name(&self) -> &str {
        &self.name
    }

    async fn export(&self, record: &EepExportRecord) -> McpResult<()> {
        // Only process records with AI consent
        if !record.consent.ai_training {
            return Ok(());
        }

        // Generate embedding
        let embedding = self.generate_embedding(record).unwrap_or_else(|| {
            vec![0.0f32; self.dimension]
        });

        // Convert to training record with embedding
        let training_record = self.to_training_record(record, embedding);

        tracing::debug!(
            entity_id = %training_record.entity_id,
            entity_class = %training_record.entity_class,
            embedding_dim = training_record.embedding.len(),
            "Collected training record with embedding"
        );

        // Add to pending records
        let mut records = self.training_records.write().await;
        records.push(training_record);

        // Auto-flush if threshold reached
        if self.auto_flush && records.len() >= self.flush_threshold {
            drop(records); // Release lock before flush
            self.flush().await?;
        }

        Ok(())
    }

    async fn health_check(&self) -> bool {
        true
    }
}

/// Builder for EmbedvecExportTarget
pub struct EmbedvecExportTargetBuilder {
    name: String,
    dimension: usize,
    output_dir: Option<PathBuf>,
    auto_flush: bool,
    flush_threshold: usize,
}

impl EmbedvecExportTargetBuilder {
    /// Create a new builder
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            dimension: 256,
            output_dir: None,
            auto_flush: false,
            flush_threshold: 1000,
        }
    }

    /// Set embedding dimension
    pub fn dimension(mut self, dim: usize) -> Self {
        self.dimension = dim;
        self
    }

    /// Set output directory
    pub fn output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.output_dir = Some(dir.into());
        self
    }

    /// Enable auto-flush
    pub fn auto_flush(mut self, threshold: usize) -> Self {
        self.auto_flush = true;
        self.flush_threshold = threshold;
        self
    }

    /// Build the export target
    pub fn build(self) -> EmbedvecExportTarget {
        let mut target = EmbedvecExportTarget::new(self.name, self.dimension);
        if let Some(dir) = self.output_dir {
            target = target.with_output_dir(dir);
        }
        if self.auto_flush {
            target = target.with_auto_flush(self.flush_threshold);
        }
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_embedvec_export_target() {
        let target = EmbedvecExportTarget::new("test", 128);
        assert_eq!(target.name(), "test");
        assert_eq!(target.pending_count().await, 0);
    }
}
