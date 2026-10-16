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

        // Without a `&World` the attachments can't be resolved here;
        // `sync_beam_transforms` stretches the unit cylinder between them.
        let mesh_handle = ctx.meshes.add(beam_mesh(&beam));
        let material_handle = ctx.standard_materials.add(beam_material(&beam));

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
// Live sync — the "Wave 4 sync system" `BeamSegmentLink` was always meant
// to feed (see its doc comment above). Runs every frame: reads each beam's
// two attachment UUIDs, resolves them against the live ECS via
// `Instance.uuid` (NOT a WorldDb round-trip — that's the persisted-store
// lookup used by `find_entity_by_uuid`, wrong layer for a per-frame system),
// and keeps the stretched-cylinder mesh taut between them. This is what
// makes a dragged mind-map node's edges follow it instead of staying
// wherever they were baked.
// ============================================================================

/// The beam's cylinder: unit height, because `sync_beam_transforms` writes the
/// span between the attachments into `scale.y` (a 4 m mesh drew every edge
/// four times too long). The radius is half the mean width. An unresolved
/// beam shows as a 1 m stub along +Y.
fn beam_mesh(beam: &Beam) -> Mesh {
    let radius = ((beam.width0 + beam.width1) * 0.25).max(0.005);
    Cylinder::new(radius, 1.0).mesh().build()
}

/// The beam's material: the first colour key (white when there is none),
/// the first transparency key as alpha, `light_emission` as glow.
fn beam_material(beam: &Beam) -> StandardMaterial {
    let color = beam.color_sequence.first().map(|(_, c)| *c).unwrap_or(Color::WHITE);
    let alpha = 1.0 - beam.transparency_sequence.first().map(|(_, t)| *t).unwrap_or(0.0).clamp(0.0, 1.0);
    let glow = beam.light_emission.max(0.0);
    let emissive = if glow > 0.0 {
        // LinearRgba is what bevy materials want for emissive HDR.
        let srgba = color.to_srgba();
        LinearRgba::new(srgba.red * glow, srgba.green * glow, srgba.blue * glow, 1.0)
    } else {
        LinearRgba::BLACK
    };
    StandardMaterial {
        base_color: color.with_alpha(alpha),
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
    }
}

/// Beams loaded from a Space folder arrive as data only: the instance loader
/// adds `Beam` and its `BeamSegmentLink`, no geometry. Give each the cylinder
/// and material `BeamSpawner` builds, so edges on disk are visible.
pub fn hydrate_beam_visuals(
    mut commands: Commands,
    beams: Query<(Entity, &Beam), (Added<Beam>, Without<Mesh3d>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, beam) in &beams {
        let visibility = if beam.enabled { Visibility::default() } else { Visibility::Hidden };
        commands.entity(entity).insert((
            Mesh3d(meshes.add(beam_mesh(beam))),
            MeshMaterial3d(materials.add(beam_material(beam))),
            BeamLodMode::Live,
            visibility,
        ));
    }
}

/// Keeps every `Beam`'s transform taut between its two attachments, every
/// frame. Unresolved attachments (UUID doesn't match any live entity, or a
/// beam was authored with no attachments at all) are left exactly as the
/// spawner built them — this system only ever tightens a beam it can fully
/// resolve, never collapses one it can't.
///
/// Each beam's two ends are kept between frames rather than found by hashing
/// the uuid of every instance each frame (about 120,000 on a large Space). A
/// beam is looked up again when its link changed, when a kept end stopped
/// carrying its uuid (or was despawned, or lost its `Instance` or
/// `GlobalTransform`), or when an instance took up one of its uuids since
/// the last run: one spawned, one whose uuid was written, one that just got
/// a `GlobalTransform`. Only then can the entity a uuid names change, so the
/// kept ends are the ones the full lookup would find. A uuid that several
/// entities carry goes to whichever of them the query visits last, which can
/// change from frame to frame, so a beam naming one is looked up every frame
/// while it lasts.
///
/// The transform is written only when a value changes, so `Changed<Transform>`
/// readers wake only for a beam that moved.
#[allow(clippy::type_complexity)]
pub fn sync_beam_transforms(
    mut beams: Query<(Entity, Ref<BeamSegmentLink>, &mut Transform), With<Beam>>,
    targets: Query<(Entity, &Instance, &GlobalTransform)>,
    arrivals: Query<&Instance, (With<GlobalTransform>, Or<(Changed<Instance>, Added<GlobalTransform>)>)>,
    // Each beam's two ends, in order, or `None` when an end matched no
    // entity at the last lookup.
    mut ends: Local<bevy::ecs::entity::EntityHashMap<Option<(Entity, Entity)>>>,
) {
    if beams.is_empty() {
        ends.clear();
        return;
    }

    // The uuids an entity may have taken up since the last run. Usually
    // there are none.
    let arrived: std::collections::HashSet<&str> = arrivals
        .iter()
        .map(|instance| instance.uuid.as_str())
        .filter(|uuid| !uuid.is_empty())
        .collect();
    // Where `entity` is, while it still carries `uuid`.
    let position = |entity: Entity, uuid: &str| {
        targets
            .get(entity)
            .ok()
            .filter(|(_, instance, _)| instance.uuid == uuid)
            .map(|(_, _, gt)| gt.translation())
    };
    // Every instance by uuid, built only on a frame with a lookup to do. The
    // entity the query visits last wins a uuid that several carry; those
    // uuids are collected too.
    let mut index: Option<(std::collections::HashMap<&str, (Entity, Vec3)>, std::collections::HashSet<&str>)> =
        None;
    let mut kept = 0;

    for (beam, link, mut transform) in &mut beams {
        let (Some(a_uuid), Some(b_uuid)) = (&link.attachment0_uuid, &link.attachment1_uuid) else {
            ends.remove(&beam);
            continue;
        };
        let (a_uuid, b_uuid) = (a_uuid.as_str(), b_uuid.as_str());

        // The ends kept from an earlier frame, while they still hold.
        let mut at = None;
        let mut holds = false;
        if !link.is_changed() && !arrived.contains(a_uuid) && !arrived.contains(b_uuid) {
            match ends.get(&beam) {
                // An end matched nothing, and no instance has taken up its
                // uuid since.
                Some(&None) => holds = true,
                // Both ends still carry their uuids, and no other instance
                // has taken up either since.
                Some(&Some((a, b))) => {
                    if let (Some(a_pos), Some(b_pos)) = (position(a, a_uuid), position(b, b_uuid)) {
                        at = Some((a_pos, b_pos));
                        holds = true;
                    }
                }
                None => {}
            }
        }
        if holds {
            kept += 1;
        } else {
            let (by_uuid, shared) = index.get_or_insert_with(|| {
                let mut by_uuid = std::collections::HashMap::new();
                let mut shared = std::collections::HashSet::new();
                for (entity, instance, gt) in &targets {
                    if !instance.uuid.is_empty()
                        && by_uuid.insert(instance.uuid.as_str(), (entity, gt.translation())).is_some()
                    {
                        shared.insert(instance.uuid.as_str());
                    }
                }
                (by_uuid, shared)
            });
            match (by_uuid.get(a_uuid), by_uuid.get(b_uuid)) {
                (Some(&(a, a_pos)), Some(&(b, b_pos))) => {
                    at = Some((a_pos, b_pos));
                    // Which entity wins a shared uuid can change from frame
                    // to frame, so a beam naming one is not kept.
                    if shared.contains(a_uuid) || shared.contains(b_uuid) {
                        ends.remove(&beam);
                    } else {
                        ends.insert(beam, Some((a, b)));
                        kept += 1;
                    }
                }
                _ => {
                    ends.insert(beam, None);
                    kept += 1;
                }
            }
        }
        let Some((a_pos, b_pos)) = at else {
            continue;
        };

        let delta = b_pos - a_pos;
        let length = delta.length();
        if length < 1e-5 {
            continue;
        }
        let dir = delta / length;

        let translation = (a_pos + b_pos) * 0.5;
        // Shortest-arc rotation from the cylinder mesh's local +Y (its long
        // axis) onto the live direction between the two attachments — same
        // convention the spawner used to build the beam once; this just
        // re-runs it continuously instead of only at spawn time.
        let rotation = Quat::from_rotation_arc(Vec3::Y, dir);
        // Compared bit for bit: `==` holds 0.0 equal to -0.0, so a write
        // skipped on it could leave the other zero in place.
        if !same_bits(&transform.translation.to_array(), &translation.to_array())
            || !same_bits(&transform.rotation.to_array(), &rotation.to_array())
            || transform.scale.y.to_bits() != length.to_bits()
        {
            transform.translation = translation;
            transform.rotation = rotation;
            transform.scale.y = length;
        }
    }

    // Forget the beams that were despawned or stopped being beams.
    if ends.len() > kept {
        ends.retain(|beam, _| beams.contains(*beam));
    }
}

/// Whether two runs of floats hold the same bits.
fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
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

    // ---- sync_beam_transforms against its every-frame form ----

    /// `sync_beam_transforms` without kept ends, hashing every instance by
    /// uuid and writing every resolved beam every frame: the body it had
    /// before, comments dropped. The kept-ends system must leave every beam
    /// exactly as this one does.
    fn sync_beam_transforms_every_frame(
        mut beams: Query<(&BeamSegmentLink, &mut Transform), With<Beam>>,
        targets: Query<(&Instance, &GlobalTransform)>,
    ) {
        if beams.is_empty() {
            return;
        }

        let mut positions: std::collections::HashMap<&str, Vec3> = std::collections::HashMap::new();
        for (instance, gt) in &targets {
            if !instance.uuid.is_empty() {
                positions.insert(instance.uuid.as_str(), gt.translation());
            }
        }

        for (link, mut transform) in &mut beams {
            let (Some(a_uuid), Some(b_uuid)) = (&link.attachment0_uuid, &link.attachment1_uuid) else {
                continue;
            };
            let (Some(&a_pos), Some(&b_pos)) =
                (positions.get(a_uuid.as_str()), positions.get(b_uuid.as_str()))
            else {
                continue;
            };

            let delta = b_pos - a_pos;
            let length = delta.length();
            if length < 1e-5 {
                continue;
            }
            let dir = delta / length;

            transform.translation = (a_pos + b_pos) * 0.5;
            transform.rotation = Quat::from_rotation_arc(Vec3::Y, dir);
            transform.scale.y = length;
        }
    }

    fn attachment(uuid: &str, at: Vec3) -> (Instance, Transform, GlobalTransform) {
        (
            Instance { uuid: uuid.to_string(), ..Default::default() },
            Transform::from_translation(at),
            GlobalTransform::from_translation(at),
        )
    }

    fn beam_between(uuid: &str, a: Option<&str>, b: Option<&str>) -> (Instance, Beam, BeamSegmentLink, Transform) {
        (
            Instance { uuid: uuid.to_string(), class_name: ClassName::Beam, ..Default::default() },
            Beam::default(),
            BeamSegmentLink {
                attachment0_uuid: a.map(str::to_string),
                attachment1_uuid: b.map(str::to_string),
                ..Default::default()
            },
            Transform::default(),
        )
    }

    /// Moves an attachment. A bare app has no transform propagation, so the
    /// `GlobalTransform` the sync reads is written too.
    fn move_end(world: &mut World, end: Entity, to: Vec3) {
        world.entity_mut(end).insert((Transform::from_translation(to), GlobalTransform::from_translation(to)));
    }

    fn beam_app(kept: bool) -> App {
        let mut app = App::new();
        if kept {
            app.add_systems(Update, sync_beam_transforms);
        } else {
            app.add_systems(Update, sync_beam_transforms_every_frame);
        }
        app
    }

    /// One app's entities, in the order the script spawned them.
    #[derive(Default)]
    struct BeamScene {
        ends: Vec<Entity>,
        beams: Vec<Entity>,
    }

    /// The edits made before frame `step`, the same in both apps.
    fn beam_step(world: &mut World, step: usize, scene: &mut BeamScene) {
        let BeamScene { ends, beams } = scene;
        match step {
            0 => {
                for (uuid, at) in [
                    ("a0", Vec3::ZERO),
                    ("a1", Vec3::new(0.0, 4.0, 0.0)),
                    ("a2", Vec3::new(3.0, 0.0, 4.0)),
                    ("a3", Vec3::new(1.0, 1.0, 1.0)),
                    ("a4", Vec3::new(-2.0, 5.0, 1.0)),
                ] {
                    ends.push(world.spawn(attachment(uuid, at)).id());
                }
                for (uuid, a, b) in [
                    ("b0", Some("a0"), Some("a1")),
                    ("b1", Some("a1"), Some("a2")),
                    // Dangling until step 5.
                    ("b2", Some("a0"), Some("late")),
                    // Authored without ends until step 14.
                    ("b3", None, None),
                    // Zero length.
                    ("b4", Some("a3"), Some("a3")),
                    ("b5", Some("a2"), Some("a4")),
                    ("b6", Some("a0"), None),
                    ("b7", Some("a3"), Some("a4")),
                ] {
                    beams.push(world.spawn(beam_between(uuid, a, b)).id());
                }
            }
            2 => move_end(world, ends[0], Vec3::new(-1.0, 0.5, 2.0)),
            // A link edit on a beam whose ends are kept.
            3 => world.get_mut::<BeamSegmentLink>(beams[1]).unwrap().attachment1_uuid = Some("a3".to_string()),
            // b5 loses an end.
            4 => world.entity_mut(ends[2]).despawn(),
            // The late attachment b2 has waited for: ends[5].
            5 => ends.push(world.spawn(attachment("late", Vec3::new(5.0, 5.0, 5.0))).id()),
            // A uuid written in place: b0 and b1 lose "a1".
            6 => world.get_mut::<Instance>(ends[1]).unwrap().uuid = "a1-renamed".to_string(),
            // A second "a0", ends[6], spawned after the first: the query
            // visits it last, so it wins b2's end.
            7 => ends.push(world.spawn(attachment("a0", Vec3::new(9.0, 0.0, 0.0))).id()),
            8 => move_end(world, ends[6], Vec3::new(9.0, 3.0, -2.0)),
            // The first "a0" moves to a table of its own, created last, and
            // the query now visits it last: it wins b2's end back.
            9 => {
                world.entity_mut(ends[0]).insert(Name::new("a0, in a new table"));
            }
            // "a0" is one entity again.
            10 => world.entity_mut(ends[6]).despawn(),
            // b7 loses an end with its GlobalTransform, then gets it back.
            11 => {
                world.entity_mut(ends[3]).remove::<GlobalTransform>();
            }
            12 => {
                world.entity_mut(ends[3]).insert(GlobalTransform::from_translation(Vec3::new(2.0, -1.0, 0.0)));
            }
            // A write to a beam's transform from elsewhere.
            13 => world.get_mut::<Transform>(beams[7]).unwrap().translation = Vec3::ZERO,
            14 => {
                let mut link = world.get_mut::<BeamSegmentLink>(beams[3]).unwrap();
                link.attachment0_uuid = Some("a3".to_string());
                link.attachment1_uuid = Some("late".to_string());
            }
            15 => world.entity_mut(beams[1]).despawn(),
            // "a1" comes back: b0 is whole again.
            16 => world.get_mut::<Instance>(ends[1]).unwrap().uuid = "a1".to_string(),
            _ => {}
        }
    }

    fn beam_transforms(app: &App, beams: &[Entity]) -> Vec<Option<[u32; 10]>> {
        let world = app.world();
        beams.iter().map(|&beam| world.get::<Transform>(beam).map(transform_bits)).collect()
    }

    fn transform_bits(transform: &Transform) -> [u32; 10] {
        let [tx, ty, tz] = transform.translation.to_array();
        let [rx, ry, rz, rw] = transform.rotation.to_array();
        let [sx, sy, sz] = transform.scale.to_array();
        [tx, ty, tz, rx, ry, rz, rw, sx, sy, sz].map(f32::to_bits)
    }

    #[test]
    fn kept_beam_ends_leave_what_looking_them_up_every_frame_does() {
        let mut every = beam_app(false);
        let mut kept = beam_app(true);
        let (mut every_scene, mut kept_scene) = (BeamScene::default(), BeamScene::default());
        let mut history = Vec::new();
        for step in 0..19 {
            beam_step(every.world_mut(), step, &mut every_scene);
            beam_step(kept.world_mut(), step, &mut kept_scene);
            every.update();
            kept.update();
            let want = beam_transforms(&every, &every_scene.beams);
            assert_eq!(beam_transforms(&kept, &kept_scene.beams), want, "frame {step}");
            history.push(want);
        }
        // The script reached what it is for: b2 waits for its late end, then
        // follows whichever "a0" the query visits last.
        let b2_at = |frame: usize| {
            let bits = history[frame][2].expect("b2 is never despawned");
            Vec3::new(f32::from_bits(bits[0]), f32::from_bits(bits[1]), f32::from_bits(bits[2]))
        };
        assert_eq!(b2_at(4), Vec3::ZERO, "b2 is left as built while its end is missing");
        assert_eq!(b2_at(7), Vec3::new(7.0, 2.5, 2.5), "the second \"a0\" wins");
        assert_eq!(b2_at(9), Vec3::new(2.0, 2.75, 3.5), "the first \"a0\" wins back");
        assert_eq!(history[18][1], None, "b1 was despawned");
    }

    /// The beams whose `Transform` changed since this last ran, per frame.
    #[derive(Resource, Default)]
    struct MovedBeams(Vec<usize>);

    fn count_moved_beams(moved: Query<(), (With<Beam>, Changed<Transform>)>, mut counts: ResMut<MovedBeams>) {
        counts.0.push(moved.iter().count());
    }

    #[test]
    fn a_beam_at_rest_is_not_marked_changed() {
        for kept in [false, true] {
            let mut app = App::new();
            app.init_resource::<MovedBeams>();
            if kept {
                app.add_systems(Update, (sync_beam_transforms, count_moved_beams).chain());
            } else {
                app.add_systems(Update, (sync_beam_transforms_every_frame, count_moved_beams).chain());
            }
            let world = app.world_mut();
            let a = world.spawn(attachment("a", Vec3::ZERO)).id();
            world.spawn(attachment("b", Vec3::new(0.0, 2.0, 0.0)));
            world.spawn(attachment("c", Vec3::new(3.0, 0.0, 0.0)));
            for (uuid, x, y) in [("ab", "a", "b"), ("bc", "b", "c"), ("ca", "c", "a"), ("ax", "a", "missing")] {
                world.spawn(beam_between(uuid, Some(x), Some(y)));
            }
            for frame in 0..6 {
                if frame == 4 {
                    move_end(app.world_mut(), a, Vec3::new(0.0, 0.0, -1.0));
                }
                app.update();
            }
            // Frame 0 counts every beam, a new one counting as changed. After
            // that the every-frame body writes the three it resolves each
            // frame; the kept-ends one writes only the two that `a` moved.
            let want: &[usize] = if kept { &[4, 0, 0, 0, 2, 0] } else { &[4, 3, 3, 3, 3, 3] };
            assert_eq!(app.world().resource::<MovedBeams>().0, want, "kept: {kept}");
        }
    }
}
