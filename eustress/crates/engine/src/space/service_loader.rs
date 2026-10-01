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
            PropertyValue::Float(f) => format!("{:.2}", f),
            PropertyValue::String(s) => s.clone(),
            PropertyValue::Vec3(v) => format!("{:.2}, {:.2}, {:.2}", v[0], v[1], v[2]),
            // Colors on 0-255 scale with alpha as decimal
            PropertyValue::Vec4(v) => format!("{:.0}, {:.0}, {:.0}, {:.2}",
                v[0] * 255.0, v[1] * 255.0, v[2] * 255.0, v[3]),
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
    /// Original creator — stamped once on first signed write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<crate::space::instance_loader::CreatorStamp>,
    /// Append-only signed modification chain (never capped — doubles as AI
    /// training signal for "who is capable of what" attribution).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifications: Vec<crate::space::instance_loader::CreatorStamp>,
    /// The unit the file's lengths and speeds are in (`"stud"` on an imported
    /// StarterPlayer); none means the reader's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
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

/// Load a service definition from a _service.toml file. Normalises
/// any-case keys to snake_case before strict-typed deserialization so
/// a hand-authored or migration-affected PascalCase service file still
/// parses cleanly.
pub fn load_service_definition(path: &Path) -> Result<ServiceDefinition, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    load_service_definition_from_str(&content)
}

/// In-memory twin — parse a service `_service.toml` from content the
/// caller already sourced through `SpaceSource` (Fjall tree or disk).
/// No `std::fs`, so a Fjall-authoritative world loads services with
/// zero disk reads.
pub fn load_service_definition_from_str(content: &str) -> Result<ServiceDefinition, String> {
    let mut value: toml::Value = content
        .parse()
        .map_err(|e: toml::de::Error| format!("Failed to parse service TOML: {}", e))?;
    eustress_common::class_schema::normalise_keys(&mut value);
    value
        .try_into()
        .map_err(|e: toml::de::Error| format!("Failed to deserialize service TOML: {}", e))
}

/// Convert a toml::Value to a PropertyValue
pub(crate) fn toml_to_property_value(value: &toml::Value) -> Option<PropertyValue> {
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
pub(crate) fn property_value_to_toml(value: &PropertyValue) -> toml::Value {
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
/// Every property a `_service.toml` defines, from BOTH places it can live.
///
/// Values sit either flattened under `[service]` (where `save_service_to_file`
/// writes them) or in a top-level `[properties]` SECTION, where all 15 shipped
/// service templates put them.
///
/// The section needs expanding by hand. `ServiceDefinition.properties` is
/// `#[serde(flatten)]`, which gathers every unrecognised top-level key -- so a
/// `[properties]` section arrives as ONE entry, `"properties"` holding a
/// `toml::Value::Table`, and `toml_to_property_value` has no arm for tables.
/// Iterating the map directly therefore dropped every templated value for every
/// service: a fresh Space ran Lighting at the built-in 12:00 and latitude 45
/// instead of the template's 14:00 and 41.73. Worse, the first save then wrote
/// an empty `[properties]`, so the template values were gone from disk too.
/// Once the component holds them, saving moves them into `[service]`, where
/// they load on every open.
///
/// `[properties]` is merged second so it wins a collision. Both spawn paths call
/// this, so StarterGui (`spawn_service_as_ui_root`) gets the same treatment.
fn merged_service_properties(definition: &ServiceDefinition) -> HashMap<String, PropertyValue> {
    let mut properties = HashMap::new();
    for (key, value) in &definition.service.properties {
        insert_service_property(&mut properties, key, value);
    }
    // Top-level keys, then the `[properties]` section last, so it wins a
    // collision whatever the map's order (as common's
    // service_document_properties reads a file for a Player).
    for (key, value) in &definition.properties {
        if key != "properties" {
            insert_service_property(&mut properties, key, value);
        }
    }
    if let Some(toml::Value::Table(section)) = definition.properties.get("properties") {
        for (inner_key, inner_value) in section {
            insert_service_property(&mut properties, inner_key, inner_value);
        }
    }
    properties
}

fn insert_service_property(
    properties: &mut HashMap<String, PropertyValue>,
    key: &str,
    value: &toml::Value,
) {
    if let Some(prop_val) = toml_to_property_value(value) {
        properties.insert(key.to_string(), prop_val);
    }
}

/// The component a service's file loads as: its header (the icon is the
/// file's, else the class name lowercased) and every property from both
/// places a file keeps them ([`merged_service_properties`]). Both spawn paths
/// build it here, and the writer compares with it, so a save sees exactly
/// what the file loaded as.
pub(crate) fn service_component(path: std::path::PathBuf, definition: &ServiceDefinition) -> ServiceComponent {
    let props = &definition.service;
    ServiceComponent {
        class_name: props.class_name.clone(),
        toml_path: path,
        icon: props.icon.clone().unwrap_or_else(|| props.class_name.to_lowercase()),
        description: props.description.clone().unwrap_or_default(),
        can_have_children: props.can_have_children,
        properties: merged_service_properties(definition),
    }
}

/// A service's own properties in the tree, by the rules a Player's reader
/// shares: StarterPlayer's, from `record::starter_player_props` given the
/// service's document as its file lays it out (`[service]`, `[properties]`,
/// `[metadata] unit`), which reads each key from `[properties]`, else from
/// `[service]` where an older save moved it. Empty for other services.
pub(crate) fn service_class_props(
    definition: &ServiceDefinition,
) -> Vec<(String, eustress_common::datamodel::DmValue)> {
    if definition.service.class_name != "StarterPlayer" {
        return Vec::new();
    }
    match toml::Value::try_from(definition) {
        Ok(doc) => eustress_common::datamodel::record::starter_player_props(&doc),
        Err(_) => Vec::new(),
    }
}

pub fn spawn_service(
    commands: &mut Commands,
    path: std::path::PathBuf,
    definition: ServiceDefinition,
) -> Entity {
    let class_name = definition.service.class_name.clone();
    let class_props = service_class_props(&definition);

    // Map class_name string to ClassName enum
    // Only Workspace and Lighting have dedicated variants; others use Folder as base
    let class_enum = match class_name.as_str() {
        "Workspace" => eustress_common::classes::ClassName::Workspace,
        "Lighting" => eustress_common::classes::ClassName::Lighting,
        _ => eustress_common::classes::ClassName::Folder,
    };
    
    let service_component = service_component(path.clone(), &definition);

    let entity = commands.spawn((
        eustress_common::classes::Instance {
            name: class_name.clone(),
            class_name: class_enum,
            archivable: true,
            id: 0,
            ai: false,
                uuid: String::new(),
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
    if !class_props.is_empty() {
        commands.entity(entity).insert(eustress_common::datamodel::record::RecordClassProps(class_props));
    }
    
    info!("🏛️ Spawned service entity from {:?}", path);
    entity
}

/// Spawn a service entity as a Bevy UI root (for StarterGui).
/// Uses Node + GlobalZIndex instead of Transform + Visibility so child
/// ScreenGui / Frame / TextLabel entities form a valid Bevy UI hierarchy.
pub fn spawn_service_as_ui_root(
    commands: &mut Commands,
    path: std::path::PathBuf,
    definition: ServiceDefinition,
) -> Entity {
    let class_name = definition.service.class_name.clone();
    let class_props = service_class_props(&definition);
    let service_component = service_component(path.clone(), &definition);

    let entity = commands.spawn((
        eustress_common::classes::Instance {
            name: class_name.clone(),
            class_name: eustress_common::classes::ClassName::Folder,
            archivable: true,
            id: 0,
            ai: false,
                uuid: String::new(),
        },
        service_component,
        super::file_loader::LoadedFromFile {
            path: path.clone(),
            file_type: super::file_loader::FileType::Toml,
            service: class_name.clone(),
        },
        Name::new(class_name),
        // Bevy UI root — transparent fullscreen container so children can be UI nodes
        bevy::prelude::Node {
            width: bevy::prelude::Val::Percent(100.0),
            height: bevy::prelude::Val::Percent(100.0),
            position_type: bevy::prelude::PositionType::Absolute,
            ..Default::default()
        },
        bevy::prelude::GlobalZIndex(99), // Below ScreenGui (100), above 3D
        bevy::prelude::BackgroundColor(bevy::prelude::Color::NONE),
    )).id();
    if !class_props.is_empty() {
        commands.entity(entity).insert(eustress_common::datamodel::record::RecordClassProps(class_props));
    }

    info!("🏛️ Spawned UI service entity (StarterGui) from {:?}", path);
    entity
}

/// Save service properties back to _service.toml file.
/// Unsigned variant — leaves the audit chain untouched.
pub fn save_service_to_file(service: &ServiceComponent) -> Result<(), String> {
    save_service_to_file_signed(service, None)
}

/// Signed variant: a save that changes something stamps it (`created_by` when
/// missing, and the modification chain, where a run of saves by one author is
/// one entry). A save that changes nothing writes nothing (see
/// [`planned_service_write`]).
pub fn save_service_to_file_signed(
    service: &ServiceComponent,
    stamp: Option<&crate::space::instance_loader::CreatorStamp>,
) -> Result<(), String> {
    let Some(toml_str) = planned_service_write(service, stamp)? else {
        return Ok(());
    };

    // A service can be listed in the Explorer before its folder exists: canonical
    // services are synthesized as header-only entries in Spaces that predate
    // them, and the folder is otherwise created lazily with the first child.
    // Without this the first property edit on such a service failed with a
    // path-not-found error and the value never persisted.
    if let Some(parent) = service.toml_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    super::gui_loader::write_atomic(&service.toml_path, toml_str.as_bytes())
        .map_err(|e| format!("Failed to write {}: {}", service.toml_path.display(), e))?;

    info!("💾 Saved service to {:?}", service.toml_path);
    Ok(())
}

/// The text a service save leaves in its file; `None` when the file already
/// holds the service. An existing file is edited in place: only the values
/// that differ from how the file loads now ([`service_component`]) are
/// written, where the file keeps them (`[properties]`, `[service]` or the top
/// level), in its own layout; a value the service no longer has is deleted;
/// every key the service does not carry stays, an import's
/// `[properties.extras]` included. A stamp goes in only with a change. A new
/// or unreadable file gets the whole service.
fn planned_service_write(
    service: &ServiceComponent,
    stamp: Option<&crate::space::instance_loader::CreatorStamp>,
) -> Result<Option<String>, String> {
    let now = chrono::Utc::now().to_rfc3339();
    if let Ok(text) = std::fs::read_to_string(&service.toml_path) {
        if let (Ok(definition), Ok(mut doc)) =
            (load_service_definition_from_str(&text), text.parse::<toml_edit::DocumentMut>())
        {
            let loaded = service_component(service.toml_path.clone(), &definition);
            if !edit_service(&mut doc, &loaded, service) {
                return Ok(None);
            }
            stamp_service(&mut doc, &definition.metadata, stamp, &now, &service.class_name);
            return Ok(Some(doc.to_string()));
        }
    }
    fresh_service_text(service, stamp, &now).map(Some)
}

/// Writes into `doc` every property and header value where `service` differs
/// from `loaded` (how the file loads now), and deletes the properties it no
/// longer has. Returns whether anything changed.
fn edit_service(doc: &mut toml_edit::DocumentMut, loaded: &ServiceComponent, service: &ServiceComponent) -> bool {
    use crate::space::instance_loader::same_value;
    let mut changed = false;
    let mut keys: Vec<&String> = loaded.properties.keys().chain(service.properties.keys()).collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        let old = loaded.properties.get(key).map(property_value_to_toml);
        let new = service.properties.get(key).map(property_value_to_toml);
        match (old, new) {
            (Some(o), Some(n)) if same_value(&o, &n) => {}
            (_, Some(n)) => {
                set_service_value(doc, key, &n);
                changed = true;
            }
            (Some(_), None) => {
                remove_service_value(doc, key);
                changed = true;
            }
            (None, None) => {}
        }
    }
    let header = [
        ("class_name", toml::Value::String(loaded.class_name.clone()), toml::Value::String(service.class_name.clone())),
        ("icon", toml::Value::String(loaded.icon.clone()), toml::Value::String(service.icon.clone())),
        ("description", toml::Value::String(loaded.description.clone()), toml::Value::String(service.description.clone())),
        ("can_have_children", toml::Value::Boolean(loaded.can_have_children), toml::Value::Boolean(service.can_have_children)),
    ];
    for (key, old, new) in header {
        if old != new {
            set_in_section(doc, "service", key, &new);
            changed = true;
        }
    }
    changed
}

/// Where a service value lives in its file: `[properties]` (which wins on
/// load), then `[service]`, then the top level; keys match in any case or
/// underscore style.
fn find_service_value(doc: &toml_edit::DocumentMut, key: &str) -> Option<(Option<&'static str>, String)> {
    use crate::space::instance_loader::same_key;
    for section in ["properties", "service"] {
        if let Some(table) = doc.get(section).and_then(|i| i.as_table_like()) {
            if let Some(found) = table.iter().map(|(k, _)| k.to_string()).find(|k| same_key(k, key)) {
                return Some((Some(section), found));
            }
        }
    }
    doc.as_table()
        .iter()
        .filter(|(_, item)| item.is_value())
        .map(|(k, _)| k.to_string())
        .find(|k| same_key(k, key))
        .map(|found| (None, found))
}

/// A property written where the file keeps it, keeping its comments; a new
/// one goes in `[properties]` when the file has that section, else
/// `[service]`.
fn set_service_value(doc: &mut toml_edit::DocumentMut, key: &str, value: &toml::Value) {
    match find_service_value(doc, key) {
        Some((section, found)) => {
            let table: &mut dyn toml_edit::TableLike = match section {
                Some(section) => match doc.get_mut(section).and_then(|i| i.as_table_like_mut()) {
                    Some(t) => t,
                    None => return,
                },
                None => doc.as_table_mut(),
            };
            let mut new = crate::space::instance_loader::to_value(value);
            if let Some(old) = table.get(&found).and_then(|i| i.as_value()) {
                *new.decor_mut() = old.decor().clone();
            }
            table.insert(&found, toml_edit::Item::Value(new));
        }
        None => {
            let section = if doc.get("properties").is_some_and(|i| i.is_table_like()) { "properties" } else { "service" };
            set_in_section(doc, section, key, value);
        }
    }
}

/// A value under `[section]`, the section made when missing.
fn set_in_section(doc: &mut toml_edit::DocumentMut, section: &str, key: &str, value: &toml::Value) {
    let item = doc.entry(section).or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    if let Some(table) = item.as_table_like_mut() {
        let mut new = crate::space::instance_loader::to_value(value);
        if let Some(old) = table.get(key).and_then(|i| i.as_value()) {
            *new.decor_mut() = old.decor().clone();
        }
        table.insert(key, toml_edit::Item::Value(new));
    }
}

/// A property deleted from every place the file keeps it.
fn remove_service_value(doc: &mut toml_edit::DocumentMut, key: &str) {
    while let Some((section, found)) = find_service_value(doc, key) {
        let removed = match section {
            Some(section) => doc
                .get_mut(section)
                .and_then(|i| i.as_table_like_mut())
                .and_then(|table| table.remove(&found))
                .is_some(),
            None => doc.as_table_mut().remove(&found).is_some(),
        };
        if !removed {
            break;
        }
    }
}

/// The `[metadata]` of a save that changed something: `last_modified`, a
/// missing `id` and `created`, and with a stamp `created_by` (when missing)
/// and the modification chain, where a run by one author is one entry.
fn stamp_service(
    doc: &mut toml_edit::DocumentMut,
    metadata: &ServiceMetadata,
    stamp: Option<&crate::space::instance_loader::CreatorStamp>,
    now: &str,
    class_name: &str,
) {
    use crate::space::instance_loader::{record_modification, to_item};
    let item = doc.entry("metadata").or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let Some(meta) = item.as_table_like_mut() else { return };
    let set = |meta: &mut dyn toml_edit::TableLike, key: &str, value: toml::Value| {
        meta.insert(key, to_item(&value, false));
    };
    if metadata.id.is_empty() {
        set(meta, "id", toml::Value::String(format!("{}-service", class_name.to_lowercase())));
    }
    if metadata.created.is_empty() {
        set(meta, "created", toml::Value::String(now.to_string()));
    }
    match stamp {
        Some(s) => {
            set(meta, "last_modified", toml::Value::String(s.timestamp.clone()));
            if metadata.created_by.is_none() {
                if let Ok(value) = toml::Value::try_from(s) {
                    set(meta, "created_by", value);
                }
            }
            let mut chain = metadata.modifications.clone();
            record_modification(&mut chain, s);
            if let Ok(value) = toml::Value::try_from(&chain) {
                set(meta, "modifications", value);
            }
        }
        None => set(meta, "last_modified", toml::Value::String(now.to_string())),
    }
}

/// A new service file, whole: its header and properties under `[service]`,
/// arrays on one line.
fn fresh_service_text(
    service: &ServiceComponent,
    stamp: Option<&crate::space::instance_loader::CreatorStamp>,
    now: &str,
) -> Result<String, String> {
    let mut modifications = Vec::new();
    if let Some(s) = stamp {
        crate::space::instance_loader::record_modification(&mut modifications, s);
    }
    let definition = ServiceDefinition {
        service: ServiceProperties {
            class_name: service.class_name.clone(),
            icon: Some(service.icon.clone()),
            description: if service.description.is_empty() { None } else { Some(service.description.clone()) },
            can_have_children: service.can_have_children,
            properties: service.properties.iter().map(|(k, v)| (k.clone(), property_value_to_toml(v))).collect(),
        },
        metadata: ServiceMetadata {
            id: format!("{}-service", service.class_name.to_lowercase()),
            created: now.to_string(),
            last_modified: stamp.map_or_else(|| now.to_string(), |s| s.timestamp.clone()),
            created_by: stamp.cloned(),
            modifications,
            unit: None,
        },
        properties: HashMap::new(),
    };
    toml::to_string(&definition).map_err(|e| format!("Failed to serialize service: {}", e))
}

#[cfg(test)]
mod starter_player_node_tests {
    use super::*;
    use eustress_common::datamodel::DmValue;

    fn props(text: &str) -> Vec<(String, DmValue)> {
        service_class_props(&load_service_definition_from_str(text).unwrap())
    }

    fn number(props: &[(String, DmValue)], key: &str) -> Option<f64> {
        props.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
            DmValue::Number(n) => Some(*n),
            _ => None,
        })
    }

    /// An imported StarterPlayer's raw Roblox values reach the tree in
    /// metres, through its unit, and only the keys the file sets.
    #[test]
    fn starter_player_values_reach_the_tree_in_metres() {
        let tagged = props(
            "[service]\nclass_name = \"StarterPlayer\"\n\n[properties]\ncharacter_walk_speed = 16.0\n\n[metadata]\nunit = \"stud\"\n",
        );
        let stud = eustress_common::units::Unit::Stud.to_meters();
        assert!((number(&tagged, "CharacterWalkSpeed").unwrap() - 16.0 * stud).abs() < 1e-9);
        assert!(number(&tagged, "CharacterJumpPower").is_none(), "no invented value");
        // An older Studio save moved the values into [service]; they still count.
        let moved = props("[service]\nclass_name = \"StarterPlayer\"\ncharacter_walk_speed = 16.0\n");
        assert!(number(&moved, "CharacterWalkSpeed").is_some());
        // Other services have none.
        assert!(props("[service]\nclass_name = \"Lighting\"\n").is_empty());
    }
}

#[cfg(test)]
mod writer_tests {
    use super::*;
    use crate::space::instance_loader::CreatorStamp;

    /// Box Head's Workspace service as a template-era writer left it.
    const WORKSPACE: &str = r#"# Workspace Service — Root container for all 3D content in the simulation
# _service.toml marks this folder as a Service and defines all editable properties.

[service]
class_name = "Workspace"
icon = "workspace"
description = "Root container for all game entities and physics simulation"
can_have_children = true

[properties]
gravity = 196.2
fallen_parts_destroy_height = -500.0
global_wind = [0.0, 0.0, 0.0]
streaming_enabled = false
streaming_min_radius = 64
streaming_target_radius = 1024
render_distance = 5000.0
ambient_color = [0.5, 0.5, 0.5, 1.0]
outdoor_ambient = [0.5, 0.5, 0.5, 1.0]
brightness = 2.0
color_correction_saturation = 0.0
color_correction_contrast = 0.0
color_correction_brightness = 0.0
signal_behavior = "Default"
touches_use_collision_groups = false
allow_third_party_sales = false

[metadata]
id = "workspace-service"
created = ""
last_modified = ""
"#;

    fn file(name: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_service_write_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("_service.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn component(path: &std::path::Path) -> ServiceComponent {
        let definition = load_service_definition(path).unwrap();
        service_component(path.to_path_buf(), &definition)
    }

    fn stamp(who: &str, at: &str) -> CreatorStamp {
        CreatorStamp {
            name: who.to_string(),
            public_key: format!("{who}-key"),
            timestamp: at.to_string(),
            first_timestamp: None,
            saves: 1,
        }
    }

    fn text(path: &std::path::Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn done(path: &std::path::Path) {
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Saving a service nobody edited (F9, Ctrl+S) leaves its file byte for
    /// byte and adds no stamp.
    #[test]
    fn an_unchanged_service_save_writes_nothing() {
        let path = file("unchanged", WORKSPACE);
        let service = component(&path);
        save_service_to_file(&service).unwrap();
        save_service_to_file_signed(&service, Some(&stamp("a", "t1"))).unwrap();
        assert_eq!(text(&path), WORKSPACE);
        done(&path);
    }

    /// A changed value rewrites its own line where the file keeps it; every
    /// key the service does not carry stays, an import's extras included.
    #[test]
    fn a_change_rewrites_one_line_and_keeps_the_extras() {
        let with_extras = format!("{WORKSPACE}\n[properties.extras]\nAirDensity = 0.0012\nAuthorityMode = 1\n");
        let path = file("change", &with_extras);
        let mut service = component(&path);
        service.properties.insert("brightness".to_string(), PropertyValue::Float(3.0));
        save_service_to_file_signed(&service, Some(&stamp("a", "t1"))).unwrap();
        let after = text(&path);
        let doc: toml::Value = after.parse().unwrap();
        assert_eq!(doc["properties"]["brightness"].as_float(), Some(3.0));
        assert_eq!(doc["properties"]["extras"]["AuthorityMode"].as_integer(), Some(1), "{after}");
        assert_eq!(doc["properties"]["gravity"].as_float(), Some(196.2));
        assert!(doc["service"].get("brightness").is_none(), "the value stays where the file keeps it");
        assert!(after.starts_with("# Workspace Service"), "comments stay");
        assert!(after.contains("global_wind = [0.0, 0.0, 0.0]\n"), "arrays keep their line");
        assert_eq!(doc["metadata"]["last_modified"].as_str(), Some("t1"));
        assert_eq!(doc["metadata"]["modifications"].as_array().map(|a| a.len()), Some(1));

        // A second save by the same author merges into that entry.
        let mut service = component(&path);
        service.properties.insert("brightness".to_string(), PropertyValue::Float(4.0));
        save_service_to_file_signed(&service, Some(&stamp("a", "t2"))).unwrap();
        let doc: toml::Value = text(&path).parse().unwrap();
        let chain = doc["metadata"]["modifications"].as_array().unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0]["saves"].as_integer(), Some(2));
        done(&path);
    }

    /// A new value goes where the file keeps its values; a removed one goes.
    #[test]
    fn new_values_join_their_section_and_removed_ones_go() {
        let path = file("add_remove", WORKSPACE);
        let mut service = component(&path);
        service.properties.insert("wind_gusts".to_string(), PropertyValue::Bool(true));
        service.properties.remove("render_distance");
        save_service_to_file(&service).unwrap();
        let doc: toml::Value = text(&path).parse().unwrap();
        assert_eq!(doc["properties"]["wind_gusts"].as_bool(), Some(true));
        assert!(doc["properties"].get("render_distance").is_none());
        done(&path);
    }

    /// A service with no file yet is written whole and loads back the same.
    #[test]
    fn a_new_service_file_is_written_whole() {
        let dir = std::env::temp_dir().join(format!("eustress_service_write_fresh_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("_service.toml");
        let mut service = ServiceComponent { class_name: "Teams".to_string(), toml_path: path.clone(), ..Default::default() };
        service.properties.insert("auto_assign".to_string(), PropertyValue::Bool(true));
        save_service_to_file(&service).unwrap();
        let back = component(&path);
        assert_eq!(back.class_name, "Teams");
        assert!(matches!(back.properties.get("auto_assign"), Some(PropertyValue::Bool(true))));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn float(props: &HashMap<String, PropertyValue>, key: &str) -> Option<f64> {
        match props.get(key) {
            Some(PropertyValue::Float(v)) => Some(*v as f64),
            _ => None,
        }
    }

    /// A value a file holds in more than one place is read with a fixed
    /// precedence: `[properties]` wins over a top-level key, which wins over
    /// `[service]`. Parsed many times, since the maps' order is random per
    /// parse and the old single walk resolved a collision by that order.
    #[test]
    fn a_service_value_is_read_with_a_fixed_precedence() {
        let text = "brightness = 2.0\nfog_end = 60.0\n\n\
                    [service]\nclass_name = \"Lighting\"\nbrightness = 1.0\nfog_end = 50.0\nclock_time = 6.0\n\n\
                    [properties]\nbrightness = 3.0\n";
        for _ in 0..64 {
            let props = merged_service_properties(&load_service_definition_from_str(text).unwrap());
            assert_eq!(float(&props, "brightness"), Some(3.0), "[properties] wins");
            assert_eq!(float(&props, "fog_end"), Some(60.0), "a top-level key wins over [service]");
            assert_eq!(float(&props, "clock_time"), Some(6.0), "[service] alone");
        }
    }

    /// The shipped Lighting template, loaded exactly as production loads it
    /// (`load_service_definition_from_str`: parse, `normalise_keys`,
    /// deserialize). Its values live in a `[properties]` SECTION, which
    /// `#[serde(flatten)]` delivers as a single Table entry -- the case that
    /// silently dropped every templated service value.
    #[test]
    fn shipped_lighting_template_values_reach_the_component() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../common/assets/service_templates/Lighting/_service.toml");
        let text = std::fs::read_to_string(&path).expect("read the shipped Lighting template");
        let def = load_service_definition_from_str(&text).expect("parse the Lighting template");
        let props = merged_service_properties(&def);

        assert_eq!(float(&props, "clock_time"), Some(14.0), "clock_time must come from [properties]");
        assert_eq!(float(&props, "geographic_latitude"), Some(41.73), "latitude must come from [properties]");
        assert_eq!(float(&props, "brightness"), Some(2.0));
        assert!(
            !props.contains_key("properties"),
            "the section itself must be expanded, not stored as a key"
        );
    }

    /// A value written flat under `[service]` (where saving puts it) still
    /// loads, and `[properties]` wins when both define the same key.
    #[test]
    fn service_table_values_load_and_properties_section_wins() {
        let text = "[service]\nclass_name = \"Lighting\"\nclock_time = 9.0\nbrightness = 3.0\n\n[properties]\nclock_time = 14.0\n";
        let def = load_service_definition_from_str(text).expect("parse");
        let props = merged_service_properties(&def);
        assert_eq!(float(&props, "brightness"), Some(3.0), "a [service] value loads");
        assert_eq!(float(&props, "clock_time"), Some(14.0), "[properties] wins a collision");
    }
}
