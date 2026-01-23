//! Spatial context embedding for world-building AI
//!
//! ## Table of Contents
//! 1. SpatialContextEmbedder - Encodes Transform, neighbors, scene graph
//! 2. SpatialFeatures - Extracted spatial features for embedding
//! 3. NeighborContext - Nearby entity information

use crate::embedder::PropertyEmbedder;
use crate::error::Result;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Features extracted from spatial context
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SpatialFeatures {
    /// Entity position (world space)
    pub position: [f32; 3],
    /// Entity rotation (euler angles in radians)
    pub rotation: [f32; 3],
    /// Entity scale
    pub scale: [f32; 3],
    /// Distance to origin
    pub distance_to_origin: f32,
    /// Height (Y coordinate)
    pub height: f32,
    /// Quantized position bucket for locality-sensitive hashing
    pub position_bucket: [i32; 3],
    /// Neighbor count within radius
    pub neighbor_count: u32,
    /// Average distance to neighbors
    pub avg_neighbor_distance: f32,
    /// Nearest neighbor distance
    pub nearest_neighbor_distance: f32,
    /// Parent entity class (if any)
    pub parent_class: Option<String>,
    /// Depth in scene hierarchy
    pub hierarchy_depth: u32,
    /// Tags from the entity
    pub tags: Vec<String>,
}

impl SpatialFeatures {
    /// Create from a Transform component
    pub fn from_transform(transform: &Transform) -> Self {
        let pos = transform.translation;
        let (rx, ry, rz) = transform.rotation.to_euler(EulerRot::XYZ);
        let scale = transform.scale;

        // Quantize position to 10-unit buckets for LSH
        let bucket_size = 10.0;
        let position_bucket = [
            (pos.x / bucket_size).floor() as i32,
            (pos.y / bucket_size).floor() as i32,
            (pos.z / bucket_size).floor() as i32,
        ];

        Self {
            position: [pos.x, pos.y, pos.z],
            rotation: [rx, ry, rz],
            scale: [scale.x, scale.y, scale.z],
            distance_to_origin: pos.length(),
            height: pos.y,
            position_bucket,
            neighbor_count: 0,
            avg_neighbor_distance: 0.0,
            nearest_neighbor_distance: f32::MAX,
            parent_class: None,
            hierarchy_depth: 0,
            tags: Vec::new(),
        }
    }

    /// Add neighbor information
    pub fn with_neighbors(mut self, distances: &[f32]) -> Self {
        self.neighbor_count = distances.len() as u32;
        if !distances.is_empty() {
            self.avg_neighbor_distance = distances.iter().sum::<f32>() / distances.len() as f32;
            self.nearest_neighbor_distance = distances.iter().cloned().fold(f32::MAX, f32::min);
        }
        self
    }

    /// Add hierarchy information
    pub fn with_hierarchy(mut self, parent_class: Option<String>, depth: u32) -> Self {
        self.parent_class = parent_class;
        self.hierarchy_depth = depth;
        self
    }

    /// Add tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Convert to property map for embedding
    pub fn to_properties(&self) -> HashMap<String, Value> {
        let mut props = HashMap::new();

        props.insert("pos_x".to_string(), Value::Number(serde_json::Number::from_f64(self.position[0] as f64).unwrap_or(0.into())));
        props.insert("pos_y".to_string(), Value::Number(serde_json::Number::from_f64(self.position[1] as f64).unwrap_or(0.into())));
        props.insert("pos_z".to_string(), Value::Number(serde_json::Number::from_f64(self.position[2] as f64).unwrap_or(0.into())));

        props.insert("rot_x".to_string(), Value::Number(serde_json::Number::from_f64(self.rotation[0] as f64).unwrap_or(0.into())));
        props.insert("rot_y".to_string(), Value::Number(serde_json::Number::from_f64(self.rotation[1] as f64).unwrap_or(0.into())));
        props.insert("rot_z".to_string(), Value::Number(serde_json::Number::from_f64(self.rotation[2] as f64).unwrap_or(0.into())));

        props.insert("scale_x".to_string(), Value::Number(serde_json::Number::from_f64(self.scale[0] as f64).unwrap_or(0.into())));
        props.insert("scale_y".to_string(), Value::Number(serde_json::Number::from_f64(self.scale[1] as f64).unwrap_or(0.into())));
        props.insert("scale_z".to_string(), Value::Number(serde_json::Number::from_f64(self.scale[2] as f64).unwrap_or(0.into())));

        props.insert("distance_to_origin".to_string(), Value::Number(serde_json::Number::from_f64(self.distance_to_origin as f64).unwrap_or(0.into())));
        props.insert("height".to_string(), Value::Number(serde_json::Number::from_f64(self.height as f64).unwrap_or(0.into())));

        props.insert("bucket_x".to_string(), Value::Number(self.position_bucket[0].into()));
        props.insert("bucket_y".to_string(), Value::Number(self.position_bucket[1].into()));
        props.insert("bucket_z".to_string(), Value::Number(self.position_bucket[2].into()));

        props.insert("neighbor_count".to_string(), Value::Number(self.neighbor_count.into()));
        props.insert("avg_neighbor_dist".to_string(), Value::Number(serde_json::Number::from_f64(self.avg_neighbor_distance as f64).unwrap_or(0.into())));
        props.insert("nearest_neighbor_dist".to_string(), Value::Number(serde_json::Number::from_f64(self.nearest_neighbor_distance as f64).unwrap_or(0.into())));

        props.insert("hierarchy_depth".to_string(), Value::Number(self.hierarchy_depth.into()));

        if let Some(parent) = &self.parent_class {
            props.insert("parent_class".to_string(), Value::String(parent.clone()));
        }

        if !self.tags.is_empty() {
            props.insert("tags".to_string(), Value::Array(
                self.tags.iter().map(|t| Value::String(t.clone())).collect()
            ));
        }

        props
    }
}

/// Embedder specialized for spatial context
/// Combines position, rotation, scale, and neighbor information
pub struct SpatialContextEmbedder {
    /// Output dimension
    dimension: usize,
    /// Seed for hashing
    seed: u64,
    /// Weight for position features
    position_weight: f32,
    /// Weight for neighbor features
    neighbor_weight: f32,
    /// Weight for hierarchy features
    hierarchy_weight: f32,
}

impl Default for SpatialContextEmbedder {
    fn default() -> Self {
        Self::new(256)
    }
}

impl SpatialContextEmbedder {
    /// Create a new spatial embedder with the given dimension
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            seed: 0x7a3b9c1d5e8f2a4b,
            position_weight: 1.0,
            neighbor_weight: 0.5,
            hierarchy_weight: 0.3,
        }
    }

    /// Set feature weights
    pub fn with_weights(mut self, position: f32, neighbor: f32, hierarchy: f32) -> Self {
        self.position_weight = position;
        self.neighbor_weight = neighbor;
        self.hierarchy_weight = hierarchy;
        self
    }

    /// Embed spatial features directly
    pub fn embed_spatial(&self, features: &SpatialFeatures) -> Result<Vec<f32>> {
        let props = features.to_properties();
        self.embed_properties(&props)
    }

    /// Hash a string to a position and sign
    fn hash_feature(&self, feature: &str, weight: f32) -> (usize, f32) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.seed.hash(&mut hasher);
        feature.hash(&mut hasher);
        let hash = hasher.finish();

        let position = (hash as usize) % self.dimension;
        let sign = if (hash >> 63) == 0 { weight } else { -weight };

        (position, sign)
    }

    /// Normalize a vector to unit length
    fn normalize(vec: &mut [f32]) {
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-10 {
            for x in vec.iter_mut() {
                *x /= norm;
            }
        }
    }
}

impl PropertyEmbedder for SpatialContextEmbedder {
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed_properties(&self, properties: &HashMap<String, Value>) -> Result<Vec<f32>> {
        let mut embedding = vec![0.0f32; self.dimension];

        for (key, value) in properties {
            // Determine weight based on feature type
            let weight = if key.starts_with("pos_") || key.starts_with("bucket_") {
                self.position_weight
            } else if key.starts_with("neighbor") || key.starts_with("nearest") || key.starts_with("avg_") {
                self.neighbor_weight
            } else if key.starts_with("hierarchy") || key.starts_with("parent") {
                self.hierarchy_weight
            } else {
                1.0
            };

            // Create features from key-value pairs
            let features = spatial_value_to_features(key, value);

            for feature in features {
                let (pos, sign) = self.hash_feature(&feature, weight);
                embedding[pos] += sign;
            }
        }

        Self::normalize(&mut embedding);
        Ok(embedding)
    }

    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        let mut embedding = vec![0.0f32; self.dimension];

        // Parse spatial keywords
        let spatial_keywords = [
            ("near", 1.5),
            ("far", 1.5),
            ("above", 1.2),
            ("below", 1.2),
            ("left", 1.2),
            ("right", 1.2),
            ("behind", 1.2),
            ("front", 1.2),
            ("high", 1.0),
            ("low", 1.0),
            ("tall", 1.0),
            ("short", 1.0),
            ("large", 0.8),
            ("small", 0.8),
            ("cluster", 1.3),
            ("isolated", 1.3),
            ("dense", 1.3),
            ("sparse", 1.3),
        ];

        for token in query.split_whitespace() {
            let lower = token.to_lowercase();

            // Check for spatial keywords with boosted weight
            let weight = spatial_keywords
                .iter()
                .find(|(kw, _)| lower.contains(kw))
                .map(|(_, w)| *w)
                .unwrap_or(1.0);

            let (pos, sign) = self.hash_feature(&lower, weight);
            embedding[pos] += sign;
        }

        Self::normalize(&mut embedding);
        Ok(embedding)
    }
}

/// Convert spatial values to features for hashing
fn spatial_value_to_features(key: &str, value: &Value) -> Vec<String> {
    let mut features = Vec::new();

    match value {
        Value::Number(n) => {
            // Quantize numbers for spatial features
            if let Some(f) = n.as_f64() {
                // Different quantization for different feature types
                let quantized = if key.starts_with("pos_") || key.starts_with("bucket_") {
                    // Position: 1-unit precision
                    (f).round()
                } else if key.starts_with("rot_") {
                    // Rotation: 15-degree buckets
                    (f * 180.0 / std::f64::consts::PI / 15.0).round() * 15.0
                } else if key.starts_with("scale_") {
                    // Scale: 0.5 precision
                    (f * 2.0).round() / 2.0
                } else if key.contains("dist") {
                    // Distance: 5-unit buckets
                    (f / 5.0).round() * 5.0
                } else {
                    (f * 10.0).round() / 10.0
                };

                features.push(format!("{}={:.0}", key, quantized));
                features.push(key.to_string());
            }
        }
        Value::String(s) => {
            features.push(format!("{}={}", key, s));
            for token in s.split_whitespace() {
                features.push(format!("{}:{}", key, token.to_lowercase()));
            }
        }
        Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                if let Value::String(s) = item {
                    features.push(format!("{}[{}]={}", key, i, s));
                    features.push(format!("tag:{}", s.to_lowercase()));
                }
            }
        }
        _ => {
            features.push(format!("{}={:?}", key, value));
        }
    }

    features
}

/// Training data record for spatial LLM
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpatialTrainingRecord {
    /// Unique record ID
    pub id: String,
    /// Entity embedding
    pub embedding: Vec<f32>,
    /// Spatial features
    pub features: SpatialFeatures,
    /// Entity class/type
    pub entity_class: String,
    /// Entity name
    pub entity_name: Option<String>,
    /// Additional properties
    pub properties: HashMap<String, Value>,
    /// Timestamp
    pub timestamp: u64,
}

impl SpatialTrainingRecord {
    /// Create a new training record
    pub fn new(
        embedding: Vec<f32>,
        features: SpatialFeatures,
        entity_class: String,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            embedding,
            features,
            entity_class,
            entity_name: None,
            properties: HashMap::new(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }

    /// Add entity name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.entity_name = Some(name.into());
        self
    }

    /// Add properties
    pub fn with_properties(mut self, properties: HashMap<String, Value>) -> Self {
        self.properties = properties;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_features_from_transform() {
        let transform = Transform::from_xyz(10.0, 5.0, -20.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));

        let features = SpatialFeatures::from_transform(&transform);

        assert_eq!(features.position[0], 10.0);
        assert_eq!(features.position[1], 5.0);
        assert_eq!(features.position[2], -20.0);
        assert_eq!(features.position_bucket, [1, 0, -2]); // 10/10, 5/10, -20/10
    }

    #[test]
    fn test_spatial_embedder() {
        let embedder = SpatialContextEmbedder::new(128);

        let features = SpatialFeatures::from_transform(&Transform::from_xyz(10.0, 5.0, -20.0))
            .with_neighbors(&[5.0, 10.0, 15.0])
            .with_tags(vec!["tree".to_string(), "vegetation".to_string()]);

        let embedding = embedder.embed_spatial(&features).unwrap();
        assert_eq!(embedding.len(), 128);

        // Check normalization
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_spatial_query_embedding() {
        let embedder = SpatialContextEmbedder::new(128);

        let query_emb = embedder.embed_query("find trees near the player").unwrap();
        assert_eq!(query_emb.len(), 128);
    }
}
