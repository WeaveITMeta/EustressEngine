//! # Sound Plugin
//! 
//! Registers SoundService and audio classes.

use bevy::prelude::*;
use eustress_common::classes::Sound;
use eustress_common::services::sound::{SoundService, SoundGroup, PlaySoundEvent, StopSoundEvent, PlaySoundAtEvent};

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
            
            // Events
            .add_message::<PlaySoundEvent>()
            .add_message::<StopSoundEvent>()
            .add_message::<PlaySoundAtEvent>();
    }
}
