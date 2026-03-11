//! Runtime UI System - User-Created Game UI powered by Slint
//!
//! This module handles dynamic UI rendering for user-created game interfaces
//! (ScreenGui, Frame, TextLabel, etc.) defined in TOML files.
//!
//! See docs/development/SLINT_UI_SYSTEM.md for full specification.

use bevy::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eustress_common::classes::ClassName;

// ============================================================================
// Runtime UI Plugin
// ============================================================================

/// Marker component: this entity has already had Bevy UI nodes spawned for it.
/// Prevents re-spawning every frame.
#[derive(Component, Debug, Clone)]
pub struct BevyGuiSpawned;

/// Plugin for managing user-created runtime UI
pub struct RuntimeUIPlugin;

impl Plugin for RuntimeUIPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RuntimeUIManager>()
            .init_resource::<ShowDevelopmentUIState>()
            .add_systems(Update, (
                watch_ui_files,
                process_ui_events,
                spawn_bevy_gui_from_loaded_entities,
                sync_development_ui_visibility,
            ));
    }
}

/// Cached state so sync_development_ui_visibility only runs when the value changes
#[derive(Resource, Default)]
struct ShowDevelopmentUIState {
    last_value: Option<bool>,
}

// ============================================================================
// Resources
// ============================================================================

/// Manages all user-created runtime UI instances
#[derive(Resource, Default)]
pub struct RuntimeUIManager {
    /// Active ScreenGuis (player-local, full-screen overlays)
    pub screen_guis: HashMap<Entity, ScreenGuiInstance>,
    
    /// Active BillboardGuis (world-space, camera-facing)
    pub billboard_guis: HashMap<Entity, BillboardGuiInstance>,
    
    /// Active SurfaceGuis (world-space, surface-aligned)
    pub surface_guis: HashMap<Entity, SurfaceGuiInstance>,
    
    /// File modification times for hot-reload
    file_mtimes: HashMap<PathBuf, SystemTime>,
}

impl RuntimeUIManager {
    /// Load a ScreenGui from a TOML file
    pub fn load_screen_gui(&mut self, entity: Entity, path: &Path) -> Result<(), RuntimeUIError> {
        let toml_content = std::fs::read_to_string(path)
            .map_err(|e| RuntimeUIError::IoError(e.to_string()))?;
        
        let data: toml::Value = toml::from_str(&toml_content)
            .map_err(|e| RuntimeUIError::ParseError(e.to_string()))?;
        
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        
        self.file_mtimes.insert(path.to_path_buf(), mtime);
        
        let instance = ScreenGuiInstance {
            path: path.to_path_buf(),
            elements: HashMap::new(),
            mtime,
            enabled: data.get("screengui")
                .and_then(|s| s.get("enabled"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            display_order: data.get("screengui")
                .and_then(|s| s.get("display_order"))
                .and_then(|v| v.as_integer())
                .unwrap_or(0) as i32,
            ignore_gui_inset: data.get("screengui")
                .and_then(|s| s.get("ignore_gui_inset"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        };
        
        self.screen_guis.insert(entity, instance);
        Ok(())
    }
    
    /// Check if any UI files have been modified and need reloading
    pub fn check_for_updates(&mut self) -> Vec<PathBuf> {
        let mut updated = Vec::new();
        
        for (path, cached_mtime) in &self.file_mtimes {
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(current_mtime) = metadata.modified() {
                    if current_mtime > *cached_mtime {
                        updated.push(path.clone());
                    }
                }
            }
        }
        
        updated
    }
    
    /// Reload a UI file
    pub fn reload(&mut self, path: &Path) -> Result<(), RuntimeUIError> {
        // Find which entity owns this path
        for (entity, instance) in &self.screen_guis {
            if instance.path == path {
                let entity = *entity;
                return self.load_screen_gui(entity, path);
            }
        }
        
        Err(RuntimeUIError::NotFound(path.to_string_lossy().to_string()))
    }
}

// ============================================================================
// UI Instance Types
// ============================================================================

/// A loaded ScreenGui instance (full-screen overlay)
#[derive(Debug, Clone)]
pub struct ScreenGuiInstance {
    /// Path to the _instance.toml or .screengui.toml
    pub path: PathBuf,
    
    /// Child element handles (name → element data)
    pub elements: HashMap<String, GuiElement>,
    
    /// Last modification time for hot-reload
    pub mtime: SystemTime,
    
    /// Whether the GUI is visible
    pub enabled: bool,
    
    /// Z-order for multiple ScreenGuis
    pub display_order: i32,
    
    /// Whether to ignore the top bar inset
    pub ignore_gui_inset: bool,
}

/// A loaded BillboardGui instance (world-space, camera-facing)
#[derive(Debug, Clone)]
pub struct BillboardGuiInstance {
    pub path: PathBuf,
    pub elements: HashMap<String, GuiElement>,
    pub mtime: SystemTime,
    pub enabled: bool,
    pub adornee: Option<Entity>,
    pub size: [f32; 2],
    pub studs_offset: [f32; 3],
    pub always_on_top: bool,
}

/// A loaded SurfaceGui instance (world-space, surface-aligned)
#[derive(Debug, Clone)]
pub struct SurfaceGuiInstance {
    pub path: PathBuf,
    pub elements: HashMap<String, GuiElement>,
    pub mtime: SystemTime,
    pub enabled: bool,
    pub adornee: Option<Entity>,
    pub face: SurfaceFace,
    pub size_per_stud: f32,
}

/// Surface face for SurfaceGui
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SurfaceFace {
    #[default]
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
}

// ============================================================================
// GUI Element Types
// ============================================================================

/// A single GUI element (Frame, TextLabel, etc.)
#[derive(Debug, Clone)]
pub struct GuiElement {
    /// Element name
    pub name: String,
    
    /// Class type
    pub class_name: GuiElementType,
    
    /// Position relative to parent [x, y] in pixels or scale
    pub position: [f32; 2],
    
    /// Size [width, height] in pixels or scale
    pub size: [f32; 2],
    
    /// Anchor point [x, y] (0-1, where 0.5 = center)
    pub anchor_point: [f32; 2],
    
    /// Z-index for layering
    pub z_index: i32,
    
    /// Visibility
    pub visible: bool,
    
    /// Background color [r, g, b, a]
    pub background_color: [f32; 4],
    
    /// Background transparency (0 = opaque, 1 = invisible)
    pub background_transparency: f32,
    
    /// Border size in pixels
    pub border_size: f32,
    
    /// Border color [r, g, b, a]
    pub border_color: [f32; 4],
    
    /// Corner radius (single value or [tl, tr, br, bl])
    pub corner_radius: f32,
    
    /// Text content (for TextLabel, TextButton, TextBox)
    pub text: Option<String>,
    
    /// Text color [r, g, b, a]
    pub text_color: Option<[f32; 4]>,
    
    /// Font size in pixels
    pub font_size: Option<f32>,
    
    /// Font family name
    pub font_family: Option<String>,
    
    /// Image asset path (for ImageLabel, ImageButton)
    pub image: Option<String>,
    
    /// Image tint color [r, g, b, a]
    pub image_color: Option<[f32; 4]>,
    
    /// Child elements
    pub children: Vec<GuiElement>,
}

impl Default for GuiElement {
    fn default() -> Self {
        Self {
            name: String::new(),
            class_name: GuiElementType::Frame,
            position: [0.0, 0.0],
            size: [100.0, 100.0],
            anchor_point: [0.0, 0.0],
            z_index: 0,
            visible: true,
            background_color: [1.0, 1.0, 1.0, 1.0],
            background_transparency: 0.0,
            border_size: 0.0,
            border_color: [0.0, 0.0, 0.0, 1.0],
            corner_radius: 0.0,
            text: None,
            text_color: None,
            font_size: None,
            font_family: None,
            image: None,
            image_color: None,
            children: Vec::new(),
        }
    }
}

/// GUI element types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GuiElementType {
    #[default]
    Frame,
    ScrollingFrame,
    TextLabel,
    TextButton,
    TextBox,
    ImageLabel,
    ImageButton,
    ViewportFrame,
    VideoFrame,
    DocumentFrame,
    WebFrame,
}

impl GuiElementType {
    pub fn from_class_name(class_name: &str) -> Option<Self> {
        match class_name {
            "Frame" => Some(Self::Frame),
            "ScrollingFrame" => Some(Self::ScrollingFrame),
            "TextLabel" => Some(Self::TextLabel),
            "TextButton" => Some(Self::TextButton),
            "TextBox" => Some(Self::TextBox),
            "ImageLabel" => Some(Self::ImageLabel),
            "ImageButton" => Some(Self::ImageButton),
            "ViewportFrame" => Some(Self::ViewportFrame),
            "VideoFrame" => Some(Self::VideoFrame),
            "DocumentFrame" => Some(Self::DocumentFrame),
            "WebFrame" => Some(Self::WebFrame),
            _ => None,
        }
    }
}

// ============================================================================
// Events
// ============================================================================

/// UI events that can be handled by Soul scripts
#[derive(Message, Debug, Clone)]
pub enum UIEvent {
    /// Button was clicked
    ButtonClicked {
        gui_entity: Entity,
        element_path: String,
    },
    
    /// Text content changed (TextBox)
    TextChanged {
        gui_entity: Entity,
        element_path: String,
        new_text: String,
    },
    
    /// Mouse entered element
    MouseEnter {
        gui_entity: Entity,
        element_path: String,
    },
    
    /// Mouse left element
    MouseLeave {
        gui_entity: Entity,
        element_path: String,
    },
    
    /// Element gained focus
    FocusGained {
        gui_entity: Entity,
        element_path: String,
    },
    
    /// Element lost focus
    FocusLost {
        gui_entity: Entity,
        element_path: String,
    },
}

/// Event to trigger UI file reload
#[derive(Message, Debug, Clone)]
pub struct RuntimeUIReloadEvent {
    pub path: PathBuf,
}

// ============================================================================
// Errors
// ============================================================================

/// Runtime UI errors
#[derive(Debug, Clone)]
pub enum RuntimeUIError {
    IoError(String),
    ParseError(String),
    NotFound(String),
    InvalidElement(String),
}

impl std::fmt::Display for RuntimeUIError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(msg) => write!(f, "IO error: {}", msg),
            Self::ParseError(msg) => write!(f, "Parse error: {}", msg),
            Self::NotFound(msg) => write!(f, "Not found: {}", msg),
            Self::InvalidElement(msg) => write!(f, "Invalid element: {}", msg),
        }
    }
}

impl std::error::Error for RuntimeUIError {}

// ============================================================================
// Systems
// ============================================================================

/// Watch UI files for changes and trigger hot-reload
fn watch_ui_files(
    mut ui_manager: ResMut<RuntimeUIManager>,
    mut reload_events: MessageWriter<RuntimeUIReloadEvent>,
) {
    let updated_paths = ui_manager.check_for_updates();
    
    for path in updated_paths {
        if let Err(e) = ui_manager.reload(&path) {
            warn!("Failed to reload UI file {:?}: {}", path, e);
        } else {
            info!("Hot-reloaded UI file: {:?}", path);
            reload_events.write(RuntimeUIReloadEvent { path });
        }
    }
}

/// Process UI events and route to Soul scripts
fn process_ui_events(
    mut ui_events: MessageReader<UIEvent>,
    // TODO: Add Soul VM resource for script callbacks
) {
    for event in ui_events.read() {
        match event {
            UIEvent::ButtonClicked { gui_entity, element_path } => {
                debug!("Button clicked: {:?} -> {}", gui_entity, element_path);
                // TODO: Call Soul script on_click handler
            }
            UIEvent::TextChanged { gui_entity, element_path, new_text } => {
                debug!("Text changed: {:?} -> {} = {}", gui_entity, element_path, new_text);
                // TODO: Call Soul script on_text_changed handler
            }
            _ => {}
        }
    }
}

// ============================================================================
// TOML Parsing Helpers
// ============================================================================

/// Parse a GUI element from TOML
pub fn parse_gui_element(name: &str, data: &toml::Value) -> Result<GuiElement, RuntimeUIError> {
    let class_name = data.get("instance")
        .and_then(|i| i.get("class_name"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| RuntimeUIError::InvalidElement("Missing class_name".to_string()))?;
    
    let element_type = GuiElementType::from_class_name(class_name)
        .ok_or_else(|| RuntimeUIError::InvalidElement(format!("Unknown GUI class: {}", class_name)))?;
    
    let gui = data.get("gui").cloned().unwrap_or(toml::Value::Table(toml::map::Map::new()));
    
    let mut element = GuiElement {
        name: name.to_string(),
        class_name: element_type,
        ..Default::default()
    };
    
    // Parse position
    if let Some(pos) = gui.get("position").and_then(|v| v.as_array()) {
        if pos.len() >= 2 {
            element.position = [
                pos[0].as_float().unwrap_or(0.0) as f32,
                pos[1].as_float().unwrap_or(0.0) as f32,
            ];
        }
    }
    
    // Parse size
    if let Some(size) = gui.get("size").and_then(|v| v.as_array()) {
        if size.len() >= 2 {
            element.size = [
                size[0].as_float().unwrap_or(100.0) as f32,
                size[1].as_float().unwrap_or(100.0) as f32,
            ];
        }
    }
    
    // Parse anchor point
    if let Some(anchor) = gui.get("anchor_point").and_then(|v| v.as_array()) {
        if anchor.len() >= 2 {
            element.anchor_point = [
                anchor[0].as_float().unwrap_or(0.0) as f32,
                anchor[1].as_float().unwrap_or(0.0) as f32,
            ];
        }
    }
    
    // Parse z_index
    element.z_index = gui.get("z_index")
        .and_then(|v| v.as_integer())
        .unwrap_or(0) as i32;
    
    // Parse visible
    element.visible = gui.get("visible")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    
    // Parse background_color
    if let Some(color) = gui.get("background_color").and_then(|v| v.as_array()) {
        if color.len() >= 4 {
            element.background_color = [
                color[0].as_float().unwrap_or(1.0) as f32,
                color[1].as_float().unwrap_or(1.0) as f32,
                color[2].as_float().unwrap_or(1.0) as f32,
                color[3].as_float().unwrap_or(1.0) as f32,
            ];
        }
    }
    
    // Parse background_transparency
    element.background_transparency = gui.get("background_transparency")
        .and_then(|v| v.as_float())
        .unwrap_or(0.0) as f32;
    
    // Parse border_size
    element.border_size = gui.get("border_size")
        .and_then(|v| v.as_float())
        .unwrap_or(0.0) as f32;
    
    // Parse border_color
    if let Some(color) = gui.get("border_color").and_then(|v| v.as_array()) {
        if color.len() >= 4 {
            element.border_color = [
                color[0].as_float().unwrap_or(0.0) as f32,
                color[1].as_float().unwrap_or(0.0) as f32,
                color[2].as_float().unwrap_or(0.0) as f32,
                color[3].as_float().unwrap_or(1.0) as f32,
            ];
        }
    }
    
    // Parse corner_radius
    element.corner_radius = gui.get("corner_radius")
        .and_then(|v| v.as_float())
        .unwrap_or(0.0) as f32;
    
    // Parse text properties
    if let Some(text_section) = data.get("text") {
        element.text = text_section.get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        if let Some(color) = text_section.get("text_color").and_then(|v| v.as_array()) {
            if color.len() >= 4 {
                element.text_color = Some([
                    color[0].as_float().unwrap_or(0.0) as f32,
                    color[1].as_float().unwrap_or(0.0) as f32,
                    color[2].as_float().unwrap_or(0.0) as f32,
                    color[3].as_float().unwrap_or(1.0) as f32,
                ]);
            }
        }
        
        element.font_size = text_section.get("font_size")
            .and_then(|v| v.as_float())
            .map(|f| f as f32);
        
        element.font_family = text_section.get("font_family")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    
    // Parse image properties
    if let Some(image_section) = data.get("image") {
        element.image = image_section.get("image")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        if let Some(color) = image_section.get("image_color").and_then(|v| v.as_array()) {
            if color.len() >= 4 {
                element.image_color = Some([
                    color[0].as_float().unwrap_or(1.0) as f32,
                    color[1].as_float().unwrap_or(1.0) as f32,
                    color[2].as_float().unwrap_or(1.0) as f32,
                    color[3].as_float().unwrap_or(1.0) as f32,
                ]);
            }
        }
    }
    
    Ok(element)
}

/// Load a GUI tree from a folder (ScreenGui, Frame, etc.)
pub fn load_gui_tree(folder_path: &Path) -> Result<GuiElement, RuntimeUIError> {
    let instance_path = folder_path.join("_instance.toml");
    
    if !instance_path.exists() {
        return Err(RuntimeUIError::NotFound(
            format!("No _instance.toml in {:?}", folder_path)
        ));
    }
    
    let toml_content = std::fs::read_to_string(&instance_path)
        .map_err(|e| RuntimeUIError::IoError(e.to_string()))?;
    
    let data: toml::Value = toml::from_str(&toml_content)
        .map_err(|e| RuntimeUIError::ParseError(e.to_string()))?;
    
    let name = folder_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown");
    
    let mut element = parse_gui_element(name, &data)?;
    
    // Load child elements from sibling files
    if let Ok(entries) = std::fs::read_dir(folder_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            
            // Skip _instance.toml
            if path.file_name() == Some("_instance.toml".as_ref()) {
                continue;
            }
            
            // Handle subdirectories (nested containers)
            if path.is_dir() {
                if let Ok(child) = load_gui_tree(&path) {
                    element.children.push(child);
                }
                continue;
            }
            
            // Handle .toml files (leaf elements)
            if path.extension() == Some("toml".as_ref()) {
                let child_content = std::fs::read_to_string(&path)
                    .map_err(|e| RuntimeUIError::IoError(e.to_string()))?;
                
                let child_data: toml::Value = toml::from_str(&child_content)
                    .map_err(|e| RuntimeUIError::ParseError(e.to_string()))?;
                
                let child_name = path.file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown");
                
                // Remove extension suffix (e.g., "Title.textlabel" -> "Title")
                let child_name = child_name.split('.').next().unwrap_or(child_name);
                
                if let Ok(child) = parse_gui_element(child_name, &child_data) {
                    element.children.push(child);
                }
            }
        }
    }
    
    // Sort children by z_index
    element.children.sort_by_key(|c| c.z_index);
    
    Ok(element)
}

// ============================================================================
// Bevy UI Spawner
// ============================================================================

/// Spawn Bevy UI Node hierarchy for every ScreenGui entity under StarterGui
/// that hasn't been spawned yet. Runs once per entity (guarded by BevyGuiSpawned).
///
/// Architecture:
///   ScreenGui entity (loaded by file_loader, class=ScreenGui/Frame)
///     └─ child TOML entities (TextLabel, TextButton, Frame, etc.)
///
/// Each entity gets a Bevy Node + BackgroundColor + optional Text spawned on it.
/// The Slint overlay sits above this layer (camera order), so runtime game UI
/// appears beneath the editor chrome.
fn spawn_bevy_gui_from_loaded_entities(
    mut commands: Commands,
    // All loaded GUI entities that haven't been given Bevy UI nodes yet
    unspawned: Query<
        (Entity, &crate::space::file_loader::LoadedFromFile, &eustress_common::classes::Instance),
        Without<BevyGuiSpawned>,
    >,
    // StarterGui service component to read ShowDevelopmentUI
    service_components: Query<&crate::space::service_loader::ServiceComponent>,
) {
    use eustress_common::classes::ClassName;
    use crate::space::service_loader::PropertyValue;

    // Read ShowDevelopmentUI from the StarterGui service (defaults to false if not set)
    let show_development_ui = service_components
        .iter()
        .find(|sc| sc.class_name == "StarterGui")
        .and_then(|sc| sc.properties.get("ShowDevelopmentUI"))
        .map(|v| matches!(v, PropertyValue::Bool(true)))
        .unwrap_or(false);

    for (entity, loaded, instance) in &unspawned {
        // Stamp BevyGuiSpawned on ALL GUI-related class names so the query
        // never re-processes them and causes flicker.
        // ScreenGui/BillboardGui/SurfaceGui are container roots — stamp them
        // too even though they don't get Bevy Node components.
        let is_container = matches!(
            instance.class_name,
            ClassName::ScreenGui | ClassName::BillboardGui | ClassName::SurfaceGui
        );
        let is_gui = matches!(
            instance.class_name,
            ClassName::Frame
            | ClassName::TextLabel
            | ClassName::TextButton
            | ClassName::TextBox
            | ClassName::ImageLabel
            | ClassName::ImageButton
            | ClassName::ScrollingFrame
            | ClassName::ViewportFrame
        );

        if is_container {
            // Stamp the marker so the system doesn't re-query every frame
            commands.entity(entity).insert(BevyGuiSpawned);
            continue;
        }

        if !is_gui {
            continue;
        }

        // Read the TOML to get layout/style data
        let Ok(toml_str) = std::fs::read_to_string(&loaded.path) else { continue };
        let Ok(data) = toml::from_str::<toml::Value>(&toml_str) else { continue };

        let gui = data.get("gui");

        // Position (pixels from top-left)
        let pos_x = gui.and_then(|g| g.get("position"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_float())
            .unwrap_or(0.0) as f32;
        let pos_y = gui.and_then(|g| g.get("position"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(1))
            .and_then(|v| v.as_float())
            .unwrap_or(0.0) as f32;

        // Size
        let width = gui.and_then(|g| g.get("size"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_float())
            .unwrap_or(100.0) as f32;
        let height = gui.and_then(|g| g.get("size"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(1))
            .and_then(|v| v.as_float())
            .unwrap_or(30.0) as f32;

        // Background color [r, g, b, a]
        let bg_r = gui.and_then(|g| g.get("background_color")).and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
        let bg_g = gui.and_then(|g| g.get("background_color")).and_then(|v| v.as_array()).and_then(|a| a.get(1)).and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
        let bg_b = gui.and_then(|g| g.get("background_color")).and_then(|v| v.as_array()).and_then(|a| a.get(2)).and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
        let bg_a = gui.and_then(|g| g.get("background_color")).and_then(|v| v.as_array()).and_then(|a| a.get(3)).and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
        let bg = bevy::prelude::Color::srgba(bg_r, bg_g, bg_b, bg_a);

        let visible = gui.and_then(|g| g.get("visible"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Elements under StarterGui are only shown when ShowDevelopmentUI is true
        let is_starter_gui = loaded.service == "StarterGui";
        let visibility = if visible && (!is_starter_gui || show_development_ui) {
            bevy::prelude::Visibility::Visible
        } else {
            bevy::prelude::Visibility::Hidden
        };

        // Build the Bevy Node with absolute positioning
        let node = bevy::prelude::Node {
            position_type: bevy::prelude::PositionType::Absolute,
            left: bevy::prelude::Val::Px(pos_x),
            top: bevy::prelude::Val::Px(pos_y),
            width: bevy::prelude::Val::Px(width),
            height: bevy::prelude::Val::Px(height),
            ..Default::default()
        };

        let bg_color = bevy::prelude::BackgroundColor(bg);

        commands.entity(entity).insert((node, bg_color, visibility, BevyGuiSpawned));

        // For text-bearing elements, insert Text components
        if matches!(instance.class_name, ClassName::TextLabel | ClassName::TextButton | ClassName::TextBox) {
            let text_section = data.get("text");
            let text_str = text_section
                .and_then(|t| t.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let font_size = text_section
                .and_then(|t| t.get("font_size"))
                .and_then(|v| v.as_float())
                .unwrap_or(14.0) as f32;
            let tc_r = text_section.and_then(|t| t.get("text_color")).and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
            let tc_g = text_section.and_then(|t| t.get("text_color")).and_then(|v| v.as_array()).and_then(|a| a.get(1)).and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
            let tc_b = text_section.and_then(|t| t.get("text_color")).and_then(|v| v.as_array()).and_then(|a| a.get(2)).and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
            let tc_a = text_section.and_then(|t| t.get("text_color")).and_then(|v| v.as_array()).and_then(|a| a.get(3)).and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
            let text_color = bevy::prelude::Color::srgba(tc_r, tc_g, tc_b, tc_a);

            commands.entity(entity).insert((
                bevy::prelude::Text::new(text_str),
                bevy::prelude::TextColor(text_color),
                bevy::prelude::TextFont {
                    font_size,
                    ..Default::default()
                },
            ));
        }
    }
}

/// Watches ShowDevelopmentUI on StarterGui and updates Visibility of all
/// already-spawned StarterGui child entities so toggling the property
/// takes effect immediately without requiring a restart.
fn sync_development_ui_visibility(
    mut last_state: ResMut<ShowDevelopmentUIState>,
    service_components: Query<&crate::space::service_loader::ServiceComponent>,
    spawned: Query<
        (Entity, &crate::space::file_loader::LoadedFromFile, &mut Visibility),
        With<BevyGuiSpawned>,
    >,
    mut commands: Commands,
) {
    use crate::space::service_loader::PropertyValue;

    let show = service_components
        .iter()
        .find(|sc| sc.class_name == "StarterGui")
        .and_then(|sc| sc.properties.get("ShowDevelopmentUI"))
        .map(|v| matches!(v, PropertyValue::Bool(true)))
        .unwrap_or(false);

    // Only update when value actually changed
    if last_state.last_value == Some(show) {
        return;
    }
    last_state.last_value = Some(show);

    let new_vis = if show {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };

    for (entity, loaded, _vis) in &spawned {
        if loaded.service == "StarterGui" {
            commands.entity(entity).insert(new_vis);
        }
    }
}

// ============================================================================
// Component Markers
// ============================================================================

/// Marker component for entities that have a ScreenGui
#[derive(Component, Debug, Clone)]
pub struct HasScreenGui;

/// Marker component for entities that have a BillboardGui
#[derive(Component, Debug, Clone)]
pub struct HasBillboardGui {
    pub adornee: Option<Entity>,
}

/// Marker component for entities that have a SurfaceGui
#[derive(Component, Debug, Clone)]
pub struct HasSurfaceGui {
    pub adornee: Option<Entity>,
    pub face: SurfaceFace,
}
