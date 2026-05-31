//! # ValueObject spawners — Wave 6.A
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md` §1:
//! the 11 Roblox ValueObject classes. Each is a **non-visual value container**
//! that holds exactly ONE typed value — the simplest possible classes. Luau
//! scripts use them to stash data on the instance tree (e.g. a `StringValue`
//! named "Difficulty" carrying "Hard").
//!
//! | `ClassName`         | Payload                                  |
//! |---------------------|------------------------------------------|
//! | `StringValue`       | `String`                                 |
//! | `IntValue`          | `i64`                                    |
//! | `NumberValue`       | `f64`                                    |
//! | `BoolValue`         | `bool`                                   |
//! | `ObjectValue`       | `Option<String>` (target uuid/path)      |
//! | `Color3Value`       | `[f32; 3]`                               |
//! | `Vector3Value`      | `[f32; 3]`                               |
//! | `CFrameValue`       | `Transform` (position + rotation)        |
//! | `BrickColorValue`   | `i32` (BrickColor palette index)         |
//! | `RayValue`          | `[f32; 6]` (origin xyz + direction xyz)  |
//! | `BinaryStringValue` | `String` (base64/opaque)                 |
//!
//! ## The "full vertical" pattern
//!
//! These are the first classes to prove the complete pattern end-to-end:
//! `ClassName` enum variant + component struct (in
//! [`eustress_common::classes`]) + `compat::ClassMapping` entry + `ClassSpawner`
//! impl + registration. Once the enum variants + compat entries exist, the
//! Roblox importer (`eustress-roblox-import`) picks these up automatically —
//! its `class_map` delegates to `compat::ClassMapping` + `ClassName::from_str`.
//!
//! The real value comes from each spawner's `import_from_roblox`: Roblox
//! ValueObjects store their payload in a property named `Value`, which the
//! spawner reads into the component (e.g. `StringValue.Value` → `component.value`).
//!
//! ## Why a sub-plugin / no LOD / stub persistence
//!
//! Same shape as the Wave 5 networking spawners
//! ([`crate::spawners::networking`]): a self-contained Bevy `Plugin` that
//! registers its classes with the
//! [`ClassRegistry`][eustress_common::class_registry::ClassRegistry] resource.
//! ValueObjects are non-visual ⇒ empty LOD bundle at every tier; persistence is
//! stubbed (empty bytes / empty bag) until a later wave lights up the Fjall
//! write path — the value survives via TOML round-trip in the meantime.
//!
//! Per `docs/process/AGENT_DISPATCH.md` LOOP 5: this sub-plugin introduces NO
//! Bevy resources and never touches `drain_slint_actions` or the legacy
//! `StudioUiPlugin`. It only `register_class`-es into the shared registry.

pub mod binary_string_value;
pub mod bool_value;
pub mod brick_color_value;
pub mod cframe_value;
pub mod color3_value;
pub mod int_value;
pub mod number_value;
pub mod object_value;
pub mod ray_value;
pub mod string_value;
pub mod vector3_value;

pub use binary_string_value::BinaryStringValueSpawner;
pub use bool_value::BoolValueSpawner;
pub use brick_color_value::BrickColorValueSpawner;
pub use cframe_value::CFrameValueSpawner;
pub use color3_value::Color3ValueSpawner;
pub use int_value::IntValueSpawner;
pub use number_value::NumberValueSpawner;
pub use object_value::ObjectValueSpawner;
pub use ray_value::RayValueSpawner;
pub use string_value::StringValueSpawner;
pub use vector3_value::Vector3ValueSpawner;

use bevy::prelude::*;

use eustress_common::class_registry::RegisterClassExt;

/// Bevy plugin that registers all 11 ValueObject [`ClassSpawner`]s with the
/// shared [`ClassRegistry`][eustress_common::class_registry::ClassRegistry]
/// resource.
///
/// Wired into `SlintUiPlugin::build`'s `add_plugins` tuple alongside the other
/// spawner group plugins (containers, lights, gui, networking, …). The
/// `ClassRegistryPlugin` must run first so the registry resource exists before
/// any `register_class` call — the standard wiring contract for every spawner
/// sub-plugin (see [`crate::spawners`] module docs).
///
/// Registration order is irrelevant (the registry is keyed by `ClassName`);
/// double-registration of a class panics at registration time.
pub struct ValueObjectsSpawnerPlugin;

impl Plugin for ValueObjectsSpawnerPlugin {
    fn build(&self, app: &mut App) {
        app.register_class::<StringValueSpawner>()
            .register_class::<IntValueSpawner>()
            .register_class::<NumberValueSpawner>()
            .register_class::<BoolValueSpawner>()
            .register_class::<ObjectValueSpawner>()
            .register_class::<Color3ValueSpawner>()
            .register_class::<Vector3ValueSpawner>()
            .register_class::<CFrameValueSpawner>()
            .register_class::<BrickColorValueSpawner>()
            .register_class::<RayValueSpawner>()
            .register_class::<BinaryStringValueSpawner>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner, PropertyBag, SpawnCtx};
    use eustress_common::classes::{ClassName, PropertyValue};

    /// Every ValueObject class this plugin owns — the registration roster.
    const VALUE_OBJECT_CLASSES: [ClassName; 11] = [
        ClassName::StringValue,
        ClassName::IntValue,
        ClassName::NumberValue,
        ClassName::BoolValue,
        ClassName::ObjectValue,
        ClassName::Color3Value,
        ClassName::Vector3Value,
        ClassName::CFrameValue,
        ClassName::BrickColorValue,
        ClassName::RayValue,
        ClassName::BinaryStringValue,
    ];

    /// Adding `ValueObjectsSpawnerPlugin` registers all 11 ValueObject
    /// spawners under their canonical `ClassName` keys, and each spawner's
    /// `class_name()` matches its registration key.
    #[test]
    fn plugin_registers_all_eleven_value_objects() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(ValueObjectsSpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        for class in VALUE_OBJECT_CLASSES {
            assert!(
                registry.contains(class),
                "ValueObjectsSpawnerPlugin must register a spawner for {}",
                class.as_str()
            );
            let spawner: &dyn ClassSpawner = registry.get(class).unwrap();
            assert_eq!(spawner.class_name(), class);
        }

        assert_eq!(
            registry.len(),
            11,
            "ValueObjectsSpawnerPlugin registers exactly 11 spawners — \
             any more means another group's plugin leaked in"
        );
    }

    /// **ClassName round-trip (deliverable #9).** Each ValueObject class
    /// round-trips through `from_str(as_str()) == Ok(self)` — the invariant
    /// that lets the Roblox importer + TOML loader resolve these classes.
    #[test]
    fn class_name_round_trips_for_every_value_object() {
        for class in VALUE_OBJECT_CLASSES {
            let s = class.as_str();
            assert_eq!(
                ClassName::from_str(s),
                Ok(class),
                "ClassName round-trip failed for {s}: from_str(as_str()) must equal the variant"
            );
        }
        // Spot-check the literal strings, too — guards against an as_str()
        // typo that would still round-trip against a matching from_str() typo.
        assert_eq!(ClassName::StringValue.as_str(), "StringValue");
        assert_eq!(ClassName::from_str("CFrameValue"), Ok(ClassName::CFrameValue));
    }

    /// **Spawn smoke test (deliverable #9).** Drives the REAL
    /// [`StringValueSpawner::spawn`] through a real [`SpawnCtx`] inside a Bevy
    /// exclusive system, then asserts the spawned entity carries both the
    /// value (`StringValue.value == "Hard"`) and the cross-cutting `Instance`
    /// every spawner must attach. This is the end-to-end proof that the
    /// registry-dispatched `PropertyBag` → `spawn` → component path works.
    ///
    /// The `SpawnCtx::commands` field is `&'w mut Commands<'w, 's>`, so the
    /// ctx must be built where a real `Commands` is alive at the world-borrow
    /// lifetime — an exclusive system supplies exactly that. We build the ctx,
    /// dispatch through the `ClassRegistry` (proving the registry wiring, not
    /// just a bare struct call), and Bevy applies the command queue on system
    /// return.
    #[test]
    fn string_value_spawns_with_value_and_instance() {
        use bevy::ecs::system::SystemState;
        use eustress_common::class_schema::ClassSchemaResource;
        use eustress_common::classes::{Instance, StringValue};
        use eustress_common::units::MeasureUnit;

        // AssetPlugin gives a real AssetServer; init_asset registers each
        // `Assets<T>` collection the SpawnCtx borrows. No render plugins are
        // needed — ValueObject spawners touch none of the asset stores, but
        // the SpawnCtx fields must still resolve to live resources.
        let mut app = App::new();
        app.add_plugins(bevy::asset::AssetPlugin::default());
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.init_resource::<ClassSchemaResource>();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(ValueObjectsSpawnerPlugin);

        // Props the importer/loader hands to `spawn`.
        let mut props = PropertyBag::new();
        props.set("metadata.name", PropertyValue::String("Difficulty".into()));
        props.set("value", PropertyValue::String("Hard".into()));

        let world = app.world_mut();
        let mut state: SystemState<(
            Commands,
            Res<AssetServer>,
            Res<ClassSchemaResource>,
            ResMut<Assets<Mesh>>,
            ResMut<Assets<StandardMaterial>>,
            ResMut<Assets<Image>>,
            Res<ClassRegistry>,
        )> = SystemState::new(world);

        let entity = {
            let (mut commands, asset_server, class_schema, mut meshes, mut mats, mut images, registry) =
                state.get_mut(world);

            let spawner = registry
                .get(ClassName::StringValue)
                .expect("registry must hold a StringValue spawner");

            let mut ctx = SpawnCtx {
                commands: &mut commands,
                asset_server: &asset_server,
                class_schema: &class_schema,
                source_path: None,
                meshes: &mut meshes,
                standard_materials: &mut mats,
                images: &mut images,
                parent_entity: None,
                measure_unit: MeasureUnit::default(),
                load_in_progress: false,
                extra: None,
            };
            spawner.spawn(&mut ctx, &props)
        };
        // Flush the queued spawn into the world.
        state.apply(world);

        let entity_ref = world.entity(entity);
        let comp = entity_ref
            .get::<StringValue>()
            .expect("StringValue component present after spawn");
        assert_eq!(comp.value, "Hard", "spawn must carry the imported value");

        let instance = entity_ref
            .get::<Instance>()
            .expect("every spawner must attach Instance");
        assert_eq!(instance.class_name, ClassName::StringValue);
        assert_eq!(instance.name, "Difficulty");
    }
}
