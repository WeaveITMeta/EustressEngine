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
/// SoulService properties that once carried a Soul API key into a Space
/// file. No Space keeps a key now: the loader drops them, the writer never
/// writes them, and opening a Space removes them from its file and its
/// database record.
pub const LEGACY_SOUL_PROPERTIES: &[&str] = &["api_key", "use_global_key"];

fn is_legacy_soul_property(key: &str) -> bool {
    LEGACY_SOUL_PROPERTIES.iter().any(|legacy| crate::space::instance_loader::same_key(key, legacy))
}

/// Marks a SoulService that loaded with a legacy key property. Holds the
/// removed key only until the on-open cleanup compares it with the saved
/// key; it is never used, stored or set.
#[derive(Component)]
pub struct SoulServiceLegacyKey {
    pub api_key: Option<String>,
    /// The file half and the notice are done; the database half may wait
    /// for the Space's database to open.
    pub file_done: bool,
    /// Frames spent waiting for that open.
    pub frames_waited: u32,
}

impl std::fmt::Debug for SoulServiceLegacyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.api_key.is_some() { "SoulServiceLegacyKey(<key>)" } else { "SoulServiceLegacyKey(none)" })
    }
}

/// Drop the legacy key properties from a SoulService component. `Some`
/// when it had any, with the key it held, for the on-open cleanup.
pub fn take_legacy_soul_properties(service: &mut ServiceComponent) -> Option<SoulServiceLegacyKey> {
    if service.class_name != "SoulService" {
        return None;
    }
    let mut found = false;
    let mut api_key = None;
    service.properties.retain(|key, value| {
        if !is_legacy_soul_property(key) {
            return true;
        }
        found = true;
        if crate::space::instance_loader::same_key(key, "api_key") {
            if let PropertyValue::String(s) = value {
                if !s.trim().is_empty() {
                    api_key = Some(s.trim().to_string());
                }
            }
        }
        false
    });
    found.then_some(SoulServiceLegacyKey { api_key, file_done: false, frames_waited: 0 })
}

/// The component a service's file spawns as: [`service_component`]
/// without the legacy key properties, and the marker when it had them.
pub(crate) fn spawned_service_component(
    path: std::path::PathBuf,
    definition: &ServiceDefinition,
) -> (ServiceComponent, Option<SoulServiceLegacyKey>) {
    let mut service = service_component(path, definition);
    let mut legacy = take_legacy_soul_properties(&mut service);
    if service.class_name == "SoulService" {
        if let Ok(document) = toml::Value::try_from(definition) {
            let (mut found, mut api_key) = (false, None);
            find_legacy_soul_keys(&document, &mut found, &mut api_key);
            if found {
                let marker = legacy.get_or_insert(SoulServiceLegacyKey { api_key: None, file_done: false, frames_waited: 0 });
                if marker.api_key.is_none() {
                    marker.api_key = api_key;
                }
            }
        }
    }
    (service, legacy)
}

/// Legacy key properties at any depth of a SoulService document, exact
/// names (`api_key`, `use_global_key`): whether any is there, and the first
/// key found.
fn find_legacy_soul_keys(value: &toml::Value, found: &mut bool, api_key: &mut Option<String>) {
    match value {
        toml::Value::Table(table) => {
            for (name, inner) in table {
                if LEGACY_SOUL_PROPERTIES.contains(&name.as_str()) {
                    *found = true;
                    if name == "api_key" && api_key.is_none() {
                        *api_key = inner.as_str().map(str::trim).filter(|k| !k.is_empty()).map(str::to_string);
                    }
                }
                find_legacy_soul_keys(inner, found, api_key);
            }
        }
        toml::Value::Array(items) => items.iter().for_each(|item| find_legacy_soul_keys(item, found, api_key)),
        _ => {}
    }
}

/// Delete the legacy key properties at any depth of a SoulService
/// document, exact names only (the export strip uses the same rule).
/// Whether anything was deleted.
pub(crate) fn strip_legacy_soul_keys(doc: &mut toml_edit::DocumentMut) -> bool {
    fn table(t: &mut dyn toml_edit::TableLike) -> bool {
        let mut removed = false;
        for name in LEGACY_SOUL_PROPERTIES {
            removed |= t.remove(name).is_some();
        }
        for (_, inner) in t.iter_mut() {
            removed |= item(inner);
        }
        removed
    }
    fn item(i: &mut toml_edit::Item) -> bool {
        match i {
            toml_edit::Item::Table(t) => table(t),
            toml_edit::Item::ArrayOfTables(list) => list.iter_mut().fold(false, |acc, t| table(t) | acc),
            toml_edit::Item::Value(v) => value(v),
            toml_edit::Item::None => false,
        }
    }
    fn value(v: &mut toml_edit::Value) -> bool {
        match v {
            toml_edit::Value::InlineTable(t) => table(t),
            toml_edit::Value::Array(items) => items.iter_mut().fold(false, |acc, v| value(v) | acc),
            _ => false,
        }
    }
    table(doc.as_table_mut())
}

/// Whether `text` still holds a legacy key property at any depth.
pub fn text_holds_legacy_soul_keys(text: &str) -> bool {
    text.parse::<toml_edit::DocumentMut>().is_ok_and(|mut doc| strip_legacy_soul_keys(&mut doc))
}

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
    
    let (service_component, legacy_soul_key) = spawned_service_component(path.clone(), &definition);

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
    if let Some(marker) = legacy_soul_key {
        commands.entity(entity).insert(marker);
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
    let (service_component, legacy_soul_key) = spawned_service_component(path.clone(), &definition);

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
    if let Some(marker) = legacy_soul_key {
        commands.entity(entity).insert(marker);
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
    let current = std::fs::read_to_string(&service.toml_path).ok();
    planned_service_edit(service, stamp, current.as_deref())
}

/// [`planned_service_write`] against `current`, the file's text as it is
/// now (`None`: no readable file). A SoulService never takes a legacy key
/// property into its file, and one the file holds is deleted.
fn planned_service_edit(
    service: &ServiceComponent,
    stamp: Option<&crate::space::instance_loader::CreatorStamp>,
    current: Option<&str>,
) -> Result<Option<String>, String> {
    let without_legacy;
    let service = if service.class_name == "SoulService" && service.properties.keys().any(|k| is_legacy_soul_property(k)) {
        let mut clean = service.clone();
        take_legacy_soul_properties(&mut clean);
        without_legacy = clean;
        &without_legacy
    } else {
        service
    };
    let now = chrono::Utc::now().to_rfc3339();
    if let Some(text) = current {
        if let (Ok(definition), Ok(mut doc)) =
            (load_service_definition_from_str(text), text.parse::<toml_edit::DocumentMut>())
        {
            let loaded = service_component(service.toml_path.clone(), &definition);
            let changed = edit_service(&mut doc, &loaded, service);
            // A SoulService file loses every legacy key property, at any
            // depth, on every save. This cannot fail the save.
            let stripped = service.class_name == "SoulService" && strip_legacy_soul_keys(&mut doc);
            if !changed && !stripped {
                return Ok(None);
            }
            stamp_service(&mut doc, &definition.metadata, stamp, &now, &service.class_name);
            return Ok(Some(doc.to_string()));
        }
    }
    fresh_service_text(service, stamp, &now).map(Some)
}

/// Opening a Space, the file half: delete the legacy key properties, at any
/// depth, from its SoulService file through the usual writer
/// ([`save_service_to_file_signed`], so the save is stamped in `[metadata]`
/// like any other). A file without them is not touched.
pub fn remove_legacy_from_file(
    path: &Path,
    stamp: Option<&crate::space::instance_loader::CreatorStamp>,
) -> Result<(), String> {
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Some(clean) = legacy_cleanup(path, &text)? {
            save_service_to_file_signed(&clean, stamp)?;
        }
    }
    Ok(())
}

/// Opening a Space, the database half: the same deletion in the Space
/// database's record of the file. `Ok(false)` when no database is active or
/// the record holds nothing to delete; see [`record_cleanup_step`].
pub fn remove_legacy_from_record(
    path: &Path,
    stamp: Option<&crate::space::instance_loader::CreatorStamp>,
) -> Result<bool, String> {
    crate::space::active_db::update_instance_text(path, |text| legacy_record_edit(path, text, stamp))
}

/// A record's text without its legacy key properties (stamped like a file
/// save), or `None` when it holds none.
pub(crate) fn legacy_record_edit(
    path: &Path,
    text: &str,
    stamp: Option<&crate::space::instance_loader::CreatorStamp>,
) -> Result<Option<String>, String> {
    match legacy_cleanup(path, text)? {
        Some(clean) => planned_service_edit(&clean, stamp, Some(text)),
        None => Ok(None),
    }
}

/// The component `text` loads as, without its legacy key properties;
/// `None` when it holds none at any depth.
fn legacy_cleanup(path: &Path, text: &str) -> Result<Option<ServiceComponent>, String> {
    let definition = load_service_definition_from_str(text)?;
    if definition.service.class_name != "SoulService" {
        return Ok(None);
    }
    let mut service = service_component(path.to_path_buf(), &definition);
    let in_component = take_legacy_soul_properties(&mut service).is_some();
    Ok((in_component || text_holds_legacy_soul_keys(text)).then_some(service))
}

/// What the database half of the on-open cleanup does this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordStep {
    /// A database open is still in flight: try again next frame.
    Wait,
    /// Cleared, nothing to clear, or a file-only Space: finished, silently.
    Done,
    /// A database-backed Space whose record could not be cleared: say so.
    Warn(String),
}

/// Frames the cleanup waits for a database open before it warns.
pub const RECORD_WAIT_FRAMES: u32 = 3600;

/// The database half's decision. `open_settled`: no database open is in
/// flight. `db_active`: a database is installed. `attempt`: the record clear.
/// `still_holds`: whether the record still holds a legacy key property.
/// `Ok(false)` is the normal answer for a file-only Space and for a clean
/// record; it is never taken as success while the record still holds one.
pub fn record_cleanup_step(
    open_settled: bool,
    db_active: bool,
    frames_waited: u32,
    attempt: impl FnOnce() -> Result<bool, String>,
    still_holds: impl FnOnce() -> bool,
) -> RecordStep {
    if !open_settled {
        return if frames_waited >= RECORD_WAIT_FRAMES {
            RecordStep::Warn("the Space's database did not finish opening".to_string())
        } else {
            RecordStep::Wait
        };
    }
    if !db_active {
        return RecordStep::Done;
    }
    match attempt() {
        Ok(true) => RecordStep::Done,
        Ok(false) if still_holds() => RecordStep::Warn("its database record could not be updated".to_string()),
        Ok(false) => RecordStep::Done,
        Err(e) => RecordStep::Warn(e),
    }
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
mod soul_key_tests {
    //! A Space never carries a Soul key: the loader drops it, the writer never
    //! writes it, and opening the Space removes it from the file and from the
    //! database record, at any depth. Synthetic keys only.
    use super::*;

    const KEY: &str = concat!("sk-ant-", "TEST0000", "a1b2c3d4e5f6");
    const LEGACY: &str = concat!(
        "[service]\nclass_name = \"SoulService\"\napi_key = \"sk-ant-", "TEST0000", "a1b2c3d4e5f6",
        "\"\nuse_global_key = true\n\n[properties]\nother = 1\n\n[properties.extras]\napi_key = \"sk-ant-", "TEST0000", "a1b2c3d4e5f6",
        "\"\nuse_global_key = false\nkept = 2\n"
    );
    const NESTED_ONLY: &str = concat!(
        "[service]\nclass_name = \"SoulService\"\n\n[properties]\nother = 1\n\n[properties.extras]\napi_key = \"sk-ant-", "TEST0000", "a1b2c3d4e5f6",
        "\"\nkept = 2\n"
    );

    fn scratch_file(tag: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eus_soul_service_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("SoulService")).unwrap();
        let path = dir.join("SoulService").join("_service.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn holds_any(text: &str) -> bool {
        text.contains("api_key") || text.contains("use_global_key") || text.contains(KEY)
    }

    /// T5: the spawned component never holds a legacy property; the marker
    /// holds the key for the check, and its Debug shows none.
    #[test]
    fn t5_a_soul_service_spawns_without_its_key() {
        for text in [LEGACY, NESTED_ONLY] {
            let definition = load_service_definition_from_str(text).unwrap();
            let (component, marker) =
                spawned_service_component(std::path::PathBuf::from("SoulService/_service.toml"), &definition);
            assert!(!component.properties.keys().any(|k| is_legacy_soul_property(k)), "a legacy property spawned");
            assert!(!format!("{:?}", component.properties).contains(KEY), "the key spawned");
            let marker = marker.expect("a legacy property marks the service");
            assert_eq!(marker.api_key.as_deref(), Some(KEY));
            assert!(!format!("{marker:?}").contains(KEY), "the marker's Debug shows the key");
        }
    }

    /// T8: opening removes them from the file at any depth and keeps the
    /// rest; [properties.extras] included.
    #[test]
    fn t8_open_removes_the_key_from_the_file_at_any_depth() {
        for (tag, text) in [("t8", LEGACY), ("t8nested", NESTED_ONLY)] {
            let path = scratch_file(tag, text);
            remove_legacy_from_file(&path, None).unwrap();
            let after = std::fs::read_to_string(&path).unwrap();
            assert!(!holds_any(&after), "{tag}: {after}");
            assert!(after.contains("kept = 2") && after.contains("other = 1"), "{tag}: {after}");
        }
    }

    /// T8: a file without them is not touched.
    #[test]
    fn t8_a_clean_file_is_not_touched() {
        let clean = "[service]\nclass_name = \"SoulService\"\n\n[properties]\nother = 1\n";
        let path = scratch_file("t8clean", clean);
        remove_legacy_from_file(&path, None).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), clean);
    }

    /// The writer filter on the file path: an ordinary SoulService save drops
    /// them at any depth, whatever the component carried.
    #[test]
    fn writer_filter_file_path() {
        let path = scratch_file("wf", LEGACY);
        let definition = load_service_definition_from_str(LEGACY).unwrap();
        let mut service = service_component(path.clone(), &definition);
        service.properties.insert("other".to_string(), PropertyValue::Int(5));
        save_service_to_file_signed(&service, None).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(!holds_any(&after), "{after}");
        assert!(after.contains("other = 5"), "{after}");
    }

    /// The writer filter on the database path: the record's edit drops them
    /// at any depth; a clean record is left.
    #[test]
    fn writer_filter_database_path() {
        let path = std::path::PathBuf::from("Space/SoulService/_service.toml");
        for text in [LEGACY, NESTED_ONLY] {
            let edited = legacy_record_edit(&path, text, None).unwrap().expect("a record with them is rewritten");
            assert!(!holds_any(&edited), "{edited}");
            assert!(edited.contains("kept = 2"), "{edited}");
        }
        let clean = "[service]\nclass_name = \"SoulService\"\n";
        assert_eq!(legacy_record_edit(&path, clean, None).unwrap(), None, "a clean record is left");
    }

    /// (a) A database-backed Space whose record is stale: the clear waits for
    /// the open, then runs; it is cleared or warned, never silent.
    #[test]
    fn record_clear_waits_for_the_database_then_clears_or_warns() {
        assert_eq!(record_cleanup_step(false, false, 0, || panic!("no attempt before the open settles"), || true), RecordStep::Wait);
        let mut cleared = false;
        assert_eq!(record_cleanup_step(true, true, 5, || { cleared = true; Ok(true) }, || false), RecordStep::Done);
        assert!(cleared, "the record clear ran once the database was active");
        assert!(matches!(record_cleanup_step(true, true, 5, || Err("the write failed".into()), || true), RecordStep::Warn(_)));
        assert!(matches!(record_cleanup_step(true, true, 5, || Ok(false), || true), RecordStep::Warn(_)), "Ok(false) on a stale record is not success");
        assert_eq!(record_cleanup_step(true, true, 5, || Ok(false), || false), RecordStep::Done, "a clean record in an open database is not a warning");
        assert!(matches!(record_cleanup_step(false, false, RECORD_WAIT_FRAMES, || Ok(false), || true), RecordStep::Warn(_)));
    }

    /// (b) A file-only Space opens with no warning.
    #[test]
    fn a_file_only_space_opens_without_a_warning() {
        assert_eq!(record_cleanup_step(true, false, 0, || Ok(false), || false), RecordStep::Done);
    }
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
