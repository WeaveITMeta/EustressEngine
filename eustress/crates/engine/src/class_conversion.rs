//! Class Conversion System
//!
//! Handles converting Eustress instances from one class to another in Studio.
//! See docs/development/CLASS_CONVERSION.md for full specification.

use bevy::prelude::*;
use std::path::{Path, PathBuf};

use eustress_common::classes::ClassName;

// ============================================================================
// Conversion Categories
// ============================================================================

/// Category of a class for conversion compatibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversionCategory {
    /// Category A: Solid Geometry (BasePart descendants)
    Geometry,
    /// Category B: Containers (Folder-based)
    Container,
    /// Category C: GUI Containers
    GuiContainer,
    /// Category D: GUI Leaves
    GuiLeaf,
    /// Category E: Lights
    Light,
    /// Category F: Constraints/Attachments
    Constraint,
    /// Category X: Non-Convertible
    NonConvertible,
}

impl ConversionCategory {
    /// Get the category for a class name
    pub fn from_class_name(class_name: ClassName) -> Self {
        match class_name {
            // Category A: Geometry
            ClassName::Part | ClassName::Seat | ClassName::VehicleSeat | ClassName::SpawnLocation => {
                Self::Geometry
            }
            
            // Category B: Containers
            ClassName::Model | ClassName::Folder => Self::Container,
            
            // Category C: GUI Containers
            ClassName::ScreenGui | ClassName::BillboardGui | ClassName::SurfaceGui |
            ClassName::Frame | ClassName::ScrollingFrame => Self::GuiContainer,
            
            // Category D: GUI Leaves
            ClassName::TextLabel | ClassName::TextButton | ClassName::TextBox |
            ClassName::ImageLabel | ClassName::ImageButton => Self::GuiLeaf,
            
            // Category E: Lights
            ClassName::PointLight | ClassName::SpotLight | ClassName::SurfaceLight |
            ClassName::DirectionalLight => Self::Light,
            
            // Category F: Constraints
            ClassName::Attachment | ClassName::WeldConstraint | ClassName::Motor6D => {
                Self::Constraint
            }
            
            // Category X: Non-Convertible
            _ => Self::NonConvertible,
        }
    }
}

// ============================================================================
// Conversion Result
// ============================================================================

/// Result of a conversion check
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionResult {
    /// Direct conversion, all data preserved
    Direct,
    /// Conversion with data loss (some sections dropped)
    WithDataLoss,
    /// Not convertible
    NotConvertible,
    /// Same class (no-op)
    SameClass,
}

impl ConversionResult {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Direct | Self::WithDataLoss)
    }
}

// ============================================================================
// Conversion Matrix
// ============================================================================

/// Check if conversion from one class to another is allowed
pub fn can_convert(from: ClassName, to: ClassName) -> ConversionResult {
    if from == to {
        return ConversionResult::SameClass;
    }
    
    let from_cat = ConversionCategory::from_class_name(from);
    let to_cat = ConversionCategory::from_class_name(to);
    
    // Non-convertible classes cannot be converted to or from
    if from_cat == ConversionCategory::NonConvertible || to_cat == ConversionCategory::NonConvertible {
        return ConversionResult::NotConvertible;
    }
    
    // Same category = direct conversion (usually)
    if from_cat == to_cat {
        return match from_cat {
            ConversionCategory::Geometry => ConversionResult::Direct,
            ConversionCategory::Container => ConversionResult::Direct,
            ConversionCategory::Light => ConversionResult::Direct,
            ConversionCategory::GuiContainer => {
                // GUI containers have sub-categories
                check_gui_container_conversion(from, to)
            }
            ConversionCategory::GuiLeaf => {
                // Text ↔ Image conversions lose data
                check_gui_leaf_conversion(from, to)
            }
            ConversionCategory::Constraint => {
                // Attachment cannot convert to constraints
                check_constraint_conversion(from, to)
            }
            ConversionCategory::NonConvertible => ConversionResult::NotConvertible,
        };
    }
    
    // Cross-category conversions are not allowed
    ConversionResult::NotConvertible
}

fn check_gui_container_conversion(from: ClassName, to: ClassName) -> ConversionResult {
    // Root GUI containers (ScreenGui, BillboardGui, SurfaceGui) can convert between each other with data loss
    let root_guis = [ClassName::ScreenGui, ClassName::BillboardGui, ClassName::SurfaceGui];
    let child_containers = [ClassName::Frame, ClassName::ScrollingFrame];
    
    let from_is_root = root_guis.contains(&from);
    let to_is_root = root_guis.contains(&to);
    let from_is_child = child_containers.contains(&from);
    let to_is_child = child_containers.contains(&to);
    
    if from_is_root && to_is_root {
        ConversionResult::WithDataLoss
    } else if from_is_child && to_is_child {
        ConversionResult::Direct
    } else {
        ConversionResult::NotConvertible
    }
}

fn check_gui_leaf_conversion(from: ClassName, to: ClassName) -> ConversionResult {
    let text_classes = [ClassName::TextLabel, ClassName::TextButton, ClassName::TextBox];
    let image_classes = [ClassName::ImageLabel, ClassName::ImageButton];
    
    let from_is_text = text_classes.contains(&from);
    let to_is_text = text_classes.contains(&to);
    let from_is_image = image_classes.contains(&from);
    let to_is_image = image_classes.contains(&to);
    
    if (from_is_text && to_is_text) || (from_is_image && to_is_image) {
        ConversionResult::Direct
    } else {
        // Text ↔ Image loses content
        ConversionResult::WithDataLoss
    }
}

fn check_constraint_conversion(from: ClassName, to: ClassName) -> ConversionResult {
    // Attachment is a position marker, not a constraint
    if from == ClassName::Attachment || to == ClassName::Attachment {
        return ConversionResult::NotConvertible;
    }
    
    // Weld ↔ Motor6D works but Motor6D loses animation fields when going to Weld
    if from == ClassName::Motor6D && to == ClassName::WeldConstraint {
        ConversionResult::WithDataLoss
    } else {
        ConversionResult::Direct
    }
}

/// Get all valid conversion targets for a class
pub fn get_valid_targets(from: ClassName) -> Vec<(ClassName, ConversionResult)> {
    let all_classes = [
        // Category A
        ClassName::Part, ClassName::Seat, ClassName::VehicleSeat, ClassName::SpawnLocation,
        // Category B
        ClassName::Model, ClassName::Folder,
        // Category C
        ClassName::ScreenGui, ClassName::BillboardGui, ClassName::SurfaceGui,
        ClassName::Frame, ClassName::ScrollingFrame,
        // Category D
        ClassName::TextLabel, ClassName::TextButton, ClassName::TextBox,
        ClassName::ImageLabel, ClassName::ImageButton,
        // Category E
        ClassName::PointLight, ClassName::SpotLight, ClassName::SurfaceLight, ClassName::DirectionalLight,
        // Category F
        ClassName::WeldConstraint, ClassName::Motor6D,
    ];
    
    all_classes
        .iter()
        .filter_map(|&to| {
            let result = can_convert(from, to);
            if result.is_allowed() {
                Some((to, result))
            } else {
                None
            }
        })
        .collect()
}

// ============================================================================
// Section Survival
// ============================================================================

/// Sections that are always preserved during conversion
pub const ALWAYS_PRESERVED_SECTIONS: &[&str] = &[
    "instance",
    "metadata",
    "tags",
    "attributes",
    "parameters",
    "consent",
];

/// Sections preserved for Category A (Geometry) conversions
pub const GEOMETRY_SECTIONS: &[&str] = &[
    "transform",
    "geometry",
    "appearance",
    "physics",
    "rendering",
    "asset",
];

/// Sections preserved for Category B (Container) conversions
pub const CONTAINER_SECTIONS: &[&str] = &[
    // No additional sections beyond always-preserved
];

/// Sections preserved for Category C/D (GUI) conversions
pub const GUI_SECTIONS: &[&str] = &[
    "gui",
    "layout",
];

/// Sections preserved for Category E (Light) conversions
pub const LIGHT_SECTIONS: &[&str] = &[
    "light",
];

/// Get sections to preserve when converting between classes
pub fn get_preserved_sections(from: ClassName, to: ClassName) -> Vec<&'static str> {
    let mut sections: Vec<&'static str> = ALWAYS_PRESERVED_SECTIONS.to_vec();
    
    let from_cat = ConversionCategory::from_class_name(from);
    let to_cat = ConversionCategory::from_class_name(to);
    
    // Add category-specific sections if staying in same category
    if from_cat == to_cat {
        match from_cat {
            ConversionCategory::Geometry => {
                sections.extend(GEOMETRY_SECTIONS);
            }
            ConversionCategory::Container => {
                sections.extend(CONTAINER_SECTIONS);
            }
            ConversionCategory::GuiContainer | ConversionCategory::GuiLeaf => {
                sections.extend(GUI_SECTIONS);
            }
            ConversionCategory::Light => {
                sections.extend(LIGHT_SECTIONS);
            }
            _ => {}
        }
    }
    
    sections
}

/// Get sections to add with defaults for the target class
pub fn get_sections_to_add(to: ClassName) -> Vec<&'static str> {
    match to {
        ClassName::Seat => vec!["seat"],
        ClassName::VehicleSeat => vec!["vehicleseat"],
        ClassName::SpawnLocation => vec!["spawn"],
        ClassName::Model => vec!["model"],
        ClassName::ScreenGui => vec!["screengui"],
        ClassName::BillboardGui => vec!["billboardgui"],
        ClassName::SurfaceGui => vec!["surfacegui"],
        ClassName::Frame => vec!["frame"],
        ClassName::ScrollingFrame => vec!["scrollingframe"],
        ClassName::TextLabel | ClassName::TextButton | ClassName::TextBox => vec!["text"],
        ClassName::ImageLabel | ClassName::ImageButton => vec!["image"],
        ClassName::SpotLight => vec!["spot"],
        ClassName::SurfaceLight => vec!["surface"],
        ClassName::Motor6D => vec!["motor6d"],
        ClassName::WeldConstraint => vec!["weld"],
        _ => vec![],
    }
}

// ============================================================================
// File Extension Mapping
// ============================================================================

/// Get the file extension for a class
pub fn get_extension(class_name: ClassName) -> &'static str {
    match class_name {
        ClassName::Part => ".part.toml",
        ClassName::Seat => ".seat.toml",
        ClassName::VehicleSeat => ".vehicleseat.toml",
        ClassName::SpawnLocation => ".spawn.toml",
        ClassName::PointLight => ".pointlight.toml",
        ClassName::SpotLight => ".spotlight.toml",
        ClassName::SurfaceLight => ".surfacelight.toml",
        ClassName::DirectionalLight => ".dirlight.toml",
        ClassName::TextLabel => ".textlabel.toml",
        ClassName::TextButton => ".textbutton.toml",
        ClassName::TextBox => ".textbox.toml",
        ClassName::ImageLabel => ".imagelabel.toml",
        ClassName::ImageButton => ".imagebutton.toml",
        ClassName::WeldConstraint => ".weld.toml",
        ClassName::Motor6D => ".motor6d.toml",
        ClassName::Attachment => ".attachment.toml",
        ClassName::Sound => ".sound.toml",
        ClassName::ParticleEmitter => ".particles.toml",
        ClassName::Beam => ".beam.toml",
        ClassName::Decal => ".decal.toml",
        ClassName::Camera => ".camera.toml",
        ClassName::Humanoid => ".humanoid.toml",
        ClassName::Animator => ".animator.toml",
        ClassName::Star => ".star.toml",
        ClassName::Moon => ".moon.toml",
        ClassName::Atmosphere => ".atmosphere.toml",
        ClassName::Clouds => ".clouds.toml",
        ClassName::Sky => ".sky.toml",
        ClassName::Terrain => ".terrain.toml",
        ClassName::ChunkedWorld => "_instance.toml",
        // Containers use _instance.toml
        ClassName::Model | ClassName::Folder |
        ClassName::ScreenGui | ClassName::BillboardGui | ClassName::SurfaceGui |
        ClassName::Frame | ClassName::ScrollingFrame => "_instance.toml",
        // Default
        _ => ".toml",
    }
}

/// Check if a class uses folder-based storage
pub fn is_folder_based(class_name: ClassName) -> bool {
    matches!(
        class_name,
        ClassName::Model | ClassName::Folder |
        ClassName::ScreenGui | ClassName::BillboardGui | ClassName::SurfaceGui |
        ClassName::Frame | ClassName::ScrollingFrame |
        ClassName::ChunkedWorld
    )
}

// ============================================================================
// Conversion Operation
// ============================================================================

/// A conversion operation to perform
#[derive(Debug, Clone)]
pub struct ConversionOperation {
    /// Source file path
    pub source_path: PathBuf,
    /// Target file path (may be same directory with different extension)
    pub target_path: PathBuf,
    /// Source class
    pub from_class: ClassName,
    /// Target class
    pub to_class: ClassName,
    /// Sections to preserve from source
    pub preserved_sections: Vec<String>,
    /// Sections to add with defaults
    pub sections_to_add: Vec<String>,
    /// Whether this conversion has data loss
    pub has_data_loss: bool,
}

impl ConversionOperation {
    /// Create a new conversion operation
    pub fn new(source_path: PathBuf, from_class: ClassName, to_class: ClassName) -> Result<Self, ConversionError> {
        let result = can_convert(from_class, to_class);
        
        if !result.is_allowed() {
            return Err(ConversionError::NotConvertible {
                from: from_class,
                to: to_class,
            });
        }
        
        let preserved = get_preserved_sections(from_class, to_class)
            .iter()
            .map(|s| s.to_string())
            .collect();
        
        let to_add = get_sections_to_add(to_class)
            .iter()
            .map(|s| s.to_string())
            .collect();
        
        // Calculate target path
        let target_path = if is_folder_based(from_class) && is_folder_based(to_class) {
            // Same path, just update _instance.toml content
            source_path.clone()
        } else if is_folder_based(from_class) && !is_folder_based(to_class) {
            // Converting from folder to file - error
            return Err(ConversionError::FolderToFileNotSupported);
        } else if !is_folder_based(from_class) && is_folder_based(to_class) {
            // Converting from file to folder - error
            return Err(ConversionError::FileToFolderNotSupported);
        } else {
            // File to file - change extension
            let parent = source_path.parent().unwrap_or(Path::new(""));
            let stem = source_path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed");
            
            // Remove old extension suffix (e.g., "Chair.seat" -> "Chair")
            let base_name = stem.split('.').next().unwrap_or(stem);
            let new_ext = get_extension(to_class);
            
            parent.join(format!("{}{}", base_name, new_ext))
        };
        
        Ok(Self {
            source_path,
            target_path,
            from_class,
            to_class,
            preserved_sections: preserved,
            sections_to_add: to_add,
            has_data_loss: result == ConversionResult::WithDataLoss,
        })
    }
    
    /// Execute the conversion
    pub fn execute(&self) -> Result<(), ConversionError> {
        // Read source TOML
        let source_content = std::fs::read_to_string(&self.source_path)
            .map_err(|e| ConversionError::IoError(e.to_string()))?;
        
        let mut source_data: toml::Table = toml::from_str(&source_content)
            .map_err(|e| ConversionError::ParseError(e.to_string()))?;
        
        // Build target TOML
        let mut target_data = toml::Table::new();
        
        // Copy preserved sections
        for section in &self.preserved_sections {
            if let Some(value) = source_data.remove(section.as_str()) {
                target_data.insert(section.clone(), value);
            }
        }
        
        // Update class_name in [instance]
        if let Some(toml::Value::Table(ref mut instance)) = target_data.get_mut("instance") {
            instance.insert(
                "class_name".to_string(),
                toml::Value::String(self.to_class.as_str().to_string()),
            );
        }
        
        // Update last_modified in [metadata]
        if let Some(toml::Value::Table(ref mut metadata)) = target_data.get_mut("metadata") {
            let now = chrono::Utc::now().to_rfc3339();
            metadata.insert(
                "last_modified".to_string(),
                toml::Value::String(now),
            );
        }
        
        // Add new sections with defaults
        for section in &self.sections_to_add {
            if !target_data.contains_key(section.as_str()) {
                target_data.insert(
                    section.clone(),
                    toml::Value::Table(get_default_section(section)),
                );
            }
        }
        
        // Write target file
        let target_content = toml::to_string_pretty(&target_data)
            .map_err(|e| ConversionError::SerializeError(e.to_string()))?;
        
        std::fs::write(&self.target_path, target_content)
            .map_err(|e| ConversionError::IoError(e.to_string()))?;
        
        // Delete source file if different from target
        if self.source_path != self.target_path {
            std::fs::remove_file(&self.source_path)
                .map_err(|e| ConversionError::IoError(e.to_string()))?;
        }
        
        Ok(())
    }
}

/// Get default values for a section
fn get_default_section(section: &str) -> toml::Table {
    let mut table = toml::Table::new();
    
    match section {
        "seat" => {
            table.insert("disabled".to_string(), toml::Value::Boolean(false));
            table.insert("sit_height".to_string(), toml::Value::Float(0.0));
        }
        "vehicleseat" => {
            table.insert("disabled".to_string(), toml::Value::Boolean(false));
            table.insert("max_speed".to_string(), toml::Value::Float(25.0));
            table.insert("torque".to_string(), toml::Value::Float(10.0));
            table.insert("turn_speed".to_string(), toml::Value::Float(1.0));
        }
        "spawn" => {
            table.insert("allow_team_change_on_touch".to_string(), toml::Value::Boolean(false));
            table.insert("duration".to_string(), toml::Value::Float(10.0));
            table.insert("enabled".to_string(), toml::Value::Boolean(true));
            table.insert("neutral".to_string(), toml::Value::Boolean(true));
        }
        "model" => {
            table.insert("primary_part".to_string(), toml::Value::String(String::new()));
        }
        "screengui" => {
            table.insert("enabled".to_string(), toml::Value::Boolean(true));
            table.insert("display_order".to_string(), toml::Value::Integer(0));
            table.insert("ignore_gui_inset".to_string(), toml::Value::Boolean(false));
        }
        "billboardgui" => {
            table.insert("enabled".to_string(), toml::Value::Boolean(true));
            table.insert("adornee".to_string(), toml::Value::String(String::new()));
            table.insert("size".to_string(), toml::Value::Array(vec![
                toml::Value::Float(100.0),
                toml::Value::Float(100.0),
            ]));
            table.insert("always_on_top".to_string(), toml::Value::Boolean(false));
        }
        "surfacegui" => {
            table.insert("enabled".to_string(), toml::Value::Boolean(true));
            table.insert("adornee".to_string(), toml::Value::String(String::new()));
            table.insert("face".to_string(), toml::Value::String("Front".to_string()));
        }
        "text" => {
            table.insert("text".to_string(), toml::Value::String("Label".to_string()));
            table.insert("text_color".to_string(), toml::Value::Array(vec![
                toml::Value::Float(0.0),
                toml::Value::Float(0.0),
                toml::Value::Float(0.0),
                toml::Value::Float(1.0),
            ]));
            table.insert("font_size".to_string(), toml::Value::Float(14.0));
        }
        "image" => {
            table.insert("image".to_string(), toml::Value::String(String::new()));
            table.insert("image_color".to_string(), toml::Value::Array(vec![
                toml::Value::Float(1.0),
                toml::Value::Float(1.0),
                toml::Value::Float(1.0),
                toml::Value::Float(1.0),
            ]));
        }
        "spot" => {
            table.insert("angle".to_string(), toml::Value::Float(45.0));
        }
        "surface" => {
            table.insert("face".to_string(), toml::Value::String("Front".to_string()));
        }
        "motor6d" => {
            table.insert("part0".to_string(), toml::Value::String(String::new()));
            table.insert("part1".to_string(), toml::Value::String(String::new()));
            table.insert("c0".to_string(), toml::Value::Array(vec![
                toml::Value::Float(0.0), toml::Value::Float(0.0), toml::Value::Float(0.0),
                toml::Value::Float(0.0), toml::Value::Float(0.0), toml::Value::Float(0.0),
            ]));
            table.insert("c1".to_string(), toml::Value::Array(vec![
                toml::Value::Float(0.0), toml::Value::Float(0.0), toml::Value::Float(0.0),
                toml::Value::Float(0.0), toml::Value::Float(0.0), toml::Value::Float(0.0),
            ]));
        }
        "weld" => {
            table.insert("part0".to_string(), toml::Value::String(String::new()));
            table.insert("part1".to_string(), toml::Value::String(String::new()));
        }
        _ => {}
    }
    
    table
}

// ============================================================================
// Errors
// ============================================================================

/// Conversion errors
#[derive(Debug, Clone)]
pub enum ConversionError {
    NotConvertible { from: ClassName, to: ClassName },
    IoError(String),
    ParseError(String),
    SerializeError(String),
    FolderToFileNotSupported,
    FileToFolderNotSupported,
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConvertible { from, to } => {
                write!(f, "Cannot convert {} to {}", from.as_str(), to.as_str())
            }
            Self::IoError(msg) => write!(f, "IO error: {}", msg),
            Self::ParseError(msg) => write!(f, "Parse error: {}", msg),
            Self::SerializeError(msg) => write!(f, "Serialize error: {}", msg),
            Self::FolderToFileNotSupported => {
                write!(f, "Converting folder-based class to file-based class is not supported")
            }
            Self::FileToFolderNotSupported => {
                write!(f, "Converting file-based class to folder-based class is not supported")
            }
        }
    }
}

impl std::error::Error for ConversionError {}

// ============================================================================
// Bevy Plugin
// ============================================================================

/// Plugin for class conversion in Studio
pub struct ClassConversionPlugin;

impl Plugin for ClassConversionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, handle_conversion_events);
    }
}

/// Event to request a class conversion
#[derive(Message, Debug, Clone)]
pub struct ConvertClassEvent {
    /// Entity to convert
    pub entity: Entity,
    /// Target class
    pub target_class: ClassName,
    /// Source file path (if known)
    pub source_path: Option<PathBuf>,
}

/// Event fired when conversion is complete
#[derive(Message, Debug, Clone)]
pub struct ConversionCompleteEvent {
    /// Entity that was converted
    pub entity: Entity,
    /// Original class
    pub from_class: ClassName,
    /// New class
    pub to_class: ClassName,
    /// New file path
    pub new_path: PathBuf,
    /// Whether there was data loss
    pub had_data_loss: bool,
}

/// System to handle conversion events
fn handle_conversion_events(
    mut convert_events: MessageReader<ConvertClassEvent>,
    mut complete_events: MessageWriter<ConversionCompleteEvent>,
    query: Query<&eustress_common::classes::Instance>,
) {
    for event in convert_events.read() {
        let Ok(instance) = query.get(event.entity) else {
            warn!("ConvertClassEvent: Entity {:?} has no Instance component", event.entity);
            continue;
        };
        
        let from_class = instance.class_name;
        let to_class = event.target_class;
        
        // Check if conversion is valid
        let result = can_convert(from_class, to_class);
        if !result.is_allowed() {
            warn!(
                "Cannot convert {} to {}: {:?}",
                from_class.as_str(),
                to_class.as_str(),
                result
            );
            continue;
        }
        
        // Get source path
        let Some(source_path) = &event.source_path else {
            warn!("ConvertClassEvent: No source path provided for entity {:?}", event.entity);
            continue;
        };
        
        // Create and execute conversion operation
        match ConversionOperation::new(source_path.clone(), from_class, to_class) {
            Ok(operation) => {
                let new_path = operation.target_path.clone();
                let had_data_loss = operation.has_data_loss;
                
                if let Err(e) = operation.execute() {
                    error!("Conversion failed: {}", e);
                    continue;
                }
                
                info!(
                    "Converted {} from {} to {} (data loss: {})",
                    source_path.display(),
                    from_class.as_str(),
                    to_class.as_str(),
                    had_data_loss
                );
                
                complete_events.write(ConversionCompleteEvent {
                    entity: event.entity,
                    from_class,
                    to_class,
                    new_path,
                    had_data_loss,
                });
            }
            Err(e) => {
                error!("Failed to create conversion operation: {}", e);
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_geometry_conversions() {
        assert_eq!(can_convert(ClassName::Part, ClassName::Seat), ConversionResult::Direct);
        assert_eq!(can_convert(ClassName::Seat, ClassName::Part), ConversionResult::Direct);
        assert_eq!(can_convert(ClassName::Part, ClassName::VehicleSeat), ConversionResult::Direct);
    }
    
    #[test]
    fn test_container_conversions() {
        assert_eq!(can_convert(ClassName::Model, ClassName::Folder), ConversionResult::Direct);
        assert_eq!(can_convert(ClassName::Folder, ClassName::Model), ConversionResult::Direct);
    }
    
    #[test]
    fn test_cross_category_blocked() {
        assert_eq!(can_convert(ClassName::Part, ClassName::Model), ConversionResult::NotConvertible);
        assert_eq!(can_convert(ClassName::Part, ClassName::PointLight), ConversionResult::NotConvertible);
        assert_eq!(can_convert(ClassName::TextLabel, ClassName::Part), ConversionResult::NotConvertible);
    }
    
    #[test]
    fn test_non_convertible_classes() {
        assert_eq!(can_convert(ClassName::Terrain, ClassName::Part), ConversionResult::NotConvertible);
        assert_eq!(can_convert(ClassName::Part, ClassName::Terrain), ConversionResult::NotConvertible);
        assert_eq!(can_convert(ClassName::Camera, ClassName::Humanoid), ConversionResult::NotConvertible);
    }
    
    #[test]
    fn test_gui_leaf_conversions() {
        assert_eq!(can_convert(ClassName::TextLabel, ClassName::TextButton), ConversionResult::Direct);
        assert_eq!(can_convert(ClassName::ImageLabel, ClassName::ImageButton), ConversionResult::Direct);
        assert_eq!(can_convert(ClassName::TextLabel, ClassName::ImageLabel), ConversionResult::WithDataLoss);
    }
    
    #[test]
    fn test_light_conversions() {
        assert_eq!(can_convert(ClassName::PointLight, ClassName::SpotLight), ConversionResult::Direct);
        assert_eq!(can_convert(ClassName::SpotLight, ClassName::DirectionalLight), ConversionResult::Direct);
    }
    
    #[test]
    fn test_constraint_conversions() {
        assert_eq!(can_convert(ClassName::WeldConstraint, ClassName::Motor6D), ConversionResult::Direct);
        assert_eq!(can_convert(ClassName::Motor6D, ClassName::WeldConstraint), ConversionResult::WithDataLoss);
        assert_eq!(can_convert(ClassName::Attachment, ClassName::WeldConstraint), ConversionResult::NotConvertible);
    }
    
    #[test]
    fn test_get_valid_targets() {
        let targets = get_valid_targets(ClassName::Part);
        assert!(targets.iter().any(|(c, _)| *c == ClassName::Seat));
        assert!(targets.iter().any(|(c, _)| *c == ClassName::VehicleSeat));
        assert!(!targets.iter().any(|(c, _)| *c == ClassName::Model));
    }
}
