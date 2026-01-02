//! # Tween Service
//! 
//! Animation and interpolation (like Eustress's TweenService).
//! 
//! ## Classes
//! - `TweenService`: Global tween management
//! - `Tween`: Active tween instance
//! - `TweenInfo`: Tween configuration

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// TweenService Resource
// ============================================================================

/// TweenService - manages all active tweens
#[derive(Resource, Reflect, Clone, Debug, Default)]
#[reflect(Resource)]
pub struct TweenService {
    /// Number of active tweens
    pub active_count: u32,
    /// Max concurrent tweens (0 = unlimited)
    pub max_tweens: u32,
    /// Global time scale for tweens
    pub time_scale: f32,
}

// ============================================================================
// Tween Component
// ============================================================================

/// Tween - an active interpolation
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct Tween {
    /// Tween configuration
    pub info: TweenInfo,
    /// Target property
    pub property: TweenProperty,
    /// Start value
    pub start_value: TweenValue,
    /// End value
    pub end_value: TweenValue,
    /// Current progress (0-1)
    pub progress: f32,
    /// Current time elapsed
    pub elapsed: f32,
    /// Current repeat count
    pub repeat_count: u32,
    /// Is tween playing
    pub playing: bool,
    /// Is tween completed
    pub completed: bool,
}

impl Default for Tween {
    fn default() -> Self {
        Self {
            info: TweenInfo::default(),
            property: TweenProperty::Position,
            start_value: TweenValue::Vec3(Vec3::ZERO),
            end_value: TweenValue::Vec3(Vec3::ZERO),
            progress: 0.0,
            elapsed: 0.0,
            repeat_count: 0,
            playing: false,
            completed: false,
        }
    }
}

impl Tween {
    /// Create a new tween
    pub fn new(property: TweenProperty, end_value: TweenValue, info: TweenInfo) -> Self {
        Self {
            info,
            property,
            end_value,
            playing: true,
            ..default()
        }
    }
    
    /// Play the tween
    pub fn play(&mut self) {
        self.playing = true;
    }
    
    /// Pause the tween
    pub fn pause(&mut self) {
        self.playing = false;
    }
    
    /// Cancel the tween
    pub fn cancel(&mut self) {
        self.playing = false;
        self.completed = true;
    }
}

// ============================================================================
// TweenInfo
// ============================================================================

/// TweenInfo - configuration for a tween
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct TweenInfo {
    /// Duration in seconds
    pub duration: f32,
    /// Easing style
    pub easing_style: EasingStyle,
    /// Easing direction
    pub easing_direction: EasingDirection,
    /// Number of times to repeat (0 = once)
    pub repeat_count: u32,
    /// Reverse on repeat
    pub reverses: bool,
    /// Delay before starting
    pub delay: f32,
}

impl Default for TweenInfo {
    fn default() -> Self {
        Self {
            duration: 1.0,
            easing_style: EasingStyle::Quad,
            easing_direction: EasingDirection::Out,
            repeat_count: 0,
            reverses: false,
            delay: 0.0,
        }
    }
}

impl TweenInfo {
    pub fn new(duration: f32) -> Self {
        Self {
            duration,
            ..default()
        }
    }
    
    pub fn with_easing(mut self, style: EasingStyle, direction: EasingDirection) -> Self {
        self.easing_style = style;
        self.easing_direction = direction;
        self
    }
    
    pub fn with_repeat(mut self, count: u32, reverses: bool) -> Self {
        self.repeat_count = count;
        self.reverses = reverses;
        self
    }
    
    pub fn with_delay(mut self, delay: f32) -> Self {
        self.delay = delay;
        self
    }
}

// ============================================================================
// Easing
// ============================================================================

/// Easing style
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum EasingStyle {
    Linear,
    Sine,
    #[default]
    Quad,
    Cubic,
    Quart,
    Quint,
    Exponential,
    Circular,
    Back,
    Elastic,
    Bounce,
}

/// Easing direction
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum EasingDirection {
    In,
    #[default]
    Out,
    InOut,
}

/// Calculate eased value
pub fn ease(t: f32, style: EasingStyle, direction: EasingDirection) -> f32 {
    let t = t.clamp(0.0, 1.0);
    
    match direction {
        EasingDirection::In => ease_in(t, style),
        EasingDirection::Out => 1.0 - ease_in(1.0 - t, style),
        EasingDirection::InOut => {
            if t < 0.5 {
                ease_in(t * 2.0, style) / 2.0
            } else {
                1.0 - ease_in((1.0 - t) * 2.0, style) / 2.0
            }
        }
    }
}

fn ease_in(t: f32, style: EasingStyle) -> f32 {
    match style {
        EasingStyle::Linear => t,
        EasingStyle::Sine => 1.0 - (t * std::f32::consts::FRAC_PI_2).cos(),
        EasingStyle::Quad => t * t,
        EasingStyle::Cubic => t * t * t,
        EasingStyle::Quart => t * t * t * t,
        EasingStyle::Quint => t * t * t * t * t,
        EasingStyle::Exponential => if t == 0.0 { 0.0 } else { 2.0_f32.powf(10.0 * (t - 1.0)) },
        EasingStyle::Circular => 1.0 - (1.0 - t * t).sqrt(),
        EasingStyle::Back => t * t * (2.70158 * t - 1.70158),
        EasingStyle::Elastic => {
            if t == 0.0 || t == 1.0 { t }
            else { -2.0_f32.powf(10.0 * (t - 1.0)) * ((t - 1.1) * 5.0 * std::f32::consts::PI).sin() }
        },
        EasingStyle::Bounce => 1.0 - bounce_out(1.0 - t),
    }
}

fn bounce_out(t: f32) -> f32 {
    if t < 1.0 / 2.75 {
        7.5625 * t * t
    } else if t < 2.0 / 2.75 {
        let t = t - 1.5 / 2.75;
        7.5625 * t * t + 0.75
    } else if t < 2.5 / 2.75 {
        let t = t - 2.25 / 2.75;
        7.5625 * t * t + 0.9375
    } else {
        let t = t - 2.625 / 2.75;
        7.5625 * t * t + 0.984375
    }
}

// ============================================================================
// Tween Properties and Values
// ============================================================================

/// Properties that can be tweened
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum TweenProperty {
    #[default]
    Position,
    Rotation,
    Scale,
    Color,
    Transparency,
    Size,
    Custom(u32),
}

/// Tween value types
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub enum TweenValue {
    Float(f32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    Quat(Quat),
    Color([f32; 4]),
}

impl TweenValue {
    /// Interpolate between two values
    pub fn lerp(&self, other: &TweenValue, t: f32) -> TweenValue {
        match (self, other) {
            (TweenValue::Float(a), TweenValue::Float(b)) => TweenValue::Float(a + (b - a) * t),
            (TweenValue::Vec2(a), TweenValue::Vec2(b)) => TweenValue::Vec2(a.lerp(*b, t)),
            (TweenValue::Vec3(a), TweenValue::Vec3(b)) => TweenValue::Vec3(a.lerp(*b, t)),
            (TweenValue::Vec4(a), TweenValue::Vec4(b)) => TweenValue::Vec4(a.lerp(*b, t)),
            (TweenValue::Quat(a), TweenValue::Quat(b)) => TweenValue::Quat(a.slerp(*b, t)),
            (TweenValue::Color(a), TweenValue::Color(b)) => TweenValue::Color([
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
                a[3] + (b[3] - a[3]) * t,
            ]),
            _ => self.clone(),
        }
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event when tween completes
#[derive(Message, Clone, Debug)]
pub struct TweenCompletedEvent {
    pub entity: Entity,
}
