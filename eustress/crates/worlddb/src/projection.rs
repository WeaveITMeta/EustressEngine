//! `projection` — the serde **WorldState DTO**, projected from the canonical
//! [`ArchInstanceCore`].
//!
//! Three surfaces read entity state today and historically drifted: the engine
//! bridge `ecs.inspect`, the MCP `inspect_scene` proxy (which just reformats the
//! bridge's output), and the Slint Properties inspector (which still re-parses
//! disk TOML — the C1 authority split). This module is the ONE serde projection
//! they should all derive from, sourced from the canonical core
//! ([`ArchInstanceCore`], the rkyv persisted model). It is **additive**: nothing
//! consumes it yet — the read-surface migration (bridge first, Properties later)
//! is staged behind it.
//!
//! serde-only (no Bevy). The core's `extra` tail is [`EusValue`] (rkyv, NOT
//! serde), so it is projected to serde-native [`toml::Value`] via the existing
//! `From<EusValue>` conversion — lossless for the world model's value tree.

use serde::{Deserialize, Serialize};

use crate::rkyv_values::ArchInstanceCore;

/// Position / rotation / scale, serde-native (mirrors the core's `t`/`r`/`s`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct TransformSnapshot {
    /// translation x,y,z
    pub pos: [f32; 3],
    /// rotation quaternion x,y,z,w
    pub rot: [f32; 4],
    /// scale x,y,z
    pub scale: [f32; 3],
}

/// One entity, projected from [`ArchInstanceCore`] plus caller-supplied identity.
///
/// **Core-derived** fields (class/mesh/scene/material/color/transform/flags/
/// tags/extra) come from [`Self::from_core`]. The **resident-only** fields
/// (`runtime_id`, `visible`, `parent_uuid`, `source`, `resident`) are NOT in the
/// core — the caller fills them (a resident/live entity supplies its live
/// values; a DB-only entity leaves them defaulted). Override via struct-update:
///
/// ```ignore
/// EntitySnapshot {
///     runtime_id: Some(id),
///     resident: true,
///     ..EntitySnapshot::from_core(core, uuid, name)
/// }
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EntitySnapshot {
    /// Durable identity (the instance UUID — NOT the volatile `index vN`).
    pub uuid: String,
    /// Display name (the Bevy `Name` / folder name — not stored in the core).
    pub name: String,
    pub class: String,
    pub mesh: Option<String>,
    pub scene: Option<String>,
    pub material: Option<String>,
    /// Linear-rgba (matches the core).
    pub color: [f32; 4],
    pub transparency: f32,
    pub reflectance: f32,
    pub transform: TransformSnapshot,
    pub anchored: bool,
    pub can_collide: bool,
    pub cast_shadow: bool,
    pub locked: bool,
    pub tags: Vec<String>,
    /// Cold tail (Material / Thermo / UI / attributes / parameters), projected
    /// from the core's rkyv `EusValue` tail into serde-native `toml::Value`.
    pub extra: Vec<(String, toml::Value)>,

    // ── resident-only (not present in the persisted core) ──
    /// Live Bevy entity id `"{index}v{generation}"`, for selection. Resident only.
    pub runtime_id: Option<String>,
    /// Render-side visibility (resident only — not a persisted core field).
    pub visible: Option<bool>,
    pub parent_uuid: Option<String>,
    /// Space-relative TOML source path, or `None` for binary-ECS-only entities.
    pub source: Option<String>,
    /// `true` if this came from a live ECS entity, `false` if DB-only.
    pub resident: bool,
}

impl EntitySnapshot {
    /// Project a canonical [`ArchInstanceCore`] into the serde DTO.
    ///
    /// `uuid` is the durable identity and `name` the display name — neither
    /// lives in the core, so they are supplied here. Resident-only fields
    /// default (`None`/`false`); set them via struct-update (see the type docs).
    pub fn from_core(
        core: &ArchInstanceCore,
        uuid: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        // Empty strings in the core mean "unset" for these optional fields.
        let str_opt = |s: &str| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        };
        EntitySnapshot {
            uuid: uuid.into(),
            name: name.into(),
            class: core.class_name.clone(),
            mesh: str_opt(&core.mesh),
            scene: str_opt(&core.scene),
            material: str_opt(&core.material),
            color: core.color,
            transparency: core.transparency,
            reflectance: core.reflectance,
            transform: TransformSnapshot {
                pos: core.t,
                rot: core.r,
                scale: core.s,
            },
            anchored: core.anchored,
            can_collide: core.can_collide,
            cast_shadow: core.cast_shadow,
            locked: core.locked,
            tags: core.tags.clone(),
            // EusValue is rkyv-only → project to serde-native toml::Value.
            extra: core
                .extra
                .iter()
                .map(|(k, v)| (k.clone(), toml::Value::from(v.clone())))
                .collect(),
            runtime_id: None,
            visible: None,
            parent_uuid: None,
            source: None,
            resident: false,
        }
    }
}

/// A scene's worth of [`EntitySnapshot`]s — the broad read projection that
/// `ecs.inspect` / `inspect_scene` will eventually return.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct WorldSnapshot {
    pub entities: Vec<EntitySnapshot>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rkyv_values::EusValue;

    fn sample_core() -> ArchInstanceCore {
        ArchInstanceCore {
            class_name: "Part".into(),
            mesh: "parts/block.glb".into(),
            scene: String::new(),
            t: [1.0, 2.0, 3.0],
            r: [0.0, 0.0, 0.0, 1.0],
            s: [4.0, 1.0, 4.0],
            color: [0.3, 0.4, 0.5, 1.0],
            transparency: 0.0,
            reflectance: 0.1,
            anchored: true,
            can_collide: true,
            cast_shadow: true,
            locked: false,
            material: "Plastic".into(),
            tags: vec!["a".into(), "b".into()],
            extra: vec![
                ("mass".into(), EusValue::Float(12.5)),
                ("layer".into(), EusValue::Int(3)),
            ],
        }
    }

    #[test]
    fn from_core_maps_fields() {
        let core = sample_core();
        let snap = EntitySnapshot::from_core(&core, "uuid-1", "MyPart");
        assert_eq!(snap.uuid, "uuid-1");
        assert_eq!(snap.name, "MyPart");
        assert_eq!(snap.class, "Part");
        assert_eq!(snap.mesh.as_deref(), Some("parts/block.glb"));
        assert_eq!(snap.scene, None); // empty core field → None
        assert_eq!(snap.material.as_deref(), Some("Plastic"));
        assert_eq!(snap.transform.pos, [1.0, 2.0, 3.0]);
        assert_eq!(snap.transform.scale, [4.0, 1.0, 4.0]);
        assert!(snap.anchored && snap.can_collide && !snap.locked);
        assert_eq!(snap.tags, vec!["a".to_string(), "b".to_string()]);
        // resident-only fields default
        assert!(!snap.resident && snap.runtime_id.is_none() && snap.source.is_none());
        // extra tail projected EusValue → toml::Value
        assert_eq!(snap.extra.len(), 2);
        assert_eq!(snap.extra[0], ("mass".to_string(), toml::Value::Float(12.5)));
        assert_eq!(snap.extra[1], ("layer".to_string(), toml::Value::Integer(3)));
    }

    #[test]
    fn serde_json_round_trips() {
        let core = sample_core();
        let snap = EntitySnapshot {
            runtime_id: Some("1944v0".into()),
            resident: true,
            source: Some("Workspace/Part/_instance.toml".into()),
            ..EntitySnapshot::from_core(&core, "uuid-1", "MyPart")
        };
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: EntitySnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.uuid, snap.uuid);
        assert_eq!(back.name, snap.name);
        assert_eq!(back.runtime_id.as_deref(), Some("1944v0"));
        assert!(back.resident);
        assert_eq!(back.source.as_deref(), Some("Workspace/Part/_instance.toml"));
        assert_eq!(back.transform, snap.transform);
        assert_eq!(back.extra, snap.extra);
    }
}
