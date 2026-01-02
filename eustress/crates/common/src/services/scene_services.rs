//! # Scene Services Integration
//!
//! Systems and events for applying scene settings to runtime services.
//! This bridges the gap between scene files (.ron) and runtime resources.
//!
//! ## Flow
//! ```text
//! Scene File (.ron)
//!     │
//!     ▼ (load)
//! LoadedSceneData
//!     │
//!     ▼ (ApplySceneEvent)
//! apply_scene_to_services system
//!     │
//!     ├──▶ LightingService (atmosphere, time, fog)
//!     ├──▶ Workspace (gravity, speed limits)
//!     └──▶ PlayerService (walk speed, jump power)
//! ```

use bevy::prelude::*;
#[allow(unused_imports)]
use crate::scene::{Scene, AtmosphereSettings, WorkspaceSettings, PlayerSettings, SpawnLocationData};
use crate::services::{LightingService, Workspace, PlayerService};

// ============================================================================
// Events
// ============================================================================

/// Event to trigger applying scene settings to services
#[derive(Event, Message, Debug, Clone)]
pub struct ApplySceneEvent {
    /// The scene data to apply
    pub scene: Scene,
}

/// Event fired when scene settings have been applied
#[derive(Event, Message, Debug, Clone)]
pub struct SceneAppliedEvent {
    /// Scene name
    pub scene_name: String,
    /// Number of entities in scene
    pub entity_count: usize,
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for scene-to-services integration
pub struct SceneServicesPlugin;

impl Plugin for SceneServicesPlugin {
    fn build(&self, app: &mut App) {
        app
            .add_message::<ApplySceneEvent>()
            .add_message::<SceneAppliedEvent>()
            .add_systems(Update, apply_scene_to_services);
    }
}

// ============================================================================
// Systems
// ============================================================================

/// System that applies scene settings to runtime services
/// 
/// This is the central integration point between scene files and runtime state.
/// When a scene is loaded, this system updates:
/// - LightingService (atmosphere, time of day, fog, sun)
/// - Workspace (gravity, speed limits, streaming)
/// - PlayerService (walk speed, jump power, defaults)
pub fn apply_scene_to_services(
    mut events: MessageReader<ApplySceneEvent>,
    mut applied_events: MessageWriter<SceneAppliedEvent>,
    mut lighting: Option<ResMut<LightingService>>,
    mut workspace: Option<ResMut<Workspace>>,
    mut player_service: Option<ResMut<PlayerService>>,
) {
    for event in events.read() {
        let scene = &event.scene;
        info!("🎬 Applying scene settings: {}", scene.metadata.name);
        
        // Apply atmosphere to LightingService
        if let Some(ref mut lighting) = lighting {
            apply_atmosphere_to_lighting(&scene.atmosphere, lighting);
            info!("  ✓ Applied atmosphere to LightingService");
        }
        
        // Apply workspace settings
        if let Some(ref mut workspace) = workspace {
            apply_workspace_settings(&scene.workspace_settings, workspace);
            info!("  ✓ Applied workspace settings (gravity: {})", workspace.gravity.y);
        }
        
        // Apply player settings
        if let Some(ref mut player_service) = player_service {
            apply_player_settings(&scene.player_settings, player_service);
            info!("  ✓ Applied player settings (walk: {}, jump: {})", 
                  player_service.default_walk_speed, 
                  player_service.default_jump_power);
        }
        
        // Fire completion event
        applied_events.write(SceneAppliedEvent {
            scene_name: scene.metadata.name.clone(),
            entity_count: scene.entities.len(),
        });
        
        info!("✅ Scene settings applied successfully");
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Apply atmosphere settings to LightingService
fn apply_atmosphere_to_lighting(atmosphere: &AtmosphereSettings, lighting: &mut LightingService) {
    // Time of day
    lighting.time_of_day = parse_time_of_day(&atmosphere.time_of_day);
    lighting.clock_time = atmosphere.time_of_day.clone();
    
    // Sun
    lighting.sun_color = atmosphere.sun_color;
    lighting.sun_intensity = atmosphere.sun_intensity;
    
    // Ambient
    lighting.ambient = atmosphere.ambient_color;
    lighting.brightness = atmosphere.brightness;
    
    // Fog
    lighting.fog_enabled = atmosphere.fog_density > 0.0;
    lighting.fog_color = atmosphere.fog_color;
    lighting.fog_start = atmosphere.fog_start;
    lighting.fog_end = atmosphere.fog_end;
    
    // Sky colors
    lighting.sky_color = atmosphere.sky_color;
    lighting.horizon_color = atmosphere.horizon_color;
    
    // Shadows
    lighting.shadows_enabled = atmosphere.shadows_enabled;
    lighting.shadow_softness = atmosphere.shadow_softness;
}

/// Apply workspace settings to Workspace resource
fn apply_workspace_settings(settings: &WorkspaceSettings, workspace: &mut Workspace) {
    workspace.gravity = Vec3::new(0.0, -settings.gravity, 0.0);
    workspace.max_entity_speed = settings.max_entity_speed;
    workspace.streaming_enabled = settings.streaming_enabled;
    workspace.streaming_target_radius = settings.streaming_target_radius;
    workspace.streaming_min_radius = settings.streaming_min_radius;
}

/// Apply player settings to PlayerService
fn apply_player_settings(settings: &PlayerSettings, player_service: &mut PlayerService) {
    player_service.default_walk_speed = settings.walk_speed;
    player_service.default_jump_power = settings.jump_power;
    player_service.default_max_health = settings.max_health;
    player_service.auto_jump_enabled = settings.auto_jump_enabled;
    player_service.respawn_time = settings.respawn_time;
}

/// Parse time of day string to float (0.0 = midnight, 0.5 = noon, 1.0 = midnight)
fn parse_time_of_day(time_str: &str) -> f32 {
    // Try parsing as "HH:MM:SS" or "HH:MM"
    let parts: Vec<&str> = time_str.split(':').collect();
    
    if parts.len() >= 2 {
        if let (Ok(hours), Ok(minutes)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>()) {
            let seconds = if parts.len() >= 3 {
                parts[2].parse::<f32>().unwrap_or(0.0)
            } else {
                0.0
            };
            
            let total_seconds = hours * 3600.0 + minutes * 60.0 + seconds;
            return total_seconds / 86400.0; // Normalize to 0.0-1.0
        }
    }
    
    // Try parsing as float directly
    time_str.parse::<f32>().unwrap_or(0.5) // Default to noon
}

// ============================================================================
// Workspace Extension Methods
// ============================================================================

impl Workspace {
    /// Apply settings from scene WorkspaceSettings
    pub fn apply_scene_settings(&mut self, settings: &WorkspaceSettings) {
        self.gravity = Vec3::new(0.0, -settings.gravity, 0.0);
        self.max_entity_speed = settings.max_entity_speed;
        self.streaming_enabled = settings.streaming_enabled;
        self.streaming_target_radius = settings.streaming_target_radius;
        self.streaming_min_radius = settings.streaming_min_radius;
    }
}

// ============================================================================
// LightingService Extension Methods
// ============================================================================

impl LightingService {
    /// Apply settings from scene AtmosphereSettings
    pub fn apply_atmosphere(&mut self, atmosphere: &AtmosphereSettings) {
        apply_atmosphere_to_lighting(atmosphere, self);
    }
    
    /// Create from scene atmosphere settings
    pub fn from_atmosphere(atmosphere: &AtmosphereSettings) -> Self {
        let mut lighting = Self::default();
        lighting.apply_atmosphere(atmosphere);
        lighting
    }
}
