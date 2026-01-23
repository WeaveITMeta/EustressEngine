//! # Attributes Module
//!
//! Custom attributes and tags for entities.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Plugin for attributes system
pub struct AttributesPlugin;

impl Plugin for AttributesPlugin {
    fn build(&self, _app: &mut App) {
        // TODO: Register systems
    }
}

/// Collection service for managing tagged entities
#[derive(Resource, Default)]
pub struct CollectionService {
    collections: HashMap<String, Vec<Entity>>,
}

impl CollectionService {
    pub fn add_to_collection(&mut self, name: &str, entity: Entity) {
        self.collections.entry(name.to_string()).or_default().push(entity);
    }
    
    pub fn get_collection(&self, name: &str) -> Option<&Vec<Entity>> {
        self.collections.get(name)
    }
    
    pub fn remove_from_collection(&mut self, name: &str, entity: Entity) {
        if let Some(entities) = self.collections.get_mut(name) {
            entities.retain(|e| *e != entity);
        }
    }
}

/// Tags component for entity categorization
#[derive(Component, Default, Clone, Debug, Serialize, Deserialize)]
pub struct Tags(pub Vec<String>);

impl Tags {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    
    pub fn add(&mut self, tag: &str) {
        if !self.0.contains(&tag.to_string()) {
            self.0.push(tag.to_string());
        }
    }
    
    pub fn remove(&mut self, tag: &str) {
        self.0.retain(|t| t != tag);
    }
    
    pub fn has(&self, tag: &str) -> bool {
        self.0.contains(&tag.to_string())
    }
}

/// Attributes component for custom key-value data
#[derive(Component, Default, Clone, Debug, Serialize, Deserialize)]
pub struct Attributes {
    pub values: HashMap<String, AttributeValue>,
}

impl Attributes {
    pub fn new() -> Self {
        Self { values: HashMap::new() }
    }
    
    pub fn set(&mut self, key: &str, value: AttributeValue) {
        self.values.insert(key.to_string(), value);
    }
    
    pub fn get(&self, key: &str) -> Option<&AttributeValue> {
        self.values.get(key)
    }
    
    pub fn remove(&mut self, key: &str) -> Option<AttributeValue> {
        self.values.remove(key)
    }
}

/// Attribute value types
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum AttributeValue {
    String(String),
    Number(f64),
    Int(i64),
    Bool(bool),
    Vector3(Vec3),
    Color3(Color),
    CFrame(Transform),
    Object(Option<Entity>),
    NumberSequence(Vec<NumberSequenceKeypoint>),
    ColorSequence(Vec<ColorSequenceKeypoint>),
}

/// String value wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StringValue(pub String);

/// Number value wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NumberValue(pub f64);

/// Int value wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntValue(pub i64);

/// Bool value wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoolValue(pub bool);

/// Vector3 value wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vector3Value(pub Vec3);

/// Color3 value wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Color3Value(pub Color);

/// CFrame value wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CFrameValue(pub Transform);

/// Object reference value wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectValue(pub Option<Entity>);

/// Keypoint for number sequences
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NumberSequenceKeypoint {
    pub time: f32,
    pub value: f32,
    pub envelope: f32,
}

/// Keypoint for color sequences
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ColorSequenceKeypoint {
    pub time: f32,
    pub color: Color,
}
