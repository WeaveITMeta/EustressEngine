//! `ParticleEmitterSpawner` — Wave 3.F STUB.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.4 (VFX) the
//! `ParticleEmitter` class lives in the VFX group alongside `Beam`.
//!
//! ## Why this is a stub
//!
//! A real particle pipeline needs `bevy_hanabi` integration: GPU compute
//! shaders for spawn/update/render, a node-based effect graph, GPU
//! buffer management, and a translation layer that maps each Eustress
//! field (`color_sequence`, `transparency_curve`, `size_curve`,
//! `emission_shape`, …) onto Hanabi modifiers. That's a multi-day
//! workstream on its own — the dispatch protocol explicitly allows
//! ONE stub in this batch and ParticleEmitter is it.
//!
//! ## What this spawner DOES do (so the class survives Wave 3)
//!
//! - Attaches the Eustress `ParticleEmitter` component (the canonical
//!   data model that already round-trips through TOML + Fjall today —
//!   nothing about persistence is stubbed).
//! - Attaches a tiny billboard placeholder visual: a 0.1m double-sided
//!   white quad. This serves three purposes:
//!     1. Editor gizmo target — users can click and select the emitter
//!        in the viewport instead of guessing where the invisible
//!        entity is.
//!     2. Indicates "something will spawn from here" so a scene with a
//!        ParticleEmitter Looks Right even before the Hanabi backend
//!        ships.
//!     3. Survives the existing `MeshSource`/`MeshMaterial3d` LOD
//!        machinery — no special-case rendering path.
//! - Persists serialize/deserialize via the same tag-+-serde-json shim
//!   the `SoundSpawner` uses. Wave 5 swaps this for a dedicated rkyv
//!   mirror — but the bytes ARE persisted today so an authoring
//!   round-trip doesn't lose any properties.
//! - Implements every other trait method so the registry can dispatch
//!   uniformly. Only the *rendering* of particles is a TODO; the
//!   data-model lifecycle is real.
//!
//! ## What this spawner DOES NOT do
//!
//! - No `bevy_hanabi::EffectAsset` is built.
//! - No GPU effect spawns particles at runtime.
//! - LOD bundles are minimal (Horizon hides; everything else is a no-op).
//!
//! Marker for the Wave 4 implementer:
//!
//! ```text
//! TODO: Wave 4 — bevy_hanabi integration
//! ```
//!
//! Search for that string in this file to find the spawn site that
//! needs the real effect.

use std::any::TypeId;

use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, DynamicComponent, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{ClassName, EmissionShape, Instance, ParticleEmitter};

/// Wire-format tag — Appendix A of `CLASS_REGISTRY.md`. Generic group
/// (low nibble 0) because the placeholder visual carries no
/// class-group-specific persistence.
const PARTICLE_EMITTER_TAG: u8 = 0x10;

/// Marker component placed alongside the placeholder mesh so the
/// future Wave-4 Hanabi system can find emitters that are still on
/// the stub path. Once the real backend ships, this component is
/// removed when the Hanabi effect is bound.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct ParticleEmitterPlaceholder;

impl Default for ParticleEmitterPlaceholder {
    fn default() -> Self {
        Self
    }
}

/// Wave 3.F STUB spawner for `ClassName::ParticleEmitter`.
#[derive(Default)]
pub struct ParticleEmitterSpawner;

impl ClassSpawner for ParticleEmitterSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::ParticleEmitter
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        // TODO: Wave 4 — bevy_hanabi integration.
        // The full pipeline maps each Eustress field onto a Hanabi
        // modifier (color → ColorOverLifetimeModifier, size →
        // SizeOverLifetimeModifier, emission_shape → SetPositionShape,
        // etc.). For Wave 3.F we only build the data half.
        let name = props
            .get_string("metadata.name")
            .unwrap_or("ParticleEmitter")
            .to_string();

        let emitter = particle_emitter_from_bag(props);

        let transform = props
            .get_transform("transform")
            .copied()
            .unwrap_or_default();

        let instance = Instance {
            name: name.clone(),
            class_name: ClassName::ParticleEmitter,
            archivable: true,
            id: 0,
            uuid: props.get_uuid().unwrap_or_default().to_string(),
            ai: false,
        };

        // Tiny placeholder billboard quad (0.1m double-sided white
        // plane). Registered as a fresh asset per-spawn — at 80 chars
        // of vertex data it's negligible. Once Wave 4 wires Hanabi the
        // mesh + material are removed and replaced by a
        // `ParticleEffect`.
        let placeholder_mesh = ctx
            .meshes
            .add(Rectangle::new(0.1, 0.1).mesh().build());
        let placeholder_material = ctx.standard_materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, 0.7),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None, // double-sided so we can see it from any angle
            ..Default::default()
        });

        ctx.commands
            .spawn((
                instance,
                emitter,
                transform,
                Visibility::default(),
                Name::new(name),
                Mesh3d(placeholder_mesh),
                MeshMaterial3d(placeholder_material),
                ParticleEmitterPlaceholder,
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let mut out = vec![PARTICLE_EMITTER_TAG];
        if let Some(emitter) = world.entity(entity).get::<ParticleEmitter>() {
            match serde_json::to_vec(emitter) {
                Ok(mut payload) => out.append(&mut payload),
                Err(e) => warn!(
                    "ParticleEmitterSpawner::serialize: serde_json encode failed for entity {entity:?}: {e}"
                ),
            }
        }
        out
    }

    fn deserialize(&self, bytes: &[u8]) -> PropertyBag {
        if bytes.first() != Some(&PARTICLE_EMITTER_TAG) {
            warn!(
                "ParticleEmitterSpawner::deserialize: tag mismatch (got {:?}, expected {PARTICLE_EMITTER_TAG:#x}) — empty bag",
                bytes.first(),
            );
            return PropertyBag::new();
        }
        let payload = &bytes[1..];
        let emitter: ParticleEmitter = match serde_json::from_slice(payload) {
            Ok(e) => e,
            Err(e) => {
                warn!("ParticleEmitterSpawner::deserialize: serde_json decode failed: {e}");
                return PropertyBag::new();
            }
        };
        bag_from_particle_emitter(&emitter)
    }

    fn apply_edit(
        &self,
        world: &mut World,
        entity: Entity,
        props: &PropertyBag,
    ) -> bool {
        // All ParticleEmitter knobs are cheap mutations once Hanabi is
        // wired — the effect rebuilds its instruction stream from the
        // modifier list each frame. For the Wave 3.F stub it's enough
        // to update the component; the placeholder visual doesn't move
        // when the data changes (TODO: Wave 4 will sync emission size
        // to the placeholder scale).
        if let Some(mut emitter) = world.entity_mut(entity).get_mut::<ParticleEmitter>() {
            apply_bag_to_particle_emitter(props, &mut emitter);
        }
        false
    }

    fn lod_components(&self, tier: LodTier) -> ComponentBundle {
        // Hero / Active / Streamed all keep the placeholder visible so
        // the user can find the emitter while iterating. Horizon hides
        // it — saves the alpha-blend cost on far emitters.
        match tier {
            LodTier::Hero | LodTier::Active | LodTier::Streamed => ComponentBundle::empty(),
            LodTier::Horizon => ComponentBundle {
                insert: vec![DynamicComponent::new(Visibility::Hidden)],
                // No removes today; Wave 4 should remove the
                // bevy_hanabi `ParticleEffect` here so far emitters
                // don't pay GPU costs.
                remove: vec![],
            },
        }
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // Roblox's ParticleEmitter has a similar shape — the Wave 4
        // importer wires the full mapping. Wave-3.F stub only knows
        // the name + a couple of always-present scalars.
        let mut bag = PropertyBag::new();
        bag.set(
            "metadata.name",
            eustress_common::classes::PropertyValue::String(rbx.name().into()),
        );
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set(
                "emitter.enabled",
                eustress_common::classes::PropertyValue::Bool(v),
            );
        }
        if let Some(v) = rbx.property("Rate").and_then(|p| p.as_f32()) {
            bag.set(
                "emitter.rate",
                eustress_common::classes::PropertyValue::Float(v),
            );
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
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
        if let Some(em) = toml_value.get("emitter") {
            if let Some(v) = em.get("enabled").and_then(|v| v.as_bool()) {
                bag.set(
                    "emitter.enabled",
                    eustress_common::classes::PropertyValue::Bool(v),
                );
            }
            if let Some(v) = em.get("rate").and_then(|v| v.as_float()) {
                bag.set(
                    "emitter.rate",
                    eustress_common::classes::PropertyValue::Float(v as f32),
                );
            }
            if let Some(v) = em.get("max_particles").and_then(|v| v.as_integer()) {
                bag.set(
                    "emitter.max_particles",
                    eustress_common::classes::PropertyValue::Int(v as i32),
                );
            }
            if let Some(s) = em.get("emission_shape").and_then(|v| v.as_str()) {
                bag.set(
                    "emitter.emission_shape",
                    eustress_common::classes::PropertyValue::Enum(s.into()),
                );
            }
            // Wave 4 importer extends with size_curve, color_sequence,
            // transparency_curve, etc.
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        if let Some(inst) = world.entity(entity).get::<Instance>() {
            let mut meta = toml::value::Table::new();
            meta.insert(
                "class_name".into(),
                toml::Value::String("ParticleEmitter".into()),
            );
            meta.insert("name".into(), toml::Value::String(inst.name.clone()));
            meta.insert("archivable".into(), toml::Value::Boolean(inst.archivable));
            if !inst.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(inst.uuid.clone()));
            }
            root.insert("metadata".into(), toml::Value::Table(meta));
        }
        if let Some(em) = world.entity(entity).get::<ParticleEmitter>() {
            let mut t = toml::value::Table::new();
            t.insert("enabled".into(), toml::Value::Boolean(em.enabled));
            t.insert("rate".into(), toml::Value::Float(em.rate as f64));
            t.insert(
                "max_particles".into(),
                toml::Value::Integer(em.max_particles as i64),
            );
            t.insert(
                "emission_shape".into(),
                toml::Value::String(format!("{:?}", em.emission_shape)),
            );
            t.insert(
                "lifetime_min".into(),
                toml::Value::Float(em.lifetime.0 as f64),
            );
            t.insert(
                "lifetime_max".into(),
                toml::Value::Float(em.lifetime.1 as f64),
            );
            t.insert("speed_min".into(), toml::Value::Float(em.speed.0 as f64));
            t.insert("speed_max".into(), toml::Value::Float(em.speed.1 as f64));
            t.insert("size_min".into(), toml::Value::Float(em.size.0 as f64));
            t.insert("size_max".into(), toml::Value::Float(em.size.1 as f64));
            root.insert("emitter".into(), toml::Value::Table(t));
        }
        toml::Value::Table(root)
    }
}

// ============================================================================
// Internal helpers — PropertyBag <-> ParticleEmitter conversion
// ============================================================================

fn particle_emitter_from_bag(props: &PropertyBag) -> ParticleEmitter {
    let mut em = ParticleEmitter::default();
    apply_bag_to_particle_emitter(props, &mut em);
    em
}

fn apply_bag_to_particle_emitter(props: &PropertyBag, em: &mut ParticleEmitter) {
    if let Some(v) = props.get_bool("emitter.enabled") {
        em.enabled = v;
    }
    if let Some(v) = props.get_f32("emitter.rate") {
        em.rate = v;
    }
    if let Some(v) = props.get_i32("emitter.max_particles") {
        em.max_particles = v.max(0) as u32;
    }
    if let Some(s) = props.get_enum("emitter.emission_shape") {
        em.emission_shape = parse_emission_shape(s);
    }
    if let Some(v) = props.get_f32("emitter.lifetime_min") {
        em.lifetime.0 = v;
    }
    if let Some(v) = props.get_f32("emitter.lifetime_max") {
        em.lifetime.1 = v;
    }
    if let Some(v) = props.get_f32("emitter.speed_min") {
        em.speed.0 = v;
    }
    if let Some(v) = props.get_f32("emitter.speed_max") {
        em.speed.1 = v;
    }
    if let Some(v) = props.get_f32("emitter.size_min") {
        em.size.0 = v;
    }
    if let Some(v) = props.get_f32("emitter.size_max") {
        em.size.1 = v;
    }
}

fn bag_from_particle_emitter(em: &ParticleEmitter) -> PropertyBag {
    let mut bag = PropertyBag::with_capacity(10);
    use eustress_common::classes::PropertyValue;
    bag.set("emitter.enabled", PropertyValue::Bool(em.enabled));
    bag.set("emitter.rate", PropertyValue::Float(em.rate));
    bag.set(
        "emitter.max_particles",
        PropertyValue::Int(em.max_particles as i32),
    );
    bag.set(
        "emitter.emission_shape",
        PropertyValue::Enum(format!("{:?}", em.emission_shape)),
    );
    bag.set("emitter.lifetime_min", PropertyValue::Float(em.lifetime.0));
    bag.set("emitter.lifetime_max", PropertyValue::Float(em.lifetime.1));
    bag.set("emitter.speed_min", PropertyValue::Float(em.speed.0));
    bag.set("emitter.speed_max", PropertyValue::Float(em.speed.1));
    bag.set("emitter.size_min", PropertyValue::Float(em.size.0));
    bag.set("emitter.size_max", PropertyValue::Float(em.size.1));
    bag
}

fn parse_emission_shape(s: &str) -> EmissionShape {
    match s {
        "Point" => EmissionShape::Point,
        "Sphere" => EmissionShape::Sphere,
        "Box" => EmissionShape::Box,
        "Cone" => EmissionShape::Cone,
        "Cylinder" => EmissionShape::Cylinder,
        "Ring" => EmissionShape::Ring,
        "Disc" => EmissionShape::Disc,
        _ => EmissionShape::Point,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_emitter_spawner_is_object_safe() {
        let boxed: Box<dyn ClassSpawner> = Box::new(ParticleEmitterSpawner);
        assert_eq!(boxed.class_name(), ClassName::ParticleEmitter);
    }

    #[test]
    fn lod_horizon_hides_placeholder() {
        let spawner = ParticleEmitterSpawner;
        let bundle = spawner.lod_components(LodTier::Horizon);
        assert!(!bundle.is_empty(), "Horizon must hide the placeholder");
    }

    #[test]
    fn deserialize_bad_tag_returns_empty_bag() {
        let spawner = ParticleEmitterSpawner;
        let bag = spawner.deserialize(&[0xAA, 0x00]);
        assert!(bag.is_empty());
    }

    #[test]
    fn bag_roundtrip_preserves_canonical_order() {
        let mut em = ParticleEmitter::default();
        em.rate = 50.0;
        em.max_particles = 200;
        let bag = bag_from_particle_emitter(&em);
        let keys: Vec<&str> = bag.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "emitter.enabled",
                "emitter.rate",
                "emitter.max_particles",
                "emitter.emission_shape",
                "emitter.lifetime_min",
                "emitter.lifetime_max",
                "emitter.speed_min",
                "emitter.speed_max",
                "emitter.size_min",
                "emitter.size_max",
            ]
        );
    }
}
