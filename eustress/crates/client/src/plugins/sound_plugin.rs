//! # Sound Plugin (Client)
//! 
//! Registers SoundService and handles audio playback.

use bevy::prelude::*;
use eustress_common::services::sound::*;
use eustress_common::classes::Sound;

#[allow(dead_code)]
pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resource
            .init_resource::<SoundService>()
            .register_type::<SoundService>()
            
            // Components
            .register_type::<Sound>()
            .register_type::<SoundGroup>()
            
            // Messages (Bevy 0.17)
            .add_message::<PlaySoundEvent>()
            .add_message::<StopSoundEvent>()
            .add_message::<PlaySoundAtEvent>()
            
            // Systems
            .add_systems(Update, (
                handle_play_sound_events,
                handle_stop_sound_events,
            ));
    }
}

/// Handle PlaySoundEvent
#[allow(dead_code)]
fn handle_play_sound_events(
    mut events: MessageReader<PlaySoundEvent>,
    mut sounds: Query<&mut Sound>,
) {
    for event in events.read() {
        if let Ok(mut sound) = sounds.get_mut(event.entity) {
            sound.playing = true;
        }
    }
}

/// Handle StopSoundEvent
#[allow(dead_code)]
fn handle_stop_sound_events(
    mut events: MessageReader<StopSoundEvent>,
    mut sounds: Query<&mut Sound>,
) {
    for event in events.read() {
        if let Ok(mut sound) = sounds.get_mut(event.entity) {
            sound.playing = false;
        }
    }
}
