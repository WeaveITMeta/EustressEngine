// ============================================================================
// XR Support - Virtual, Augmented, and Mixed Reality
// ============================================================================
//
// Metaverse-ready XR support for:
// - VR headsets (Meta Quest, Valve Index, PSVR2, Apple Vision Pro)
// - AR devices (HoloLens, Magic Leap, smartphone AR)
// - MR passthrough (Quest 3, Vision Pro)
// - WebXR (browser-based VR/AR)
//
// Table of Contents:
// 1. XR Device Types and Capabilities
// 2. Tracking and Input
// 3. Rendering Configuration
// 4. Spatial Anchors and Persistence
// 5. Multiuser/Social XR
// 6. AR Features (Plane detection, Meshing, etc.)
// ============================================================================

use bevy::prelude::*;
use serde::{Serialize, Deserialize};

// ============================================================================
// 1. XR Device Types and Capabilities
// ============================================================================

/// XR runtime/platform
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum XRRuntime {
    /// OpenXR (cross-platform standard)
    OpenXR,
    /// Meta Quest native
    MetaQuest,
    /// SteamVR/OpenVR
    SteamVR,
    /// Apple visionOS
    VisionOS,
    /// WebXR (browser)
    WebXR,
    /// ARCore (Android)
    ARCore,
    /// ARKit (iOS)
    ARKit,
    /// Windows Mixed Reality
    WMR,
    /// Simulated (for development)
    Simulated,
}

/// XR device category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum XRDeviceType {
    /// Fully immersive VR headset
    VRHeadset,
    /// AR glasses/headset
    ARHeadset,
    /// Mixed reality with passthrough
    MRHeadset,
    /// Smartphone AR
    PhoneAR,
    /// Desktop with VR headset
    PCVR,
    /// Standalone VR (Quest, Pico)
    StandaloneVR,
}

/// XR device capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XRCapabilities {
    /// Device type
    pub device_type: XRDeviceType,
    /// Runtime being used
    pub runtime: XRRuntime,
    /// Supports hand tracking
    pub hand_tracking: bool,
    /// Supports eye tracking
    pub eye_tracking: bool,
    /// Supports face tracking
    pub face_tracking: bool,
    /// Supports body tracking
    pub body_tracking: bool,
    /// Supports passthrough/video see-through
    pub passthrough: bool,
    /// Supports depth sensing
    pub depth_sensing: bool,
    /// Supports spatial anchors
    pub spatial_anchors: bool,
    /// Supports plane detection
    pub plane_detection: bool,
    /// Supports mesh reconstruction
    pub mesh_reconstruction: bool,
    /// Supports scene understanding
    pub scene_understanding: bool,
    /// Maximum refresh rate (Hz)
    pub max_refresh_rate: u32,
    /// Per-eye resolution
    pub eye_resolution: (u32, u32),
    /// Field of view (degrees)
    pub fov_degrees: f32,
    /// Supports foveated rendering
    pub foveated_rendering: bool,
    /// Controller type
    pub controller_type: XRControllerType,
}

impl Default for XRCapabilities {
    fn default() -> Self {
        Self {
            device_type: XRDeviceType::VRHeadset,
            runtime: XRRuntime::OpenXR,
            hand_tracking: false,
            eye_tracking: false,
            face_tracking: false,
            body_tracking: false,
            passthrough: false,
            depth_sensing: false,
            spatial_anchors: false,
            plane_detection: false,
            mesh_reconstruction: false,
            scene_understanding: false,
            max_refresh_rate: 90,
            eye_resolution: (1920, 1920),
            fov_degrees: 100.0,
            foveated_rendering: false,
            controller_type: XRControllerType::Standard6DOF,
        }
    }
}

impl XRCapabilities {
    /// Meta Quest 3 capabilities
    pub fn quest_3() -> Self {
        Self {
            device_type: XRDeviceType::MRHeadset,
            runtime: XRRuntime::MetaQuest,
            hand_tracking: true,
            eye_tracking: true,
            face_tracking: true,
            body_tracking: true,
            passthrough: true,
            depth_sensing: true,
            spatial_anchors: true,
            plane_detection: true,
            mesh_reconstruction: true,
            scene_understanding: true,
            max_refresh_rate: 120,
            eye_resolution: (2064, 2208),
            fov_degrees: 110.0,
            foveated_rendering: true,
            controller_type: XRControllerType::QuestTouch,
        }
    }
    
    /// Apple Vision Pro capabilities
    pub fn vision_pro() -> Self {
        Self {
            device_type: XRDeviceType::MRHeadset,
            runtime: XRRuntime::VisionOS,
            hand_tracking: true,
            eye_tracking: true,
            face_tracking: true,
            body_tracking: false,
            passthrough: true,
            depth_sensing: true,
            spatial_anchors: true,
            plane_detection: true,
            mesh_reconstruction: true,
            scene_understanding: true,
            max_refresh_rate: 100,
            eye_resolution: (3660, 3200),
            fov_degrees: 100.0,
            foveated_rendering: true,
            controller_type: XRControllerType::HandsOnly,
        }
    }
    
    /// Valve Index capabilities
    pub fn valve_index() -> Self {
        Self {
            device_type: XRDeviceType::PCVR,
            runtime: XRRuntime::SteamVR,
            hand_tracking: true,
            eye_tracking: false,
            face_tracking: false,
            body_tracking: true,
            passthrough: false,
            depth_sensing: false,
            spatial_anchors: false,
            plane_detection: false,
            mesh_reconstruction: false,
            scene_understanding: false,
            max_refresh_rate: 144,
            eye_resolution: (1440, 1600),
            fov_degrees: 130.0,
            foveated_rendering: false,
            controller_type: XRControllerType::IndexKnuckles,
        }
    }
}

/// XR controller type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum XRControllerType {
    /// Standard 6DOF controllers
    #[default]
    Standard6DOF,
    /// Meta Quest Touch controllers
    QuestTouch,
    /// Valve Index Knuckles
    IndexKnuckles,
    /// Hands only (no controllers)
    HandsOnly,
    /// 3DOF controllers (rotation only)
    Controller3DOF,
    /// Gaze-based input
    GazeOnly,
}

// ============================================================================
// 2. Tracking and Input
// ============================================================================

/// XR tracking origin mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum XRTrackingOrigin {
    /// Floor level (standing/room-scale)
    #[default]
    Floor,
    /// Eye level (seated)
    EyeLevel,
    /// Device (3DOF, phone AR)
    Device,
    /// Stage (bounded play area)
    Stage,
    /// Unbounded (world-scale AR)
    Unbounded,
}

/// Hand tracking data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandTrackingData {
    /// Which hand
    pub hand: Hand,
    /// Joint positions (25 joints per hand)
    pub joints: [Vec3; 25],
    /// Joint rotations
    pub rotations: [Quat; 25],
    /// Joint radii (for collision)
    pub radii: [f32; 25],
    /// Tracking confidence (0-1)
    pub confidence: f32,
    /// Detected gesture
    pub gesture: Option<HandGesture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hand {
    Left,
    Right,
}

/// Recognized hand gestures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HandGesture {
    /// Open palm
    Open,
    /// Closed fist
    Fist,
    /// Pointing (index finger extended)
    Point,
    /// Pinch (thumb + index)
    Pinch,
    /// Thumbs up
    ThumbsUp,
    /// Peace sign
    Peace,
    /// OK sign
    OK,
    /// Grab (all fingers curled)
    Grab,
}

/// Eye tracking data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EyeTrackingData {
    /// Combined gaze origin
    pub gaze_origin: Vec3,
    /// Combined gaze direction
    pub gaze_direction: Vec3,
    /// Left eye data
    pub left_eye: EyeData,
    /// Right eye data
    pub right_eye: EyeData,
    /// Fixation point in world space
    pub fixation_point: Option<Vec3>,
    /// Tracking confidence
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EyeData {
    pub position: Vec3,
    pub direction: Vec3,
    pub openness: f32,
    pub pupil_diameter: f32,
}

// ============================================================================
// 3. Rendering Configuration
// ============================================================================

/// XR rendering settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XRRenderSettings {
    /// Target refresh rate
    pub target_refresh_rate: u32,
    /// Resolution scale (1.0 = native)
    pub resolution_scale: f32,
    /// Enable foveated rendering
    pub foveated_rendering: bool,
    /// Foveation level (0-4)
    pub foveation_level: u8,
    /// Enable dynamic foveation (eye-tracked)
    pub dynamic_foveation: bool,
    /// Enable reprojection/ASW
    pub reprojection: bool,
    /// Render scale for far objects
    pub far_render_scale: f32,
    /// Enable passthrough
    pub passthrough_enabled: bool,
    /// Passthrough opacity (0-1)
    pub passthrough_opacity: f32,
}

impl Default for XRRenderSettings {
    fn default() -> Self {
        Self {
            target_refresh_rate: 90,
            resolution_scale: 1.0,
            foveated_rendering: true,
            foveation_level: 2,
            dynamic_foveation: true,
            reprojection: true,
            far_render_scale: 0.5,
            passthrough_enabled: false,
            passthrough_opacity: 1.0,
        }
    }
}

// ============================================================================
// 4. Spatial Anchors and Persistence
// ============================================================================

/// Spatial anchor for persistent AR content
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct SpatialAnchor {
    /// Unique anchor ID
    pub id: String,
    /// Anchor position in world space
    pub position: Vec3,
    /// Anchor rotation
    pub rotation: Quat,
    /// Tracking state
    pub tracking_state: AnchorTrackingState,
    /// Cloud anchor ID (for sharing)
    pub cloud_id: Option<String>,
    /// Creation timestamp
    pub created_at: u64,
    /// Last seen timestamp
    pub last_seen: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AnchorTrackingState {
    #[default]
    NotTracking,
    Tracking,
    Limited,
    Paused,
}

/// Cloud anchor for multi-user shared spaces
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudAnchor {
    /// Cloud anchor ID
    pub cloud_id: String,
    /// Local anchor ID
    pub local_id: String,
    /// Hosting state
    pub state: CloudAnchorState,
    /// Expiration time (Unix timestamp)
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloudAnchorState {
    Pending,
    Hosted,
    Resolved,
    Failed,
    Expired,
}

// ============================================================================
// 5. Multiuser/Social XR
// ============================================================================

/// XR avatar representation
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct XRAvatar {
    /// User ID
    pub user_id: String,
    /// Display name
    pub display_name: String,
    /// Head position
    pub head_position: Vec3,
    /// Head rotation
    pub head_rotation: Quat,
    /// Left hand transform
    pub left_hand: Option<Transform>,
    /// Right hand transform
    pub right_hand: Option<Transform>,
    /// Full body pose (if available)
    pub body_pose: Option<BodyPose>,
    /// Voice activity level (0-1)
    pub voice_level: f32,
    /// Avatar model ID
    pub avatar_model: String,
}

/// Full body pose data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyPose {
    /// Spine joints
    pub spine: [Transform; 4],
    /// Left arm joints
    pub left_arm: [Transform; 4],
    /// Right arm joints
    pub right_arm: [Transform; 4],
    /// Left leg joints
    pub left_leg: [Transform; 4],
    /// Right leg joints
    pub right_leg: [Transform; 4],
}

/// Shared XR space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedSpace {
    /// Space ID
    pub space_id: String,
    /// Host user ID
    pub host_id: String,
    /// Connected users
    pub users: Vec<String>,
    /// Shared anchor ID
    pub anchor_id: String,
    /// Space bounds (if defined)
    pub bounds: Option<SpaceBounds>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceBounds {
    pub center: Vec3,
    pub size: Vec3,
    pub boundary_points: Vec<Vec2>,
}

// ============================================================================
// 6. AR Features
// ============================================================================

/// Detected AR plane
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct ARPlane {
    /// Plane ID
    pub id: u64,
    /// Plane center
    pub center: Vec3,
    /// Plane normal
    pub normal: Vec3,
    /// Plane extents
    pub extents: Vec2,
    /// Plane type
    pub plane_type: ARPlaneType,
    /// Boundary polygon (in local space)
    pub boundary: Vec<Vec2>,
    /// Tracking state
    pub tracking_state: AnchorTrackingState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ARPlaneType {
    HorizontalUp,   // Floor, table
    HorizontalDown, // Ceiling
    Vertical,       // Wall
    Unknown,
}

/// AR mesh (reconstructed environment)
#[derive(Component, Debug, Clone)]
pub struct ARMesh {
    /// Mesh ID
    pub id: u64,
    /// Vertices
    pub vertices: Vec<Vec3>,
    /// Indices
    pub indices: Vec<u32>,
    /// Normals
    pub normals: Vec<Vec3>,
    /// Classification per vertex
    pub classification: Vec<ARMeshClassification>,
    /// Last update time
    pub last_update: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ARMeshClassification {
    None,
    Wall,
    Floor,
    Ceiling,
    Table,
    Seat,
    Window,
    Door,
}

/// AR light estimation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ARLightEstimate {
    /// Ambient intensity (lux)
    pub ambient_intensity: f32,
    /// Ambient color temperature (Kelvin)
    pub color_temperature: f32,
    /// Main light direction
    pub main_light_direction: Vec3,
    /// Main light intensity
    pub main_light_intensity: f32,
    /// Spherical harmonics (for IBL)
    pub spherical_harmonics: Option<[Vec3; 9]>,
}

// ============================================================================
// 7. Bevy Resources
// ============================================================================

/// Global XR state resource
#[derive(Resource, Default)]
pub struct XRState {
    /// Whether XR is active
    pub active: bool,
    /// Current capabilities
    pub capabilities: Option<XRCapabilities>,
    /// Render settings
    pub render_settings: XRRenderSettings,
    /// Tracking origin
    pub tracking_origin: XRTrackingOrigin,
    /// Head pose
    pub head_pose: Transform,
    /// Left controller pose
    pub left_controller: Option<Transform>,
    /// Right controller pose
    pub right_controller: Option<Transform>,
    /// Hand tracking data
    pub hand_tracking: Option<(HandTrackingData, HandTrackingData)>,
    /// Eye tracking data
    pub eye_tracking: Option<EyeTrackingData>,
    /// Current frame timing
    pub frame_timing: XRFrameTiming,
}

#[derive(Debug, Clone, Default)]
pub struct XRFrameTiming {
    /// Predicted display time
    pub predicted_display_time: f64,
    /// Frame interval (seconds)
    pub frame_interval: f32,
    /// GPU frame time (ms)
    pub gpu_time_ms: f32,
    /// CPU frame time (ms)
    pub cpu_time_ms: f32,
    /// Compositor time (ms)
    pub compositor_time_ms: f32,
}

// ============================================================================
// 8. Performance Targets for XR
// ============================================================================

pub mod xr_performance {
    /// Target frame time for 90 Hz (11.11ms)
    pub const TARGET_90HZ_MS: f32 = 11.11;
    
    /// Target frame time for 120 Hz (8.33ms)
    pub const TARGET_120HZ_MS: f32 = 8.33;
    
    /// Maximum allowed motion-to-photon latency (ms)
    pub const MAX_LATENCY_MS: f32 = 20.0;
    
    /// Recommended GPU budget per eye (ms)
    pub const GPU_BUDGET_PER_EYE_MS: f32 = 4.0;
    
    /// Point cloud budget for XR (reduced for performance)
    pub const XR_POINT_BUDGET: u32 = 2_000_000;
    
    /// Maximum draw calls for XR
    pub const MAX_DRAW_CALLS: u32 = 500;
}
