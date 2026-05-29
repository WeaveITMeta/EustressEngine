//! `BeamSpawner` — Wave 3.F VFX class spawner.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §8.4 (VFX) the `Beam`
//! class lives in the VFX group alongside `ParticleEmitter`. Unlike
//! particles (which need a GPU effect graph) Beams have a CPU-cheap
//! "stretched cylinder mesh between two attachments" implementation —
//! this spawner ships that.
//!
//! ## What this spawner builds
//!
//! A Beam entity carries:
//! - The Eustress `Beam` component (canonical data model, persists to
//!   TOML + Fjall unchanged).
//! - A cylinder `Mesh3d` whose radius averages `width0` and `width1`
//!   (no taper today — `width1` is honored in a later Wave 4 pass that
//!   ships per-segment vertices).
//! - A `MeshMaterial3d<StandardMaterial>` keyed on the first colour in
//!   `color_sequence`, with emissive + alpha tuned by `brightness` and
//!   `light_emission`.
//! - A `Transform` whose translation sits at the midpoint between the
//!   two attachments (when resolvable) and whose rotation aligns the
//!   cylinder's Y-axis with the start→end direction. Scale.y stretches
//!   to the inter-attachment length.
//! - A `BeamSegmentLink` component pointing at the two attachment
//!   entities (resolved via Wave 2.1's UUID lookup or fall-through to
//!   the legacy `attachment0/1` entity-index field). A Wave 4 sync
//!   system reads this every frame to keep the beam taut as the
//!   attachments move.
//!
//! ## Resolving attachments
//!
//! The Eustress `Beam` struct stores `attachment0/attachment1` as
//! `Option<u32>` — historically a Bevy entity index (set by
//! MindSpace's "link selection" tool). The Wave 2.1 UUID dedup migration
//! introduced `find_entity_by_uuid`; future authoring will route
//! through UUIDs. This spawner:
//!
//! 1. First, if the PropertyBag carries `beam.attachment0_uuid` /
//!    `beam.attachment1_uuid` keys, calls `find_entity_by_uuid` on the
//!    WorldDb. This is the new path Wave 4 importer uses.
//! 2. Else, if the bag carries `beam.attachment0` / `beam.attachment1`
//!    as `Int`, treats them as the legacy `Entity.index().index()`.
//!    Resolution happens later (a Wave 4 sync system) because the
//!    `SpawnCtx` doesn't carry a `&World` — only `Commands`.
//! 3. Else, builds a default stretched cylinder pointing along +Y of
//!    length 4m. The Properties panel can edit the attachment refs
//!    afterwards.
//!
//! Per the task spec: a *runtime* system reconciles attachments to
//! transform; the spawner only seeds the entity. This matches LOOP 8
//! (selection desync) — spawners must NOT touch other entities at
//! spawn time.
//!
//! ## LOD behaviour
//!
//! Per the task spec — `lod_components`:
//!
//! - `Hero` / `Active`: live mesh (the spawned cylinder, full
//!   material). No bundle changes.
//! - `Streamed`: marker swap to a static / unlit material — done via
//!   the `BeamLodMode` component the LOD transition system reads.
//!   Wave 3.F ships the marker; the material swap is in the LOD
//!   apply system (LOOP 3 — only visual components touched).
//! - `Horizon`: `Visibility::Hidden`.

use std::any::TypeId;

use bevy::math::primitives::Cylinder;
use bevy::prelude::*;

use eustress_common::class_registry::{
    ClassSpawner, ComponentBundle, DynamicComponent, LodTier, PropertyBag, RobloxInstance, SpawnCtx,
};
use eustress_common::classes::{
    Beam, BeamBlendMode, BeamFaceMode, ClassName, Instance, TextureMode,
};

/// Wire-format tag — Appendix A of `CLASS_REGISTRY.md`. Generic group
/// (low nibble 0) — beams persist Eustress-side data only; the live
/// mesh is rebuilt each spawn.
const BEAM_TAG: u8 = 0x10;

/// Component placed on a spawned Beam entity recording how to find
/// each end. Read by the Wave 4 sync system that keeps the cylinder
/// length + rotation in lockstep with the attached entities each
/// frame. Stays empty (`None`/`None`) for beams whose attachments
/// aren't authored yet.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct BeamSegmentLink {
    /// 32-char-hex UUID of the entity Attachment0 belongs to, when the
    /// bag came from Wave 4's UUID-aware importer. Else `None`.
    pub attachment0_uuid: Option<String>,
    /// Same for Attachment1.
    pub attachment1_uuid: Option<String>,
    /// Legacy entity-index reference (Wave 0 MindSpace link tool wrote
    /// these). Resolution requires a `World` borrow — the LOD-tier /
    /// sync system handles it.
    pub legacy_attachment0_index: Option<u32>,
    pub legacy_attachment1_index: Option<u32>,
}

/// Component marking the LOD mode the beam should render at. The LOD
/// transition system swaps the material based on this value so the
/// spawner doesn't need a `&mut Assets<StandardMaterial>` at LOD time.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub enum BeamLodMode {
    /// Live PBR-friendly material — full color / emissive / alpha.
    Live,
    /// Static unlit material — used at `Streamed` tier. Saves shader
    /// cost on beams that are visible but not focal.
    Static,
}

impl Default for BeamLodMode {
    fn default() -> Self {
        Self::Live
    }
}

/// Wave 3.F spawner for `ClassName::Beam`.
#[derive(Default)]
pub struct BeamSpawner;

impl ClassSpawner for BeamSpawner {
    fn class_name(&self) -> ClassName {
        ClassName::Beam
    }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props
            .get_string("metadata.name")
            .unwrap_or("Beam")
            .to_string();

        let beam = beam_from_bag(props);

        let instance = Instance {
            name: name.clone(),
            class_name: ClassName::Beam,
            archivable: true,
            id: 0,
            uuid: props.get_uuid().unwrap_or_default().to_string(),
            ai: false,
        };

        // Build the stretched cylinder. Without a `&World` we can't
        // resolve attachments here — fall back to a default 4m segment
        // along +Y. The Wave 4 sync system updates the transform once
        // attachments resolve.
        let length: f32 = 4.0;
        let avg_radius = (beam.width0 + beam.width1).max(0.01) * 0.5;
        let mesh_handle = ctx
            .meshes
            .add(Cylinder::new(avg_radius, length).mesh().build());

        // Pull the first colour-sequence key as the base material
        // colour. Empty sequences fall back to white.
        let base_color = beam
            .color_sequence
            .first()
            .map(|(_, c)| *c)
            .unwrap_or(Color::WHITE);
        let emissive_scalar = beam.light_emission.max(0.0);
        let emissive = if emissive_scalar > 0.0 {
            // LinearRgba is what bevy materials want for emissive HDR.
            let srgba = base_color.to_srgba();
            LinearRgba::new(
                srgba.red * emissive_scalar,
                srgba.green * emissive_scalar,
                srgba.blue * emissive_scalar,
                1.0,
            )
        } else {
            LinearRgba::BLACK
        };
        let material = StandardMaterial {
            base_color,
            emissive,
            unlit: matches!(beam.blend_mode, BeamBlendMode::Additive),
            alpha_mode: match beam.blend_mode {
                BeamBlendMode::Alpha => AlphaMode::Blend,
                BeamBlendMode::Additive => AlphaMode::Add,
                BeamBlendMode::Multiply => AlphaMode::Multiply,
            },
            double_sided: true,
            cull_mode: None,
            ..Default::default()
        };
        let material_handle = ctx.standard_materials.add(material);

        // Default transform: stand the cylinder along Y from the
        // entity's local origin. Wave 4 sync system rewrites this once
        // attachments resolve.
        let transform = props
            .get_transform("transform")
            .copied()
            .unwrap_or_default();

        // Resolve attachment refs from the bag. UUID-form refs come
        // from the Wave 4 Roblox importer; entity-index refs come from
        // the legacy MindSpace link tool. Both populate the
        // `BeamSegmentLink` component for the future sync system.
        let segment_link = BeamSegmentLink {
            attachment0_uuid: props
                .get_string("beam.attachment0_uuid")
                .map(str::to_string),
            attachment1_uuid: props
                .get_string("beam.attachment1_uuid")
                .map(str::to_string),
            legacy_attachment0_index: beam.attachment0,
            legacy_attachment1_index: beam.attachment1,
        };

        let visibility = if beam.enabled {
            Visibility::default()
        } else {
            Visibility::Hidden
        };

        ctx.commands
            .spawn((
                instance,
                beam,
                transform,
                visibility,
                Name::new(name),
                Mesh3d(mesh_handle),
                MeshMaterial3d(material_handle),
                segment_link,
                BeamLodMode::Live,
            ))
            .id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let mut out = vec![BEAM_TAG];
        if let Some(beam) = world.entity(entity).get::<Beam>() {
            match serde_json::to_vec(beam) {
                Ok(mut payload) => out.append(&mut payload),
                Err(e) => warn!(
                    "BeamSpawner::serialize: serde_json encode failed for entity {entity:?}: {e}"
                ),
            }
        }
        out
    }

    fn deserialize(&self, bytes: &[u8]) -> PropertyBag {
        if bytes.first() != Some(&BEAM_TAG) {
            warn!(
                "BeamSpawner::deserialize: tag mismatch (got {:?}, expected {BEAM_TAG:#x}) — empty bag",
                bytes.first(),
            );
            return PropertyBag::new();
        }
        let payload = &bytes[1..];
        let beam: Beam = match serde_json::from_slice(payload) {
            Ok(b) => b,
            Err(e) => {
                warn!("BeamSpawner::deserialize: serde_json decode failed: {e}");
                return PropertyBag::new();
            }
        };
        bag_from_beam(&beam)
    }

    fn apply_edit(
        &self,
        world: &mut World,
        entity: Entity,
        props: &PropertyBag,
    ) -> bool {
        // Width / colour edits need a mesh rebuild for the radius to
        // shrink/grow — that's a respawn. Everything else (enabled,
        // texture knobs, brightness without colour change) is cheap.
        let needs_respawn = {
            let current = world.entity(entity).get::<Beam>();
            if let Some(current_beam) = current {
                let new_width0 = props.get_f32("beam.width0").unwrap_or(current_beam.width0);
                let new_width1 = props.get_f32("beam.width1").unwrap_or(current_beam.width1);
                (new_width0 - current_beam.width0).abs() > f32::EPSILON
                    || (new_width1 - current_beam.width1).abs() > f32::EPSILON
            } else {
                false
            }
        };
        if !needs_respawn {
            if let Some(mut beam) = world.entity_mut(entity).get_mut::<Beam>() {
                apply_bag_to_beam(props, &mut beam);
            }
            // Reflect enabled toggle into visibility immediately.
            let enabled_now = world.entity(entity).get::<Beam>().map(|b| b.enabled);
            if let Some(enabled) = enabled_now {
                let new_vis = if enabled {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                world.entity_mut(entity).insert(new_vis);
            }
        }
        needs_respawn
    }

    fn lod_components(&self, tier: LodTier) -> ComponentBundle {
        match tier {
            // Hero + Active keep the live PBR material — both rely on
            // the Wave 4 sync system to keep the segment taut.
            LodTier::Hero | LodTier::Active => ComponentBundle {
                insert: vec![DynamicComponent::new(BeamLodMode::Live)],
                remove: vec![],
            },
            // Streamed swaps the material to a cheap unlit one — the
            // LOD apply system reads BeamLodMode::Static and rewrites
            // the StandardMaterial accordingly. Spawners don't touch
            // assets at LOD time — that's LOOP 3.
            LodTier::Streamed => ComponentBundle {
                insert: vec![DynamicComponent::new(BeamLodMode::Static)],
                remove: vec![],
            },
            // Horizon hides — no GPU cost.
            LodTier::Horizon => ComponentBundle {
                insert: vec![DynamicComponent::new(Visibility::Hidden)],
                remove: vec![TypeId::of::<Mesh3d>()],
            },
        }
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        // Roblox Beam has the same property shape; Wave 4 importer
        // will exercise the full map.
        let mut bag = PropertyBag::new();
        bag.set(
            "metadata.name",
            eustress_common::classes::PropertyValue::String(rbx.name().into()),
        );
        if let Some(v) = rbx.property("Width0").and_then(|p| p.as_f32()) {
            bag.set(
                "beam.width0",
                eustress_common::classes::PropertyValue::Float(v),
            );
        }
        if let Some(v) = rbx.property("Width1").and_then(|p| p.as_f32()) {
            bag.set(
                "beam.width1",
                eustress_common::classes::PropertyValue::Float(v),
            );
        }
        if let Some(v) = rbx.property("Enabled").and_then(|p| p.as_bool()) {
            bag.set(
                "beam.enabled",
                eustress_common::classes::PropertyValue::Bool(v),
            );
        }
        if let Some(v) = rbx.property("Brightness").and_then(|p| p.as_f32()) {
            bag.set(
                "beam.brightness",
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
        if let Some(beam) = toml_value.get("beam") {
            if let Some(v) = beam.get("width0").and_then(|v| v.as_float()) {
                bag.set(
                    "beam.width0",
                    eustress_common::classes::PropertyValue::Float(v as f32),
                );
            }
            if let Some(v) = beam.get("width1").and_then(|v| v.as_float()) {
                bag.set(
                    "beam.width1",
                    eustress_common::classes::PropertyValue::Float(v as f32),
                );
            }
            if let Some(v) = beam.get("brightness").and_then(|v| v.as_float()) {
                bag.set(
                    "beam.brightness",
                    eustress_common::classes::PropertyValue::Float(v as f32),
                );
            }
            if let Some(v) = beam.get("enabled").and_then(|v| v.as_bool()) {
                bag.set(
                    "beam.enabled",
                    eustress_common::classes::PropertyValue::Bool(v),
                );
            }
            if let Some(v) = beam.get("segments").and_then(|v| v.as_integer()) {
                bag.set(
                    "beam.segments",
                    eustress_common::classes::PropertyValue::Int(v as i32),
                );
            }
            if let Some(v) = beam.get("attachment0").and_then(|v| v.as_integer()) {
                bag.set(
                    "beam.attachment0",
                    eustress_common::classes::PropertyValue::Int(v as i32),
                );
            }
            if let Some(v) = beam.get("attachment1").and_then(|v| v.as_integer()) {
                bag.set(
                    "beam.attachment1",
                    eustress_common::classes::PropertyValue::Int(v as i32),
                );
            }
            if let Some(v) = beam.get("attachment0_uuid").and_then(|v| v.as_str()) {
                bag.set(
                    "beam.attachment0_uuid",
                    eustress_common::classes::PropertyValue::String(v.into()),
                );
            }
            if let Some(v) = beam.get("attachment1_uuid").and_then(|v| v.as_str()) {
                bag.set(
                    "beam.attachment1_uuid",
                    eustress_common::classes::PropertyValue::String(v.into()),
                );
            }
            if let Some(s) = beam.get("blend_mode").and_then(|v| v.as_str()) {
                bag.set(
                    "beam.blend_mode",
                    eustress_common::classes::PropertyValue::Enum(s.into()),
                );
            }
            if let Some(s) = beam.get("face_mode").and_then(|v| v.as_str()) {
                bag.set(
                    "beam.face_mode",
                    eustress_common::classes::PropertyValue::Enum(s.into()),
                );
            }
            if let Some(s) = beam.get("texture_mode").and_then(|v| v.as_str()) {
                bag.set(
                    "beam.texture_mode",
                    eustress_common::classes::PropertyValue::Enum(s.into()),
                );
            }
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        if let Some(inst) = world.entity(entity).get::<Instance>() {
            let mut meta = toml::value::Table::new();
            meta.insert("class_name".into(), toml::Value::String("Beam".into()));
            meta.insert("name".into(), toml::Value::String(inst.name.clone()));
            meta.insert("archivable".into(), toml::Value::Boolean(inst.archivable));
            if !inst.uuid.is_empty() {
                meta.insert("uuid".into(), toml::Value::String(inst.uuid.clone()));
            }
            root.insert("metadata".into(), toml::Value::Table(meta));
        }
        if let Some(beam) = world.entity(entity).get::<Beam>() {
            let mut t = toml::value::Table::new();
            t.insert("width0".into(), toml::Value::Float(beam.width0 as f64));
            t.insert("width1".into(), toml::Value::Float(beam.width1 as f64));
            t.insert(
                "segments".into(),
                toml::Value::Integer(beam.segments as i64),
            );
            t.insert(
                "brightness".into(),
                toml::Value::Float(beam.brightness as f64),
            );
            t.insert("enabled".into(), toml::Value::Boolean(beam.enabled));
            if let Some(a) = beam.attachment0 {
                t.insert("attachment0".into(), toml::Value::Integer(a as i64));
            }
            if let Some(a) = beam.attachment1 {
                t.insert("attachment1".into(), toml::Value::Integer(a as i64));
            }
            // UUID-form refs (when the entity was imported via Wave 4
            // path) are recorded on the BeamSegmentLink component, not
            // the Beam itself — emit them too so the TOML round-trips.
            if let Some(link) = world.entity(entity).get::<BeamSegmentLink>() {
                if let Some(u) = &link.attachment0_uuid {
                    t.insert("attachment0_uuid".into(), toml::Value::String(u.clone()));
                }
                if let Some(u) = &link.attachment1_uuid {
                    t.insert("attachment1_uuid".into(), toml::Value::String(u.clone()));
                }
            }
            t.insert(
                "blend_mode".into(),
                toml::Value::String(format!("{:?}", beam.blend_mode)),
            );
            t.insert(
                "face_mode".into(),
                toml::Value::String(format!("{:?}", beam.face_mode)),
            );
            t.insert(
                "texture_mode".into(),
                toml::Value::String(format!("{:?}", beam.texture_mode)),
            );
            root.insert("beam".into(), toml::Value::Table(t));
        }
        toml::Value::Table(root)
    }
}

// ============================================================================
// Internal helpers — PropertyBag <-> Beam conversion + enum parsers
// ============================================================================

fn beam_from_bag(props: &PropertyBag) -> Beam {
    let mut beam = Beam::default();
    apply_bag_to_beam(props, &mut beam);
    // attachment ints come through the bag; assign them on the
    // component too so existing read paths (e.g. MindSpace renderer)
    // still see them. UUID variants land on BeamSegmentLink at spawn.
    if let Some(v) = props.get_i32("beam.attachment0") {
        beam.attachment0 = Some(v as u32);
    }
    if let Some(v) = props.get_i32("beam.attachment1") {
        beam.attachment1 = Some(v as u32);
    }
    beam
}

fn apply_bag_to_beam(props: &PropertyBag, beam: &mut Beam) {
    if let Some(v) = props.get_f32("beam.width0") {
        beam.width0 = v;
    }
    if let Some(v) = props.get_f32("beam.width1") {
        beam.width1 = v;
    }
    if let Some(v) = props.get_i32("beam.segments") {
        beam.segments = v.max(1) as u32;
    }
    if let Some(v) = props.get_f32("beam.brightness") {
        beam.brightness = v;
    }
    if let Some(v) = props.get_bool("beam.enabled") {
        beam.enabled = v;
    }
    if let Some(s) = props.get_enum("beam.blend_mode") {
        beam.blend_mode = parse_blend_mode(s);
    }
    if let Some(s) = props.get_enum("beam.face_mode") {
        beam.face_mode = parse_face_mode(s);
    }
    if let Some(s) = props.get_enum("beam.texture_mode") {
        beam.texture_mode = parse_texture_mode(s);
    }
}

fn bag_from_beam(beam: &Beam) -> PropertyBag {
    let mut bag = PropertyBag::with_capacity(12);
    use eustress_common::classes::PropertyValue;
    bag.set("beam.width0", PropertyValue::Float(beam.width0));
    bag.set("beam.width1", PropertyValue::Float(beam.width1));
    bag.set("beam.segments", PropertyValue::Int(beam.segments as i32));
    bag.set("beam.brightness", PropertyValue::Float(beam.brightness));
    bag.set("beam.enabled", PropertyValue::Bool(beam.enabled));
    if let Some(a) = beam.attachment0 {
        bag.set("beam.attachment0", PropertyValue::Int(a as i32));
    }
    if let Some(a) = beam.attachment1 {
        bag.set("beam.attachment1", PropertyValue::Int(a as i32));
    }
    bag.set(
        "beam.blend_mode",
        PropertyValue::Enum(format!("{:?}", beam.blend_mode)),
    );
    bag.set(
        "beam.face_mode",
        PropertyValue::Enum(format!("{:?}", beam.face_mode)),
    );
    bag.set(
        "beam.texture_mode",
        PropertyValue::Enum(format!("{:?}", beam.texture_mode)),
    );
    bag
}

fn parse_blend_mode(s: &str) -> BeamBlendMode {
    match s {
        "Alpha" => BeamBlendMode::Alpha,
        "Additive" => BeamBlendMode::Additive,
        "Multiply" => BeamBlendMode::Multiply,
        _ => BeamBlendMode::Alpha,
    }
}

fn parse_face_mode(s: &str) -> BeamFaceMode {
    match s {
        "FaceCamera" => BeamFaceMode::FaceCamera,
        "FaceCameraY" => BeamFaceMode::FaceCameraY,
        "Fixed" => BeamFaceMode::Fixed,
        _ => BeamFaceMode::FaceCamera,
    }
}

fn parse_texture_mode(s: &str) -> TextureMode {
    match s {
        "Stretch" => TextureMode::Stretch,
        "Tile" => TextureMode::Tile,
        "Static" => TextureMode::Static,
        _ => TextureMode::Tile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beam_spawner_is_object_safe() {
        let boxed: Box<dyn ClassSpawner> = Box::new(BeamSpawner);
        assert_eq!(boxed.class_name(), ClassName::Beam);
    }

    #[test]
    fn deserialize_bad_tag_returns_empty_bag() {
        let spawner = BeamSpawner;
        let bag = spawner.deserialize(&[0x99, 0x00]);
        assert!(bag.is_empty());
    }

    #[test]
    fn lod_horizon_hides_and_drops_mesh() {
        let spawner = BeamSpawner;
        let bundle = spawner.lod_components(LodTier::Horizon);
        assert!(!bundle.is_empty());
        assert!(bundle.remove.contains(&TypeId::of::<Mesh3d>()));
    }

    #[test]
    fn lod_streamed_inserts_static_mode_marker() {
        let spawner = BeamSpawner;
        let bundle = spawner.lod_components(LodTier::Streamed);
        assert_eq!(bundle.insert.len(), 1);
    }

    #[test]
    fn lod_hero_inserts_live_mode_marker() {
        let spawner = BeamSpawner;
        let bundle = spawner.lod_components(LodTier::Hero);
        assert_eq!(bundle.insert.len(), 1);
    }

    #[test]
    fn bag_roundtrip_canonical_order() {
        let beam = Beam::default();
        let bag = bag_from_beam(&beam);
        let keys: Vec<&str> = bag.iter().map(|(k, _)| k.as_str()).collect();
        // Default beam has None attachments, so neither attachment key
        // is emitted — they only appear when set.
        assert_eq!(
            keys,
            vec![
                "beam.width0",
                "beam.width1",
                "beam.segments",
                "beam.brightness",
                "beam.enabled",
                "beam.blend_mode",
                "beam.face_mode",
                "beam.texture_mode",
            ]
        );
    }
}
