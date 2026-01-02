//! # Run Service
//! 
//! Game loop hooks and state management (like Eustress's RunService).
//! 
//! ## Classes
//! - `RunService`: Game loop state
//! - `GameState`: Current game state

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// RunService Resource
// ============================================================================

/// RunService - game loop state and timing
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct RunService {
    /// Current game state
    pub state: GameState,
    /// Is game running (not paused)
    pub running: bool,
    /// Is this the client
    pub is_client: bool,
    /// Is this the server
    pub is_server: bool,
    /// Is this in studio/editor
    pub is_studio: bool,
    /// Time since game started
    pub time_elapsed: f64,
    /// Delta time this frame
    pub delta_time: f32,
    /// Fixed delta time for physics
    pub fixed_delta_time: f32,
    /// Current frame number
    pub frame_count: u64,
    /// Target FPS (0 = unlimited)
    pub target_fps: u32,
    /// Time scale (1.0 = normal)
    pub time_scale: f32,
}

impl Default for RunService {
    fn default() -> Self {
        Self {
            state: GameState::Loading,
            running: false,
            is_client: true,
            is_server: false,
            is_studio: false,
            time_elapsed: 0.0,
            delta_time: 0.0,
            fixed_delta_time: 1.0 / 60.0,
            frame_count: 0,
            target_fps: 60,
            time_scale: 1.0,
        }
    }
}

impl RunService {
    /// Create for client
    pub fn client() -> Self {
        Self {
            is_client: true,
            is_server: false,
            is_studio: false,
            ..default()
        }
    }
    
    /// Create for server
    pub fn server() -> Self {
        Self {
            is_client: false,
            is_server: true,
            is_studio: false,
            ..default()
        }
    }
    
    /// Create for studio/editor
    pub fn studio() -> Self {
        Self {
            is_client: false,
            is_server: false,
            is_studio: true,
            ..default()
        }
    }
    
    /// Pause the game
    pub fn pause(&mut self) {
        self.running = false;
        self.state = GameState::Paused;
    }
    
    /// Resume the game
    pub fn resume(&mut self) {
        self.running = true;
        self.state = GameState::Playing;
    }
}

// ============================================================================
// Game State
// ============================================================================

/// Game state enum
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize, Hash)]
pub enum GameState {
    /// Initial loading
    #[default]
    Loading,
    /// Main menu
    MainMenu,
    /// In-game playing
    Playing,
    /// Game paused
    Paused,
    /// Game over / results
    GameOver,
    /// Editor mode (studio only)
    Editing,
    /// Play testing in editor
    PlayTesting,
}

// ============================================================================
// Events
// ============================================================================

/// Event fired every render frame
#[derive(Message, Clone, Debug)]
pub struct RenderSteppedEvent {
    pub delta: f32,
}

/// Event fired every physics step
#[derive(Message, Clone, Debug)]
pub struct SteppedEvent {
    pub delta: f32,
}

/// Event fired every heartbeat (slower, for logic)
#[derive(Message, Clone, Debug)]
pub struct HeartbeatEvent {
    pub delta: f32,
}

/// Event when game state changes
#[derive(Message, Clone, Debug)]
pub struct GameStateChangedEvent {
    pub old_state: GameState,
    pub new_state: GameState,
}

// ============================================================================
// Bevy State Integration
// ============================================================================

/// Plugin to add RunService and state management
pub struct RunServiceTypes;

impl RunServiceTypes {
    /// Register all types (call from plugin)
    pub fn register(app: &mut App) {
        app
            .init_resource::<RunService>()
            .register_type::<RunService>()
            .register_type::<GameState>()
            .add_message::<RenderSteppedEvent>()
            .add_message::<SteppedEvent>()
            .add_message::<HeartbeatEvent>()
            .add_message::<GameStateChangedEvent>();
    }
}
