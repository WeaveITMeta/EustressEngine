//! `InstanceDefinition` ↔ `ArchInstanceCore` mapping (K2, the
//! rkyv-everywhere binary model).
//!
//! The engine keeps the serde [`InstanceDefinition`] as its **parse
//! model** (it owns the `#[serde(flatten)]` + `toml::Value` machinery
//! that makes rich `.instance.toml` files load). The worlddb crate owns
//! the **archive model** [`eustress_worlddb::ArchInstanceCore`] — a
//! per-type rkyv mirror whose load hot path is a zero-copy cast, not a
//! TOML parse. This module is the bake/load bridge between the two.
//!
//! ## Split: typed hot core + extensible cold tail
//!
//! The rendered/physics-hot fields (identity, asset, transform, color,
//! the physics + shadow flags, material name, tags) map to
//! `ArchInstanceCore`'s **typed** fields — the only bytes the 50k-part
//! load path touches, read zero-copy.
//!
//! Everything else (full [`InstanceMetadata`] — attribution chain,
//! timestamps, authoring unit; the optional realism `material` /
//! `thermodynamic` / `electrochemical` sections; `ui`; `attributes`;
//! `parameters`; and the flattened `extra` sections) folds into the
//! `extra: Vec<(String, EusValue)>` **cold tail** under reserved
//! double-underscore keys, via worlddb's `toml::Value ↔ EusValue`
//! bridge. This keeps the round-trip **lossless** (the modification
//! chain is training data — it must survive a TOML→Fjall→TOML trip)
//! while keeping the hot prefix small.
//!
//! ## Status — ADDITIVE, NOT YET WIRED
//!
//! Nothing calls these functions on the live load/save path yet, so this
//! module cannot break the running engine. The next K2 increments wire
//! it in: (1) `active_db` swaps its bincode `#bin` codec for
//! `encode_instance_core`; (2) the loader reads `access_instance_core`
//! zero-copy. Both land with the build loop.

use std::collections::HashMap;

use eustress_worlddb::{ArchInstanceCore, EusValue};

use super::instance_loader::{
    AssetReference, InstanceDefinition, InstanceMetadata, InstanceProperties, TomlElectrochemicalState,
    TomlMaterialProperties, TomlThermodynamicState, TransformData, UiInstanceProperties,
};

// Reserved tail keys for the structured cold sections. Double-underscore
// so they never collide with a real flattened `[Section]` name. The tail
// is sorted by key (see `instance_to_arch`) for deterministic archive
// bytes.
const META_KEY: &str = "__meta";
const MATERIAL_KEY: &str = "__material";
const THERMO_KEY: &str = "__thermodynamic";
const ELECTRO_KEY: &str = "__electrochemical";
const UI_KEY: &str = "__ui";
const ATTRS_KEY: &str = "__attributes";
const PARAMS_KEY: &str = "__parameters";
const EXTRA_KEY: &str = "__extra";

/// Default glTF scene name (mirrors `instance_loader::default_scene`,
/// inlined here to avoid widening that module's visibility).
const DEFAULT_SCENE: &str = "Scene0";

// ── small conversion helpers ────────────────────────────────────────

/// Serialize any serde struct into an [`EusValue`] (struct → TOML value
/// → EusValue). Returns `None` if the value can't be represented as TOML
/// (e.g. a non-finite float — the same constraint the disk writer has);
/// the caller then simply omits that cold section.
fn struct_to_eus<T: serde::Serialize>(v: &T) -> Option<EusValue> {
    toml::Value::try_from(v).ok().map(EusValue::from)
}

/// Deserialize a struct back out of an [`EusValue`] (EusValue → TOML
/// value → struct). `None` if the stored shape doesn't fit `T`.
fn eus_to_struct<T: serde::de::DeserializeOwned>(v: &EusValue) -> Option<T> {
    let tv: toml::Value = v.clone().into();
    tv.try_into().ok()
}

/// `HashMap<String, toml::Value>` → an `EusValue::Table` (keys sorted by
/// the `From<toml::Value>` impl).
fn map_to_eus(map: &HashMap<String, toml::Value>) -> EusValue {
    let mut t = toml::value::Table::new();
    for (k, v) in map {
        t.insert(k.clone(), v.clone());
    }
    EusValue::from(toml::Value::Table(t))
}

/// Inverse of [`map_to_eus`]. A non-table EusValue yields an empty map.
fn eus_to_map(v: &EusValue) -> HashMap<String, toml::Value> {
    match toml::Value::from(v.clone()) {
        toml::Value::Table(t) => t.into_iter().collect(),
        _ => HashMap::new(),
    }
}

/// Look up a reserved cold-tail section by key.
fn tail_get<'a>(tail: &'a [(String, EusValue)], key: &str) -> Option<&'a EusValue> {
    tail.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

// ── the mapping ─────────────────────────────────────────────────────

/// Bake an [`InstanceDefinition`] (parse model) into an
/// [`ArchInstanceCore`] (archive model). Lossless: every field either
/// lands in a typed hot field or in the reserved cold tail.
pub fn instance_to_arch(def: &InstanceDefinition) -> ArchInstanceCore {
    let (mesh, scene) = match &def.asset {
        Some(a) => (a.mesh.clone(), a.scene.clone()),
        None => (String::new(), String::new()),
    };

    let mut extra: Vec<(String, EusValue)> = Vec::new();

    // Full metadata — class_name is also mirrored into the typed field
    // for the hot path, but the whole struct is kept here so the reverse
    // map reconstructs the attribution chain / timestamps / unit exactly.
    if let Some(v) = struct_to_eus(&def.metadata) {
        extra.push((META_KEY.to_string(), v));
    }
    if let Some(m) = &def.material {
        if let Some(v) = struct_to_eus(m) {
            extra.push((MATERIAL_KEY.to_string(), v));
        }
    }
    if let Some(t) = &def.thermodynamic {
        if let Some(v) = struct_to_eus(t) {
            extra.push((THERMO_KEY.to_string(), v));
        }
    }
    if let Some(e) = &def.electrochemical {
        if let Some(v) = struct_to_eus(e) {
            extra.push((ELECTRO_KEY.to_string(), v));
        }
    }
    if let Some(u) = &def.ui {
        if let Some(v) = struct_to_eus(u) {
            extra.push((UI_KEY.to_string(), v));
        }
    }
    if let Some(attrs) = &def.attributes {
        if !attrs.is_empty() {
            extra.push((ATTRS_KEY.to_string(), map_to_eus(attrs)));
        }
    }
    if let Some(params) = &def.parameters {
        if !params.is_empty() {
            extra.push((PARAMS_KEY.to_string(), map_to_eus(params)));
        }
    }
    if !def.extra.is_empty() {
        extra.push((EXTRA_KEY.to_string(), map_to_eus(&def.extra)));
    }
    // Deterministic archive bytes.
    extra.sort_by(|a, b| a.0.cmp(&b.0));

    ArchInstanceCore {
        class_name: def.metadata.class_name.clone(),
        mesh,
        scene,
        t: def.transform.position,
        r: def.transform.rotation,
        s: def.transform.scale,
        color: def.properties.color,
        transparency: def.properties.transparency,
        reflectance: def.properties.reflectance,
        anchored: def.properties.anchored,
        can_collide: def.properties.can_collide,
        cast_shadow: def.properties.cast_shadow,
        locked: def.properties.locked,
        material: def.properties.material.clone(),
        tags: def.tags.clone().unwrap_or_default(),
        extra,
    }
}

/// Inverse of [`instance_to_arch`] — reconstruct the serde
/// [`InstanceDefinition`] from an owned [`ArchInstanceCore`] (cold path:
/// save-back / TOML export). The hot load path does NOT call this; it
/// reads the archived view's typed fields directly.
pub fn arch_to_instance(core: &ArchInstanceCore) -> InstanceDefinition {
    let asset = if core.mesh.is_empty() {
        None
    } else {
        Some(AssetReference {
            mesh: core.mesh.clone(),
            scene: if core.scene.is_empty() {
                DEFAULT_SCENE.to_string()
            } else {
                core.scene.clone()
            },
        })
    };

    let metadata = tail_get(&core.extra, META_KEY)
        .and_then(eus_to_struct::<InstanceMetadata>)
        .unwrap_or_else(|| InstanceMetadata {
            class_name: core.class_name.clone(),
            ..Default::default()
        });

    let material = tail_get(&core.extra, MATERIAL_KEY).and_then(eus_to_struct::<TomlMaterialProperties>);
    let thermodynamic =
        tail_get(&core.extra, THERMO_KEY).and_then(eus_to_struct::<TomlThermodynamicState>);
    let electrochemical =
        tail_get(&core.extra, ELECTRO_KEY).and_then(eus_to_struct::<TomlElectrochemicalState>);
    let ui = tail_get(&core.extra, UI_KEY).and_then(eus_to_struct::<UiInstanceProperties>);

    let attributes = tail_get(&core.extra, ATTRS_KEY).map(eus_to_map);
    let parameters = tail_get(&core.extra, PARAMS_KEY).map(eus_to_map);
    let extra = tail_get(&core.extra, EXTRA_KEY).map(eus_to_map).unwrap_or_default();

    let tags = if core.tags.is_empty() {
        None
    } else {
        Some(core.tags.clone())
    };

    InstanceDefinition {
        asset,
        transform: TransformData {
            position: core.t,
            rotation: core.r,
            scale: core.s,
        },
        properties: InstanceProperties {
            color: core.color,
            transparency: core.transparency,
            anchored: core.anchored,
            can_collide: core.can_collide,
            cast_shadow: core.cast_shadow,
            reflectance: core.reflectance,
            material: core.material.clone(),
            locked: core.locked,
        },
        metadata,
        material,
        thermodynamic,
        electrochemical,
        ui,
        attributes,
        tags,
        parameters,
        extra,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn instance_arch_roundtrip_lossless() {
        // A representative instance: asset + transform + rendered/physics
        // props + full metadata (incl. unit + display name) + tags +
        // attributes + parameters + a flattened [Appearance] section.
        let mut attributes = HashMap::new();
        attributes.insert("hp".to_string(), toml::Value::Integer(100));

        let mut parameters = HashMap::new();
        parameters.insert("speed".to_string(), toml::Value::Float(2.5));

        let mut appearance = toml::value::Table::new();
        appearance.insert("emissive".to_string(), toml::Value::Float(0.0));
        let mut extra = HashMap::new();
        extra.insert("Appearance".to_string(), toml::Value::Table(appearance));

        let original = InstanceDefinition {
            asset: Some(AssetReference {
                mesh: "parts/block.glb".into(),
                scene: "Scene0".into(),
            }),
            transform: TransformData {
                position: [1.0, 2.0, 3.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.5, 1.5, 1.5],
            },
            properties: InstanceProperties {
                color: [0.2, 0.4, 0.6, 1.0],
                transparency: 0.0,
                anchored: true,
                can_collide: false,
                cast_shadow: true,
                reflectance: 0.1,
                material: "Plastic".into(),
                locked: false,
            },
            metadata: InstanceMetadata {
                class_name: "Part".into(),
                archivable: true,
                name: Some("Block".into()),
                created: "2026-05-21T00:00:00Z".into(),
                last_modified: "2026-05-21T00:00:00Z".into(),
                created_by: None,
                modifications: Vec::new(),
                unit: Some("m".into()),
            },
            material: None,
            thermodynamic: None,
            electrochemical: None,
            ui: None,
            attributes: Some(attributes),
            tags: Some(vec!["bench".into(), "static".into()]),
            parameters: Some(parameters),
            extra,
        };

        // Bake to the archive model and check the typed hot fields.
        let arch = instance_to_arch(&original);
        assert_eq!(arch.class_name, "Part");
        assert_eq!(arch.mesh, "parts/block.glb");
        assert_eq!(arch.t, [1.0, 2.0, 3.0]);
        assert_eq!(arch.color, [0.2, 0.4, 0.6, 1.0]);
        assert!(!arch.can_collide);
        assert!(arch.cast_shadow);
        assert_eq!(arch.tags, vec!["bench".to_string(), "static".to_string()]);

        // rkyv encode → decode (proves the worlddb archive layer is
        // reachable + roundtrips from the engine side).
        let bytes = eustress_worlddb::encode_instance_core(&arch).unwrap();
        let arch2 = eustress_worlddb::decode_instance_core(&bytes).unwrap();
        assert_eq!(arch, arch2);

        // Map back and verify the cold tail reconstructed losslessly.
        // (Field-by-field rather than whole-struct: `InstanceDefinition`
        // has no `PartialEq`, and this avoids the toml flatten serializer.)
        let back = arch_to_instance(&arch2);
        assert_eq!(back.metadata.class_name, original.metadata.class_name);
        assert_eq!(back.metadata.name, original.metadata.name);
        assert_eq!(back.metadata.unit, original.metadata.unit);
        assert_eq!(back.metadata.created, original.metadata.created);
        assert_eq!(back.transform.position, original.transform.position);
        assert_eq!(back.transform.scale, original.transform.scale);
        assert_eq!(back.properties.color, original.properties.color);
        assert_eq!(back.properties.cast_shadow, original.properties.cast_shadow);
        assert_eq!(back.properties.material, original.properties.material);
        assert_eq!(back.tags, original.tags);
        assert_eq!(back.attributes, original.attributes);
        assert_eq!(back.parameters, original.parameters);
        assert_eq!(back.extra, original.extra);
        assert!(back.asset.is_some());
    }

    #[test]
    fn non_visual_instance_has_no_asset() {
        // A meshless instance (e.g. lighting / atmosphere) round-trips to
        // `asset: None` rather than an empty-mesh AssetReference.
        let def = InstanceDefinition {
            asset: None,
            transform: TransformData::default(),
            properties: InstanceProperties::default(),
            metadata: InstanceMetadata::default(),
            material: None,
            thermodynamic: None,
            electrochemical: None,
            ui: None,
            attributes: None,
            tags: None,
            parameters: None,
            extra: HashMap::new(),
        };
        let arch = instance_to_arch(&def);
        assert!(arch.mesh.is_empty());
        let back = arch_to_instance(&arch);
        assert!(back.asset.is_none());
        assert!(back.tags.is_none());
    }
}
