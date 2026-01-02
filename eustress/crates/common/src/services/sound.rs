//! # Sound Service
//! 
//! Audio playback and management.
//! 
//! Sound component matches engine/src/classes.rs::Sound for compatibility.
//! SoundService is the global audio settings resource.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// SoundService Resource
// ============================================================================

/// SoundService - global audio settings (like Eustress's SoundService)
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct SoundService {
    /// Master volume (0-1)
    pub master_volume: f32,
    /// Ambient volume (0-1)
    pub ambient_volume: f32,
    /// Distance factor for 3D audio
    pub distance_factor: f32,
    /// Doppler scale
    pub doppler_scale: f32,
    /// Rolloff scale for distance attenuation
    pub rolloff_scale: f32,
    /// Is audio muted globally
    pub muted: bool,
    /// Respect distance for volume falloff
    pub respect_filtering_enabled: bool,
}

impl Default for SoundService {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            ambient_volume: 0.5,
            distance_factor: 1.0,
            doppler_scale: 1.0,
            rolloff_scale: 1.0,
            muted: false,
            respect_filtering_enabled: true,
        }
    }
}

// ============================================================================
// SoundInstance Component (runtime audio instance)
// ============================================================================

/// Runtime sound instance with playback state
/// Use classes::Sound for the base component, this for runtime state
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SoundInstance {
    /// Audio asset (Eustress "SoundId")
    /// Bevy: Handle<AudioSource>
    pub sound_id: String,
    
    /// Volume (Eustress "Volume" 0-1)
    /// Bevy: PlaybackSettings.volume
    pub volume: f32,
    
    /// Pitch multiplier (Eustress "Pitch" 0.5-2)
    /// Bevy: PlaybackSettings.speed
    pub pitch: f32,
    
    /// Loop behavior (Eustress "Looped")
    /// Bevy: PlaybackSettings.looped
    pub looped: bool,
    
    /// Playback state (Eustress "Playing")
    /// Bevy: Play/pause
    #[serde(skip)]
    pub playing: bool,
    
    /// 3D spatial audio (Eustress implicit)
    pub spatial: bool,
    
    /// Max distance (Eustress "RollOffMaxDistance")
    pub roll_off_max_distance: f32,
    
    /// Min distance (Eustress "RollOffMinDistance")
    pub roll_off_min_distance: f32,
    
    /// Sound group for volume control
    pub sound_group: Option<String>,
    
    /// Time position in seconds (runtime)
    #[serde(skip)]
    pub time_position: f32,
    
    /// Total length in seconds (runtime)
    #[serde(skip)]
    pub time_length: f32,
}

impl Default for SoundInstance {
    fn default() -> Self {
        Self {
            sound_id: String::new(),
            volume: 0.5,
            pitch: 1.0,
            looped: false,
            playing: false,
            spatial: true,
            roll_off_max_distance: 10000.0,
            roll_off_min_distance: 10.0,
            sound_group: None,
            time_position: 0.0,
            time_length: 0.0,
        }
    }
}

impl SoundInstance {
    pub fn new(sound_id: impl Into<String>) -> Self {
        Self {
            sound_id: sound_id.into(),
            ..default()
        }
    }
    
    pub fn looped(mut self) -> Self {
        self.looped = true;
        self
    }
    
    pub fn volume(mut self, vol: f32) -> Self {
        self.volume = vol.clamp(0.0, 1.0);
        self
    }
    
    pub fn spatial(mut self, is_spatial: bool) -> Self {
        self.spatial = is_spatial;
        self
    }
}

// ============================================================================
// SoundGroup
// ============================================================================

/// SoundGroup - volume group for sounds (like Eustress's SoundGroup)
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct SoundGroup {
    /// Group name
    pub name: String,
    /// Group volume (0-1)
    pub volume: f32,
}

impl Default for SoundGroup {
    fn default() -> Self {
        Self {
            name: "Master".to_string(),
            volume: 1.0,
        }
    }
}

// ============================================================================
// Sound Events
// ============================================================================

/// Message to play a sound
#[derive(Message, Clone, Debug)]
pub struct PlaySoundEvent {
    pub entity: Entity,
}

/// Message to stop a sound
#[derive(Message, Clone, Debug)]
pub struct StopSoundEvent {
    pub entity: Entity,
}

/// Message to play a sound at a position (one-shot)
#[derive(Message, Clone, Debug)]
pub struct PlaySoundAtEvent {
    pub sound_id: String,
    pub position: Vec3,
    pub volume: f32,
}
