//! Instance loader - loads .glb.toml files as entity instances
//!
//! Architecture:
//! - Mesh assets live in assets/meshes/ (shared, reusable)
//! - Instance files (.glb.toml) live in Workspace/ (unique per entity)
//! - Each .toml references a mesh asset and defines instance-specific properties

use bevy::prelude::*;
use bevy::camera::primitives::MeshAabb;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use avian3d::prelude::{Collider, RigidBody};
use crate::rendering::PartEntity;
use eustress_common::{Attributes, Tags};

/// Instance definition loaded from .glb.toml or .instance.toml file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceDefinition {
    /// Mesh reference — optional for non-visual instances (lighting, sky, atmosphere)
    #[serde(default)]
    pub asset: Option<AssetReference>,
    /// World transform — optional for non-visual instances
    #[serde(default)]
    pub transform: TransformData,
    /// Standard part properties (color, anchored, etc.) — all defaulted
    #[serde(default)]
    pub properties: InstanceProperties,
    pub metadata: InstanceMetadata,
    /// Optional realism material properties (dynamic on any class)
    #[serde(default)]
    pub material: Option<TomlMaterialProperties>,
    /// Optional thermodynamic state (dynamic on any class)
    #[serde(default)]
    pub thermodynamic: Option<TomlThermodynamicState>,
    /// Optional electrochemical state (dynamic on any class)
    #[serde(default)]
    pub electrochemical: Option<TomlElectrochemicalState>,
    /// Optional UI class properties (TextLabel, TextButton, Frame, ImageLabel, etc.)
    #[serde(default)]
    pub ui: Option<UiInstanceProperties>,
    /// Custom attributes (key-value pairs for scripting)
    #[serde(default)]
    pub attributes: Option<std::collections::HashMap<String, toml::Value>>,
    /// Tags for CollectionService grouping
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Instance parameters (custom configuration values)
    #[serde(default)]
    pub parameters: Option<std::collections::HashMap<String, toml::Value>>,
    /// All unknown top-level sections (e.g. [Appearance], [Position], [Lighting]) captured
    /// via flatten so rich-schema .instance.toml files work without hardcoded field names.
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, toml::Value>,
}

/// Reference to a shared mesh asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReference {
    /// Path to mesh file (relative to Space root)
    pub mesh: String,
    /// glTF scene name (usually "Scene0")
    #[serde(default = "default_scene")]
    pub scene: String,
}

fn default_scene() -> String {
    "Scene0".to_string()
}

/// Transform data (position, rotation, scale)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformData {
    pub position: [f32; 3],
    pub rotation: [f32; 4], // Quaternion (x, y, z, w)
    pub scale: [f32; 3],
}

impl Default for TransformData {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

impl From<TransformData> for Transform {
    fn from(data: TransformData) -> Self {
        Transform {
            translation: Vec3::from_array(data.position),
            rotation: Quat::from_xyzw(
                data.rotation[0],
                data.rotation[1],
                data.rotation[2],
                data.rotation[3],
            ),
            scale: Vec3::from_array(data.scale),
        }
    }
}

impl From<Transform> for TransformData {
    fn from(transform: Transform) -> Self {
        Self {
            position: transform.translation.to_array(),
            rotation: [
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
                transform.rotation.w,
            ],
            scale: transform.scale.to_array(),
        }
    }
}

/// Instance-specific properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceProperties {
    /// RGBA color (0.0-1.0 floats internally).
    /// TOML accepts both 0-255 integer arrays `[163, 162, 165]` (RGB)
    /// and legacy 0.0-1.0 float arrays `[0.5, 0.5, 0.5, 1.0]` (RGBA).
    #[serde(default = "default_color", deserialize_with = "deserialize_color_flexible", serialize_with = "serialize_color_as_u8")]
    pub color: [f32; 4], // RGBA
    #[serde(default)]
    pub transparency: f32,
    #[serde(default)]
    pub anchored: bool,
    #[serde(default = "default_true")]
    pub can_collide: bool,
    #[serde(default = "default_true")]
    pub cast_shadow: bool,
    #[serde(default)]
    pub reflectance: f32,
    /// Material name — resolved from MaterialRegistry first, then Material enum fallback
    #[serde(default = "default_material_name_plastic")]
    pub material: String,
    /// When true, the entity cannot be selected via 3D click (e.g. Baseplate)
    #[serde(default)]
    pub locked: bool,
}

fn default_material_name_plastic() -> String {
    "Plastic".to_string()
}

fn default_color() -> [f32; 4] {
    // Default: medium gray [163, 162, 165] in 0-255 → 0.0-1.0
    [163.0 / 255.0, 162.0 / 255.0, 165.0 / 255.0, 1.0]
}

/// Custom deserializer that accepts both 0-255 integer RGB/RGBA and 0.0-1.0 float RGBA arrays.
/// - `[163, 162, 165]`     → RGB integers, alpha defaults to 1.0
/// - `[163, 162, 165, 200]` → RGBA integers
/// - `[0.639, 0.635, 0.647, 1.0]` → legacy RGBA floats (values ≤ 1.0)
/// Detection heuristic: if ALL values are integers, treat as 0-255. Otherwise treat as floats.
fn deserialize_color_flexible<'de, D>(deserializer: D) -> Result<[f32; 4], D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values: Vec<toml::Value> = serde::Deserialize::deserialize(deserializer)?;

    if values.len() < 3 {
        return Err(serde::de::Error::custom(
            "color array must have at least 3 elements (RGB)",
        ));
    }

    // Check if all values are integers (0-255 format)
    let all_integers = values.iter().all(|v| v.is_integer());

    if all_integers {
        // 0-255 integer format
        let r = values[0].as_integer().unwrap_or(128) as f32 / 255.0;
        let g = values[1].as_integer().unwrap_or(128) as f32 / 255.0;
        let b = values[2].as_integer().unwrap_or(128) as f32 / 255.0;
        let a = if values.len() >= 4 {
            values[3].as_integer().unwrap_or(255) as f32 / 255.0
        } else {
            1.0
        };
        Ok([r, g, b, a])
    } else {
        // 0.0-1.0 float format (legacy)
        let r = values[0].as_float().or_else(|| values[0].as_integer().map(|i| i as f64)).unwrap_or(0.5) as f32;
        let g = values[1].as_float().or_else(|| values[1].as_integer().map(|i| i as f64)).unwrap_or(0.5) as f32;
        let b = values[2].as_float().or_else(|| values[2].as_integer().map(|i| i as f64)).unwrap_or(0.5) as f32;
        let a = if values.len() >= 4 {
            values[3].as_float().or_else(|| values[3].as_integer().map(|i| i as f64)).unwrap_or(1.0) as f32
        } else {
            1.0
        };
        Ok([r, g, b, a])
    }
}

/// Custom serializer that writes color as 0-255 RGB integer array.
/// If alpha is not 1.0 (fully opaque), writes RGBA; otherwise just RGB.
fn serialize_color_as_u8<S>(color: &[f32; 4], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let r = (color[0] * 255.0).round() as u8;
    let g = (color[1] * 255.0).round() as u8;
    let b = (color[2] * 255.0).round() as u8;
    let a = (color[3] * 255.0).round() as u8;
    if a == 255 {
        // Opaque — write compact RGB
        let mut seq = serializer.serialize_seq(Some(3))?;
        seq.serialize_element(&r)?;
        seq.serialize_element(&g)?;
        seq.serialize_element(&b)?;
        seq.end()
    } else {
        // Semi-transparent — write RGBA
        let mut seq = serializer.serialize_seq(Some(4))?;
        seq.serialize_element(&r)?;
        seq.serialize_element(&g)?;
        seq.serialize_element(&b)?;
        seq.serialize_element(&a)?;
        seq.end()
    }
}

fn default_true() -> bool {
    true
}

impl Default for InstanceProperties {
    fn default() -> Self {
        Self {
            color: default_color(),
            transparency: 0.0,
            anchored: false,
            can_collide: true,
            cast_shadow: true,
            reflectance: 0.0,
            material: default_material_name_plastic(),
            locked: false,
        }
    }
}

/// Signed attribution for a create or modify event. Lightweight by design —
/// we keep every signature (no cap, no consolidation) because the modification
/// history doubles as AI training data: the system learns "who is capable of
/// what kinds of changes" by reading the full stamp chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreatorStamp {
    /// Display name at time of edit (AuthUser.username; "anonymous" if offline).
    pub name: String,
    /// Stable identity (AuthUser.id today; upgrade to full public key later).
    pub public_key: String,
    /// RFC 3339 timestamp of the edit.
    pub timestamp: String,
}

/// Instance metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceMetadata {
    #[serde(default = "default_class_name")]
    pub class_name: String,
    #[serde(default = "default_true")]
    pub archivable: bool,
    /// Display name override. When present, used instead of filename-derived name.
    /// Allows multiple instances with the same display name but unique filenames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub last_modified: String,
    /// Original creator — stamped once on first write by a logged-in user.
    /// Absent for entities created offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<CreatorStamp>,
    /// Append-only record of every signed modification. Never capped — the full
    /// chain is kept as training signal for Bliss attribution + AI learning.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifications: Vec<CreatorStamp>,
}

fn default_class_name() -> String {
    "Part".to_string()
}

impl Default for InstanceMetadata {
    fn default() -> Self {
        Self {
            class_name: default_class_name(),
            archivable: true,
            name: None,
            created: String::new(),
            last_modified: String::new(),
            created_by: None,
            modifications: Vec::new(),
        }
    }
}

// ============================================================================
// TOML-serializable realism property structs
// ============================================================================

/// Material properties as they appear in .glb.toml [material] section
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TomlMaterialProperties {
    #[serde(default = "default_material_name")]
    pub name: String,
    #[serde(default)]
    pub young_modulus: f32,
    #[serde(default)]
    pub poisson_ratio: f32,
    #[serde(default)]
    pub yield_strength: f32,
    #[serde(default)]
    pub ultimate_strength: f32,
    #[serde(default)]
    pub fracture_toughness: f32,
    #[serde(default)]
    pub hardness: f32,
    #[serde(default)]
    pub thermal_conductivity: f32,
    #[serde(default)]
    pub specific_heat: f32,
    #[serde(default)]
    pub thermal_expansion: f32,
    #[serde(default)]
    pub melting_point: f32,
    #[serde(default)]
    pub density: f32,
    #[serde(default)]
    pub friction_static: f32,
    #[serde(default)]
    pub friction_kinetic: f32,
    #[serde(default)]
    pub restitution: f32,
    /// Domain-specific extensions (porosity, electrical_conductivity, role, etc.)
    /// Accepts both numeric and string values from TOML; only f64 values
    /// are forwarded to the realism MaterialProperties component.
    #[serde(default)]
    pub custom: HashMap<String, toml::Value>,
}

fn default_material_name() -> String {
    "Steel".to_string()
}

impl TomlMaterialProperties {
    /// Convert to realism MaterialProperties component
    pub fn to_component(&self) -> eustress_common::realism::materials::prelude::MaterialProperties {
        eustress_common::realism::materials::prelude::MaterialProperties {
            name: self.name.clone(),
            young_modulus: self.young_modulus,
            poisson_ratio: self.poisson_ratio,
            yield_strength: self.yield_strength,
            ultimate_strength: self.ultimate_strength,
            fracture_toughness: self.fracture_toughness,
            hardness: self.hardness,
            thermal_conductivity: self.thermal_conductivity,
            specific_heat: self.specific_heat,
            thermal_expansion: self.thermal_expansion,
            melting_point: self.melting_point,
            density: self.density,
            friction_static: self.friction_static,
            friction_kinetic: self.friction_kinetic,
            restitution: self.restitution,
            custom_properties: self.custom.iter()
                .filter_map(|(k, v)| match v {
                    toml::Value::Float(f) => Some((k.clone(), *f)),
                    toml::Value::Integer(i) => Some((k.clone(), *i as f64)),
                    _ => None, // skip strings, bools, etc.
                })
                .collect(),
        }
    }
}

/// Thermodynamic state as it appears in .glb.toml [thermodynamic] section
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TomlThermodynamicState {
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_pressure")]
    pub pressure: f32,
    #[serde(default)]
    pub volume: f32,
    #[serde(default)]
    pub internal_energy: f32,
    #[serde(default)]
    pub entropy: f32,
    #[serde(default)]
    pub enthalpy: f32,
    #[serde(default = "default_one")]
    pub moles: f32,
}

fn default_temperature() -> f32 { 298.15 }
fn default_pressure() -> f32 { 101_325.0 }
fn default_one() -> f32 { 1.0 }

impl TomlThermodynamicState {
    /// Convert to realism ThermodynamicState component
    pub fn to_component(&self) -> eustress_common::realism::particles::prelude::ThermodynamicState {
        eustress_common::realism::particles::prelude::ThermodynamicState {
            temperature: self.temperature,
            pressure: self.pressure,
            volume: self.volume,
            internal_energy: self.internal_energy,
            entropy: self.entropy,
            enthalpy: self.enthalpy,
            moles: self.moles,
        }
    }
}

// ============================================================================
// UI class properties — covers TextLabel, TextButton, Frame, ImageLabel,
// TextBox, ScrollingFrame. Stored under [ui] in the .glb.toml file.
// ============================================================================

/// Universal UI-element properties stored under [ui] in the instance TOML.
/// All UI classes share layout/appearance fields; class-specific fields use
/// serde(default) so missing keys are silently zero/false.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiInstanceProperties {
    // ---- Text (TextLabel / TextButton / TextBox) ----
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub rich_text: bool,
    #[serde(default)]
    pub text_scaled: bool,
    #[serde(default)]
    pub text_wrapped: bool,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub line_height: f32,
    #[serde(default = "default_font")]
    pub font: String,
    #[serde(default)]
    pub text_color3: [f32; 3],
    #[serde(default)]
    pub text_transparency: f32,
    #[serde(default)]
    pub text_stroke_color3: [f32; 3],
    #[serde(default = "default_one")]
    pub text_stroke_transparency: f32,
    #[serde(default = "default_text_x_alignment")]
    pub text_x_alignment: String,   // "Left" | "Center" | "Right"
    #[serde(default = "default_text_y_alignment")]
    pub text_y_alignment: String,   // "Top" | "Center" | "Bottom"
    // ---- Appearance (all UI elements) ----
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_white")]
    pub background_color3: [f32; 3],
    #[serde(default)]
    pub background_transparency: f32,
    #[serde(default)]
    pub border_color3: [f32; 3],
    #[serde(default)]
    pub border_size_pixel: i32,
    #[serde(default = "default_border_mode")]
    pub border_mode: String,        // "Outline" | "Middle" | "Inset"
    #[serde(default)]
    pub clips_descendants: bool,
    #[serde(default = "default_one_i32")]
    pub z_index: i32,
    #[serde(default)]
    pub layout_order: i32,
    #[serde(default)]
    pub rotation: f32,
    // ---- Layout / UDim2 (position + size) ----
    #[serde(default)]
    pub anchor_point: [f32; 2],
    #[serde(default)]
    pub position_scale: [f32; 2],
    #[serde(default)]
    pub position_offset: [f32; 2],
    #[serde(default)]
    pub size_scale: [f32; 2],
    #[serde(default = "default_size_offset")]
    pub size_offset: [f32; 2],
    // ---- Behavior ----
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default = "default_true")]
    pub auto_button_color: bool,
    // ---- Image (ImageLabel / ImageButton) ----
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub image_color3: [f32; 3],
    #[serde(default)]
    pub image_transparency: f32,
    #[serde(default = "default_scale_type")]
    pub scale_type: String,         // "Stretch" | "Slice" | "Tile" | "Fit" | "Crop"
    // ---- ScrollingFrame ----
    #[serde(default = "default_true")]
    pub scrolling_enabled: bool,
    #[serde(default)]
    pub scroll_bar_thickness: i32,
    // ---- AutomaticSize ----
    #[serde(default = "default_automatic_size")]
    pub automatic_size: String,     // "None" | "X" | "Y" | "XY"
}

fn default_font_size() -> f32 { 14.0 }
fn default_font() -> String { "SourceSans".to_string() }
fn default_text_x_alignment() -> String { "Center".to_string() }
fn default_text_y_alignment() -> String { "Center".to_string() }
fn default_white() -> [f32; 3] { [1.0, 1.0, 1.0] }
fn default_one_i32() -> i32 { 1 }
fn default_border_mode() -> String { "Outline".to_string() }
fn default_scale_type() -> String { "Stretch".to_string() }
fn default_automatic_size() -> String { "None".to_string() }
fn default_size_offset() -> [f32; 2] { [100.0, 100.0] }

impl Default for UiInstanceProperties {
    fn default() -> Self {
        Self {
            text: String::new(),
            rich_text: false,
            text_scaled: false,
            text_wrapped: false,
            font_size: default_font_size(),
            line_height: 0.0,
            font: default_font(),
            text_color3: [0.0, 0.0, 0.0],
            text_transparency: 0.0,
            text_stroke_color3: [0.0, 0.0, 0.0],
            text_stroke_transparency: 1.0,
            text_x_alignment: default_text_x_alignment(),
            text_y_alignment: default_text_y_alignment(),
            visible: true,
            background_color3: default_white(),
            background_transparency: 0.0,
            border_color3: [0.0, 0.0, 0.0],
            border_size_pixel: 0,
            border_mode: default_border_mode(),
            clips_descendants: false,
            z_index: 1,
            layout_order: 0,
            rotation: 0.0,
            anchor_point: [0.0, 0.0],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: default_size_offset(),
            active: true,
            auto_button_color: true,
            image: String::new(),
            image_color3: [1.0, 1.0, 1.0],
            image_transparency: 0.0,
            scale_type: default_scale_type(),
            scrolling_enabled: true,
            scroll_bar_thickness: 12,
            automatic_size: default_automatic_size(),
        }
    }
}

impl UiInstanceProperties {
    /// Convert the stored font string to the ECS Font enum
    fn to_font(&self) -> eustress_common::classes::Font {
        use eustress_common::classes::Font;
        match self.font.as_str() {
            "RobotoMono"  => Font::RobotoMono,
            "GothamBold"  => Font::GothamBold,
            "GothamLight" => Font::GothamLight,
            "Fantasy"     => Font::Fantasy,
            "Bangers"     => Font::Bangers,
            "Merriweather"=> Font::Merriweather,
            "Nunito"      => Font::Nunito,
            "Ubuntu"      => Font::Ubuntu,
            _             => Font::SourceSans,
        }
    }
    fn to_x_align(&self) -> eustress_common::classes::TextXAlignment {
        use eustress_common::classes::TextXAlignment;
        match self.text_x_alignment.as_str() {
            "Left"  => TextXAlignment::Left,
            "Right" => TextXAlignment::Right,
            _       => TextXAlignment::Center,
        }
    }
    fn to_y_align(&self) -> eustress_common::classes::TextYAlignment {
        use eustress_common::classes::TextYAlignment;
        match self.text_y_alignment.as_str() {
            "Top"    => TextYAlignment::Top,
            "Bottom" => TextYAlignment::Bottom,
            _        => TextYAlignment::Center,
        }
    }
    fn to_auto_size(&self) -> eustress_common::classes::AutomaticSize {
        use eustress_common::classes::AutomaticSize;
        match self.automatic_size.as_str() {
            "X"  => AutomaticSize::X,
            "Y"  => AutomaticSize::Y,
            "XY" => AutomaticSize::XY,
            _    => AutomaticSize::None,
        }
    }
    fn to_border_mode(&self) -> eustress_common::classes::BorderMode {
        use eustress_common::classes::BorderMode;
        match self.border_mode.as_str() {
            "Middle" => BorderMode::Middle,
            "Inset"  => BorderMode::Inset,
            _        => BorderMode::Outline,
        }
    }
}

/// Electrochemical state as it appears in .glb.toml [electrochemical] section
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TomlElectrochemicalState {
    #[serde(default = "default_voltage")]
    pub voltage: f32,
    #[serde(default = "default_voltage")]
    pub terminal_voltage: f32,
    #[serde(default)]
    pub capacity_ah: f32,
    #[serde(default = "default_one")]
    pub soc: f32,
    #[serde(default)]
    pub current: f32,
    #[serde(default)]
    pub internal_resistance: f32,
    #[serde(default)]
    pub ionic_conductivity: f32,
    #[serde(default)]
    pub cycle_count: u32,
    #[serde(default)]
    pub c_rate: f32,
    #[serde(default = "default_one")]
    pub capacity_retention: f32,
    #[serde(default)]
    pub heat_generation: f32,
    #[serde(default)]
    pub dendrite_risk: f32,
}

fn default_voltage() -> f32 { 2.23 }

impl TomlElectrochemicalState {
    /// Convert to realism ElectrochemicalState component
    pub fn to_component(&self) -> eustress_common::realism::particles::prelude::ElectrochemicalState {
        eustress_common::realism::particles::prelude::ElectrochemicalState {
            voltage: self.voltage,
            terminal_voltage: self.terminal_voltage,
            capacity_ah: self.capacity_ah,
            soc: self.soc,
            current: self.current,
            internal_resistance: self.internal_resistance,
            ionic_conductivity: self.ionic_conductivity,
            cycle_count: self.cycle_count,
            c_rate: self.c_rate,
            capacity_retention: self.capacity_retention,
            heat_generation: self.heat_generation,
            dendrite_risk: self.dendrite_risk,
        }
    }
}

/// Component marking an entity as loaded from an instance file.
/// For folder-based instances: toml_path = folder/_instance.toml
/// For legacy flat files: toml_path = folder/Name.glb.toml
#[derive(Component, Debug, Clone)]
pub struct InstanceFile {
    /// Path to the instance TOML file (_instance.toml or .glb.toml)
    pub toml_path: PathBuf,
    /// Path to the referenced mesh asset
    pub mesh_path: PathBuf,
    /// Instance name (derived from filename)
    pub name: String,
}

/// Marker placed on custom-mesh Parts so a polling system can update their
/// `BasePart.size` once the mesh asset has finished loading. Without this,
/// custom-mesh parts keep the TOML's `transform.scale` value (typically
/// 1×1×1) as their collision + gizmo size — which is correct for unit
/// primitives but wrong for anything with real geometry. The marker is
/// removed after the size is applied so the system becomes a no-op.
#[derive(Component, Debug)]
pub struct NeedsMeshSize;

/// Load instance definition from .glb.toml file
pub fn load_instance_definition(toml_path: &Path) -> Result<InstanceDefinition, String> {
    load_instance_definition_with_defaults(toml_path, None)
}

/// Load instance definition from .glb.toml file, merging class defaults for any missing fields.
///
/// When a ClassDefaultsRegistry is provided, the loader:
/// 1. Parses the raw TOML into a generic toml::Value
/// 2. Reads `metadata.class_name` to determine which class defaults to apply
/// 3. Deep-merges missing keys from the class defaults
/// 4. Deserializes the merged TOML into InstanceDefinition
///
/// This ensures that TOML files on disk only need to specify the properties that
/// differ from the class defaults — everything else is filled in automatically.
pub fn load_instance_definition_with_defaults(
    toml_path: &Path,
    registry: Option<&super::class_defaults::ClassDefaultsRegistry>,
) -> Result<InstanceDefinition, String> {
    let toml_str = std::fs::read_to_string(toml_path)
        .map_err(|e| format!("Failed to read {}: {}", toml_path.display(), e))?;

    // If no registry, skip the merge step and deserialize directly
    let Some(registry) = registry else {
        let instance: InstanceDefinition = toml::from_str(&toml_str)
            .map_err(|e| format!("Failed to parse {}: {}", toml_path.display(), e))?;
        return Ok(instance);
    };

    // Parse into generic TOML value for merge
    let mut toml_value: toml::Value = toml_str.parse()
        .map_err(|e: toml::de::Error| format!("Failed to parse {}: {}", toml_path.display(), e))?;

    // Extract class_name to look up defaults
    let class_name = toml_value
        .get("metadata")
        .and_then(|m| m.get("class_name"))
        .and_then(|c| c.as_str())
        .unwrap_or("Part")
        .to_string();

    // Merge class defaults for missing fields
    if let Some(defaults) = registry.get(&class_name) {
        super::class_defaults::merge_defaults(&mut toml_value, defaults);
    }

    // Deserialize the merged TOML
    let instance: InstanceDefinition = toml_value.try_into()
        .map_err(|e: toml::de::Error| format!("Failed to deserialize merged {}: {}", toml_path.display(), e))?;

    Ok(instance)
}

/// Write instance definition to .glb.toml file
pub fn write_instance_definition(
    toml_path: &Path,
    instance: &InstanceDefinition,
) -> Result<(), String> {
    let toml_str = toml::to_string_pretty(instance)
        .map_err(|e| format!("Failed to serialize instance: {}", e))?;

    std::fs::write(toml_path, toml_str)
        .map_err(|e| format!("Failed to write {}: {}", toml_path.display(), e))?;

    Ok(())
}

/// Returns true if `name` is available for a new entity in `dir` — i.e. no
/// existing file or folder would collide.
///
/// Entity names map to two disk shapes: folder-based (`BASE/_instance.toml`)
/// and legacy flat (`BASE.toml`, `BASE.glb.toml`, `BASE.<ext>.toml`). The
/// file-loader treats both shapes as the same entity name "BASE". A naive
/// `dir.join(name).exists()` check only catches the folder form — so
/// duplicating a flat-file entity would create a sibling folder with the
/// same name, producing two conflicting "BASE" entities on reload (the
/// corruption the user reported: Block.toml + Block/_instance.toml).
///
/// This helper rejects the name if ANY entry in `dir` would resolve to it:
/// a folder named `BASE`, a file `BASE.toml`, or any `BASE.<anything>.toml`.
pub fn entity_name_is_available(dir: &Path, name: &str) -> bool {
    if name.is_empty() { return false; }
    // Folder with this exact name — the common path.
    if dir.join(name).exists() { return false; }
    // Any flat file whose first path segment (before the first `.`) matches.
    // `.split('.').next()` yields the stem up to the first dot, so
    // `Block.toml`, `Block.glb.toml`, and `Block.script.toml` all resolve
    // to "Block" and therefore conflict.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name();
            let Some(s) = fname.to_str() else { continue };
            if s.split('.').next() == Some(name) {
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    return false;
                }
            }
        }
    }
    true
}

/// Pick a unique entity name in `dir`, falling back to `BASE`, `BASE1`, …
/// `BASE9999`, then `BASE_<unix-timestamp>` if all are taken. Centralizes
/// the collision check so every creation / paste / duplicate site gets the
/// flat-file-aware behavior from [`entity_name_is_available`].
pub fn unique_entity_name(dir: &Path, base: &str) -> String {
    if entity_name_is_available(dir, base) {
        return base.to_string();
    }
    for i in 1..10_000u32 {
        let candidate = format!("{}{}", base, i);
        if entity_name_is_available(dir, &candidate) {
            return candidate;
        }
    }
    format!("{}_{}", base, chrono::Utc::now().timestamp())
}

/// Return a [`CreatorStamp`] for the currently-authenticated user, or `None`
/// if the user is offline / not logged in. Offline edits stay unsigned so the
/// Bliss-eligible audit trail only records provable identities.
pub fn current_stamp(auth: &crate::auth::AuthState) -> Option<CreatorStamp> {
    use crate::auth::AuthStatus;
    if auth.status != AuthStatus::LoggedIn { return None; }
    let user = auth.user.as_ref()?;
    Some(CreatorStamp {
        name: user.username.clone(),
        public_key: user.id.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Write an instance definition, stamping the modification audit trail.
///
/// Behaviour:
/// - If `stamp` is `Some` and `metadata.created_by` is `None`, sets `created_by`.
/// - If `stamp` is `Some`, appends a new entry to `modifications`. Every signed
///   save is preserved — no cap, no consolidation — because the chain serves
///   both Bliss attribution and AI training signal.
/// - Updates `metadata.last_modified` to the stamp's timestamp (or "now" if
///   unsigned).
///
/// Offline (`stamp == None`) writes leave the audit chain untouched.
pub fn write_instance_definition_signed(
    toml_path: &Path,
    instance: &mut InstanceDefinition,
    stamp: Option<&CreatorStamp>,
) -> Result<(), String> {
    match stamp {
        Some(s) => {
            if instance.metadata.created_by.is_none() {
                instance.metadata.created_by = Some(s.clone());
            }
            instance.metadata.modifications.push(s.clone());
            instance.metadata.last_modified = s.timestamp.clone();
        }
        None => {
            instance.metadata.last_modified = chrono::Utc::now().to_rfc3339();
        }
    }
    write_instance_definition(toml_path, instance)
}

/// Convert a raw `toml::Value` (the `value` field extracted from a rich-schema
/// `{ type = "...", value = ..., description = "..." }` inline table) into an
/// `AttributeValue` suitable for storage in the ECS `Attributes` component.
fn rich_toml_value_to_attribute(v: &toml::Value) -> Option<eustress_common::AttributeValue> {
    match v {
        toml::Value::Boolean(b) => Some(eustress_common::AttributeValue::Bool(*b)),
        toml::Value::Integer(i) => Some(eustress_common::AttributeValue::Int(*i)),
        toml::Value::Float(f)   => Some(eustress_common::AttributeValue::Number(*f)),
        toml::Value::String(s)  => Some(eustress_common::AttributeValue::String(s.clone())),
        toml::Value::Array(arr) => {
            let floats: Vec<f64> = arr.iter().filter_map(|item| match item {
                toml::Value::Float(f)   => Some(*f),
                toml::Value::Integer(i) => Some(*i as f64),
                _ => None,
            }).collect();
            match floats.len() {
                2 => Some(eustress_common::AttributeValue::Vector2(
                    Vec2::new(floats[0] as f32, floats[1] as f32),
                )),
                3 => Some(eustress_common::AttributeValue::Vector3(
                    Vec3::new(floats[0] as f32, floats[1] as f32, floats[2] as f32),
                )),
                4 => Some(eustress_common::AttributeValue::Color(
                    Color::srgba(floats[0] as f32, floats[1] as f32, floats[2] as f32, floats[3] as f32),
                )),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Known primitive mesh filenames that map to engine asset parts
const PRIMITIVE_MESHES: &[(&str, &str, eustress_common::classes::PartType)] = &[
    ("block", "parts/block.glb", eustress_common::classes::PartType::Block),
    ("ball", "parts/ball.glb", eustress_common::classes::PartType::Ball),
    ("cylinder", "parts/cylinder.glb", eustress_common::classes::PartType::Cylinder),
    ("wedge", "parts/wedge.glb", eustress_common::classes::PartType::Wedge),
    ("corner_wedge", "parts/corner_wedge.glb", eustress_common::classes::PartType::CornerWedge),
    ("cone", "parts/cone.glb", eustress_common::classes::PartType::Cone),
];

/// Bevy system — once a custom-mesh Part's asset finishes loading, compute
/// the mesh AABB and set `BasePart.size` to the AABB dimensions. Removes
/// the `NeedsMeshSize` marker so the work happens exactly once per entity.
/// Works for any Part that references a custom `.glb` — V-Cell was the
/// visible symptom but this generalises.
pub fn update_base_part_size_from_mesh(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    mut query: Query<(
        Entity,
        &Mesh3d,
        &mut eustress_common::classes::BasePart,
        Option<&mut Transform>,
    ), With<NeedsMeshSize>>,
) {
    for (entity, mesh_handle, mut base_part, transform) in query.iter_mut() {
        let Some(mesh) = meshes.get(&mesh_handle.0) else { continue; };
        let Some(aabb) = mesh.compute_aabb() else { continue; };

        // Mesh AABB half-extents → full size. Scale from the existing
        // Transform is preserved so the user can still stretch a part
        // beyond its natural size if desired.
        let half = aabb.half_extents;
        let natural_size = Vec3::new(half.x * 2.0, half.y * 2.0, half.z * 2.0);
        let scale_factor = transform.as_ref().map(|t| t.scale).unwrap_or(Vec3::ONE);
        base_part.size = Vec3::new(
            natural_size.x * scale_factor.x,
            natural_size.y * scale_factor.y,
            natural_size.z * scale_factor.z,
        );

        commands.entity(entity).remove::<NeedsMeshSize>();
    }
}

/// Cache of loaded primitive mesh handles to avoid repeated asset_server.load()
/// calls for the same GLB path across thousands of entities.
/// Without this cache, 10K entities each call `asset_server.load("parts/block.glb#Mesh0/Primitive0")`
/// which involves string formatting + path resolution per entity.
#[derive(Resource, Default)]
pub struct PrimitiveMeshCache {
    /// GLB asset path → loaded mesh handle
    cache: HashMap<String, Handle<Mesh>>,
}

impl PrimitiveMeshCache {
    /// Get or load a primitive mesh handle, caching the result.
    pub fn get_or_load(
        &mut self,
        asset_server: &AssetServer,
        glb_path: &str,
    ) -> Handle<Mesh> {
        self.cache.entry(glb_path.to_string()).or_insert_with(|| {
            asset_server.load(format!("{}#Mesh0/Primitive0", glb_path))
        }).clone()
    }
}

/// Spawn entity from instance definition, loading actual GLB meshes.
///
/// - **No asset** (`asset: None`): spawns a non-visual entity (Atmosphere, Sky, Moon, etc.)
/// - **Primitives** (block.glb, ball.glb, etc.): loaded from engine `assets/parts/`
/// - **Custom meshes** (V-Cell, user models): resolved relative to the .glb.toml
///   file's parent directory and loaded as a GLTF scene via AssetServer
///
/// Scale from [transform] sets the entity size via Transform.scale.
pub fn spawn_instance(
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    material_registry: &mut super::material_loader::MaterialRegistry,
    mesh_cache: &mut PrimitiveMeshCache,
    toml_path: PathBuf,
    instance: InstanceDefinition,
) -> Entity {
    // Instance display name: prefer metadata.name, fall back to folder/file name.
    // For folder-based instances (_instance.toml), use the parent folder name.
    // For legacy flat files (.glb.toml), use the filename stem.
    let name = instance.metadata.name.clone().unwrap_or_else(|| {
        let fname = toml_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if fname == "_instance.toml" {
            toml_path.parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string()
        } else {
            fname.split('.').next().unwrap_or("Unknown").to_string()
        }
    });

    // Parse class name early — needed for the no-mesh branch too
    let class_name = eustress_common::classes::ClassName::from_str(
        &instance.metadata.class_name
    ).unwrap_or(eustress_common::classes::ClassName::Part);

    // ── No mesh: spawn a non-visual Instance entity (Atmosphere, Sky, Moon, Star, etc.) ──
    if instance.asset.is_none() {
        // Parse rich-schema sections: each entry in `extra` is either a flat value
        // OR a named section (Table) whose entries are { type, value, description } inline tables.
        // Both cases are stored in Attributes for the Properties panel to display.
        let mut attrs = Attributes::new();
        for (_section_name, section_val) in &instance.extra {
            // Each top-level entry under [extra] is a section table (e.g. [Appearance])
            if let toml::Value::Table(props) = section_val {
                for (prop_key, prop_val) in props {
                    // Rich schema: { type = "...", value = ..., description = "..." }
                    let raw_value = if let toml::Value::Table(inline) = prop_val {
                        inline.get("value").cloned().unwrap_or(prop_val.clone())
                    } else {
                        prop_val.clone()
                    };
                    let attr_val = rich_toml_value_to_attribute(&raw_value);
                    if let Some(av) = attr_val {
                        attrs.set(prop_key, av);
                    }
                }
            } else {
                // Flat value at section level
                if let Some(av) = rich_toml_value_to_attribute(section_val) {
                    attrs.set(_section_name, av);
                }
            }
        }

        let entity = commands.spawn((
            eustress_common::classes::Instance {
                name: name.clone(),
                class_name,
                archivable: instance.metadata.archivable,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            Transform::from(instance.transform),
            Visibility::default(),
            Tags::new(),
            attrs,
            InstanceFile {
                toml_path: toml_path.clone(),
                mesh_path: PathBuf::new(),
                name: name.clone(),
            },
            Name::new(name.clone()),
        )).id();
        info!("🌅 Spawned non-visual instance '{}' ({}) from {:?}", name, instance.metadata.class_name, toml_path);
        return entity;
    }

    // ── Has mesh: resolve and load GLB ────────────────────────────────────────
    let asset_ref = instance.asset.as_ref().unwrap();
    // Resolve the mesh path: check if it's a known primitive or a custom GLB
    let mesh_ref = asset_ref.mesh.to_lowercase();
    let primitive = PRIMITIVE_MESHES.iter().find(|(hint, _, _)| {
        let fname = mesh_ref.rsplit('/').next().unwrap_or(&mesh_ref);
        fname.contains(hint)
    });
    
    let (is_custom_mesh, part_shape) = if let Some((_, _, shape)) = primitive {
        (false, *shape)
    } else {
        // Custom mesh — default to Block shape for bounding-box purposes
        (true, eustress_common::classes::PartType::Block)
    };
    
    // Determine the absolute path for the GLB mesh file. We normalize the
    // result so `..` segments (common when a folder-based Part references
    // `../meshes/Foo.glb`) don't leak into the asset URL. Without this,
    // Bevy's `space://` reader treats the `..` literally on some platforms
    // and the mesh fails to load silently — V-Cell's sub-parts all use this
    // relative shape, so they were the visible symptom.
    //
    // We normalize manually instead of using `canonicalize()` because on
    // Windows canonicalize prepends the `\\?\` verbatim prefix, which would
    // then fail `strip_prefix(&space_root)` downstream.
    fn normalize_path(p: &Path) -> PathBuf {
        use std::path::Component;
        let mut out = PathBuf::new();
        for comp in p.components() {
            match comp {
                Component::ParentDir => { out.pop(); }
                Component::CurDir => {} // skip "."
                _ => out.push(comp.as_os_str()),
            }
        }
        out
    }
    let toml_dir = toml_path.parent().unwrap_or(Path::new("."));
    let absolute_mesh_path = normalize_path(&toml_dir.join(&asset_ref.mesh));
    
    debug!("🔍 Instance '{}': mesh_ref='{}', is_custom={}, absolute_path={:?}, exists={}",
        name, mesh_ref, is_custom_mesh, absolute_mesh_path, absolute_mesh_path.exists());
    
    // Build material from properties — registry-first, enum fallback
    let [r, g, b, a] = instance.properties.color;
    let transparency = instance.properties.transparency;
    let base_color = Color::srgba(r, g, b, a);
    let material_handle = super::material_loader::resolve_material(
        &instance.properties.material,
        material_registry,
        materials,
        base_color,
        transparency,
        instance.properties.reflectance,
    );
    
    let scale = Vec3::from_array(instance.transform.scale);
    
    // Build BasePart so the Properties panel can read/display part properties
    let base_part = eustress_common::classes::BasePart {
        size: scale,
        color: Color::srgba(r, g, b, a),
        transparency,
        reflectance: instance.properties.reflectance,
        anchored: instance.properties.anchored,
        can_collide: instance.properties.can_collide,
        locked: instance.properties.locked,
        material: eustress_common::classes::Material::from_string(&instance.properties.material),
        material_name: instance.properties.material.clone(),
        cframe: Transform::from(instance.transform.clone()),
        ..default()
    };
    
    let transform = Transform::from(instance.transform);
    
    if is_custom_mesh && absolute_mesh_path.exists() {
        // Check for Draco compression before loading
        if super::draco_decoder::is_draco_compressed(&absolute_mesh_path) {
            super::draco_decoder::warn_draco_file(&absolute_mesh_path);
            // Fall through to primitive mesh rendering as fallback
        } else {
            // ── Custom GLB mesh: load the mesh directly (bypasses scene spawner) ──
            // Use the "space://" asset source which is registered to the Space root directory
            // Convert the absolute mesh path to a path relative to the Space root
            let space_root = super::default_space_root();
        let relative_mesh_path = absolute_mesh_path
            .strip_prefix(&space_root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| absolute_mesh_path.to_string_lossy().replace('\\', "/"));
        
        // Load mesh and material directly instead of using SceneRoot (avoids unregistered type panic)
        let mesh_path = format!("space://{}#Mesh0/Primitive0", relative_mesh_path);
        let material_path = format!("space://{}#Material0", relative_mesh_path);
        debug!("🔧 Loading mesh from: {} (absolute: {:?}, space_root: {:?})", mesh_path, absolute_mesh_path, space_root);
        let mesh_handle: Handle<Mesh> = asset_server.load(mesh_path);
        let material_handle: Handle<StandardMaterial> = asset_server.load(material_path);
        
        // Spawn the core visual entity first (no physics — added conditionally below)
        let entity = commands.spawn((
            Mesh3d(mesh_handle),
            MeshMaterial3d(material_handle),
            transform,
            Visibility::default(),
            eustress_common::classes::Instance {
                name: name.clone(),
                class_name,
                archivable: instance.metadata.archivable,
                id: 0,
                ai: false,
                uuid: String::new(),
            },
            base_part,
            eustress_common::classes::Part { shape: part_shape },
            PartEntity { part_id: String::new() }, // filled in below
            Attributes::new(),
            Tags::new(),
            InstanceFile {
                toml_path: toml_path.clone(),
                mesh_path: absolute_mesh_path,
                name: name.clone(),
            },
            Name::new(name.clone()),
            // Mark this entity so `update_base_part_size_from_mesh` computes
            // `BasePart.size` from the mesh AABB once the asset finishes
            // loading. Works for any custom-mesh part, not just V-Cell.
            NeedsMeshSize,
        )).id();
        let part_id = format!("{}v{}", entity.index(), entity.generation());
        let mut ec = commands.entity(entity);
        ec.insert(PartEntity { part_id });

        // Only add physics collider when can_collide is true — avoids broadphase
        // overhead for thousands of static decorative parts.
        // GLB meshes are unit meshes ([-0.5, 0.5]), so Transform.scale = part size in studs.
        // Avian3D colliders take HALF-extents for cuboid and HALF-height for cylinder.
        if instance.properties.can_collide {
            let half = scale * 0.5;
            let collider = match part_shape {
                eustress_common::classes::PartType::Ball => Collider::sphere(half.x),
                eustress_common::classes::PartType::Cylinder | eustress_common::classes::PartType::Cone => {
                    Collider::cylinder(half.x, half.y)
                }
                _ => Collider::cuboid(half.x, half.y, half.z),
            };
            ec.insert((collider, RigidBody::Static));
        }

        // Attach realism components if present in TOML
        if let Some(ref mat) = instance.material {
            ec.insert(mat.to_component());
            debug!("  + MaterialProperties: {}", mat.name);
        }
        if let Some(ref thermo) = instance.thermodynamic {
            ec.insert(thermo.to_component());
            debug!("  + ThermodynamicState: T={:.1}K P={:.0}Pa", thermo.temperature, thermo.pressure);
        }
        if let Some(ref echem) = instance.electrochemical {
            ec.insert(echem.to_component());
            debug!("  + ElectrochemicalState: V={:.2}V SOC={:.1}%", echem.voltage, echem.soc * 100.0);
        }
        // Attach UI ECS component if this is a UI class
        attach_ui_component(&mut ec, class_name, instance.ui.as_ref());
        debug!("Spawned custom mesh '{}' ({}) from {:?}", name, instance.metadata.class_name, toml_path);
        return entity;
        }
    }
    
    // Fallback to primitive mesh (either Draco-compressed or no custom mesh)
    // ── Primitive mesh: load from engine assets/parts/ ──
    let glb_path = if let Some((_, asset_path, _)) = primitive {
        *asset_path
    } else {
        "parts/block.glb" // fallback
    };
    let mesh_handle: Handle<Mesh> = mesh_cache.get_or_load(asset_server, glb_path);
    
    // Spawn the core visual entity first (no physics — added conditionally below)
    let entity = commands.spawn((
        Mesh3d(mesh_handle),
        MeshMaterial3d(material_handle),
        transform,
        Visibility::default(),
        eustress_common::classes::Instance {
            name: name.clone(),
            class_name,
            archivable: instance.metadata.archivable,
            id: 0,
            ai: false,
                uuid: String::new(),
        },
        base_part,
        eustress_common::classes::Part { shape: part_shape },
        PartEntity { part_id: String::new() }, // filled in below
        Attributes::new(),
        Tags::new(),
        InstanceFile {
            toml_path: toml_path.clone(),
            mesh_path: absolute_mesh_path,
            name: name.clone(),
        },
        Name::new(name.clone()),
    )).id();
    let part_id = format!("{}v{}", entity.index(), entity.generation());
    let mut ec = commands.entity(entity);
    ec.insert(PartEntity { part_id });

    // Only add physics collider when can_collide is true — avoids broadphase
    // overhead for thousands of static decorative parts.
    // Avian3D colliders take HALF-extents for cuboid and HALF-height for cylinder.
    if instance.properties.can_collide {
        let half = scale * 0.5;
        let collider = match part_shape {
            eustress_common::classes::PartType::Ball => Collider::sphere(half.x),
            eustress_common::classes::PartType::Cylinder | eustress_common::classes::PartType::Cone => {
                Collider::cylinder(half.x, half.y)
            }
            _ => Collider::cuboid(half.x, half.y, half.z),
        };
        ec.insert((collider, RigidBody::Static));
    }

    // Attach realism components if present in TOML
    if let Some(ref mat) = instance.material {
        ec.insert(mat.to_component());
        debug!("  + MaterialProperties: {}", mat.name);
    }
    if let Some(ref thermo) = instance.thermodynamic {
        ec.insert(thermo.to_component());
        debug!("  + ThermodynamicState: T={:.1}K P={:.0}Pa", thermo.temperature, thermo.pressure);
    }
    if let Some(ref echem) = instance.electrochemical {
        ec.insert(echem.to_component());
        debug!("  + ElectrochemicalState: V={:.2}V SOC={:.1}%", echem.voltage, echem.soc * 100.0);
    }
    // Attach UI ECS component if this is a UI class
    attach_ui_component(&mut ec, class_name, instance.ui.as_ref());
    debug!("Spawned primitive '{}' ({}) from {:?}", name, instance.metadata.class_name, toml_path);
    entity
}

/// Insert the appropriate ECS UI component onto an entity based on class name and [ui] data.
/// If no [ui] section is present, component defaults are used.
pub fn attach_ui_component(
    ec: &mut bevy::ecs::system::EntityCommands,
    class_name: eustress_common::classes::ClassName,
    ui: Option<&UiInstanceProperties>,
) {
    use eustress_common::classes::{
        ClassName, TextLabel, TextButton, TextBox, Frame, ImageLabel, ImageButton, ScrollingFrame,
    };
    let ui_defaults = UiInstanceProperties::default();
    let u = ui.unwrap_or(&ui_defaults);

    match class_name {
        ClassName::TextLabel => {
            ec.insert(TextLabel {
                text: u.text.clone(),
                rich_text: u.rich_text,
                text_scaled: u.text_scaled,
                text_wrapped: u.text_wrapped,
                max_visible_graphemes: -1,
                font: u.to_font(),
                font_size: u.font_size,
                line_height: if u.line_height > 0.0 { u.line_height } else { 1.0 },
                text_color3: u.text_color3,
                text_transparency: u.text_transparency,
                text_stroke_color3: u.text_stroke_color3,
                text_stroke_transparency: u.text_stroke_transparency,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                text_x_alignment: u.to_x_align(),
                text_y_alignment: u.to_y_align(),
                position: u.position_offset,
                size: u.size_offset,
                anchor_point: u.anchor_point,
                rotation: u.rotation,
                z_index: u.z_index,
                active: u.active,
                visible: u.visible,
                clips_descendants: u.clips_descendants,
                border_size_pixel: u.border_size_pixel,
                automatic_size: u.to_auto_size(),
                ..Default::default()
            });
        }
        ClassName::TextButton => {
            ec.insert(TextButton {
                text: u.text.clone(),
                font_size: u.font_size,
                text_color3: u.text_color3,
                text_transparency: u.text_transparency,
                text_stroke_color3: u.text_stroke_color3,
                text_stroke_transparency: u.text_stroke_transparency,
                text_x_alignment: u.to_x_align(),
                text_y_alignment: u.to_y_align(),
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                z_index: u.z_index,
                layout_order: u.layout_order,
                rotation: u.rotation,
                anchor_point: u.anchor_point,
                position_scale: u.position_scale,
                position_offset: u.position_offset,
                size_scale: u.size_scale,
                size_offset: u.size_offset,
                visible: u.visible,
                active: u.active,
                auto_button_color: u.auto_button_color,
                ..Default::default()
            });
        }
        ClassName::TextBox => {
            ec.insert(TextBox {
                text: u.text.clone(),
                font_size: u.font_size,
                text_color3: u.text_color3,
                text_transparency: u.text_transparency,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                z_index: u.z_index,
                visible: u.visible,
                ..Default::default()
            });
        }
        ClassName::Frame => {
            ec.insert(Frame {
                visible: u.visible,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                border_mode: u.to_border_mode(),
                clips_descendants: u.clips_descendants,
                z_index: u.z_index,
                layout_order: u.layout_order,
                rotation: u.rotation,
                anchor_point: u.anchor_point,
                position_scale: u.position_scale,
                position_offset: u.position_offset,
                size_scale: u.size_scale,
                size_offset: u.size_offset,
            });
        }
        ClassName::ImageLabel => {
            ec.insert(ImageLabel {
                image: u.image.clone(),
                image_color3: u.image_color3,
                image_transparency: u.image_transparency,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                z_index: u.z_index,
                layout_order: u.layout_order,
                rotation: u.rotation,
                anchor_point: u.anchor_point,
                position_scale: u.position_scale,
                position_offset: u.position_offset,
                size_scale: u.size_scale,
                size_offset: u.size_offset,
                visible: u.visible,
                ..Default::default()
            });
        }
        ClassName::ImageButton => {
            ec.insert(ImageButton {
                image: u.image.clone(),
                image_color3: u.image_color3,
                image_transparency: u.image_transparency,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                z_index: u.z_index,
                layout_order: u.layout_order,
                rotation: u.rotation,
                anchor_point: u.anchor_point,
                position_scale: u.position_scale,
                position_offset: u.position_offset,
                size_scale: u.size_scale,
                size_offset: u.size_offset,
                visible: u.visible,
                active: u.active,
                auto_button_color: u.auto_button_color,
                ..Default::default()
            });
        }
        ClassName::ScrollingFrame => {
            ec.insert(ScrollingFrame {
                visible: u.visible,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                z_index: u.z_index,
                layout_order: u.layout_order,
                rotation: u.rotation,
                anchor_point: u.anchor_point,
                position_scale: u.position_scale,
                position_offset: u.position_offset,
                size_scale: u.size_scale,
                size_offset: u.size_offset,
                scrolling_enabled: u.scrolling_enabled,
                scroll_bar_thickness: u.scroll_bar_thickness,
                ..Default::default()
            });
        }
        _ => {}
    }
}

// NOTE: Instance loading is handled by SpaceFileLoaderPlugin (file_loader.rs)
// which properly creates folder hierarchy with parent-child relationships.
// The load_instance_files_system was removed to avoid duplicate loading.

/// System to write instance changes back to .glb.toml files.
///
/// PERF: Uses `Changed<Transform>` BUT excludes `Added<Transform>`.
/// Bevy marks newly-inserted components as Changed, so without the exclusion
/// ALL 10K entities would trigger 20K disk I/O ops on the first frame after
/// spawn (read TOML + write TOML per entity = 1-second freeze).
/// Only entities whose Transform was **modified** (gizmo, properties panel)
/// after initial spawn will be written back.
pub fn write_instance_changes_system(
    instances: Query<(Entity, &Transform, &InstanceFile), Changed<Transform>>,
    added_instances: Query<Entity, Added<Transform>>,
    mut recently_written: ResMut<super::file_watcher::RecentlyWrittenFiles>,
) {
    // Collect entities that were just added this tick — skip them entirely.
    // Bevy marks newly-inserted components as Changed, so without this check
    // ALL 10K entities would trigger 20K disk I/O ops on their first frame.
    let just_added: std::collections::HashSet<Entity> = added_instances.iter().collect();

    // Collect all write jobs this frame, then dispatch to background thread.
    let mut jobs: Vec<(std::path::PathBuf, TransformData)> = Vec::new();

    for (entity, transform, instance_file) in instances.iter() {
        if just_added.contains(&entity) {
            continue;
        }
        if recently_written.was_recently_written(&instance_file.toml_path) {
            continue;
        }

        recently_written.mark_written(instance_file.toml_path.clone());
        jobs.push((instance_file.toml_path.clone(), TransformData::from(*transform)));
    }

    if jobs.is_empty() {
        return;
    }

    // Dispatch all writes to a background thread — never block the main frame.
    let job_count = jobs.len();
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        for (path, transform_data) in &jobs {
            match load_instance_definition(path) {
                Ok(mut instance) => {
                    instance.transform = transform_data.clone();
                    instance.metadata.last_modified = chrono::Utc::now().to_rfc3339();
                    if let Err(e) = write_instance_definition(path, &instance) {
                        tracing::error!("Failed to write instance {:?}: {}", path, e);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to load instance for write-back {:?}: {}", path, e);
                }
            }
        }
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 50 {
            tracing::warn!("🐌 Background instance writes: {:.1}ms ({} files)", elapsed.as_secs_f64() * 1000.0, job_count);
        }
    });
}
