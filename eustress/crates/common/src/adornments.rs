//! # Adornment System
//!
//! Meta-entities for editor tooling that are hidden from the Explorer.
//! Adornments provide visual feedback and interaction handles for tools.
//!
//! ## Class Hierarchy
//! ```text
//! Instance
//! └── GuiBase3d (Color, Transparency, Visible)
//!     └── PVAdornment (Adornee)
//!         ├── HandleAdornment (CFrame, AlwaysOnTop, ZIndex, SizeRelativeOffset)
//!         │   ├── BoxHandleAdornment (Size)
//!         │   ├── SphereHandleAdornment (Radius)
//!         │   ├── ConeHandleAdornment (Height, Radius)
//!         │   ├── CylinderHandleAdornment (Height, Radius, InnerRadius, Angle)
//!         │   ├── LineHandleAdornment (Length, Thickness)
//!         │   ├── PyramidHandleAdornment (Size)
//!         │   ├── WireframeHandleAdornment (Scale)
//!         │   └── ImageHandleAdornment (Image, Size)
//!         ├── PartAdornment
//!         │   ├── SelectionBox (LineThickness, SurfaceColor, SurfaceTransparency)
//!         │   ├── SelectionSphere (SurfaceColor, SurfaceTransparency)
//!         │   └── SurfaceSelection (TargetSurface, SurfaceColor, SurfaceTransparency)
//!         └── HandlesBase
//!             ├── ArcHandles (Axes, rotation arcs)
//!             └── Handles (Faces, Style, translation/scale arrows)
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// Core Marker Components
// ============================================================================

/// Marker component for all adornment entities.
/// Entities with this component are hidden from the Explorer when `meta = true`.
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Adornment {
    /// If true, this entity is hidden from the Explorer tree
    pub meta: bool,
}

impl Default for Adornment {
    fn default() -> Self {
        Self { meta: true }
    }
}

/// Links an adornment to its target entity (the "adornee")
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct AdornmentAdornee {
    /// The entity this adornment is attached to
    pub target: Entity,
}

// ============================================================================
// GuiBase3d Properties (inherited by all adornments)
// ============================================================================

/// Base 3D GUI properties inherited by all adornments
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct GuiBase3d {
    /// Adornment color (RGB)
    pub color: Color,
    /// Transparency (0 = opaque, 1 = invisible)
    pub transparency: f32,
    /// Whether the adornment is visible
    pub visible: bool,
}

impl Default for GuiBase3d {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            transparency: 0.0,
            visible: true,
        }
    }
}

// ============================================================================
// HandleAdornment Base Properties
// ============================================================================

/// Culling mode for adornments
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize, Default)]
pub enum AdornCullingMode {
    /// Automatically cull based on distance
    #[default]
    Automatic,
    /// Never cull (always render)
    Never,
}

/// Base properties for all handle adornments
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct HandleAdornment {
    /// Local transform offset from adornee
    pub cframe: Transform,
    /// If true, renders on top of all 3D objects
    pub always_on_top: bool,
    /// Draw order (-1 to 10, higher = in front)
    pub z_index: i32,
    /// Offset as scale of adornee size (0-1 range moves to edge)
    pub size_relative_offset: Vec3,
    /// Culling behavior
    pub adorn_culling_mode: AdornCullingMode,
}

impl Default for HandleAdornment {
    fn default() -> Self {
        Self {
            cframe: Transform::IDENTITY,
            always_on_top: true,
            z_index: 0,
            size_relative_offset: Vec3::ZERO,
            adorn_culling_mode: AdornCullingMode::Automatic,
        }
    }
}

// ============================================================================
// Shading Mode
// ============================================================================

/// Shading mode for handle adornments
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize, Default)]
pub enum AdornShading {
    /// Default PBR shading
    #[default]
    Default,
    /// Flat shading (no lighting)
    Flat,
    /// Unlit (emissive)
    Unlit,
}

// ============================================================================
// Specific Handle Adornment Types
// ============================================================================

/// Box-shaped handle (scale tool corners/edges)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct BoxHandleAdornment {
    /// Size in studs (x, y, z)
    pub size: Vec3,
    /// Shading mode
    pub shading: AdornShading,
}

impl Default for BoxHandleAdornment {
    fn default() -> Self {
        Self {
            size: Vec3::ONE,
            shading: AdornShading::Default,
        }
    }
}

/// Sphere-shaped handle (rotation pivot, scale corners)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SphereHandleAdornment {
    /// Radius in studs
    pub radius: f32,
    /// Shading mode
    pub shading: AdornShading,
}

impl Default for SphereHandleAdornment {
    fn default() -> Self {
        Self {
            radius: 0.5,
            shading: AdornShading::Default,
        }
    }
}

/// Cone-shaped handle (move tool axis arrows)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ConeHandleAdornment {
    /// Height in studs (cone points along local +Y)
    pub height: f32,
    /// Base radius in studs
    pub radius: f32,
    /// Shading mode
    pub shading: AdornShading,
}

impl Default for ConeHandleAdornment {
    fn default() -> Self {
        Self {
            height: 1.0,
            radius: 0.3,
            shading: AdornShading::Default,
        }
    }
}

/// Cylinder-shaped handle (move tool axis shafts, rotation rings)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct CylinderHandleAdornment {
    /// Height in studs (cylinder axis along local Y)
    pub height: f32,
    /// Outer radius in studs
    pub radius: f32,
    /// Inner radius for torus/ring shapes (0 = solid cylinder)
    pub inner_radius: f32,
    /// Arc angle in degrees (360 = full cylinder, 90 = quarter)
    pub angle: f32,
    /// Shading mode
    pub shading: AdornShading,
}

impl Default for CylinderHandleAdornment {
    fn default() -> Self {
        Self {
            height: 2.0,
            radius: 0.1,
            inner_radius: 0.0,
            angle: 360.0,
            shading: AdornShading::Default,
        }
    }
}

/// Line-shaped handle (axis indicators, connections)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct LineHandleAdornment {
    /// Length in studs (extends along local +Y)
    pub length: f32,
    /// Line thickness in studs
    pub thickness: f32,
}

impl Default for LineHandleAdornment {
    fn default() -> Self {
        Self {
            length: 5.0,
            thickness: 0.05,
        }
    }
}

/// Pyramid-shaped handle (directional indicators)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct PyramidHandleAdornment {
    /// Size: base width, height, base depth
    pub size: Vec3,
    /// Shading mode
    pub shading: AdornShading,
}

impl Default for PyramidHandleAdornment {
    fn default() -> Self {
        Self {
            size: Vec3::ONE,
            shading: AdornShading::Default,
        }
    }
}

/// Wireframe box handle (bounding box visualization)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct WireframeHandleAdornment {
    /// Scale relative to adornee size
    pub scale: Vec3,
}

impl Default for WireframeHandleAdornment {
    fn default() -> Self {
        Self { scale: Vec3::ONE }
    }
}

/// Image-based handle (custom icons/textures)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ImageHandleAdornment {
    /// Asset path to image texture
    pub image: String,
    /// Size in studs (width, height)
    pub size: Vec2,
}

impl Default for ImageHandleAdornment {
    fn default() -> Self {
        Self {
            image: String::new(),
            size: Vec2::ONE,
        }
    }
}

// ============================================================================
// Selection Adornments
// ============================================================================

/// Wireframe box highlighting selected entities
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SelectionBox {
    /// Thickness of wireframe lines in studs
    pub line_thickness: f32,
    /// Color of selection highlight
    pub surface_color: Color,
    /// Surface transparency (0 = opaque fill, 1 = wireframe only)
    pub surface_transparency: f32,
}

impl Default for SelectionBox {
    fn default() -> Self {
        Self {
            line_thickness: 0.05,
            surface_color: Color::srgb(0.0, 0.6, 1.0), // Eustress blue
            surface_transparency: 0.8,
        }
    }
}

/// Wireframe sphere highlighting selected spherical entities
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SelectionSphere {
    /// Color of selection highlight
    pub surface_color: Color,
    /// Surface transparency (0 = opaque fill, 1 = wireframe only)
    pub surface_transparency: f32,
}

impl Default for SelectionSphere {
    fn default() -> Self {
        Self {
            surface_color: Color::srgb(0.0, 0.6, 1.0),
            surface_transparency: 0.8,
        }
    }
}

/// Surface type for SurfaceSelection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize, Default)]
pub enum SurfaceType {
    #[default]
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
}

/// Highlights a specific face/surface of an entity
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SurfaceSelection {
    /// Which surface to highlight
    pub target_surface: SurfaceType,
    /// Color of surface highlight
    pub surface_color: Color,
    /// Surface transparency (0 = opaque, 1 = invisible)
    pub surface_transparency: f32,
}

impl Default for SurfaceSelection {
    fn default() -> Self {
        Self {
            target_surface: SurfaceType::Top,
            surface_color: Color::srgb(0.0, 0.6, 1.0),
            surface_transparency: 0.5,
        }
    }
}

// ============================================================================
// Handle Systems (ArcHandles, Handles)
// ============================================================================

/// Axis enum for handle systems
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// Face enum for directional handles
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize, Hash)]
pub enum Face {
    Right,  // +X
    Left,   // -X
    Top,    // +Y
    Bottom, // -Y
    Back,   // +Z
    Front,  // -Z
}

/// Handle style for Handles component
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize, Default)]
pub enum HandleStyle {
    /// Arrow handles for movement
    #[default]
    Movement,
    /// Cube handles for resizing
    Resize,
}

/// 3D rotation arcs for X/Y/Z axes (Rotate Tool)
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ArcHandles {
    /// Which axes to display arcs for
    pub axes: Vec<Axis>,
    /// Radius of rotation arcs in studs
    pub arc_radius: f32,
    /// Thickness of arc tubes
    pub arc_thickness: f32,
    /// Arc sweep angle in degrees (360 = full circle)
    pub arc_angle: f32,
    /// X axis color (red)
    pub x_axis_color: Color,
    /// Y axis color (green)
    pub y_axis_color: Color,
    /// Z axis color (blue)
    pub z_axis_color: Color,
    /// Brightness multiplier when hovered
    pub hover_brightness: f32,
}

impl Default for ArcHandles {
    fn default() -> Self {
        Self {
            axes: vec![Axis::X, Axis::Y, Axis::Z],
            arc_radius: 2.0,
            arc_thickness: 0.08,
            arc_angle: 360.0,
            x_axis_color: Color::srgb(1.0, 0.2, 0.2),
            y_axis_color: Color::srgb(0.2, 1.0, 0.2),
            z_axis_color: Color::srgb(0.2, 0.2, 1.0),
            hover_brightness: 1.5,
        }
    }
}

/// 6-directional handles for Move/Scale tools
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Handles {
    /// Which faces to display handles for
    pub faces: Vec<Face>,
    /// Handle style (Movement arrows or Resize cubes)
    pub style: HandleStyle,
    /// Size of handle geometry in studs
    pub handle_size: f32,
    /// Length of axis shafts (Movement style)
    pub shaft_length: f32,
    /// Radius of axis shafts
    pub shaft_radius: f32,
    /// X axis color (red)
    pub x_axis_color: Color,
    /// Y axis color (green)
    pub y_axis_color: Color,
    /// Z axis color (blue)
    pub z_axis_color: Color,
    /// XY plane color (yellow)
    pub xy_plane_color: Color,
    /// XZ plane color (magenta)
    pub xz_plane_color: Color,
    /// YZ plane color (cyan)
    pub yz_plane_color: Color,
    /// Center handle color (white)
    pub center_color: Color,
    /// Brightness multiplier when hovered
    pub hover_brightness: f32,
}

impl Default for Handles {
    fn default() -> Self {
        Self {
            faces: vec![
                Face::Right,
                Face::Top,
                Face::Back,
                Face::Left,
                Face::Bottom,
                Face::Front,
            ],
            style: HandleStyle::Movement,
            handle_size: 0.3,
            shaft_length: 2.0,
            shaft_radius: 0.05,
            x_axis_color: Color::srgb(1.0, 0.2, 0.2),
            y_axis_color: Color::srgb(0.2, 1.0, 0.2),
            z_axis_color: Color::srgb(0.2, 0.2, 1.0),
            xy_plane_color: Color::srgb(1.0, 1.0, 0.2),
            xz_plane_color: Color::srgb(1.0, 0.2, 1.0),
            yz_plane_color: Color::srgb(0.2, 1.0, 1.0),
            center_color: Color::WHITE,
            hover_brightness: 1.5,
        }
    }
}

// ============================================================================
// Smart Grid System
// ============================================================================

/// Corner enum for GridSensor tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize, Default, Hash)]
pub enum Corner {
    #[default]
    None,
    TopFrontLeft,
    TopFrontRight,
    TopBackLeft,
    TopBackRight,
    BottomFrontLeft,
    BottomFrontRight,
    BottomBackLeft,
    BottomBackRight,
}

/// Guide type for AlignmentGuide
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize, Default)]
pub enum GuideType {
    #[default]
    Edge,
    Center,
    Corner,
    Surface,
}

/// Snap type for SnapIndicator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize, Default)]
pub enum SnapType {
    #[default]
    Grid,
    Edge,
    Center,
    Corner,
    Surface,
}

/// GridSensor - Dynamic corner indicator that tracks nearest grid/alignment point
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct GridSensor {
    /// Detection radius in studs
    pub sensor_radius: f32,
    /// Distance threshold for snapping
    pub snap_distance: f32,
    /// Visual size of corner indicator
    pub corner_size: f32,
    /// Which corner is currently active (nearest to mouse)
    pub active_corner: Corner,
    /// Thickness of corner lines
    pub line_thickness: f32,
    /// Length of corner indicator lines
    pub line_length: f32,
    /// Color when no snap available
    pub idle_color: Color,
    /// Color when hovering
    pub hover_color: Color,
    /// Color when snap detected
    pub snap_color: Color,
}

impl Default for GridSensor {
    fn default() -> Self {
        Self {
            sensor_radius: 2.0,
            snap_distance: 0.5,
            corner_size: 0.15,
            active_corner: Corner::None,
            line_thickness: 0.02,
            line_length: 0.3,
            idle_color: Color::srgb(0.5, 0.5, 0.5),
            hover_color: Color::srgb(0.0, 0.6, 1.0),
            snap_color: Color::srgb(0.2, 1.0, 0.2),
        }
    }
}

/// AlignmentGuide - Visual line showing edge/center alignment with other objects
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct AlignmentGuide {
    /// Guide type determines color and behavior
    pub guide_type: GuideType,
    /// Axis this guide aligns on
    pub axis: Axis,
    /// Line start point
    pub start_point: Vec3,
    /// Line end point
    pub end_point: Vec3,
    /// Line thickness in studs
    pub thickness: f32,
    /// Dash length (0 = solid line)
    pub dash_length: f32,
    /// Gap between dashes
    pub dash_gap: f32,
    /// Color for edge alignment
    pub edge_color: Color,
    /// Color for center alignment
    pub center_color: Color,
    /// Color for corner alignment
    pub corner_color: Color,
    /// Color for surface alignment
    pub surface_color: Color,
    /// Pulse animation speed (Hz)
    pub pulse_speed: f32,
    /// Fade out duration on release (seconds)
    pub fade_duration: f32,
}

impl Default for AlignmentGuide {
    fn default() -> Self {
        Self {
            guide_type: GuideType::Edge,
            axis: Axis::X,
            start_point: Vec3::ZERO,
            end_point: Vec3::ZERO,
            thickness: 0.02,
            dash_length: 0.0,
            dash_gap: 0.0,
            edge_color: Color::srgb(1.0, 0.2, 0.2),
            center_color: Color::srgb(0.2, 1.0, 0.2),
            corner_color: Color::srgb(0.2, 0.2, 1.0),
            surface_color: Color::srgb(0.2, 1.0, 1.0),
            pulse_speed: 2.0,
            fade_duration: 0.3,
        }
    }
}

/// SnapIndicator - Ghost preview showing snapped position before release
#[derive(Component, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SnapIndicator {
    /// Target position for snap
    pub target_position: Vec3,
    /// Target rotation for snap (quaternion)
    pub target_rotation: Quat,
    /// Snap type determines visual style
    pub snap_type: SnapType,
    /// Confidence affects opacity (0-1)
    pub confidence: f32,
    /// Show as wireframe (true) or solid ghost (false)
    pub wireframe: bool,
    /// Wireframe line thickness
    pub wireframe_thickness: f32,
    /// Transparency when showing solid ghost
    pub ghost_transparency: f32,
    /// Enable subtle pulse animation
    pub pulse_enabled: bool,
    /// Scale variation during pulse
    pub pulse_amplitude: f32,
    /// Pulse frequency (Hz)
    pub pulse_speed: f32,
    /// Duration of merge animation (seconds)
    pub merge_duration: f32,
}

impl Default for SnapIndicator {
    fn default() -> Self {
        Self {
            target_position: Vec3::ZERO,
            target_rotation: Quat::IDENTITY,
            snap_type: SnapType::Grid,
            confidence: 1.0,
            wireframe: true,
            wireframe_thickness: 0.02,
            ghost_transparency: 0.7,
            pulse_enabled: true,
            pulse_amplitude: 0.05,
            pulse_speed: 3.0,
            merge_duration: 0.15,
        }
    }
}

/// Smart Grid configuration resource
#[derive(Resource, Debug, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct SmartGridConfig {
    /// Enable smart grid system
    pub enabled: bool,
    /// Distance threshold for snapping (studs)
    pub snap_distance: f32,
    /// Detection radius for grid sensor
    pub sensor_radius: f32,
    /// Show red/green alignment lines
    pub show_alignment_guides: bool,
    /// Show ghost preview
    pub show_snap_preview: bool,
    /// World grid size (studs)
    pub grid_size: f32,
    /// Subdivisions for fine snapping
    pub grid_subdivisions: u32,
    /// Maximum simultaneous alignment guides
    pub max_guides: usize,
    /// Color for edge alignment guides
    pub guide_color_edge: Color,
    /// Color for center alignment guides
    pub guide_color_center: Color,
    /// Color for corner alignment guides
    pub guide_color_corner: Color,
}

impl Default for SmartGridConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            snap_distance: 0.5,
            sensor_radius: 2.0,
            show_alignment_guides: true,
            show_snap_preview: true,
            grid_size: 1.0,
            grid_subdivisions: 4,
            max_guides: 8,
            guide_color_edge: Color::srgb(1.0, 0.2, 0.2),
            guide_color_center: Color::srgb(0.2, 1.0, 0.2),
            guide_color_corner: Color::srgb(0.2, 0.2, 1.0),
        }
    }
}

/// Key points for an entity (corners, face centers, center)
#[derive(Debug, Clone, Reflect)]
pub struct KeyPoints {
    /// 8 corners of AABB
    pub corners: [Vec3; 8],
    /// 6 face centers
    pub face_centers: [Vec3; 6],
    /// Object center
    pub center: Vec3,
}

impl KeyPoints {
    /// Compute key points from an AABB
    pub fn from_aabb(min: Vec3, max: Vec3) -> Self {
        let center = (min + max) * 0.5;
        
        // 8 corners
        let corners = [
            Vec3::new(min.x, max.y, min.z), // TopBackLeft
            Vec3::new(max.x, max.y, min.z), // TopBackRight
            Vec3::new(min.x, max.y, max.z), // TopFrontLeft
            Vec3::new(max.x, max.y, max.z), // TopFrontRight
            Vec3::new(min.x, min.y, min.z), // BottomBackLeft
            Vec3::new(max.x, min.y, min.z), // BottomBackRight
            Vec3::new(min.x, min.y, max.z), // BottomFrontLeft
            Vec3::new(max.x, min.y, max.z), // BottomFrontRight
        ];
        
        // 6 face centers
        let face_centers = [
            Vec3::new(max.x, center.y, center.z), // Right (+X)
            Vec3::new(min.x, center.y, center.z), // Left (-X)
            Vec3::new(center.x, max.y, center.z), // Top (+Y)
            Vec3::new(center.x, min.y, center.z), // Bottom (-Y)
            Vec3::new(center.x, center.y, max.z), // Front (+Z)
            Vec3::new(center.x, center.y, min.z), // Back (-Z)
        ];
        
        Self {
            corners,
            face_centers,
            center,
        }
    }
    
    /// Get all 15 key points as an iterator
    pub fn all_points(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.corners.iter()
            .chain(self.face_centers.iter())
            .chain(std::iter::once(&self.center))
            .copied()
    }
    
    /// Find the corner nearest to a world position
    pub fn nearest_corner(&self, pos: Vec3) -> (Corner, Vec3, f32) {
        let corners_with_enum = [
            (Corner::TopBackLeft, self.corners[0]),
            (Corner::TopBackRight, self.corners[1]),
            (Corner::TopFrontLeft, self.corners[2]),
            (Corner::TopFrontRight, self.corners[3]),
            (Corner::BottomBackLeft, self.corners[4]),
            (Corner::BottomBackRight, self.corners[5]),
            (Corner::BottomFrontLeft, self.corners[6]),
            (Corner::BottomFrontRight, self.corners[7]),
        ];
        
        corners_with_enum
            .into_iter()
            .map(|(corner, corner_pos)| (corner, corner_pos, pos.distance(corner_pos)))
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((Corner::None, Vec3::ZERO, f32::MAX))
    }
}

// ============================================================================
// Adornment Events
// ============================================================================

/// Event fired when mouse enters an adornment
#[derive(Event, Message, Debug, Clone)]
pub struct AdornmentMouseEnter {
    pub adornment: Entity,
    pub axis: Option<Axis>,
    pub face: Option<Face>,
}

/// Event fired when mouse leaves an adornment
#[derive(Event, Message, Debug, Clone)]
pub struct AdornmentMouseLeave {
    pub adornment: Entity,
}

/// Event fired when mouse button is pressed on an adornment
#[derive(Event, Message, Debug, Clone)]
pub struct AdornmentMouseDown {
    pub adornment: Entity,
    pub axis: Option<Axis>,
    pub face: Option<Face>,
}

/// Event fired when mouse button is released on an adornment
#[derive(Event, Message, Debug, Clone)]
pub struct AdornmentMouseUp {
    pub adornment: Entity,
    pub axis: Option<Axis>,
    pub face: Option<Face>,
}

/// Event fired during mouse drag on an adornment
#[derive(Event, Message, Debug, Clone)]
pub struct AdornmentMouseDrag {
    pub adornment: Entity,
    pub axis: Option<Axis>,
    pub face: Option<Face>,
    /// For rotation: angle delta in radians
    /// For translation/scale: distance delta in studs
    pub delta: f32,
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin that registers all adornment components and events
pub struct AdornmentPlugin;

impl Plugin for AdornmentPlugin {
    fn build(&self, app: &mut App) {
        app
            // Register core adornment components
            .register_type::<Adornment>()
            .register_type::<AdornmentAdornee>()
            .register_type::<GuiBase3d>()
            .register_type::<HandleAdornment>()
            // Handle adornment types
            .register_type::<BoxHandleAdornment>()
            .register_type::<SphereHandleAdornment>()
            .register_type::<ConeHandleAdornment>()
            .register_type::<CylinderHandleAdornment>()
            .register_type::<LineHandleAdornment>()
            .register_type::<PyramidHandleAdornment>()
            .register_type::<WireframeHandleAdornment>()
            .register_type::<ImageHandleAdornment>()
            // Selection adornments
            .register_type::<SelectionBox>()
            .register_type::<SelectionSphere>()
            .register_type::<SurfaceSelection>()
            // Handle systems
            .register_type::<ArcHandles>()
            .register_type::<Handles>()
            // Smart Grid components
            .register_type::<GridSensor>()
            .register_type::<AlignmentGuide>()
            .register_type::<SnapIndicator>()
            .register_type::<SmartGridConfig>()
            .register_type::<KeyPoints>()
            // Smart Grid resource
            .init_resource::<SmartGridConfig>()
            // Register events
            .add_message::<AdornmentMouseEnter>()
            .add_message::<AdornmentMouseLeave>()
            .add_message::<AdornmentMouseDown>()
            .add_message::<AdornmentMouseUp>()
            .add_message::<AdornmentMouseDrag>();
    }
}
