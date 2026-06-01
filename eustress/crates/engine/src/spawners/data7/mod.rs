//! # Data / curve / misc spawners — Wave 7.F
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md`:
//! the 16 Roblox data-struct / curve / misc classes. Each spawner attaches
//! its config (or marker) component + the cross-cutting [`Instance`] /
//! [`Name`] and persists it.
//!
//! | `ClassName`                 | Component attached            |
//! |-----------------------------|-------------------------------|
//! | `DataStoreGetOptions`       | [`DataStoreGetOptions`]       |
//! | `DataStoreSetOptions`       | [`DataStoreSetOptions`] (marker) |
//! | `DataStoreIncrementOptions` | [`DataStoreIncrementOptions`] (marker) |
//! | `DataStoreOptions`          | [`DataStoreOptions`]          |
//! | `FloatCurve`                | [`FloatCurve`] (marker)       |
//! | `RotationCurve`             | [`RotationCurve`] (marker)    |
//! | `EulerRotationCurve`        | [`EulerRotationCurve`] (marker) |
//! | `Vector3Curve`              | [`Vector3Curve`] (marker)     |
//! | `MarkerCurve`               | [`MarkerCurve`] (marker)      |
//! | `Path2D`                    | [`Path2D`]                    |
//! | `LocalizationTable`         | [`LocalizationTable`]         |
//! | `Configuration`             | [`Configuration`] (marker)    |
//! | `Noise`                     | [`Noise`]                     |
//! | `UnreliableRemoteEvent`     | [`UnreliableRemoteEvent`] (marker) |
//! | `Wire`                      | [`Wire`]                      |
//! | `OperationGraph`            | [`OperationGraph`] (marker)   |
//!
//! ## Pattern — pure data-attach
//!
//! These are non-visual data containers; the spawner attaches the Eustress
//! component reading defaults (many are field-less markers). The runtime
//! semantics (DataStore request options, curve sampling, noise generation,
//! Wire pin routing) are a later phase. Mirrors the Wave 6.A ValueObject
//! shape (data-only attach + empty LOD + stub Fjall persistence + TOML
//! round-trip).
//!
//! [`DataStoreGetOptions`]: eustress_common::classes::DataStoreGetOptions
//! [`DataStoreSetOptions`]: eustress_common::classes::DataStoreSetOptions
//! [`DataStoreIncrementOptions`]: eustress_common::classes::DataStoreIncrementOptions
//! [`DataStoreOptions`]: eustress_common::classes::DataStoreOptions
//! [`FloatCurve`]: eustress_common::classes::FloatCurve
//! [`RotationCurve`]: eustress_common::classes::RotationCurve
//! [`EulerRotationCurve`]: eustress_common::classes::EulerRotationCurve
//! [`Vector3Curve`]: eustress_common::classes::Vector3Curve
//! [`MarkerCurve`]: eustress_common::classes::MarkerCurve
//! [`Path2D`]: eustress_common::classes::Path2D
//! [`LocalizationTable`]: eustress_common::classes::LocalizationTable
//! [`Configuration`]: eustress_common::classes::Configuration
//! [`Noise`]: eustress_common::classes::Noise
//! [`UnreliableRemoteEvent`]: eustress_common::classes::UnreliableRemoteEvent
//! [`Wire`]: eustress_common::classes::Wire
//! [`OperationGraph`]: eustress_common::classes::OperationGraph
//! [`Instance`]: eustress_common::classes::Instance

use bevy::prelude::*;

use eustress_common::class_registry::{PropertyBag, RegisterClassExt};
use eustress_common::classes::{ClassName, Instance, PropertyValue};

pub mod configuration;
pub mod data_store_get_options;
pub mod data_store_increment_options;
pub mod data_store_options;
pub mod data_store_set_options;
pub mod euler_rotation_curve;
pub mod float_curve;
pub mod localization_table;
pub mod marker_curve;
pub mod noise;
pub mod operation_graph;
pub mod path2d;
pub mod rotation_curve;
pub mod unreliable_remote_event;
pub mod vector3_curve;
pub mod wire;

pub use configuration::ConfigurationSpawner;
pub use data_store_get_options::DataStoreGetOptionsSpawner;
pub use data_store_increment_options::DataStoreIncrementOptionsSpawner;
pub use data_store_options::DataStoreOptionsSpawner;
pub use data_store_set_options::DataStoreSetOptionsSpawner;
pub use euler_rotation_curve::EulerRotationCurveSpawner;
pub use float_curve::FloatCurveSpawner;
pub use localization_table::LocalizationTableSpawner;
pub use marker_curve::MarkerCurveSpawner;
pub use noise::NoiseSpawner;
pub use operation_graph::OperationGraphSpawner;
pub use path2d::Path2DSpawner;
pub use rotation_curve::RotationCurveSpawner;
pub use unreliable_remote_event::UnreliableRemoteEventSpawner;
pub use vector3_curve::Vector3CurveSpawner;
pub use wire::WireSpawner;

/// Bevy plugin registering every data / curve / misc spawner shipped by
/// Wave 7.F with the
/// [`ClassRegistry`][eustress_common::class_registry::ClassRegistry].
pub struct Data7SpawnerPlugin;

impl Plugin for Data7SpawnerPlugin {
    fn build(&self, app: &mut App) {
        app.register_class::<DataStoreGetOptionsSpawner>()
            .register_class::<DataStoreSetOptionsSpawner>()
            .register_class::<DataStoreIncrementOptionsSpawner>()
            .register_class::<DataStoreOptionsSpawner>()
            .register_class::<FloatCurveSpawner>()
            .register_class::<RotationCurveSpawner>()
            .register_class::<EulerRotationCurveSpawner>()
            .register_class::<Vector3CurveSpawner>()
            .register_class::<MarkerCurveSpawner>()
            .register_class::<Path2DSpawner>()
            .register_class::<LocalizationTableSpawner>()
            .register_class::<ConfigurationSpawner>()
            .register_class::<NoiseSpawner>()
            .register_class::<UnreliableRemoteEventSpawner>()
            .register_class::<WireSpawner>()
            .register_class::<OperationGraphSpawner>();
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Build the cross-cutting [`Instance`] every data entity carries.
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

/// Read an optional Eustress instance-id reference from the bag.
pub(crate) fn read_optional_ref(bag: &PropertyBag, key: &str) -> Option<u32> {
    bag.get_i32(key)
        .and_then(|i| if i < 0 { None } else { Some(i as u32) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};

    const DATA_CLASSES: [ClassName; 16] = [
        ClassName::DataStoreGetOptions,
        ClassName::DataStoreSetOptions,
        ClassName::DataStoreIncrementOptions,
        ClassName::DataStoreOptions,
        ClassName::FloatCurve,
        ClassName::RotationCurve,
        ClassName::EulerRotationCurve,
        ClassName::Vector3Curve,
        ClassName::MarkerCurve,
        ClassName::Path2D,
        ClassName::LocalizationTable,
        ClassName::Configuration,
        ClassName::Noise,
        ClassName::UnreliableRemoteEvent,
        ClassName::Wire,
        ClassName::OperationGraph,
    ];

    #[test]
    fn plugin_registers_all_sixteen_data_classes() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(Data7SpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        for class in DATA_CLASSES {
            assert!(registry.contains(class), "must register {}", class.as_str());
            let spawner: &dyn ClassSpawner = registry.get(class).unwrap();
            assert_eq!(spawner.class_name(), class);
        }
        assert_eq!(registry.len(), 16);
    }

    #[test]
    fn class_name_round_trips_for_every_data_class() {
        for class in DATA_CLASSES {
            assert_eq!(ClassName::from_str(class.as_str()), Ok(class));
        }
    }
}
