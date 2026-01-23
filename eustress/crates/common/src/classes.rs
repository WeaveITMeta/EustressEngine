//! # Eustress Class System
//! 
//! Instance hierarchy mapped to Bevy ECS components.
//! This is the canonical source - engine and client import from here.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// Table of Contents
// ============================================================================
// 1. Core Enums (PartType, Material, ClassName)
// 2. Instance (Base Class)
// 3. PVInstance (Pivot/Position)
// 4. BasePart (Core 3D Object)
// 5. Part (Primitive Shapes)
// 6. MeshPart (Custom Meshes)
// 7. Model (Container/Groups)
// 8. Humanoid (Character Controller)
// 9. Camera (Viewport Control)
// 10. Light Classes (PointLight, SpotLight, SurfaceLight)
// 11. Property Trait System
// 12. Bevy Component Mappings

// ============================================================================
// 1. Core Enums
// ============================================================================

/// Part shapes (procedural meshes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum PartType {
    Block,      // Cube (default)
    Ball,       // Sphere
    Cylinder,
    Wedge,
    CornerWedge,
    Cone,
}

impl Default for PartType {
    fn default() -> Self {
        PartType::Block
    }
}

impl PartType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "block" | "cube" | "part" => Some(PartType::Block),
            "ball" | "sphere" => Some(PartType::Ball),
            "cylinder" => Some(PartType::Cylinder),
            "wedge" => Some(PartType::Wedge),
            "cornerwedge" => Some(PartType::CornerWedge),
            "cone" => Some(PartType::Cone),
            _ => None,
        }
    }
    
    /// Parse part type from string, returning Block as default
    pub fn from_string(s: &str) -> Self {
        Self::from_str(s).unwrap_or(PartType::Block)
    }
}

/// Eustress Material enum (PBR rendering presets)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum Material {
    Plastic,
    SmoothPlastic,
    Wood,
    WoodPlanks,
    Metal,
    CorrodedMetal,
    DiamondPlate,
    Foil,
    Grass,
    Concrete,
    Brick,
    Granite,
    Marble,
    Slate,
    Sand,
    Fabric,
    Glass,
    Neon,
    Ice,
}

impl Default for Material {
    fn default() -> Self {
        Material::Plastic
    }
}

impl Material {
    /// Parse material from string name
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "plastic" => Material::Plastic,
            "smoothplastic" => Material::SmoothPlastic,
            "wood" => Material::Wood,
            "woodplanks" => Material::WoodPlanks,
            "metal" => Material::Metal,
            "corrodedmetal" => Material::CorrodedMetal,
            "diamondplate" => Material::DiamondPlate,
            "foil" => Material::Foil,
            "grass" => Material::Grass,
            "concrete" => Material::Concrete,
            "brick" => Material::Brick,
            "granite" => Material::Granite,
            "marble" => Material::Marble,
            "slate" => Material::Slate,
            "sand" => Material::Sand,
            "fabric" => Material::Fabric,
            "glass" => Material::Glass,
            "neon" => Material::Neon,
            "ice" => Material::Ice,
            _ => Material::Plastic, // Default fallback
        }
    }
    
    /// Get PBR parameters for this material (roughness, metallic, reflectance)
    pub fn pbr_params(&self) -> (f32, f32, f32) {
        match self {
            Material::Plastic => (0.6, 0.0, 0.4),           // More reflective for better lighting
            Material::SmoothPlastic => (0.2, 0.0, 0.5),     // Very smooth and reflective
            Material::Wood => (0.85, 0.0, 0.15),            // Natural matte wood
            Material::WoodPlanks => (0.75, 0.0, 0.2),       // Slightly smoother than raw wood
            Material::Metal => (0.3, 1.0, 0.6),             // Shiny metal with good reflectance
            Material::CorrodedMetal => (0.65, 0.7, 0.25),   // Rougher but still metallic
            Material::DiamondPlate => (0.35, 0.9, 0.65),    // Industrial metal texture
            Material::Foil => (0.08, 1.0, 0.92),            // Very reflective and metallic
            Material::Grass => (0.95, 0.0, 0.1),            // Very rough and matte
            Material::Concrete => (0.92, 0.0, 0.18),         // Very rough surface
            Material::Brick => (0.88, 0.0, 0.22),            // Rough with slight reflection
            Material::Granite => (0.55, 0.0, 0.35),          // Polished stone
            Material::Marble => (0.25, 0.0, 0.55),           // Very smooth and reflective
            Material::Slate => (0.65, 0.0, 0.28),            // Natural stone texture
            Material::Sand => (0.98, 0.0, 0.08),             // Extremely matte
            Material::Fabric => (0.92, 0.0, 0.12),           // Soft and matte
            Material::Glass => (0.02, 0.0, 0.95),            // Very smooth and reflective
            Material::Neon => (0.1, 0.0, 0.0),               // Smooth for glow effect
            Material::Ice => (0.08, 0.0, 0.88),              // Very reflective and smooth
        }
    }
}

/// Class names for type identification (Eustress ClassName)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum ClassName {
    Instance,
    PVInstance,
    BasePart,
    Part,
    MeshPart,
    Model,
    Humanoid,
    Camera,
    PointLight,
    SpotLight,
    SurfaceLight,
    DirectionalLight,
    Attachment,
    WeldConstraint,
    Motor6D,
    SpecialMesh,
    Decal,
    Folder,
    BillboardGui,
    SurfaceGui,
    ScreenGui,
    TextLabel,
    Frame,
    ScrollingFrame,
    ImageLabel,
    TextButton,
    ImageButton,
    Animator,
    KeyframeSequence,
    ParticleEmitter,
    Beam,
    Sound,
    Terrain,
    Sky,
    UnionOperation,
    // Soul Scripting - Single unified script class
    SoulScript,
    // Service/Environment Classes
    Lighting,       // Scene lighting container (like Roblox's Lighting service)
    Atmosphere,     // Atmospheric effects (fog, haze, etc.)
    SpawnLocation,  // Player spawn point
    Workspace,      // Root container for 3D objects
    // Celestial Classes (Lighting children)
    Clouds,         // Volumetric cloud system
    Sun,            // Sun celestial body with day/night cycle
    Moon,           // Moon with phases and night lighting
    // Seat Classes (extends BasePart)
    Seat,           // Basic seat - characters auto-sit on touch
    VehicleSeat,    // Vehicle seat with throttle/steer input
    // Team Classes
    Team,           // Team for grouping players (child of Teams service)
    // Media Asset Classes (Tabbed Viewer content)
    Document,       // PDF, DOCX, PPTX, XLSX, Google Docs/Sheets/Slides
    ImageAsset,     // PNG, JPG, GIF, WebP, SVG images
    VideoAsset,     // MP4, WebM, streaming video
    // Media UI Classes (embed media in UI)
    VideoFrame,     // UI element to display video (links to VideoAsset)
    DocumentFrame,  // UI element to display documents (links to Document)
    WebFrame,       // UI element to display embedded web content
    // Input UI Classes
    TextBox,        // Text input field
    // Advanced UI Classes
    ViewportFrame,  // 3D viewport within UI
    // Orbital Coordinate Grid Classes
    SolarSystem,      // Container for orbital hierarchies
    CelestialBody,    // Orbital object with n-body gravity
    RegionChunk,      // Geospatial fragment with floating origin
}

impl ClassName {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClassName::Instance => "Instance",
            ClassName::PVInstance => "PVInstance",
            ClassName::BasePart => "BasePart",
            ClassName::Part => "Part",
            ClassName::MeshPart => "MeshPart",
            ClassName::Model => "Model",
            ClassName::Humanoid => "Humanoid",
            ClassName::Camera => "Camera",
            ClassName::PointLight => "PointLight",
            ClassName::SpotLight => "SpotLight",
            ClassName::SurfaceLight => "SurfaceLight",
            ClassName::DirectionalLight => "DirectionalLight",
            ClassName::Attachment => "Attachment",
            ClassName::WeldConstraint => "WeldConstraint",
            ClassName::Motor6D => "Motor6D",
            ClassName::SpecialMesh => "SpecialMesh",
            ClassName::Decal => "Decal",
            ClassName::Folder => "Folder",
            ClassName::BillboardGui => "BillboardGui",
            ClassName::SurfaceGui => "SurfaceGui",
            ClassName::ScreenGui => "ScreenGui",
            ClassName::TextLabel => "TextLabel",
            ClassName::Frame => "Frame",
            ClassName::ScrollingFrame => "ScrollingFrame",
            ClassName::ImageLabel => "ImageLabel",
            ClassName::TextButton => "TextButton",
            ClassName::ImageButton => "ImageButton",
            ClassName::Animator => "Animator",
            ClassName::KeyframeSequence => "KeyframeSequence",
            ClassName::ParticleEmitter => "ParticleEmitter",
            ClassName::Beam => "Beam",
            ClassName::Sound => "Sound",
            ClassName::Terrain => "Terrain",
            ClassName::Sky => "Sky",
            ClassName::UnionOperation => "UnionOperation",
            ClassName::SoulScript => "SoulScript",
            ClassName::Lighting => "Lighting",
            ClassName::Atmosphere => "Atmosphere",
            ClassName::SpawnLocation => "SpawnLocation",
            ClassName::Workspace => "Workspace",
            ClassName::Clouds => "Clouds",
            ClassName::Sun => "Sun",
            ClassName::Moon => "Moon",
            ClassName::Seat => "Seat",
            ClassName::VehicleSeat => "VehicleSeat",
            ClassName::Team => "Team",
            ClassName::Document => "Document",
            ClassName::ImageAsset => "ImageAsset",
            ClassName::VideoAsset => "VideoAsset",
            ClassName::VideoFrame => "VideoFrame",
            ClassName::DocumentFrame => "DocumentFrame",
            ClassName::WebFrame => "WebFrame",
            ClassName::TextBox => "TextBox",
            ClassName::ViewportFrame => "ViewportFrame",
            ClassName::SolarSystem => "SolarSystem",
            ClassName::CelestialBody => "CelestialBody",
            ClassName::RegionChunk => "RegionChunk",
        }
    }
    
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "Instance" => Ok(ClassName::Instance),
            "PVInstance" => Ok(ClassName::PVInstance),
            "BasePart" => Ok(ClassName::BasePart),
            "Part" => Ok(ClassName::Part),
            "MeshPart" => Ok(ClassName::MeshPart),
            "Model" => Ok(ClassName::Model),
            "Humanoid" => Ok(ClassName::Humanoid),
            "Camera" => Ok(ClassName::Camera),
            "PointLight" => Ok(ClassName::PointLight),
            "SpotLight" => Ok(ClassName::SpotLight),
            "SurfaceLight" => Ok(ClassName::SurfaceLight),
            "DirectionalLight" => Ok(ClassName::DirectionalLight),
            "Attachment" => Ok(ClassName::Attachment),
            "WeldConstraint" => Ok(ClassName::WeldConstraint),
            "Motor6D" => Ok(ClassName::Motor6D),
            "SpecialMesh" => Ok(ClassName::SpecialMesh),
            "Decal" => Ok(ClassName::Decal),
            "Folder" => Ok(ClassName::Folder),
            "BillboardGui" => Ok(ClassName::BillboardGui),
            "SurfaceGui" => Ok(ClassName::SurfaceGui),
            "ScreenGui" => Ok(ClassName::ScreenGui),
            "TextLabel" => Ok(ClassName::TextLabel),
            "Frame" => Ok(ClassName::Frame),
            "ScrollingFrame" => Ok(ClassName::ScrollingFrame),
            "ImageLabel" => Ok(ClassName::ImageLabel),
            "TextButton" => Ok(ClassName::TextButton),
            "ImageButton" => Ok(ClassName::ImageButton),
            "Animator" => Ok(ClassName::Animator),
            "KeyframeSequence" => Ok(ClassName::KeyframeSequence),
            "ParticleEmitter" => Ok(ClassName::ParticleEmitter),
            "Beam" => Ok(ClassName::Beam),
            "Sound" => Ok(ClassName::Sound),
            "Terrain" => Ok(ClassName::Terrain),
            "Sky" => Ok(ClassName::Sky),
            "UnionOperation" => Ok(ClassName::UnionOperation),
            "SoulScript" => Ok(ClassName::SoulScript),
            "Lighting" => Ok(ClassName::Lighting),
            "Atmosphere" => Ok(ClassName::Atmosphere),
            "SpawnLocation" => Ok(ClassName::SpawnLocation),
            "Workspace" => Ok(ClassName::Workspace),
            "Clouds" => Ok(ClassName::Clouds),
            "Sun" => Ok(ClassName::Sun),
            "Moon" => Ok(ClassName::Moon),
            "Seat" => Ok(ClassName::Seat),
            "VehicleSeat" => Ok(ClassName::VehicleSeat),
            "Document" => Ok(ClassName::Document),
            "ImageAsset" => Ok(ClassName::ImageAsset),
            "VideoAsset" => Ok(ClassName::VideoAsset),
            "VideoFrame" => Ok(ClassName::VideoFrame),
            "DocumentFrame" => Ok(ClassName::DocumentFrame),
            "WebFrame" => Ok(ClassName::WebFrame),
            "TextBox" => Ok(ClassName::TextBox),
            "ViewportFrame" => Ok(ClassName::ViewportFrame),
            "SolarSystem" => Ok(ClassName::SolarSystem),
            "CelestialBody" => Ok(ClassName::CelestialBody),
            "RegionChunk" => Ok(ClassName::RegionChunk),
            _ => Err(format!("Unknown class name: {}", s)),
        }
    }
}

// ============================================================================
// 2. Instance (Base Class: All Entities)
// ============================================================================

/// Core hierarchy/identity component (base for all ~200 Eustress classes)
/// Bevy equivalent: Name + Parent + Entity metadata
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Instance {
    /// Editable label (Eustress "Name")
    pub name: String,
    
    /// ReadOnly type identifier (Eustress "ClassName")
    pub class_name: ClassName,
    
    /// Save eligibility (Eustress "Archivable")
    pub archivable: bool,
    
    /// Unique entity ID (maps to Bevy Entity internally)
    pub id: u32,
    
    /// AI training opt-in flag (default: false)
    /// When true, this entity is included in SpatialVortex training data exports
    /// Controls quality of AI training by allowing selective data inclusion
    pub ai: bool,
}

impl Default for Instance {
    fn default() -> Self {
        Self {
            name: "Instance".to_string(),
            class_name: ClassName::Instance,
            archivable: true,
            id: 0,
            ai: false,
        }
    }
}

// ============================================================================
// 3. PVInstance (Parent of BasePart: Pivot/Position)
// ============================================================================

/// Adds world-space pivot to Instance
/// Bevy equivalent: Transform (pivot point)
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct PVInstance {
    /// Model pivot point (Eustress "Pivot" CFrame)
    /// Bevy maps to Transform for model root
    pub pivot: Transform,
}

impl Default for PVInstance {
    fn default() -> Self {
        Self {
            pivot: Transform::IDENTITY,
        }
    }
}

// ============================================================================
// 4. BasePart (Abstract: Core 3D Object; ~50 props)
// ============================================================================

/// Physical primitive base (inherited by Part, MeshPart, etc.)
/// Handles transform, physics, rendering (~50 properties in Eustress)
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct BasePart {
    // === Transform/Geometry ===
    /// Full pose (Eustress "CFrame")
    /// Bevy: Transform component
    pub cframe: Transform,
    
    /// Dimensions in studs (Eustress "Size")
    /// Bevy: Transform.scale
    pub size: Vec3,
    
    /// Local pivot offset (Eustress "PivotOffset")
    pub pivot_offset: Transform,
    
    // === Appearance/Rendering ===
    /// Tint color (Eustress "Color")
    /// Bevy: StandardMaterial.base_color
    pub color: Color,
    
    /// Material preset (Eustress "Material" enum)
    /// Bevy: StandardMaterial variants with PBR params
    pub material: Material,
    
    /// Opacity 0-1 (Eustress "Transparency")
    /// Bevy: StandardMaterial.alpha_mode
    pub transparency: f32,
    
    /// Mirror-like reflectance 0-1 (Eustress "Reflectance")
    /// Bevy: StandardMaterial.reflectance
    pub reflectance: f32,
    
    // === Physics/Collision ===
    /// Immovable (Eustress "Anchored")
    /// Bevy: RigidBody::Fixed vs Dynamic
    pub anchored: bool,
    
    /// Physics interactions (Eustress "CanCollide")
    /// Bevy: Collider active/inactive
    pub can_collide: bool,
    
    /// Touch event filtering (Eustress "CanTouch")
    pub can_touch: bool,
    
    /// Assembly linear velocity (Eustress "AssemblyLinearVelocity")
    /// Bevy: Velocity.linear (bevy_rapier3d)
    pub assembly_linear_velocity: Vec3,
    
    /// Assembly angular velocity (Eustress "AssemblyAngularVelocity")
    /// Bevy: Velocity.angular
    pub assembly_angular_velocity: Vec3,
    
    /// Custom density/friction (Eustress "CustomPhysicalProperties")
    /// Bevy: ColliderMassProperties
    pub custom_physical_properties: Option<PhysicalProperties>,
    
    /// Collision filtering (Eustress "CollisionGroup")
    /// Bevy: Collision groups (Rapier)
    pub collision_group: String,
    
    // === Physics Properties ===
    /// Material density in kg/m³ (default: 900 for Plastic)
    /// Used to compute mass from volume: mass = density × volume
    /// Common values: Plastic=900, Wood=600, Concrete=2400, Metal=7850, Water=1000
    pub density: f32,
    
    /// Mass in kg - computed from density × volume, or set directly
    /// If set directly, density will be back-calculated from volume
    pub mass: f32,
    
    /// Locked for editing (custom property)
    pub locked: bool,
    
    // === Deformation ===
    /// Enable soft body deformation simulation
    /// When true: mesh vertices deform from stress, temperature, and impacts
    /// When false: behaves as rigid body (default)
    pub deformation: bool,
}

impl Default for BasePart {
    fn default() -> Self {
        Self {
            cframe: Transform::IDENTITY,
            size: Vec3::new(1.0, 1.0, 1.0), // Default 1m³ cube (meters, not studs)
            pivot_offset: Transform::IDENTITY,
            color: Color::srgb(0.6, 0.6, 0.6), // Medium gray
            material: Material::Plastic,
            transparency: 0.0,
            reflectance: 0.0,
            anchored: false,
            can_collide: true,
            can_touch: true,
            assembly_linear_velocity: Vec3::ZERO,
            assembly_angular_velocity: Vec3::ZERO,
            custom_physical_properties: None,
            collision_group: "Default".to_string(),
            // Default density for Plastic material: 900 kg/m³
            density: 900.0,
            // Mass computed from density × volume (1m³ default = 900 kg)
            mass: 900.0,
            locked: false,
            deformation: false,
        }
    }
}

impl BasePart {
    /// Calculate volume in cubic meters from size
    pub fn volume(&self) -> f32 {
        self.size.x * self.size.y * self.size.z
    }
    
    /// Compute mass from current density and size
    /// mass = density × volume
    pub fn compute_mass(&self) -> f32 {
        self.density * self.volume()
    }
    
    /// Update mass based on current density and size
    pub fn update_mass(&mut self) {
        self.mass = self.compute_mass();
    }
    
    /// Set density and automatically update mass
    pub fn set_density(&mut self, density: f32) {
        self.density = density;
        self.update_mass();
    }
    
    /// Set mass and back-calculate density from volume
    /// density = mass / volume
    pub fn set_mass(&mut self, mass: f32) {
        self.mass = mass;
        let volume = self.volume();
        if volume > 0.0 {
            self.density = mass / volume;
        }
    }
    
    /// Update size and recalculate mass (keeping density constant)
    pub fn set_size(&mut self, size: Vec3) {
        self.size = size;
        self.update_mass();
    }
    
    /// Get density for the current material type (default values)
    /// Returns density in kg/m³
    pub fn material_default_density(material: &Material) -> f32 {
        match material {
            Material::Plastic => 900.0,
            Material::SmoothPlastic => 900.0,
            Material::Wood => 600.0,
            Material::WoodPlanks => 500.0,
            Material::Metal => 7850.0,       // Steel
            Material::CorrodedMetal => 7800.0,
            Material::DiamondPlate => 7800.0,
            Material::Foil => 2700.0,        // Aluminum
            Material::Grass => 1000.0,
            Material::Concrete => 2400.0,
            Material::Brick => 1900.0,
            Material::Granite => 2700.0,
            Material::Marble => 2600.0,
            Material::Slate => 2700.0,
            Material::Sand => 1600.0,
            Material::Fabric => 300.0,
            Material::Glass => 2500.0,
            Material::Neon => 0.9,         // Light/gas
            Material::Ice => 918.0,
        }
    }
    
    /// Apply the default density for the current material
    pub fn apply_material_density(&mut self) {
        self.density = Self::material_default_density(&self.material);
        self.update_mass();
    }
}

/// Custom physics properties (Eustress PhysicalProperties)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Reflect)]
pub struct PhysicalProperties {
    pub density: f32,        // kg/stud³
    pub friction: f32,       // 0-2
    pub elasticity: f32,     // 0-1 (bounciness)
    pub friction_weight: f32,
    pub elasticity_weight: f32,
}

impl Default for PhysicalProperties {
    fn default() -> Self {
        Self {
            density: 0.7,  // Plastic default
            friction: 0.3,
            elasticity: 0.5,
            friction_weight: 1.0,
            elasticity_weight: 1.0,
        }
    }
}

// ============================================================================
// 5. Part (Extends BasePart: Primitive Shapes)
// ============================================================================

/// Adds built-in procedural meshes to BasePart
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Part {
    /// Shape type (Eustress "Shape" enum)
    /// Bevy: Handle<Mesh> from procedural generation
    pub shape: PartType,
}

impl Default for Part {
    fn default() -> Self {
        Self {
            shape: PartType::Block,
        }
    }
}

// ============================================================================
// 6. MeshPart (Extends BasePart: Custom Meshes)
// ============================================================================

/// Asset-loaded geometry/textures
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct MeshPart {
    /// Mesh asset URL (Eustress "MeshId" rbxassetid://)
    /// Bevy: Handle<Mesh> from AssetServer
    pub mesh_id: String,
    
    /// Texture asset URL (Eustress "TextureId")
    /// Bevy: Handle<Image>
    pub texture_id: String,
}

impl Default for MeshPart {
    fn default() -> Self {
        Self {
            mesh_id: String::new(),
            texture_id: String::new(),
        }
    }
}

// ============================================================================
// 7. Model (Container: Groups Parts)
// ============================================================================

/// Hierarchical assemblies (e.g., tools, characters)
/// Bevy: Parent entity with Children component
/// 
/// # Domain Scope
/// To use a Model as a domain scope container, add a Parameters component
/// with `is_domain_scope: true` and configure `sync_config`.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Model {
    /// Pivot reference part (Eustress "PrimaryPart")
    /// Bevy: Entity ID of scene root
    pub primary_part: Option<u32>,
    
    /// Computed group pose (Eustress "WorldPivot")
    /// Bevy: GlobalTransform of model entity
    pub world_pivot: Transform,
    
    /// Total mass of all descendant BaseParts in kg (computed, read-only)
    /// 
    /// This is the recursive sum of:
    /// - All direct BasePart children's `mass` values
    /// - All nested Model/Folder children's `assembly_mass` values
    /// 
    /// Updated by `update_assembly_mass_system` which traverses the hierarchy.
    pub assembly_mass: f32,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            primary_part: None,
            world_pivot: Transform::IDENTITY,
            assembly_mass: 0.0,
        }
    }
}

impl Model {
    /// Compute assembly mass from direct children only (non-recursive).
    /// For full recursive computation, use `compute_recursive_assembly_mass`.
    pub fn compute_direct_mass(base_parts: &[&BasePart]) -> f32 {
        base_parts.iter().map(|bp| bp.mass).sum()
    }
}

// ============================================================================
// 8. Humanoid (Character Controller)
// ============================================================================

/// Attached to Model; handles movement/animation
/// Bevy: Custom controller with Rapier/Avian physics
/// 
/// # Network Replication
/// All speed/health properties are replicated to clients.
/// Server validates movement against these values for anti-exploit.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Humanoid {
    // === Movement Properties (studs/sec) ===
    
    /// Base movement speed (Eustress "WalkSpeed")
    /// Default: 16.0 studs/s (~4.5 m/s)
    pub walk_speed: f32,
    
    /// Sprint/run speed (Eustress "RunSpeed" - custom extension)
    /// Default: 32.0 studs/s (~9 m/s)
    pub run_speed: f32,
    
    /// Jump impulse (Eustress "JumpPower")
    /// Default: 50.0 studs/s (yields ~2.5 stud jump height)
    pub jump_power: f32,
    
    /// Maximum slope angle character can walk up (degrees)
    /// Default: 89.0 (nearly vertical)
    pub max_slope_angle: f32,
    
    // === Physics Properties ===
    
    /// Capsule collider offset (Eustress "HipHeight")
    /// Distance from ground to character root (studs)
    pub hip_height: f32,
    
    /// Character can jump when grounded
    pub can_jump: bool,
    
    /// Character can move (set false to freeze)
    pub can_move: bool,
    
    // === Health Properties ===
    
    /// Current health (Eustress "Health")
    pub health: f32,
    
    /// Maximum health (Eustress "MaxHealth")
    pub max_health: f32,
    
    // === Behavior Properties ===
    
    /// Auto-rotate to face movement direction (Eustress "AutoRotate")
    pub auto_rotate: bool,
    
    /// Use platform stand (character stands on moving platforms)
    pub platform_stand: bool,
    
    /// Whether the character is currently sitting (in a Seat)
    pub sitting: bool,
    
    /// The seat entity the character is sitting in (if any)
    #[serde(skip)]
    pub seat_part: Option<Entity>,
}

impl Default for Humanoid {
    fn default() -> Self {
        Self {
            walk_speed: 16.0,      // studs/s (Roblox default)
            run_speed: 32.0,       // studs/s (2x walk)
            jump_power: 50.0,      // studs/s impulse
            max_slope_angle: 89.0, // degrees
            hip_height: 2.0,       // studs
            can_jump: true,
            can_move: true,
            health: 100.0,
            max_health: 100.0,
            auto_rotate: true,
            platform_stand: false,
            sitting: false,
            seat_part: None,
        }
    }
}

impl Humanoid {
    /// Create with custom walk/run speeds
    pub fn with_speeds(walk: f32, run: f32) -> Self {
        Self {
            walk_speed: walk,
            run_speed: run,
            ..default()
        }
    }
    
    /// Get effective speed based on sprint state
    pub fn effective_speed(&self, sprinting: bool) -> f32 {
        if sprinting { self.run_speed } else { self.walk_speed }
    }
    
    /// Take damage, returns true if character died
    pub fn take_damage(&mut self, amount: f32) -> bool {
        self.health = (self.health - amount).max(0.0);
        self.health <= 0.0
    }
    
    /// Heal the character
    pub fn heal(&mut self, amount: f32) {
        self.health = (self.health + amount).min(self.max_health);
    }
    
    /// Is the character alive?
    pub fn is_alive(&self) -> bool {
        self.health > 0.0
    }
    
    /// Set sitting state
    pub fn set_sitting(&mut self, sitting: bool) {
        self.sitting = sitting;
    }
}

// ============================================================================
// 8b. Seat (Extends BasePart: Character Seating)
// ============================================================================

/// Seat component - characters auto-sit when touching (unless disabled)
/// Creates a SeatWeld child that welds HumanoidRootPart to the seat
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Seat {
    /// Whether the seat is disabled (no auto-sit on touch)
    pub disabled: bool,
    
    /// Entity currently occupying the seat (None if empty)
    /// Server-authoritative for networking
    #[serde(skip)]
    pub occupant: Option<Entity>,
    
    /// Offset where character sits relative to seat center (studs)
    pub seat_offset: Vec3,
    
    /// The SeatWeld entity (child of this seat)
    #[serde(skip)]
    pub weld_entity: Option<Entity>,
}

impl Default for Seat {
    fn default() -> Self {
        Self {
            disabled: false,
            occupant: None,
            seat_offset: Vec3::new(0.0, 0.5, 0.0), // Slightly above seat surface
            weld_entity: None,
        }
    }
}

impl Seat {
    /// Check if seat is occupied
    pub fn is_occupied(&self) -> bool {
        self.occupant.is_some()
    }
    
    /// Check if a character can sit (not disabled, not occupied)
    pub fn can_sit(&self) -> bool {
        !self.disabled && self.occupant.is_none()
    }
}

// ============================================================================
// 8c. VehicleSeat (Extends Seat: Vehicle Control)
// ============================================================================

/// Transmission type for vehicle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum TransmissionType {
    #[default]
    Automatic,
    Manual,
    CVT,
}

/// VehicleSeat component - provides throttle/steer input to scripts
/// Input comes from player WASD/controller, scripts can override
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct VehicleSeat {
    // === Base Seat Properties ===
    /// Whether the seat is disabled
    pub disabled: bool,
    
    /// Entity currently occupying the seat
    #[serde(skip)]
    pub occupant: Option<Entity>,
    
    /// Offset where character sits
    pub seat_offset: Vec3,
    
    /// The SeatWeld entity
    #[serde(skip)]
    pub weld_entity: Option<Entity>,
    
    // === Input State (updated by player input or scripts) ===
    /// Throttle input (-1.0 to 1.0, negative = reverse)
    /// From keyboard: W = 1.0, S = -1.0
    /// From controller: Right trigger = 0-1, Left trigger = 0 to -1
    pub throttle: f32,
    
    /// Steering input (-1.0 to 1.0, negative = left)
    /// From keyboard: A = -1.0, D = 1.0
    /// From controller: Left stick X axis
    pub steer: f32,
    
    /// Handbrake state (from Space or controller button)
    pub handbrake: bool,
    
    // === Vehicle Properties ===
    /// Maximum speed in studs/sec
    pub max_speed: f32,
    
    /// Engine torque (force multiplier)
    pub torque: f32,
    
    /// Turn speed in degrees/sec at max steer
    pub turn_speed: f32,
    
    // === Realistic Physics (Avian3D integration) ===
    /// Current gear (0 = neutral, -1 = reverse, 1+ = forward gears)
    pub gear: i32,
    
    /// Gear ratios for each gear (index 0 = reverse, 1 = first, etc.)
    pub gear_ratios: Vec<f32>,
    
    /// Final drive ratio (differential)
    pub final_drive_ratio: f32,
    
    /// Transmission type
    pub transmission: TransmissionType,
    
    /// Engine RPM (current)
    pub rpm: f32,
    
    /// Engine idle RPM
    pub idle_rpm: f32,
    
    /// Engine redline RPM
    pub redline_rpm: f32,
    
    /// Wheel radius in studs (for speed calculation)
    pub wheel_radius: f32,
    
    /// Vehicle mass in kg (for physics)
    pub mass: f32,
    
    /// Drag coefficient (air resistance)
    pub drag_coefficient: f32,
    
    /// Rolling resistance coefficient
    pub rolling_resistance: f32,
    
    /// Brake force multiplier
    pub brake_force: f32,
    
    /// Whether script has overridden player input
    pub script_override: bool,
}

impl Default for VehicleSeat {
    fn default() -> Self {
        Self {
            // Base seat
            disabled: false,
            occupant: None,
            seat_offset: Vec3::new(0.0, 0.5, 0.0),
            weld_entity: None,
            
            // Input
            throttle: 0.0,
            steer: 0.0,
            handbrake: false,
            
            // Vehicle properties
            max_speed: 100.0,  // studs/sec (~28 m/s, ~100 km/h)
            torque: 500.0,
            turn_speed: 90.0,  // degrees/sec
            
            // Realistic physics
            gear: 0,  // Neutral
            gear_ratios: vec![
                -3.5,  // Reverse
                3.5,   // 1st
                2.5,   // 2nd
                1.8,   // 3rd
                1.3,   // 4th
                1.0,   // 5th
                0.8,   // 6th
            ],
            final_drive_ratio: 3.7,
            transmission: TransmissionType::Automatic,
            rpm: 800.0,
            idle_rpm: 800.0,
            redline_rpm: 7000.0,
            wheel_radius: 1.0,  // studs
            mass: 1500.0,       // kg
            drag_coefficient: 0.3,
            rolling_resistance: 0.015,
            brake_force: 2000.0,
            script_override: false,
        }
    }
}

impl VehicleSeat {
    /// Check if seat is occupied
    pub fn is_occupied(&self) -> bool {
        self.occupant.is_some()
    }
    
    /// Check if a character can sit
    pub fn can_sit(&self) -> bool {
        !self.disabled && self.occupant.is_none()
    }
    
    /// Get current gear ratio (returns 0 if in neutral)
    pub fn current_gear_ratio(&self) -> f32 {
        if self.gear == 0 {
            0.0
        } else if self.gear < 0 {
            self.gear_ratios.first().copied().unwrap_or(-3.5)
        } else {
            self.gear_ratios.get(self.gear as usize).copied().unwrap_or(1.0)
        }
    }
    
    /// Calculate wheel torque from engine torque
    pub fn wheel_torque(&self) -> f32 {
        self.torque * self.current_gear_ratio() * self.final_drive_ratio * self.throttle
    }
    
    /// Calculate theoretical max speed for current gear
    pub fn theoretical_max_speed(&self) -> f32 {
        if self.gear == 0 {
            return 0.0;
        }
        let gear_ratio = self.current_gear_ratio().abs();
        if gear_ratio == 0.0 {
            return 0.0;
        }
        // v = (RPM * wheel_circumference) / (gear_ratio * final_drive * 60)
        let wheel_circumference = 2.0 * std::f32::consts::PI * self.wheel_radius;
        (self.redline_rpm * wheel_circumference) / (gear_ratio * self.final_drive_ratio * 60.0)
    }
    
    /// Shift up (manual transmission)
    pub fn shift_up(&mut self) {
        if self.gear < (self.gear_ratios.len() as i32 - 1) {
            self.gear += 1;
        }
    }
    
    /// Shift down (manual transmission)
    pub fn shift_down(&mut self) {
        if self.gear > -1 {
            self.gear -= 1;
        }
    }
    
    /// Auto-shift based on RPM (for automatic transmission)
    pub fn auto_shift(&mut self) {
        if self.transmission != TransmissionType::Automatic {
            return;
        }
        
        // Shift up at 80% of redline
        if self.rpm > self.redline_rpm * 0.8 && self.gear > 0 {
            self.shift_up();
        }
        // Shift down at 30% of redline
        else if self.rpm < self.redline_rpm * 0.3 && self.gear > 1 {
            self.shift_down();
        }
    }
}

// ============================================================================
// 9. Team Class (Child of Teams service)
// ============================================================================

/// Team - Groups players together for gameplay purposes
/// Similar to Roblox Team, child of Teams service
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Team {
    /// Team display name
    pub name: String,
    
    /// Team color (RGBA) - used for player name tags, leaderboard, etc.
    pub team_color: [f32; 4],
    
    /// Whether players can be auto-assigned to this team
    pub auto_assignable: bool,
}

impl Default for Team {
    fn default() -> Self {
        Self {
            name: "Team".to_string(),
            team_color: [0.0, 0.5, 1.0, 1.0], // Blue default
            auto_assignable: true,
        }
    }
}

impl Team {
    /// Create a new team with a name and color
    pub fn new(name: &str, color: [f32; 4]) -> Self {
        Self {
            name: name.to_string(),
            team_color: color,
            auto_assignable: true,
        }
    }
    
    /// Preset team colors (Roblox-style)
    pub fn red() -> Self {
        Self::new("Red Team", [1.0, 0.2, 0.2, 1.0])
    }
    
    pub fn blue() -> Self {
        Self::new("Blue Team", [0.2, 0.4, 1.0, 1.0])
    }
    
    pub fn green() -> Self {
        Self::new("Green Team", [0.2, 0.8, 0.2, 1.0])
    }
    
    pub fn yellow() -> Self {
        Self::new("Yellow Team", [1.0, 0.9, 0.2, 1.0])
    }
}

// ============================================================================
// 10. SeatWeld (Constraint: Welds character to seat)
// ============================================================================

/// SeatWeld - Welds a character's HumanoidRootPart to a Seat
/// This is a child entity of the Seat, created when a character sits
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct SeatWeld {
    /// The seat entity this weld belongs to
    pub seat: Entity,
    
    /// The HumanoidRootPart entity being welded
    pub humanoid_root_part: Entity,
    
    /// Offset from seat to character (CFrame offset)
    pub c0: Transform,
    
    /// Offset from character root (usually identity)
    pub c1: Transform,
    
    /// Whether the weld is currently active
    pub enabled: bool,
}

impl Default for SeatWeld {
    fn default() -> Self {
        Self {
            seat: Entity::PLACEHOLDER,
            humanoid_root_part: Entity::PLACEHOLDER,
            c0: Transform::IDENTITY,
            c1: Transform::IDENTITY,
            enabled: true,
        }
    }
}

// ============================================================================
// 9. Camera (Viewport Control)
// ============================================================================

/// Per-player view (Eustress Camera)
/// Bevy: Camera3dBundle + custom controller
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct EustressCamera {
    /// View pose (Eustress "CFrame")
    /// Bevy: Transform + Camera component
    pub cframe: Transform,
    
    /// FOV in degrees (Eustress "FieldOfView")
    /// Bevy: PerspectiveProjection.fov (radians)
    pub field_of_view: f32,
    
    /// Near clipping plane distance
    pub near_clip: f32,
    
    /// Far clipping plane distance
    pub far_clip: f32,
    
    /// Focus target (Eustress "Focus")
    pub focus: Option<u32>,
    
    /// Camera type (Eustress "CameraType")
    /// Options: "Custom", "Scriptable", "Follow", "Track", "Watch", "Attach", "Fixed"
    pub camera_type: String,
    
    /// Camera subject (Eustress "CameraSubject")
    pub camera_subject: Option<u32>,
    
    /// Maximum zoom distance
    pub max_zoom_distance: f32,
    
    /// Minimum zoom distance
    pub min_zoom_distance: f32,
    
    /// Head movement enabled (for first-person)
    pub head_locked: bool,
    
    /// Head scale (affects first-person view)
    pub head_scale: f32,
}

impl Default for EustressCamera {
    fn default() -> Self {
        Self {
            cframe: Transform::from_xyz(0.0, 10.0, 10.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
            field_of_view: 70.0,  // Default from user rules
            near_clip: 0.1,
            far_clip: 10000.0,
            focus: None,
            camera_type: "Custom".to_string(),
            camera_subject: None,
            max_zoom_distance: 400.0,
            min_zoom_distance: 0.5,
            head_locked: false,
            head_scale: 1.0,
        }
    }
}

// ============================================================================
// 10. Light Classes
// ============================================================================

/// Omni light (Eustress PointLight)
/// Bevy: PointLightBundle with optional PointLightTexture
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct EustressPointLight {
    /// Strength (Eustress "Brightness")
    /// Bevy: PointLight.intensity
    pub brightness: f32,
    
    /// Hue (Eustress "Color")
    /// Bevy: PointLight.color
    pub color: Color,
    
    /// Falloff distance (Eustress "Range")
    /// Bevy: PointLight.range
    pub range: f32,
    
    /// Cast shadows (Eustress "Shadows")
    pub shadows: bool,
    
    /// Optional light texture/cookie (Bevy 0.17+: PointLightTexture)
    /// Asset path to a cubemap texture that modulates light intensity.
    /// Used for artistic effects like stained glass, gobos, or patterned shadows.
    /// Format: "assets/textures/light_cookie.ktx2" or content hash
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
}

impl Default for EustressPointLight {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            color: Color::WHITE,
            range: 60.0,
            shadows: true,
            texture: None,
        }
    }
}

/// Spot light (Eustress SpotLight)
/// Bevy: SpotLightBundle with optional SpotLightTexture
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct EustressSpotLight {
    pub brightness: f32,
    pub color: Color,
    pub range: f32,
    /// Cone angle in degrees (Eustress "Angle")
    pub angle: f32,
    pub shadows: bool,
    
    /// Optional light texture/cookie (Bevy 0.17+: SpotLightTexture)
    /// Asset path to a 2D texture that modulates light intensity.
    /// Projects the texture pattern onto illuminated surfaces.
    /// Format: "assets/textures/spotlight_cookie.png" or content hash
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
}

impl Default for EustressSpotLight {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            color: Color::WHITE,
            range: 60.0,
            angle: 90.0,
            shadows: true,
            texture: None,
        }
    }
}

/// Surface light (Eustress SurfaceLight)
/// Emits light from a specific face of a part
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct SurfaceLight {
    pub brightness: f32,
    pub color: Color,
    pub range: f32,
    pub face: String,  // "Top", "Bottom", "Front", "Back", "Left", "Right"
    pub shadows: bool,
    
    /// Optional light texture/cookie
    /// Asset path to a 2D texture that modulates light intensity from this surface.
    /// Format: "assets/textures/surface_cookie.png" or content hash
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
}

impl Default for SurfaceLight {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            color: Color::WHITE,
            range: 60.0,
            face: "Front".to_string(),
            shadows: true,
            texture: None,
        }
    }
}

/// Directional light (Eustress DirectionalLight / Sun)
/// Bevy: DirectionalLight with optional DirectionalLightTexture
/// 
/// Simulates distant light sources like the sun. All rays are parallel.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct EustressDirectionalLight {
    /// Light intensity/brightness
    pub brightness: f32,
    
    /// Light color
    pub color: Color,
    
    /// Cast shadows
    pub shadows: bool,
    
    /// Shadow depth bias (prevents shadow acne)
    #[serde(default = "default_shadow_depth_bias")]
    pub shadow_depth_bias: f32,
    
    /// Shadow normal bias
    #[serde(default = "default_shadow_normal_bias")]
    pub shadow_normal_bias: f32,
    
    /// Optional light texture/cookie (Bevy 0.17+: DirectionalLightTexture)
    /// Asset path to a 2D texture that modulates light intensity.
    /// Creates patterns like cloud shadows, window frames, or foliage dappling.
    /// Format: "assets/textures/cloud_shadows.png" or content hash
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
}

fn default_shadow_depth_bias() -> f32 { 0.02 }
fn default_shadow_normal_bias() -> f32 { 1.8 }

impl Default for EustressDirectionalLight {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            color: Color::WHITE,
            shadows: true,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 1.8,
            texture: None,
        }
    }
}

// Note: Using full Eustress* names to avoid conflicts with Bevy types

// ============================================================================
// SpawnLocation (Player Spawn Point)
// ============================================================================

/// Player spawn point - characters spawn at these locations
/// Similar to Roblox's SpawnLocation class
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct SpawnLocation {
    /// Team ID this spawn belongs to (0 = neutral/all teams)
    pub team_id: u32,
    
    /// Team name for display/legacy (optional, prefer team_id)
    #[serde(default)]
    pub team_name: String,
    
    /// Is this spawn enabled
    pub enabled: bool,
    
    /// Duration player is invulnerable after spawning (seconds)
    pub spawn_protection_duration: f32,
    
    /// Allow team change when touching this spawn
    pub allow_team_change: bool,
    
    /// Spawn priority (higher = more likely to be chosen)
    pub priority: i32,
    
    /// Neutral spawn - any team can use (overrides team_id)
    #[serde(default = "default_true")]
    pub neutral: bool,
}

fn default_true() -> bool { true }

impl Default for SpawnLocation {
    fn default() -> Self {
        Self {
            team_id: 0,
            team_name: String::new(),
            enabled: true,
            spawn_protection_duration: 3.0,
            allow_team_change: false,
            priority: 0,
            neutral: true,
        }
    }
}

impl SpawnLocation {
    /// Create a neutral spawn (any team can use)
    pub fn neutral() -> Self {
        Self::default()
    }
    
    /// Create a team-specific spawn by ID
    pub fn for_team_id(team_id: u32) -> Self {
        Self {
            team_id,
            neutral: false,
            ..default()
        }
    }
    
    /// Create a team-specific spawn by name (legacy)
    pub fn for_team(team_name: impl Into<String>) -> Self {
        Self {
            team_name: team_name.into(),
            neutral: false,
            ..default()
        }
    }
    
    /// Check if this spawn can be used by a player with given team ID
    pub fn can_spawn_team_id(&self, player_team_id: Option<u32>) -> bool {
        if !self.enabled {
            return false;
        }
        
        // Neutral spawns work for everyone
        if self.neutral || self.team_id == 0 {
            return true;
        }
        
        // Team-specific spawns only work for matching team
        match player_team_id {
            Some(id) => self.team_id == id,
            None => false, // No team can't use team-specific spawns
        }
    }
    
    /// Check if this spawn can be used by a team (legacy string-based)
    pub fn can_spawn(&self, player_team: Option<&str>) -> bool {
        if !self.enabled {
            return false;
        }
        
        // Neutral spawns work for everyone
        if self.neutral || (self.team_id == 0 && self.team_name.is_empty()) {
            return true;
        }
        
        // Team-specific spawns only work for matching team
        match player_team {
            Some(team) => self.team_name == team,
            None => false,
        }
    }
}

// ============================================================================
// 11. Attachment (Local Offset for Lights/Joints)
// ============================================================================

/// Defines child positions relative to a BasePart (e.g., gun muzzle)
/// Bevy: Child entity with relative Transform
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Attachment {
    /// Local position (Eustress "Position")
    /// Bevy: Transform.translation (relative to parent)
    pub position: Vec3,
    
    /// Local rotation (Eustress "Orientation" in degrees)
    /// Bevy: Transform.rotation
    pub orientation: Vec3,
    
    /// Computed local transform (Eustress "CFrame")
    /// ReadOnly; syncs to Bevy Transform
    pub cframe: Transform,
    
    /// Identifier for targeting (Eustress "Name")
    pub name: String,
}

impl Default for Attachment {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            orientation: Vec3::ZERO,
            cframe: Transform::IDENTITY,
            name: "Attachment".to_string(),
        }
    }
}

// ============================================================================
// 12. WeldConstraint (Physics Joint: Fixed Link)
// ============================================================================

/// Welds two BaseParts rigidly
/// Bevy: bevy_rapier3d::FixedJoint
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct WeldConstraint {
    /// Parent part (Eustress "Part0")
    /// Bevy: Joint entity A
    pub part0: Option<u32>,
    
    /// Child part (Eustress "Part1")
    /// Bevy: Joint entity B
    pub part1: Option<u32>,
    
    /// Relative offset for Part0 (Eustress "C0")
    pub c0: Transform,
    
    /// Relative offset for Part1 (Eustress "C1")
    pub c1: Transform,
    
    /// Toggle joint (Eustress "Enabled")
    /// Bevy: JointEnabled component
    pub enabled: bool,
}

impl Default for WeldConstraint {
    fn default() -> Self {
        Self {
            part0: None,
            part1: None,
            c0: Transform::IDENTITY,
            c1: Transform::IDENTITY,
            enabled: true,
        }
    }
}

// ============================================================================
// 13. Motor6D (Animation Joint: Dynamic Weld)
// ============================================================================

/// For rigs/animations; rotates/translates
/// Bevy: bevy_rapier::RevoluteJoint + AnimationClip
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Motor6D {
    /// Parent bone (Eustress "Part0")
    pub part0: Option<u32>,
    
    /// Child bone (Eustress "Part1")
    pub part1: Option<u32>,
    
    /// Bind pose for Part0 (Eustress "C0")
    pub c0: Transform,
    
    /// Bind pose for Part1 (Eustress "C1")
    pub c1: Transform,
    
    /// Animated local pose (Eustress "Transform")
    /// Runtime pose from animation
    pub transform: Transform,
    
    /// Current desired angle (Eustress "DesiredAngle")
    pub desired_angle: f32,
    
    /// Max velocity (Eustress "MaxVelocity")
    pub max_velocity: f32,
}

impl Default for Motor6D {
    fn default() -> Self {
        Self {
            part0: None,
            part1: None,
            c0: Transform::IDENTITY,
            c1: Transform::IDENTITY,
            transform: Transform::IDENTITY,
            desired_angle: 0.0,
            max_velocity: 0.1,
        }
    }
}

// ============================================================================
// 14. SpecialMesh (Legacy Mesh Scaler)
// ============================================================================

/// Scales imported meshes
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct SpecialMesh {
    /// Mesh type (Eustress "MeshType" enum)
    pub mesh_type: MeshType,
    
    /// Non-uniform scale (Eustress "Scale")
    /// Bevy: Transform.scale
    pub scale: Vec3,
    
    /// Asset reference (Eustress "MeshId")
    /// Bevy: Handle<Mesh>
    pub mesh_id: String,
    
    /// Offset position (Eustress "Offset")
    pub offset: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum MeshType {
    FileMesh,
    Head,
    Torso,
    Brick,
    Sphere,
    Cylinder,
}

impl MeshType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MeshType::FileMesh => "FileMesh",
            MeshType::Head => "Head",
            MeshType::Torso => "Torso",
            MeshType::Brick => "Brick",
            MeshType::Sphere => "Sphere",
            MeshType::Cylinder => "Cylinder",
        }
    }
}

impl Default for SpecialMesh {
    fn default() -> Self {
        Self {
            mesh_type: MeshType::FileMesh,
            scale: Vec3::ONE,
            mesh_id: String::new(),
            offset: Vec3::ZERO,
        }
    }
}

// ============================================================================
// 15. Decal (Surface Texture)
// ============================================================================

/// Projects image onto surfaces using Bevy's native ForwardDecal system (Bevy 0.16+)
/// 
/// Forward decals (contact projective decals) project textures onto geometry
/// in the scene. They work by sampling the depth buffer and projecting the
/// decal texture onto surfaces within the decal's bounding volume.
/// 
/// # Usage
/// Spawn with `ForwardDecal` component, `MeshMaterial3d<ForwardDecalMaterial>`,
/// and a `Transform` for position/scale. The camera must have `DepthPrepass`.
/// 
/// # Example
/// ```ignore
/// commands.spawn((
///     Decal::new("textures/blood_splatter.png"),
///     ForwardDecal,
///     MeshMaterial3d(decal_materials.add(ForwardDecalMaterial {
///         base: StandardMaterial {
///             base_color_texture: Some(asset_server.load("textures/blood_splatter.png")),
///             alpha_mode: AlphaMode::Blend,
///             ..default()
///         },
///         extension: ForwardDecalMaterialExt {
///             depth_fade_factor: 1.0,
///         },
///     })),
///     Transform::from_xyz(0.0, 0.1, 0.0).with_scale(Vec3::splat(2.0)),
/// ));
/// ```
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Decal {
    /// Texture asset path for the decal image
    /// Bevy: Loaded via AssetServer, used in ForwardDecalMaterial
    pub texture: String,
    
    /// Target face for legacy compatibility (Eustress "Face")
    /// Note: Bevy's ForwardDecal projects in -Z direction of the transform
    /// Use transform rotation to control projection direction
    pub face: Face,
    
    /// Transparency/alpha value (0.0 = opaque, 1.0 = invisible)
    /// Bevy: Use AlphaMode::Blend in the material
    pub transparency: f32,
    
    /// Depth fade factor - how quickly the decal fades at surface edges
    /// Bevy: ForwardDecalMaterialExt::depth_fade_factor
    /// Higher values = sharper edges, lower = softer blend
    pub depth_fade_factor: f32,
    
    /// Decal color tint (multiplied with texture)
    pub color: [f32; 4],
    
    /// Z-buffer offset for layering multiple decals (legacy)
    pub z_index: i32,
}

/// Face enum for legacy compatibility with Roblox-style face-based decals
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum Face {
    Top,
    Bottom,
    #[default]
    Front,
    Back,
    Left,
    Right,
}

impl Face {
    pub fn as_str(&self) -> &'static str {
        match self {
            Face::Top => "Top",
            Face::Bottom => "Bottom",
            Face::Front => "Front",
            Face::Back => "Back",
            Face::Left => "Left",
            Face::Right => "Right",
        }
    }
    
    /// Convert face to a rotation quaternion for ForwardDecal projection
    /// ForwardDecal projects in the -Z direction, so we rotate accordingly
    pub fn to_rotation(&self) -> bevy::math::Quat {
        use bevy::math::Quat;
        use std::f32::consts::FRAC_PI_2;
        
        match self {
            Face::Front => Quat::IDENTITY,                              // -Z (default)
            Face::Back => Quat::from_rotation_y(std::f32::consts::PI),  // +Z
            Face::Left => Quat::from_rotation_y(FRAC_PI_2),             // -X
            Face::Right => Quat::from_rotation_y(-FRAC_PI_2),           // +X
            Face::Top => Quat::from_rotation_x(FRAC_PI_2),              // -Y (down)
            Face::Bottom => Quat::from_rotation_x(-FRAC_PI_2),          // +Y (up)
        }
    }
}

impl Default for Decal {
    fn default() -> Self {
        Self {
            texture: String::new(),
            face: Face::Front,
            transparency: 0.0,
            depth_fade_factor: 1.0,
            color: [1.0, 1.0, 1.0, 1.0],
            z_index: 0,
        }
    }
}

impl Decal {
    /// Create a new decal with the given texture path
    pub fn new(texture: impl Into<String>) -> Self {
        Self {
            texture: texture.into(),
            ..default()
        }
    }
    
    /// Set the target face for projection
    pub fn with_face(mut self, face: Face) -> Self {
        self.face = face;
        self
    }
    
    /// Set the transparency (0.0 = opaque, 1.0 = invisible)
    pub fn with_transparency(mut self, transparency: f32) -> Self {
        self.transparency = transparency.clamp(0.0, 1.0);
        self
    }
    
    /// Set the depth fade factor for edge blending
    pub fn with_depth_fade(mut self, factor: f32) -> Self {
        self.depth_fade_factor = factor.max(0.0);
        self
    }
    
    /// Set the color tint
    pub fn with_color(mut self, color: [f32; 4]) -> Self {
        self.color = color;
        self
    }
    
    /// Get the alpha value for the material (inverse of transparency)
    pub fn alpha(&self) -> f32 {
        1.0 - self.transparency
    }
}

// ============================================================================
// 16. Folder (Hierarchy Container)
// ============================================================================

/// Non-rendered logical grouping container
/// Bevy: Entity with Children but no rendering components
/// 
/// # Domain Scope
/// To use a Folder as a domain scope container, add a Parameters component
/// with `is_domain_scope: true` and configure `sync_config`.
/// 
/// # Example
/// ```text
/// 📁 Hospital_A (Parameters with domain: "Patient", is_domain_scope: true)
///    ├── Patient_001 (inherits Patient domain)
///    └── Patient_002 (inherits Patient domain)
/// ```
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Folder {
    /// Total mass of all descendant BaseParts in kg (computed, read-only)
    /// 
    /// This is the recursive sum of:
    /// - All direct BasePart children's `mass` values
    /// - All nested Model/Folder children's `assembly_mass` values
    /// 
    /// Updated by `update_assembly_mass_system` which traverses the hierarchy.
    pub assembly_mass: f32,
}

impl Default for Folder {
    fn default() -> Self {
        Self {
            assembly_mass: 0.0,
        }
    }
}

impl Folder {
    /// Compute assembly mass from direct children only (non-recursive).
    /// For full recursive computation, use `compute_recursive_assembly_mass`.
    pub fn compute_direct_mass(base_parts: &[&BasePart]) -> f32 {
        base_parts.iter().map(|bp| bp.mass).sum()
    }
}

/// Configuration for syncing domain data to child entities
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct DomainSyncConfig {
    /// Target class type for spawned entities
    pub target_class: SyncTargetClass,
    
    /// Spawn layout pattern
    pub layout: SpawnLayout,
    
    /// Spacing between spawned entities (studs)
    pub spacing: [f32; 3],
    
    /// Starting position offset from folder origin
    pub origin_offset: [f32; 3],
    
    /// Default size for spawned Parts/MeshParts
    pub default_size: [f32; 3],
    
    /// Default color for spawned entities (RGBA)
    pub default_color: [f32; 4],
    
    /// Field path to derive entity color from (e.g., "status" -> red/green)
    pub color_field: Option<String>,
    
    /// Color mapping rules (field_value -> color)
    pub color_mappings: Vec<ColorMapping>,
    
    /// Field path to derive entity name from
    pub name_field: Option<String>,
    
    /// Show BillboardGui label above entities
    pub show_billboard: bool,
    
    /// Field path for billboard text content
    pub billboard_field: Option<String>,
    
    /// Billboard offset from entity center
    pub billboard_offset: [f32; 3],
    
    /// Billboard text alignment (maps to TextLabel.TextXAlignment)
    pub billboard_alignment: TextXAlignment,
    
    /// Field mappings from data to entity properties/attributes
    pub field_mappings: Vec<crate::parameters::FieldMapping>,
    
    /// Track which entities were synced (for stale detection)
    pub last_sync_ids: Vec<String>,
}

/// Target class type for domain sync spawning
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum SyncTargetClass {
    #[default]
    Part,
    MeshPart,
    Model,
    Folder,
}

/// Spawn layout pattern for domain-synced entities
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub enum SpawnLayout {
    /// Spawn vertically (Y-axis)
    #[default]
    Vertical,
    /// Spawn horizontally (X-axis)
    Horizontal,
    /// Spawn in depth (Z-axis)
    Depth,
    /// Spawn in a grid pattern
    Grid { columns: u32 },
    /// Spawn in a radial pattern
    Radial { radius: f32 },
    /// Stack on top of each other
    Stacked,
}

/// Color mapping rule for conditional formatting
#[derive(Debug, Clone, Serialize, Deserialize, Reflect, Default)]
pub struct ColorMapping {
    /// Field value to match (string comparison)
    pub field_value: String,
    /// Color to apply when matched (RGBA)
    pub color: [f32; 4],
}

/// Marker component for entities synced from domain data
/// Used for stale detection and update tracking
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct DomainSyncedEntity {
    /// The unique ID from the external data source
    pub source_id: String,
    /// Timestamp of last sync
    pub last_synced: f64,
    /// Whether this entity is stale (data no longer in source)
    pub is_stale: bool,
}

// ============================================================================
// 17. BillboardGui (3D Camera-Facing GUI)
// ============================================================================

/// Camera-facing GUI in 3D space (e.g., nametags)
/// Bevy: bevy_billboard or custom billboard shader
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct BillboardGui {
    // Behavior
    pub active: bool,
    pub adornee: Option<Entity>,  // Parent part/attachment
    pub always_on_top: bool,
    pub enabled: bool,
    pub clips_descendants: bool,
    pub reset_on_spawn: bool,
    pub stiffness_by_distance: bool,
    
    // Distance
    pub distance_lower_limit: f32,
    pub distance_step: f32,
    pub distance_upper_limit: f32,
    pub max_distance: f32,
    
    // Appearance
    pub brightness: f32,
    pub light_influence: f32,
    
    // Size/Position
    pub size: [f32; 2],  // UDim2 simplified (scale only)
    pub size_offset: [f32; 2],  // Normalized offset
    pub extents_offset: [f32; 3],  // Pixel offset
    pub extents_offset_world_space: [f32; 3],
    pub units_offset: [f32; 3],  // Local offset (units)
    pub units_offset_world_space: [f32; 3],
    
    // Sorting
    pub z_index_behavior: ZIndexBehavior,
    
    // Runtime (read-only)
    pub current_distance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum ZIndexBehavior {
    Sibling,
    Global,
}

impl Default for BillboardGui {
    fn default() -> Self {
        Self {
            active: true,
            adornee: None,
            always_on_top: false,
            enabled: true,
            clips_descendants: false,
            reset_on_spawn: true,
            stiffness_by_distance: false,
            
            distance_lower_limit: 0.0,
            distance_step: 0.0,
            distance_upper_limit: f32::MAX,
            max_distance: 1962.0,  // Eustress default
            
            brightness: 1.0,
            light_influence: 1.0,
            
            size: [1.0, 1.0],
            size_offset: [0.0, 0.0],
            extents_offset: [0.0, 0.0, 0.0],
            extents_offset_world_space: [0.0, 0.0, 0.0],
            units_offset: [0.0, 0.0, 0.0],
            units_offset_world_space: [0.0, 0.0, 0.0],
            
            z_index_behavior: ZIndexBehavior::Sibling,
            
            current_distance: 0.0,
        }
    }
}

// ============================================================================
// 17b. SurfaceGui (GUI Rendered on Part Surface)
// ============================================================================

/// Which face of a part the SurfaceGui renders on
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum NormalId {
    #[default]
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
}

impl NormalId {
    pub fn to_normal(&self) -> Vec3 {
        match self {
            NormalId::Front => Vec3::new(0.0, 0.0, 1.0),
            NormalId::Back => Vec3::new(0.0, 0.0, -1.0),
            NormalId::Top => Vec3::new(0.0, 1.0, 0.0),
            NormalId::Bottom => Vec3::new(0.0, -1.0, 0.0),
            NormalId::Right => Vec3::new(1.0, 0.0, 0.0),
            NormalId::Left => Vec3::new(-1.0, 0.0, 0.0),
        }
    }
    
    pub fn as_str(&self) -> &'static str {
        match self {
            NormalId::Front => "Front",
            NormalId::Back => "Back",
            NormalId::Top => "Top",
            NormalId::Bottom => "Bottom",
            NormalId::Left => "Left",
            NormalId::Right => "Right",
        }
    }
}

/// GUI that renders on a face of a 3D part
/// Similar to Roblox's SurfaceGui - renders UI elements on part surfaces
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SurfaceGui {
    /// Whether the GUI is active
    pub active: bool,
    /// Whether the GUI is enabled
    pub enabled: bool,
    /// The part this GUI is attached to (parent)
    pub adornee: Option<Entity>,
    /// Which face of the part to render on
    pub face: NormalId,
    /// Canvas size in pixels (virtual resolution)
    pub canvas_size: [f32; 2],
    /// Whether to always render on top of 3D geometry
    pub always_on_top: bool,
    /// Brightness multiplier
    pub brightness: f32,
    /// How much scene lighting affects the GUI (0-1)
    pub light_influence: f32,
    /// Pixels per unit (resolution scaling)
    pub pixels_per_unit: f32,
    /// Z-index sorting behavior
    pub z_index_behavior: ZIndexBehavior,
    /// Whether to clip child elements to bounds
    pub clips_descendants: bool,
    /// Maximum render distance (studs)
    pub max_distance: f32,
    /// Horizontal alignment on the face
    pub horizontal_alignment: HorizontalAlignment,
    /// Vertical alignment on the face
    pub vertical_alignment: VerticalAlignment,
    /// Size relative to face (0-1 scale)
    pub size_scale: [f32; 2],
    /// Offset in pixels
    pub size_offset: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum HorizontalAlignment {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum VerticalAlignment {
    Top,
    #[default]
    Center,
    Bottom,
}

impl Default for SurfaceGui {
    fn default() -> Self {
        Self {
            active: true,
            enabled: true,
            adornee: None,
            face: NormalId::Front,
            canvas_size: [800.0, 600.0],
            always_on_top: false,
            brightness: 1.0,
            light_influence: 0.0,
            pixels_per_unit: 50.0,
            z_index_behavior: ZIndexBehavior::Sibling,
            clips_descendants: true,
            max_distance: 1000.0,
            horizontal_alignment: HorizontalAlignment::Center,
            vertical_alignment: VerticalAlignment::Center,
            size_scale: [1.0, 1.0],
            size_offset: [0.0, 0.0],
        }
    }
}

// ============================================================================
// 17c. ScreenGui (2D Screen-Space GUI)
// ============================================================================

/// GUI that renders in 2D screen space (HUD, menus, etc.)
/// Similar to Roblox's ScreenGui
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ScreenGui {
    /// Whether the GUI is enabled
    pub enabled: bool,
    /// Display order (higher = rendered on top)
    pub display_order: i32,
    /// Whether to ignore the GUI inset (safe area)
    pub ignore_gui_inset: bool,
    /// Whether to reset on player spawn
    pub reset_on_spawn: bool,
    /// Z-index sorting behavior
    pub z_index_behavior: ZIndexBehavior,
    /// Whether to clip child elements to screen bounds
    pub clips_descendants: bool,
    /// Screen position (anchor point)
    pub screen_insets: ScreenInsets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum ScreenInsets {
    #[default]
    None,
    DeviceSafeInsets,
    CoreUISafeInsets,
}

impl Default for ScreenGui {
    fn default() -> Self {
        Self {
            enabled: true,
            display_order: 0,
            ignore_gui_inset: false,
            reset_on_spawn: true,
            z_index_behavior: ZIndexBehavior::Sibling,
            clips_descendants: true,
            screen_insets: ScreenInsets::None,
        }
    }
}

// ============================================================================
// 17d. Frame (Container Element)
// ============================================================================

/// Container element for organizing UI elements
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Frame {
    /// Whether the frame is visible
    pub visible: bool,
    /// Background color (RGB)
    pub background_color3: [f32; 3],
    /// Background transparency (0 = opaque, 1 = invisible)
    pub background_transparency: f32,
    /// Border color (RGB)
    pub border_color3: [f32; 3],
    /// Border size in pixels
    pub border_size_pixel: i32,
    /// Border mode
    pub border_mode: BorderMode,
    /// Whether to clip children to frame bounds
    pub clips_descendants: bool,
    /// Z-index for layering
    pub z_index: i32,
    /// Layout order for automatic layouts
    pub layout_order: i32,
    /// Rotation in degrees
    pub rotation: f32,
    /// Anchor point (0-1, relative to size)
    pub anchor_point: [f32; 2],
    /// Position (scale + offset)
    pub position_scale: [f32; 2],
    pub position_offset: [f32; 2],
    /// Size (scale + offset)
    pub size_scale: [f32; 2],
    pub size_offset: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum BorderMode {
    #[default]
    Outline,
    Middle,
    Inset,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            visible: true,
            background_color3: [1.0, 1.0, 1.0],
            background_transparency: 0.0,
            border_color3: [0.1, 0.1, 0.1],
            border_size_pixel: 1,
            border_mode: BorderMode::Outline,
            clips_descendants: true,
            z_index: 1,
            layout_order: 0,
            rotation: 0.0,
            anchor_point: [0.0, 0.0],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: [100.0, 100.0],
        }
    }
}

// ============================================================================
// 17d-2. ScrollingFrame (Scrollable Container)
// ============================================================================

/// Scrollable container for UI elements with scrollbars
/// Roblox-style scrolling frame with canvas and scrollbar customization
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ScrollingFrame {
    /// Whether the frame is visible
    pub visible: bool,
    /// Background color (RGB)
    pub background_color3: [f32; 3],
    /// Background transparency
    pub background_transparency: f32,
    /// Border color (RGB)
    pub border_color3: [f32; 3],
    /// Border size in pixels
    pub border_size_pixel: i32,
    /// Border mode
    pub border_mode: BorderMode,
    /// Z-index for layering
    pub z_index: i32,
    /// Layout order
    pub layout_order: i32,
    /// Rotation in degrees
    pub rotation: f32,
    /// Anchor point (0-1)
    pub anchor_point: [f32; 2],
    /// Position (scale + offset)
    pub position_scale: [f32; 2],
    pub position_offset: [f32; 2],
    /// Size (scale + offset)
    pub size_scale: [f32; 2],
    pub size_offset: [f32; 2],
    
    // === Scrolling-specific properties ===
    
    /// Size of the scrollable canvas (absolute pixels)
    /// Content larger than frame size will be scrollable
    pub canvas_size: [f32; 2],
    /// Current canvas position (scroll offset)
    pub canvas_position: [f32; 2],
    /// Whether horizontal scrolling is enabled
    pub scroll_bar_enabled_x: bool,
    /// Whether vertical scrolling is enabled
    pub scroll_bar_enabled_y: bool,
    /// Scrollbar image color
    pub scroll_bar_image_color3: [f32; 3],
    /// Scrollbar image transparency
    pub scroll_bar_image_transparency: f32,
    /// Scrollbar thickness in pixels
    pub scroll_bar_thickness: i32,
    /// Whether to auto-hide scrollbars when not scrolling
    pub scrolling_enabled: bool,
    /// Scrollbar inset from edges (pixels)
    pub top_image: String,
    pub mid_image: String,
    pub bottom_image: String,
    /// Elastic scrolling behavior (bounce at edges)
    pub elastic_behavior: ElasticBehavior,
    /// Scroll direction constraint
    pub scroll_direction: ScrollDirection,
    /// Whether to show vertical scrollbar
    pub vertical_scroll_bar_inset: ScrollBarInset,
    /// Whether to show horizontal scrollbar
    pub horizontal_scroll_bar_inset: ScrollBarInset,
}

/// Elastic scrolling behavior at content boundaries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum ElasticBehavior {
    /// Content stops at boundaries
    #[default]
    Never,
    /// Content bounces at boundaries (mobile-style)
    Always,
    /// Bounce only when content is smaller than frame
    WhenScrollable,
}

/// Scroll direction constraint
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum ScrollDirection {
    /// Allow both X and Y scrolling
    #[default]
    XY,
    /// Only horizontal scrolling
    X,
    /// Only vertical scrolling
    Y,
}

/// Scrollbar inset mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum ScrollBarInset {
    /// No inset, scrollbar at edge
    #[default]
    None,
    /// Scrollbar inset by thickness of other scrollbar
    ScrollBar,
    /// Always inset
    Always,
}

impl Default for ScrollingFrame {
    fn default() -> Self {
        Self {
            visible: true,
            background_color3: [1.0, 1.0, 1.0],
            background_transparency: 0.0,
            border_color3: [0.1, 0.1, 0.1],
            border_size_pixel: 1,
            border_mode: BorderMode::Outline,
            z_index: 1,
            layout_order: 0,
            rotation: 0.0,
            anchor_point: [0.0, 0.0],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: [200.0, 200.0],
            // Scrolling defaults
            canvas_size: [0.0, 0.0], // 0 = auto-size to children
            canvas_position: [0.0, 0.0],
            scroll_bar_enabled_x: true,
            scroll_bar_enabled_y: true,
            scroll_bar_image_color3: [0.3, 0.3, 0.3],
            scroll_bar_image_transparency: 0.0,
            scroll_bar_thickness: 12,
            scrolling_enabled: true,
            top_image: String::new(),
            mid_image: String::new(),
            bottom_image: String::new(),
            elastic_behavior: ElasticBehavior::Never,
            scroll_direction: ScrollDirection::XY,
            vertical_scroll_bar_inset: ScrollBarInset::None,
            horizontal_scroll_bar_inset: ScrollBarInset::None,
        }
    }
}

// ============================================================================
// 17d-3. VideoFrame (Video Display in UI)
// ============================================================================

/// UI element to display video content (links to VideoAsset)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct VideoFrame {
    /// Whether the frame is visible
    pub visible: bool,
    /// Background color (RGB)
    pub background_color3: [f32; 3],
    /// Background transparency
    pub background_transparency: f32,
    /// Border color (RGB)
    pub border_color3: [f32; 3],
    /// Border size in pixels
    pub border_size_pixel: i32,
    /// Z-index for layering
    pub z_index: i32,
    /// Layout order
    pub layout_order: i32,
    /// Rotation in degrees
    pub rotation: f32,
    /// Anchor point (0-1)
    pub anchor_point: [f32; 2],
    /// Position (scale + offset)
    pub position_scale: [f32; 2],
    pub position_offset: [f32; 2],
    /// Size (scale + offset)
    pub size_scale: [f32; 2],
    pub size_offset: [f32; 2],
    
    // === Video-specific properties ===
    
    /// Asset reference - can be:
    /// - Asset ID: "asset://abc123"
    /// - Local path: "file:///path/to/video.mp4"
    /// - URL: "https://example.com/video.mp4"
    /// - Entity name: "entity://MyVideoAsset"
    pub video: String,
    /// Whether video plays automatically
    pub autoplay: bool,
    /// Whether video loops
    pub looping: bool,
    /// Volume (0.0 - 1.0)
    pub volume: f32,
    /// Playback speed multiplier
    pub playback_speed: f32,
    /// Current playback time in seconds (for seeking)
    pub time_position: f32,
    /// Whether video is currently playing
    pub playing: bool,
    /// Whether to show playback controls
    pub show_controls: bool,
    /// Scale type for video fitting
    pub scale_type: ScaleType,
    /// Video tint color
    pub video_color3: [f32; 3],
    /// Video transparency
    pub video_transparency: f32,
}

impl Default for VideoFrame {
    fn default() -> Self {
        Self {
            visible: true,
            background_color3: [0.0, 0.0, 0.0],
            background_transparency: 0.0,
            border_color3: [0.1, 0.1, 0.1],
            border_size_pixel: 0,
            z_index: 1,
            layout_order: 0,
            rotation: 0.0,
            anchor_point: [0.0, 0.0],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: [320.0, 180.0], // 16:9 default
            video: String::new(),
            autoplay: false,
            looping: false,
            volume: 1.0,
            playback_speed: 1.0,
            time_position: 0.0,
            playing: false,
            show_controls: true,
            scale_type: ScaleType::Fit,
            video_color3: [1.0, 1.0, 1.0],
            video_transparency: 0.0,
        }
    }
}

// ============================================================================
// 17d-4. DocumentFrame (Document Display in UI)
// ============================================================================

/// UI element to display document content (links to Document)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct DocumentFrame {
    /// Whether the frame is visible
    pub visible: bool,
    /// Background color (RGB)
    pub background_color3: [f32; 3],
    /// Background transparency
    pub background_transparency: f32,
    /// Border color (RGB)
    pub border_color3: [f32; 3],
    /// Border size in pixels
    pub border_size_pixel: i32,
    /// Z-index for layering
    pub z_index: i32,
    /// Layout order
    pub layout_order: i32,
    /// Rotation in degrees
    pub rotation: f32,
    /// Anchor point (0-1)
    pub anchor_point: [f32; 2],
    /// Position (scale + offset)
    pub position_scale: [f32; 2],
    pub position_offset: [f32; 2],
    /// Size (scale + offset)
    pub size_scale: [f32; 2],
    pub size_offset: [f32; 2],
    
    // === Document-specific properties ===
    
    /// Asset reference - can be:
    /// - Asset ID: "asset://abc123"
    /// - Local path: "file:///path/to/doc.pdf"
    /// - URL: "https://example.com/doc.pdf"
    /// - Entity name: "entity://MyDocument"
    pub document: String,
    /// Current page number (1-indexed)
    pub current_page: u32,
    /// Zoom level (1.0 = 100%)
    pub zoom: f32,
    /// Whether to show page navigation controls
    pub show_controls: bool,
    /// Whether to allow text selection
    pub selectable: bool,
    /// Scroll position within page (0-1)
    pub scroll_position: [f32; 2],
    /// Whether to enable scrolling
    pub scrolling_enabled: bool,
    /// Page display mode
    pub page_mode: PageDisplayMode,
    /// Document tint color
    pub document_color3: [f32; 3],
    /// Document transparency
    pub document_transparency: f32,
}

/// How pages are displayed in DocumentFrame
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum PageDisplayMode {
    /// Single page at a time
    #[default]
    SinglePage,
    /// Continuous vertical scroll
    Continuous,
    /// Two pages side by side (book view)
    TwoPage,
    /// Fit width to frame
    FitWidth,
    /// Fit entire page to frame
    FitPage,
}

impl Default for DocumentFrame {
    fn default() -> Self {
        Self {
            visible: true,
            background_color3: [1.0, 1.0, 1.0],
            background_transparency: 0.0,
            border_color3: [0.1, 0.1, 0.1],
            border_size_pixel: 1,
            z_index: 1,
            layout_order: 0,
            rotation: 0.0,
            anchor_point: [0.0, 0.0],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: [400.0, 500.0],
            document: String::new(),
            current_page: 1,
            zoom: 1.0,
            show_controls: true,
            selectable: true,
            scroll_position: [0.0, 0.0],
            scrolling_enabled: true,
            page_mode: PageDisplayMode::SinglePage,
            document_color3: [1.0, 1.0, 1.0],
            document_transparency: 0.0,
        }
    }
}

// ============================================================================
// 17d-5. WebFrame (Embedded Web Content in UI)
// ============================================================================

/// UI element to display embedded web content (like an iframe)
/// Can be used in ScreenGui for 2D overlays or SurfaceGui for 3D world displays
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct WebFrame {
    /// Whether the frame is visible
    pub visible: bool,
    /// Background color (RGB) - shown while loading
    pub background_color3: [f32; 3],
    /// Background transparency
    pub background_transparency: f32,
    /// Border color (RGB)
    pub border_color3: [f32; 3],
    /// Border size in pixels
    pub border_size_pixel: i32,
    /// Z-index for layering
    pub z_index: i32,
    /// Layout order
    pub layout_order: i32,
    /// Rotation in degrees
    pub rotation: f32,
    /// Anchor point (0-1)
    pub anchor_point: [f32; 2],
    /// Position (scale + offset)
    pub position_scale: [f32; 2],
    pub position_offset: [f32; 2],
    /// Size (scale + offset)
    pub size_scale: [f32; 2],
    pub size_offset: [f32; 2],
    
    // === Web-specific properties ===
    
    /// URL to display
    pub url: String,
    /// Whether the webview is interactive (mouse/keyboard input)
    pub interactive: bool,
    /// Whether to allow JavaScript execution
    pub javascript_enabled: bool,
    /// Whether to allow navigation away from initial URL
    pub navigation_enabled: bool,
    /// Zoom level (1.0 = 100%)
    pub zoom: f32,
    /// Whether to show scrollbars
    pub scrollbars_visible: bool,
    /// Whether content is currently loading
    pub loading: bool,
    /// Current page title (read-only, updated by webview)
    pub title: String,
    /// Whether to use transparent background
    pub transparent: bool,
    /// Resolution multiplier for rendering quality
    pub resolution_scale: f32,
    /// User agent string override (empty = default)
    pub user_agent: String,
}

impl Default for WebFrame {
    fn default() -> Self {
        Self {
            visible: true,
            background_color3: [0.1, 0.1, 0.1],
            background_transparency: 0.0,
            border_color3: [0.2, 0.2, 0.2],
            border_size_pixel: 1,
            z_index: 1,
            layout_order: 0,
            rotation: 0.0,
            anchor_point: [0.0, 0.0],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: [800.0, 600.0],
            url: String::from("about:blank"),
            interactive: true,
            javascript_enabled: true,
            navigation_enabled: true,
            zoom: 1.0,
            scrollbars_visible: true,
            loading: false,
            title: String::new(),
            transparent: false,
            resolution_scale: 1.0,
            user_agent: String::new(),
        }
    }
}

// ============================================================================
// 17e. ImageLabel (Image Display)
// ============================================================================

/// Displays an image in the UI
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ImageLabel {
    /// Whether the image is visible
    pub visible: bool,
    /// Asset ID or path to the image
    pub image: String,
    /// Image color tint (RGB)
    pub image_color3: [f32; 3],
    /// Image transparency
    pub image_transparency: f32,
    /// Scale type for image fitting
    pub scale_type: ScaleType,
    /// Slice center for 9-slice scaling (pixels from edges)
    pub slice_center: [f32; 4],  // left, top, right, bottom
    /// Slice scale multiplier
    pub slice_scale: f32,
    /// Tile size for tiled images
    pub tile_size: [f32; 2],
    /// Background color
    pub background_color3: [f32; 3],
    pub background_transparency: f32,
    /// Border
    pub border_color3: [f32; 3],
    pub border_size_pixel: i32,
    /// Layout
    pub z_index: i32,
    pub layout_order: i32,
    pub rotation: f32,
    pub anchor_point: [f32; 2],
    pub position_scale: [f32; 2],
    pub position_offset: [f32; 2],
    pub size_scale: [f32; 2],
    pub size_offset: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum ScaleType {
    #[default]
    Stretch,
    Slice,
    Tile,
    Fit,
    Crop,
}

impl Default for ImageLabel {
    fn default() -> Self {
        Self {
            visible: true,
            image: String::new(),
            image_color3: [1.0, 1.0, 1.0],
            image_transparency: 0.0,
            scale_type: ScaleType::Stretch,
            slice_center: [0.0, 0.0, 0.0, 0.0],
            slice_scale: 1.0,
            tile_size: [1.0, 1.0],
            background_color3: [1.0, 1.0, 1.0],
            background_transparency: 1.0,
            border_color3: [0.1, 0.1, 0.1],
            border_size_pixel: 0,
            z_index: 1,
            layout_order: 0,
            rotation: 0.0,
            anchor_point: [0.0, 0.0],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: [100.0, 100.0],
        }
    }
}

// ============================================================================
// 17f. TextButton (Clickable Text)
// ============================================================================

/// Clickable text button
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct TextButton {
    /// Whether the button is visible
    pub visible: bool,
    /// Whether the button is active (can be clicked)
    pub active: bool,
    /// Whether the button auto-sizes to text
    pub auto_button_color: bool,
    /// Text content
    pub text: String,
    /// Font size
    pub font_size: f32,
    /// Text color
    pub text_color3: [f32; 3],
    pub text_transparency: f32,
    /// Text stroke
    pub text_stroke_color3: [f32; 3],
    pub text_stroke_transparency: f32,
    /// Text alignment
    pub text_x_alignment: TextXAlignment,
    pub text_y_alignment: TextYAlignment,
    /// Background
    pub background_color3: [f32; 3],
    pub background_transparency: f32,
    /// Border
    pub border_color3: [f32; 3],
    pub border_size_pixel: i32,
    /// Layout
    pub z_index: i32,
    pub layout_order: i32,
    pub rotation: f32,
    pub anchor_point: [f32; 2],
    pub position_scale: [f32; 2],
    pub position_offset: [f32; 2],
    pub size_scale: [f32; 2],
    pub size_offset: [f32; 2],
}

impl Default for TextButton {
    fn default() -> Self {
        Self {
            visible: true,
            active: true,
            auto_button_color: true,
            text: "Button".to_string(),
            font_size: 14.0,
            text_color3: [0.1, 0.1, 0.1],
            text_transparency: 0.0,
            text_stroke_color3: [0.0, 0.0, 0.0],
            text_stroke_transparency: 1.0,
            text_x_alignment: TextXAlignment::Center,
            text_y_alignment: TextYAlignment::Center,
            background_color3: [0.8, 0.8, 0.8],
            background_transparency: 0.0,
            border_color3: [0.1, 0.1, 0.1],
            border_size_pixel: 1,
            z_index: 1,
            layout_order: 0,
            rotation: 0.0,
            anchor_point: [0.0, 0.0],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: [200.0, 50.0],
        }
    }
}

// ============================================================================
// 17g. ImageButton (Clickable Image)
// ============================================================================

/// Clickable image button
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ImageButton {
    /// Whether the button is visible
    pub visible: bool,
    /// Whether the button is active (can be clicked)
    pub active: bool,
    /// Whether to auto-adjust color on hover/press
    pub auto_button_color: bool,
    /// Image asset ID or path
    pub image: String,
    /// Hover image
    pub hover_image: String,
    /// Pressed image
    pub pressed_image: String,
    /// Image color tint
    pub image_color3: [f32; 3],
    pub image_transparency: f32,
    /// Scale type
    pub scale_type: ScaleType,
    /// Background
    pub background_color3: [f32; 3],
    pub background_transparency: f32,
    /// Border
    pub border_color3: [f32; 3],
    pub border_size_pixel: i32,
    /// Layout
    pub z_index: i32,
    pub layout_order: i32,
    pub rotation: f32,
    pub anchor_point: [f32; 2],
    pub position_scale: [f32; 2],
    pub position_offset: [f32; 2],
    pub size_scale: [f32; 2],
    pub size_offset: [f32; 2],
}

impl Default for ImageButton {
    fn default() -> Self {
        Self {
            visible: true,
            active: true,
            auto_button_color: true,
            image: String::new(),
            hover_image: String::new(),
            pressed_image: String::new(),
            image_color3: [1.0, 1.0, 1.0],
            image_transparency: 0.0,
            scale_type: ScaleType::Stretch,
            background_color3: [1.0, 1.0, 1.0],
            background_transparency: 1.0,
            border_color3: [0.1, 0.1, 0.1],
            border_size_pixel: 0,
            z_index: 1,
            layout_order: 0,
            rotation: 0.0,
            anchor_point: [0.0, 0.0],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: [100.0, 100.0],
        }
    }
}

// ============================================================================
// 17e. TextBox (Text Input Field)
// ============================================================================

/// Text input field for user text entry
/// Bevy: Custom text input (Bevy has no native text input)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct TextBox {
    /// Whether the text box is visible
    pub visible: bool,
    /// Current text content
    pub text: String,
    /// Placeholder text when empty
    pub placeholder_text: String,
    /// Whether the text box is focused
    pub focused: bool,
    /// Whether text can be edited
    pub text_editable: bool,
    /// Whether to clear text on focus
    pub clear_text_on_focus: bool,
    /// Whether this is a multi-line text box
    pub multi_line: bool,
    /// Maximum text length (-1 = unlimited)
    pub max_length: i32,
    
    // Font
    pub font_size: f32,
    
    // Colors
    pub text_color3: [f32; 3],
    pub text_transparency: f32,
    pub placeholder_color3: [f32; 3],
    pub background_color3: [f32; 3],
    pub background_transparency: f32,
    pub border_color3: [f32; 3],
    
    // Layout
    pub position_scale: [f32; 2],
    pub position_offset: [f32; 2],
    pub size_scale: [f32; 2],
    pub size_offset: [f32; 2],
    pub anchor_point: [f32; 2],
    pub z_index: i32,
    pub border_size_pixel: i32,
}

impl Default for TextBox {
    fn default() -> Self {
        Self {
            visible: true,
            text: String::new(),
            placeholder_text: "Enter text...".to_string(),
            focused: false,
            text_editable: true,
            clear_text_on_focus: false,
            multi_line: false,
            max_length: -1,
            font_size: 14.0,
            text_color3: [0.0, 0.0, 0.0],
            text_transparency: 0.0,
            placeholder_color3: [0.5, 0.5, 0.5],
            background_color3: [1.0, 1.0, 1.0],
            background_transparency: 0.0,
            border_color3: [0.3, 0.3, 0.3],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: [200.0, 30.0],
            anchor_point: [0.0, 0.0],
            z_index: 1,
            border_size_pixel: 1,
        }
    }
}

// ============================================================================
// 17f. ViewportFrame (3D Viewport in UI)
// ============================================================================

/// 3D viewport rendered within UI (for inventory previews, minimaps, etc.)
/// Bevy: ViewportNode (direct mapping!)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ViewportFrame {
    /// Whether the viewport is visible
    pub visible: bool,
    /// Background color (RGB)
    pub background_color3: [f32; 3],
    /// Background transparency
    pub background_transparency: f32,
    /// Z-index for layering
    pub z_index: i32,
    /// Layout order
    pub layout_order: i32,
    /// Anchor point (0-1)
    pub anchor_point: [f32; 2],
    /// Position (scale + offset)
    pub position_scale: [f32; 2],
    pub position_offset: [f32; 2],
    /// Size (scale + offset)
    pub size_scale: [f32; 2],
    pub size_offset: [f32; 2],
    
    // Viewport-specific properties
    /// Camera entity to render from (if None, uses default)
    pub current_camera: Option<u64>,
    /// Whether to use ambient lighting
    pub ambient: bool,
    /// Light color for viewport
    pub light_color: [f32; 3],
    /// Light direction
    pub light_direction: [f32; 3],
    /// Image transparency
    pub image_transparency: f32,
}

impl Default for ViewportFrame {
    fn default() -> Self {
        Self {
            visible: true,
            background_color3: [0.1, 0.1, 0.1],
            background_transparency: 0.0,
            z_index: 1,
            layout_order: 0,
            anchor_point: [0.0, 0.0],
            position_scale: [0.0, 0.0],
            position_offset: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            size_offset: [200.0, 200.0],
            current_camera: None,
            ambient: true,
            light_color: [1.0, 1.0, 1.0],
            light_direction: [0.0, -1.0, 0.0],
            image_transparency: 0.0,
        }
    }
}

// ============================================================================
// 18. TextLabel (Static Text Display)
// ============================================================================

/// Text display element (2D or 3D via BillboardGui/SurfaceGui)
/// Bevy: bevy_text::TextBundle or egui
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct TextLabel {
    // Text Content
    pub text: String,
    pub rich_text: bool,
    pub text_scaled: bool,
    pub text_wrapped: bool,
    pub max_visible_graphemes: i32,  // -1 = unlimited
    
    // Font
    pub font: Font,
    pub font_size: f32,
    pub line_height: f32,
    
    // Colors
    pub text_color3: [f32; 3],
    pub text_transparency: f32,
    pub text_stroke_color3: [f32; 3],
    pub text_stroke_transparency: f32,
    pub background_color3: [f32; 3],
    pub background_transparency: f32,
    pub border_color3: [f32; 3],
    
    // Alignment
    pub text_x_alignment: TextXAlignment,
    pub text_y_alignment: TextYAlignment,
    
    // Layout
    pub position: [f32; 2],  // UDim2 simplified
    pub size: [f32; 2],
    pub anchor_point: [f32; 2],  // 0-1
    pub rotation: f32,  // Degrees
    pub z_index: i32,
    
    // Behavior
    pub active: bool,
    pub visible: bool,
    pub clips_descendants: bool,
    pub border_size_pixel: i32,
    
    // Auto-sizing
    pub automatic_size: AutomaticSize,
    
    // Runtime (read-only)
    pub text_fits: bool,
    pub text_bounds: [f32; 2],
    pub absolute_content_size: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum TextXAlignment {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum TextYAlignment {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum AutomaticSize {
    None,
    X,
    Y,
    XY,
}

/// Font families supported by TextLabel (Bevy UI compatible)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum Font {
    /// Default sans-serif font (FiraSans)
    #[default]
    SourceSans,
    /// Monospace font for code
    RobotoMono,
    /// Bold variant
    GothamBold,
    /// Light/thin variant
    GothamLight,
    /// Fantasy/decorative font
    Fantasy,
    /// Handwriting style
    Bangers,
    /// Classic serif font
    Merriweather,
    /// Modern geometric sans
    Nunito,
    /// Condensed font
    Ubuntu,
}

impl Default for TextLabel {
    fn default() -> Self {
        Self {
            text: String::new(),
            rich_text: false,
            text_scaled: false,
            text_wrapped: false,
            max_visible_graphemes: -1,
            
            font: Font::default(),
            font_size: 14.0,
            line_height: 1.0,
            
            text_color3: [0.0, 0.0, 0.0],  // Black text
            text_transparency: 0.0,
            text_stroke_color3: [0.0, 0.0, 0.0],
            text_stroke_transparency: 1.0,  // No stroke
            background_color3: [1.0, 1.0, 1.0],  // White background
            background_transparency: 1.0,  // Transparent
            border_color3: [0.165, 0.165, 0.165],
            
            text_x_alignment: TextXAlignment::Center,
            text_y_alignment: TextYAlignment::Center,
            
            position: [0.0, 0.0],
            size: [1.0, 1.0],
            anchor_point: [0.0, 0.0],
            rotation: 0.0,
            z_index: 1,
            
            active: true,
            visible: true,
            clips_descendants: false,
            border_size_pixel: 1,
            
            automatic_size: AutomaticSize::None,
            
            text_fits: true,
            text_bounds: [0.0, 0.0],
            absolute_content_size: [0.0, 0.0],
        }
    }
}

// ============================================================================
// 19. Animator (Plays Animations on Humanoid/Model)
// ============================================================================

/// Applies KeyframeSequences to rigs
/// Bevy: bevy::animation::AnimationPlayer + GLTF skeleton
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Animator {
    /// Playback speed multiplier (Eustress "PreferredAnimationSpeed")
    /// Bevy: AnimationPlayer.playback_speed
    pub preferred_animation_speed: f32,
    
    /// Skeleton type (Eustress implicit from rig)
    pub rig_type: RigType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum RigType {
    Humanoid,
    R15,
    R6,
    Custom,
}

impl Default for Animator {
    fn default() -> Self {
        Self {
            preferred_animation_speed: 1.0,
            rig_type: RigType::R15,
        }
    }
}

// ============================================================================
// 18. KeyframeSequence (Animation Asset: Sequence of Poses)
// ============================================================================

/// Stores poses over time
/// Bevy: Handle<AnimationClip>
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct KeyframeSequence {
    /// Loop behavior (Eustress "Loop")
    /// Bevy: AnimationClip.loop_mode
    pub looped: bool,
    
    /// Layer blending priority (Eustress "Priority")
    pub priority: AnimationPriority,
    
    /// Keyframe data (Eustress implicit)
    /// Bevy: AnimationClip keyframes
    pub keyframes: Vec<Keyframe>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum AnimationPriority {
    Core,
    Idle,
    Movement,
    Action,
}

#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct Keyframe {
    pub time: f32,
    pub pose: Transform,
    pub easing: EasingStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum EasingStyle {
    Linear,
    Constant,
    Cubic,
    Elastic,
}

impl Default for KeyframeSequence {
    fn default() -> Self {
        Self {
            looped: false,
            priority: AnimationPriority::Core,
            keyframes: Vec::new(),
        }
    }
}

// ============================================================================
// 19. ParticleEmitter (2D/3D Particles)
// ============================================================================

/// Attached to Attachment/BasePart for effects (fire/smoke/sparks/magic)
/// Bevy: Custom particle system with physics simulation
/// 
/// # AAA Quality Features
/// - Physically simulated particles with collisions and workspace gravity
/// - Custom image assets for particle textures
/// - Color gradients over lifetime
/// - Direction, spread, speed, rate controls
/// - Size over lifetime curves
/// - Rotation and angular velocity
/// - Drag and acceleration
/// - Emission shapes (point, sphere, box, cone)
/// - Flipbook animation support
/// - Light emission per particle
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct ParticleEmitter {
    // ═══════════════════════════════════════════════════════════════════════════
    // EMISSION PROPERTIES
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Active state (Eustress "Enabled")
    pub enabled: bool,
    
    /// Emission rate - particles per second (Eustress "Rate")
    pub rate: f32,
    
    /// Maximum particles alive at once (prevents performance issues)
    pub max_particles: u32,
    
    /// Emit in bursts instead of continuous stream
    pub burst_mode: bool,
    
    /// Particles per burst (when burst_mode is true)
    pub burst_count: u32,
    
    /// Time between bursts in seconds
    pub burst_interval: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // EMISSION SHAPE
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Shape of emission volume
    pub emission_shape: EmissionShape,
    
    /// Size of emission shape (radius for sphere/cone, half-extents for box)
    pub emission_size: Vec3,
    
    /// Emit from surface only (hollow shapes)
    pub surface_only: bool,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // LIFETIME & TIMING
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Particle lifetime range in seconds (min, max)
    pub lifetime: (f32, f32),
    
    /// Delay before emitter starts (seconds)
    pub emit_delay: f32,
    
    /// Duration of emission (0 = infinite)
    pub emit_duration: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // MOVEMENT & PHYSICS
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Initial speed range (min, max) in studs/second
    pub speed: (f32, f32),
    
    /// Spread angle in degrees (X = horizontal, Y = vertical)
    pub spread_angle: Vec2,
    
    /// Emission direction (local space, normalized)
    pub direction: Vec3,
    
    /// Acceleration applied each frame (local space)
    pub acceleration: Vec3,
    
    /// Use workspace gravity (multiplied by gravity_scale)
    pub use_gravity: bool,
    
    /// Gravity multiplier (1.0 = normal, 0.5 = half, -1.0 = reverse)
    pub gravity_scale: f32,
    
    /// Air resistance / drag coefficient (0 = none, 1 = heavy)
    pub drag: f32,
    
    /// Velocity inheritance from parent (0-1)
    pub velocity_inheritance: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // COLLISION
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Enable particle-world collision
    pub collision_enabled: bool,
    
    /// Bounciness on collision (0 = stick, 1 = perfect bounce)
    pub collision_bounce: f32,
    
    /// Friction on collision (0 = slide, 1 = stop)
    pub collision_friction: f32,
    
    /// Kill particle on collision
    pub collision_kill: bool,
    
    /// Collision radius multiplier (relative to particle size)
    pub collision_radius: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // APPEARANCE - TEXTURE
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Texture asset ID (Eustress "Texture") - custom image asset
    pub texture: String,
    
    /// Flipbook animation columns (for sprite sheets)
    pub flipbook_columns: u32,
    
    /// Flipbook animation rows
    pub flipbook_rows: u32,
    
    /// Flipbook frames per second
    pub flipbook_fps: f32,
    
    /// Randomize flipbook start frame
    pub flipbook_random_start: bool,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // APPEARANCE - SIZE
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Initial size range (min, max) in studs
    pub size: (f32, f32),
    
    /// Size over lifetime curve (time 0-1, scale multiplier)
    pub size_curve: Vec<(f32, f32)>,
    
    /// Uniform scaling (true) or stretch in velocity direction (false)
    pub uniform_size: bool,
    
    /// Stretch amount when non-uniform (velocity-based)
    pub stretch_factor: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // APPEARANCE - COLOR
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Color over lifetime (time 0-1, color with alpha)
    pub color_sequence: Vec<(f32, Color)>,
    
    /// Transparency over lifetime (time 0-1, alpha 0-1)
    pub transparency_curve: Vec<(f32, f32)>,
    
    /// Blend mode for rendering
    pub blend_mode: ParticleBlendMode,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // APPEARANCE - ROTATION
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Initial rotation range in degrees (min, max)
    pub rotation: (f32, f32),
    
    /// Angular velocity range in degrees/second (min, max)
    pub rotation_speed: (f32, f32),
    
    /// Face camera (billboard) or face velocity direction
    pub face_camera: bool,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // LIGHTING
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Emit light from particles
    pub light_emission: bool,
    
    /// Light range per particle (studs)
    pub light_range: f32,
    
    /// Light brightness multiplier
    pub light_brightness: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // ADVANCED
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Wind influence (0-1)
    pub wind_influence: f32,
    
    /// Noise-based movement amplitude
    pub noise_strength: f32,
    
    /// Noise frequency
    pub noise_frequency: f32,
    
    /// Sort particles by distance (for proper alpha blending)
    pub depth_sort: bool,
    
    /// Render in local space (move with parent) vs world space
    pub local_space: bool,
}

/// Emission shape for particle spawning
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum EmissionShape {
    /// Emit from a single point
    #[default]
    Point,
    /// Emit from within a sphere
    Sphere,
    /// Emit from within a box
    Box,
    /// Emit from a cone (direction + angle)
    Cone,
    /// Emit from a cylinder
    Cylinder,
    /// Emit from a ring/torus
    Ring,
    /// Emit from a disc
    Disc,
}

/// Blend mode for particle rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum ParticleBlendMode {
    /// Standard alpha blending
    #[default]
    Alpha,
    /// Additive (glow effects)
    Additive,
    /// Multiply (shadows)
    Multiply,
    /// Premultiplied alpha
    Premultiplied,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            // Emission
            enabled: true,
            rate: 20.0,
            max_particles: 500,
            burst_mode: false,
            burst_count: 10,
            burst_interval: 1.0,
            
            // Emission shape
            emission_shape: EmissionShape::Point,
            emission_size: Vec3::ZERO,
            surface_only: false,
            
            // Lifetime
            lifetime: (2.0, 4.0),
            emit_delay: 0.0,
            emit_duration: 0.0,
            
            // Movement
            speed: (5.0, 10.0),
            spread_angle: Vec2::new(15.0, 15.0),
            direction: Vec3::Y,
            acceleration: Vec3::ZERO,
            use_gravity: false,
            gravity_scale: 1.0,
            drag: 0.0,
            velocity_inheritance: 0.0,
            
            // Collision
            collision_enabled: false,
            collision_bounce: 0.3,
            collision_friction: 0.5,
            collision_kill: false,
            collision_radius: 1.0,
            
            // Texture
            texture: String::new(),
            flipbook_columns: 1,
            flipbook_rows: 1,
            flipbook_fps: 0.0,
            flipbook_random_start: false,
            
            // Size
            size: (0.5, 1.0),
            size_curve: vec![(0.0, 1.0), (1.0, 0.0)],
            uniform_size: true,
            stretch_factor: 1.0,
            
            // Color
            color_sequence: vec![
                (0.0, Color::srgba(1.0, 1.0, 1.0, 1.0)),
                (1.0, Color::srgba(1.0, 1.0, 1.0, 0.0)),
            ],
            transparency_curve: vec![(0.0, 0.0), (0.8, 0.0), (1.0, 1.0)],
            blend_mode: ParticleBlendMode::Alpha,
            
            // Rotation
            rotation: (0.0, 360.0),
            rotation_speed: (-45.0, 45.0),
            face_camera: true,
            
            // Lighting
            light_emission: false,
            light_range: 4.0,
            light_brightness: 1.0,
            
            // Advanced
            wind_influence: 0.0,
            noise_strength: 0.0,
            noise_frequency: 1.0,
            depth_sort: true,
            local_space: false,
        }
    }
}

// ============================================================================
// 20. Beam (Curved Line Effect) - AAA Quality
// ============================================================================

/// Beam blend mode for rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum BeamBlendMode {
    /// Standard alpha blending
    #[default]
    Alpha,
    /// Additive (glow effects)
    Additive,
    /// Multiply
    Multiply,
}

/// Beam face mode - how the beam orients to camera
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum BeamFaceMode {
    /// Always face the camera (billboard)
    #[default]
    FaceCamera,
    /// Face camera but only rotate on Y axis
    FaceCameraY,
    /// Fixed orientation in world space
    Fixed,
}

/// Connects two Attachments with a curved, textured beam
/// Bevy: Procedural Mesh with custom beam shader
/// 
/// # AAA Features
/// - Bezier curve interpolation with configurable segments
/// - Width tapering from start to end
/// - Color gradient over length
/// - Transparency gradient over length
/// - Texture animation (scrolling, flipbook)
/// - Light emission
/// - Soft particle blending at intersections
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Beam {
    // ═══════════════════════════════════════════════════════════════════════════
    // ATTACHMENT POINTS
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Start attachment entity ID (Eustress "Attachment0")
    pub attachment0: Option<u32>,
    
    /// End attachment entity ID (Eustress "Attachment1")
    pub attachment1: Option<u32>,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // CURVE SHAPE
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Bezier control point offset at start (studs, Eustress "CurveSize0")
    /// Positive = curve outward from attachment0's forward direction
    pub curve_size0: f32,
    
    /// Bezier control point offset at end (studs, Eustress "CurveSize1")
    pub curve_size1: f32,
    
    /// Number of segments for curve interpolation (Eustress "Segments")
    /// Higher = smoother curve, more vertices
    pub segments: u32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // WIDTH
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Width at start in studs (Eustress "Width0")
    pub width0: f32,
    
    /// Width at end in studs (Eustress "Width1")
    pub width1: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // COLOR & TRANSPARENCY
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Color sequence over beam length (position 0-1, color)
    /// Interpolates between keyframes
    pub color_sequence: Vec<(f32, Color)>,
    
    /// Transparency sequence over beam length (position 0-1, transparency 0-1)
    pub transparency_sequence: Vec<(f32, f32)>,
    
    /// Overall brightness multiplier
    pub brightness: f32,
    
    /// Light emission intensity (0 = no emission)
    pub light_emission: f32,
    
    /// Light emission color (if different from beam color)
    pub light_color: Option<[f32; 4]>,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // TEXTURE
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Texture asset ID (Eustress "Texture")
    pub texture: String,
    
    /// Texture length in studs (how much beam length = 1 texture repeat)
    pub texture_length: f32,
    
    /// Texture scroll speed (studs per second, Eustress "TextureSpeed")
    pub texture_speed: f32,
    
    /// Texture mode (stretch, tile, static)
    pub texture_mode: TextureMode,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // RENDERING
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Blend mode for rendering
    pub blend_mode: BeamBlendMode,
    
    /// Face mode - how beam orients to camera
    pub face_mode: BeamFaceMode,
    
    /// Z-offset for layering multiple beams
    pub z_offset: f32,
    
    /// Whether beam is enabled/visible
    pub enabled: bool,
}

/// Texture mode for beam
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum TextureMode {
    /// Stretch texture over entire beam length
    Stretch,
    /// Tile texture based on texture_length
    #[default]
    Tile,
    /// Static texture (no scrolling)
    Static,
}

impl Default for Beam {
    fn default() -> Self {
        Self {
            attachment0: None,
            attachment1: None,
            curve_size0: 0.0,
            curve_size1: 0.0,
            segments: 10,
            width0: 1.0,
            width1: 1.0,
            color_sequence: vec![
                (0.0, Color::WHITE),
                (1.0, Color::WHITE),
            ],
            transparency_sequence: vec![
                (0.0, 0.0),
                (1.0, 0.0),
            ],
            brightness: 1.0,
            light_emission: 0.0,
            light_color: None,
            texture: String::new(),
            texture_length: 1.0,
            texture_speed: 0.0,
            texture_mode: TextureMode::Tile,
            blend_mode: BeamBlendMode::Alpha,
            face_mode: BeamFaceMode::FaceCamera,
            z_offset: 0.0,
            enabled: true,
        }
    }
}

impl Beam {
    /// Create a simple beam with uniform color
    pub fn simple(color: Color, width: f32) -> Self {
        Self {
            width0: width,
            width1: width,
            color_sequence: vec![(0.0, color), (1.0, color)],
            ..Default::default()
        }
    }
    
    /// Create a laser beam (additive, glowing)
    pub fn laser(color: Color) -> Self {
        Self {
            width0: 0.2,
            width1: 0.2,
            color_sequence: vec![(0.0, color), (1.0, color)],
            blend_mode: BeamBlendMode::Additive,
            light_emission: 2.0,
            brightness: 2.0,
            ..Default::default()
        }
    }
    
    /// Create a lightning beam (jagged, bright)
    pub fn lightning() -> Self {
        Self {
            width0: 0.5,
            width1: 0.1,
            color_sequence: vec![
                (0.0, Color::srgba(0.8, 0.9, 1.0, 1.0)),
                (0.5, Color::srgba(1.0, 1.0, 1.0, 1.0)),
                (1.0, Color::srgba(0.6, 0.7, 1.0, 0.5)),
            ],
            blend_mode: BeamBlendMode::Additive,
            light_emission: 5.0,
            segments: 20,
            ..Default::default()
        }
    }
    
    /// Create a rope/chain beam
    pub fn rope() -> Self {
        Self {
            width0: 0.3,
            width1: 0.3,
            color_sequence: vec![(0.0, Color::srgb(0.4, 0.3, 0.2)), (1.0, Color::srgb(0.4, 0.3, 0.2))],
            curve_size0: 2.0,
            curve_size1: 2.0,
            segments: 15,
            ..Default::default()
        }
    }
    
    /// Create a trail effect beam (fades out)
    pub fn trail(color: Color) -> Self {
        Self {
            width0: 1.0,
            width1: 0.0,
            color_sequence: vec![(0.0, color), (1.0, color)],
            transparency_sequence: vec![
                (0.0, 0.0),
                (0.7, 0.3),
                (1.0, 1.0),
            ],
            ..Default::default()
        }
    }
    
    /// Get interpolated width at position t (0-1)
    pub fn width_at(&self, t: f32) -> f32 {
        self.width0 + (self.width1 - self.width0) * t.clamp(0.0, 1.0)
    }
    
    /// Get interpolated color at position t (0-1)
    pub fn color_at(&self, t: f32) -> Color {
        if self.color_sequence.is_empty() {
            return Color::WHITE;
        }
        if self.color_sequence.len() == 1 {
            return self.color_sequence[0].1;
        }
        
        let t = t.clamp(0.0, 1.0);
        
        // Find surrounding keyframes
        for i in 0..self.color_sequence.len() - 1 {
            let (t0, c0) = &self.color_sequence[i];
            let (t1, c1) = &self.color_sequence[i + 1];
            
            if t >= *t0 && t <= *t1 {
                let local_t = (t - t0) / (t1 - t0);
                let c0_srgba = c0.to_srgba();
                let c1_srgba = c1.to_srgba();
                return Color::srgba(
                    c0_srgba.red + (c1_srgba.red - c0_srgba.red) * local_t,
                    c0_srgba.green + (c1_srgba.green - c0_srgba.green) * local_t,
                    c0_srgba.blue + (c1_srgba.blue - c0_srgba.blue) * local_t,
                    c0_srgba.alpha + (c1_srgba.alpha - c0_srgba.alpha) * local_t,
                );
            }
        }
        
        self.color_sequence.last().map(|(_, c)| *c).unwrap_or(Color::WHITE)
    }
    
    /// Get interpolated transparency at position t (0-1)
    pub fn transparency_at(&self, t: f32) -> f32 {
        if self.transparency_sequence.is_empty() {
            return 0.0;
        }
        if self.transparency_sequence.len() == 1 {
            return self.transparency_sequence[0].1;
        }
        
        let t = t.clamp(0.0, 1.0);
        
        for i in 0..self.transparency_sequence.len() - 1 {
            let (t0, a0) = self.transparency_sequence[i];
            let (t1, a1) = self.transparency_sequence[i + 1];
            
            if t >= t0 && t <= t1 {
                let local_t = (t - t0) / (t1 - t0);
                return a0 + (a1 - a0) * local_t;
            }
        }
        
        self.transparency_sequence.last().map(|(_, a)| *a).unwrap_or(0.0)
    }
}

// ============================================================================
// 21. Sound (Audio Playback) - AAA Quality
// ============================================================================

/// Sound rolloff model for 3D spatial audio
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum SoundRolloffMode {
    /// Linear falloff (simple, predictable)
    Linear,
    /// Inverse distance (realistic, 1/r)
    #[default]
    Inverse,
    /// Inverse distance squared (more realistic, 1/r²)
    InverseSquared,
    /// Logarithmic falloff (natural sounding)
    Logarithmic,
    /// No falloff (constant volume regardless of distance)
    None,
    /// Custom curve (use rolloff_curve)
    Custom,
}

/// Sound group for mixing and ducking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum SoundGroup {
    /// Master group (affects all sounds)
    Master,
    /// Sound effects (footsteps, impacts, etc.)
    #[default]
    SFX,
    /// Music and ambient tracks
    Music,
    /// Voice and dialogue
    Voice,
    /// Ambient environmental sounds
    Ambient,
    /// UI sounds (clicks, notifications)
    UI,
}

/// 3D positional audio component - AAA quality spatial sound
/// Bevy: bevy::audio::AudioBundle with SpatialAudioSink
/// 
/// # AAA Features
/// - 3D spatial positioning with configurable rolloff
/// - Distance-based volume attenuation
/// - Doppler effect support
/// - Sound groups for mixing
/// - Fade in/out with easing
/// - Reverb and effects sends
/// - Playback position seeking
/// - Event callbacks (on_end, on_loop)
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Sound {
    // ═══════════════════════════════════════════════════════════════════════════
    // AUDIO SOURCE
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Audio asset ID or path (Eustress "SoundId")
    /// Supports: asset://id, file:///path, https://url
    pub sound_id: String,
    
    /// Sound group for mixing
    pub sound_group: SoundGroup,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // PLAYBACK
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Whether sound is currently playing (Eustress "Playing")
    pub playing: bool,
    
    /// Whether sound loops (Eustress "Looped")
    pub looped: bool,
    
    /// Current playback position in seconds (Eustress "TimePosition")
    pub time_position: f32,
    
    /// Total duration in seconds (read-only, set by audio system)
    pub time_length: f32,
    
    /// Playback speed multiplier (1.0 = normal)
    pub playback_speed: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // VOLUME & PITCH
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Base volume (0.0 - 1.0, Eustress "Volume")
    pub volume: f32,
    
    /// Pitch multiplier (0.5 - 2.0, Eustress "PlaybackSpeed")
    /// Affects both pitch and playback speed
    pub pitch: f32,
    
    /// Whether pitch affects playback speed (true) or just pitch (false)
    /// False requires pitch-shifting which is more expensive
    pub pitch_affects_speed: bool,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // 3D SPATIAL AUDIO
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Enable 3D spatial audio
    pub spatial: bool,
    
    /// Minimum distance before rolloff starts (studs, Eustress "RollOffMinDistance")
    /// Sound is at full volume within this radius
    pub roll_off_min_distance: f32,
    
    /// Maximum distance for rolloff (studs, Eustress "RollOffMaxDistance")
    /// Sound is inaudible beyond this distance
    pub roll_off_max_distance: f32,
    
    /// Rolloff model (how volume decreases with distance)
    pub roll_off_mode: SoundRolloffMode,
    
    /// Custom rolloff curve (distance 0-1, volume 0-1)
    /// Only used when roll_off_mode is Custom
    pub roll_off_curve: Vec<(f32, f32)>,
    
    /// Doppler effect scale (0 = off, 1 = realistic, >1 = exaggerated)
    pub doppler_scale: f32,
    
    /// Spread angle in degrees (0 = point source, 360 = omnidirectional)
    /// Affects stereo width at close range
    pub spread_angle: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // FADING
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Fade in duration in seconds (0 = instant)
    pub fade_in_time: f32,
    
    /// Fade out duration in seconds (0 = instant)
    pub fade_out_time: f32,
    
    /// Current fade state (0-1, managed by audio system)
    pub fade_progress: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // EFFECTS
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Reverb send level (0-1)
    pub reverb_send: f32,
    
    /// Low-pass filter cutoff frequency (Hz, 0 = off)
    /// Useful for muffled/underwater effects
    pub low_pass_cutoff: f32,
    
    /// High-pass filter cutoff frequency (Hz, 0 = off)
    pub high_pass_cutoff: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // BEHAVIOR
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Play on awake (auto-play when spawned)
    pub play_on_awake: bool,
    
    /// Destroy entity when sound finishes (for one-shot sounds)
    pub destroy_on_end: bool,
    
    /// Priority (higher = less likely to be culled when too many sounds)
    pub priority: i32,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            // Source
            sound_id: String::new(),
            sound_group: SoundGroup::SFX,
            
            // Playback
            playing: false,
            looped: false,
            time_position: 0.0,
            time_length: 0.0,
            playback_speed: 1.0,
            
            // Volume & Pitch
            volume: 0.5,
            pitch: 1.0,
            pitch_affects_speed: true,
            
            // Spatial
            spatial: true,
            roll_off_min_distance: 10.0,
            roll_off_max_distance: 10000.0,
            roll_off_mode: SoundRolloffMode::Inverse,
            roll_off_curve: Vec::new(),
            doppler_scale: 1.0,
            spread_angle: 0.0,
            
            // Fading
            fade_in_time: 0.0,
            fade_out_time: 0.0,
            fade_progress: 1.0,
            
            // Effects
            reverb_send: 0.0,
            low_pass_cutoff: 0.0,
            high_pass_cutoff: 0.0,
            
            // Behavior
            play_on_awake: false,
            destroy_on_end: false,
            priority: 0,
        }
    }
}

impl Sound {
    /// Create a simple sound effect
    pub fn effect(sound_id: impl Into<String>) -> Self {
        Self {
            sound_id: sound_id.into(),
            sound_group: SoundGroup::SFX,
            ..Default::default()
        }
    }
    
    /// Create a looping music track
    pub fn music(sound_id: impl Into<String>) -> Self {
        Self {
            sound_id: sound_id.into(),
            sound_group: SoundGroup::Music,
            looped: true,
            spatial: false,
            volume: 0.7,
            fade_in_time: 1.0,
            fade_out_time: 1.0,
            ..Default::default()
        }
    }
    
    /// Create an ambient sound (looping, spatial)
    pub fn ambient(sound_id: impl Into<String>) -> Self {
        Self {
            sound_id: sound_id.into(),
            sound_group: SoundGroup::Ambient,
            looped: true,
            spatial: true,
            roll_off_min_distance: 5.0,
            roll_off_max_distance: 100.0,
            ..Default::default()
        }
    }
    
    /// Create a UI sound (non-spatial, high priority)
    pub fn ui(sound_id: impl Into<String>) -> Self {
        Self {
            sound_id: sound_id.into(),
            sound_group: SoundGroup::UI,
            spatial: false,
            priority: 100,
            ..Default::default()
        }
    }
    
    /// Create a voice/dialogue sound
    pub fn voice(sound_id: impl Into<String>) -> Self {
        Self {
            sound_id: sound_id.into(),
            sound_group: SoundGroup::Voice,
            spatial: true,
            priority: 50,
            roll_off_min_distance: 2.0,
            roll_off_max_distance: 50.0,
            ..Default::default()
        }
    }
    
    /// Play the sound
    pub fn play(&mut self) {
        self.playing = true;
        self.time_position = 0.0;
        self.fade_progress = if self.fade_in_time > 0.0 { 0.0 } else { 1.0 };
    }
    
    /// Stop the sound
    pub fn stop(&mut self) {
        self.playing = false;
        self.time_position = 0.0;
    }
    
    /// Pause the sound
    pub fn pause(&mut self) {
        self.playing = false;
    }
    
    /// Resume the sound
    pub fn resume(&mut self) {
        self.playing = true;
    }
    
    /// Calculate volume at given distance using rolloff model
    pub fn volume_at_distance(&self, distance: f32) -> f32 {
        if !self.spatial {
            return self.volume;
        }
        
        if distance <= self.roll_off_min_distance {
            return self.volume;
        }
        
        if distance >= self.roll_off_max_distance {
            return 0.0;
        }
        
        let normalized_distance = (distance - self.roll_off_min_distance) 
            / (self.roll_off_max_distance - self.roll_off_min_distance);
        
        let attenuation = match self.roll_off_mode {
            SoundRolloffMode::Linear => 1.0 - normalized_distance,
            SoundRolloffMode::Inverse => 1.0 / (1.0 + normalized_distance * 10.0),
            SoundRolloffMode::InverseSquared => 1.0 / (1.0 + normalized_distance * normalized_distance * 100.0),
            SoundRolloffMode::Logarithmic => (1.0 - normalized_distance.ln().max(0.0) / 10.0).max(0.0),
            SoundRolloffMode::None => 1.0,
            SoundRolloffMode::Custom => {
                // Interpolate custom curve
                if self.roll_off_curve.is_empty() {
                    1.0 - normalized_distance
                } else {
                    self.interpolate_curve(normalized_distance)
                }
            }
        };
        
        self.volume * attenuation * self.fade_progress
    }
    
    /// Interpolate custom rolloff curve
    fn interpolate_curve(&self, t: f32) -> f32 {
        if self.roll_off_curve.is_empty() {
            return 1.0;
        }
        if self.roll_off_curve.len() == 1 {
            return self.roll_off_curve[0].1;
        }
        
        for i in 0..self.roll_off_curve.len() - 1 {
            let (t0, v0) = self.roll_off_curve[i];
            let (t1, v1) = self.roll_off_curve[i + 1];
            
            if t >= t0 && t <= t1 {
                let local_t = (t - t0) / (t1 - t0);
                return v0 + (v1 - v0) * local_t;
            }
        }
        
        self.roll_off_curve.last().map(|(_, v)| *v).unwrap_or(0.0)
    }
    
    /// Get effective volume (base * fade)
    pub fn effective_volume(&self) -> f32 {
        self.volume * self.fade_progress
    }
}

// ============================================================================
// 22. Terrain (Voxel Grid)
// ============================================================================

/// Procedural landscape (Workspace child)
/// Bevy: Uses terrain module with LOD, heightmaps, and splat textures
/// See `eustress_common::terrain` for the full terrain system
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Terrain {
    /// Voxel materials with colors (Eustress "MaterialColors")
    pub material_colors: Vec<(String, Color)>,
    
    /// Water simulation params (Eustress "WaterWaveSize")
    pub water_wave_size: f32,
    
    /// Water transparency (Eustress "WaterTransparency")
    pub water_transparency: f32,
    
    /// Water color (Eustress "WaterColor")
    pub water_color: Color,
    
    // ========== New Terrain System Properties ==========
    
    /// Size of each chunk in world units
    pub chunk_size: f32,
    
    /// Resolution of each chunk (vertices per side)
    pub chunk_resolution: u32,
    
    /// Number of chunks in X direction (from center)
    pub chunks_x: u32,
    
    /// Number of chunks in Z direction (from center)
    pub chunks_z: u32,
    
    /// Number of LOD levels (0 = highest detail)
    pub lod_levels: u32,
    
    /// Maximum view distance for chunk culling
    pub view_distance: f32,
    
    /// Height scale multiplier
    pub height_scale: f32,
    
    /// Seed for procedural generation
    pub seed: u32,
    
    /// Path to heightmap image (optional)
    pub heightmap_path: Option<String>,
    
    /// Path to splatmap image (optional)
    pub splatmap_path: Option<String>,
}

impl Default for Terrain {
    fn default() -> Self {
        Self {
            material_colors: vec![
                ("Grass".to_string(), Color::srgb(0.35, 0.55, 0.25)),
                ("Rock".to_string(), Color::srgb(0.5, 0.5, 0.5)),
                ("Dirt".to_string(), Color::srgb(0.55, 0.4, 0.25)),
                ("Snow".to_string(), Color::srgb(0.95, 0.95, 0.98)),
            ],
            water_wave_size: 0.15,
            water_transparency: 0.3,
            water_color: Color::srgb(0.0, 0.3, 0.6),
            // New terrain defaults
            chunk_size: 64.0,
            chunk_resolution: 64,
            chunks_x: 4,
            chunks_z: 4,
            lod_levels: 4,
            view_distance: 1000.0,
            height_scale: 50.0,
            seed: 42,
            heightmap_path: None,
            splatmap_path: None,
        }
    }
}

impl Terrain {
    /// Create a small terrain for testing
    pub fn small() -> Self {
        Self {
            chunk_size: 32.0,
            chunk_resolution: 32,
            chunks_x: 2,
            chunks_z: 2,
            lod_levels: 4,
            view_distance: 1000.0,
            height_scale: 20.0,
            ..default()
        }
    }
    
    /// Create a large terrain for production
    pub fn large() -> Self {
        Self {
            chunk_size: 128.0,
            chunk_resolution: 128,
            chunks_x: 8,
            chunks_z: 8,
            lod_levels: 5,
            view_distance: 4000.0,
            height_scale: 100.0,
            ..default()
        }
    }
    
    /// Convert to TerrainConfig for the terrain system
    pub fn to_config(&self) -> crate::terrain::TerrainConfig {
        crate::terrain::TerrainConfig {
            chunk_size: self.chunk_size,
            chunk_resolution: self.chunk_resolution,
            chunks_x: self.chunks_x,
            chunks_z: self.chunks_z,
            lod_levels: self.lod_levels,
            lod_distances: (0..self.lod_levels)
                .map(|i| self.view_distance * (i as f32 + 1.0) / self.lod_levels as f32)
                .collect(),
            view_distance: self.view_distance,
            height_scale: self.height_scale,
            seed: self.seed,
        }
    }
}

// ============================================================================
// 23. Sky (Skybox)
// ============================================================================

/// Environment map (Lighting child)
/// Bevy: Skybox component with cubemap
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Sky {
    /// Skybox 6-face texture (Eustress "SkyboxBk/Ft/Lf/Rt/Up/Dn")
    /// Bevy: EnvironmentMapLight texture selector
    pub skybox_textures: SkyboxTextures,
    
    /// Star density (Eustress "StarCount")
    pub star_count: u32,
    
    /// Celestial bodies enabled (Eustress "CelestialBodiesShown")
    pub celestial_bodies_shown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct SkyboxTextures {
    pub back: String,
    pub front: String,
    pub left: String,
    pub right: String,
    pub up: String,
    pub down: String,
}

impl Default for Sky {
    fn default() -> Self {
        Self {
            skybox_textures: SkyboxTextures {
                back: String::new(),
                front: String::new(),
                left: String::new(),
                right: String::new(),
                up: String::new(),
                down: String::new(),
            },
            star_count: 3000,
            celestial_bodies_shown: true,
        }
    }
}

// ============================================================================
// 23b. Atmosphere (Lighting child)
// ============================================================================

/// Atmospheric effects component (Lighting child)
/// Controls fog, haze, sky color, and atmospheric scattering
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Atmosphere {
    /// Density of the atmosphere (0.0 - 1.0)
    /// Higher values create thicker, hazier atmosphere
    pub density: f32,
    
    /// Offset for density calculation
    pub offset: f32,
    
    /// Primary atmosphere color (tints the sky)
    pub color: [f32; 4],
    
    /// Decay/haze color at the horizon
    pub decay: [f32; 4],
    
    /// Glare intensity around the sun (0.0 - 1.0)
    pub glare: f32,
    
    /// Haze amount (0.0 - 1.0)
    pub haze: f32,
}

impl Default for Atmosphere {
    fn default() -> Self {
        Self {
            // Bevy Earth-like atmosphere defaults
            density: 0.35,                        // Moderate density for clear sky
            offset: 0.0,
            color: [0.4, 0.6, 1.0, 1.0],          // Blue sky (Rayleigh scattering)
            decay: [0.3, 0.3, 0.3, 1.0],          // Neutral ground albedo
            glare: 0.0,
            haze: 0.0,
        }
    }
}

impl Atmosphere {
    /// Create a clear day atmosphere
    pub fn clear_day() -> Self {
        Self {
            density: 0.3,
            haze: 0.0,
            glare: 0.0,
            color: [0.5, 0.7, 1.0, 1.0],
            decay: [0.9, 0.85, 0.8, 1.0],
            ..Default::default()
        }
    }
    
    /// Create a sunset atmosphere
    pub fn sunset() -> Self {
        Self {
            density: 0.5,
            haze: 0.2,
            glare: 0.3,
            color: [1.0, 0.6, 0.3, 1.0],
            decay: [0.8, 0.3, 0.1, 1.0],
            ..Default::default()
        }
    }
    
    /// Create a foggy atmosphere
    pub fn foggy() -> Self {
        Self {
            density: 0.8,
            haze: 0.7,
            glare: 0.0,
            color: [0.7, 0.7, 0.75, 1.0],
            decay: [0.6, 0.6, 0.65, 1.0],
            ..Default::default()
        }
    }
}

// ============================================================================
// 23c. Clouds (Lighting child - Volumetric Cloud System)
// ============================================================================

/// Cloud coverage hemisphere mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum CloudCoverage {
    /// Clouds cover entire sky uniformly
    #[default]
    Full,
    /// Clouds concentrated in northern hemisphere
    Northern,
    /// Clouds concentrated in southern hemisphere  
    Southern,
    /// Clouds concentrated in eastern hemisphere
    Eastern,
    /// Clouds concentrated in western hemisphere
    Western,
    /// Clouds form a ring around horizon
    Horizon,
    /// Clouds concentrated at zenith (overhead)
    Zenith,
    /// Scattered patches across sky
    Scattered,
}

/// Cloud layer type for multi-layer cloud systems
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum CloudLayerType {
    /// Fluffy cumulus clouds (fair weather)
    #[default]
    Cumulus,
    /// High wispy cirrus clouds
    Cirrus,
    /// Flat stratus layer clouds
    Stratus,
    /// Towering cumulonimbus (storm clouds)
    Cumulonimbus,
    /// Alto-level mid-altitude clouds
    Altocumulus,
}

/// Volumetric cloud system component (Lighting child)
/// AAA-quality procedural clouds with wind movement and lighting interaction
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Clouds {
    /// Whether clouds are enabled
    pub enabled: bool,
    
    /// Cloud density (0.0 - 1.0) - affects how many clouds appear
    pub density: f32,
    
    /// Cloud coverage amount (0.0 - 1.0) - percentage of sky covered
    pub coverage: f32,
    
    /// Cloud spread/scale - larger values = bigger, more spread out clouds
    pub spread: f32,
    
    /// Cloud layer altitude in studs (height above ground)
    pub altitude: f32,
    
    /// Cloud layer thickness in studs
    pub thickness: f32,
    
    /// Primary cloud color (lit by sun)
    pub color: [f32; 4],
    
    /// Shadow/underside color
    pub shadow_color: [f32; 4],
    
    /// Cloud softness (0.0 = hard edges, 1.0 = very fluffy)
    pub softness: f32,
    
    /// Wind direction in degrees (0 = North, 90 = East)
    pub wind_direction: f32,
    
    /// Wind speed in studs per second
    pub wind_speed: f32,
    
    /// Cloud coverage hemisphere mode
    pub coverage_mode: CloudCoverage,
    
    /// Coverage bias (0.0 - 1.0) - how strongly clouds concentrate in coverage_mode direction
    pub coverage_bias: f32,
    
    /// Cloud layer type (affects shape and behavior)
    pub layer_type: CloudLayerType,
    
    /// Whether clouds cast shadows on terrain
    pub cast_shadows: bool,
    
    /// Shadow intensity (0.0 - 1.0)
    pub shadow_intensity: f32,
    
    /// Noise scale for cloud shape variation
    pub noise_scale: f32,
    
    /// Animation speed multiplier
    pub animation_speed: f32,
    
    /// Whether clouds are affected by time of day (sunrise/sunset coloring)
    pub time_of_day_tinting: bool,
}

impl Default for Clouds {
    fn default() -> Self {
        Self {
            enabled: true,
            density: 0.5,
            coverage: 0.4,
            spread: 1.0,
            altitude: 500.0,
            thickness: 100.0,
            color: [1.0, 1.0, 1.0, 1.0],
            shadow_color: [0.4, 0.4, 0.5, 1.0],
            softness: 0.7,
            wind_direction: 45.0,  // Northeast
            wind_speed: 10.0,
            coverage_mode: CloudCoverage::Full,
            coverage_bias: 0.5,
            layer_type: CloudLayerType::Cumulus,
            cast_shadows: true,
            shadow_intensity: 0.3,
            noise_scale: 1.0,
            animation_speed: 1.0,
            time_of_day_tinting: true,
        }
    }
}

impl Clouds {
    /// Create clear sky (minimal clouds)
    pub fn clear() -> Self {
        Self {
            density: 0.1,
            coverage: 0.1,
            ..Default::default()
        }
    }
    
    /// Create partly cloudy sky
    pub fn partly_cloudy() -> Self {
        Self {
            density: 0.4,
            coverage: 0.3,
            layer_type: CloudLayerType::Cumulus,
            ..Default::default()
        }
    }
    
    /// Create overcast sky
    pub fn overcast() -> Self {
        Self {
            density: 0.9,
            coverage: 0.85,
            layer_type: CloudLayerType::Stratus,
            color: [0.8, 0.8, 0.85, 1.0],
            shadow_color: [0.5, 0.5, 0.55, 1.0],
            softness: 0.3,
            ..Default::default()
        }
    }
    
    /// Create stormy sky
    pub fn stormy() -> Self {
        Self {
            density: 0.95,
            coverage: 0.9,
            layer_type: CloudLayerType::Cumulonimbus,
            color: [0.4, 0.4, 0.45, 1.0],
            shadow_color: [0.2, 0.2, 0.25, 1.0],
            thickness: 200.0,
            wind_speed: 30.0,
            shadow_intensity: 0.6,
            ..Default::default()
        }
    }
    
    /// Create high cirrus clouds
    pub fn cirrus() -> Self {
        Self {
            density: 0.3,
            coverage: 0.4,
            layer_type: CloudLayerType::Cirrus,
            altitude: 1000.0,
            thickness: 50.0,
            softness: 0.9,
            spread: 2.0,
            ..Default::default()
        }
    }
    
    /// Get wind direction as a normalized Vec3 (XZ plane)
    pub fn wind_direction_vec(&self) -> Vec3 {
        let rad = self.wind_direction.to_radians();
        Vec3::new(rad.sin(), 0.0, rad.cos())
    }
}

// ============================================================================
// 23d. Sun (Lighting child - Celestial Body)
// ============================================================================

/// Sun celestial body component (Lighting child)
/// Controls day/night cycle, directional lighting, and sky appearance
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Sun {
    /// Whether the sun is enabled
    pub enabled: bool,
    
    /// Current time of day in hours (0.0 - 24.0)
    /// 0 = midnight, 6 = sunrise, 12 = noon, 18 = sunset
    pub time_of_day: f32,
    
    /// Speed of day/night cycle (1.0 = real-time, 60.0 = 1 minute = 1 hour)
    pub cycle_speed: f32,
    
    /// Whether the cycle is paused
    pub cycle_paused: bool,
    
    /// Geographic latitude (-90 to 90) - affects sun path angle
    pub latitude: f32,
    
    /// Day of year (1-365) - affects sun path and day length
    pub day_of_year: u16,
    
    /// Sun angular diameter in degrees (real sun is ~0.53°)
    pub angular_size: f32,
    
    /// Sun color at noon
    pub noon_color: [f32; 4],
    
    /// Sun color at sunrise/sunset
    pub horizon_color: [f32; 4],
    
    /// Sun intensity at noon (lux)
    pub noon_intensity: f32,
    
    /// Sun intensity at horizon
    pub horizon_intensity: f32,
    
    /// Whether sun casts shadows
    pub cast_shadows: bool,
    
    /// Shadow softness (0.0 = hard, 1.0 = very soft)
    pub shadow_softness: f32,
    
    /// Ambient light color during day
    pub ambient_day_color: [f32; 4],
    
    /// Ambient light color during night
    pub ambient_night_color: [f32; 4],
    
    /// Sun corona/glow intensity
    pub corona_intensity: f32,
    
    /// God rays / light shaft intensity
    pub god_rays_intensity: f32,
    
    /// Sun texture asset ID (for visual rendering in sky)
    /// If empty, uses procedural sun rendering
    pub texture: String,
}

impl Default for Sun {
    fn default() -> Self {
        Self {
            enabled: true,
            time_of_day: 12.0,  // Noon
            cycle_speed: 0.0,   // Paused by default
            cycle_paused: true,
            latitude: 45.0,     // Mid-latitude
            day_of_year: 172,   // Summer solstice (longest day)
            angular_size: 5.3, // Visible but not overwhelming (real sun is ~0.53°)
            noon_color: [1.0, 0.98, 0.95, 1.0],
            horizon_color: [1.0, 0.5, 0.2, 1.0],
            noon_intensity: 100000.0,  // ~100k lux (bright sunlight)
            horizon_intensity: 1000.0,
            cast_shadows: true,
            shadow_softness: 0.2,
            ambient_day_color: [0.4, 0.5, 0.7, 1.0],
            ambient_night_color: [0.02, 0.02, 0.05, 1.0],
            corona_intensity: 0.3,
            god_rays_intensity: 0.0,
            texture: String::new(),
        }
    }
}

impl Sun {
    /// Get sun elevation angle in degrees (-90 to 90)
    /// Negative = below horizon, 0 = at horizon, 90 = directly overhead
    pub fn elevation(&self) -> f32 {
        // Simplified solar position calculation
        let hour_angle = (self.time_of_day - 12.0) * 15.0; // degrees from solar noon
        let declination = 23.45 * ((360.0 / 365.0) * (self.day_of_year as f32 - 81.0)).to_radians().sin();
        
        let lat_rad = self.latitude.to_radians();
        let dec_rad = declination.to_radians();
        let hour_rad = hour_angle.to_radians();
        
        let sin_elevation = lat_rad.sin() * dec_rad.sin() 
            + lat_rad.cos() * dec_rad.cos() * hour_rad.cos();
        
        sin_elevation.asin().to_degrees()
    }
    
    /// Get sun azimuth angle in degrees (0 = North, 90 = East)
    pub fn azimuth(&self) -> f32 {
        let hour_angle = (self.time_of_day - 12.0) * 15.0;
        let declination = 23.45 * ((360.0 / 365.0) * (self.day_of_year as f32 - 81.0)).to_radians().sin();
        
        let lat_rad = self.latitude.to_radians();
        let dec_rad = declination.to_radians();
        let _hour_rad = hour_angle.to_radians();
        let elev_rad = self.elevation().to_radians();
        
        let cos_azimuth = (dec_rad.sin() - lat_rad.sin() * elev_rad.sin()) 
            / (lat_rad.cos() * elev_rad.cos());
        
        let azimuth = cos_azimuth.clamp(-1.0, 1.0).acos().to_degrees();
        
        if hour_angle > 0.0 { 360.0 - azimuth } else { azimuth }
    }
    
    /// Get sun direction vector (normalized, pointing toward sun)
    pub fn direction(&self) -> Vec3 {
        let elev_rad = self.elevation().to_radians();
        let azim_rad = self.azimuth().to_radians();
        
        Vec3::new(
            azim_rad.sin() * elev_rad.cos(),
            elev_rad.sin(),
            azim_rad.cos() * elev_rad.cos(),
        ).normalize()
    }
    
    /// Check if it's daytime (sun above horizon)
    pub fn is_day(&self) -> bool {
        self.elevation() > 0.0
    }
    
    /// Get current sun color interpolated by elevation
    pub fn current_color(&self) -> [f32; 4] {
        let elev = self.elevation();
        let t = ((elev + 10.0) / 30.0).clamp(0.0, 1.0); // Transition zone
        
        [
            self.horizon_color[0] + (self.noon_color[0] - self.horizon_color[0]) * t,
            self.horizon_color[1] + (self.noon_color[1] - self.horizon_color[1]) * t,
            self.horizon_color[2] + (self.noon_color[2] - self.horizon_color[2]) * t,
            1.0,
        ]
    }
    
    /// Get current sun intensity interpolated by elevation
    pub fn current_intensity(&self) -> f32 {
        let elev = self.elevation();
        if elev < -6.0 {
            0.0  // Civil twilight ended
        } else if elev < 0.0 {
            // Twilight
            self.horizon_intensity * ((elev + 6.0) / 6.0)
        } else {
            // Day
            let t = (elev / 60.0).clamp(0.0, 1.0);
            self.horizon_intensity + (self.noon_intensity - self.horizon_intensity) * t
        }
    }
    
    /// Create a noon sun
    pub fn noon() -> Self {
        Self {
            time_of_day: 12.0,
            ..Default::default()
        }
    }
    
    /// Create a sunrise sun
    pub fn sunrise() -> Self {
        Self {
            time_of_day: 6.0,
            ..Default::default()
        }
    }
    
    /// Create a sunset sun
    pub fn sunset() -> Self {
        Self {
            time_of_day: 18.0,
            ..Default::default()
        }
    }
    
    /// Create a midnight sun (for polar regions)
    pub fn midnight() -> Self {
        Self {
            time_of_day: 0.0,
            ..Default::default()
        }
    }
}

// ============================================================================
// 23e. Moon (Lighting child - Celestial Body)
// ============================================================================

/// Moon phase enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum MoonPhase {
    /// New moon (not visible)
    NewMoon,
    /// Waxing crescent (growing, less than half)
    WaxingCrescent,
    /// First quarter (half moon, growing)
    FirstQuarter,
    /// Waxing gibbous (growing, more than half)
    WaxingGibbous,
    /// Full moon
    #[default]
    FullMoon,
    /// Waning gibbous (shrinking, more than half)
    WaningGibbous,
    /// Last quarter (half moon, shrinking)
    LastQuarter,
    /// Waning crescent (shrinking, less than half)
    WaningCrescent,
}

impl MoonPhase {
    /// Get illumination fraction (0.0 - 1.0)
    pub fn illumination(&self) -> f32 {
        match self {
            MoonPhase::NewMoon => 0.0,
            MoonPhase::WaxingCrescent => 0.25,
            MoonPhase::FirstQuarter => 0.5,
            MoonPhase::WaxingGibbous => 0.75,
            MoonPhase::FullMoon => 1.0,
            MoonPhase::WaningGibbous => 0.75,
            MoonPhase::LastQuarter => 0.5,
            MoonPhase::WaningCrescent => 0.25,
        }
    }
    
    /// Calculate phase from lunar day (0-29.5)
    pub fn from_lunar_day(day: f32) -> Self {
        let day = day % 29.53; // Synodic month
        match day {
            d if d < 1.85 => MoonPhase::NewMoon,
            d if d < 7.38 => MoonPhase::WaxingCrescent,
            d if d < 9.23 => MoonPhase::FirstQuarter,
            d if d < 14.76 => MoonPhase::WaxingGibbous,
            d if d < 16.61 => MoonPhase::FullMoon,
            d if d < 22.14 => MoonPhase::WaningGibbous,
            d if d < 23.99 => MoonPhase::LastQuarter,
            _ => MoonPhase::WaningCrescent,
        }
    }
}

/// Moon celestial body component (Lighting child)
/// Realistic moon with phases, celestial path, and night lighting
/// 
/// # Orbital Mechanics
/// The Moon follows a realistic orbital path:
/// - Orbital period: ~27.3 days (sidereal month)
/// - Synodic period: ~29.53 days (phase cycle, new moon to new moon)
/// - Orbital inclination: ~5.1° to the ecliptic plane
/// - Moon rises ~50 minutes later each day
/// 
/// # Phase Calculation
/// Moon phase is calculated from the Sun-Moon elongation angle:
/// - New Moon: Moon near Sun (0° elongation)
/// - First Quarter: Moon 90° east of Sun
/// - Full Moon: Moon opposite Sun (180° elongation)
/// - Last Quarter: Moon 90° west of Sun
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Moon {
    /// Whether the moon is enabled
    pub enabled: bool,
    
    /// Current lunar day (0.0 - 29.53, synodic month)
    /// Determines phase and position relative to sun
    pub lunar_day: f32,
    
    /// Whether lunar cycle advances with sun cycle
    pub sync_with_sun: bool,
    
    /// Moon angular diameter in degrees (real moon is ~0.52°)
    pub angular_size: f32,
    
    /// Moon surface color
    pub color: [f32; 4],
    
    /// Moon glow/halo color
    pub glow_color: [f32; 4],
    
    /// Moon intensity at full moon (lux, real is ~0.3 lux)
    pub full_intensity: f32,
    
    /// Whether moon casts shadows at night
    pub cast_shadows: bool,
    
    /// Shadow intensity (0.0 - 1.0)
    pub shadow_intensity: f32,
    
    /// Shadow color (bluish for moonlight)
    pub shadow_color: [f32; 4],
    
    /// Glow intensity around moon
    pub glow_intensity: f32,
    
    /// Earthshine intensity (faint illumination of dark side)
    pub earthshine_intensity: f32,
    
    /// Moon texture/surface detail level (0.0 - 1.0)
    pub surface_detail: f32,
    
    /// Orbital inclination to ecliptic in degrees (real: ~5.1°)
    pub orbital_inclination: f32,
    
    /// Current ascending node longitude (precesses over 18.6 years)
    /// This affects where the moon crosses the ecliptic plane
    pub ascending_node: f32,
    
    /// Moon texture asset ID (for visual rendering in sky)
    /// If empty, uses procedural moon rendering
    pub texture: String,
}

impl Default for Moon {
    fn default() -> Self {
        Self {
            enabled: true,
            lunar_day: 14.76,  // Full moon
            sync_with_sun: true,
            angular_size: 5.2, // Large and visible (real moon is ~0.52°)
            color: [0.95, 0.95, 1.0, 1.0],
            glow_color: [0.8, 0.85, 1.0, 0.3],
            full_intensity: 0.5,  // Slightly brighter than real for gameplay
            cast_shadows: true,
            shadow_intensity: 0.15,
            shadow_color: [0.1, 0.1, 0.2, 1.0],
            glow_intensity: 0.2,
            earthshine_intensity: 0.02,
            surface_detail: 0.8,
            orbital_inclination: 5.145,  // Real moon orbital inclination
            ascending_node: 0.0,         // Starting position of ascending node
            texture: String::new(),
        }
    }
}

impl Moon {
    /// Synodic month length in days (new moon to new moon)
    pub const SYNODIC_MONTH: f32 = 29.53;
    
    /// Sidereal month length in days (orbital period)
    pub const SIDEREAL_MONTH: f32 = 27.32;
    
    /// Get current moon phase
    pub fn phase(&self) -> MoonPhase {
        MoonPhase::from_lunar_day(self.lunar_day)
    }
    
    /// Get current illumination (0.0 - 1.0) based on Sun-Moon elongation
    /// 
    /// Illumination is calculated from the phase angle (elongation from sun):
    /// - 0° elongation (new moon) = 0% illumination
    /// - 90° elongation (quarter) = 50% illumination  
    /// - 180° elongation (full moon) = 100% illumination
    pub fn illumination(&self) -> f32 {
        let elongation = self.elongation_from_sun();
        // Illumination = (1 - cos(elongation)) / 2
        (1.0 - elongation.to_radians().cos()) / 2.0
    }
    
    /// Get elongation angle from Sun in degrees (0-360)
    /// This is the angular separation between Moon and Sun as seen from Earth
    /// 
    /// - 0° = New Moon (Moon near Sun)
    /// - 90° = First Quarter (Moon 90° east of Sun)
    /// - 180° = Full Moon (Moon opposite Sun)
    /// - 270° = Last Quarter (Moon 90° west of Sun)
    pub fn elongation_from_sun(&self) -> f32 {
        (self.lunar_day / Self::SYNODIC_MONTH) * 360.0
    }
    
    /// Calculate Moon's ecliptic longitude offset from Sun
    /// The Moon moves ~12.2° per day along the ecliptic
    pub fn ecliptic_longitude_offset(&self) -> f32 {
        self.elongation_from_sun()
    }
    
    /// Calculate Moon's ecliptic latitude based on orbital inclination
    /// The Moon oscillates ±5.1° above/below the ecliptic plane
    /// 
    /// # Arguments
    /// * `orbital_position` - Position in orbit (0-360°), derived from sidereal position
    pub fn ecliptic_latitude(&self) -> f32 {
        // Moon's position in its inclined orbit
        // The inclination causes the moon to move above/below the ecliptic
        let orbital_angle = (self.lunar_day / Self::SIDEREAL_MONTH) * 360.0;
        let node_angle = orbital_angle - self.ascending_node;
        
        // Latitude = inclination * sin(angle from ascending node)
        self.orbital_inclination * node_angle.to_radians().sin()
    }
    
    /// Get moon direction vector using realistic orbital mechanics
    /// 
    /// The Moon follows the Sun's path (ecliptic) but:
    /// 1. Offset by elongation angle (determines phase)
    /// 2. Inclined ~5.1° to the ecliptic (causes latitude variation)
    /// 
    /// # Arguments
    /// * `sun` - The Sun component for latitude/time calculations
    pub fn direction_realistic(&self, sun: &Sun) -> Vec3 {
        // Get Sun's position
        let sun_elevation = sun.elevation();
        let sun_azimuth = sun.azimuth();
        
        // Moon's elongation from sun (determines phase)
        let elongation = self.elongation_from_sun();
        
        // Moon's azimuth = Sun's azimuth + elongation
        // Moon rises ~50 min later each day, moves ~12° east per day
        let moon_azimuth = (sun_azimuth + elongation) % 360.0;
        
        // Moon's elevation follows similar path to sun but offset
        // At full moon (180° elongation), moon is highest when sun is lowest
        let hour_offset = elongation / 15.0; // Convert degrees to hours
        let effective_hour = (sun.time_of_day + hour_offset) % 24.0;
        
        // Calculate moon elevation using same formula as sun but with offset time
        let hour_angle = (effective_hour - 12.0) * 15.0;
        let declination = 23.45 * ((360.0 / 365.0) * (sun.day_of_year as f32 - 81.0)).to_radians().sin();
        
        // Add moon's orbital inclination effect
        let moon_declination = declination + self.ecliptic_latitude();
        
        let lat_rad = sun.latitude.to_radians();
        let dec_rad = moon_declination.to_radians();
        let hour_rad = hour_angle.to_radians();
        
        let sin_elevation = lat_rad.sin() * dec_rad.sin() 
            + lat_rad.cos() * dec_rad.cos() * hour_rad.cos();
        
        let moon_elevation = sin_elevation.asin().to_degrees();
        
        // Convert to direction vector
        let elev_rad = moon_elevation.to_radians();
        let azim_rad = moon_azimuth.to_radians();
        
        Vec3::new(
            azim_rad.sin() * elev_rad.cos(),
            elev_rad.sin(),
            azim_rad.cos() * elev_rad.cos(),
        ).normalize()
    }
    
    /// Simplified direction calculation (legacy compatibility)
    /// Uses sun direction and rotates by elongation angle
    pub fn direction(&self, sun_direction: Vec3) -> Vec3 {
        let elongation_rad = self.elongation_from_sun().to_radians();
        let inclination_rad = self.orbital_inclination.to_radians();
        
        // Calculate moon's position in its inclined orbit
        let orbital_angle = (self.lunar_day / Self::SIDEREAL_MONTH) * std::f32::consts::TAU;
        let lat_offset = inclination_rad * (orbital_angle - self.ascending_node.to_radians()).sin();
        
        // Rotate sun direction by elongation around Y axis
        let cos_e = elongation_rad.cos();
        let sin_e = elongation_rad.sin();
        
        // Apply rotation and inclination
        let rotated = Vec3::new(
            sun_direction.x * cos_e - sun_direction.z * sin_e,
            sun_direction.y + lat_offset.sin() * 0.2, // Subtle elevation variation from inclination
            sun_direction.x * sin_e + sun_direction.z * cos_e,
        );
        
        rotated.normalize()
    }
    
    /// Get current light intensity based on phase and sun position
    pub fn current_intensity(&self, sun_elevation: f32) -> f32 {
        if sun_elevation > 6.0 {
            // Bright daylight - moon light negligible
            0.0
        } else if sun_elevation > 0.0 {
            // Twilight - moon becoming visible
            let twilight_factor = 1.0 - (sun_elevation / 6.0);
            self.full_intensity * self.illumination() * twilight_factor * 0.3
        } else {
            // Night - full moon intensity scaled by illumination
            self.full_intensity * self.illumination()
        }
    }
    
    /// Check if moon is above horizon given sun position
    pub fn is_visible(&self, sun: &Sun) -> bool {
        let moon_dir = self.direction_realistic(sun);
        moon_dir.y > -0.1 // Slightly below horizon for atmospheric refraction
    }
    
    /// Get the phase angle for rendering (0 = full face lit, PI = new moon)
    /// This is the angle Sun-Moon-Earth, used for rendering the lit portion
    pub fn phase_angle(&self) -> f32 {
        // Phase angle is supplementary to elongation for rendering
        std::f32::consts::PI - self.elongation_from_sun().to_radians()
    }
    
    /// Advance lunar day by given time delta
    pub fn advance(&mut self, days: f32) {
        self.lunar_day = (self.lunar_day + days) % Self::SYNODIC_MONTH;
        // Slowly precess the ascending node (18.6 year cycle)
        self.ascending_node = (self.ascending_node + days * (360.0 / (18.6 * 365.25))) % 360.0;
    }
    
    /// Create a full moon
    pub fn full() -> Self {
        Self {
            lunar_day: Self::SYNODIC_MONTH / 2.0, // 180° elongation
            ..Default::default()
        }
    }
    
    /// Create a new moon
    pub fn new_moon() -> Self {
        Self {
            lunar_day: 0.0, // 0° elongation
            ..Default::default()
        }
    }
    
    /// Create a first quarter moon (half moon, waxing)
    pub fn first_quarter() -> Self {
        Self {
            lunar_day: Self::SYNODIC_MONTH / 4.0, // 90° elongation
            ..Default::default()
        }
    }
    
    /// Create a last quarter moon (half moon, waning)
    pub fn last_quarter() -> Self {
        Self {
            lunar_day: Self::SYNODIC_MONTH * 3.0 / 4.0, // 270° elongation
            ..Default::default()
        }
    }
    
    /// Create a half moon (first quarter) - legacy alias
    pub fn half() -> Self {
        Self::first_quarter()
    }
    
    /// Create a crescent moon (waxing)
    pub fn crescent() -> Self {
        Self {
            lunar_day: Self::SYNODIC_MONTH / 8.0, // 45° elongation
            ..Default::default()
        }
    }
    
    /// Create moon at specific elongation from sun
    pub fn at_elongation(elongation_degrees: f32) -> Self {
        Self {
            lunar_day: (elongation_degrees / 360.0) * Self::SYNODIC_MONTH,
            ..Default::default()
        }
    }
}

// ============================================================================
// 24. UnionOperation (CSG: Mesh Union/Subtract/Intersect) - AAA Quality
// ============================================================================

/// CSG operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum CSGOperation {
    /// Combine meshes (A + B)
    #[default]
    Union,
    /// Subtract second from first (A - B)
    Subtract,
    /// Keep only intersection (A ∩ B)
    Intersect,
}

/// CSG collision fidelity - how detailed the collision mesh is
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum CollisionFidelity {
    /// Use bounding box (fastest, least accurate)
    Box,
    /// Use convex hull (fast, good for simple shapes)
    Hull,
    /// Use simplified mesh (balanced)
    #[default]
    Default,
    /// Use exact mesh (slowest, most accurate)
    PreciseConvexDecomposition,
}

/// CSG render fidelity - visual quality of the result
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum RenderFidelity {
    /// Automatic based on part count
    #[default]
    Automatic,
    /// Lower quality, better performance
    Performance,
    /// Higher quality, more triangles
    Precise,
}

/// Combines BaseParts into single mesh using CSG boolean operations
/// Bevy: Procedural Mesh from CSG library (parry3d for boolean ops)
/// 
/// # AAA Features
/// - Union, Subtract, Intersect operations
/// - Configurable collision fidelity
/// - Configurable render fidelity
/// - Smooth shading options
/// - Material preservation from source parts
/// - Async mesh generation for complex operations
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct UnionOperation {
    // ═══════════════════════════════════════════════════════════════════════════
    // OPERATION
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// CSG operation type
    pub operation: CSGOperation,
    
    /// Source part entity IDs to combine
    pub source_parts: Vec<u32>,
    
    /// Whether operation has been computed
    pub is_computed: bool,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // APPEARANCE
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Use colors from source parts (Eustress "UsePartColor")
    pub use_part_color: bool,
    
    /// Override color (if use_part_color is false)
    pub color: [f32; 4],
    
    /// Override material (if use_part_color is false)
    pub material: Material,
    
    /// Transparency (0 = opaque, 1 = invisible)
    pub transparency: f32,
    
    /// Reflectance (0-1)
    pub reflectance: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // QUALITY
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Collision mesh fidelity
    pub collision_fidelity: CollisionFidelity,
    
    /// Render mesh fidelity
    pub render_fidelity: RenderFidelity,
    
    /// Smoothing angle in degrees (0 = flat shading, 180 = smooth)
    pub smoothing_angle: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // PHYSICS
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Whether the union is anchored (immovable)
    pub anchored: bool,
    
    /// Whether the union can collide
    pub can_collide: bool,
    
    /// Whether the union can be touched (trigger events)
    pub can_touch: bool,
    
    /// Collision group name
    pub collision_group: String,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // METADATA
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Triangle count of result mesh (read-only)
    pub triangle_count: u32,
    
    /// Whether mesh needs recomputation
    pub needs_recompute: bool,
    
    /// Asset ID of cached mesh (for fast loading)
    pub cached_mesh_id: Option<String>,
}

impl Default for UnionOperation {
    fn default() -> Self {
        Self {
            operation: CSGOperation::Union,
            source_parts: Vec::new(),
            is_computed: false,
            use_part_color: true,
            color: [0.6, 0.6, 0.6, 1.0],
            material: Material::Plastic,
            transparency: 0.0,
            reflectance: 0.0,
            collision_fidelity: CollisionFidelity::Default,
            render_fidelity: RenderFidelity::Automatic,
            smoothing_angle: 30.0,
            anchored: true,
            can_collide: true,
            can_touch: true,
            collision_group: "Default".to_string(),
            triangle_count: 0,
            needs_recompute: true,
            cached_mesh_id: None,
        }
    }
}

impl UnionOperation {
    /// Create a union of parts
    pub fn union(parts: Vec<u32>) -> Self {
        Self {
            operation: CSGOperation::Union,
            source_parts: parts,
            ..Default::default()
        }
    }
    
    /// Create a subtraction (first part minus others)
    pub fn subtract(base: u32, subtract: Vec<u32>) -> Self {
        let mut parts = vec![base];
        parts.extend(subtract);
        Self {
            operation: CSGOperation::Subtract,
            source_parts: parts,
            ..Default::default()
        }
    }
    
    /// Create an intersection of parts
    pub fn intersect(parts: Vec<u32>) -> Self {
        Self {
            operation: CSGOperation::Intersect,
            source_parts: parts,
            ..Default::default()
        }
    }
    
    /// Mark as needing recomputation
    pub fn invalidate(&mut self) {
        self.needs_recompute = true;
        self.is_computed = false;
    }
    
    /// Add a source part
    pub fn add_part(&mut self, part_id: u32) {
        self.source_parts.push(part_id);
        self.invalidate();
    }
    
    /// Remove a source part
    pub fn remove_part(&mut self, part_id: u32) {
        self.source_parts.retain(|&id| id != part_id);
        self.invalidate();
    }
}

// ============================================================================
// 24b. Lighting (Scene Lighting Container Component)
// ============================================================================

/// Lighting component - scene lighting container (child of DataModel)
/// This is the **component** version for scene hierarchy.
/// See `LightingService` resource for the runtime service.
/// 
/// Children: Atmosphere, Sky, Clouds, Sun, Moon
/// 
/// # AAA Features
/// - Time of day control with realistic sun position
/// - Global ambient and outdoor ambient
/// - Fog with distance-based falloff
/// - Shadow configuration
/// - Exposure and tone mapping
/// - Color grading
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Lighting {
    // ═══════════════════════════════════════════════════════════════════════════
    // TIME
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Time of day (0.0 = midnight, 0.5 = noon, 1.0 = midnight)
    pub time_of_day: f32,
    
    /// Clock time string for display (e.g., "14:30:00")
    pub clock_time: String,
    
    /// Geographic latitude for sun position (-90 to 90)
    pub geographic_latitude: f32,
    
    /// Whether time advances automatically
    pub time_cycle_enabled: bool,
    
    /// Speed of time cycle (1.0 = real-time, 60.0 = 1 min = 1 hour)
    pub time_cycle_speed: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // AMBIENT LIGHTING
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Indoor ambient light color (RGBA)
    pub ambient: [f32; 4],
    
    /// Outdoor ambient light color (RGBA)
    pub outdoor_ambient: [f32; 4],
    
    /// Overall brightness multiplier
    pub brightness: f32,
    
    /// Environment diffuse scale (affects indirect lighting)
    pub environment_diffuse_scale: f32,
    
    /// Environment specular scale (affects reflections)
    pub environment_specular_scale: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // FOG
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Whether fog is enabled
    pub fog_enabled: bool,
    
    /// Fog color (RGBA)
    pub fog_color: [f32; 4],
    
    /// Fog start distance (studs)
    pub fog_start: f32,
    
    /// Fog end distance (studs)
    pub fog_end: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // SHADOWS
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Whether shadows are enabled globally
    pub shadows_enabled: bool,
    
    /// Shadow softness (0 = hard, 1 = very soft)
    pub shadow_softness: f32,
    
    /// Shadow color (for colored shadows)
    pub shadow_color: [f32; 4],
    
    // ═══════════════════════════════════════════════════════════════════════════
    // EXPOSURE & TONE MAPPING
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Exposure compensation (EV, -5 to +5)
    pub exposure_compensation: f32,
    
    /// Auto-exposure enabled
    pub auto_exposure: bool,
    
    /// Auto-exposure min (EV)
    pub auto_exposure_min: f32,
    
    /// Auto-exposure max (EV)
    pub auto_exposure_max: f32,
    
    /// Auto-exposure adaptation speed
    pub auto_exposure_speed: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // COLOR GRADING
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Color correction enabled
    pub color_correction_enabled: bool,
    
    /// Saturation (0 = grayscale, 1 = normal, 2 = oversaturated)
    pub saturation: f32,
    
    /// Contrast (0.5 = low, 1 = normal, 2 = high)
    pub contrast: f32,
    
    /// Tint color (multiplied with final image)
    pub tint_color: [f32; 4],
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            // Time
            time_of_day: 0.5, // Noon
            clock_time: "12:00:00".to_string(),
            geographic_latitude: 45.0,
            time_cycle_enabled: false,
            time_cycle_speed: 0.0,
            
            // Ambient
            ambient: [0.4, 0.45, 0.5, 1.0],
            outdoor_ambient: [0.5, 0.55, 0.6, 1.0],
            brightness: 1.0,
            environment_diffuse_scale: 1.0,
            environment_specular_scale: 1.0,
            
            // Fog
            fog_enabled: false,
            fog_color: [0.8, 0.85, 0.9, 1.0],
            fog_start: 100.0,
            fog_end: 500.0,
            
            // Shadows
            shadows_enabled: true,
            shadow_softness: 0.5,
            shadow_color: [0.0, 0.0, 0.0, 0.5],
            
            // Exposure
            exposure_compensation: 0.0,
            auto_exposure: false,
            auto_exposure_min: -2.0,
            auto_exposure_max: 2.0,
            auto_exposure_speed: 1.0,
            
            // Color grading
            color_correction_enabled: false,
            saturation: 1.0,
            contrast: 1.0,
            tint_color: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

impl Lighting {
    /// Create a bright sunny day lighting
    pub fn sunny_day() -> Self {
        Self {
            time_of_day: 0.5,
            brightness: 1.2,
            shadows_enabled: true,
            shadow_softness: 0.3,
            ..Default::default()
        }
    }
    
    /// Create a sunset lighting
    pub fn sunset() -> Self {
        Self {
            time_of_day: 0.75, // 6 PM
            ambient: [0.5, 0.35, 0.25, 1.0],
            outdoor_ambient: [0.8, 0.5, 0.3, 1.0],
            brightness: 0.8,
            tint_color: [1.0, 0.9, 0.8, 1.0],
            color_correction_enabled: true,
            ..Default::default()
        }
    }
    
    /// Create a night lighting
    pub fn night() -> Self {
        Self {
            time_of_day: 0.0, // Midnight
            ambient: [0.05, 0.05, 0.1, 1.0],
            outdoor_ambient: [0.02, 0.02, 0.05, 1.0],
            brightness: 0.3,
            shadows_enabled: true,
            shadow_softness: 0.8,
            ..Default::default()
        }
    }
    
    /// Create an overcast lighting
    pub fn overcast() -> Self {
        Self {
            time_of_day: 0.5,
            ambient: [0.5, 0.5, 0.55, 1.0],
            outdoor_ambient: [0.6, 0.6, 0.65, 1.0],
            brightness: 0.7,
            shadows_enabled: false,
            fog_enabled: true,
            fog_start: 50.0,
            fog_end: 300.0,
            ..Default::default()
        }
    }
    
    /// Get hours from time_of_day
    pub fn hours(&self) -> u32 {
        (self.time_of_day * 24.0) as u32
    }
    
    /// Get minutes from time_of_day
    pub fn minutes(&self) -> u32 {
        ((self.time_of_day * 24.0 * 60.0) % 60.0) as u32
    }
    
    /// Set time from hours and minutes
    pub fn set_time(&mut self, hours: u32, minutes: u32) {
        self.time_of_day = (hours as f32 + minutes as f32 / 60.0) / 24.0;
        self.clock_time = format!("{:02}:{:02}:00", hours % 24, minutes % 60);
    }
    
    /// Check if it's daytime (sun above horizon)
    pub fn is_day(&self) -> bool {
        self.time_of_day > 0.25 && self.time_of_day < 0.75
    }
}

// ============================================================================
// 24c. WorkspaceComponent (Root 3D Container Component)
// ============================================================================

/// Workspace component - root container for 3D objects (child of DataModel)
/// This is the **component** version for scene hierarchy.
/// See `Workspace` resource for the runtime service with physics settings.
/// 
/// Children: All 3D objects (Parts, Models, Terrain, etc.)
/// 
/// # AAA Features
/// - World bounds for spatial partitioning
/// - Physics configuration
/// - Streaming settings
/// - Camera configuration
/// - Filtering settings
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct WorkspaceComponent {
    // ═══════════════════════════════════════════════════════════════════════════
    // PHYSICS
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Gravity in studs/s² (default: 196.2, Roblox-style)
    /// Negative Y = downward
    pub gravity: f32,
    
    /// Air density for drag calculations
    pub air_density: f32,
    
    /// Whether physics simulation is enabled
    pub physics_enabled: bool,
    
    /// Physics timestep (seconds per step)
    pub physics_timestep: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // WORLD BOUNDS
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Minimum world bounds (studs)
    pub world_bounds_min: [f32; 3],
    
    /// Maximum world bounds (studs)
    pub world_bounds_max: [f32; 3],
    
    /// Fall-through Y level (entities below this are destroyed)
    pub fallen_parts_destroy_height: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // STREAMING
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Whether streaming is enabled (for large worlds)
    pub streaming_enabled: bool,
    
    /// Streaming target radius (studs)
    pub streaming_target_radius: f32,
    
    /// Streaming min radius (always loaded)
    pub streaming_min_radius: f32,
    
    /// Streaming integrity mode
    pub streaming_integrity_mode: StreamingIntegrityMode,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // CAMERA
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Current camera entity ID
    pub current_camera: Option<u32>,
    
    /// Default camera distance from character
    pub default_camera_distance: f32,
    
    /// Camera collision enabled
    pub camera_collision_enabled: bool,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // FILTERING
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Filtering enabled (client-server separation)
    pub filtering_enabled: bool,
    
    /// Replication lag compensation
    pub replication_lag: f32,
    
    // ═══════════════════════════════════════════════════════════════════════════
    // RENDERING
    // ═══════════════════════════════════════════════════════════════════════════
    
    /// Global render distance (studs)
    pub render_distance: f32,
    
    /// Whether to use distance-based LOD
    pub distance_lod_enabled: bool,
    
    /// LOD bias (negative = higher quality, positive = lower quality)
    pub lod_bias: f32,
}

/// Streaming integrity mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum StreamingIntegrityMode {
    /// Default - balance between performance and integrity
    #[default]
    Default,
    /// Minimal - prioritize performance, allow more popping
    Minimal,
    /// Pause on unload - pause game when critical content unloads
    PauseOnUnload,
}

impl Default for WorkspaceComponent {
    fn default() -> Self {
        Self {
            // Physics
            gravity: 196.2, // Roblox-style gravity
            air_density: 1.225, // kg/m³ at sea level
            physics_enabled: true,
            physics_timestep: 1.0 / 60.0,
            
            // World bounds
            world_bounds_min: [-10000.0, -500.0, -10000.0],
            world_bounds_max: [10000.0, 10000.0, 10000.0],
            fallen_parts_destroy_height: -500.0,
            
            // Streaming
            streaming_enabled: false,
            streaming_target_radius: 1024.0,
            streaming_min_radius: 256.0,
            streaming_integrity_mode: StreamingIntegrityMode::Default,
            
            // Camera
            current_camera: None,
            default_camera_distance: 15.0,
            camera_collision_enabled: true,
            
            // Filtering
            filtering_enabled: true,
            replication_lag: 0.0,
            
            // Rendering
            render_distance: 5000.0,
            distance_lod_enabled: true,
            lod_bias: 0.0,
        }
    }
}

impl WorkspaceComponent {
    /// Create a small workspace (for testing/small games)
    pub fn small() -> Self {
        Self {
            world_bounds_min: [-1000.0, -100.0, -1000.0],
            world_bounds_max: [1000.0, 1000.0, 1000.0],
            render_distance: 1000.0,
            streaming_enabled: false,
            ..Default::default()
        }
    }
    
    /// Create a large workspace (for open world games)
    pub fn large() -> Self {
        Self {
            world_bounds_min: [-50000.0, -1000.0, -50000.0],
            world_bounds_max: [50000.0, 10000.0, 50000.0],
            render_distance: 10000.0,
            streaming_enabled: true,
            streaming_target_radius: 2048.0,
            ..Default::default()
        }
    }
    
    /// Check if a position is within world bounds
    pub fn is_in_bounds(&self, position: Vec3) -> bool {
        position.x >= self.world_bounds_min[0] && position.x <= self.world_bounds_max[0]
            && position.y >= self.world_bounds_min[1] && position.y <= self.world_bounds_max[1]
            && position.z >= self.world_bounds_min[2] && position.z <= self.world_bounds_max[2]
    }
    
    /// Check if a position is below the destroy height
    pub fn should_destroy(&self, position: Vec3) -> bool {
        position.y < self.fallen_parts_destroy_height
    }
    
    /// Get gravity as Vec3
    pub fn gravity_vec(&self) -> Vec3 {
        Vec3::new(0.0, -self.gravity, 0.0)
    }
}

// ============================================================================
// 25. Media Asset Classes (Document, ImageAsset, VideoAsset)
// ============================================================================

/// Asset source type - where the asset is stored/loaded from
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum AssetSourceType {
    /// Local file path on disk
    #[default]
    LocalPath,
    /// Cloud storage URL (MinIO, AWS S3, etc.)
    CloudUrl,
    /// Asset pipeline managed (uploaded to project assets)
    AssetPipeline,
    /// External URL (Google Docs, Dropbox, etc.)
    ExternalUrl,
    /// Embedded base64 data (small assets only)
    Embedded,
}

/// Document type for the Document class
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum DocumentType {
    #[default]
    PDF,
    DOCX,
    PPTX,
    XLSX,
    GoogleDoc,
    GoogleSheet,
    GoogleSlide,
    Markdown,
    PlainText,
    RTF,
}

impl DocumentType {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "pdf" => Self::PDF,
            "docx" | "doc" => Self::DOCX,
            "pptx" | "ppt" => Self::PPTX,
            "xlsx" | "xls" => Self::XLSX,
            "md" | "markdown" => Self::Markdown,
            "txt" => Self::PlainText,
            "rtf" => Self::RTF,
            _ => Self::PDF,
        }
    }
    
    pub fn icon(&self) -> &'static str {
        match self {
            Self::PDF => "📕",
            Self::DOCX => "📘",
            Self::PPTX => "📙",
            Self::XLSX => "📗",
            Self::GoogleDoc => "📄",
            Self::GoogleSheet => "📊",
            Self::GoogleSlide => "📽",
            Self::Markdown => "📝",
            Self::PlainText => "📃",
            Self::RTF => "📜",
        }
    }
}

/// Document class - PDF, DOCX, PPTX, XLSX, Google Docs/Sheets/Slides
/// Displayed in Tabbed Viewer with appropriate renderer
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Document {
    /// Document type (PDF, DOCX, etc.)
    pub document_type: DocumentType,
    /// Source type (local, cloud, asset pipeline)
    pub source_type: AssetSourceType,
    /// Path or URL to the document
    pub source_path: String,
    /// Asset ID if uploaded to asset pipeline
    pub asset_id: Option<String>,
    /// Cloud storage bucket (for MinIO/S3)
    pub cloud_bucket: Option<String>,
    /// Cloud storage key/path
    pub cloud_key: Option<String>,
    /// File size in bytes (cached)
    pub file_size: u64,
    /// Page count (for PDF/PPTX)
    pub page_count: Option<u32>,
    /// Last modified timestamp (Unix epoch)
    pub last_modified: u64,
    /// Content hash for change detection
    pub content_hash: Option<String>,
    /// Whether to auto-sync from source
    pub auto_sync: bool,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            document_type: DocumentType::PDF,
            source_type: AssetSourceType::LocalPath,
            source_path: String::new(),
            asset_id: None,
            cloud_bucket: None,
            cloud_key: None,
            file_size: 0,
            page_count: None,
            last_modified: 0,
            content_hash: None,
            auto_sync: false,
        }
    }
}

/// Image format type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum ImageFormat {
    #[default]
    PNG,
    JPG,
    GIF,
    WebP,
    SVG,
    BMP,
    TIFF,
    ICO,
}

impl ImageFormat {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "png" => Self::PNG,
            "jpg" | "jpeg" => Self::JPG,
            "gif" => Self::GIF,
            "webp" => Self::WebP,
            "svg" => Self::SVG,
            "bmp" => Self::BMP,
            "tiff" | "tif" => Self::TIFF,
            "ico" => Self::ICO,
            _ => Self::PNG,
        }
    }
    
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::PNG => "image/png",
            Self::JPG => "image/jpeg",
            Self::GIF => "image/gif",
            Self::WebP => "image/webp",
            Self::SVG => "image/svg+xml",
            Self::BMP => "image/bmp",
            Self::TIFF => "image/tiff",
            Self::ICO => "image/x-icon",
        }
    }
}

/// ImageAsset class - PNG, JPG, GIF, WebP, SVG images
/// Displayed in Tabbed Viewer with image viewer, can also be used as textures
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct ImageAsset {
    /// Image format
    pub format: ImageFormat,
    /// Source type (local, cloud, asset pipeline)
    pub source_type: AssetSourceType,
    /// Path or URL to the image
    pub source_path: String,
    /// Asset ID if uploaded to asset pipeline
    pub asset_id: Option<String>,
    /// Cloud storage bucket
    pub cloud_bucket: Option<String>,
    /// Cloud storage key/path
    pub cloud_key: Option<String>,
    /// Image width in pixels
    pub width: u32,
    /// Image height in pixels
    pub height: u32,
    /// File size in bytes
    pub file_size: u64,
    /// Whether image is animated (GIF, WebP)
    pub animated: bool,
    /// Frame count for animated images
    pub frame_count: Option<u32>,
    /// Content hash for change detection
    pub content_hash: Option<String>,
    /// Whether to auto-sync from source
    pub auto_sync: bool,
    /// Thumbnail asset ID (for preview in Explorer)
    pub thumbnail_id: Option<String>,
}

impl Default for ImageAsset {
    fn default() -> Self {
        Self {
            format: ImageFormat::PNG,
            source_type: AssetSourceType::LocalPath,
            source_path: String::new(),
            asset_id: None,
            cloud_bucket: None,
            cloud_key: None,
            width: 0,
            height: 0,
            file_size: 0,
            animated: false,
            frame_count: None,
            content_hash: None,
            auto_sync: false,
            thumbnail_id: None,
        }
    }
}

/// Video format type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum VideoFormat {
    #[default]
    MP4,
    WebM,
    MOV,
    AVI,
    MKV,
    /// HLS streaming
    HLS,
    /// DASH streaming
    DASH,
}

impl VideoFormat {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "mp4" => Self::MP4,
            "webm" => Self::WebM,
            "mov" => Self::MOV,
            "avi" => Self::AVI,
            "mkv" => Self::MKV,
            "m3u8" => Self::HLS,
            "mpd" => Self::DASH,
            _ => Self::MP4,
        }
    }
    
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::MP4 => "video/mp4",
            Self::WebM => "video/webm",
            Self::MOV => "video/quicktime",
            Self::AVI => "video/x-msvideo",
            Self::MKV => "video/x-matroska",
            Self::HLS => "application/x-mpegURL",
            Self::DASH => "application/dash+xml",
        }
    }
    
    pub fn is_streaming(&self) -> bool {
        matches!(self, Self::HLS | Self::DASH)
    }
}

/// VideoAsset class - MP4, WebM, streaming video
/// Displayed in Tabbed Viewer with video player
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct VideoAsset {
    /// Video format
    pub format: VideoFormat,
    /// Source type (local, cloud, asset pipeline)
    pub source_type: AssetSourceType,
    /// Path or URL to the video
    pub source_path: String,
    /// Asset ID if uploaded to asset pipeline
    pub asset_id: Option<String>,
    /// Cloud storage bucket
    pub cloud_bucket: Option<String>,
    /// Cloud storage key/path
    pub cloud_key: Option<String>,
    /// Video width in pixels
    pub width: u32,
    /// Video height in pixels
    pub height: u32,
    /// Duration in seconds
    pub duration: f32,
    /// Frame rate (fps)
    pub frame_rate: f32,
    /// File size in bytes
    pub file_size: u64,
    /// Has audio track
    pub has_audio: bool,
    /// Content hash for change detection
    pub content_hash: Option<String>,
    /// Whether to auto-sync from source
    pub auto_sync: bool,
    /// Thumbnail/poster image asset ID
    pub thumbnail_id: Option<String>,
    /// Streaming URL (for HLS/DASH)
    pub streaming_url: Option<String>,
    /// Whether video should loop
    pub looping: bool,
    /// Whether video should autoplay
    pub autoplay: bool,
    /// Volume (0.0 - 1.0)
    pub volume: f32,
}

impl Default for VideoAsset {
    fn default() -> Self {
        Self {
            format: VideoFormat::MP4,
            source_type: AssetSourceType::LocalPath,
            source_path: String::new(),
            asset_id: None,
            cloud_bucket: None,
            cloud_key: None,
            width: 0,
            height: 0,
            duration: 0.0,
            frame_rate: 30.0,
            file_size: 0,
            has_audio: true,
            content_hash: None,
            auto_sync: false,
            thumbnail_id: None,
            streaming_url: None,
            looping: false,
            autoplay: false,
            volume: 1.0,
        }
    }
}

// ============================================================================
// 26. Orbital Coordinate Grid Classes
// ============================================================================

/// SolarSystem - Container for orbital hierarchies (like Model for space)
/// 
/// Integrates with `orbital::OrbitalGravity` for n-body simulation.
/// Children are typically CelestialBody entities.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct SolarSystem {
    /// Primary body entity (e.g., Sun for solar system, Earth for Earth-Moon)
    pub primary_body: Option<Entity>,
    
    /// Time scale for orbital simulation (1.0 = real-time)
    pub time_scale: f64,
    
    /// Custom gravitational constant (default: 6.67430e-11)
    pub gravity_constant: f64,
    
    /// Whether n-body simulation is active
    pub simulation_active: bool,
    
    /// Reference frame origin (ECEF position of system barycenter)
    pub barycenter_ecef: [f64; 3],
}

impl Default for SolarSystem {
    fn default() -> Self {
        Self {
            primary_body: None,
            time_scale: 1.0,
            gravity_constant: 6.67430e-11,
            simulation_active: true,
            barycenter_ecef: [0.0, 0.0, 0.0],
        }
    }
}

impl SolarSystem {
    /// Create Earth-centric system (for Earth One)
    pub fn earth_centric() -> Self {
        Self {
            primary_body: None, // Set after spawning Earth
            time_scale: 1.0,
            gravity_constant: 6.67430e-11,
            simulation_active: true,
            barycenter_ecef: [0.0, 0.0, 0.0], // Earth center
        }
    }
    
    /// Create with custom time scale (for fast-forward simulation)
    pub fn with_time_scale(mut self, scale: f64) -> Self {
        self.time_scale = scale;
        self
    }
}

/// CelestialBodyClass - Orbital object with n-body gravity influence
/// 
/// Integrates with `orbital::CelestialBody` for physics and `orbital::GlobalPosition` for coordinates.
/// This is the ECS component; orbital::CelestialBody is the physics data.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct CelestialBodyClass {
    /// Global ECEF position (high precision, meters from reference origin)
    pub global_ecef: [f64; 3],
    
    /// Orbital velocity (m/s in ECEF frame)
    pub orbital_velocity: [f64; 3],
    
    /// Mass in kilograms (for gravity influence)
    pub mass: f64,
    
    /// Gravitational parameter GM (m³/s²) - precomputed for performance
    pub gm: f64,
    
    /// Mean radius in meters
    pub radius: f64,
    
    /// Rotation period in seconds (sidereal day)
    pub rotation_period: f64,
    
    /// Axial tilt in degrees (obliquity)
    pub axial_tilt: f32,
    
    /// Current rotation angle (degrees, for day/night)
    pub rotation_angle: f32,
    
    /// Atmosphere thickness in meters (0 = no atmosphere)
    pub atmosphere_height: f32,
    
    /// Surface material preset
    pub surface_material: Material,
    
    /// Whether this body contributes to gravity calculations
    pub gravitational: bool,
    
    /// Parent body (for orbital hierarchy, e.g., Moon → Earth)
    pub parent_body: Option<Entity>,
    
    /// Semi-major axis of orbit (meters, if orbiting parent)
    pub semi_major_axis: f64,
    
    /// Orbital eccentricity (0 = circular)
    pub eccentricity: f64,
    
    /// Orbital inclination (degrees)
    pub inclination: f32,
}

impl Default for CelestialBodyClass {
    fn default() -> Self {
        Self {
            global_ecef: [0.0, 0.0, 0.0],
            orbital_velocity: [0.0, 0.0, 0.0],
            mass: 5.972e24,           // Earth mass
            gm: 3.986004418e14,       // Earth GM
            radius: 6.371e6,          // Earth mean radius
            rotation_period: 86164.1, // Sidereal day
            axial_tilt: 23.44,
            rotation_angle: 0.0,
            atmosphere_height: 100_000.0, // 100km Karman line
            surface_material: Material::Grass,
            gravitational: true,
            parent_body: None,
            semi_major_axis: 0.0,
            eccentricity: 0.0,
            inclination: 0.0,
        }
    }
}

impl CelestialBodyClass {
    /// Create Earth with standard parameters
    pub fn earth() -> Self {
        Self::default()
    }
    
    /// Create Moon with standard parameters
    pub fn moon() -> Self {
        Self {
            global_ecef: [384_400_000.0, 0.0, 0.0], // Average distance
            orbital_velocity: [0.0, 1022.0, 0.0],   // ~1 km/s orbital velocity
            mass: 7.342e22,
            gm: 4.9048695e12,
            radius: 1_737_400.0,
            rotation_period: 2_360_591.5, // Tidally locked (same as orbital)
            axial_tilt: 6.68,
            rotation_angle: 0.0,
            atmosphere_height: 0.0, // No atmosphere
            surface_material: Material::Slate,
            gravitational: true,
            parent_body: None, // Set after spawning
            semi_major_axis: 384_400_000.0,
            eccentricity: 0.0549,
            inclination: 5.145,
        }
    }
    
    /// Create Sun with standard parameters
    pub fn sun() -> Self {
        Self {
            global_ecef: [1.496e11, 0.0, 0.0], // 1 AU
            orbital_velocity: [0.0, 0.0, 0.0],
            mass: 1.989e30,
            gm: 1.32712440018e20,
            radius: 6.9634e8,
            rotation_period: 2_160_000.0, // ~25 days at equator
            axial_tilt: 7.25,
            rotation_angle: 0.0,
            atmosphere_height: 0.0, // Photosphere handled separately
            surface_material: Material::Neon, // Emissive
            gravitational: true,
            parent_body: None,
            semi_major_axis: 0.0,
            eccentricity: 0.0,
            inclination: 0.0,
        }
    }
    
    /// Create Mars with standard parameters
    pub fn mars() -> Self {
        Self {
            global_ecef: [2.279e11, 0.0, 0.0], // ~1.52 AU average
            orbital_velocity: [0.0, 24_077.0, 0.0], // ~24 km/s orbital velocity
            mass: 6.4171e23,
            gm: 4.282837e13,
            radius: 3.3895e6, // Mean radius
            rotation_period: 88_642.66, // Sol (Martian day) in seconds
            axial_tilt: 25.19,
            rotation_angle: 0.0,
            atmosphere_height: 11_000.0, // ~11 km scale height
            surface_material: Material::Brick, // Reddish
            gravitational: true,
            parent_body: None, // Set after spawning (Sun)
            semi_major_axis: 2.279e11, // 1.524 AU
            eccentricity: 0.0934,
            inclination: 1.85,
        }
    }
    
    /// Convert to orbital::CelestialBody for physics calculations
    pub fn to_orbital_body(&self, name: &str) -> crate::orbital::CelestialBody {
        crate::orbital::CelestialBody {
            name: name.to_string(),
            mass: self.mass,
            gm: self.gm,
            radius: self.radius,
            position: crate::orbital::GlobalPosition::new(
                self.global_ecef[0],
                self.global_ecef[1],
                self.global_ecef[2],
            ),
            active: self.gravitational,
        }
    }
    
    /// Get surface gravity magnitude (m/s²)
    pub fn surface_gravity(&self) -> f64 {
        self.gm / (self.radius * self.radius)
    }
    
    /// Get escape velocity at surface (m/s)
    pub fn escape_velocity(&self) -> f64 {
        (2.0 * self.gm / self.radius).sqrt()
    }
    
    /// Get orbital velocity for circular orbit at given altitude (m/s)
    pub fn orbital_velocity_at(&self, altitude: f64) -> f64 {
        let r = self.radius + altitude;
        (self.gm / r).sqrt()
    }
}

/// RegionChunk - Geospatial fragment with relative Euclidean space
/// 
/// Integrates with `orbital::Region` and `orbital::RegionId` for coordinate management.
/// Children are standard Eustress entities (Part, Model, etc.) in local space.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct RegionChunk {
    /// Global center in ECEF (high precision anchor point)
    pub center_ecef: [f64; 3],
    
    /// Geodetic center (latitude, longitude, altitude) for convenience
    pub center_geodetic: [f64; 3],
    
    /// Local bounds extents (half-size in each axis, meters)
    pub bounds_extents: Vec3,
    
    /// Tile level in 3D Tiles hierarchy (0 = whole Earth, 24 = ~1m)
    pub tile_level: u8,
    
    /// Tile face (cube-sphere mapping, 0-5)
    pub tile_face: u8,
    
    /// Tile X index at this level
    pub tile_x: u32,
    
    /// Tile Y index at this level
    pub tile_y: u32,
    
    /// Whether Gaussian Splatting overlay is enabled
    pub gs_overlay_enabled: bool,
    
    /// Path/URL to point cloud or GIS asset
    pub gis_data_ref: String,
    
    /// Heightmap resolution (vertices per side)
    pub heightmap_resolution: u32,
    
    /// Water level relative to local origin (meters)
    pub water_level: f32,
    
    /// Custom gravity override (None = use orbital gravity)
    pub custom_gravity: Option<Vec3>,
    
    /// Whether this is an abstract (non-Earth) region
    pub is_abstract: bool,
    
    /// Parent region for abstract spaces (links to Earth location)
    pub parent_region: Option<Entity>,
    
    /// Offset from parent region origin (for abstract spaces)
    pub parent_offset: Option<Vec3>,
    
    /// Whether region is currently active/loaded
    pub active: bool,
}

impl Default for RegionChunk {
    fn default() -> Self {
        Self {
            center_ecef: [6_371_000.0, 0.0, 0.0], // On equator at prime meridian
            center_geodetic: [0.0, 0.0, 0.0],     // Null Island
            bounds_extents: Vec3::splat(500.0),   // 1km cube
            tile_level: 16,
            tile_face: 0,
            tile_x: 0,
            tile_y: 0,
            gs_overlay_enabled: false,
            gis_data_ref: String::new(),
            heightmap_resolution: 256,
            water_level: 0.0,
            custom_gravity: None,
            is_abstract: false,
            parent_region: None,
            parent_offset: None,
            active: true,
        }
    }
}

impl RegionChunk {
    /// Create from geodetic coordinates
    pub fn from_geodetic(lat: f64, lon: f64, alt: f64, size: f32) -> Self {
        let ecef = crate::orbital::geodetic_to_ecef(lat, lon, alt);
        let region_id = crate::orbital::RegionId::from_geodetic(lat, lon);
        
        Self {
            center_ecef: [ecef.x, ecef.y, ecef.z],
            center_geodetic: [lat, lon, alt],
            bounds_extents: Vec3::splat(size / 2.0),
            tile_level: region_id.level,
            tile_face: region_id.face,
            tile_x: region_id.x,
            tile_y: region_id.y,
            ..Default::default()
        }
    }
    
    /// Create abstract (non-Earth) region
    pub fn abstract_space(size: Vec3, gravity: Option<Vec3>) -> Self {
        Self {
            center_ecef: [0.0, 0.0, 0.0],
            center_geodetic: [0.0, 0.0, 0.0],
            bounds_extents: size / 2.0,
            tile_level: 255, // Abstract marker
            tile_face: 255,
            is_abstract: true,
            custom_gravity: gravity,
            ..Default::default()
        }
    }
    
    /// Create abstract region linked to Earth location
    pub fn abstract_linked(parent: Entity, offset: Vec3, size: Vec3, gravity: Option<Vec3>) -> Self {
        Self {
            bounds_extents: size / 2.0,
            is_abstract: true,
            parent_region: Some(parent),
            parent_offset: Some(offset),
            custom_gravity: gravity,
            tile_level: 255,
            tile_face: 255,
            ..Default::default()
        }
    }
    
    /// Convert to orbital::Region for coordinate management
    pub fn to_orbital_region(&self) -> crate::orbital::Region {
        if self.is_abstract {
            crate::orbital::Region::abstract_space(
                0, // Will be assigned by registry
                self.bounds_extents * 2.0,
            )
        } else {
            crate::orbital::Region::from_geodetic(
                self.center_geodetic[0],
                self.center_geodetic[1],
                (self.bounds_extents.x * 2.0) as f64,
            )
        }
    }
    
    /// Get RegionId for P2P chunk mapping
    pub fn to_region_id(&self) -> crate::orbital::RegionId {
        if self.is_abstract {
            crate::orbital::RegionId::abstract_region(
                ((self.tile_x as u64) << 32) | (self.tile_y as u64)
            )
        } else {
            crate::orbital::RegionId {
                level: self.tile_level,
                face: self.tile_face,
                x: self.tile_x,
                y: self.tile_y,
                z: 0,
            }
        }
    }
    
    /// Check if a local position is within bounds
    pub fn contains_local(&self, pos: Vec3) -> bool {
        pos.x.abs() <= self.bounds_extents.x &&
        pos.y.abs() <= self.bounds_extents.y &&
        pos.z.abs() <= self.bounds_extents.z
    }
}

// ============================================================================
// 27. Property Trait System
// ============================================================================

/// Unified property access for all classes (for UI/serialization)
pub trait PropertyAccess {
    fn get_property(&self, name: &str) -> Option<PropertyValue>;
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String>;
    fn list_properties(&self) -> Vec<PropertyDescriptor>;
    
    /// Check if a property exists
    fn has_property(&self, name: &str) -> bool {
        self.get_property(name).is_some()
    }
}

/// Property value enum (covers all Eustress data types)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyValue {
    String(String),
    Float(f32),
    Int(i32),
    Bool(bool),
    Vector2([f32; 2]),
    Vector3(Vec3),
    Color(Color),
    Color3([f32; 3]),  // RGB as [r, g, b] in 0.0-1.0 range
    Transform(Transform),
    Material(Material),
    Enum(String),
}

/// Property metadata (for UI generation)
#[derive(Debug, Clone)]
pub struct PropertyDescriptor {
    pub name: String,
    pub property_type: String,
    pub read_only: bool,
    pub category: String,
}

// NOTE: Spawn helpers (spawn_part, spawn_model, etc.) are in engine/src/classes.rs
// as they depend on engine-specific rendering components.

// ============================================================================
// Assembly Mass Computation (Recursive)
// ============================================================================

/// Compute the recursive assembly mass for an entity by summing:
/// - Direct BasePart children's `mass` values
/// - Nested Model/Folder children's `assembly_mass` values (which are themselves recursive)
/// 
/// This function should be called by a Bevy system that queries the hierarchy.
/// 
/// # Arguments
/// * `entity` - The entity (Model or Folder) to compute mass for
/// * `children_query` - Query for Children component
/// * `base_part_query` - Query for BasePart components
/// * `model_query` - Query for Model components  
/// * `folder_query` - Query for Folder components
/// 
/// # Returns
/// Total mass in kg of all descendant BaseParts
pub fn compute_recursive_assembly_mass(
    entity: Entity,
    children_query: &Query<&Children>,
    base_part_query: &Query<&BasePart>,
    model_query: &Query<&Model>,
    folder_query: &Query<&Folder>,
) -> f32 {
    let mut total_mass = 0.0;
    
    // Get direct children
    if let Ok(children) = children_query.get(entity) {
        for child in children.iter() {
            // Check if child is a BasePart - add its mass directly
            if let Ok(base_part) = base_part_query.get(child) {
                total_mass += base_part.mass;
            }
            // Check if child is a Model - add its assembly_mass (already computed recursively)
            else if let Ok(model) = model_query.get(child) {
                total_mass += model.assembly_mass;
            }
            // Check if child is a Folder - add its assembly_mass (already computed recursively)
            else if let Ok(folder) = folder_query.get(child) {
                total_mass += folder.assembly_mass;
            }
        }
    }
    
    total_mass
}

/// Marker trait for entities that have assembly_mass (Model, Folder)
/// Used by the assembly mass update system to identify containers
pub trait HasAssemblyMass {
    fn get_assembly_mass(&self) -> f32;
    fn set_assembly_mass(&mut self, mass: f32);
}

impl HasAssemblyMass for Model {
    fn get_assembly_mass(&self) -> f32 { self.assembly_mass }
    fn set_assembly_mass(&mut self, mass: f32) { self.assembly_mass = mass; }
}

impl HasAssemblyMass for Folder {
    fn get_assembly_mass(&self) -> f32 { self.assembly_mass }
    fn set_assembly_mass(&mut self, mass: f32) { self.assembly_mass = mass; }
}

