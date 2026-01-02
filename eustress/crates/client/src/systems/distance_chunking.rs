//! Distance-based chunking system for enhancement pipeline
//! Only enhances nodes within range of player camera

use bevy::prelude::*;
use crate::components::PendingEnhancement;
use crate::systems::enhancement_scheduler::EnhanceNodeEvent;

/// Resource to track player position for distance culling
#[derive(Resource, Default)]
pub struct PlayerPosition {
    pub position: Vec3,
}

/// Settings for distance-based enhancement
#[derive(Resource)]
#[allow(dead_code)]
pub struct ChunkingSettings {
    /// Maximum distance to start enhancement (meters)
    pub enhancement_range: f32,
    /// Maximum distance to keep enhanced assets loaded (meters)
    pub unload_range: f32,
    /// Check frequency (seconds between checks)
    pub check_interval: f32,
    /// Timer for checking
    pub timer: Timer,
}

impl Default for ChunkingSettings {
    fn default() -> Self {
        Self {
            enhancement_range: 100.0,
            unload_range: 150.0,
            check_interval: 0.5,
            timer: Timer::from_seconds(0.5, TimerMode::Repeating),
        }
    }
}

/// System to update player position from camera
pub fn update_player_position_system(
    mut player_pos: ResMut<PlayerPosition>,
    camera_query: Query<&Transform, With<Camera3d>>,
) {
    if let Ok(camera_transform) = camera_query.single() {
        player_pos.position = camera_transform.translation;
    }
}

/// System to trigger enhancement for nodes within range
pub fn distance_chunking_system(
    time: Res<Time>,
    mut settings: ResMut<ChunkingSettings>,
    player_pos: Res<PlayerPosition>,
    pending_query: Query<(Entity, &Transform, &PendingEnhancement)>,
    mut enhance_events: MessageWriter<EnhanceNodeEvent>,
) {
    // Only check periodically to reduce overhead
    settings.timer.tick(time.delta());
    if !settings.timer.just_finished() {
        return;
    }
    
    // Find nodes within enhancement range that haven't been triggered yet
    for (entity, transform, pending) in pending_query.iter() {
        let distance = player_pos.position.distance(transform.translation);
        
        if distance <= settings.enhancement_range {
            info!(
                "🎯 Node '{}' within range ({:.1}m) - triggering enhancement",
                pending.prompt,
                distance
            );
            
            // Send enhancement event
            enhance_events.write(EnhanceNodeEvent {
                entity,
                node_id: pending.node_id,
                prompt: pending.prompt.clone(),
                category: pending.category,
                detail_level: pending.detail_level,
            });
        }
    }
}

/// System to unload far assets (future optimization)
#[allow(dead_code)]
pub fn unload_distant_assets_system(
    _settings: Res<ChunkingSettings>,
    _player_pos: Res<PlayerPosition>,
    // TODO: Query for Enhanced entities with Transform
    // Despawn or hide entities beyond unload_range
) {
    // Placeholder for future implementation
    // This would despawn or hide enhanced entities too far from player
    // to save memory in large scenes
}
