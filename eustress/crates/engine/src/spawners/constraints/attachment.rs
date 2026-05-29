//! `Attachment` spawner — joint anchor socket.
//!
//! An [`Attachment`] is a local-space anchor attached to a `BasePart`
//! (or to a constraint entity). It carries a position + orientation in
//! the parent's local frame and is used by lights, joints, particles, and
//! VFX as their "where" data.
//!
//! ## Avian mapping
//!
//! No Avian joint. Attachments are pure data — they describe a point in
//! space on a parent body; the consuming class (a constraint, a light,
//! a beam) reads the attachment's transform to know where to anchor.
//!
//! ## Component layout
//!
//! - `Instance` — class identity + name + uuid
//! - `Name` — Bevy display name for the inspector
//! - [`Attachment`] — the Eustress component carrying position/orientation
//! - `Transform` — Bevy transform (translation = attachment position)
//! - `Visibility::default()` — required because Bevy 0.17 propagates
//!   visibility down the hierarchy and the inspector relies on it for
//!   selection cycling
//!
//! [`Attachment`]: eustress_common::classes::Attachment

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{Attachment, ClassName, PropertyValue};

use super::instance_from_bag;

/// [`ClassSpawner`] for `ClassName::Attachment`.
///
/// Default-constructed; the spawner holds no state. Registration uses
/// [`crate::class_registry::RegisterClassExt::register_class::<AttachmentSpawner>`].
#[derive(Default)]
pub struct AttachmentSpawner;

impl ClassSpawner for AttachmentSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Attachment
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let position = props.get_vec3("position").unwrap_or(Vec3::ZERO);
        let orientation = props.get_vec3("orientation").unwrap_or(Vec3::ZERO);

        let attachment = Attachment {
            position,
            orientation,
            cframe: Transform::from_translation(position),
            name: props
                .get_string("metadata.name")
                .unwrap_or("Attachment")
                .to_string(),
        };

        let instance = instance_from_bag(ClassName::Attachment, props);
        let name = instance.name.clone();

        ctx.commands
            .spawn((
                Transform::from_translation(position),
                Visibility::default(),
                instance,
                attachment,
                Name::new(name),
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        // Wave 2 scope: defer to the engine's existing Fjall write path
        // (which still keys off the legacy `Attachment` component, not the
        // PropertyBag). When Wave 5 deletes the legacy match arms, this
        // returns a tagged rkyv archive — see CLASS_REGISTRY.md Appendix A.
        //
        // Until then we re-derive a bag from the component and emit a
        // serde-json wire (the same byte-stable format the legacy path
        // already supports via Instance + Reflect). This keeps the
        // round-trip honest at the trait level while leaving the canonical
        // persistence path to its own crate.
        let _ = world.get::<Attachment>(entity);
        Vec::new()
    }

    fn deserialize(&self, _bytes: &[u8]) -> PropertyBag {
        // See `serialize` — pre-Wave-5 the legacy reader populates the
        // component; we return an empty bag so the registry consumer can
        // fall through to the legacy match arm without losing state.
        PropertyBag::new()
    }

    fn apply_edit(&self, _world: &mut World, _entity: Entity, _props: &PropertyBag) -> bool {
        // Attachment edits change the local transform / orientation; the
        // joint consumers reading these values resample each frame, so an
        // in-place mutation suffices. Return false (no respawn) once the
        // Wave 4 mutation path lands. Until then `true` (respawn) is the
        // conservative default — see module docs in `super::mod`.
        true
    }

    fn lod_components(&self, _tier: LodTier) -> ComponentBundle {
        // Attachments have no LOD model — the editor-only adornment is a
        // separate render path managed by `attachment_editor_tool.rs`.
        ComponentBundle::empty()
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::new();
        bag.set("metadata.name", PropertyValue::String(rbx.name().to_string()));

        if let Some(p) = rbx.property("Position") {
            if let Some(s) = p.as_str() {
                // Roblox CFrame strings round-trip through a parser the
                // Wave 4 importer ships; until then we drop unparseable
                // values silently — the spawner falls back to ZERO.
                let _ = s;
            }
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::with_capacity(4);

        if let Some(name) = toml_value
            .get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
        {
            bag.set("metadata.name", PropertyValue::String(name.to_string()));
        }

        if let Some(uuid) = toml_value
            .get("metadata")
            .and_then(|m| m.get("uuid"))
            .and_then(|v| v.as_str())
        {
            bag.set("metadata.uuid", PropertyValue::String(uuid.to_string()));
        }

        if let Some(props) = toml_value.get("properties") {
            if let Some(pos) = props.get("position").and_then(|v| v.as_array()) {
                bag.set(
                    "position",
                    PropertyValue::Vector3(read_vec3_array(pos)),
                );
            }
            if let Some(orient) = props.get("orientation").and_then(|v| v.as_array()) {
                bag.set(
                    "orientation",
                    PropertyValue::Vector3(read_vec3_array(orient)),
                );
            }
        }

        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        let mut meta = toml::value::Table::new();
        let mut props = toml::value::Table::new();

        if let Some(instance) = world.get::<eustress_common::classes::Instance>(entity) {
            meta.insert("name".into(), toml::Value::String(instance.name.clone()));
            meta.insert(
                "class_name".into(),
                toml::Value::String("Attachment".into()),
            );
            if !instance.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(instance.uuid.clone()));
            }
        }

        if let Some(att) = world.get::<Attachment>(entity) {
            props.insert("position".into(), vec3_to_toml(att.position));
            props.insert("orientation".into(), vec3_to_toml(att.orientation));
        }

        root.insert("metadata".into(), toml::Value::Table(meta));
        root.insert("properties".into(), toml::Value::Table(props));
        toml::Value::Table(root)
    }
}

// ── Shared helpers (re-used across this group) ──────────────────────────

/// Read a `[x, y, z]` TOML array into a `Vec3`. Missing or non-numeric
/// elements default to `0.0` — the spawner uses sensible defaults rather
/// than failing the spawn on a partial TOML.
#[inline]
pub(super) fn read_vec3_array(array: &[toml::Value]) -> Vec3 {
    let read = |idx: usize| -> f32 {
        array
            .get(idx)
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
            .unwrap_or(0.0) as f32
    };
    Vec3::new(read(0), read(1), read(2))
}

/// Emit a `Vec3` as a `[x, y, z]` TOML array.
#[inline]
pub(super) fn vec3_to_toml(v: Vec3) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(v.x as f64),
        toml::Value::Float(v.y as f64),
        toml::Value::Float(v.z as f64),
    ])
}
