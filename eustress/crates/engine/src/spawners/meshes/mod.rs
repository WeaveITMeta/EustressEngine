//! # Mesh / surface / visual-adornment spawners — Wave 7.C
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8 + `docs/FEATURE_PARITY.md`:
//! the 10 Roblox mesh / surface / decoration classes that modify or decorate
//! the parent part's rendering.
//!
//! | `ClassName`         | Component attached       |
//! |---------------------|--------------------------|
//! | `BlockMesh`         | [`BlockMesh`]            |
//! | `FileMesh`          | [`FileMesh`]             |
//! | `Texture`           | [`Texture`]              |
//! | `SurfaceAppearance` | [`SurfaceAppearance`]    |
//! | `MaterialVariant`   | [`MaterialVariant`]      |
//! | `Highlight`         | [`Highlight`]            |
//! | `Bone`              | [`Bone`]                 |
//! | `WrapDeformer`      | [`WrapDeformer`]         |
//! | `WrapLayer`         | [`WrapLayer`]            |
//! | `WrapTarget`        | [`WrapTarget`]           |
//!
//! ## Pattern
//!
//! Mirrors the Wave 6.A ValueObject / 6.D interaction groups: each spawner is
//! a zero-sized [`ClassSpawner`] that hydrates its component from the
//! `PropertyBag` and attaches the cross-cutting [`Instance`] + [`Name`].
//! These are **data-attach** decorators — the spawner attaches + persists the
//! config; the actual render-application (texture-map override, highlight
//! pass, cage deformation, skinned-bone transform) is a later phase.
//!
//! ## Why no LOD / stub persistence
//!
//! These modify the parent part's render rather than being independently
//! LOD-managed renderables. `lod_components` returns an empty bundle at every
//! tier; `serialize`/`deserialize` are stubbed (the value survives via TOML
//! round-trip) until a later wave lights up the Fjall write path.
//!
//! [`BlockMesh`]: eustress_common::classes::BlockMesh
//! [`FileMesh`]: eustress_common::classes::FileMesh
//! [`Texture`]: eustress_common::classes::Texture
//! [`SurfaceAppearance`]: eustress_common::classes::SurfaceAppearance
//! [`MaterialVariant`]: eustress_common::classes::MaterialVariant
//! [`Highlight`]: eustress_common::classes::Highlight
//! [`Bone`]: eustress_common::classes::Bone
//! [`WrapDeformer`]: eustress_common::classes::WrapDeformer
//! [`WrapLayer`]: eustress_common::classes::WrapLayer
//! [`WrapTarget`]: eustress_common::classes::WrapTarget
//! [`Instance`]: eustress_common::classes::Instance

use bevy::prelude::*;

use eustress_common::class_registry::{PropertyBag, RegisterClassExt};
use eustress_common::classes::{ClassName, Instance, PropertyValue};

pub mod block_mesh;
pub mod bone;
pub mod file_mesh;
pub mod highlight;
pub mod material_variant;
pub mod surface_appearance;
pub mod texture;
pub mod wrap_deformer;
pub mod wrap_layer;
pub mod wrap_target;

pub use block_mesh::BlockMeshSpawner;
pub use bone::BoneSpawner;
pub use file_mesh::FileMeshSpawner;
pub use highlight::HighlightSpawner;
pub use material_variant::MaterialVariantSpawner;
pub use surface_appearance::SurfaceAppearanceSpawner;
pub use texture::TextureSpawner;
pub use wrap_deformer::WrapDeformerSpawner;
pub use wrap_layer::WrapLayerSpawner;
pub use wrap_target::WrapTargetSpawner;

/// Bevy plugin registering every mesh / surface / adornment spawner shipped
/// by Wave 7.C with the
/// [`ClassRegistry`][eustress_common::class_registry::ClassRegistry].
///
/// Wired into `SlintUiPlugin::build`'s `add_plugins` tuple. The
/// `ClassRegistryPlugin` must run first so the registry resource exists
/// before any `register_class` call.
pub struct MeshesSpawnerPlugin;

impl Plugin for MeshesSpawnerPlugin {
    fn build(&self, app: &mut App) {
        app.register_class::<BlockMeshSpawner>()
            .register_class::<FileMeshSpawner>()
            .register_class::<TextureSpawner>()
            .register_class::<SurfaceAppearanceSpawner>()
            .register_class::<MaterialVariantSpawner>()
            .register_class::<HighlightSpawner>()
            .register_class::<BoneSpawner>()
            .register_class::<WrapDeformerSpawner>()
            .register_class::<WrapLayerSpawner>()
            .register_class::<WrapTargetSpawner>();
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Build the cross-cutting [`Instance`] every mesh/decoration entity carries.
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

/// Read a `[f32; 3]` color triple from a TOML array, defaulting to white.
pub(crate) fn read_color3(arr: &[toml::Value]) -> [f32; 3] {
    let get = |i: usize| arr.get(i).and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
    [get(0), get(1), get(2)]
}

/// Serialize a `[f32; 3]` color triple to a TOML array.
pub(crate) fn color3_to_toml(c: [f32; 3]) -> toml::Value {
    toml::Value::Array(c.iter().map(|v| toml::Value::Float(*v as f64)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::{ClassRegistry, ClassSpawner};

    const MESH_CLASSES: [ClassName; 10] = [
        ClassName::BlockMesh,
        ClassName::FileMesh,
        ClassName::Texture,
        ClassName::SurfaceAppearance,
        ClassName::MaterialVariant,
        ClassName::Highlight,
        ClassName::Bone,
        ClassName::WrapDeformer,
        ClassName::WrapLayer,
        ClassName::WrapTarget,
    ];

    #[test]
    fn plugin_registers_all_ten_mesh_classes() {
        let mut app = App::new();
        app.init_resource::<ClassRegistry>();
        app.add_plugins(MeshesSpawnerPlugin);

        let registry = app.world().resource::<ClassRegistry>();
        for class in MESH_CLASSES {
            assert!(registry.contains(class), "must register {}", class.as_str());
            let spawner: &dyn ClassSpawner = registry.get(class).unwrap();
            assert_eq!(spawner.class_name(), class);
        }
        assert_eq!(registry.len(), 10);
    }

    #[test]
    fn class_name_round_trips_for_every_mesh_class() {
        for class in MESH_CLASSES {
            assert_eq!(ClassName::from_str(class.as_str()), Ok(class));
        }
    }
}
