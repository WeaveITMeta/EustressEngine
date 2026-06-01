//! # Legacy joint / mover spawners — Wave 7.A
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md`:
//! the 7 Roblox legacy joint / force classes (predecessors of the
//! `*Constraint` family) that pin or drive parts.
//!
//! | `ClassName`             | Component attached       | Future Avian wiring |
//! |-------------------------|--------------------------|---------------------|
//! | `Weld`                  | [`Weld`]                 | `FixedJoint` (TODO) |
//! | `Motor`                 | [`Motor`]                | `RevoluteJoint` + motor (TODO) |
//! | `VelocityMotor`         | [`VelocityMotor`]        | `RevoluteJoint` + velocity drive (TODO) |
//! | `NoCollisionConstraint` | [`NoCollisionConstraint`]| collision-layer filter (TODO) |
//! | `RigidConstraint`       | [`RigidConstraint`]      | `FixedJoint` between attachments (TODO) |
//! | `LineForce`             | [`LineForce`]            | per-frame applied force (TODO) |
//! | `AnimationConstraint`   | [`AnimationConstraint`]  | drive toward target pose (TODO) |
//!
//! ## Pattern — config-attach, defer the physics wiring
//!
//! Unlike the Wave 3 `constraints/` group (which wires Avian joints directly),
//! these legacy classes follow the **Wave 6.B mover convention**: the spawner
//! attaches the Eustress config component + the cross-cutting [`Instance`] /
//! [`Name`] and persists it; the actual Avian joint / force actuation is left
//! to a later runtime phase (see the per-class `// TODO(avian)` notes). This
//! keeps the spawner a pure "build this entity" recipe and avoids spawning
//! placeholder-body joints that have no resolver system yet on this branch.
//!
//! Entity references (`Part0`/`Part1`/`Attachment0`/…) are legacy Eustress
//! instance IDs (`Option<u32>`); they round-trip through the bag as `Int`
//! (`-1` ⇒ `None`) and through TOML as integers, exactly like the Wave 3
//! `WeldConstraint` spawner does.
//!
//! ## Why no LOD / stub persistence
//!
//! Joints/forces have no independent visual; `lod_components` returns an empty
//! bundle at every tier and `serialize`/`deserialize` are stubbed (TOML
//! round-trip carries the value) until a later wave lights up the Fjall write
//! path.
//!
//! [`Weld`]: eustress_common::classes::Weld
//! [`Motor`]: eustress_common::classes::Motor
//! [`VelocityMotor`]: eustress_common::classes::VelocityMotor
//! [`NoCollisionConstraint`]: eustress_common::classes::NoCollisionConstraint
//! [`RigidConstraint`]: eustress_common::classes::RigidConstraint
//! [`LineForce`]: eustress_common::classes::LineForce
//! [`AnimationConstraint`]: eustress_common::classes::AnimationConstraint
//! [`Instance`]: eustress_common::classes::Instance

use bevy::prelude::*;

use eustress_common::class_registry::{PropertyBag, RegisterClassExt};
use eustress_common::classes::{ClassName, Instance, PropertyValue};

pub mod animation_constraint;
pub mod line_force;
pub mod motor;
pub mod no_collision_constraint;
pub mod rigid_constraint;
pub mod velocity_motor;
pub mod weld;

pub use animation_constraint::AnimationConstraintSpawner;
pub use line_force::LineForceSpawner;
pub use motor::MotorSpawner;
pub use no_collision_constraint::NoCollisionConstraintSpawner;
pub use rigid_constraint::RigidConstraintSpawner;
pub use velocity_motor::VelocityMotorSpawner;
pub use weld::WeldSpawner;

/// Bevy plugin registering every legacy joint / mover spawner shipped by
/// Wave 7.A with the
/// [`ClassRegistry`][eustress_common::class_registry::ClassRegistry].
pub struct LegacyJointsSpawnerPlugin;

impl Plugin for LegacyJointsSpawnerPlugin {
    fn build(&self, app: &mut App) {
        app.register_class::<WeldSpawner>()
            .register_class::<MotorSpawner>()
            .register_class::<VelocityMotorSpawner>()
            .register_class::<NoCollisionConstraintSpawner>()
            .register_class::<RigidConstraintSpawner>()
            .register_class::<LineForceSpawner>()
            .register_class::<AnimationConstraintSpawner>();
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Build the cross-cutting [`Instance`] every legacy-joint entity carries.
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

/// Read an optional Eustress instance-id reference from the bag. Stored as
/// `Int`; a negative value (or absence) means `None` — "unresolved at spawn".
pub(crate) fn read_optional_ref(bag: &PropertyBag, key: &str) -> Option<u32> {
    bag.get_i32(key)
        .and_then(|i| if i < 0 { None } else { Some(i as u32) })
}

/// Insert an `Option<u32>` reference into a TOML props table as an integer
/// (skipped when `None`, matching the Wave 3 WeldConstraint export).
pub(crate) fn insert_optional_ref(props: &mut toml::value::Table, key: &str, value: Option<u32>) {
    if let Some(v) = value {
        props.insert(key.to_string(), toml::Value::Integer(v as i64));
    }
}

/// Read a `c0`/`c1`-style Transform from the bag (translation-only, matching
/// the Wave 3 constraint convention; rotation defaults to identity).
pub(crate) fn read_anchor_transform(bag: &PropertyBag, key: &str) -> Transform {
    if let Some(t) = bag.get_transform(key) {
        return *t;
    }
    if let Some(v) = bag.get_vec3(key) {
        return Transform::from_translation(v);
    }
    Transform::IDENTITY
}

/// Serialize an anchor [`Transform`]'s translation to a 3-element TOML array
/// (matching the Wave 3 WeldConstraint `c0`/`c1` export shape).
pub(crate) fn anchor_to_toml(t: Transform) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(t.translation.x as f64),
        toml::Value::Float(t.translation.y as f64),
        toml::Value::Float(t.translation.z as f64),
    ])
}

/// Read a translation-only Transform from a TOML 3-element array.
pub(crate) fn read_anchor_array(arr: &[toml::Value]) -> Transform {
    let get = |i: usize| arr.get(i).and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
    Transform::from_translation(Vec3::new(get(0), get(1), get(2)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};

    const LEGACY_JOINT_CLASSES: [ClassName; 7] = [
        ClassName::Weld,
        ClassName::Motor,
        ClassName::VelocityMotor,
        ClassName::NoCollisionConstraint,
        ClassName::RigidConstraint,
        ClassName::LineForce,
        ClassName::AnimationConstraint,
    ];

    #[test]
    fn plugin_registers_all_seven_legacy_joint_classes() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(LegacyJointsSpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        for class in LEGACY_JOINT_CLASSES {
            assert!(registry.contains(class), "must register {}", class.as_str());
            let spawner: &dyn ClassSpawner = registry.get(class).unwrap();
            assert_eq!(spawner.class_name(), class);
        }
        assert_eq!(registry.len(), 7);
    }

    #[test]
    fn class_name_round_trips_for_every_legacy_joint_class() {
        for class in LEGACY_JOINT_CLASSES {
            assert_eq!(ClassName::from_str(class.as_str()), Ok(class));
        }
    }
}
