//! # Audio DSP / routing spawners — Wave 7.E
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md`:
//! the 26 Roblox audio-DSP / routing / legacy-SoundEffect classes. Each
//! spawner attaches its parameter component + the cross-cutting
//! [`Instance`] / [`Name`] and persists it.
//!
//! ## Pattern — config-attach, defer the DSP graph
//!
//! These are **parameter carriers**: the spawner attaches the Eustress
//! component reading defaults; the actual DSP chain (the rodio/cpal audio
//! graph wiring, the new-API node routing, the legacy-SoundEffect attachment
//! to a parent `Sound`) is a later runtime phase. This mirrors the Wave 6.A
//! ValueObject group shape (data-only attach + empty LOD + stub Fjall
//! persistence + TOML round-trip).
//!
//! ## Why no LOD / stub persistence
//!
//! Audio nodes have no visual; `lod_components` returns an empty bundle at
//! every tier and `serialize`/`deserialize` are stubbed (TOML round-trip
//! carries the value) until a later wave lights up the Fjall write path.
//!
//! [`Instance`]: eustress_common::classes::Instance

use bevy::prelude::*;

use eustress_common::class_registry::{PropertyBag, RegisterClassExt};
use eustress_common::classes::{ClassName, Instance, PropertyValue};

pub mod audio_analyzer;
pub mod audio_chorus;
pub mod audio_compressor;
pub mod audio_device_input;
pub mod audio_device_output;
pub mod audio_distortion;
pub mod audio_echo;
pub mod audio_emitter;
pub mod audio_equalizer;
pub mod audio_fader;
pub mod audio_filter;
pub mod audio_flanger;
pub mod audio_listener;
pub mod audio_pitch_shifter;
pub mod audio_player;
pub mod audio_reverb;
pub mod audio_search_params;
pub mod chorus_sound_effect;
pub mod compressor_sound_effect;
pub mod distortion_sound_effect;
pub mod echo_sound_effect;
pub mod equalizer_sound_effect;
pub mod flange_sound_effect;
pub mod pitch_shift_sound_effect;
pub mod reverb_sound_effect;
pub mod tremolo_sound_effect;

pub use audio_analyzer::AudioAnalyzerSpawner;
pub use audio_chorus::AudioChorusSpawner;
pub use audio_compressor::AudioCompressorSpawner;
pub use audio_device_input::AudioDeviceInputSpawner;
pub use audio_device_output::AudioDeviceOutputSpawner;
pub use audio_distortion::AudioDistortionSpawner;
pub use audio_echo::AudioEchoSpawner;
pub use audio_emitter::AudioEmitterSpawner;
pub use audio_equalizer::AudioEqualizerSpawner;
pub use audio_fader::AudioFaderSpawner;
pub use audio_filter::AudioFilterSpawner;
pub use audio_flanger::AudioFlangerSpawner;
pub use audio_listener::AudioListenerSpawner;
pub use audio_pitch_shifter::AudioPitchShifterSpawner;
pub use audio_player::AudioPlayerSpawner;
pub use audio_reverb::AudioReverbSpawner;
pub use audio_search_params::AudioSearchParamsSpawner;
pub use chorus_sound_effect::ChorusSoundEffectSpawner;
pub use compressor_sound_effect::CompressorSoundEffectSpawner;
pub use distortion_sound_effect::DistortionSoundEffectSpawner;
pub use echo_sound_effect::EchoSoundEffectSpawner;
pub use equalizer_sound_effect::EqualizerSoundEffectSpawner;
pub use flange_sound_effect::FlangeSoundEffectSpawner;
pub use pitch_shift_sound_effect::PitchShiftSoundEffectSpawner;
pub use reverb_sound_effect::ReverbSoundEffectSpawner;
pub use tremolo_sound_effect::TremoloSoundEffectSpawner;

/// Bevy plugin registering every audio-DSP / routing spawner shipped by
/// Wave 7.E with the
/// [`ClassRegistry`][eustress_common::class_registry::ClassRegistry].
pub struct AudioDspSpawnerPlugin;

impl Plugin for AudioDspSpawnerPlugin {
    fn build(&self, app: &mut App) {
        app.register_class::<AudioReverbSpawner>()
            .register_class::<AudioEchoSpawner>()
            .register_class::<AudioDistortionSpawner>()
            .register_class::<AudioEqualizerSpawner>()
            .register_class::<AudioCompressorSpawner>()
            .register_class::<AudioChorusSpawner>()
            .register_class::<AudioFlangerSpawner>()
            .register_class::<AudioFaderSpawner>()
            .register_class::<AudioFilterSpawner>()
            .register_class::<AudioPitchShifterSpawner>()
            .register_class::<AudioEmitterSpawner>()
            .register_class::<AudioListenerSpawner>()
            .register_class::<AudioPlayerSpawner>()
            .register_class::<AudioDeviceInputSpawner>()
            .register_class::<AudioDeviceOutputSpawner>()
            .register_class::<AudioAnalyzerSpawner>()
            .register_class::<AudioSearchParamsSpawner>()
            .register_class::<ReverbSoundEffectSpawner>()
            .register_class::<EchoSoundEffectSpawner>()
            .register_class::<DistortionSoundEffectSpawner>()
            .register_class::<EqualizerSoundEffectSpawner>()
            .register_class::<CompressorSoundEffectSpawner>()
            .register_class::<ChorusSoundEffectSpawner>()
            .register_class::<FlangeSoundEffectSpawner>()
            .register_class::<PitchShiftSoundEffectSpawner>()
            .register_class::<TremoloSoundEffectSpawner>();
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Build the cross-cutting [`Instance`] every audio-node entity carries.
pub(crate) fn instance_from_bag(class_name: ClassName, bag: &PropertyBag) -> Instance {
    let name = bag
        .get_string("metadata.name")
        .unwrap_or(class_name.as_str())
        .to_string();
    Instance {
        name,
        class_name,
        archivable: bag.get_bool("metadata.archivable").unwrap_or(true),
        id: 0,
        uuid: bag.get_uuid().unwrap_or_default().to_string(),
        ai: false,
    }
}

/// Copy `metadata.*` keys from a `toml::Value`'s `[metadata]` table into `bag`.
pub(crate) fn import_metadata(toml_value: &toml::Value, bag: &mut PropertyBag) {
    let Some(meta) = toml_value.get("metadata") else {
        return;
    };
    if let Some(name) = meta.get("name").and_then(|v| v.as_str()) {
        bag.set("metadata.name", PropertyValue::String(name.to_string()));
    }
    if let Some(archivable) = meta.get("archivable").and_then(|v| v.as_bool()) {
        bag.set("metadata.archivable", PropertyValue::Bool(archivable));
    }
    if let Some(uuid) = meta.get("uuid").and_then(|v| v.as_str()) {
        bag.set("metadata.uuid", PropertyValue::String(uuid.to_string()));
    }
}

/// Emit the canonical `[metadata]` table for `export_to_toml`.
pub(crate) fn export_metadata(
    world: &World,
    entity: Entity,
    class_name: &str,
) -> toml::value::Table {
    let mut meta = toml::value::Table::new();
    meta.insert(
        "class_name".to_string(),
        toml::Value::String(class_name.to_string()),
    );
    if let Some(instance) = world.get::<Instance>(entity) {
        meta.insert("name".to_string(), toml::Value::String(instance.name.clone()));
        meta.insert(
            "archivable".to_string(),
            toml::Value::Boolean(instance.archivable),
        );
        if !instance.uuid.is_empty() {
            meta.insert("uuid".to_string(), toml::Value::String(instance.uuid.clone()));
        }
    }
    meta
}

/// Apply the canonical `metadata.*` edits (name + archivable) in place.
pub(crate) fn apply_metadata_edit(world: &mut World, entity: Entity, props: &PropertyBag) {
    if let Ok(mut em) = world.get_entity_mut(entity) {
        let new_name = props.get_string("metadata.name").map(str::to_string);
        if let Some(mut instance) = em.get_mut::<Instance>() {
            if let Some(ref n) = new_name {
                instance.name = n.clone();
            }
            if let Some(a) = props.get_bool("metadata.archivable") {
                instance.archivable = a;
            }
        }
        if let Some(ref n) = new_name {
            if let Some(mut name) = em.get_mut::<Name>() {
                name.set(n.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};

    const AUDIO_CLASSES: [ClassName; 26] = [
        ClassName::AudioReverb,
        ClassName::AudioEcho,
        ClassName::AudioDistortion,
        ClassName::AudioEqualizer,
        ClassName::AudioCompressor,
        ClassName::AudioChorus,
        ClassName::AudioFlanger,
        ClassName::AudioFader,
        ClassName::AudioFilter,
        ClassName::AudioPitchShifter,
        ClassName::AudioEmitter,
        ClassName::AudioListener,
        ClassName::AudioPlayer,
        ClassName::AudioDeviceInput,
        ClassName::AudioDeviceOutput,
        ClassName::AudioAnalyzer,
        ClassName::AudioSearchParams,
        ClassName::ReverbSoundEffect,
        ClassName::EchoSoundEffect,
        ClassName::DistortionSoundEffect,
        ClassName::EqualizerSoundEffect,
        ClassName::CompressorSoundEffect,
        ClassName::ChorusSoundEffect,
        ClassName::FlangeSoundEffect,
        ClassName::PitchShiftSoundEffect,
        ClassName::TremoloSoundEffect,
    ];

    #[test]
    fn plugin_registers_all_twenty_six_audio_classes() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(AudioDspSpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        for class in AUDIO_CLASSES {
            assert!(registry.contains(class), "must register {}", class.as_str());
            let spawner: &dyn ClassSpawner = registry.get(class).unwrap();
            assert_eq!(spawner.class_name(), class);
        }
        assert_eq!(registry.len(), 26);
    }

    #[test]
    fn class_name_round_trips_for_every_audio_class() {
        for class in AUDIO_CLASSES {
            assert_eq!(ClassName::from_str(class.as_str()), Ok(class));
        }
    }
}
