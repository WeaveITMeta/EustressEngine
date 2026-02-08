//! # Soul Context
//!
//! Runtime context for Soul script execution.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Soul execution context
#[derive(Debug, Clone, Default)]
pub struct SoulContext {
    pub entity: Option<Entity>,
    pub delta_time: f32,
}

/// Handle to an entity in Soul scripts
#[derive(Debug, Clone, Copy)]
pub struct EntityHandle(pub Entity);

/// Dynamic value type for Soul scripts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Vec3(f32, f32, f32),
    Color(f32, f32, f32, f32),
}

impl Default for Value {
    fn default() -> Self {
        Value::Nil
    }
}

/// Shape types for Soul scripts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    Box,
    Sphere,
    Cylinder,
    Capsule,
    Plane,
}

/// Light types for Soul scripts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightType {
    Point,
    Spot,
    Directional,
}

/// Easing functions for animations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

/// Command script type marker
#[derive(Debug, Clone, Default)]
pub struct CommandScript;

/// Service script type marker
#[derive(Debug, Clone, Default)]
pub struct SoulServiceScript;

/// Entity script type marker
#[derive(Debug, Clone, Default)]
pub struct SoulEntityScript;

/// Workspace script type marker
#[derive(Debug, Clone, Default)]
pub struct SoulWorkspaceScript;
