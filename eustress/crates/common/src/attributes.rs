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
    
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }
    
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    
    pub fn len(&self) -> usize {
        self.0.len()
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
    
    pub fn iter(&self) -> impl Iterator<Item = (&String, &AttributeValue)> {
        self.values.iter()
    }
    
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    
    pub fn len(&self) -> usize {
        self.values.len()
    }
}

/// Attribute value types
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum AttributeValue {
    String(String),
    Number(f64),
    Int(i64),
    Bool(bool),
    Vector2(Vec2),
    Vector3(Vec3),
    Color(Color),
    Color3(Color),
    BrickColor(u32),
    CFrame(Transform),
    Object(Option<Entity>),
    EntityRef(Option<Entity>),
    UDim2 { x_scale: f32, x_offset: f32, y_scale: f32, y_offset: f32 },
    Rect { min: Vec2, max: Vec2 },
    Font { family: String, weight: u32, style: String },
    NumberRange { min: f64, max: f64 },
    NumberSequence(Vec<NumberSequenceKeypoint>),
    ColorSequence(Vec<ColorSequenceKeypoint>),
}

impl AttributeValue {
    /// Get the type name of this attribute value
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::String(_) => "String",
            Self::Number(_) => "Number",
            Self::Int(_) => "Int",
            Self::Bool(_) => "Bool",
            Self::Vector2(_) => "Vector2",
            Self::Vector3(_) => "Vector3",
            Self::Color(_) => "Color",
            Self::Color3(_) => "Color3",
            Self::BrickColor(_) => "BrickColor",
            Self::CFrame(_) => "CFrame",
            Self::Object(_) => "Object",
            Self::EntityRef(_) => "EntityRef",
            Self::UDim2 { .. } => "UDim2",
            Self::Rect { .. } => "Rect",
            Self::Font { .. } => "Font",
            Self::NumberRange { .. } => "NumberRange",
            Self::NumberSequence(_) => "NumberSequence",
            Self::ColorSequence(_) => "ColorSequence",
        }
    }
    
    /// Get a display-friendly string representation of the value
    pub fn display_value(&self) -> String {
        match self {
            Self::String(s) => format!("\"{}\"", s),
            Self::Number(n) => format!("{:.2}", n),
            Self::Int(i) => i.to_string(),
            Self::Bool(b) => b.to_string(),
            Self::Vector2(v) => format!("({:.2}, {:.2})", v.x, v.y),
            Self::Vector3(v) => format!("({:.2}, {:.2}, {:.2})", v.x, v.y, v.z),
            Self::Color(c) => {
                let srgba = c.to_srgba();
                format!("rgba({:.0}, {:.0}, {:.0}, {:.2})", srgba.red * 255.0, srgba.green * 255.0, srgba.blue * 255.0, srgba.alpha)
            }
            Self::Color3(c) => {
                let srgba = c.to_srgba();
                format!("rgb({:.0}, {:.0}, {:.0})", srgba.red * 255.0, srgba.green * 255.0, srgba.blue * 255.0)
            }
            Self::BrickColor(bc) => format!("BrickColor({})", bc),
            Self::CFrame(t) => format!("CFrame({:.2}, {:.2}, {:.2})", t.translation.x, t.translation.y, t.translation.z),
            Self::Object(e) => format!("Object({:?})", e),
            Self::EntityRef(e) => format!("EntityRef({:?})", e),
            Self::UDim2 { x_scale, x_offset, y_scale, y_offset } => {
                format!("UDim2({}, {}, {}, {})", x_scale, x_offset, y_scale, y_offset)
            }
            Self::Rect { min, max } => format!("Rect({:.2}, {:.2}, {:.2}, {:.2})", min.x, min.y, max.x, max.y),
            Self::Font { family, weight, style } => format!("Font({}, {}, {})", family, weight, style),
            Self::NumberRange { min, max } => format!("NumberRange({:.2}, {:.2})", min, max),
            Self::NumberSequence(kps) => format!("NumberSequence({} keypoints)", kps.len()),
            Self::ColorSequence(kps) => format!("ColorSequence({} keypoints)", kps.len()),
        }
    }
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
