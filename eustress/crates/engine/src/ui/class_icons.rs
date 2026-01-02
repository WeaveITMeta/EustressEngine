// Class Icons - Visual indicators for each Roblox class
// Phase 2, Week 2: Explorer Enhancement
// Updated: Now uses vector icons instead of emojis

use bevy_egui::egui;
use crate::classes::ClassName;
use super::icons;

/// Get color for a class based on its category
pub fn class_color(class_name: ClassName) -> egui::Color32 {
    match class_name {
        // Core - Blue
        ClassName::Part | ClassName::MeshPart | ClassName::BasePart | ClassName::PVInstance => {
            egui::Color32::from_rgb(100, 150, 255)
        }
        
        // Container - Orange
        ClassName::Model | ClassName::Folder => {
            egui::Color32::from_rgb(255, 200, 100)
        }
        
        // Character - Green
        ClassName::Humanoid => {
            egui::Color32::from_rgb(100, 255, 150)
        }
        
        // Rendering - Cyan
        ClassName::Camera => {
            egui::Color32::from_rgb(100, 200, 255)
        }
        
        // Lighting - Yellow
        ClassName::PointLight | ClassName::SpotLight | ClassName::SurfaceLight | ClassName::DirectionalLight => {
            egui::Color32::from_rgb(255, 255, 100)
        }
        
        // Constraints - Steel
        ClassName::Attachment | ClassName::WeldConstraint | ClassName::Motor6D => {
            egui::Color32::from_rgb(150, 150, 180)
        }
        
        // Meshes - Purple
        ClassName::SpecialMesh | ClassName::UnionOperation => {
            egui::Color32::from_rgb(200, 150, 255)
        }
        
        // Visuals - Pink
        ClassName::Decal => {
            egui::Color32::from_rgb(255, 150, 200)
        }
        
        // Animation - Salmon
        ClassName::Animator | ClassName::KeyframeSequence => {
            egui::Color32::from_rgb(255, 180, 150)
        }
        
        // Effects - Magenta
        ClassName::ParticleEmitter | ClassName::Beam => {
            egui::Color32::from_rgb(255, 100, 255)
        }
        
        // Audio - Lime
        ClassName::Sound => {
            egui::Color32::from_rgb(150, 255, 100)
        }
        
        // Environment - Teal
        ClassName::Terrain | ClassName::Sky => {
            egui::Color32::from_rgb(100, 200, 150)
        }
        
        // GUI - Cyan
        ClassName::BillboardGui | ClassName::SurfaceGui | ClassName::ScreenGui | 
        ClassName::TextLabel | ClassName::Frame | ClassName::ImageLabel |
        ClassName::TextButton | ClassName::ImageButton => {
            egui::Color32::from_rgb(0, 200, 200)
        }
        
        // Soul Scripting - Purple
        ClassName::SoulScript => {
            egui::Color32::from_rgb(180, 100, 255)
        }
        
        // Services/Environment - Gold
        ClassName::Lighting | ClassName::Atmosphere | ClassName::SpawnLocation | ClassName::Workspace => {
            egui::Color32::from_rgb(255, 215, 0)
        }
        
        // Celestial - Sky Blue/Silver
        ClassName::Clouds => {
            egui::Color32::from_rgb(200, 220, 255)
        }
        ClassName::Sun => {
            egui::Color32::from_rgb(255, 230, 100)
        }
        ClassName::Moon => {
            egui::Color32::from_rgb(200, 200, 220)
        }
        
        // Seats - Brown/Vehicle Blue
        ClassName::Seat => {
            egui::Color32::from_rgb(139, 90, 43) // Brown
        }
        ClassName::VehicleSeat => {
            egui::Color32::from_rgb(70, 130, 180) // Steel blue
        }
        
        // Media Assets
        ClassName::Document => {
            egui::Color32::from_rgb(180, 80, 80) // Red-brown
        }
        ClassName::ImageAsset => {
            egui::Color32::from_rgb(100, 180, 100) // Green
        }
        ClassName::VideoAsset => {
            egui::Color32::from_rgb(180, 100, 180) // Purple
        }
        
        // ScrollingFrame
        ClassName::ScrollingFrame => {
            egui::Color32::from_rgb(100, 149, 237) // Cornflower blue
        }
        
        // VideoFrame
        ClassName::VideoFrame => {
            egui::Color32::from_rgb(220, 100, 180) // Pink-purple
        }
        
        // DocumentFrame
        ClassName::DocumentFrame => {
            egui::Color32::from_rgb(200, 120, 80) // Orange-brown
        }
        
        // WebFrame
        ClassName::WebFrame => {
            egui::Color32::from_rgb(70, 130, 180) // Steel blue
        }
        
        // TextBox
        ClassName::TextBox => {
            egui::Color32::from_rgb(100, 180, 100) // Green for input
        }
        
        // ViewportFrame
        ClassName::ViewportFrame => {
            egui::Color32::from_rgb(150, 100, 200) // Purple for 3D viewport
        }
        
        // Team - Red (team color)
        ClassName::Team => {
            egui::Color32::from_rgb(255, 100, 100) // Red for teams
        }
        
        // Orbital - Deep space blue/purple
        ClassName::SolarSystem => {
            egui::Color32::from_rgb(50, 50, 150) // Deep blue
        }
        ClassName::CelestialBody => {
            egui::Color32::from_rgb(100, 80, 180) // Purple-blue
        }
        ClassName::RegionChunk => {
            egui::Color32::from_rgb(80, 150, 100) // Earth green
        }
        
        // Default - Gray
        ClassName::Instance => {
            egui::Color32::GRAY
        }
    }
}

/// Get category name for a class
pub fn class_category(class_name: ClassName) -> &'static str {
    match class_name {
        ClassName::Instance | ClassName::PVInstance | ClassName::BasePart | 
        ClassName::Part | ClassName::MeshPart => "Core",
        
        ClassName::Model | ClassName::Folder => "Container",
        
        ClassName::Humanoid => "Character",
        
        ClassName::Camera => "Rendering",
        
        ClassName::PointLight | ClassName::SpotLight | ClassName::SurfaceLight | ClassName::DirectionalLight => "Lighting",
        
        ClassName::Attachment | ClassName::WeldConstraint | ClassName::Motor6D => "Constraints",
        
        ClassName::SpecialMesh | ClassName::UnionOperation => "Meshes",
        
        ClassName::Decal => "Visuals",
        
        ClassName::Animator | ClassName::KeyframeSequence => "Animation",
        
        ClassName::ParticleEmitter | ClassName::Beam => "Effects",
        
        ClassName::Sound => "Audio",
        
        ClassName::Terrain | ClassName::Sky => "Environment",
        
        ClassName::BillboardGui | ClassName::SurfaceGui | ClassName::ScreenGui |
        ClassName::TextLabel | ClassName::Frame | ClassName::ImageLabel |
        ClassName::TextButton | ClassName::ImageButton => "GUI",
        
        ClassName::SoulScript => "Soul",
        
        ClassName::Lighting | ClassName::Atmosphere | ClassName::SpawnLocation | ClassName::Workspace => "Services",
        
        ClassName::Clouds | ClassName::Sun | ClassName::Moon => "Celestial",
        
        ClassName::Seat | ClassName::VehicleSeat => "Seats",
        
        ClassName::Document | ClassName::ImageAsset | ClassName::VideoAsset => "Media",
        
        ClassName::ScrollingFrame | ClassName::VideoFrame | ClassName::DocumentFrame | ClassName::WebFrame |
        ClassName::TextBox | ClassName::ViewportFrame => "GUI",
        
        ClassName::Team => "Teams",
        
        ClassName::SolarSystem | ClassName::CelestialBody | ClassName::RegionChunk => "Orbital",
    }
}

/// Get a styled label for a class (icon + name + color)
pub fn class_label(ui: &mut egui::Ui, class_name: ClassName, name: &str) {
    let color = class_color(class_name);
    
    ui.horizontal(|ui| {
        // Vector icon
        let (rect, _response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        icons::draw_class_icon(ui.painter(), rect.min, class_name, 16.0);
        
        // Name with color
        ui.label(
            egui::RichText::new(name)
                .color(color)
        );
        
        // Class type in parentheses (dimmed)
        ui.label(
            egui::RichText::new(format!("({})", class_name.as_str()))
                .small()
                .color(egui::Color32::GRAY)
        );
    });
}

/// Get a compact class label (icon + name, no class type)
pub fn class_label_compact(ui: &mut egui::Ui, class_name: ClassName, name: &str) -> egui::Response {
    let color = class_color(class_name);
    
    ui.horizontal(|ui| {
        // Vector icon
        let (rect, _response) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        icons::draw_class_icon(ui.painter(), rect.min, class_name, 14.0);
        
        ui.label(
            egui::RichText::new(name)
                .color(color)
        );
    }).response
}

/// Get class filter options for search/filter UI
pub fn class_filter_options() -> Vec<(&'static str, Vec<ClassName>)> {
    vec![
        ("All", vec![]),  // Empty = show all
        ("Core", vec![
            ClassName::Instance, ClassName::PVInstance, ClassName::BasePart,
            ClassName::Part, ClassName::MeshPart,
        ]),
        ("Container", vec![ClassName::Model, ClassName::Folder]),
        ("Character", vec![ClassName::Humanoid]),
        ("Rendering", vec![ClassName::Camera]),
        ("Lighting", vec![
            ClassName::PointLight, ClassName::SpotLight, ClassName::SurfaceLight, ClassName::DirectionalLight,
        ]),
        ("Constraints", vec![
            ClassName::Attachment, ClassName::WeldConstraint, ClassName::Motor6D,
        ]),
        ("Meshes", vec![ClassName::SpecialMesh, ClassName::UnionOperation]),
        ("Visuals", vec![ClassName::Decal]),
        ("Animation", vec![ClassName::Animator, ClassName::KeyframeSequence]),
        ("Effects", vec![ClassName::ParticleEmitter, ClassName::Beam]),
        ("Audio", vec![ClassName::Sound]),
        ("Environment", vec![ClassName::Terrain, ClassName::Sky]),
    ]
}

/// Check if a class matches a filter
pub fn matches_filter(class_name: ClassName, filter_classes: &[ClassName]) -> bool {
    if filter_classes.is_empty() {
        true  // Empty filter = show all
    } else {
        filter_classes.contains(&class_name)
    }
}

/// Get a simple text icon for a class (for use in search results)
pub fn class_icon(class_name: ClassName) -> &'static str {
    match class_name {
        ClassName::Part | ClassName::MeshPart | ClassName::BasePart => "▣",
        ClassName::Model => "📦",
        ClassName::Folder => "📁",
        ClassName::Humanoid => "🧍",
        ClassName::Camera => "📷",
        ClassName::PointLight | ClassName::SpotLight | ClassName::SurfaceLight | ClassName::DirectionalLight => "💡",
        ClassName::Attachment | ClassName::WeldConstraint | ClassName::Motor6D => "🔗",
        ClassName::SpecialMesh | ClassName::UnionOperation => "🔷",
        ClassName::Decal => "🖼",
        ClassName::Animator | ClassName::KeyframeSequence => "🎬",
        ClassName::ParticleEmitter | ClassName::Beam => "✨",
        ClassName::Sound => "🔊",
        ClassName::Terrain => "🏔",
        ClassName::Sky | ClassName::Atmosphere | ClassName::Clouds => "☁",
        ClassName::Sun => "☀",
        ClassName::Moon => "🌙",
        ClassName::BillboardGui | ClassName::SurfaceGui | ClassName::ScreenGui => "🖥",
        ClassName::TextLabel | ClassName::TextButton | ClassName::TextBox => "T",
        ClassName::Frame | ClassName::ScrollingFrame => "▢",
        ClassName::ImageLabel | ClassName::ImageButton => "🖼",
        ClassName::VideoFrame | ClassName::VideoAsset => "🎥",
        ClassName::DocumentFrame | ClassName::Document => "📄",
        ClassName::WebFrame => "🌐",
        ClassName::ViewportFrame => "🎮",
        ClassName::SoulScript => "📜",
        ClassName::Lighting | ClassName::Workspace => "⚙",
        ClassName::SpawnLocation => "🚩",
        ClassName::Seat => "🪑",
        ClassName::VehicleSeat => "🚗",
        ClassName::Team => "👥",
        ClassName::ImageAsset => "🖼",
        ClassName::SolarSystem | ClassName::CelestialBody | ClassName::RegionChunk => "🌍",
        _ => "•",
    }
}

/// Get tooltip text for a class
pub fn class_tooltip(class_name: ClassName) -> String {
    let category = class_category(class_name);
    let description = match class_name {
        ClassName::Instance => "Base class for all objects",
        ClassName::PVInstance => "Base class for 3D objects with position/rotation",
        ClassName::BasePart => "Base class for physical parts with properties",
        ClassName::Part => "Basic geometric part (cube, sphere, cylinder, wedge)",
        ClassName::MeshPart => "Part with custom 3D mesh",
        ClassName::Model => "Container for grouping multiple parts",
        ClassName::Humanoid => "Character controller with health and movement",
        ClassName::Camera => "Viewport camera for rendering",
        ClassName::PointLight => "Omnidirectional light source",
        ClassName::SpotLight => "Directional cone light",
        ClassName::SurfaceLight => "Light emitting from a surface",
        ClassName::DirectionalLight => "Distant parallel light (sun)",
        ClassName::Attachment => "Mount point for effects and constraints",
        ClassName::WeldConstraint => "Fixed joint between two parts",
        ClassName::Motor6D => "Animated joint for character rigs",
        ClassName::SpecialMesh => "Mesh modifier for parts",
        ClassName::Decal => "2D texture on a surface",
        ClassName::UnionOperation => "CSG boolean operation result",
        ClassName::Animator => "Animation player",
        ClassName::KeyframeSequence => "Animation data",
        ClassName::ParticleEmitter => "Particle system (fire, smoke, etc.)",
        ClassName::Beam => "Curved line effect between attachments",
        ClassName::Sound => "3D spatial audio",
        ClassName::Terrain => "Voxel-based ground",
        ClassName::Sky => "Skybox and atmosphere",
        ClassName::Folder => "Organizational container",
        ClassName::BillboardGui => "3D UI that faces the camera",
        ClassName::SurfaceGui => "UI rendered on a part's surface",
        ClassName::ScreenGui => "2D screen-space UI (HUD, menus)",
        ClassName::TextLabel => "Text display element in UI",
        ClassName::Frame => "Container element for UI layout",
        ClassName::ImageLabel => "Image display element in UI",
        ClassName::TextButton => "Clickable text button",
        ClassName::ImageButton => "Clickable image button",
        ClassName::SoulScript => "Markdown script compiled to Rust via Claude API",
        ClassName::Lighting => "Scene lighting service (sun, ambient, fog)",
        ClassName::Atmosphere => "Atmospheric effects (haze, fog, scattering)",
        ClassName::SpawnLocation => "Player spawn point",
        ClassName::Workspace => "Root container for 3D objects",
        ClassName::Clouds => "Volumetric cloud system with wind movement",
        ClassName::Sun => "Sun celestial body with day/night cycle",
        ClassName::Moon => "Moon with phases and night lighting",
        ClassName::Seat => "Seat that characters can sit in (auto-sit on touch)",
        ClassName::VehicleSeat => "Vehicle seat with throttle/steer input for driving",
        ClassName::Document => "Document asset (PDF, DOCX, PPTX, XLSX, Google Docs)",
        ClassName::ImageAsset => "Image asset (PNG, JPG, GIF, WebP, SVG)",
        ClassName::VideoAsset => "Video asset (MP4, WebM, streaming)",
        ClassName::ScrollingFrame => "Scrollable container with scrollbars for UI content",
        ClassName::VideoFrame => "UI element to display video content (links to VideoAsset)",
        ClassName::DocumentFrame => "UI element to display documents (links to Document)",
        ClassName::WebFrame => "UI element to display embedded web content (like iframe)",
        ClassName::TextBox => "Text input field for user text entry",
        ClassName::ViewportFrame => "3D viewport embedded within UI",
        ClassName::Team => "Team for grouping players (child of Teams service)",
        ClassName::SolarSystem => "Container for orbital hierarchies (planets, moons, stars)",
        ClassName::CelestialBody => "Orbital object with n-body gravity (planet, moon, star)",
        ClassName::RegionChunk => "Geospatial fragment with floating origin for Earth-scale scenes",
    };
    
    format!("{} | Category: {}\n{}", class_name.as_str(), category, description)
}
