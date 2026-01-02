//! # Animation Service
//! 
//! Comprehensive animation system inspired by AAA games like Uncharted 4 and GTA V.
//! 
//! ## Architecture
//! 
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         AnimationService                                 │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐ │
//! │  │ AnimationGraph│  │ BlendTree    │  │ StateMachine │  │ IK System   │ │
//! │  │ (layers)     │  │ (1D/2D blend)│  │ (transitions)│  │ (foot/hand) │ │
//! │  └──────────────┘  └──────────────┘  └──────────────┘  └─────────────┘ │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐ │
//! │  │ Locomotion   │  │ Upper Body   │  │ Additive     │  │ Procedural  │ │
//! │  │ (walk/run)   │  │ (aim/gesture)│  │ (breathing)  │  │ (physics)   │ │
//! │  └──────────────┘  └──────────────┘  └──────────────┘  └─────────────┘ │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//! 
//! ## Features
//! 
//! - **Blend Trees**: 1D/2D animation blending based on speed/direction
//! - **State Machine**: Smooth transitions between animation states
//! - **Layered Animation**: Full body, upper body, additive layers
//! - **Inverse Kinematics**: Foot placement, hand targeting
//! - **Procedural Animation**: Physics-driven secondary motion
//! - **Root Motion**: Animation-driven movement

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Animation Service Resource
// ============================================================================

/// Global animation service configuration
#[derive(Resource, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct AnimationService {
    /// Global animation speed multiplier
    pub time_scale: f32,
    /// Enable IK foot placement
    pub enable_foot_ik: bool,
    /// Enable procedural animation
    pub enable_procedural: bool,
    /// Default blend time for transitions
    pub default_blend_time: f32,
    /// Maximum active animations per entity
    pub max_active_animations: usize,
}

impl Default for AnimationService {
    fn default() -> Self {
        Self {
            time_scale: 1.0,
            enable_foot_ik: true,
            enable_procedural: true,
            default_blend_time: 0.2,
            max_active_animations: 8,
        }
    }
}

// ============================================================================
// Humanoid Rig Definition
// ============================================================================

/// Standard humanoid bone names (compatible with Mixamo, UE4, Unity rigs)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
pub enum HumanoidBone {
    // Spine
    Hips,
    Spine,
    Spine1,
    Spine2,
    Chest,
    Neck,
    Head,
    
    // Left Arm
    LeftShoulder,
    LeftUpperArm,
    LeftLowerArm,
    LeftHand,
    
    // Right Arm
    RightShoulder,
    RightUpperArm,
    RightLowerArm,
    RightHand,
    
    // Left Leg
    LeftUpperLeg,
    LeftLowerLeg,
    LeftFoot,
    LeftToe,
    
    // Right Leg
    RightUpperLeg,
    RightLowerLeg,
    RightFoot,
    RightToe,
}

impl HumanoidBone {
    /// Get parent bone in hierarchy
    pub fn parent(&self) -> Option<HumanoidBone> {
        use HumanoidBone::*;
        match self {
            Hips => None,
            Spine => Some(Hips),
            Spine1 => Some(Spine),
            Spine2 => Some(Spine1),
            Chest => Some(Spine2),
            Neck => Some(Chest),
            Head => Some(Neck),
            
            LeftShoulder => Some(Chest),
            LeftUpperArm => Some(LeftShoulder),
            LeftLowerArm => Some(LeftUpperArm),
            LeftHand => Some(LeftLowerArm),
            
            RightShoulder => Some(Chest),
            RightUpperArm => Some(RightShoulder),
            RightLowerArm => Some(RightUpperArm),
            RightHand => Some(RightLowerArm),
            
            LeftUpperLeg => Some(Hips),
            LeftLowerLeg => Some(LeftUpperLeg),
            LeftFoot => Some(LeftLowerLeg),
            LeftToe => Some(LeftFoot),
            
            RightUpperLeg => Some(Hips),
            RightLowerLeg => Some(RightUpperLeg),
            RightFoot => Some(RightLowerLeg),
            RightToe => Some(RightFoot),
        }
    }
    
    /// Is this a leg bone?
    pub fn is_leg(&self) -> bool {
        use HumanoidBone::*;
        matches!(self, 
            LeftUpperLeg | LeftLowerLeg | LeftFoot | LeftToe |
            RightUpperLeg | RightLowerLeg | RightFoot | RightToe
        )
    }
    
    /// Is this an arm bone?
    pub fn is_arm(&self) -> bool {
        use HumanoidBone::*;
        matches!(self,
            LeftShoulder | LeftUpperArm | LeftLowerArm | LeftHand |
            RightShoulder | RightUpperArm | RightLowerArm | RightHand
        )
    }
}

/// Humanoid rig component - maps bone names to entities
#[derive(Component, Reflect, Clone, Debug, Default)]
#[reflect(Component)]
pub struct HumanoidRig {
    /// Bone entity mapping
    #[reflect(ignore)]
    pub bones: HashMap<HumanoidBone, Entity>,
    /// Bind pose transforms (T-pose)
    #[reflect(ignore)]
    pub bind_poses: HashMap<HumanoidBone, Transform>,
    /// Total height of the rig
    pub height: f32,
    /// Arm span
    pub arm_span: f32,
    /// Leg length
    pub leg_length: f32,
}

impl HumanoidRig {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add a bone to the rig
    pub fn with_bone(mut self, bone: HumanoidBone, entity: Entity, bind_pose: Transform) -> Self {
        self.bones.insert(bone, entity);
        self.bind_poses.insert(bone, bind_pose);
        self
    }
    
    /// Get bone entity
    pub fn get_bone(&self, bone: HumanoidBone) -> Option<Entity> {
        self.bones.get(&bone).copied()
    }
}

// ============================================================================
// Animation State Machine
// ============================================================================

/// Animation state identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
pub enum AnimationState {
    // Locomotion
    Idle,
    Walk,
    Run,
    Sprint,
    
    // Jumping
    JumpStart,
    JumpAir,
    JumpLand,
    
    // Falling
    FallStart,
    Falling,
    FallLand,
    
    // Combat/Actions
    Crouch,
    Slide,
    Roll,
    
    // Custom states
    Custom(String),
}

impl Default for AnimationState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Transition between animation states
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct AnimationTransition {
    /// Source state
    pub from: AnimationState,
    /// Target state
    pub to: AnimationState,
    /// Blend duration in seconds
    pub blend_time: f32,
    /// Transition curve
    pub curve: TransitionCurve,
    /// Can interrupt other transitions
    pub can_interrupt: bool,
}

/// Transition easing curve
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Reflect)]
pub enum TransitionCurve {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Smooth,
}

impl TransitionCurve {
    pub fn evaluate(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            Self::Smooth => t * t * (3.0 - 2.0 * t), // Smoothstep
        }
    }
}

/// Animation state machine component
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct AnimationStateMachine {
    /// Current state
    pub current_state: AnimationState,
    /// Previous state (for blending)
    pub previous_state: AnimationState,
    /// Transition progress (0.0 - 1.0)
    pub transition_progress: f32,
    /// Current transition (if any)
    #[reflect(ignore)]
    pub active_transition: Option<AnimationTransition>,
    /// Available transitions
    #[reflect(ignore)]
    pub transitions: Vec<AnimationTransition>,
    /// State entry time
    pub state_time: f32,
}

impl Default for AnimationStateMachine {
    fn default() -> Self {
        Self {
            current_state: AnimationState::Idle,
            previous_state: AnimationState::Idle,
            transition_progress: 1.0,
            active_transition: None,
            transitions: Self::default_transitions(),
            state_time: 0.0,
        }
    }
}

impl AnimationStateMachine {
    /// Create default locomotion transitions
    fn default_transitions() -> Vec<AnimationTransition> {
        vec![
            // Idle <-> Walk
            AnimationTransition {
                from: AnimationState::Idle,
                to: AnimationState::Walk,
                blend_time: 0.2,
                curve: TransitionCurve::EaseInOut,
                can_interrupt: true,
            },
            AnimationTransition {
                from: AnimationState::Walk,
                to: AnimationState::Idle,
                blend_time: 0.25,
                curve: TransitionCurve::EaseOut,
                can_interrupt: true,
            },
            // Walk <-> Run
            AnimationTransition {
                from: AnimationState::Walk,
                to: AnimationState::Run,
                blend_time: 0.15,
                curve: TransitionCurve::Linear,
                can_interrupt: true,
            },
            AnimationTransition {
                from: AnimationState::Run,
                to: AnimationState::Walk,
                blend_time: 0.2,
                curve: TransitionCurve::EaseOut,
                can_interrupt: true,
            },
            // Run <-> Sprint
            AnimationTransition {
                from: AnimationState::Run,
                to: AnimationState::Sprint,
                blend_time: 0.1,
                curve: TransitionCurve::Linear,
                can_interrupt: true,
            },
            AnimationTransition {
                from: AnimationState::Sprint,
                to: AnimationState::Run,
                blend_time: 0.15,
                curve: TransitionCurve::EaseOut,
                can_interrupt: true,
            },
            // Any -> Jump
            AnimationTransition {
                from: AnimationState::Idle,
                to: AnimationState::JumpStart,
                blend_time: 0.1,
                curve: TransitionCurve::EaseIn,
                can_interrupt: false,
            },
            AnimationTransition {
                from: AnimationState::Walk,
                to: AnimationState::JumpStart,
                blend_time: 0.1,
                curve: TransitionCurve::EaseIn,
                can_interrupt: false,
            },
            AnimationTransition {
                from: AnimationState::Run,
                to: AnimationState::JumpStart,
                blend_time: 0.1,
                curve: TransitionCurve::EaseIn,
                can_interrupt: false,
            },
            // Jump sequence
            AnimationTransition {
                from: AnimationState::JumpStart,
                to: AnimationState::JumpAir,
                blend_time: 0.05,
                curve: TransitionCurve::Linear,
                can_interrupt: false,
            },
            AnimationTransition {
                from: AnimationState::JumpAir,
                to: AnimationState::JumpLand,
                blend_time: 0.1,
                curve: TransitionCurve::EaseOut,
                can_interrupt: false,
            },
            AnimationTransition {
                from: AnimationState::JumpLand,
                to: AnimationState::Idle,
                blend_time: 0.2,
                curve: TransitionCurve::EaseOut,
                can_interrupt: true,
            },
            // Fall sequence
            AnimationTransition {
                from: AnimationState::JumpAir,
                to: AnimationState::Falling,
                blend_time: 0.3,
                curve: TransitionCurve::Linear,
                can_interrupt: true,
            },
            AnimationTransition {
                from: AnimationState::Falling,
                to: AnimationState::FallLand,
                blend_time: 0.1,
                curve: TransitionCurve::EaseOut,
                can_interrupt: false,
            },
        ]
    }
    
    /// Request a state transition
    pub fn request_transition(&mut self, target: AnimationState) -> bool {
        if self.current_state == target {
            return false;
        }
        
        // Find valid transition
        if let Some(transition) = self.transitions.iter().find(|t| {
            t.from == self.current_state && t.to == target
        }).cloned() {
            // Check if we can interrupt current transition
            if self.active_transition.is_some() {
                if let Some(ref active) = self.active_transition {
                    if !active.can_interrupt {
                        return false;
                    }
                }
            }
            
            self.previous_state = self.current_state.clone();
            self.current_state = target;
            self.transition_progress = 0.0;
            self.active_transition = Some(transition);
            self.state_time = 0.0;
            true
        } else {
            false
        }
    }
    
    /// Update transition progress
    pub fn update(&mut self, delta: f32) {
        self.state_time += delta;
        
        if let Some(ref transition) = self.active_transition {
            self.transition_progress += delta / transition.blend_time;
            if self.transition_progress >= 1.0 {
                self.transition_progress = 1.0;
                self.active_transition = None;
            }
        }
    }
    
    /// Get blend weight for current transition
    pub fn get_blend_weight(&self) -> f32 {
        if let Some(ref transition) = self.active_transition {
            transition.curve.evaluate(self.transition_progress)
        } else {
            1.0
        }
    }
}

// ============================================================================
// Blend Tree (1D/2D Animation Blending)
// ============================================================================

/// 1D Blend Tree - blends animations based on a single parameter
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct BlendTree1D {
    /// Parameter name (e.g., "speed")
    pub parameter: String,
    /// Blend nodes (threshold, animation_name)
    pub nodes: Vec<(f32, String)>,
}

impl BlendTree1D {
    pub fn new(parameter: impl Into<String>) -> Self {
        Self {
            parameter: parameter.into(),
            nodes: Vec::new(),
        }
    }
    
    /// Add a blend node
    pub fn with_node(mut self, threshold: f32, animation: impl Into<String>) -> Self {
        self.nodes.push((threshold, animation.into()));
        self.nodes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        self
    }
    
    /// Get blend weights for a parameter value
    pub fn get_weights(&self, value: f32) -> Vec<(String, f32)> {
        if self.nodes.is_empty() {
            return vec![];
        }
        
        if self.nodes.len() == 1 {
            return vec![(self.nodes[0].1.clone(), 1.0)];
        }
        
        // Find surrounding nodes
        let mut lower_idx = 0;
        let mut upper_idx = 0;
        
        for (i, (threshold, _)) in self.nodes.iter().enumerate() {
            if *threshold <= value {
                lower_idx = i;
            }
            if *threshold >= value && upper_idx == 0 {
                upper_idx = i;
                break;
            }
        }
        
        if lower_idx == upper_idx {
            return vec![(self.nodes[lower_idx].1.clone(), 1.0)];
        }
        
        // Interpolate between nodes
        let lower = &self.nodes[lower_idx];
        let upper = &self.nodes[upper_idx];
        let t = (value - lower.0) / (upper.0 - lower.0);
        
        vec![
            (lower.1.clone(), 1.0 - t),
            (upper.1.clone(), t),
        ]
    }
}

/// 2D Blend Tree - blends animations based on two parameters (e.g., velocity X/Y)
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct BlendTree2D {
    /// X parameter name (e.g., "velocity_x")
    pub parameter_x: String,
    /// Y parameter name (e.g., "velocity_y")
    pub parameter_y: String,
    /// Blend nodes (x, y, animation_name)
    pub nodes: Vec<(f32, f32, String)>,
}

impl BlendTree2D {
    pub fn new(param_x: impl Into<String>, param_y: impl Into<String>) -> Self {
        Self {
            parameter_x: param_x.into(),
            parameter_y: param_y.into(),
            nodes: Vec::new(),
        }
    }
    
    /// Add a blend node
    pub fn with_node(mut self, x: f32, y: f32, animation: impl Into<String>) -> Self {
        self.nodes.push((x, y, animation.into()));
        self
    }
    
    /// Create a standard 8-direction locomotion blend tree
    pub fn locomotion_8dir() -> Self {
        Self::new("velocity_x", "velocity_z")
            .with_node(0.0, 0.0, "idle")
            .with_node(0.0, 1.0, "walk_forward")
            .with_node(0.0, -1.0, "walk_backward")
            .with_node(1.0, 0.0, "walk_right")
            .with_node(-1.0, 0.0, "walk_left")
            .with_node(0.707, 0.707, "walk_forward_right")
            .with_node(-0.707, 0.707, "walk_forward_left")
            .with_node(0.707, -0.707, "walk_backward_right")
            .with_node(-0.707, -0.707, "walk_backward_left")
    }
    
    /// Get blend weights using inverse distance weighting
    pub fn get_weights(&self, x: f32, y: f32) -> Vec<(String, f32)> {
        if self.nodes.is_empty() {
            return vec![];
        }
        
        // Calculate distances and weights
        let mut weights: Vec<(String, f32)> = Vec::new();
        let mut total_weight = 0.0;
        
        for (nx, ny, name) in &self.nodes {
            let dx = x - nx;
            let dy = y - ny;
            let dist = (dx * dx + dy * dy).sqrt();
            
            // Inverse distance weighting with small epsilon to avoid division by zero
            let weight = 1.0 / (dist + 0.001);
            weights.push((name.clone(), weight));
            total_weight += weight;
        }
        
        // Normalize weights
        for (_, w) in &mut weights {
            *w /= total_weight;
        }
        
        // Filter out very small weights
        weights.retain(|(_, w)| *w > 0.01);
        
        weights
    }
}

// ============================================================================
// Locomotion Controller
// ============================================================================

/// Locomotion parameters for animation blending
#[derive(Component, Reflect, Clone, Debug, Default)]
#[reflect(Component)]
pub struct LocomotionController {
    /// Current movement speed (0.0 - 1.0 normalized)
    pub speed: f32,
    /// Movement direction relative to facing (radians)
    pub direction: f32,
    /// Horizontal velocity for strafing
    pub strafe: f32,
    /// Is character grounded
    pub grounded: bool,
    /// Vertical velocity
    pub vertical_velocity: f32,
    /// Time since last grounded
    pub air_time: f32,
    /// Is character turning
    pub turning: f32,
    /// Lean amount for banking
    pub lean: f32,
}

impl LocomotionController {
    /// Update from velocity and character state
    pub fn update_from_velocity(&mut self, velocity: Vec3, forward: Vec3, grounded: bool, delta: f32) {
        let horizontal = Vec3::new(velocity.x, 0.0, velocity.z);
        // Speed in m/s - no normalization, use actual velocity
        self.speed = horizontal.length();
        
        self.vertical_velocity = velocity.y;
        self.grounded = grounded;
        
        if grounded {
            self.air_time = 0.0;
        } else {
            self.air_time += delta;
        }
        
        // Calculate direction relative to forward
        if horizontal.length_squared() > 0.01 {
            let move_dir = horizontal.normalize();
            let forward_2d = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
            
            // Dot product for forward/backward
            let forward_dot = forward_2d.dot(move_dir);
            // Cross product Y for left/right
            let right_cross = forward_2d.cross(move_dir).y;
            
            self.direction = right_cross.atan2(forward_dot);
            self.strafe = right_cross;
        }
        
        // Calculate lean based on turning rate
        // This would need angular velocity in a full implementation
    }
    
    /// Get the appropriate animation state based on locomotion
    pub fn get_animation_state(&self) -> AnimationState {
        if !self.grounded {
            if self.vertical_velocity > 1.0 {
                return AnimationState::JumpAir;
            } else if self.air_time > 0.5 {
                return AnimationState::Falling;
            }
        }
        
        // Speed thresholds in m/s (1 unit = 1 meter):
        // Character defaults: walk=1.8 m/s, sprint=5.4 m/s
        // Thresholds set BELOW actual speeds to ensure animation triggers
        // - Idle: < 0.3 m/s (nearly stationary)
        // - Walk: 0.3 - 3.5 m/s (triggers at walk speed of 1.8)
        // - Run: 3.5+ m/s (triggers at sprint speed of 5.4)
        if self.speed < 0.3 {
            AnimationState::Idle
        } else if self.speed < 3.5 {
            AnimationState::Walk
        } else {
            AnimationState::Run
        }
    }
}

// ============================================================================
// Inverse Kinematics
// ============================================================================

/// IK target for a limb
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct IKTarget {
    /// Target position in world space
    pub position: Vec3,
    /// Target rotation (optional)
    pub rotation: Option<Quat>,
    /// Blend weight (0.0 = animation, 1.0 = IK)
    pub weight: f32,
    /// Which limb this targets
    pub limb: IKLimb,
}

/// IK limb types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum IKLimb {
    LeftFoot,
    RightFoot,
    LeftHand,
    RightHand,
    Head,
}

/// Foot IK component for ground adaptation
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct FootIK {
    /// Enable foot IK
    pub enabled: bool,
    /// Left foot offset from animation
    pub left_offset: Vec3,
    /// Right foot offset from animation
    pub right_offset: Vec3,
    /// Hip offset to maintain leg length
    pub hip_offset: f32,
    /// Blend speed
    pub blend_speed: f32,
    /// Raycast distance for ground detection
    pub ray_distance: f32,
}

impl Default for FootIK {
    fn default() -> Self {
        Self {
            enabled: true,
            left_offset: Vec3::ZERO,
            right_offset: Vec3::ZERO,
            hip_offset: 0.0,
            blend_speed: 10.0,
            ray_distance: 0.5,
        }
    }
}

// ============================================================================
// Procedural Animation
// ============================================================================

/// Procedural animation layer for secondary motion
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct ProceduralAnimation {
    /// Enable procedural animation
    pub enabled: bool,
    
    // Breathing
    /// Breathing frequency (breaths per minute)
    pub breathing_rate: f32,
    /// Breathing amplitude
    pub breathing_amplitude: f32,
    /// Current breathing phase
    pub breathing_phase: f32,
    
    // Head look
    /// Look target position
    pub look_target: Option<Vec3>,
    /// Look weight
    pub look_weight: f32,
    
    // Body sway
    /// Idle sway amount
    pub idle_sway: f32,
    /// Sway phase
    pub sway_phase: f32,
}

impl Default for ProceduralAnimation {
    fn default() -> Self {
        Self {
            enabled: true,
            breathing_rate: 12.0, // 12 breaths per minute
            breathing_amplitude: 0.02,
            breathing_phase: 0.0,
            look_target: None,
            look_weight: 0.5,
            idle_sway: 0.01,
            sway_phase: 0.0,
        }
    }
}

impl ProceduralAnimation {
    /// Update procedural animation
    pub fn update(&mut self, delta: f32) {
        // Update breathing
        let breath_freq = self.breathing_rate / 60.0; // Convert to Hz
        self.breathing_phase += delta * breath_freq * std::f32::consts::TAU;
        if self.breathing_phase > std::f32::consts::TAU {
            self.breathing_phase -= std::f32::consts::TAU;
        }
        
        // Update sway
        self.sway_phase += delta * 0.5; // Slow sway
        if self.sway_phase > std::f32::consts::TAU {
            self.sway_phase -= std::f32::consts::TAU;
        }
    }
    
    /// Get breathing offset for chest
    pub fn get_breathing_offset(&self) -> f32 {
        self.breathing_phase.sin() * self.breathing_amplitude
    }
    
    /// Get sway offset
    pub fn get_sway_offset(&self) -> Vec3 {
        Vec3::new(
            self.sway_phase.sin() * self.idle_sway,
            0.0,
            (self.sway_phase * 1.3).cos() * self.idle_sway * 0.5,
        )
    }
}

// ============================================================================
// Animation Clip Data
// ============================================================================

/// Animation clip metadata
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct AnimationClipData {
    /// Clip name
    pub name: String,
    /// Duration in seconds
    pub duration: f32,
    /// Is this animation looping
    pub looping: bool,
    /// Root motion enabled
    pub root_motion: bool,
    /// Events at specific times
    pub events: Vec<AnimationEvent>,
}

/// Animation event (footstep, sound, effect)
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct AnimationEvent {
    /// Time in seconds
    pub time: f32,
    /// Event type
    pub event_type: AnimationEventType,
}

/// Types of animation events
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub enum AnimationEventType {
    Footstep { foot: IKLimb },
    Sound { name: String },
    Effect { name: String, bone: HumanoidBone },
    Custom { name: String, data: String },
}

// ============================================================================
// Animation Layer
// ============================================================================

/// Animation layer for blending
#[derive(Debug, Clone, Serialize, Deserialize, Reflect)]
pub struct AnimationLayer {
    /// Layer name
    pub name: String,
    /// Layer weight
    pub weight: f32,
    /// Blend mode
    pub blend_mode: LayerBlendMode,
    /// Mask (which bones this layer affects)
    pub mask: LayerMask,
}

/// How layers blend together
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Reflect)]
pub enum LayerBlendMode {
    #[default]
    Override,
    Additive,
    Multiply,
}

/// Which bones a layer affects
#[derive(Debug, Clone, Default, Serialize, Deserialize, Reflect)]
pub enum LayerMask {
    #[default]
    FullBody,
    UpperBody,
    LowerBody,
    LeftArm,
    RightArm,
    Head,
    Custom(Vec<HumanoidBone>),
}

// ============================================================================
// Animation Events (Bevy Messages)
// ============================================================================

/// Event: Play an animation
#[derive(bevy::prelude::Message, Clone, Debug)]
pub struct PlayAnimationEvent {
    pub entity: Entity,
    pub animation: String,
    pub blend_time: f32,
    pub looping: bool,
}

/// Event: Animation finished
#[derive(bevy::prelude::Message, Clone, Debug)]
pub struct AnimationFinishedEvent {
    pub entity: Entity,
    pub animation: String,
}

/// Event: Animation event triggered
#[derive(bevy::prelude::Message, Clone, Debug)]
pub struct AnimationEventTriggered {
    pub entity: Entity,
    pub event: AnimationEvent,
}

// ============================================================================
// Character Animation Bundle
// ============================================================================

/// Bundle for a fully animated character
#[derive(Bundle, Default)]
pub struct CharacterAnimationBundle {
    pub state_machine: AnimationStateMachine,
    pub locomotion: LocomotionController,
    pub procedural: ProceduralAnimation,
    pub foot_ik: FootIK,
}

impl CharacterAnimationBundle {
    pub fn new() -> Self {
        Self::default()
    }
}
