//! # Toon Module
//!
//! Character/toon definitions for Soul scripts.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Toon component - represents a character in Soul scripts
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct Toon {
    pub name: String,
    pub personality: ToonPersonality,
    pub state: ToonState,
}

impl Default for Toon {
    fn default() -> Self {
        Self {
            name: "Unnamed".to_string(),
            personality: ToonPersonality::default(),
            state: ToonState::default(),
        }
    }
}

/// Toon personality traits
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ToonPersonality {
    pub friendliness: f32,
    pub curiosity: f32,
    pub energy: f32,
    pub traits: Vec<String>,
}

/// Toon state
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ToonState {
    pub mood: Mood,
    pub activity: Option<String>,
    pub target: Option<Entity>,
}

/// Mood enum
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Mood {
    #[default]
    Neutral,
    Happy,
    Sad,
    Angry,
    Excited,
    Scared,
    Curious,
}
