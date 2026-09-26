//! `SoundSpawner`: the Sound class's spawner, and the Sound component a
//! loaded Sound file gets.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.7 (Camera & Audio) the
//! `Sound` class is one of two members of the audio group. This module
//! builds the entity and its Eustress `Sound` component (the canonical data
//! model round-tripped through TOML and Fjall persistence). It does not play
//! anything: the shared player (`eustress_play_runtime::sound_player`) plays
//! a Sound component on its own entity, a Properties preview in Edit and a
//! Play session's Sounds in Play, with Roblox's roll-off.
//!
//! A Sound file's `[sound]` section is read and written by
//! `eustress_common::services::sound` (`sound_from_section`,
//! `write_sound_section`), which every reader shares, so Studio and the
//! Player cannot disagree on a file. [`attach_sound_component`] is how a
//! Sound loaded from a file gets its component.
//!
//! The entity gets `Transform` + `Visibility` so it participates in the
//! standard ECS hierarchy: a Sound sits in the scene tree under its parent
//! Part or Attachment like any other instance, and is heard from there.

use std::any::TypeId;
use std::collections::HashMap;

use bevy::audio::{AudioPlayer, AudioSource};
use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, DynamicComponent, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, Instance, Sound};
use eustress_common::services::sound::{
    parse_rolloff_mode, parse_sound_group, rolloff_mode_name, sound_from_section, sound_group_name,
    write_sound_section,
};

/// Wire-format tag for the Wave-3.F rkyv archives. Follows Appendix A of
/// `CLASS_REGISTRY.md`: high nibble = schema_version (1), low nibble =
/// class group (0 = generic). Audio sits in the generic group because it
/// carries only Eustress-side data — no engine-only material/handle
/// fields that would need a dedicated group.
const SOUND_TAG: u8 = 0x10;

/// Give a Sound loaded from its instance file its `Sound` component, read
/// from the file's `[sound]` section (the class defaults when it has none).
/// Every other class is left alone. `extra` is the file's sections beyond
/// the ones the loader reads itself.
pub fn attach_sound_component(
    ec: &mut bevy::ecs::system::EntityCommands,
    class_name: ClassName,
    extra: &HashMap<String, toml::Value>,
) {
    if class_name != ClassName::Sound {
        return;
    }
    ec.insert(sound_from_extra(extra));
}

fn sound_from_extra(extra: &HashMap<String, toml::Value>) -> Sound {
    extra
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("sound"))
        .and_then(|(_, v)| v.as_table())
        .map(sound_from_section)
        .unwrap_or_default()
}

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
        // Playback is the shared player's: nothing here starts a clip.
        let sound = sound_from_bag(props);

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

        ctx.commands
            .spawn((instance, sound, transform, Visibility::default(), Name::new(name)))
            .id()
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
        // Every Sound field is live: the shared player reads the component
        // each frame (and restarts a clip itself when SoundId or Looped
        // changes). Returning `false` keeps the Properties panel off the
        // despawn-respawn dance.
        if let Some(mut sound) = world.entity_mut(entity).get_mut::<Sound>() {
            apply_bag_to_sound(props, &mut sound);
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
        // The `_instance.toml` layout: `[metadata]`, `[sound]`,
        // `[transform]`. `[sound]` is read by the shared reader, which
        // takes every spelling its writers have used.
        let sound = toml_value
            .get("sound")
            .and_then(|v| v.as_table())
            .map(sound_from_section)
            .unwrap_or_default();
        let mut bag = bag_from_sound(&sound);
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
            write_sound_section(sound, &mut s);
            root.insert("sound".into(), toml::Value::Table(s));
        }
        toml::Value::Table(root)
    }
}

// ============================================================================
// Internal helpers — PropertyBag <-> Sound conversion
// ============================================================================

/// Build a fully-populated `Sound` component from the bag, falling back
/// to `Default` for omitted keys.
fn sound_from_bag(props: &PropertyBag) -> Sound {
    let mut sound = Sound::default();
    apply_bag_to_sound(props, &mut sound);
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
    // PlaybackSpeed travels as `sound.pitch`; both fields carry it.
    if let Some(v) = props.get_f32("sound.pitch") {
        sound.pitch = v;
        sound.playback_speed = v;
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
    if let Some(m) = props.get_enum("sound.roll_off_mode").and_then(parse_rolloff_mode) {
        sound.roll_off_mode = m;
    }
    if let Some(g) = props.get_enum("sound.group").and_then(parse_sound_group) {
        sound.sound_group = g;
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
        PropertyValue::Enum(sound_group_name(sound.sound_group).to_string()),
    );
    bag.set("sound.playing", PropertyValue::Bool(sound.playing));
    bag.set("sound.looped", PropertyValue::Bool(sound.looped));
    bag.set("sound.volume", PropertyValue::Float(sound.volume));
    bag.set("sound.pitch", PropertyValue::Float(sound.playback_speed));
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
        PropertyValue::Enum(rolloff_mode_name(sound.roll_off_mode).to_string()),
    );
    bag
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::classes::SoundRolloffMode;

    /// The trait stays object-safe end-to-end — if this compiles, the
    /// registry can hold `Box<dyn ClassSpawner>` containing our spawner.
    #[test]
    fn sound_spawner_is_object_safe() {
        let boxed: Box<dyn ClassSpawner> = Box::new(SoundSpawner);
        assert_eq!(boxed.class_name(), ClassName::Sound);
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
        let back = sound_from_bag(&bag);
        assert_eq!(back.volume, 0.7);
        assert_eq!(back.roll_off_mode, sound.roll_off_mode);
    }

    /// A Sound file's section reaches the component in any spelling, and a
    /// file with no `[sound]` gets the class defaults.
    #[test]
    fn a_loaded_sound_gets_its_component_from_the_file() {
        let doc: toml::Table = "[sound]\nrolloff_mode = \"Linear\"\nplayback_speed = 2.0\nvolume = 0.3"
            .parse()
            .unwrap();
        let extra: HashMap<String, toml::Value> = doc.into_iter().collect();
        let sound = sound_from_extra(&extra);
        assert_eq!(sound.roll_off_mode, SoundRolloffMode::Linear);
        assert_eq!(sound.playback_speed, 2.0);
        assert_eq!(sound.volume, 0.3);
        assert_eq!(sound_from_extra(&HashMap::new()).volume, Sound::default().volume);
    }

    /// A Sound exported to TOML imports back with the same fields.
    #[test]
    fn export_then_import_keeps_the_fields() {
        let mut world = World::new();
        let sound = Sound {
            sound_id: "space://SoundService/flush.ogg".into(),
            volume: 0.25,
            looped: true,
            roll_off_mode: SoundRolloffMode::InverseSquared,
            ..Default::default()
        };
        let e = world
            .spawn((
                Instance { name: "Flush".into(), class_name: ClassName::Sound, ..Default::default() },
                sound,
            ))
            .id();
        let exported = SoundSpawner.export_to_toml(&world, e);
        let bag = SoundSpawner.import_from_toml(&exported);
        let back = sound_from_bag(&bag);
        assert_eq!(back.sound_id, "space://SoundService/flush.ogg");
        assert_eq!(back.volume, 0.25);
        assert!(back.looped);
        assert_eq!(back.roll_off_mode, SoundRolloffMode::InverseSquared);
        assert_eq!(bag.get_string("metadata.name"), Some("Flush"));
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
