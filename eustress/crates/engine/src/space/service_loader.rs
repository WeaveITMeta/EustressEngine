//! Service loader - loads _service.toml files as service entities
//!
//! Architecture:
//! - Service folders contain _service.toml marker files
//! - Each _service.toml defines service-specific properties
//! - Services are spawned as ECS entities with editable properties
//! - Properties are FULLY DATA-DRIVEN: any key-value pairs in TOML are loaded
//! - Icons are specified in TOML, not hardcoded

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Property value types supported in service definitions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PropertyValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
}

impl PropertyValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            PropertyValue::Bool(_) => "bool",
            PropertyValue::Int(_) => "int",
            PropertyValue::Float(_) => "float",
            PropertyValue::String(_) => "string",
            PropertyValue::Vec3(_) => "vec3",
            PropertyValue::Vec4(_) => "color",
        }
    }
    
    pub fn to_display_string(&self) -> String {
        match self {
            PropertyValue::Bool(b) => b.to_string(),
            PropertyValue::Int(i) => i.to_string(),
            PropertyValue::Float(f) => format!("{:.3}", f),
            PropertyValue::String(s) => s.clone(),
            PropertyValue::Vec3(v) => format!("{:.3}, {:.3}, {:.3}", v[0], v[1], v[2]),
            PropertyValue::Vec4(v) => format!("{:.3}, {:.3}, {:.3}, {:.3}", v[0], v[1], v[2], v[3]),
        }
    }
}

/// Service definition loaded from _service.toml file
/// Uses dynamic properties - any key-value pairs are valid
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDefinition {
    pub service: ServiceProperties,
    #[serde(default)]
    pub metadata: ServiceMetadata,
    /// Dynamic properties - any additional key-value pairs
    #[serde(default, flatten)]
    pub properties: HashMap<String, toml::Value>,
}

/// Core service properties (class_name and icon are required, rest is dynamic)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceProperties {
    /// The class name of the service (e.g., "Workspace", "Lighting", "MyCustomService")
    pub class_name: String,
    /// Icon filename (without extension) from assets/icons/ directory
    /// If not specified, falls back to class_name.to_lowercase() or "folder"
    #[serde(default)]
    pub icon: Option<String>,
    /// Optional description of the service
    #[serde(default)]
    pub description: Option<String>,
    /// Whether this service can contain child entities
    #[serde(default = "default_true")]
    pub can_have_children: bool,
    /// Dynamic properties - any additional key-value pairs specific to this service
    #[serde(default, flatten)]
    pub properties: HashMap<String, toml::Value>,
}

fn default_true() -> bool { true }

/// Service metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceMetadata {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub last_modified: String,
}

/// ECS component for service entities - stores ALL properties dynamically
#[derive(Component, Debug, Clone)]
pub struct ServiceComponent {
    /// The class name of the service
    pub class_name: String,
    /// Path to the _service.toml file
    pub toml_path: std::path::PathBuf,
    /// Icon filename (without extension) for Explorer display
    pub icon: String,
    /// Optional description
    pub description: String,
    /// Whether this service can contain children
    pub can_have_children: bool,
    /// All properties as dynamic key-value pairs
    /// Keys are property names, values are typed PropertyValue
    pub properties: HashMap<String, PropertyValue>,
}

impl Default for ServiceComponent {
    fn default() -> Self {
        Self {
            class_name: "Service".to_string(),
            toml_path: std::path::PathBuf::new(),
            icon: "folder".to_string(),
            description: String::new(),
            can_have_children: true,
            properties: HashMap::new(),
        }
    }
}

/// Load a service definition from a _service.toml file
pub fn load_service_definition(path: &Path) -> Result<ServiceDefinition, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    
    toml::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// Convert a toml::Value to a PropertyValue
fn toml_to_property_value(value: &toml::Value) -> Option<PropertyValue> {
    match value {
        toml::Value::Boolean(b) => Some(PropertyValue::Bool(*b)),
        toml::Value::Integer(i) => Some(PropertyValue::Int(*i)),
        toml::Value::Float(f) => Some(PropertyValue::Float(*f)),
        toml::Value::String(s) => Some(PropertyValue::String(s.clone())),
        toml::Value::Array(arr) => {
            // Try to parse as Vec3 or Vec4
            let floats: Vec<f64> = arr.iter()
                .filter_map(|v| match v {
                    toml::Value::Float(f) => Some(*f),
                    toml::Value::Integer(i) => Some(*i as f64),
                    _ => None,
                })
                .collect();
            match floats.len() {
                3 => Some(PropertyValue::Vec3([floats[0], floats[1], floats[2]])),
                4 => Some(PropertyValue::Vec4([floats[0], floats[1], floats[2], floats[3]])),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Convert a PropertyValue back to toml::Value
fn property_value_to_toml(value: &PropertyValue) -> toml::Value {
    match value {
        PropertyValue::Bool(b) => toml::Value::Boolean(*b),
        PropertyValue::Int(i) => toml::Value::Integer(*i),
        PropertyValue::Float(f) => toml::Value::Float(*f),
        PropertyValue::String(s) => toml::Value::String(s.clone()),
        PropertyValue::Vec3(v) => toml::Value::Array(vec![
            toml::Value::Float(v[0]),
            toml::Value::Float(v[1]),
            toml::Value::Float(v[2]),
        ]),
        PropertyValue::Vec4(v) => toml::Value::Array(vec![
            toml::Value::Float(v[0]),
            toml::Value::Float(v[1]),
            toml::Value::Float(v[2]),
            toml::Value::Float(v[3]),
        ]),
    }
}

/// Spawn a service entity from a ServiceDefinition
/// Fully data-driven: any properties in the TOML are loaded dynamically
pub fn spawn_service(
    commands: &mut Commands,
    path: std::path::PathBuf,
    definition: ServiceDefinition,
) -> Entity {
    let props = &definition.service;
    let class_name = props.class_name.clone();
    
    // Determine icon: explicit > class_name.to_lowercase() > "folder"
    let icon = props.icon.clone()
        .unwrap_or_else(|| class_name.to_lowercase());
    
    // Map class_name string to ClassName enum
    // Only Workspace and Lighting have dedicated variants; others use Folder as base
    let class_enum = match class_name.as_str() {
        "Workspace" => eustress_common::classes::ClassName::Workspace,
        "Lighting" => eustress_common::classes::ClassName::Lighting,
        _ => eustress_common::classes::ClassName::Folder,
    };
    
    // Convert all TOML properties to dynamic PropertyValue map
    let mut properties = HashMap::new();
    for (key, value) in &props.properties {
        if let Some(prop_val) = toml_to_property_value(value) {
            properties.insert(key.clone(), prop_val);
        }
    }
    
    let service_component = ServiceComponent {
        class_name: class_name.clone(),
        toml_path: path.clone(),
        icon,
        description: props.description.clone().unwrap_or_default(),
        can_have_children: props.can_have_children,
        properties,
    };
    
    let entity = commands.spawn((
        eustress_common::classes::Instance {
            name: class_name.clone(),
            class_name: class_enum,
            archivable: true,
            id: 0,
            ai: false,
        },
        service_component,
        super::file_loader::LoadedFromFile {
            path: path.clone(),
            file_type: super::file_loader::FileType::Toml,
            service: class_name.clone(),
        },
        Name::new(class_name),
        Transform::default(),
        Visibility::default(),
    )).id();
    
    info!("🏛️ Spawned service entity from {:?}", path);
    entity
}

/// Save service properties back to _service.toml file
/// Preserves all dynamic properties
pub fn save_service_to_file(service: &ServiceComponent) -> Result<(), String> {
    // Convert properties back to TOML values
    let mut toml_properties = HashMap::new();
    for (key, value) in &service.properties {
        toml_properties.insert(key.clone(), property_value_to_toml(value));
    }
    
    let definition = ServiceDefinition {
        service: ServiceProperties {
            class_name: service.class_name.clone(),
            icon: Some(service.icon.clone()),
            description: if service.description.is_empty() { None } else { Some(service.description.clone()) },
            can_have_children: service.can_have_children,
            properties: toml_properties,
        },
        metadata: ServiceMetadata {
            id: format!("{}-service", service.class_name.to_lowercase()),
            created: String::new(), // Preserve existing if possible
            last_modified: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        },
        properties: HashMap::new(),
    };
    
    let toml_str = toml::to_string_pretty(&definition)
        .map_err(|e| format!("Failed to serialize service: {}", e))?;
    
    std::fs::write(&service.toml_path, toml_str)
        .map_err(|e| format!("Failed to write {}: {}", service.toml_path.display(), e))?;
    
    info!("💾 Saved service to {:?}", service.toml_path);
    Ok(())
}
