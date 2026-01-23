//! Embedding strategies for converting properties to vectors
//!
//! ## Table of Contents
//! 1. PropertyEmbedder trait - Core abstraction for embedding strategies
//! 2. SimpleHashEmbedder - Fast hash-based embeddings (no ML model)
//! 3. ReflectPropertyEmbedder - Embedder using Bevy Reflect

use crate::error::Result;
use bevy::prelude::*;
use bevy::reflect::{PartialReflect, ReflectRef};
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Trait for converting properties to embedding vectors
pub trait PropertyEmbedder: Send + Sync + 'static {
    /// The dimension of embeddings produced by this embedder
    fn dimension(&self) -> usize;

    /// Embed a set of properties into a vector
    fn embed_properties(&self, properties: &HashMap<String, Value>) -> Result<Vec<f32>>;

    /// Embed a text query for similarity search
    fn embed_query(&self, query: &str) -> Result<Vec<f32>>;

    /// Embed a reflected value
    fn embed_reflect(&self, value: &dyn Reflect) -> Result<Vec<f32>> {
        let properties = reflect_to_properties(value)?;
        self.embed_properties(&properties)
    }
}

/// Simple hash-based embedder for fast, deterministic embeddings
/// Uses feature hashing (hashing trick) to convert properties to fixed-size vectors
#[derive(Clone, Debug)]
pub struct SimpleHashEmbedder {
    /// Output dimension
    dimension: usize,
    /// Seed for hashing
    seed: u64,
}

impl Default for SimpleHashEmbedder {
    fn default() -> Self {
        Self::new(128)
    }
}

impl SimpleHashEmbedder {
    /// Create a new hash embedder with the given dimension
    pub fn new(dimension: usize) -> Self {
        Self {
            dimension,
            seed: 0x517cc1b727220a95,
        }
    }

    /// Create with a custom seed
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Hash a string to a position and sign
    fn hash_feature(&self, feature: &str) -> (usize, f32) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.seed.hash(&mut hasher);
        feature.hash(&mut hasher);
        let hash = hasher.finish();

        let position = (hash as usize) % self.dimension;
        let sign = if (hash >> 63) == 0 { 1.0 } else { -1.0 };

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

impl PropertyEmbedder for SimpleHashEmbedder {
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed_properties(&self, properties: &HashMap<String, Value>) -> Result<Vec<f32>> {
        let mut embedding = vec![0.0f32; self.dimension];

        for (key, value) in properties {
            // Create features from key-value pairs
            let features = value_to_features(key, value);

            for feature in features {
                let (pos, sign) = self.hash_feature(&feature);
                embedding[pos] += sign;
            }
        }

        Self::normalize(&mut embedding);
        Ok(embedding)
    }

    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        let mut embedding = vec![0.0f32; self.dimension];

        // Tokenize query and hash each token
        for token in query.split_whitespace() {
            let (pos, sign) = self.hash_feature(token);
            embedding[pos] += sign;

            // Also hash lowercased version
            let lower = token.to_lowercase();
            if lower != token {
                let (pos, sign) = self.hash_feature(&lower);
                embedding[pos] += sign * 0.5;
            }
        }

        Self::normalize(&mut embedding);
        Ok(embedding)
    }
}

/// Embedder that uses Bevy's Reflect system to extract properties
pub struct ReflectPropertyEmbedder {
    /// Inner embedder for the actual embedding
    inner: Box<dyn PropertyEmbedder>,
}

impl ReflectPropertyEmbedder {
    /// Create a new reflect embedder with the given inner embedder
    pub fn new<E: PropertyEmbedder>(embedder: E) -> Self {
        Self {
            inner: Box::new(embedder),
        }
    }

    /// Create with default hash embedder
    pub fn with_hash_embedder(dimension: usize) -> Self {
        Self::new(SimpleHashEmbedder::new(dimension))
    }
}

impl PropertyEmbedder for ReflectPropertyEmbedder {
    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn embed_properties(&self, properties: &HashMap<String, Value>) -> Result<Vec<f32>> {
        self.inner.embed_properties(properties)
    }

    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        self.inner.embed_query(query)
    }

    fn embed_reflect(&self, value: &dyn Reflect) -> Result<Vec<f32>> {
        let properties = reflect_to_properties(value)?;
        self.inner.embed_properties(&properties)
    }
}

/// Convert a JSON value to a list of features for hashing
fn value_to_features(key: &str, value: &Value) -> Vec<String> {
    let mut features = Vec::new();

    match value {
        Value::Null => {
            features.push(format!("{}=null", key));
        }
        Value::Bool(b) => {
            features.push(format!("{}={}", key, b));
        }
        Value::Number(n) => {
            // Quantize numbers to reduce sparsity
            if let Some(f) = n.as_f64() {
                let quantized = (f * 10.0).round() / 10.0;
                features.push(format!("{}={:.1}", key, quantized));
                // Also add key presence
                features.push(key.to_string());
            } else if let Some(i) = n.as_i64() {
                features.push(format!("{}={}", key, i));
                features.push(key.to_string());
            }
        }
        Value::String(s) => {
            features.push(format!("{}={}", key, s));
            // Also tokenize the string
            for token in s.split_whitespace() {
                features.push(format!("{}:{}", key, token.to_lowercase()));
            }
        }
        Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                let sub_key = format!("{}[{}]", key, i);
                features.extend(value_to_features(&sub_key, item));
            }
        }
        Value::Object(obj) => {
            for (k, v) in obj {
                let sub_key = format!("{}.{}", key, k);
                features.extend(value_to_features(&sub_key, v));
            }
        }
    }

    features
}

/// Convert a Reflect value to a property map
pub fn reflect_to_properties(value: &dyn PartialReflect) -> Result<HashMap<String, Value>> {
    let mut properties = HashMap::new();

    match value.reflect_ref() {
        ReflectRef::Struct(s) => {
            for i in 0..s.field_len() {
                if let Some(name) = s.name_at(i) {
                    if let Some(field) = s.field_at(i) {
                        if let Some(json_value) = partial_reflect_to_json(field) {
                            properties.insert(name.to_string(), json_value);
                        }
                    }
                }
            }
        }
        ReflectRef::TupleStruct(ts) => {
            for i in 0..ts.field_len() {
                if let Some(field) = ts.field(i) {
                    if let Some(json_value) = partial_reflect_to_json(field) {
                        properties.insert(format!("field_{}", i), json_value);
                    }
                }
            }
        }
        ReflectRef::Tuple(t) => {
            for i in 0..t.field_len() {
                if let Some(field) = t.field(i) {
                    if let Some(json_value) = partial_reflect_to_json(field) {
                        properties.insert(format!("field_{}", i), json_value);
                    }
                }
            }
        }
        ReflectRef::List(l) => {
            let mut arr = Vec::new();
            for i in 0..l.len() {
                if let Some(item) = l.get(i) {
                    if let Some(json_value) = partial_reflect_to_json(item) {
                        arr.push(json_value);
                    }
                }
            }
            properties.insert("items".to_string(), Value::Array(arr));
        }
        ReflectRef::Map(m) => {
            // Iterate using iter() for Bevy 0.17 Map API
            for (key, val) in m.iter() {
                let key_str = format!("{:?}", key);
                if let Some(json_value) = partial_reflect_to_json(val) {
                    properties.insert(key_str, json_value);
                }
            }
        }
        ReflectRef::Enum(e) => {
            properties.insert("variant".to_string(), Value::String(e.variant_name().to_string()));
        }
        ReflectRef::Opaque(_) => {
            // Can't introspect opaque types
        }
        _ => {
            // Handle any other variants (Set, etc.)
        }
    }

    Ok(properties)
}

/// Convert a PartialReflect value to JSON (best effort)
fn partial_reflect_to_json(value: &dyn PartialReflect) -> Option<Value> {
    // Try to get full Reflect first for downcasting
    if let Some(reflect) = value.try_as_reflect() {
        if let Some(v) = reflect.downcast_ref::<f32>() {
            return Some(Value::Number(serde_json::Number::from_f64(*v as f64)?));
        }
        if let Some(v) = reflect.downcast_ref::<f64>() {
            return Some(Value::Number(serde_json::Number::from_f64(*v)?));
        }
        if let Some(v) = reflect.downcast_ref::<i32>() {
            return Some(Value::Number((*v).into()));
        }
        if let Some(v) = reflect.downcast_ref::<i64>() {
            return Some(Value::Number((*v).into()));
        }
        if let Some(v) = reflect.downcast_ref::<u32>() {
            return Some(Value::Number((*v).into()));
        }
        if let Some(v) = reflect.downcast_ref::<u64>() {
            return Some(Value::Number((*v).into()));
        }
        if let Some(v) = reflect.downcast_ref::<bool>() {
            return Some(Value::Bool(*v));
        }
        if let Some(v) = reflect.downcast_ref::<String>() {
            return Some(Value::String(v.clone()));
        }
    }

    // For complex types, use debug representation
    Some(Value::String(format!("{:?}", value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_hash_embedder() {
        let embedder = SimpleHashEmbedder::new(64);

        let mut props = HashMap::new();
        props.insert("health".to_string(), Value::Number(100.into()));
        props.insert("name".to_string(), Value::String("Player".to_string()));

        let embedding = embedder.embed_properties(&props).unwrap();
        assert_eq!(embedding.len(), 64);

        // Check normalization
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_query_embedding() {
        let embedder = SimpleHashEmbedder::new(64);

        let query_emb = embedder.embed_query("high damage warrior").unwrap();
        assert_eq!(query_emb.len(), 64);
    }
}
