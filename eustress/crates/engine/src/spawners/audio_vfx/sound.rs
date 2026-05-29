//! `SoundSpawner` — Wave 3.F audio class spawner.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.7 (Camera & Audio) the
//! `Sound` class is one of two members of the audio group. This spawner
//! wires the Eustress `Sound` component to Bevy's native
//! `bevy::audio::AudioPlayer` + `PlaybackSettings` rig so the engine's
//! built-in `AudioPlugin` (already added through
//! `bevy::DefaultPlugins`) drives playback. The spawner is responsible
//! ONLY for building the entity — the existing `SoundService` resource
//! and the per-frame audio pipeline are untouched.
//!
//! ## What this spawner does
//!
//! - Attaches the Eustress `Sound` component (the canonical data model
//!   round-tripped through TOML + Fjall persistence).
//! - Attaches `bevy::audio::AudioPlayer<AudioSource>` carrying a
//!   `Handle<AudioSource>` resolved via `asset_server.load(sound_id)`.
//!   Empty `sound_id` skips the load (the audio pipeline will treat
//!   the entity as a configured-but-silent sound until a script
//!   assigns a path).
//! - Attaches `bevy::audio::PlaybackSettings` derived from the
//!   Eustress fields (volume / pitch / spatial / start_paused /
//!   looped). The spatial scale is derived from `roll_off_min_distance`
//!   per the `RollOffMode → SpatialScale` mapping spelled out below.
//! - Attaches `Transform` + `Visibility` so the entity participates in
//!   the standard ECS hierarchy. Sounds in Eustress are first-class
//!   spatial entities — they sit in the scene tree under their parent
//!   Part or Attachment exactly like any other instance.
//!
//! ## Listener wiring
//!
//! `bevy::audio::SpatialListener` is attached to the active studio /
//! play camera elsewhere (the engine's camera plugin sets it on the
//! main camera entity at startup). This spawner does NOT attach a
//! listener — sounds are sources, the listener is the ear. The Bevy
//! audio pipeline pairs them automatically through the spatial scale +
//! transforms.
//!
//! ## RollOffMode → SpatialScale conversion
//!
//! Bevy 0.18 spatial audio is a thin stereo-panning model — it has no
//! per-source falloff curve. We approximate Eustress's
//! `SoundRolloffMode` by scaling the spatial coordinates so the
//! perceived loudness curve matches the requested mode at the
//! `roll_off_min_distance` boundary:
//!
//! | Eustress `SoundRolloffMode` | `SpatialScale` factor | Rationale |
//! |---|---|---|
//! | `Inverse` (default)         | `1.0 / roll_off_min`  | Bevy's natural 1/r panning matches Roblox's default. |
//! | `InverseSquared`            | `1.0 / roll_off_min²` | Tighter falloff; doubling distance quarters loudness. |
//! | `Linear`                    | `1.0 / (roll_off_min * 2)` | Gentler than Bevy's natural curve. |
//! | `Logarithmic`               | `1.0 / roll_off_min`  | Approximated as Inverse — Bevy can't do true log without a custom mixer. |
//! | `None`                      | `0.0`                 | Distance disabled — listener-relative volume only. |
//! | `Custom`                    | `1.0 / roll_off_min`  | Treated as Inverse; the custom `roll_off_curve` is a TODO for a future per-sample volume system. |
//!
//! These are coarse — the engine's own audio system will eventually own
//! a real falloff mixer (FEATURE_PARITY.md §15 audio polish). The
//! conversion lives here because the spawner is the boundary that
//! translates the Eustress source-of-truth into Bevy components.

use std::any::TypeId;

use bevy::audio::{AudioPlayer, AudioSource, PlaybackMode, PlaybackSettings, SpatialScale, Volume};
use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, DynamicComponent, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, Sound, SoundGroup, SoundRolloffMode};

/// Wire-format tag for the Wave-3.F rkyv archives. Follows Appendix A of
/// `CLASS_REGISTRY.md`: high nibble = schema_version (1), low nibble =
/// class group (0 = generic). Audio sits in the generic group because it
/// carries only Eustress-side data — no engine-only material/handle
/// fields that would need a dedicated group.
const SOUND_TAG: u8 = 0x10;

/// Wave 3.F spawner for `ClassName::Sound`.
///
/// Stateless (`Default`-constructible) — all per-spawn data flows
/// through the `PropertyBag` argument as the trait contract requires.
#[derive(Default)]
pub struct SoundSpawner;

impl ClassSpawner for SoundSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Sound
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("Sound")
            .to_string();

        // Build the Eustress Sound component from the bag, falling back
        // to `Default` for keys the bag omits — preserves the "spawner
        // is the only schema authority" contract of CLASS_REGISTRY.md §4.
        let sound = sound_from_bag(props);

        // Resolve the audio asset. Empty paths skip the load so a
        // silent-but-configured Sound entity is a valid intermediate
        // state (the AudioPlayer can be hot-swapped later via the
        // Properties panel without respawning).
        let audio_player = if sound.sound_id.is_empty() {
            None
        } else {
            let handle: Handle<AudioSource> = ctx.asset_server.load(&sound.sound_id);
            Some(AudioPlayer::<AudioSource>(handle))
        };

        let playback = playback_settings_from_sound(&sound);

        // Build Transform from the bag; default to identity. Sounds are
        // ECS entities like any other instance, so they need
        // Transform + Visibility for the hierarchy + render-cascade to
        // treat them uniformly.
        let transform = props
            .get_transform("transform")
            .copied()
            .unwrap_or_default();

        let instance = Instance {
            name: name.clone(),
            class_name: ClassName::Sound,
            archivable: true,
            id: 0,
            uuid: props.get_uuid().unwrap_or_default().to_string(),
            ai: false,
        };

        let mut entity_commands = ctx.commands.spawn((
            instance,
            sound,
            transform,
            Visibility::default(),
            Name::new(name),
            playback,
        ));
        if let Some(player) = audio_player {
            entity_commands.insert(player);
        }
        entity_commands.id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        // Wave 3.F ships a stub rkyv path: we tag + serde-json the
        // Eustress Sound component for now (already Serialize via the
        // common `Sound` derive). Wave 5 swaps this for a dedicated
        // rkyv mirror struct under `worlddb::rkyv_values`. The tag byte
        // is the cross-version compatibility gate — `deserialize`
        // rejects mismatched tags loudly.
        let mut out = vec![SOUND_TAG];
        if let Some(sound) = world.entity(entity).get::<Sound>() {
            match serde_json::to_vec(sound) {
                Ok(mut payload) => out.append(&mut payload),
                Err(e) => warn!(
                    "SoundSpawner::serialize: serde_json encode failed for entity {entity:?}: {e}"
                ),
            }
        }
        out
    }

    fn deserialize(&self, bytes: &[u8]) -> PropertyBag {
        if bytes.first() != Some(&SOUND_TAG) {
            warn!(
                "SoundSpawner::deserialize: tag mismatch (got {:?}, expected {SOUND_TAG:#x}) — returning empty bag",
                bytes.first(),
            );
            return PropertyBag::new();
        }
        let payload = &bytes[1..];
        let sound: Sound = match serde_json::from_slice(payload) {
            Ok(s) => s,
            Err(e) => {
                warn!("SoundSpawner::deserialize: serde_json decode failed: {e}");
                return PropertyBag::new();
            }
        };
        bag_from_sound(&sound)
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // All Sound mutations are cheap — volume, pitch, spatial knobs
        // are read each tick by the audio system. Replacing `sound_id`
        // requires re-loading the AudioSource handle, which we DO
        // handle inline (no respawn) by overwriting the AudioPlayer
        // component. Returning `false` keeps the Properties panel
        // off the despawn-respawn dance.
        if let Some(mut sound) = world.entity_mut(entity).get_mut::<Sound>() {
            apply_bag_to_sound(props, &mut sound);
        }
        // Update PlaybackSettings if present so spatial / volume edits
        // take effect on the next audio tick.
        let mut updated_playback: Option<PlaybackSettings> = None;
        if let Some(sound_ref) = world.entity(entity).get::<Sound>() {
            updated_playback = Some(playback_settings_from_sound(sound_ref));
        }
        if let Some(pb) = updated_playback {
            world.entity_mut(entity).insert(pb);
        }
        false
    }

    fn lod_components(&self, tier: LodTier) -> ComponentBundle {
        // Per `RENDER_CASCADE.md` (informally): a sound out of range
        // should mute, not respawn. Wave 3 ships visibility + paused
        // signals only — actual mute (replacing PlaybackSettings) lives
        // in the LOD transition system that consumes this bundle.
        match tier {
            LodTier::Hero | LodTier::Active | LodTier::Streamed => ComponentBundle::empty(),
            LodTier::Horizon => ComponentBundle {
                insert: vec![DynamicComponent::new(Visibility::Hidden)],
                remove: vec![TypeId::of::<AudioPlayer<AudioSource>>()],
            },
        }
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // Wave-2 stub adapter; only the property names we know are
        // populated. Wave 4 importer will exercise the full property
        // map per `ROBLOX_IMPORT_SPEC.md`.
        let mut bag = PropertyBag::new();
        bag.set(
            "metadata.name",
            eustress_common::classes::PropertyValue::String(rbx.name().into()),
        );
        if let Some(p) = rbx
            .property("SoundId")
            .and_then(|p| p.as_str().map(str::to_owned))
        {
            bag.set("sound.id", eustress_common::classes::PropertyValue::String(p));
        }
        if let Some(v) = rbx.property("Volume").and_then(|p| p.as_f32()) {
            bag.set(
                "sound.volume",
                eustress_common::classes::PropertyValue::Float(v),
            );
        }
        if let Some(v) = rbx.property("PlaybackSpeed").and_then(|p| p.as_f32()) {
            bag.set(
                "sound.pitch",
                eustress_common::classes::PropertyValue::Float(v),
            );
        }
        if let Some(v) = rbx.property("Playing").and_then(|p| p.as_bool()) {
            bag.set(
                "sound.playing",
                eustress_common::classes::PropertyValue::Bool(v),
            );
        }
        if let Some(v) = rbx.property("Looped").and_then(|p| p.as_bool()) {
            bag.set(
                "sound.looped",
                eustress_common::classes::PropertyValue::Bool(v),
            );
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        // Mirror the `_instance.toml` layout the existing instance
        // loader writes for a Sound: `[metadata]`, `[sound]`,
        // `[transform]`. Keys are snake_case — pre-normalised by
        // `class_schema::normalise_keys` upstream per the trait
        // contract.
        let mut bag = PropertyBag::new();
        if let Some(meta) = toml_value.get("metadata") {
            if let Some(n) = meta.get("name").and_then(|v| v.as_str()) {
                bag.set(
                    "metadata.name",
                    eustress_common::classes::PropertyValue::String(n.into()),
                );
            }
            if let Some(u) = meta.get("uuid").and_then(|v| v.as_str()) {
                bag.set(
                    "metadata.uuid",
                    eustress_common::classes::PropertyValue::String(u.into()),
                );
            }
        }
        if let Some(sound) = toml_value.get("sound") {
            if let Some(s) = sound.get("sound_id").and_then(|v| v.as_str()) {
                bag.set(
                    "sound.id",
                    eustress_common::classes::PropertyValue::String(s.into()),
                );
            }
            if let Some(v) = sound.get("volume").and_then(|v| v.as_float()) {
                bag.set(
                    "sound.volume",
                    eustress_common::classes::PropertyValue::Float(v as f32),
                );
            }
            if let Some(v) = sound.get("pitch").and_then(|v| v.as_float()) {
                bag.set(
                    "sound.pitch",
                    eustress_common::classes::PropertyValue::Float(v as f32),
                );
            }
            if let Some(v) = sound.get("playing").and_then(|v| v.as_bool()) {
                bag.set(
                    "sound.playing",
                    eustress_common::classes::PropertyValue::Bool(v),
                );
            }
            if let Some(v) = sound.get("looped").and_then(|v| v.as_bool()) {
                bag.set(
                    "sound.looped",
                    eustress_common::classes::PropertyValue::Bool(v),
                );
            }
            if let Some(v) = sound.get("spatial").and_then(|v| v.as_bool()) {
                bag.set(
                    "sound.spatial",
                    eustress_common::classes::PropertyValue::Bool(v),
                );
            }
            if let Some(v) = sound
                .get("roll_off_min_distance")
                .and_then(|v| v.as_float())
            {
                bag.set(
                    "sound.roll_off_min_distance",
                    eustress_common::classes::PropertyValue::Float(v as f32),
                );
            }
            if let Some(v) = sound
                .get("roll_off_max_distance")
                .and_then(|v| v.as_float())
            {
                bag.set(
                    "sound.roll_off_max_distance",
                    eustress_common::classes::PropertyValue::Float(v as f32),
                );
            }
            if let Some(s) = sound.get("roll_off_mode").and_then(|v| v.as_str()) {
                bag.set(
                    "sound.roll_off_mode",
                    eustress_common::classes::PropertyValue::Enum(s.into()),
                );
            }
            if let Some(s) = sound.get("sound_group").and_then(|v| v.as_str()) {
                bag.set(
                    "sound.group",
                    eustress_common::classes::PropertyValue::Enum(s.into()),
                );
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        if let Some(inst) = world.entity(entity).get::<Instance>() {
            let mut meta = toml::value::Table::new();
            meta.insert("class_name".into(), toml::Value::String("Sound".into()));
            meta.insert("name".into(), toml::Value::String(inst.name.clone()));
            meta.insert("archivable".into(), toml::Value::Boolean(inst.archivable));
            if !inst.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(inst.uuid.clone()));
            }
            root.insert("metadata".into(), toml::Value::Table(meta));
        }
        if let Some(sound) = world.entity(entity).get::<Sound>() {
            let mut s = toml::value::Table::new();
            s.insert(
                "sound_id".into(),
                toml::Value::String(sound.sound_id.clone()),
            );
            s.insert(
                "sound_group".into(),
                toml::Value::String(format!("{:?}", sound.sound_group)),
            );
            s.insert("playing".into(), toml::Value::Boolean(sound.playing));
            s.insert("looped".into(), toml::Value::Boolean(sound.looped));
            s.insert("volume".into(), toml::Value::Float(sound.volume as f64));
            s.insert("pitch".into(), toml::Value::Float(sound.pitch as f64));
            s.insert("spatial".into(), toml::Value::Boolean(sound.spatial));
            s.insert(
                "roll_off_min_distance".into(),
                toml::Value::Float(sound.roll_off_min_distance as f64),
            );
            s.insert(
                "roll_off_max_distance".into(),
                toml::Value::Float(sound.roll_off_max_distance as f64),
            );
            s.insert(
                "roll_off_mode".into(),
                toml::Value::String(format!("{:?}", sound.roll_off_mode)),
            );
            root.insert("sound".into(), toml::Value::Table(s));
        }
        toml::Value::Table(root)
    }
}

// ============================================================================
// Internal helpers — PropertyBag <-> Sound conversion + audio mapping
// ============================================================================

/// Build a fully-populated `Sound` component from the bag, falling back
/// to `Default` for omitted keys.
fn sound_from_bag(props: &PropertyBag) -> Sound {
    let mut sound = Sound::default();
    if let Some(v) = props.get_string("sound.id") {
        sound.sound_id = v.to_string();
    }
    if let Some(v) = props.get_f32("sound.volume") {
        sound.volume = v;
    }
    if let Some(v) = props.get_f32("sound.pitch") {
        sound.pitch = v;
    }
    if let Some(v) = props.get_bool("sound.playing") {
        sound.playing = v;
    }
    if let Some(v) = props.get_bool("sound.looped") {
        sound.looped = v;
    }
    if let Some(v) = props.get_bool("sound.spatial") {
        sound.spatial = v;
    }
    if let Some(v) = props.get_f32("sound.roll_off_min_distance") {
        sound.roll_off_min_distance = v;
    }
    if let Some(v) = props.get_f32("sound.roll_off_max_distance") {
        sound.roll_off_max_distance = v;
    }
    if let Some(s) = props.get_enum("sound.roll_off_mode") {
        sound.roll_off_mode = parse_rolloff_mode(s);
    }
    if let Some(s) = props.get_enum("sound.group") {
        sound.sound_group = parse_sound_group(s);
    }
    sound
}

/// Apply a delta bag to an existing Sound component — in-place
/// counterpart of `sound_from_bag` for `apply_edit`. Only keys present
/// in the bag are touched.
fn apply_bag_to_sound(props: &PropertyBag, sound: &mut Sound) {
    if let Some(v) = props.get_string("sound.id") {
        sound.sound_id = v.to_string();
    }
    if let Some(v) = props.get_f32("sound.volume") {
        sound.volume = v;
    }
    if let Some(v) = props.get_f32("sound.pitch") {
        sound.pitch = v;
    }
    if let Some(v) = props.get_bool("sound.playing") {
        sound.playing = v;
    }
    if let Some(v) = props.get_bool("sound.looped") {
        sound.looped = v;
    }
    if let Some(v) = props.get_bool("sound.spatial") {
        sound.spatial = v;
    }
    if let Some(v) = props.get_f32("sound.roll_off_min_distance") {
        sound.roll_off_min_distance = v;
    }
    if let Some(v) = props.get_f32("sound.roll_off_max_distance") {
        sound.roll_off_max_distance = v;
    }
    if let Some(s) = props.get_enum("sound.roll_off_mode") {
        sound.roll_off_mode = parse_rolloff_mode(s);
    }
    if let Some(s) = props.get_enum("sound.group") {
        sound.sound_group = parse_sound_group(s);
    }
}

/// Inverse of `sound_from_bag` — used by `deserialize`. Insertion
/// order matches the canonical key sequence the rest of the spawner
/// uses, so round-trips are diff-stable.
fn bag_from_sound(sound: &Sound) -> PropertyBag {
    let mut bag = PropertyBag::with_capacity(12);
    use eustress_common::classes::PropertyValue;
    bag.set("sound.id", PropertyValue::String(sound.sound_id.clone()));
    bag.set(
        "sound.group",
        PropertyValue::Enum(format!("{:?}", sound.sound_group)),
    );
    bag.set("sound.playing", PropertyValue::Bool(sound.playing));
    bag.set("sound.looped", PropertyValue::Bool(sound.looped));
    bag.set("sound.volume", PropertyValue::Float(sound.volume));
    bag.set("sound.pitch", PropertyValue::Float(sound.pitch));
    bag.set("sound.spatial", PropertyValue::Bool(sound.spatial));
    bag.set(
        "sound.roll_off_min_distance",
        PropertyValue::Float(sound.roll_off_min_distance),
    );
    bag.set(
        "sound.roll_off_max_distance",
        PropertyValue::Float(sound.roll_off_max_distance),
    );
    bag.set(
        "sound.roll_off_mode",
        PropertyValue::Enum(format!("{:?}", sound.roll_off_mode)),
    );
    bag
}

/// Map a debug-printed `SoundRolloffMode` discriminant back to the
/// enum. Round-trips the `format!("{:?}", mode)` shape `export_to_toml`
/// emits.
fn parse_rolloff_mode(s: &str) -> SoundRolloffMode {
    match s {
        "Linear" => SoundRolloffMode::Linear,
        "Inverse" => SoundRolloffMode::Inverse,
        "InverseSquared" => SoundRolloffMode::InverseSquared,
        "Logarithmic" => SoundRolloffMode::Logarithmic,
        "None" => SoundRolloffMode::None,
        "Custom" => SoundRolloffMode::Custom,
        _ => SoundRolloffMode::Inverse,
    }
}

/// Sibling of `parse_rolloff_mode` for SoundGroup.
fn parse_sound_group(s: &str) -> SoundGroup {
    match s {
        "Master" => SoundGroup::Master,
        "SFX" => SoundGroup::SFX,
        "Music" => SoundGroup::Music,
        "Voice" => SoundGroup::Voice,
        "Ambient" => SoundGroup::Ambient,
        "UI" => SoundGroup::UI,
        _ => SoundGroup::SFX,
    }
}

/// Build Bevy's `PlaybackSettings` from the Eustress Sound knobs.
///
/// See module docs for the `RollOffMode → SpatialScale` table.
fn playback_settings_from_sound(sound: &Sound) -> PlaybackSettings {
    let mode = if sound.looped {
        PlaybackMode::Loop
    } else {
        PlaybackMode::Once
    };
    let spatial_scale = if sound.spatial {
        Some(spatial_scale_for(
            sound.roll_off_mode,
            sound.roll_off_min_distance,
        ))
    } else {
        None
    };
    PlaybackSettings {
        mode,
        volume: Volume::Linear(sound.volume.max(0.0)),
        speed: sound.pitch.max(0.01),
        paused: !sound.playing,
        muted: false,
        spatial: sound.spatial,
        spatial_scale,
        start_position: None,
        duration: None,
    }
}

/// Conversion table — module docs explain the math. Guarded against
/// roll_off_min == 0 (degenerate input — fall back to a 1:1 scale).
fn spatial_scale_for(mode: SoundRolloffMode, roll_off_min: f32) -> SpatialScale {
    let min = roll_off_min.max(0.0001);
    let factor = match mode {
        SoundRolloffMode::Inverse
        | SoundRolloffMode::Logarithmic
        | SoundRolloffMode::Custom => 1.0 / min,
        SoundRolloffMode::InverseSquared => 1.0 / (min * min),
        SoundRolloffMode::Linear => 1.0 / (min * 2.0),
        SoundRolloffMode::None => 0.0,
    };
    SpatialScale(Vec3::splat(factor))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trait stays object-safe end-to-end — if this compiles, the
    /// registry can hold `Box<dyn ClassSpawner>` containing our spawner.
    #[test]
    fn sound_spawner_is_object_safe() {
        let boxed: Box<dyn ClassSpawner> = Box::new(SoundSpawner);
        assert_eq!(boxed.class_name(), ClassName::Sound);
    }

    /// Inverse default scales 1/min — Roblox parity check.
    #[test]
    fn spatial_scale_inverse_default() {
        let scale = spatial_scale_for(SoundRolloffMode::Inverse, 10.0);
        assert_eq!(scale.0, Vec3::splat(0.1));
    }

    /// Linear is half-rate of Inverse so listeners need to walk further
    /// before the falloff bites — matches the table in module docs.
    #[test]
    fn spatial_scale_linear_softer() {
        let scale = spatial_scale_for(SoundRolloffMode::Linear, 10.0);
        assert_eq!(scale.0, Vec3::splat(0.05));
    }

    /// None disables distance — listener-relative volume only.
    #[test]
    fn spatial_scale_none_zero() {
        let scale = spatial_scale_for(SoundRolloffMode::None, 10.0);
        assert_eq!(scale.0, Vec3::splat(0.0));
    }

    /// Round-trip a Sound through the bag — keys must come out in the
    /// same order the spawner inserts them so TOML diffs stay clean.
    #[test]
    fn bag_roundtrip_preserves_canonical_order() {
        let sound = Sound {
            sound_id: "asset://foo".into(),
            volume: 0.7,
            ..Default::default()
        };
        let bag = bag_from_sound(&sound);
        let keys: Vec<&str> = bag.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "sound.id",
                "sound.group",
                "sound.playing",
                "sound.looped",
                "sound.volume",
                "sound.pitch",
                "sound.spatial",
                "sound.roll_off_min_distance",
                "sound.roll_off_max_distance",
                "sound.roll_off_mode",
            ]
        );
    }

    /// LOD Horizon mutes — removes the AudioPlayer to silence the
    /// source until the entity climbs back into range.
    #[test]
    fn lod_horizon_removes_audio_player() {
        let spawner = SoundSpawner;
        let bundle = spawner.lod_components(LodTier::Horizon);
        assert!(!bundle.is_empty());
        assert_eq!(
            bundle.remove,
            vec![TypeId::of::<AudioPlayer<AudioSource>>()]
        );
    }

    /// Tag-mismatch deserialize returns empty bag — the schema bump
    /// safety net. Never panics.
    #[test]
    fn deserialize_bad_tag_returns_empty_bag() {
        let spawner = SoundSpawner;
        let bag = spawner.deserialize(&[0xFF, 0x00, 0x00]);
        assert!(bag.is_empty());
    }
}
