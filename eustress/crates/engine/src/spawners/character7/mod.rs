//! # Character / players / animation spawners — Wave 7.D
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md`:
//! the 24 Roblox character / player / animation classes. Each spawner attaches
//! its config (or marker) component + the cross-cutting [`Instance`] /
//! [`Name`] and persists it.
//!
//! ## Pattern — config / container attach
//!
//! Most of these are **controller markers** (`AnimationController`,
//! `HumanoidController`, the locomotion controllers, `Backpack`,
//! `StarterGear`, `CurveAnimation`, `AnimationRigData`) carrying no authored
//! fields; the spawner attaches the unit component. The rest carry a small
//! config struct (`Animation.AnimationId`, `HumanoidDescription`'s body-part
//! asset ids + scales, `Pose`'s CFrame + easing, …). The behavioral runtime
//! (locomotion solve, animation playback, IK) is a later phase — this group
//! wires spawn + import/export only, mirroring the Wave 6.A ValueObject shape.
//!
//! ## Why no LOD / stub persistence
//!
//! These are config/container carriers, not LOD-managed renderables;
//! `lod_components` returns an empty bundle at every tier and
//! `serialize`/`deserialize` are stubbed (TOML round-trip carries the value)
//! until a later wave lights up the Fjall write path.
//!
//! [`Instance`]: eustress_common::classes::Instance

use bevy::prelude::*;

use eustress_common::class_registry::{PropertyBag, RegisterClassExt};
use eustress_common::classes::{ClassName, Instance, PropertyValue};

pub mod accessory_description;
pub mod accoutrement;
pub mod air_controller;
pub mod animation;
pub mod animation_controller;
pub mod animation_rig_data;
pub mod backpack;
pub mod body_part_description;
pub mod climb_controller;
pub mod controller_manager;
pub mod controller_part_sensor;
pub mod curve_animation;
pub mod face_controls;
pub mod ground_controller;
pub mod humanoid_controller;
pub mod humanoid_description;
pub mod ik_control;
pub mod keyframe_marker;
pub mod number_pose;
pub mod pose;
pub mod skateboard_controller;
pub mod starter_gear;
pub mod swim_controller;
pub mod vehicle_controller;

pub use accessory_description::AccessoryDescriptionSpawner;
pub use accoutrement::AccoutrementSpawner;
pub use air_controller::AirControllerSpawner;
pub use animation::AnimationSpawner;
pub use animation_controller::AnimationControllerSpawner;
pub use animation_rig_data::AnimationRigDataSpawner;
pub use backpack::BackpackSpawner;
pub use body_part_description::BodyPartDescriptionSpawner;
pub use climb_controller::ClimbControllerSpawner;
pub use controller_manager::ControllerManagerSpawner;
pub use controller_part_sensor::ControllerPartSensorSpawner;
pub use curve_animation::CurveAnimationSpawner;
pub use face_controls::FaceControlsSpawner;
pub use ground_controller::GroundControllerSpawner;
pub use humanoid_controller::HumanoidControllerSpawner;
pub use humanoid_description::HumanoidDescriptionSpawner;
pub use ik_control::IKControlSpawner;
pub use keyframe_marker::KeyframeMarkerSpawner;
pub use number_pose::NumberPoseSpawner;
pub use pose::PoseSpawner;
pub use skateboard_controller::SkateboardControllerSpawner;
pub use starter_gear::StarterGearSpawner;
pub use swim_controller::SwimControllerSpawner;
pub use vehicle_controller::VehicleControllerSpawner;

/// Bevy plugin registering every character / animation spawner shipped by
/// Wave 7.D with the
/// [`ClassRegistry`][eustress_common::class_registry::ClassRegistry].
pub struct Character7SpawnerPlugin;

impl Plugin for Character7SpawnerPlugin {
    fn build(&self, app: &mut App) {
        app.register_class::<AnimationSpawner>()
            .register_class::<AnimationControllerSpawner>()
            .register_class::<HumanoidControllerSpawner>()
            .register_class::<ControllerManagerSpawner>()
            .register_class::<AirControllerSpawner>()
            .register_class::<ClimbControllerSpawner>()
            .register_class::<GroundControllerSpawner>()
            .register_class::<SwimControllerSpawner>()
            .register_class::<SkateboardControllerSpawner>()
            .register_class::<VehicleControllerSpawner>()
            .register_class::<ControllerPartSensorSpawner>()
            .register_class::<HumanoidDescriptionSpawner>()
            .register_class::<BodyPartDescriptionSpawner>()
            .register_class::<BackpackSpawner>()
            .register_class::<StarterGearSpawner>()
            .register_class::<AccoutrementSpawner>()
            .register_class::<AccessoryDescriptionSpawner>()
            .register_class::<FaceControlsSpawner>()
            .register_class::<IKControlSpawner>()
            .register_class::<KeyframeMarkerSpawner>()
            .register_class::<PoseSpawner>()
            .register_class::<NumberPoseSpawner>()
            .register_class::<CurveAnimationSpawner>()
            .register_class::<AnimationRigDataSpawner>();
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Build the cross-cutting [`Instance`] every character/animation entity carries.
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

/// Read a `[f32; 3]`-shaped TOML array into a [`Vec3`].
pub(crate) fn read_vec3_array(arr: &[toml::Value]) -> Vec3 {
    let get = |i: usize| arr.get(i).and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
    Vec3::new(get(0), get(1), get(2))
}

/// Serialize a [`Vec3`] to a 3-element TOML float array.
pub(crate) fn vec3_to_toml(v: Vec3) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(v.x as f64),
        toml::Value::Float(v.y as f64),
        toml::Value::Float(v.z as f64),
    ])
}

/// Read an optional Eustress instance-id reference from the bag.
pub(crate) fn read_optional_ref(bag: &PropertyBag, key: &str) -> Option<u32> {
    bag.get_i32(key)
        .and_then(|i| if i < 0 { None } else { Some(i as u32) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};

    const CHARACTER_CLASSES: [ClassName; 24] = [
        ClassName::Animation,
        ClassName::AnimationController,
        ClassName::HumanoidController,
        ClassName::ControllerManager,
        ClassName::AirController,
        ClassName::ClimbController,
        ClassName::GroundController,
        ClassName::SwimController,
        ClassName::SkateboardController,
        ClassName::VehicleController,
        ClassName::ControllerPartSensor,
        ClassName::HumanoidDescription,
        ClassName::BodyPartDescription,
        ClassName::Backpack,
        ClassName::StarterGear,
        ClassName::Accoutrement,
        ClassName::AccessoryDescription,
        ClassName::FaceControls,
        ClassName::IKControl,
        ClassName::KeyframeMarker,
        ClassName::Pose,
        ClassName::NumberPose,
        ClassName::CurveAnimation,
        ClassName::AnimationRigData,
    ];

    #[test]
    fn plugin_registers_all_twenty_four_character_classes() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(Character7SpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        for class in CHARACTER_CLASSES {
            assert!(registry.contains(class), "must register {}", class.as_str());
            let spawner: &dyn ClassSpawner = registry.get(class).unwrap();
            assert_eq!(spawner.class_name(), class);
        }
        assert_eq!(registry.len(), 24);
    }

    #[test]
    fn class_name_round_trips_for_every_character_class() {
        for class in CHARACTER_CLASSES {
            assert_eq!(ClassName::from_str(class.as_str()), Ok(class));
        }
    }
}
