//! Instance loader - loads .glb.toml files as entity instances
//!
//! Architecture:
//! - Mesh assets live in assets/meshes/ (shared, reusable)
//! - Instance files (.glb.toml) live in Workspace/ (unique per entity)
//! - Each .toml references a mesh asset and defines instance-specific properties

use bevy::prelude::*;
use bevy::camera::primitives::MeshAabb;
use bevy::camera::visibility::VisibilityRange;
use bevy::pbr::decal::ForwardDecalMaterial;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use avian3d::prelude::{Collider, ColliderDensity, Friction, Restitution, RigidBody};
use crate::rendering::PartEntity;
use eustress_common::{Attributes, Tags};

/// Instance definition loaded from .glb.toml or .instance.toml file.
///
/// Field names on the wire are snake_case — the engine's historic
/// convention, shared with `GuiTomlFile` + every other TOML parser.
/// The common-crate `class_schema::load_and_heal_instance` pass
/// normalises any-case incoming keys to snake_case before
/// deserialization, so TOMLs rewritten to PascalCase during the
/// aborted migration still load without change. A fresh PascalCase
/// migration (if we ever want one) needs every consumer — not just
/// this struct — migrated in lockstep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceDefinition {
    /// Mesh reference — optional for non-visual instances (lighting, sky, atmosphere)
    #[serde(default)]
    pub asset: Option<AssetReference>,
    /// World transform — optional for non-visual instances
    #[serde(default)]
    pub transform: TransformData,
    /// Standard part properties (color, anchored, etc.) — all defaulted
    #[serde(default)]
    pub properties: InstanceProperties,
    pub metadata: InstanceMetadata,
    /// Optional realism material properties (dynamic on any class)
    #[serde(default)]
    pub material: Option<TomlMaterialProperties>,
    /// Optional thermodynamic state (dynamic on any class)
    #[serde(default)]
    pub thermodynamic: Option<TomlThermodynamicState>,
    /// Optional electrochemical state (dynamic on any class)
    #[serde(default)]
    pub electrochemical: Option<TomlElectrochemicalState>,
    /// Optional nuclear reactor state (ArcReactorCore class)
    #[serde(default)]
    pub nuclear: Option<TomlNuclearState>,
    /// Optional plasma state (dynamic on any class)
    #[serde(default)]
    pub plasma: Option<TomlPlasmaState>,
    /// Optional UI class properties (TextLabel, TextButton, Frame, ImageLabel, etc.)
    #[serde(default)]
    pub ui: Option<UiInstanceProperties>,
    /// Custom attributes (key-value pairs for scripting)
    #[serde(default)]
    pub attributes: Option<std::collections::HashMap<String, toml::Value>>,
    /// Tags for CollectionService grouping
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Instance parameters (custom configuration values)
    #[serde(default)]
    pub parameters: Option<std::collections::HashMap<String, toml::Value>>,
    /// All unknown top-level sections (e.g. [Appearance], [Position], [Lighting]) captured
    /// via flatten so rich-schema .instance.toml files work without hardcoded field names.
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, toml::Value>,
}

/// Reference to a shared mesh asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReference {
    /// Path to mesh file (relative to Space root)
    pub mesh: String,
    /// glTF scene name (usually "Scene0")
    #[serde(default = "default_scene")]
    pub scene: String,
}

fn default_scene() -> String {
    "Scene0".to_string()
}

/// Clamp a TOML-loaded size vector to strictly positive, finite values.
///
/// A part saved with a zero, negative, or NaN dimension would panic
/// Avian's collider builder during space load:
/// `collision/collider/mod.rs:512: assertion failed: b.min.cmple(b.max).all()`
/// — a `Collider::cuboid(hx, hy, hz)` with a negative half-extent flips
/// the resulting AABB's min and max. This helper keeps a single floor
/// (0.1 studs) so the physics world + save round-trip stay sane.
fn sanitize_size(v: Vec3) -> Vec3 {
    const MIN: f32 = 0.1;
    Vec3::new(
        if v.x.is_finite() { v.x.abs().max(MIN) } else { MIN },
        if v.y.is_finite() { v.y.abs().max(MIN) } else { MIN },
        if v.z.is_finite() { v.z.abs().max(MIN) } else { MIN },
    )
}

/// Sanitize TOML-loaded position — non-finite components drop to zero.
/// Avian's world-space AABB routine propagates NaN from any transform
/// component into the AABB min/max, tripping the same collider
/// assertion as a bad size.
fn sanitize_pos(v: Vec3) -> Vec3 {
    Vec3::new(
        if v.x.is_finite() { v.x } else { 0.0 },
        if v.y.is_finite() { v.y } else { 0.0 },
        if v.z.is_finite() { v.z } else { 0.0 },
    )
}

/// Sanitize TOML-loaded rotation quaternion. Non-finite components or
/// zero-length quaternions fall back to identity. A valid-but-not-unit
/// quaternion gets normalized.
fn sanitize_rot(q: Quat) -> Quat {
    let arr = [q.x, q.y, q.z, q.w];
    if !arr.iter().all(|c| c.is_finite()) {
        return Quat::IDENTITY;
    }
    let len_sq = q.length_squared();
    if !len_sq.is_finite() || len_sq < 1e-8 {
        return Quat::IDENTITY;
    }
    q.normalize()
}

/// Build an Avian collider from `scale` + `part_shape`, refusing to call
/// the Avian constructor with any value Avian would assertion-panic on.
///
/// Avian's `ColliderAabb::grow` tree-update path `debug_assert!`s
/// `min <= max` on the world-space AABB. A non-finite Transform
/// component OR a non-positive half-extent propagates NaN/inverted
/// bounds through that path and crashes the engine on space load.
/// Returns `None` when inputs are unsafe so the caller can skip the
/// collider insertion (part still spawns, just as a decorative
/// visual without physics).
///
/// `transform` is also validated because Avian's `Add<Collider>`
/// observer reads `Position` + `Rotation` (both synced from Transform)
/// and passes them into `grow()`, which panics on any non-finite input.
fn safe_collider_from(
    part_shape: eustress_common::classes::PartType,
    scale: Vec3,
    transform: &Transform,
) -> Option<Collider> {
    const MIN_HALF: f32 = 0.05;
    // Transform translation/rotation must be finite — Avian's on-add
    // observer projects these into the world-space AABB.
    let t = transform.translation;
    if !t.x.is_finite() || !t.y.is_finite() || !t.z.is_finite() {
        return None;
    }
    let r = transform.rotation;
    if !r.x.is_finite() || !r.y.is_finite() || !r.z.is_finite() || !r.w.is_finite() {
        return None;
    }
    // Reject zero-length / non-unit quaternions — Avian's AABB math
    // assumes a proper rotation; a `[0,0,0,0]` quat collapses the
    // rotated bounds to a point which technically passes min<=max,
    // but more pathological inputs can produce NaN through the
    // multiplication chain.
    let r_len_sq = r.length_squared();
    if !r_len_sq.is_finite() || r_len_sq < 1e-8 {
        return None;
    }
    // The Transform's OWN scale must be finite AND strictly positive on every
    // axis. This is the entity's actual `Transform.scale` (pass the SAME
    // transform the entity is spawned with — e.g. `render_transform`, which
    // folds in a DataMesh `mesh_visual_scale` that a mirrored import can make
    // NEGATIVE). Avian's `Collider` on-insert hook overwrites the collider's
    // scale with the entity's `GlobalTransform.scale()`; a negative axis flips
    // the collider's half-extents so its AABB has `min > max`, and Avian's
    // broadphase `grow()` panics the instant the collider is inserted
    // (`collider/mod.rs:512: b.min.cmple(b.max).all()`) — synchronously, in
    // `drain_pending_spawns`'s command flush, before any Update-schedule
    // safety-net can run. Returning `None` here skips physics for such a
    // (mirrored / degenerate) part — it still spawns and renders, just without
    // a collider — which is the only sane option since Avian cannot represent a
    // negatively-scaled collider anyway.
    let s = transform.scale;
    if !s.x.is_finite() || !s.y.is_finite() || !s.z.is_finite()
        || s.x <= 0.0 || s.y <= 0.0 || s.z <= 0.0
    {
        return None;
    }
    // Every half-extent component must be finite AND strictly positive.
    let hx = if scale.x.is_finite() { (scale.x * 0.5).abs().max(MIN_HALF) } else { return None; };
    let hy = if scale.y.is_finite() { (scale.y * 0.5).abs().max(MIN_HALF) } else { return None; };
    let hz = if scale.z.is_finite() { (scale.z * 0.5).abs().max(MIN_HALF) } else { return None; };
    Some(match part_shape {
        eustress_common::classes::PartType::Ball => Collider::sphere(hx),
        eustress_common::classes::PartType::Cylinder | eustress_common::classes::PartType::Cone => {
            Collider::cylinder(hx, hy)
        }
        _ => Collider::cuboid(hx, hy, hz),
    })
}

/// Attach the Avian physics-material components (`Friction`,
/// `Restitution`, `ColliderDensity`) to a just-spawned part when the
/// importer wrote a `[properties.physics]` section. Each component is
/// inserted only when its source value is present and finite — a part
/// with no physics section keeps Avian's defaults (no extra components),
/// preserving the existing decorative-part fast path.
///
/// Roblox supplies a single `friction()` scalar; the importer seeds both
/// `friction_static` and `friction_kinetic` from it, and Avian's
/// `Friction::new(static).with_dynamic_coefficient(kinetic)` carries the
/// pair. `restitution` ← Roblox `elasticity()`. `density` → `ColliderDensity`.
fn apply_physics_material(
    ec: &mut bevy::ecs::system::EntityCommands,
    physics: Option<&PhysicsProperties>,
) {
    let Some(p) = physics else { return };
    // Friction — insert when either coefficient is present. A missing
    // side falls back to the present one so a single Roblox value still
    // produces matched static/dynamic coefficients.
    let fs = p.friction_static.filter(|v| v.is_finite());
    let fk = p.friction_kinetic.filter(|v| v.is_finite());
    if fs.is_some() || fk.is_some() {
        let static_c = fs.or(fk).unwrap();
        let dynamic_c = fk.or(fs).unwrap();
        ec.insert(Friction::new(static_c).with_dynamic_coefficient(dynamic_c));
    }
    if let Some(r) = p.restitution.filter(|v| v.is_finite()) {
        ec.insert(Restitution::new(r));
    }
    // Density must be strictly positive — Avian derives mass from it and
    // a zero/negative density would yield a degenerate rigid body.
    if let Some(d) = p.density.filter(|v| v.is_finite() && *v > 0.0) {
        ec.insert(ColliderDensity(d));
    }
}

/// Transform data (position, rotation, scale).
///
/// All three fields tolerate omission so meshless / unsized classes
/// (Attachment, SoundSource, lighting probes, …) can ship a TOML
/// with only the bits that matter. `scale` defaults to `[1, 1, 1]`,
/// `rotation` to identity quaternion, `position` to origin — same
/// values as `Default::default()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformData {
    #[serde(default = "default_position")]
    pub position: [f32; 3],
    #[serde(default = "default_rotation")]
    pub rotation: [f32; 4], // Quaternion (x, y, z, w)
    #[serde(default = "default_scale")]
    pub scale: [f32; 3],
}

fn default_position() -> [f32; 3] { [0.0, 0.0, 0.0] }
fn default_rotation() -> [f32; 4] { [0.0, 0.0, 0.0, 1.0] }
fn default_scale() -> [f32; 3] { [1.0, 1.0, 1.0] }

impl Default for TransformData {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

impl From<TransformData> for Transform {
    fn from(data: TransformData) -> Self {
        Transform {
            translation: Vec3::from_array(data.position),
            rotation: Quat::from_xyzw(
                data.rotation[0],
                data.rotation[1],
                data.rotation[2],
                data.rotation[3],
            ),
            scale: Vec3::from_array(data.scale),
        }
    }
}

/// Apply the same sanity clamps `spawn_instance` uses to a `Transform`
/// loaded from disk: zero NaN/Inf positions, replace a non-normalisable
/// quaternion with identity, and clamp scale to a positive finite floor.
/// Hot-reload + any other path that re-applies disk state to a live
/// entity should call this so a transient mid-write partial parse can't
/// inject a non-finite component that panics Avian's
/// `assert_components_finite` check.
pub fn sanitize_transform(t: Transform) -> Transform {
    Transform {
        translation: sanitize_pos(t.translation),
        rotation: sanitize_rot(t.rotation),
        scale: sanitize_size(t.scale),
    }
}

/// Hard upper bound on a part's world-space coordinates. The drag-to-
/// move + drag-to-rotate tools clamp their target positions to
/// `[-MAX_WORLD_EXTENT, MAX_WORLD_EXTENT]` on every axis so dragging a
/// part "into the sky" can't produce an unbounded translation. Beyond
/// this limit Avian's broadphase sweeps start losing precision and
/// the camera's far-plane clipping makes the part invisible anyway —
/// no user benefit to allowing further travel, and infinity-large
/// numbers bleed back into other math as NaN through subtraction.
pub const MAX_WORLD_EXTENT: f32 = 5000.0;

/// Take a candidate world position and return a value safe to write
/// onto `Transform.translation`:
///
/// * If `candidate` has any NaN/Inf component, return `fallback`
///   (typically the entity's initial position before the drag started)
///   so a degenerate frame of math doesn't teleport the part.
/// * Clamp every axis to `[-MAX_WORLD_EXTENT, MAX_WORLD_EXTENT]` so
///   "dragged into the sky" produces a bounded translation rather
///   than letting the value accumulate into territory where Avian's
///   AABB math hits float-precision walls.
///
/// Use this at every drag-tool write site (move / scale / rotate /
/// align-distribute / mirror) — the catch-all
/// [`sanitize_part_transforms_safety_net`] still runs as a backstop,
/// but rejecting bad values at the source means the user sees the
/// drag clamp visibly instead of the safety net resetting the part
/// next frame.
pub fn safe_translation(candidate: Vec3, fallback: Vec3) -> Vec3 {
    let v = if candidate.is_finite() { candidate } else { fallback };
    let fb = if fallback.is_finite() { fallback } else { Vec3::ZERO };
    Vec3::new(
        v.x.clamp(-MAX_WORLD_EXTENT, MAX_WORLD_EXTENT)
            .as_finite_or(fb.x.clamp(-MAX_WORLD_EXTENT, MAX_WORLD_EXTENT)),
        v.y.clamp(-MAX_WORLD_EXTENT, MAX_WORLD_EXTENT)
            .as_finite_or(fb.y.clamp(-MAX_WORLD_EXTENT, MAX_WORLD_EXTENT)),
        v.z.clamp(-MAX_WORLD_EXTENT, MAX_WORLD_EXTENT)
            .as_finite_or(fb.z.clamp(-MAX_WORLD_EXTENT, MAX_WORLD_EXTENT)),
    )
}

/// Internal helper for `safe_translation`'s per-axis clamp. `clamp` on
/// `f32` returns NaN when `self` is NaN, so we need a follow-up
/// "if NaN, use fallback" step.
trait FiniteOr {
    fn as_finite_or(self, fallback: Self) -> Self;
}
impl FiniteOr for f32 {
    fn as_finite_or(self, fallback: f32) -> f32 {
        if self.is_finite() { self } else { fallback }
    }
}

/// Per-frame safety-net: walk every LOADED SCENE entity (`Instance`) and
/// sanitize its `Transform` so no NaN/Inf/negative component slips into
/// Avian's `Position` / `Rotation` / `Collider` AABB math. Catches
/// drag-handler bugs we haven't identified yet, plus degenerate imported
/// data.
///
/// **Why `With<Instance>` and not `With<RigidBody>`.** Avian's `Collider`
/// `on_insert` sets the collider scale from the entity's *world*
/// `GlobalTransform.scale()`, and the broadphase then asserts the world
/// AABB is valid (`min <= max`). A collider entity itself may be perfectly
/// clean, yet inherit a degenerate world scale from a NON-collider ANCESTOR
/// (an imported Model/Folder container) whose TOML transform was never
/// sanitized — a negative axis (Roblox mirror) or a zero/NaN — flipping the
/// child's world AABB and panicking Avian mid-load
/// (`collider/mod.rs:512: b.min.cmple(b.max).all()`). A `RigidBody`-only
/// scan can't see those ancestors, so it must cover the whole loaded
/// hierarchy. `Instance` is the marker every loaded part/model/folder
/// carries (superset of the rigid-body parts), so cleaning ancestors here
/// keeps every child's propagated `GlobalTransform` finite + positive.
///
/// Runs in `Update` — the local fix lands before that frame's PostUpdate
/// transform propagation, so the world transforms Avian consumes are clean.
///
/// **Repairs.** Non-finite translation → finite fallback; non-finite/near-
/// zero rotation → identity; and scale: any non-finite, negative, or near-
/// zero axis → `abs().max(min)`. Making a negative ancestor scale positive
/// un-mirrors that (broken-for-physics-anyway) imported model — an
/// acceptable cosmetic cost versus a hard crash. The aggressive
/// `MAX_WORLD_EXTENT` position clamp stays gated to actual rigid bodies
/// (`Has<RigidBody>`) so far-but-valid container/model positions in large
/// worlds (extent can exceed 5000 studs) are never disturbed.
///
/// **Read-then-fix split.** The hot path (no degenerate transforms)
/// uses `Ref<Transform>` so iteration is strictly read-only. Only
/// entities that need a fix are collected into a small buffer; the
/// second query takes `&mut Transform` and patches them. Combined with the
/// `is_changed()` gate this keeps the per-frame cost a pure finite/sign
/// check that is a no-op for every already-valid transform.
pub fn sanitize_part_transforms_safety_net(
    mut params: ParamSet<(
        Query<(Entity, Ref<Transform>, Has<avian3d::prelude::RigidBody>), With<eustress_common::classes::Instance>>,
        Query<&mut Transform>,
    )>,
    // Throttle handle for the AGGREGATE summary log. We must NEVER log per
    // part: on a large import (Vehicle Simulator has tens of thousands of
    // out-of-range parts) the per-part `warn!` Vec3-Debug formatting alone cost
    // ~18 s/frame — 96.7% of the entire frame — while the clamp math is ~10 ms.
    mut warn_occurrences: Local<u64>,
) {
    struct Fix {
        entity: Entity,
        translation: Option<Vec3>,
        rotation: Option<Quat>,
        scale: Option<Vec3>,
    }

    let mut fixes: Vec<Fix> = Vec::new();
    // Aggregate counters — replace the old per-part WARN spam. One sample is
    // kept for the summary so a degenerate value is still diagnosable.
    let (mut pos_fixes, mut rot_fixes, mut scale_fixes) = (0usize, 0usize, 0usize);
    let mut sample: Option<(Vec3, Vec3)> = None;

    {
        let reader = params.p0();
        for (entity, t, has_rb) in reader.iter() {
            // Only inspect transforms WRITTEN this frame. A value
            // cannot become NaN / out-of-range without the Transform
            // being changed; `Ref::is_changed()` is true on add (so a
            // freshly-loaded part is still validated once) and on every
            // physics/tool write (so degenerate writes are still
            // caught) — but the stable majority is skipped. This turns
            // a per-frame scan of EVERY rigid body into work
            // proportional to what actually moved: the exact "no
            // continuous checks that destroy FPS at scale" requirement.
            if !t.is_changed() {
                continue;
            }
            let pos = t.translation;
            // Non-finite translation is always fatal (NaN propagates into the
            // world AABB). The out-of-extent CLAMP, however, only applies to
            // real rigid bodies — a valid container/model legitimately placed
            // beyond MAX_WORLD_EXTENT in a large world must not be yanked back.
            let pos_bad = !pos.is_finite()
                || (has_rb && (pos.x.abs() > MAX_WORLD_EXTENT
                    || pos.y.abs() > MAX_WORLD_EXTENT
                    || pos.z.abs() > MAX_WORLD_EXTENT));
            let new_translation = if pos_bad {
                let clamped = safe_translation(pos, Vec3::ZERO);
                if sample.is_none() {
                    sample = Some((pos, clamped));
                }
                pos_fixes += 1;
                Some(clamped)
            } else {
                None
            };

            let rot = t.rotation;
            let rot_bad = !(rot.x.is_finite()
                && rot.y.is_finite()
                && rot.z.is_finite()
                && rot.w.is_finite())
                || rot.length_squared() < 1e-8;
            let new_rotation = if rot_bad {
                rot_fixes += 1;
                Some(Quat::IDENTITY)
            } else {
                None
            };

            // Scale must be finite AND strictly positive on every axis:
            // Avian multiplies the collider by `GlobalTransform.scale()`, and a
            // negative axis inverts the resulting AABB (min > max) → broadphase
            // panic, while a zero/NaN axis collapses or NaNs it. Repair any bad
            // axis to `abs().max(MIN)`; valid positive scales are left untouched
            // (no-op), so normal parts are unaffected.
            const MIN_SCALE: f32 = 1e-3;
            let scale = t.scale;
            let scale_bad = !scale.is_finite()
                || scale.x < MIN_SCALE
                || scale.y < MIN_SCALE
                || scale.z < MIN_SCALE;
            let new_scale = if scale_bad {
                scale_fixes += 1;
                let fix_axis = |v: f32| if v.is_finite() { v.abs().max(MIN_SCALE) } else { 1.0 };
                Some(Vec3::new(fix_axis(scale.x), fix_axis(scale.y), fix_axis(scale.z)))
            } else {
                None
            };

            if new_translation.is_some() || new_rotation.is_some() || new_scale.is_some() {
                fixes.push(Fix {
                    entity,
                    translation: new_translation,
                    rotation: new_rotation,
                    scale: new_scale,
                });
            }
        }
    }

    if fixes.is_empty() {
        return;
    }

    let mut writer = params.p1();
    for fix in fixes {
        if let Ok(mut t) = writer.get_mut(fix.entity) {
            if let Some(v) = fix.translation {
                t.translation = v;
            }
            if let Some(v) = fix.rotation {
                t.rotation = v;
            }
            if let Some(v) = fix.scale {
                t.scale = v;
            }
        }
    }

    // ONE aggregated, throttled summary — never per part. Log the first few
    // occurrences, then a heartbeat every 600th, so a persistent out-of-range
    // source (e.g. a huge imported coordinate that keeps getting re-clamped)
    // still leaves a trail without ever costing more than a single line.
    let total = pos_fixes + rot_fixes + scale_fixes;
    if total > 0 {
        *warn_occurrences += 1;
        let n = *warn_occurrences;
        if n <= 3 || n % 600 == 0 {
            let eg = sample
                .map(|(was, now)| format!("; e.g. {:?} → {:?}", was, now))
                .unwrap_or_default();
            tracing::warn!(
                "🛡️ Sanitized {} part transform(s) ({} translation, {} rotation, {} scale){} [occurrence #{}]",
                total, pos_fixes, rot_fixes, scale_fixes, eg, n
            );
        }
    }
}

impl From<Transform> for TransformData {
    fn from(transform: Transform) -> Self {
        Self {
            position: transform.translation.to_array(),
            rotation: [
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
                transform.rotation.w,
            ],
            scale: transform.scale.to_array(),
        }
    }
}

/// Instance-specific properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceProperties {
    /// RGBA color (0.0-1.0 floats internally).
    /// TOML accepts both 0-255 integer arrays `[163, 162, 165]` (RGB)
    /// and legacy 0.0-1.0 float arrays `[0.5, 0.5, 0.5, 1.0]` (RGBA).
    #[serde(default = "default_color", deserialize_with = "deserialize_color_flexible", serialize_with = "serialize_color_as_u8")]
    pub color: [f32; 4], // RGBA
    #[serde(default)]
    pub transparency: f32,
    #[serde(default)]
    pub anchored: bool,
    #[serde(default = "default_true")]
    pub can_collide: bool,
    #[serde(default = "default_true")]
    pub cast_shadow: bool,
    #[serde(default)]
    pub reflectance: f32,
    /// Material name — resolved from MaterialRegistry first, then Material enum fallback
    #[serde(default = "default_material_name_plastic")]
    pub material: String,
    /// When true, the entity cannot be selected via 3D click (e.g. Baseplate)
    #[serde(default)]
    pub locked: bool,
    /// Gap 5 — opt-in: keep the mesh's embedded glTF materials instead of
    /// applying the single engine `StandardMaterial`. Surfaced to
    /// `BasePart.respect_gltf_materials`; default false → unchanged behaviour.
    #[serde(default)]
    pub respect_gltf_materials: bool,
    /// Roblox `PhysicalProperties` decomposition written by the importer
    /// under `[properties.physics]`. Optional — absent for hand-authored
    /// parts. When present, the collider-insert path attaches the
    /// matching Avian `Friction` / `Restitution` / `ColliderDensity`
    /// components so imported parts bounce / slide / weigh correctly.
    #[serde(default)]
    pub physics: Option<PhysicsProperties>,
}

/// Typed view of the importer's `[properties.physics]` table (Roblox
/// `PhysicalProperties::Custom` decomposition). Every field is optional
/// so the section round-trips even when only a subset is present. The
/// key names mirror what `roblox-import::property_map` emits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PhysicsProperties {
    /// Mass density (Avian `ColliderDensity`).
    #[serde(default)]
    pub density: Option<f32>,
    /// Static friction coefficient (Avian `Friction::static_coefficient`).
    #[serde(default)]
    pub friction_static: Option<f32>,
    /// Kinetic/dynamic friction coefficient (Avian `Friction::dynamic_coefficient`).
    #[serde(default)]
    pub friction_kinetic: Option<f32>,
    /// Bounciness (Avian `Restitution`).
    #[serde(default)]
    pub restitution: Option<f32>,
    /// Roblox friction/elasticity blend weights — preserved for round-trip,
    /// no Avian cognate today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friction_weight: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elasticity_weight: Option<f32>,
    /// Importer preset marker (e.g. "Default") — round-trip only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

fn default_material_name_plastic() -> String {
    "Plastic".to_string()
}

fn default_color() -> [f32; 4] {
    // Default: medium gray [163, 162, 165] in 0-255 → 0.0-1.0
    [163.0 / 255.0, 162.0 / 255.0, 165.0 / 255.0, 1.0]
}

/// Custom deserializer that accepts both 0-255 integer RGB/RGBA and 0.0-1.0 float RGBA arrays.
/// - `[163, 162, 165]`     → RGB integers, alpha defaults to 1.0
/// - `[163, 162, 165, 200]` → RGBA integers
/// - `[0.639, 0.635, 0.647, 1.0]` → legacy RGBA floats (values ≤ 1.0)
/// Detection heuristic: if ALL values are integers, treat as 0-255. Otherwise treat as floats.
fn deserialize_color_flexible<'de, D>(deserializer: D) -> Result<[f32; 4], D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values: Vec<toml::Value> = serde::Deserialize::deserialize(deserializer)?;

    if values.len() < 3 {
        return Err(serde::de::Error::custom(
            "color array must have at least 3 elements (RGB)",
        ));
    }

    // Check if all values are integers (0-255 format)
    let all_integers = values.iter().all(|v| v.is_integer());

    if all_integers {
        // 0-255 integer format
        let r = values[0].as_integer().unwrap_or(128) as f32 / 255.0;
        let g = values[1].as_integer().unwrap_or(128) as f32 / 255.0;
        let b = values[2].as_integer().unwrap_or(128) as f32 / 255.0;
        let a = if values.len() >= 4 {
            values[3].as_integer().unwrap_or(255) as f32 / 255.0
        } else {
            1.0
        };
        Ok([r, g, b, a])
    } else {
        // 0.0-1.0 float format (legacy)
        let r = values[0].as_float().or_else(|| values[0].as_integer().map(|i| i as f64)).unwrap_or(0.5) as f32;
        let g = values[1].as_float().or_else(|| values[1].as_integer().map(|i| i as f64)).unwrap_or(0.5) as f32;
        let b = values[2].as_float().or_else(|| values[2].as_integer().map(|i| i as f64)).unwrap_or(0.5) as f32;
        let a = if values.len() >= 4 {
            values[3].as_float().or_else(|| values[3].as_integer().map(|i| i as f64)).unwrap_or(1.0) as f32
        } else {
            1.0
        };
        Ok([r, g, b, a])
    }
}

/// Custom serializer that writes color as 0-255 RGB integer array.
/// If alpha is not 1.0 (fully opaque), writes RGBA; otherwise just RGB.
fn serialize_color_as_u8<S>(color: &[f32; 4], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let r = (color[0] * 255.0).round() as u8;
    let g = (color[1] * 255.0).round() as u8;
    let b = (color[2] * 255.0).round() as u8;
    let a = (color[3] * 255.0).round() as u8;
    if a == 255 {
        // Opaque — write compact RGB
        let mut seq = serializer.serialize_seq(Some(3))?;
        seq.serialize_element(&r)?;
        seq.serialize_element(&g)?;
        seq.serialize_element(&b)?;
        seq.end()
    } else {
        // Semi-transparent — write RGBA
        let mut seq = serializer.serialize_seq(Some(4))?;
        seq.serialize_element(&r)?;
        seq.serialize_element(&g)?;
        seq.serialize_element(&b)?;
        seq.serialize_element(&a)?;
        seq.end()
    }
}

fn default_true() -> bool {
    true
}

impl Default for InstanceProperties {
    fn default() -> Self {
        Self {
            color: default_color(),
            transparency: 0.0,
            anchored: false,
            can_collide: true,
            cast_shadow: true,
            reflectance: 0.0,
            material: default_material_name_plastic(),
            locked: false,
            physics: None,
            respect_gltf_materials: false,
        }
    }
}

/// Signed attribution for a create or modify event. Lightweight by design —
/// we keep every signature (no cap, no consolidation) because the modification
/// history doubles as AI training data: the system learns "who is capable of
/// what kinds of changes" by reading the full stamp chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreatorStamp {
    /// Display name at time of edit (AuthUser.username; "anonymous" if offline).
    pub name: String,
    /// Stable identity (AuthUser.id today; upgrade to full public key later).
    pub public_key: String,
    /// RFC 3339 timestamp of the edit.
    pub timestamp: String,
}

/// Instance metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceMetadata {
    #[serde(default = "default_class_name")]
    pub class_name: String,
    #[serde(default = "default_true")]
    pub archivable: bool,
    /// Display name override. When present, used instead of filename-derived name.
    /// Allows multiple instances with the same display name but unique filenames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub last_modified: String,
    /// Original creator — stamped once on first write by a logged-in user.
    /// Absent for entities created offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<CreatorStamp>,
    /// Append-only record of every signed modification. Never capped — the full
    /// chain is kept as training signal for Bliss attribution + AI learning.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifications: Vec<CreatorStamp>,
    /// Authoring unit for this instance's dimensional values
    /// (`[transform].position`, `[transform].scale`, any `[gui]`
    /// `*_offset`, `max_distance`, …). The disk symbol comes from
    /// [`eustress_common::units::Unit::symbol`] (`"m"`, `"cm"`,
    /// `"mm"`, `"ft"`, `"in"`, `"studs"`). Missing field → engine
    /// defaults to [`eustress_common::units::Unit::Meter`].
    ///
    /// Stored as `Option<String>` rather than the typed `Unit` so an
    /// unknown unit symbol on disk doesn't fail the whole instance
    /// load — the deserializer keeps the raw string, the spawn path
    /// parses it via `Unit::from_symbol` and falls back to the
    /// engine-native default with a warn! if it can't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Stable UUID — 32 lowercase hex chars derived from blake3(seed)[..16].
    /// Wave 2.1 (IDENTITY.md §7.1). `Option<String>` so a TOML without the
    /// field deserializes cleanly. `skip_serializing_if = "Option::is_none"`
    /// keeps newly-emitted TOMLs that somehow lose the field free of an
    /// empty `uuid = ""` line. The migration always sets it to `Some`, so
    /// the skip clause is purely defensive against round-trip code paths
    /// that drop the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

fn default_class_name() -> String {
    "Part".to_string()
}

impl Default for InstanceMetadata {
    fn default() -> Self {
        Self {
            class_name: default_class_name(),
            archivable: true,
            name: None,
            created: String::new(),
            last_modified: String::new(),
            created_by: None,
            modifications: Vec::new(),
            unit: None,
            uuid: None,
        }
    }
}

// ============================================================================
// TOML-serializable realism property structs
// ============================================================================

/// Material properties as they appear in .glb.toml [material] section
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TomlMaterialProperties {
    #[serde(default = "default_material_name")]
    pub name: String,
    #[serde(default)]
    pub young_modulus: f32,
    #[serde(default)]
    pub poisson_ratio: f32,
    #[serde(default)]
    pub yield_strength: f32,
    #[serde(default)]
    pub ultimate_strength: f32,
    #[serde(default)]
    pub fracture_toughness: f32,
    #[serde(default)]
    pub hardness: f32,
    #[serde(default)]
    pub thermal_conductivity: f32,
    #[serde(default)]
    pub specific_heat: f32,
    #[serde(default)]
    pub thermal_expansion: f32,
    #[serde(default)]
    pub melting_point: f32,
    #[serde(default)]
    pub density: f32,
    #[serde(default)]
    pub friction_static: f32,
    #[serde(default)]
    pub friction_kinetic: f32,
    #[serde(default)]
    pub restitution: f32,
    /// Domain-specific extensions (porosity, electrical_conductivity, role, etc.)
    /// Accepts both numeric and string values from TOML; only f64 values
    /// are forwarded to the realism MaterialProperties component.
    #[serde(default)]
    pub custom: HashMap<String, toml::Value>,
}

fn default_material_name() -> String {
    "Steel".to_string()
}

impl TomlMaterialProperties {
    /// Convert to realism MaterialProperties component
    pub fn to_component(&self) -> eustress_common::realism::materials::prelude::MaterialProperties {
        eustress_common::realism::materials::prelude::MaterialProperties {
            name: self.name.clone(),
            young_modulus: self.young_modulus,
            poisson_ratio: self.poisson_ratio,
            yield_strength: self.yield_strength,
            ultimate_strength: self.ultimate_strength,
            fracture_toughness: self.fracture_toughness,
            hardness: self.hardness,
            thermal_conductivity: self.thermal_conductivity,
            specific_heat: self.specific_heat,
            thermal_expansion: self.thermal_expansion,
            melting_point: self.melting_point,
            density: self.density,
            friction_static: self.friction_static,
            friction_kinetic: self.friction_kinetic,
            restitution: self.restitution,
            custom_properties: self.custom.iter()
                .filter_map(|(k, v)| match v {
                    toml::Value::Float(f) => Some((k.clone(), *f)),
                    toml::Value::Integer(i) => Some((k.clone(), *i as f64)),
                    _ => None, // skip strings, bools, etc.
                })
                .collect(),
        }
    }
}

/// Thermodynamic state as it appears in .glb.toml [thermodynamic] section
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TomlThermodynamicState {
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_pressure")]
    pub pressure: f32,
    #[serde(default)]
    pub volume: f32,
    #[serde(default)]
    pub internal_energy: f32,
    #[serde(default)]
    pub entropy: f32,
    #[serde(default)]
    pub enthalpy: f32,
    #[serde(default = "default_one")]
    pub moles: f32,
}

fn default_temperature() -> f32 { 298.15 }
fn default_pressure() -> f32 { 101_325.0 }
fn default_one() -> f32 { 1.0 }

impl TomlThermodynamicState {
    /// Convert to realism ThermodynamicState component
    pub fn to_component(&self) -> eustress_common::realism::particles::prelude::ThermodynamicState {
        eustress_common::realism::particles::prelude::ThermodynamicState {
            temperature: self.temperature,
            pressure: self.pressure,
            volume: self.volume,
            internal_energy: self.internal_energy,
            entropy: self.entropy,
            enthalpy: self.enthalpy,
            moles: self.moles,
        }
    }
}

// ============================================================================
// UI class properties — covers TextLabel, TextButton, Frame, ImageLabel,
// TextBox, ScrollingFrame. Stored under [ui] in the .glb.toml file.
// ============================================================================

/// Universal UI-element properties stored under [ui] in the instance TOML.
/// All UI classes share layout/appearance fields; class-specific fields use
/// serde(default) so missing keys are silently zero/false.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiInstanceProperties {
    // ---- Text (TextLabel / TextButton / TextBox) ----
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub rich_text: bool,
    #[serde(default)]
    pub text_scaled: bool,
    #[serde(default)]
    pub text_wrapped: bool,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub line_height: f32,
    #[serde(default = "default_font")]
    pub font: String,
    #[serde(default)]
    pub text_color3: [f32; 3],
    #[serde(default)]
    pub text_transparency: f32,
    #[serde(default)]
    pub text_stroke_color3: [f32; 3],
    #[serde(default = "default_one")]
    pub text_stroke_transparency: f32,
    #[serde(default = "default_text_x_alignment")]
    pub text_x_alignment: String,   // "Left" | "Center" | "Right"
    #[serde(default = "default_text_y_alignment")]
    pub text_y_alignment: String,   // "Top" | "Center" | "Bottom"
    // ---- Appearance (all UI elements) ----
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_white")]
    pub background_color3: [f32; 3],
    #[serde(default)]
    pub background_transparency: f32,
    #[serde(default)]
    pub border_color3: [f32; 3],
    #[serde(default)]
    pub border_size_pixel: i32,
    #[serde(default = "default_border_mode")]
    pub border_mode: String,        // "Outline" | "Middle" | "Inset"
    #[serde(default)]
    pub clips_descendants: bool,
    #[serde(default = "default_one_i32")]
    pub z_index: i32,
    #[serde(default)]
    pub layout_order: i32,
    #[serde(default)]
    pub rotation: f32,
    // ---- Layout — strict UDim2 ([scale_x, offset_x, scale_y, offset_y]) ----
    #[serde(default)]
    pub anchor_point: [f32; 2],
    #[serde(default)]
    pub position: eustress_common::ui_types::UDim2,
    #[serde(default = "default_size_udim2")]
    pub size: eustress_common::ui_types::UDim2,
    // ---- Behavior ----
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default = "default_true")]
    pub auto_button_color: bool,
    // ---- Image (ImageLabel / ImageButton) ----
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub image_color3: [f32; 3],
    #[serde(default)]
    pub image_transparency: f32,
    #[serde(default = "default_scale_type")]
    pub scale_type: String,         // "Stretch" | "Slice" | "Tile" | "Fit" | "Crop"
    // ---- ScrollingFrame ----
    #[serde(default = "default_true")]
    pub scrolling_enabled: bool,
    #[serde(default)]
    pub scroll_bar_thickness: i32,
    // ---- AutomaticSize ----
    #[serde(default = "default_automatic_size")]
    pub automatic_size: String,     // "None" | "X" | "Y" | "XY"
}

fn default_font_size() -> f32 { 14.0 }
fn default_font() -> String { "SourceSans".to_string() }
fn default_text_x_alignment() -> String { "Center".to_string() }
fn default_text_y_alignment() -> String { "Center".to_string() }
fn default_white() -> [f32; 3] { [1.0, 1.0, 1.0] }
fn default_one_i32() -> i32 { 1 }
fn default_border_mode() -> String { "Outline".to_string() }
fn default_scale_type() -> String { "Stretch".to_string() }
fn default_automatic_size() -> String { "None".to_string() }
fn default_size_udim2() -> eustress_common::ui_types::UDim2 {
    eustress_common::ui_types::UDim2::from_pixels(100.0, 100.0)
}

impl Default for UiInstanceProperties {
    fn default() -> Self {
        Self {
            text: String::new(),
            rich_text: false,
            text_scaled: false,
            text_wrapped: false,
            font_size: default_font_size(),
            line_height: 0.0,
            font: default_font(),
            text_color3: [0.0, 0.0, 0.0],
            text_transparency: 0.0,
            text_stroke_color3: [0.0, 0.0, 0.0],
            text_stroke_transparency: 1.0,
            text_x_alignment: default_text_x_alignment(),
            text_y_alignment: default_text_y_alignment(),
            visible: true,
            background_color3: default_white(),
            background_transparency: 0.0,
            border_color3: [0.0, 0.0, 0.0],
            border_size_pixel: 0,
            border_mode: default_border_mode(),
            clips_descendants: false,
            z_index: 1,
            layout_order: 0,
            rotation: 0.0,
            anchor_point: [0.0, 0.0],
            position: eustress_common::ui_types::UDim2::default(),
            size: default_size_udim2(),
            active: true,
            auto_button_color: true,
            image: String::new(),
            image_color3: [1.0, 1.0, 1.0],
            image_transparency: 0.0,
            scale_type: default_scale_type(),
            scrolling_enabled: true,
            scroll_bar_thickness: 12,
            automatic_size: default_automatic_size(),
        }
    }
}

impl UiInstanceProperties {
    /// Convert the stored font string to the ECS Font enum
    fn to_font(&self) -> eustress_common::classes::Font {
        use eustress_common::classes::Font;
        match self.font.as_str() {
            "RobotoMono"  => Font::RobotoMono,
            "GothamBold"  => Font::GothamBold,
            "GothamLight" => Font::GothamLight,
            "Fantasy"     => Font::Fantasy,
            "Bangers"     => Font::Bangers,
            "Merriweather"=> Font::Merriweather,
            "Nunito"      => Font::Nunito,
            "Ubuntu"      => Font::Ubuntu,
            _             => Font::SourceSans,
        }
    }
    fn to_x_align(&self) -> eustress_common::classes::TextXAlignment {
        use eustress_common::classes::TextXAlignment;
        match self.text_x_alignment.as_str() {
            "Left"  => TextXAlignment::Left,
            "Right" => TextXAlignment::Right,
            _       => TextXAlignment::Center,
        }
    }
    fn to_y_align(&self) -> eustress_common::classes::TextYAlignment {
        use eustress_common::classes::TextYAlignment;
        match self.text_y_alignment.as_str() {
            "Top"    => TextYAlignment::Top,
            "Bottom" => TextYAlignment::Bottom,
            _        => TextYAlignment::Center,
        }
    }
    fn to_auto_size(&self) -> eustress_common::classes::AutomaticSize {
        use eustress_common::classes::AutomaticSize;
        match self.automatic_size.as_str() {
            "X"  => AutomaticSize::X,
            "Y"  => AutomaticSize::Y,
            "XY" => AutomaticSize::XY,
            _    => AutomaticSize::None,
        }
    }
    fn to_border_mode(&self) -> eustress_common::classes::BorderMode {
        use eustress_common::classes::BorderMode;
        match self.border_mode.as_str() {
            "Middle" => BorderMode::Middle,
            "Inset"  => BorderMode::Inset,
            _        => BorderMode::Outline,
        }
    }
}

/// Electrochemical state as it appears in .glb.toml [electrochemical] section
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TomlElectrochemicalState {
    #[serde(default = "default_voltage")]
    pub voltage: f32,
    #[serde(default = "default_voltage")]
    pub terminal_voltage: f32,
    #[serde(default)]
    pub capacity_ah: f32,
    #[serde(default = "default_one")]
    pub soc: f32,
    #[serde(default)]
    pub current: f32,
    #[serde(default)]
    pub internal_resistance: f32,
    #[serde(default)]
    pub ionic_conductivity: f32,
    #[serde(default)]
    pub cycle_count: u32,
    #[serde(default)]
    pub c_rate: f32,
    #[serde(default = "default_one")]
    pub capacity_retention: f32,
    #[serde(default)]
    pub heat_generation: f32,
    #[serde(default)]
    pub dendrite_risk: f32,
}

fn default_voltage() -> f32 { 2.23 }

impl TomlElectrochemicalState {
    /// Convert to realism ElectrochemicalState component
    pub fn to_component(&self) -> eustress_common::realism::particles::prelude::ElectrochemicalState {
        eustress_common::realism::particles::prelude::ElectrochemicalState {
            voltage: self.voltage,
            terminal_voltage: self.terminal_voltage,
            capacity_ah: self.capacity_ah,
            soc: self.soc,
            current: self.current,
            internal_resistance: self.internal_resistance,
            ionic_conductivity: self.ionic_conductivity,
            cycle_count: self.cycle_count,
            c_rate: self.c_rate,
            capacity_retention: self.capacity_retention,
            heat_generation: self.heat_generation,
            dendrite_risk: self.dendrite_risk,
        }
    }
}

/// Nuclear reactor state as it appears in the [nuclear] TOML section.
///
/// All fields default to nominal ARC-1 operating conditions so a minimal
/// `[nuclear]` section (or no section at all) still produces a valid reactor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlNuclearState {
    /// Initial neutron population (normalised; 1.0 = steady-state critical).
    #[serde(default = "default_one")]
    pub neutron_population: f32,
    /// Initial core temperature [°C].
    #[serde(default = "default_core_temp")]
    pub core_temp_celsius: f32,
    /// Initial coolant flow rate [%].
    #[serde(default = "default_coolant_flow")]
    pub coolant_flow_pct: f32,
    /// Initial V-Cell state of charge [%].
    #[serde(default = "default_soc")]
    pub battery_soc_pct: f32,
    /// Initial electrical load demand [W].
    #[serde(default = "default_load_demand")]
    pub load_demand_watts: f32,
    /// Rod bank A insertion [0–100 %].
    #[serde(default = "default_rod_insertion")]
    pub rod_bank_a_pct: f32,
    /// Rod bank B insertion [0–100 %].
    #[serde(default = "default_rod_insertion")]
    pub rod_bank_b_pct: f32,
    /// Thermoelectric efficiency [fraction].
    #[serde(default = "default_te_eff")]
    pub te_efficiency: f32,
    /// Stirling engine efficiency [fraction].
    #[serde(default = "default_stirling_eff")]
    pub stirling_efficiency: f32,
    /// Whether the AI PID controller starts in Regulation mode.
    #[serde(default = "default_true_val")]
    pub ai_regulation_enabled: bool,
}

fn default_core_temp()     -> f32 { 847.0  }
fn default_coolant_flow()  -> f32 { 70.0   }
fn default_soc()           -> f32 { 82.0   }
fn default_load_demand()   -> f32 { 280.0  }
fn default_rod_insertion() -> f32 { 50.0   }
fn default_te_eff()        -> f32 { 0.14   }
fn default_stirling_eff()  -> f32 { 0.28   }
fn default_true_val()      -> bool { true  }

impl Default for TomlNuclearState {
    fn default() -> Self {
        Self {
            neutron_population: 1.0,
            core_temp_celsius: default_core_temp(),
            coolant_flow_pct: default_coolant_flow(),
            battery_soc_pct: default_soc(),
            load_demand_watts: default_load_demand(),
            rod_bank_a_pct: default_rod_insertion(),
            rod_bank_b_pct: default_rod_insertion(),
            te_efficiency: default_te_eff(),
            stirling_efficiency: default_stirling_eff(),
            ai_regulation_enabled: true,
        }
    }
}

/// Plasma state as it appears in the [plasma] TOML section. Attaches a
/// `PlasmaState` component to any class — same model as [thermodynamic].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlPlasmaState {
    #[serde(default = "default_plasma_density")]
    pub electron_density: f32,
    #[serde(default = "default_plasma_density")]
    pub ion_density: f32,
    #[serde(default = "default_plasma_temp")]
    pub electron_temperature_k: f32,
    #[serde(default = "default_plasma_temp")]
    pub ion_temperature_k: f32,
    #[serde(default = "default_one")]
    pub ionization_degree: f32,
    #[serde(default = "default_one")]
    pub magnetic_field: f32,
}

fn default_plasma_density() -> f32 { 1.0e19 }
fn default_plasma_temp()    -> f32 { 1.0e7 }

impl Default for TomlPlasmaState {
    fn default() -> Self {
        Self {
            electron_density: default_plasma_density(),
            ion_density: default_plasma_density(),
            electron_temperature_k: default_plasma_temp(),
            ion_temperature_k: default_plasma_temp(),
            ionization_degree: 1.0,
            magnetic_field: 1.0,
        }
    }
}

impl TomlPlasmaState {
    /// Convert to the realism `PlasmaState` ECS component.
    pub fn to_component(&self) -> eustress_common::realism::plasma::components::PlasmaState {
        eustress_common::realism::plasma::components::PlasmaState {
            electron_density: self.electron_density,
            ion_density: self.ion_density,
            electron_temperature_k: self.electron_temperature_k,
            ion_temperature_k: self.ion_temperature_k,
            ionization_degree: self.ionization_degree,
            magnetic_field: self.magnetic_field,
        }
    }
}

impl TomlNuclearState {
    /// Convert to the `NuclearInit` carrier component that `FissionPlugin`
    /// applies over the ArcReactorCore default components during hydration.
    pub fn to_init(&self) -> eustress_common::realism::nuclear::components::NuclearInit {
        eustress_common::realism::nuclear::components::NuclearInit {
            neutron_population: self.neutron_population,
            core_temp_celsius: self.core_temp_celsius,
            coolant_flow_pct: self.coolant_flow_pct,
            battery_soc_pct: self.battery_soc_pct,
            load_demand_watts: self.load_demand_watts,
            rod_bank_a_pct: self.rod_bank_a_pct,
            rod_bank_b_pct: self.rod_bank_b_pct,
            te_efficiency: self.te_efficiency,
            stirling_efficiency: self.stirling_efficiency,
            ai_regulation_enabled: self.ai_regulation_enabled,
        }
    }
}

/// Component marking an entity as loaded from an instance file.
/// For folder-based instances: toml_path = folder/_instance.toml
/// For legacy flat files: toml_path = folder/Name.glb.toml
#[derive(Component, Debug, Clone)]
pub struct InstanceFile {
    /// Path to the instance TOML file (_instance.toml or .glb.toml)
    pub toml_path: PathBuf,
    /// Path to the referenced mesh asset
    pub mesh_path: PathBuf,
    /// Instance name (derived from filename)
    pub name: String,
}

/// Marker placed on custom-mesh Parts so a polling system can update their
/// `BasePart.size` once the mesh asset has finished loading. Without this,
/// custom-mesh parts keep the TOML's `transform.scale` value (typically
/// 1×1×1) as their collision + gizmo size — which is correct for unit
/// primitives but wrong for anything with real geometry. The marker is
/// removed after the size is applied so the system becomes a no-op.
#[derive(Component, Debug)]
pub struct NeedsMeshSize;

/// Load an instance definition from a `_instance.toml` / `.glb.toml` /
/// `.part.toml` file on disk, routed through the common-crate schema pipeline:
///
/// 1. Read the file.
/// 2. Parse to `toml::Value` and normalise every key to PascalCase
///    (legacy snake_case files are transparently accepted).
/// 3. Merge missing sections/fields from the `ClassName`'s template.
/// 4. Rewrite the on-disk TOML when the canonical form differs (self-heal).
/// 5. Deserialize the merged value into `InstanceDefinition`.
///
/// Returns a typed `InstanceDefinition` ready for spawn. Callers that need
/// the extras list (for `ExtraSectionRegistry` dispatch) should use
/// [`load_instance_definition_with_extras`] below.
pub fn load_instance_definition(toml_path: &Path) -> Result<InstanceDefinition, String> {
    // DB-first (the full conversion): a converted Space serves the
    // binary ECS record straight from Fjall — zero disk, no TOML
    // parse. This single redirect covers every edit/tool/hot-reload
    // call site (~25) because they all funnel through here. Falls
    // through to the disk TOML pipeline only for a legacy world that
    // has no active Fjall DB yet (un-converted), so existing disk
    // worlds keep working until `convert-to-eustress` migrates them.
    if let Some(def) = crate::space::active_db::get_instance(toml_path) {
        return Ok(def);
    }
    load_instance_definition_with_extras(toml_path).map(|(def, _extras)| def)
}

/// Load an instance + return the list of `[Section]` names that the class
/// template did NOT declare. These are candidates for
/// `ExtraSectionRegistry::dispatch` so simulation plugins can attach their
/// own components (Thermodynamic, Electrochemical, Material, …) off the
/// same TOML without needing base-class support.
pub fn load_instance_definition_with_extras(
    toml_path: &Path,
) -> Result<(InstanceDefinition, Vec<String>), String> {
    // Shared registry — cheap to construct (`Default::default()` builds it
    // from the embedded `include_str!` templates on first call per thread).
    // A long-lived version will be injected as a Bevy Resource once the
    // migration lands; using a local default keeps every legacy caller
    // working without a plumbing change.
    let registry = eustress_common::class_schema::ClassSchemaRegistry::from_builtin();
    let healed = eustress_common::class_schema::load_and_heal_instance(toml_path, &registry)
        .map_err(|e| format!("schema heal {}: {}", toml_path.display(), e))?;

    let instance: InstanceDefinition = healed
        .value
        .try_into()
        .map_err(|e: toml::de::Error| {
            format!(
                "deserialize merged {} ({}): {}",
                toml_path.display(),
                e.message(),
                e
            )
        })?;
    Ok((instance, healed.extras))
}

/// Legacy signature kept for one release so `file_loader` + `slint_ui`
/// callers don't all have to change at the same commit. The `_registry`
/// parameter is ignored — the embedded common-crate schema is the source
/// of truth now. Delete once every call site has migrated.
#[deprecated(
    note = "use `load_instance_definition` — the common-crate class schema \
            is the source of truth and is loaded automatically."
)]
pub fn load_instance_definition_with_defaults(
    toml_path: &Path,
    _registry: Option<&super::class_defaults::ClassDefaultsRegistry>,
) -> Result<InstanceDefinition, String> {
    load_instance_definition(toml_path)
}

/// Parse + heal an instance from TOML *content* (no disk read). The
/// WorldDb cold-load path uses this to materialise entities from
/// `INSTANCE_META` bytes stored in Fjall, reusing the exact same
/// schema-heal + template-merge pipeline as the path-based loader so
/// a Fjall-sourced entity is byte-for-byte equivalent to a
/// TOML-sourced one.
pub fn load_instance_definition_from_str(
    content: &str,
) -> Result<InstanceDefinition, String> {
    let registry = eustress_common::class_schema::ClassSchemaRegistry::from_builtin();
    let healed = eustress_common::class_schema::heal_instance_from_str(content, &registry)
        .map_err(|e| format!("schema heal (from str): {}", e))?;
    let instance: InstanceDefinition = healed
        .value
        .try_into()
        .map_err(|e: toml::de::Error| format!("deserialize merged (from str): {}", e))?;
    Ok(instance)
}

/// Spawn one entity from TOML content held in memory (Fjall cold-load
/// path). `synthetic_toml_path` is the path the entity *would* live at
/// on disk — used only for the display-name fallback and the
/// `InstanceFile` component so a later TOML write-back (when the
/// `toml` feature is on) targets the right location. Nothing is read
/// from that path.
pub fn spawn_instance_from_toml_str(
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    material_registry: &mut super::material_loader::MaterialRegistry,
    mesh_cache: &mut PrimitiveMeshCache,
    decal_materials: &mut Assets<ForwardDecalMaterial<StandardMaterial>>,
    synthetic_toml_path: PathBuf,
    content: &str,
) -> Result<Entity, String> {
    let instance = load_instance_definition_from_str(content)?;
    Ok(spawn_instance(
        commands,
        asset_server,
        materials,
        material_registry,
        mesh_cache,
        decal_materials,
        synthetic_toml_path,
        instance,
    ))
}

/// Persist an instance definition.
///
/// DB-first: a converted Space writes the binary ECS record into
/// Fjall — no disk, no TOML serialise (disk write speed is precisely
/// the bottleneck the pivot exists to remove). Disk-TOML write happens
/// ONLY when there is no active Fjall DB, i.e. a legacy un-converted
/// world that still persists as files until `convert-to-eustress`
/// migrates it — at which point this path stops writing disk entirely.
pub fn write_instance_definition(
    toml_path: &Path,
    instance: &InstanceDefinition,
) -> Result<(), String> {
    if crate::space::active_db::put_instance(toml_path, instance) {
        return Ok(());
    }

    let toml_str = toml::to_string_pretty(instance)
        .map_err(|e| format!("Failed to serialize instance: {}", e))?;

    // Atomic write + retry on Windows file-lock races (file watcher
    // reload pass, antivirus scanning, text-editor reads).
    super::gui_loader::write_atomic(toml_path, toml_str.as_bytes())
        .map_err(|e| format!("Failed to write {}: {}", toml_path.display(), e))?;

    Ok(())
}

// Naming helpers (entity_name_is_available, is_eep_reserved_name,
// unique_entity_name) live in `eustress_common::instance_create` so
// the in-process engine and the out-of-process MCP server share the
// same uniqueness rules — disk-state mutations never disagree.
//
// Entity names map to two disk shapes: folder-based
// (`BASE/_instance.toml`) and legacy flat (`BASE.toml`, `BASE.glb.toml`,
// `BASE.<ext>.toml`). The availability check rejects ANY collision —
// folder, flat file, or EEP-reserved name (`_instance.toml`, etc.) —
// inheriting the 2026-04-25 corruption fix where a folder named
// `_instance.toml/` produced a phantom Folder in the Explorer.
pub use eustress_common::instance_create::{
    entity_name_is_available, is_eep_reserved_name, unique_entity_name,
};

/// Return a [`CreatorStamp`] for the currently-authenticated user, or `None`
/// if the user is offline / not logged in. Offline edits stay unsigned so the
/// Bliss-eligible audit trail only records provable identities.
pub fn current_stamp(auth: &crate::auth::AuthState) -> Option<CreatorStamp> {
    use crate::auth::AuthStatus;
    if auth.status != AuthStatus::LoggedIn { return None; }
    let user = auth.user.as_ref()?;
    Some(CreatorStamp {
        name: user.username.clone(),
        public_key: user.id.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Write an instance definition, stamping the modification audit trail.
///
/// Behaviour:
/// - If `stamp` is `Some` and `metadata.created_by` is `None`, sets `created_by`.
/// - If `stamp` is `Some`, appends a new entry to `modifications`. Every signed
///   save is preserved — no cap, no consolidation — because the chain serves
///   both Bliss attribution and AI training signal.
/// - Updates `metadata.last_modified` to the stamp's timestamp (or "now" if
///   unsigned).
///
/// Offline (`stamp == None`) writes leave the audit chain untouched.
pub fn write_instance_definition_signed(
    toml_path: &Path,
    instance: &mut InstanceDefinition,
    stamp: Option<&CreatorStamp>,
) -> Result<(), String> {
    match stamp {
        Some(s) => {
            if instance.metadata.created_by.is_none() {
                instance.metadata.created_by = Some(s.clone());
            }
            instance.metadata.modifications.push(s.clone());
            instance.metadata.last_modified = s.timestamp.clone();
        }
        None => {
            instance.metadata.last_modified = chrono::Utc::now().to_rfc3339();
        }
    }
    write_instance_definition(toml_path, instance)
}

/// Convert a raw `toml::Value` (the `value` field extracted from a rich-schema
/// `{ type = "...", value = ..., description = "..." }` inline table) into an
/// `AttributeValue` suitable for storage in the ECS `Attributes` component.
fn rich_toml_value_to_attribute(v: &toml::Value) -> Option<eustress_common::AttributeValue> {
    match v {
        toml::Value::Boolean(b) => Some(eustress_common::AttributeValue::Bool(*b)),
        toml::Value::Integer(i) => Some(eustress_common::AttributeValue::Int(*i)),
        toml::Value::Float(f)   => Some(eustress_common::AttributeValue::Number(*f)),
        toml::Value::String(s)  => Some(eustress_common::AttributeValue::String(s.clone())),
        toml::Value::Array(arr) => {
            let floats: Vec<f64> = arr.iter().filter_map(|item| match item {
                toml::Value::Float(f)   => Some(*f),
                toml::Value::Integer(i) => Some(*i as f64),
                _ => None,
            }).collect();
            match floats.len() {
                2 => Some(eustress_common::AttributeValue::Vector2(
                    Vec2::new(floats[0] as f32, floats[1] as f32),
                )),
                3 => Some(eustress_common::AttributeValue::Vector3(
                    Vec3::new(floats[0] as f32, floats[1] as f32, floats[2] as f32),
                )),
                4 => Some(eustress_common::AttributeValue::Color(
                    Color::srgba(floats[0] as f32, floats[1] as f32, floats[2] as f32, floats[3] as f32),
                )),
                _ => None,
            }
        }
        // Tagged inline tables produced by the Roblox-import ValueObject fold
        // (Contract A). Each holds exactly one key naming the source type:
        //   { Color3 = [r,g,b] }                          → Color3
        //   { CFrame = [px,py,pz, qx,qy,qz,qw] }          → CFrame
        //   { BrickColor = N }                            → BrickColor
        // (Bare scalars / strings / [3]-arrays decode in the arms above;
        // ObjectValue folds to a bare UUID string → AttributeValue::String,
        // also handled above — GetAttribute returns that uuid for the
        // resolver.)
        toml::Value::Table(tbl) => {
            // Helper: pull an N-float array out of a tagged-table value.
            let floats_of = |val: &toml::Value| -> Vec<f64> {
                match val {
                    toml::Value::Array(a) => a
                        .iter()
                        .filter_map(|item| match item {
                            toml::Value::Float(f) => Some(*f),
                            toml::Value::Integer(i) => Some(*i as f64),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                }
            };

            if let Some(c) = tbl.get("Color3") {
                let f = floats_of(c);
                if f.len() == 3 {
                    return Some(eustress_common::AttributeValue::Color3(Color::srgb(
                        f[0] as f32,
                        f[1] as f32,
                        f[2] as f32,
                    )));
                }
                return None;
            }
            if let Some(cf) = tbl.get("CFrame") {
                let f = floats_of(cf);
                if f.len() == 7 {
                    return Some(eustress_common::AttributeValue::CFrame(Transform {
                        translation: Vec3::new(f[0] as f32, f[1] as f32, f[2] as f32),
                        rotation: Quat::from_xyzw(
                            f[3] as f32,
                            f[4] as f32,
                            f[5] as f32,
                            f[6] as f32,
                        ),
                        ..Default::default()
                    }));
                }
                return None;
            }
            if let Some(bc) = tbl.get("BrickColor") {
                if let toml::Value::Integer(n) = bc {
                    return Some(eustress_common::AttributeValue::BrickColor(*n as u32));
                }
                return None;
            }
            None
        }
        _ => None,
    }
}

/// Build an ECS `Attributes` component from the typed `[attributes]` TOML
/// table. Previously only the no-asset spawn branch parsed any attributes
/// (and only from rich-schema `extra` sections), so every Part loaded an
/// EMPTY `Attributes` component while the Properties panel read the
/// `[attributes]` table straight from disk. That split meant panel edits +
/// the `Changed<Attributes>` write-back operated on an empty component and
/// would silently clobber the on-disk `[attributes]` table the moment any
/// attribute changed. Routing every branch through this helper makes the
/// live component the faithful in-memory mirror of disk for ALL classes.
fn attributes_from_toml_table(
    table: Option<&std::collections::HashMap<String, toml::Value>>,
) -> Attributes {
    let mut attrs = Attributes::new();
    if let Some(map) = table {
        for (k, v) in map {
            if let Some(av) = rich_toml_value_to_attribute(v) {
                attrs.set(k, av);
            }
        }
    }
    attrs
}

/// Known primitive mesh filenames that map to engine asset parts
const PRIMITIVE_MESHES: &[(&str, &str, eustress_common::classes::PartType)] = &[
    ("block", "parts/block.glb", eustress_common::classes::PartType::Block),
    ("ball", "parts/ball.glb", eustress_common::classes::PartType::Ball),
    ("cylinder", "parts/cylinder.glb", eustress_common::classes::PartType::Cylinder),
    ("wedge", "parts/wedge.glb", eustress_common::classes::PartType::Wedge),
    ("corner_wedge", "parts/corner_wedge.glb", eustress_common::classes::PartType::CornerWedge),
    ("cone", "parts/cone.glb", eustress_common::classes::PartType::Cone),
];

/// Bevy system — once a custom-mesh Part's asset finishes loading, compute
/// the mesh AABB and set `BasePart.size` to the AABB dimensions. Removes
/// the `NeedsMeshSize` marker so the work happens exactly once per entity.
/// Works for any Part that references a custom `.glb` — V-Cell was the
/// visible symptom but this generalises.
pub fn update_base_part_size_from_mesh(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    mut query: Query<(
        Entity,
        &Mesh3d,
        &mut eustress_common::classes::BasePart,
        Option<&mut Transform>,
    ), With<NeedsMeshSize>>,
) {
    for (entity, mesh_handle, mut base_part, transform) in query.iter_mut() {
        let Some(mesh) = meshes.get(&mesh_handle.0) else { continue; };
        let Some(aabb) = mesh.compute_aabb() else { continue; };

        // Mesh AABB half-extents → full size. Scale from the existing
        // Transform is preserved so the user can still stretch a part
        // beyond its natural size if desired.
        let half = aabb.half_extents;
        let natural_size = Vec3::new(half.x * 2.0, half.y * 2.0, half.z * 2.0);
        let scale_factor = transform.as_ref().map(|t| t.scale).unwrap_or(Vec3::ONE);
        base_part.size = Vec3::new(
            natural_size.x * scale_factor.x,
            natural_size.y * scale_factor.y,
            natural_size.z * scale_factor.z,
        );

        commands.entity(entity).remove::<NeedsMeshSize>();
    }
}

/// Cache of loaded primitive mesh handles to avoid repeated asset_server.load()
/// calls for the same GLB path across thousands of entities.
/// Without this cache, 10K entities each call `asset_server.load("parts/block.glb#Mesh0/Primitive0")`
/// which involves string formatting + path resolution per entity.
#[derive(Resource, Default)]
pub struct PrimitiveMeshCache {
    /// GLB asset path → loaded mesh handle
    cache: HashMap<String, Handle<Mesh>>,
    /// Custom-mesh asset URL (full space://...#Mesh0/Primitive0) -> handle.
    /// Holds a STRONG handle so streaming evict never drops the last ref and
    /// the GPU slab is never freed/reallocated. Bounded by distinct-mesh count.
    custom_cache: HashMap<String, Handle<Mesh>>,
}

impl PrimitiveMeshCache {
    /// Get or load a primitive mesh handle, caching the result.
    pub fn get_or_load(
        &mut self,
        asset_server: &AssetServer,
        glb_path: &str,
    ) -> Handle<Mesh> {
        self.cache.entry(glb_path.to_string()).or_insert_with(|| {
            asset_server.load(format!("{}#Mesh0/Primitive0", glb_path))
        }).clone()
    }

    /// Get or load a CUSTOM mesh handle by its full asset URL, keeping a
    /// resident strong handle so streaming despawn never drops the last
    /// reference (which would free + force a reload/reallocate of the GPU slab
    /// on cell re-entry).
    pub fn get_or_load_custom(
        &mut self,
        asset_server: &AssetServer,
        asset_url: &str,
    ) -> Handle<Mesh> {
        self.custom_cache
            .entry(asset_url.to_string())
            .or_insert_with(|| asset_server.load(asset_url.to_string()))
            .clone()
    }
}

/// Spawn entity from instance definition, loading actual GLB meshes.
///
/// - **No asset** (`asset: None`): spawns a non-visual entity (Atmosphere, Sky, Moon, etc.)
/// - **Primitives** (block.glb, ball.glb, etc.): loaded from engine `assets/parts/`
/// - **Custom meshes** (V-Cell, user models): resolved relative to the .glb.toml
///   file's parent directory and loaded as a GLTF scene via AssetServer
///
/// Scale from [transform] sets the entity size via Transform.scale.
/// Live mirror of the **customizable Workspace `RenderDistance`
/// property** (`WorkspaceComponent.render_distance`, exposed via
/// `PropertyAccess`). Metres; integer precision is ample for a cull
/// radius and sidesteps any const-fn float-bits concern. Seeded to
/// `WorkspaceComponent::default().render_distance` (1000 — perf QW4b
/// had lowered it to 300/500 so large imports cull most parts for a
/// local camera; raised back to 1000 on 2026-06-10 with the size-aware
/// cull margin landing, and the user can change it in the Properties
/// panel). The Workspace-property apply path calls
/// [`set_workspace_render_distance`] so editing the property in the
/// Properties panel drives every part's `VisibilityRange`. NOT a
/// hardcoded constant — it is the Workspace property's value at runtime.
static WORKSPACE_RENDER_DISTANCE_M: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(1000);

/// Push the Workspace `RenderDistance` property value into the live
/// mirror. Call this wherever `WorkspaceComponent` is applied / when
/// the property is edited; newly-spawned parts use it immediately, and
/// a `Changed<WorkspaceComponent>` system can re-stamp existing parts'
/// `VisibilityRange` by calling [`part_visibility_range`].
pub fn set_workspace_render_distance(meters: f32) {
    let m = meters.clamp(1.0, 1_000_000.0) as u32;
    WORKSPACE_RENDER_DISTANCE_M.store(m, std::sync::atomic::Ordering::Relaxed);
}

/// Distance-cull component applied to every spawned part, driven by the
/// customizable Workspace `RenderDistance`. Zero-width margins == a
/// hard cut (no crossfade); `use_aabb: false` measures to the entity
/// origin (cheap; and Bevy's `use_aabb: true` is no better here — it
/// measures to the AABB *center*, which for a part IS the origin).
///
/// `half_extent` is the part's bounding-sphere radius (world metres,
/// `scale.length() / 2` for a unit-cube mesh scaled to size). It
/// extends the cull distance so LARGE parts cull by their nearest
/// extent, not their centre: a 512 m baseplate whose origin sits 600 m
/// away is still under the camera's feet, and an origin-only test was
/// blinking exactly such parts out ("base plate disappears too
/// quickly", 2026-06-10). Sphere-vs-sphere: visible while ANY point of
/// the part's bounding sphere is within `RenderDistance`. For ordinary
/// small parts (`half_extent` ≈ 1–3 m) this changes nothing.
///
/// HONEST SCOPE: a built-in, zero-rewrite frame-rate lever that wins
/// on large worlds / walk-throughs / the 2.1M case; it does NOT help a
/// camera centred inside a grid smaller than the render distance (the
/// 50k benchmark at default 5000 m) — that still needs
/// streaming-primary.
pub fn part_visibility_range(half_extent: f32) -> VisibilityRange {
    let far = WORKSPACE_RENDER_DISTANCE_M.load(std::sync::atomic::Ordering::Relaxed) as f32;
    let far = far + half_extent.max(0.0);
    VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: far..far,
        use_aabb: false,
    }
}

/// Bounding-sphere radius (half-extent) of a part from its world
/// `Transform.scale` — for unit-mesh parts, scale IS the world size,
/// so the bounding sphere of the scaled unit cube has radius
/// `|scale| / 2`. Non-finite scales (mid-load) clamp to zero.
pub fn part_half_extent(scale: Vec3) -> f32 {
    let r = scale.length() * 0.5;
    if r.is_finite() {
        r
    } else {
        0.0
    }
}

/// Live propagation of the customizable Workspace `render_distance`
/// property. Mirrors the proven `sync_service_properties_to_lighting`
/// precedent exactly: the Properties panel writes service edits into
/// the Workspace entity's `ServiceComponent.properties` map; this
/// `Changed<ServiceComponent>`-gated system reads `render_distance` for
/// the `Workspace` service, pushes it into the runtime mirror, and
/// re-stamps `VisibilityRange` on every already-spawned part so the
/// edit takes effect immediately. Changed-gated → does nothing on a
/// frame where no service property was edited (honours "nothing per
/// frame"); the one-time part re-stamp on a deliberate edit is fine.
pub fn sync_workspace_render_distance(
    mut commands: Commands,
    service_q: Query<
        &crate::space::service_loader::ServiceComponent,
        Changed<crate::space::service_loader::ServiceComponent>,
    >,
    parts_q: Query<(Entity, &Transform), With<eustress_common::classes::Part>>,
) {
    use crate::space::service_loader::PropertyValue;
    for svc in service_q.iter() {
        if svc.class_name != "Workspace" {
            continue;
        }
        if let Some(PropertyValue::Float(v)) = svc.properties.get("render_distance") {
            set_workspace_render_distance(*v as f32);
            for (e, transform) in parts_q.iter() {
                // Transform.scale = world size for unit-mesh parts, so
                // the re-stamp keeps each part's size-aware cull margin.
                commands
                    .entity(e)
                    .insert(part_visibility_range(part_half_extent(transform.scale)));
            }
        }
    }
}

pub fn spawn_instance(
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    material_registry: &mut super::material_loader::MaterialRegistry,
    mesh_cache: &mut PrimitiveMeshCache,
    decal_materials: &mut Assets<ForwardDecalMaterial<StandardMaterial>>,
    toml_path: PathBuf,
    instance: InstanceDefinition,
) -> Entity {
    // Instance display name: prefer metadata.name, fall back to folder/file name.
    // For folder-based instances (_instance.toml), use the parent folder name.
    // For legacy flat files (.glb.toml), use the filename stem.
    let name = instance.metadata.name.clone().unwrap_or_else(|| {
        let fname = toml_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if fname == "_instance.toml" {
            toml_path.parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string()
        } else {
            fname.split('.').next().unwrap_or("Unknown").to_string()
        }
    });

    // Parse class name early — needed for the no-mesh branch too
    let class_name = eustress_common::classes::ClassName::from_str(
        &instance.metadata.class_name
    ).unwrap_or(eustress_common::classes::ClassName::Part);

    // Authoring unit — drives the engine's per-entity unit awareness.
    // Missing or unknown symbol → engine-native default (Meter). A
    // warn! highlights typos like `unit = "metere"` so they get caught
    // instead of silently degrading. Stage 3 wires this into the
    // actual dimensional conversion at load; for now we just stamp
    // the component so downstream systems can read it.
    let measure_unit = match instance.metadata.unit.as_deref() {
        Some(sym) => match eustress_common::units::Unit::from_symbol(sym) {
            Some(u) => eustress_common::units::MeasureUnit(u),
            None => {
                warn!(
                    "Unknown unit symbol {:?} in {:?} — defaulting to {:?}",
                    sym, toml_path, eustress_common::units::ENGINE_NATIVE_UNIT,
                );
                eustress_common::units::MeasureUnit::default()
            }
        },
        None => eustress_common::units::MeasureUnit::default(),
    };

    // Tags → component. Populated from `instance.tags` in the TOML.
    // Previously the loader always inserted `Tags::new()` (empty),
    // silently dropping any tags the user authored — fixed
    // 2026-04-22. Declared here at function scope so all three spawn
    // branches (no-asset, custom-mesh, primitive) can `tags.clone()`
    // uniformly; the branches below each moved their own local copy
    // prior, which is why the custom-mesh branch later lost access
    // to it when the no-asset block scoped its `let` locally.
    //
    // Any `CollectionService`-style API calls at runtime (`AddTag` /
    // `RemoveTag` MCP tools) write back through the instance_loader's
    // signed-write path so disk stays canonical.
    let tags: Tags = match &instance.tags {
        Some(t) if !t.is_empty() => Tags(t.clone()),
        _ => Tags::new(),
    };

    // Attributes → component, from the typed `[attributes]` TOML table.
    // Declared at function scope so all three spawn branches (no-asset,
    // custom-mesh, primitive) seed the SAME live component from disk —
    // see `attributes_from_toml_table` for why an empty component on the
    // asset branches was a latent clobber bug.
    let base_attributes: Attributes = attributes_from_toml_table(instance.attributes.as_ref());

    // ── Part-class fallback: default to block primitive when no [asset] section ──
    // MCP tools and external IDEs create _instance.toml files with [transform]
    // + [properties] but no [asset]. Without this, Part entities hit the
    // non-visual branch and are invisible.
    let mut instance = instance;
    if instance.asset.is_none() && matches!(class_name, eustress_common::classes::ClassName::Part) {
        instance.asset = Some(AssetReference {
            mesh: "parts/block.glb".to_string(),
            scene: default_scene(),
        });
    }

    // ── Visual-only mesh adjustment (Roblox DataMesh fold) ────────────
    //
    // The Roblox importer folds legacy SpecialMesh / BlockMesh /
    // CylinderMesh children into the parent part and records their
    // `Scale` / `Offset` as TOP-LEVEL `mesh_scale` / `mesh_offset` TOML
    // keys (arrays of 3 floats). Top-level — not inside `[asset]` —
    // because `AssetReference`'s field set is frozen (struct-literal
    // construction across the engine), while `InstanceDefinition.extra`
    // (serde flatten) already captures unknown top-level keys on every
    // load path. Extract + REMOVE them here so they never leak into
    // attributes / `PendingExtraSections`; they affect ONLY the render
    // transform below — never `BasePart.size`, the collider inputs, or
    // the on-disk transform.
    fn take_vec3_extra(
        extra: &mut std::collections::HashMap<String, toml::Value>,
        key: &str,
    ) -> Option<Vec3> {
        let v = extra.remove(key)?;
        let arr = v.as_array()?;
        if arr.len() != 3 {
            return None;
        }
        let mut out = [0.0f32; 3];
        for (slot, item) in out.iter_mut().zip(arr.iter()) {
            *slot = item
                .as_float()
                .or_else(|| item.as_integer().map(|n| n as f64))? as f32;
        }
        Some(Vec3::from_array(out))
    }
    let mesh_visual_scale = take_vec3_extra(&mut instance.extra, "mesh_scale");
    let mesh_visual_offset = take_vec3_extra(&mut instance.extra, "mesh_offset");

    // ── Stage 3: authored-unit → meter conversion ──────────────────────
    //
    // When `units_v1` is on, every dimensional value on this instance is
    // converted from the file's authored unit to the engine's native
    // unit (meters) exactly once, at the load boundary. After this point
    // every consumer — `Transform`, `BasePart.size`, Avian colliders,
    // raycasts, gizmo math — speaks meters.
    //
    // Identity short-circuit: when the file already declares meters
    // (the common case while migrating) the conversion is a no-op even
    // with the feature on, so we pay zero cost for the dominant path.
    //
    // The flag is off by default during migration: existing files that
    // either declare `unit = "m"` or omit the field entirely continue to
    // load with the same bits they had pre-units_v1, regardless of
    // whether the build was compiled with the flag.
    #[cfg(feature = "units_v1")]
    {
        let from_unit = measure_unit.0;
        let to_unit = eustress_common::units::ENGINE_NATIVE_UNIT;
        if from_unit != to_unit {
            instance.transform.position = eustress_common::units::convert_vec3_f32(
                instance.transform.position, from_unit, to_unit,
            );
            instance.transform.scale = eustress_common::units::convert_vec3_f32(
                instance.transform.scale, from_unit, to_unit,
            );
            debug!(
                "📐 Converted {:?} from {} → m (pos={:?}, scale={:?})",
                toml_path, from_unit.symbol(),
                instance.transform.position, instance.transform.scale,
            );
        }
    }

    // ── No mesh: spawn a non-visual Instance entity (Atmosphere, Sky, Moon, Star, etc.) ──
    if instance.asset.is_none() {
        // Parse rich-schema sections: each entry in `extra` is either a flat value
        // OR a named section (Table) whose entries are { type, value, description } inline tables.
        // Both cases are stored in Attributes for the Properties panel to display.

        // Seed from the typed `[attributes]` table, then layer any
        // rich-schema `extra` sections on top.
        let mut attrs = base_attributes.clone();
        for (_section_name, section_val) in &instance.extra {
            // Each top-level entry under [extra] is a section table (e.g. [Appearance])
            if let toml::Value::Table(props) = section_val {
                for (prop_key, prop_val) in props {
                    // Rich schema: { type = "...", value = ..., description = "..." }
                    let raw_value = if let toml::Value::Table(inline) = prop_val {
                        inline.get("value").cloned().unwrap_or(prop_val.clone())
                    } else {
                        prop_val.clone()
                    };
                    let attr_val = rich_toml_value_to_attribute(&raw_value);
                    if let Some(av) = attr_val {
                        attrs.set(prop_key, av);
                    }
                }
            } else {
                // Flat value at section level
                if let Some(av) = rich_toml_value_to_attribute(section_val) {
                    attrs.set(_section_name, av);
                }
            }
        }

        let entity = commands.spawn((
            eustress_common::classes::Instance {
                name: name.clone(),
                class_name,
                archivable: instance.metadata.archivable,
                id: 0,
                ai: false,
                // Carry the stable UUID through so cross-references (joint
                // part/attachment refs, etc.) can resolve by identity.
                uuid: instance.metadata.uuid.clone().unwrap_or_default(),
            },
            Transform::from(instance.transform),
            Visibility::default(),
            tags.clone(),
            attrs,
            InstanceFile {
                toml_path: toml_path.clone(),
                mesh_path: PathBuf::new(),
                name: name.clone(),
            },
            Name::new(name.clone()),
        )).id();
        commands.entity(entity).insert(measure_unit);
        // Data-only VFX attach: ParticleEmitter/Beam carry no [asset] so they
        // land here. Attaches the typed component from [particle]/[beam] so
        // Properties + scripts see live data (renderers are still stubs).
        attach_vfx_component(&mut commands.entity(entity), class_name, &instance.extra);
        // DEBUG: per-entity; an INFO here is a log-I/O stall at scale.
        debug!("🌅 Spawned non-visual instance '{}' ({}) from {:?}", name, instance.metadata.class_name, toml_path);
        return entity;
    }

    // ── Has mesh: resolve and load GLB ────────────────────────────────────────
    let asset_ref = instance.asset.as_ref().unwrap();
    // Resolve the mesh path: check if it's a known primitive or a custom GLB
    let mesh_ref = asset_ref.mesh.to_lowercase();
    let primitive = PRIMITIVE_MESHES.iter().find(|(hint, _, _)| {
        let fname = mesh_ref.rsplit('/').next().unwrap_or(&mesh_ref);
        fname.contains(hint)
    });
    
    let (is_custom_mesh, part_shape) = if let Some((_, _, shape)) = primitive {
        (false, *shape)
    } else {
        // Custom mesh — default to Block shape for bounding-box purposes
        (true, eustress_common::classes::PartType::Block)
    };
    
    // Determine the absolute path for the GLB mesh file. We normalize the
    // result so `..` segments (common when a folder-based Part references
    // `../meshes/Foo.glb`) don't leak into the asset URL. Without this,
    // Bevy's `space://` reader treats the `..` literally on some platforms
    // and the mesh fails to load silently — V-Cell's sub-parts all use this
    // relative shape, so they were the visible symptom.
    //
    // We normalize manually instead of using `canonicalize()` because on
    // Windows canonicalize prepends the `\\?\` verbatim prefix, which would
    // then fail `strip_prefix(&space_root)` downstream.
    fn normalize_path(p: &Path) -> PathBuf {
        use std::path::Component;
        let mut out = PathBuf::new();
        for comp in p.components() {
            match comp {
                Component::ParentDir => { out.pop(); }
                Component::CurDir => {} // skip "."
                _ => out.push(comp.as_os_str()),
            }
        }
        out
    }
    let toml_dir = toml_path.parent().unwrap_or(Path::new("."));
    let absolute_mesh_path = normalize_path(&toml_dir.join(&asset_ref.mesh));
    
    debug!("🔍 Instance '{}': mesh_ref='{}', is_custom={}, absolute_path={:?}, exists={}",
        name, mesh_ref, is_custom_mesh, absolute_mesh_path, absolute_mesh_path.exists());
    
    // Build material from properties — registry-first, enum fallback
    let [r, g, b, a] = instance.properties.color;
    let transparency = instance.properties.transparency;
    let base_color = Color::srgba(r, g, b, a);
    let material_handle = super::material_loader::resolve_material(
        &instance.properties.material,
        material_registry,
        materials,
        base_color,
        transparency,
        instance.properties.reflectance,
    );
    
    // Sanitize everything on the Transform from TOML — a part saved
    // with a zero/negative/NaN dimension, a NaN translation, or a
    // non-normalized/NaN rotation would panic Avian's collider builder
    // on load (`assertion failed: b.min.cmple(b.max).all()` in avian3d's
    // `collision/collider/mod.rs:512`). Avian propagates NaN from any
    // transform field into the world-space AABB, so we have to clean
    // all three components — not just size.
    let raw_pos = Vec3::from_array(instance.transform.position);
    let raw_rot = {
        let r = instance.transform.rotation;
        Quat::from_xyzw(r[0], r[1], r[2], r[3])
    };
    let raw_scale = Vec3::from_array(instance.transform.scale);
    let pos = sanitize_pos(raw_pos);
    let rot = sanitize_rot(raw_rot);
    let scale = sanitize_size(raw_scale);

    // Overwrite the TOML-derived transform with sanitized values so
    // downstream consumers (cframe, transform, collider) all see the
    // same clean data.
    let mut safe_instance_transform = instance.transform.clone();
    safe_instance_transform.position = pos.to_array();
    safe_instance_transform.rotation = [rot.x, rot.y, rot.z, rot.w];
    safe_instance_transform.scale = scale.to_array();

    // Build BasePart so the Properties panel can read/display part properties
    let base_part = eustress_common::classes::BasePart {
        size: scale,
        color: Color::srgba(r, g, b, a),
        transparency,
        reflectance: instance.properties.reflectance,
        anchored: instance.properties.anchored,
        can_collide: instance.properties.can_collide,
        locked: instance.properties.locked,
        cast_shadow: instance.properties.cast_shadow,
        material: eustress_common::classes::Material::from_string(&instance.properties.material),
        material_name: instance.properties.material.clone(),
        cframe: Transform::from(safe_instance_transform.clone()),
        respect_gltf_materials: instance.properties.respect_gltf_materials,
        ..default()
    };

    let transform = Transform::from(safe_instance_transform);

    // Render-only transform: apply the folded DataMesh `mesh_scale` /
    // `mesh_offset` (offset rotated into the part's local space). The
    // unadjusted `transform` keeps feeding `safe_collider_from` and
    // `BasePart.cframe`, so physics + persisted state stay untouched —
    // this is purely what gets drawn.
    let render_transform = if mesh_visual_scale.is_some() || mesh_visual_offset.is_some() {
        let mut t = transform;
        if let Some(ms) = mesh_visual_scale {
            t.scale *= ms;
        }
        if let Some(mo) = mesh_visual_offset {
            t.translation += transform.rotation * mo;
        }
        t
    } else {
        transform
    };

    if is_custom_mesh && absolute_mesh_path.exists() {
        // Check for Draco compression before loading
        if super::draco_decoder::is_draco_compressed(&absolute_mesh_path) {
            super::draco_decoder::warn_draco_file(&absolute_mesh_path);
            // Fall through to primitive mesh rendering as fallback
        } else {
            // ── Custom GLB mesh: load the mesh directly (bypasses scene spawner) ──
            // Use the "space://" asset source which resolves against the LIVE
            // Space root. Strip the absolute mesh path against the SAME live
            // root the dynamic reader joins (`space_asset_root()`), so the
            // resulting `space://{relative}` URL is always consistent with what
            // the reader resolves. (Was `default_space_root()`, which re-reads
            // the on-disk last-space setting and goes stale on a runtime Space
            // switch → wrong folder → missing meshes / black screen.)
            let space_root = super::space_asset_source::space_asset_root();
        let relative_mesh_path = absolute_mesh_path
            .strip_prefix(&space_root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| absolute_mesh_path.to_string_lossy().replace('\\', "/"));
        
        // Load mesh and material directly instead of using SceneRoot (avoids unregistered type panic)
        let mesh_path = format!("space://{}#Mesh0/Primitive0", relative_mesh_path);
        let material_path = format!("space://{}#Material0/std", relative_mesh_path);
        debug!("🔧 Loading mesh from: {} (absolute: {:?}, space_root: {:?})", mesh_path, absolute_mesh_path, space_root);
        // PERF: pin the custom mesh handle in the resident cache so streaming
        // evict never drops the last strong ref (which would free the GPU slab
        // and force a reload/reallocate on cell re-entry). Same URL + rendering.
        let mesh_handle: Handle<Mesh> = mesh_cache.get_or_load_custom(asset_server, &mesh_path);
        let material_handle: Handle<StandardMaterial> = asset_server.load(material_path);
        
        // Spawn the core visual entity first (no physics — added conditionally below)
        let entity = commands.spawn((
            Mesh3d(mesh_handle),
            MeshMaterial3d(material_handle),
            render_transform,
            Visibility::default(),
            eustress_common::classes::Instance {
                name: name.clone(),
                class_name,
                archivable: instance.metadata.archivable,
                id: 0,
                ai: false,
                // Carry the stable UUID so constraints can resolve this
                // part as a joint body by identity.
                uuid: instance.metadata.uuid.clone().unwrap_or_default(),
            },
            base_part,
            eustress_common::classes::Part { shape: part_shape },
            PartEntity { part_id: String::new() }, // filled in below
            base_attributes.clone(),
            tags.clone(),
            InstanceFile {
                toml_path: toml_path.clone(),
                mesh_path: absolute_mesh_path.clone(),
                name: name.clone(),
            },
            Name::new(name.clone()),
            // `MeshSource` marks this as a file-system-first part so
            // the scale tool's `apply_size_to_entity` follows the
            // "Transform.scale = size" branch instead of regenerating
            // the mesh + pinning Transform.scale at ONE. Without this
            // insertion on the reload path, a resize → save round-
            // trip was collapsing TOML scale to [1, 1, 1] (user-
            // reported 2026-04-23: "sizes are not saving, reverting
            // to 1,1,1"). Stringified path mirrors what
            // `spawn::spawn_part_glb` stores on freshly-spawned parts
            // so both entry points produce identical components.
            crate::spawn::MeshSource::new(asset_ref.mesh.clone()),
            // Mark this entity so `update_base_part_size_from_mesh` computes
            // `BasePart.size` from the mesh AABB once the asset finishes
            // loading. Works for any custom-mesh part, not just V-Cell.
            NeedsMeshSize,
        )).id();
        let part_id = format!("{}v{}", entity.index(), entity.generation());
        let mut ec = commands.entity(entity);
        ec.insert(PartEntity { part_id });
        ec.insert(measure_unit);
        ec.insert(part_visibility_range(part_half_extent(scale)));

        // Only add physics collider when can_collide is true — avoids broadphase
        // overhead for thousands of static decorative parts.
        // GLB meshes are unit meshes ([-0.5, 0.5]), so Transform.scale = part size in studs.
        // Avian3D colliders take HALF-extents for cuboid and HALF-height for cylinder.
        //
        // Perf QW5 — same huge-scene gate as the primitive branch below: on a
        // "huge" Space (`streaming_active()` true) skip the Static body +
        // Collider for these anchored decorative custom-mesh parts so a 387K+
        // import doesn't flood Avian's broadphase. Reversible + scoped:
        // no-op (gate is false) for normal scenes and when `world-db` is off.
        let huge_scene = crate::space::active_db::streaming_active();
        if instance.properties.can_collide && !huge_scene {
            // Validate `render_transform` — the transform the ENTITY actually
            // carries (and thus what Avian reads via GlobalTransform), NOT the
            // unadjusted `transform`. `render_transform` folds in a possibly-
            // negative `mesh_visual_scale`; validating it here is what catches
            // the mirrored-mesh collider AABB panic.
            if let Some(collider) = safe_collider_from(part_shape, scale, &render_transform) {
                ec.insert((collider, RigidBody::Static));
                // Imported PhysicalProperties → Avian physics material.
                // Additive: no-op when `[properties.physics]` is absent.
                apply_physics_material(&mut ec, instance.properties.physics.as_ref());
            } else {
                warn!("Skipping collider for '{}' — non-finite/negative transform scale (size={:?} render_scale={:?})",
                    name, scale, render_transform.scale);
            }
        }

        // Attach realism components if present in TOML
        if let Some(ref mat) = instance.material {
            ec.insert(mat.to_component());
            debug!("  + MaterialProperties: {}", mat.name);
        }
        if let Some(ref thermo) = instance.thermodynamic {
            ec.insert(thermo.to_component());
            debug!("  + ThermodynamicState: T={:.1}K P={:.0}Pa", thermo.temperature, thermo.pressure);
        }
        if let Some(ref echem) = instance.electrochemical {
            ec.insert(echem.to_component());
            debug!("  + ElectrochemicalState: V={:.2}V SOC={:.1}%", echem.voltage, echem.soc * 100.0);
        }
        if let Some(ref nuc) = instance.nuclear {
            ec.insert(nuc.to_init());
            debug!("  + NuclearInit: T={:.0}°C load={:.0}W", nuc.core_temp_celsius, nuc.load_demand_watts);
        }
        if let Some(ref plasma) = instance.plasma {
            ec.insert(plasma.to_component());
            debug!("  + PlasmaState: ne={:.1e} Te={:.1e}K", plasma.electron_density, plasma.electron_temperature_k);
        }
        // Attach UI ECS component if this is a UI class
        attach_ui_component(&mut ec, class_name, instance.ui.as_ref());
        // End the EntityCommands borrow so the decal/mesh attach (which
        // needs `&mut commands`) can run on the bound `entity` id. MUST run
        // BEFORE the PendingExtraSections insert below: it removes the
        // consumed `decal`/`mesh` key from `instance.extra` so the section
        // is never double-dispatched.
        drop(ec);
        attach_decal_mesh_component(
            commands, entity, asset_server, decal_materials, class_name,
            &mut instance.extra, render_transform, &name,
        );
        // Extra sections — anything present in the TOML that
        // neither the base template nor `InstanceDefinition` typed
        // fields consumed. Landed as `PendingExtraSections` so the
        // common-crate `dispatch_pending_extras` system can hand
        // each section to whichever plugin registered a claim on
        // it. Unclaimed sections are preserved on disk via the
        // `extra` flatten field for future plugin pickup.
        if !instance.extra.is_empty() {
            commands.entity(entity).insert(eustress_common::class_schema::PendingExtraSections {
                sections: instance.extra.clone(),
            });
        }
        debug!("Spawned custom mesh '{}' ({}) from {:?}", name, instance.metadata.class_name, toml_path);
        return entity;
        }
    }
    
    // ── Loud missing-mesh fallback ──
    //
    // A custom-mesh part whose `.glb` is absent on disk silently rendered
    // as a block, which made broken imports / moved asset folders look
    // like an importer geometry bug. Warn ONCE per distinct mesh path
    // (a 10K-part import referencing one missing mesh must not emit 10K
    // lines) — subsequent hits stay at the existing debug! above.
    if is_custom_mesh && !absolute_mesh_path.exists() {
        use std::sync::{Mutex, OnceLock};
        static WARNED_MISSING_MESHES: OnceLock<Mutex<std::collections::HashSet<String>>> =
            OnceLock::new();
        let warned = WARNED_MISSING_MESHES
            .get_or_init(|| Mutex::new(std::collections::HashSet::new()));
        let key = absolute_mesh_path.to_string_lossy().to_string();
        let first_hit = warned.lock().map(|mut s| s.insert(key)).unwrap_or(false);
        if first_hit {
            warn!(
                "Custom mesh missing on disk — rendering block fallback: {:?} \
                 (first hit: instance '{}' from {:?}; further parts referencing \
                 this mesh fall back silently)",
                absolute_mesh_path, name, toml_path
            );
        }
    }

    // Fallback to primitive mesh (either Draco-compressed or no custom mesh)
    // ── Primitive mesh: load from engine assets/parts/ ──
    let glb_path = if let Some((_, asset_path, _)) = primitive {
        *asset_path
    } else {
        "parts/block.glb" // fallback
    };
    let mesh_handle: Handle<Mesh> = mesh_cache.get_or_load(asset_server, glb_path);
    
    // Spawn the core visual entity first (no physics — added conditionally below)
    let entity = commands.spawn((
        Mesh3d(mesh_handle),
        MeshMaterial3d(material_handle),
        render_transform,
        Visibility::default(),
        eustress_common::classes::Instance {
            name: name.clone(),
            class_name,
            archivable: instance.metadata.archivable,
            id: 0,
            ai: false,
            // Carry the stable UUID so constraints can resolve this part
            // as a joint body by identity.
            uuid: instance.metadata.uuid.clone().unwrap_or_default(),
        },
        base_part,
        eustress_common::classes::Part { shape: part_shape },
        PartEntity { part_id: String::new() }, // filled in below
        base_attributes.clone(),
        tags.clone(),
        InstanceFile {
            toml_path: toml_path.clone(),
            mesh_path: absolute_mesh_path,
            name: name.clone(),
        },
        Name::new(name.clone()),
        // `MeshSource` keeps the primitive-reload path aligned with
        // the custom-mesh path above: the scale tool resizes by
        // setting `Transform.scale` instead of regenerating the
        // mesh, so the save round-trip writes the real dimensions
        // back to TOML. See the detailed comment on the custom-mesh
        // branch for the exact bug this closes.
        crate::spawn::MeshSource::new(glb_path),
    )).id();
    let part_id = format!("{}v{}", entity.index(), entity.generation());
    let mut ec = commands.entity(entity);
    ec.insert(PartEntity { part_id });
    ec.insert(measure_unit);
    ec.insert(part_visibility_range(part_half_extent(scale)));

    // Only add physics collider when can_collide is true — avoids broadphase
    // overhead for thousands of static decorative parts.
    // Avian3D colliders take HALF-extents for cuboid and HALF-height for cylinder.
    //
    // Perf QW5 — huge-scene gate. Every part spawned here is anchored
    // (`RigidBody::Static`), i.e. decorative collision geometry. On a "huge"
    // Space (the residency boot-load flipped `streaming_active()` true for a
    // large binary-ECS / streamed place — e.g. a 387K-part Roblox import),
    // attaching a Static body + Collider to hundreds of thousands of parts
    // floods Avian's broadphase and the per-frame rigid-body transform walk
    // for no gameplay benefit, so we SKIP it. The gate is reversible and
    // scoped: `streaming_active()` is `false` for normal-sized scenes and
    // whenever the `world-db` feature is off, so non-huge worlds attach
    // colliders exactly as before (zero behavior change). Authored
    // PhysicalProperties are skipped along with the body they'd attach to.
    let huge_scene = crate::space::active_db::streaming_active();
    if instance.properties.can_collide && !huge_scene {
        // Validate `render_transform` (the entity's actual transform / what
        // Avian reads), not the unadjusted `transform` — see the custom-mesh
        // branch above for why a negative folded scale panics Avian at insert.
        if let Some(collider) = safe_collider_from(part_shape, scale, &render_transform) {
            ec.insert((collider, RigidBody::Static));
            // Imported PhysicalProperties → Avian physics material.
            // Additive: no-op when `[properties.physics]` is absent.
            apply_physics_material(&mut ec, instance.properties.physics.as_ref());
        } else {
            warn!("Skipping collider for '{}' — non-finite/negative transform scale (size={:?} render_scale={:?})",
                name, scale, render_transform.scale);
        }
    }

    // Material Flip loader roundtrip — if the instance's `attributes`
    // carry `material_uv_ops` (written by the Material Flip tool),
    // stash them on the entity as a `PendingMaterialUvOps` component.
    // A system in `tools_smart` picks these up once the material asset
    // finishes loading and composes them into the cloned material's
    // `uv_transform`. Without this, flipped parts come back un-flipped
    // on reload. Phase-1 roundtrip per TOOLSET.md §4.13.5.
    if let Some(ref attrs) = instance.attributes {
        if let Some(toml::Value::Array(arr)) = attrs.get("material_uv_ops") {
            let ops: Vec<String> = arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !ops.is_empty() {
                ec.insert(crate::tools_smart::PendingMaterialUvOps { ops });
            }
        }
    }

    // Attach realism components if present in TOML
    if let Some(ref mat) = instance.material {
        ec.insert(mat.to_component());
        debug!("  + MaterialProperties: {}", mat.name);
    }
    if let Some(ref thermo) = instance.thermodynamic {
        ec.insert(thermo.to_component());
        debug!("  + ThermodynamicState: T={:.1}K P={:.0}Pa", thermo.temperature, thermo.pressure);
    }
    if let Some(ref echem) = instance.electrochemical {
        ec.insert(echem.to_component());
        debug!("  + ElectrochemicalState: V={:.2}V SOC={:.1}%", echem.voltage, echem.soc * 100.0);
    }
    if let Some(ref nuc) = instance.nuclear {
        ec.insert(nuc.to_init());
        debug!("  + NuclearInit: T={:.0}°C load={:.0}W", nuc.core_temp_celsius, nuc.load_demand_watts);
    }
    if let Some(ref plasma) = instance.plasma {
        ec.insert(plasma.to_component());
        debug!("  + PlasmaState: ne={:.1e} Te={:.1e}K", plasma.electron_density, plasma.electron_temperature_k);
    }
    // Attach UI ECS component if this is a UI class
    attach_ui_component(&mut ec, class_name, instance.ui.as_ref());
    // End the EntityCommands borrow before the decal/mesh attach (needs
    // `&mut commands`); the attach removes the consumed `decal`/`mesh` key
    // so PendingExtraSections below never double-dispatches it.
    drop(ec);
    attach_decal_mesh_component(
        commands, entity, asset_server, decal_materials, class_name,
        &mut instance.extra, render_transform, &name,
    );
    // Extra sections — see the custom-mesh branch above for
    // rationale. Third-party plugins claim these via
    // `ExtraSectionRegistry`.
    if !instance.extra.is_empty() {
        commands.entity(entity).insert(eustress_common::class_schema::PendingExtraSections {
            sections: instance.extra.clone(),
        });
    }
    debug!("Spawned primitive '{}' ({}) from {:?}", name, instance.metadata.class_name, toml_path);
    entity
}

/// Insert the appropriate ECS UI component onto an entity based on class name and [ui] data.
/// If no [ui] section is present, component defaults are used.
pub fn attach_ui_component(
    ec: &mut bevy::ecs::system::EntityCommands,
    class_name: eustress_common::classes::ClassName,
    ui: Option<&UiInstanceProperties>,
) {
    use eustress_common::classes::{
        ClassName, TextLabel, TextButton, TextBox, Frame, ImageLabel, ImageButton, ScrollingFrame,
    };
    let ui_defaults = UiInstanceProperties::default();
    let u = ui.unwrap_or(&ui_defaults);

    match class_name {
        ClassName::TextLabel => {
            ec.insert(TextLabel {
                text: u.text.clone(),
                rich_text: u.rich_text,
                text_scaled: u.text_scaled,
                text_wrapped: u.text_wrapped,
                max_visible_graphemes: -1,
                font: u.to_font(),
                font_size: u.font_size,
                line_height: if u.line_height > 0.0 { u.line_height } else { 1.0 },
                text_color3: u.text_color3,
                text_transparency: u.text_transparency,
                text_stroke_color3: u.text_stroke_color3,
                text_stroke_transparency: u.text_stroke_transparency,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                text_x_alignment: u.to_x_align(),
                text_y_alignment: u.to_y_align(),
                // Roblox-parity Position/Size as UDim2. The TOML schema
                // still carries split scale/offset; combine them here.
                position: u.position,
                size: u.size,
                anchor_point: u.anchor_point,
                rotation: u.rotation,
                z_index: u.z_index,
                active: u.active,
                visible: u.visible,
                clips_descendants: u.clips_descendants,
                border_size_pixel: u.border_size_pixel,
                automatic_size: u.to_auto_size(),
                ..Default::default()
            });
        }
        ClassName::TextButton => {
            ec.insert(TextButton {
                text: u.text.clone(),
                font_size: u.font_size,
                text_color3: u.text_color3,
                text_transparency: u.text_transparency,
                text_stroke_color3: u.text_stroke_color3,
                text_stroke_transparency: u.text_stroke_transparency,
                text_x_alignment: u.to_x_align(),
                text_y_alignment: u.to_y_align(),
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                z_index: u.z_index,
                layout_order: u.layout_order,
                rotation: u.rotation,
                anchor_point: u.anchor_point,
                position: u.position,
                size: u.size,
                visible: u.visible,
                active: u.active,
                auto_button_color: u.auto_button_color,
                ..Default::default()
            });
        }
        ClassName::TextBox => {
            ec.insert(TextBox {
                text: u.text.clone(),
                font_size: u.font_size,
                text_color3: u.text_color3,
                text_transparency: u.text_transparency,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                z_index: u.z_index,
                visible: u.visible,
                ..Default::default()
            });
        }
        ClassName::Frame => {
            ec.insert(Frame {
                visible: u.visible,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                border_mode: u.to_border_mode(),
                clips_descendants: u.clips_descendants,
                z_index: u.z_index,
                layout_order: u.layout_order,
                rotation: u.rotation,
                anchor_point: u.anchor_point,
                position: u.position,
                size: u.size,
            });
        }
        ClassName::ImageLabel => {
            ec.insert(ImageLabel {
                image: u.image.clone(),
                image_color3: u.image_color3,
                image_transparency: u.image_transparency,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                z_index: u.z_index,
                layout_order: u.layout_order,
                rotation: u.rotation,
                anchor_point: u.anchor_point,
                position: u.position,
                size: u.size,
                visible: u.visible,
                ..Default::default()
            });
        }
        ClassName::ImageButton => {
            ec.insert(ImageButton {
                image: u.image.clone(),
                image_color3: u.image_color3,
                image_transparency: u.image_transparency,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                z_index: u.z_index,
                layout_order: u.layout_order,
                rotation: u.rotation,
                anchor_point: u.anchor_point,
                position: u.position,
                size: u.size,
                visible: u.visible,
                active: u.active,
                auto_button_color: u.auto_button_color,
                ..Default::default()
            });
        }
        ClassName::ScrollingFrame => {
            ec.insert(ScrollingFrame {
                visible: u.visible,
                background_color3: u.background_color3,
                background_transparency: u.background_transparency,
                border_color3: u.border_color3,
                border_size_pixel: u.border_size_pixel,
                z_index: u.z_index,
                layout_order: u.layout_order,
                rotation: u.rotation,
                anchor_point: u.anchor_point,
                position: u.position,
                size: u.size,
                scrolling_enabled: u.scrolling_enabled,
                scroll_bar_thickness: u.scroll_bar_thickness,
                ..Default::default()
            });
        }
        _ => {}
    }
}

// ============================================================================
// Read-side hydrator helpers (Decal / SpecialMesh / ParticleEmitter / Beam)
// ============================================================================

/// Borrow a named `[section]` table out of the flattened `extra` map
/// (case-insensitive on the section name).
fn section_table<'a>(
    extra: &'a std::collections::HashMap<String, toml::Value>,
    name: &str,
) -> Option<&'a toml::value::Table> {
    extra
        .get(name)
        .or_else(|| extra.get(&name.to_ascii_uppercase()))
        .and_then(|v| v.as_table())
}

/// `[r,g,b]` (or `[r,g,b,a]`) 0-255 INTEGER array → normalized `[f32;4]`
/// RGBA. Tries `as_integer` (÷255) THEN `as_float` (pass-through) per
/// channel so either encoding survives. Missing/short arrays fall back to
/// the supplied default.
fn color_u8_array_to_rgba(v: Option<&toml::Value>, fallback: [f32; 4]) -> [f32; 4] {
    let Some(arr) = v.and_then(|v| v.as_array()) else {
        return fallback;
    };
    if arr.len() != 3 && arr.len() != 4 {
        return fallback;
    }
    let channel = |i: usize, def: f32| -> f32 {
        match arr.get(i) {
            Some(c) => c
                .as_integer()
                .map(|n| n as f32 / 255.0)
                .or_else(|| c.as_float().map(|f| f as f32))
                .unwrap_or(def),
            None => def,
        }
    };
    [
        channel(0, fallback[0]),
        channel(1, fallback[1]),
        channel(2, fallback[2]),
        if arr.len() == 4 { channel(3, fallback[3]) } else { fallback[3] },
    ]
}

/// Read a scalar that may be authored as int OR float.
fn toml_f32(v: Option<&toml::Value>) -> Option<f32> {
    v.and_then(|v| {
        v.as_float()
            .or_else(|| v.as_integer().map(|n| n as f64))
            .map(|f| f as f32)
    })
}

/// Read a 3-element array (int or float) → `Vec3`.
fn toml_vec3(v: Option<&toml::Value>) -> Option<Vec3> {
    let arr = v.and_then(|v| v.as_array())?;
    if arr.len() != 3 {
        return None;
    }
    let mut out = [0.0f32; 3];
    for (slot, item) in out.iter_mut().zip(arr.iter()) {
        *slot = item.as_float().or_else(|| item.as_integer().map(|n| n as f64))? as f32;
    }
    Some(Vec3::from_array(out))
}

/// Map an importer `[decal].face` string → engine `Face` enum.
fn face_from_str(s: &str) -> eustress_common::classes::Face {
    use eustress_common::classes::Face;
    match s {
        "Top" => Face::Top,
        "Bottom" => Face::Bottom,
        "Back" => Face::Back,
        "Left" => Face::Left,
        "Right" => Face::Right,
        _ => Face::Front,
    }
}

/// Map an importer `[mesh].mesh_type` string → engine `MeshType` enum.
fn mesh_type_from_str(s: &str) -> eustress_common::classes::MeshType {
    use eustress_common::classes::MeshType;
    match s {
        "Head" => MeshType::Head,
        "Torso" => MeshType::Torso,
        "Brick" => MeshType::Brick,
        "Sphere" => MeshType::Sphere,
        "Cylinder" => MeshType::Cylinder,
        _ => MeshType::FileMesh,
    }
}

/// Attach the Decal / SpecialMesh component from the importer-written
/// `[decal]` / `[mesh]` section. For a `Decal` it ALSO spawns a real
/// `ForwardDecal` child (a bare `Decal` component renders nothing) and
/// parents it to `host`. The consumed `decal`/`mesh` key is REMOVED from
/// `extra` so the later `PendingExtraSections` insert never double-
/// dispatches it.
fn attach_decal_mesh_component(
    commands: &mut Commands,
    host: Entity,
    asset_server: &AssetServer,
    decal_materials: &mut Assets<ForwardDecalMaterial<StandardMaterial>>,
    class_name: eustress_common::classes::ClassName,
    extra: &mut std::collections::HashMap<String, toml::Value>,
    base_transform: Transform,
    name: &str,
) {
    use eustress_common::classes::{ClassName, Decal, Instance, SpecialMesh};
    match class_name {
        ClassName::Decal => {
            let Some(sec) = section_table(extra, "decal") else { return; };
            let color = color_u8_array_to_rgba(sec.get("color"), [1.0, 1.0, 1.0, 1.0]);
            let transparency = toml_f32(sec.get("transparency")).unwrap_or(0.0);
            let z_index = sec
                .get("z_index")
                .and_then(|v| v.as_integer())
                .unwrap_or(0) as i32;
            let face = sec
                .get("face")
                .and_then(|v| v.as_str())
                .map(face_from_str)
                .unwrap_or(eustress_common::classes::Face::Front);
            let texture = sec
                .get("texture")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let decal = Decal {
                texture,
                face,
                transparency,
                color,
                z_index,
                ..Default::default()
            };
            let inst = Instance {
                name: name.to_string(),
                class_name: ClassName::Decal,
                archivable: true,
                id: 0,
                ai: false,
                uuid: String::new(),
            };
            let decal_entity = crate::spawn::spawn_decal(
                commands,
                asset_server,
                decal_materials,
                inst,
                decal,
                base_transform,
            );
            commands.entity(decal_entity).insert(ChildOf(host));
            extra.remove("decal");
            extra.remove("Decal");
        }
        ClassName::SpecialMesh => {
            let Some(sec) = section_table(extra, "mesh") else { return; };
            let mut sm = SpecialMesh::default();
            if let Some(s) = sec.get("mesh_type").and_then(|v| v.as_str()) {
                sm.mesh_type = mesh_type_from_str(s);
            }
            if let Some(v) = toml_vec3(sec.get("scale")) {
                sm.scale = v;
            }
            if let Some(v) = toml_vec3(sec.get("offset")) {
                sm.offset = v;
            }
            if let Some(s) = sec.get("mesh_id").and_then(|v| v.as_str()) {
                sm.mesh_id = s.to_string();
            }
            commands.entity(host).insert(sm);
            // texture_id / vertex_color have no SpecialMesh field — leave
            // them in `extra` for round-trip.
            extra.remove("mesh");
            extra.remove("Mesh");
        }
        _ => {}
    }
}

/// Attach the data-only ParticleEmitter / Beam component from the
/// importer `[particle]` / `[beam]` section. These classes have no
/// `[asset]`, so they hit the no-mesh branch and read from the flattened
/// `extra` map. NOTHING renders yet (particles.rs / beams.rs are stubs) —
/// this makes the data live for Properties / scripts only.
fn attach_vfx_component(
    ec: &mut bevy::ecs::system::EntityCommands,
    class_name: eustress_common::classes::ClassName,
    extra: &std::collections::HashMap<String, toml::Value>,
) {
    use eustress_common::classes::{Beam, ClassName, ParticleEmitter};
    match class_name {
        ClassName::ParticleEmitter => {
            let Some(sec) = section_table(extra, "particle") else { return; };
            let mut p = ParticleEmitter::default();
            if let Some(v) = sec.get("enabled").and_then(|v| v.as_bool()) { p.enabled = v; }
            if let Some(v) = toml_f32(sec.get("rate")) { p.rate = v; }
            if let Some(v) = toml_f32(sec.get("drag")) { p.drag = v; }
            if let Some(v) = toml_f32(sec.get("lifetime_min")) { p.lifetime.0 = v; }
            if let Some(v) = toml_f32(sec.get("lifetime_max")) { p.lifetime.1 = v; }
            if let Some(v) = toml_f32(sec.get("speed_min")) { p.speed.0 = v; }
            if let Some(v) = toml_f32(sec.get("speed_max")) { p.speed.1 = v; }
            if let Some(v) = toml_f32(sec.get("size")) { p.size = (v, v); }
            if let Some(v) = toml_f32(sec.get("spread_angle")) { p.spread_angle = Vec2::splat(v); }
            if let Some(v) = toml_f32(sec.get("rotation_speed_min")) { p.rotation_speed.0 = v; }
            if let Some(v) = toml_f32(sec.get("rotation_speed_max")) { p.rotation_speed.1 = v; }
            // light_emission is a float on the wire (>0 ⇒ emit).
            if let Some(v) = toml_f32(sec.get("light_emission")) { p.light_emission = v > 0.0; }
            if let Some(s) = sec.get("texture").and_then(|v| v.as_str()) { p.texture = s.to_string(); }
            // Color (+ transparency) → 2-key color_sequence.
            if let Some(rgba) = sec
                .get("color")
                .map(|c| color_u8_array_to_rgba(Some(c), [1.0, 1.0, 1.0, 1.0]))
            {
                let alpha = 1.0 - toml_f32(sec.get("transparency")).unwrap_or(0.0);
                let start = Color::srgba(rgba[0], rgba[1], rgba[2], alpha);
                let end = Color::srgba(rgba[0], rgba[1], rgba[2], 0.0);
                p.color_sequence = vec![(0.0, start), (1.0, end)];
            }
            ec.insert(p);
        }
        ClassName::Beam => {
            let Some(sec) = section_table(extra, "beam") else { return; };
            let mut b = Beam::default();
            if let Some(v) = sec.get("enabled").and_then(|v| v.as_bool()) { b.enabled = v; }
            if let Some(v) = toml_f32(sec.get("width0")) { b.width0 = v; }
            if let Some(v) = toml_f32(sec.get("width1")) { b.width1 = v; }
            if let Some(v) = toml_f32(sec.get("curve_size0")) { b.curve_size0 = v; }
            if let Some(v) = toml_f32(sec.get("curve_size1")) { b.curve_size1 = v; }
            if let Some(v) = sec.get("segments").and_then(|v| v.as_integer()) { b.segments = v.max(0) as u32; }
            if let Some(v) = toml_f32(sec.get("brightness")) { b.brightness = v; }
            if let Some(v) = toml_f32(sec.get("light_emission")) { b.light_emission = v; }
            if let Some(v) = toml_f32(sec.get("texture_length")) { b.texture_length = v; }
            if let Some(v) = toml_f32(sec.get("texture_speed")) { b.texture_speed = v; }
            if let Some(s) = sec.get("texture").and_then(|v| v.as_str()) { b.texture = s.to_string(); }
            if let Some(v) = sec.get("face_camera").and_then(|v| v.as_bool()) {
                b.face_mode = if v {
                    eustress_common::classes::BeamFaceMode::FaceCamera
                } else {
                    eustress_common::classes::BeamFaceMode::Fixed
                };
            }
            if let Some(s) = sec.get("texture_mode").and_then(|v| v.as_str()) {
                b.texture_mode = match s {
                    "Stretch" => eustress_common::classes::TextureMode::Stretch,
                    "Static" => eustress_common::classes::TextureMode::Static,
                    _ => eustress_common::classes::TextureMode::Tile,
                };
            }
            if let Some(rgba) = sec
                .get("color")
                .map(|c| color_u8_array_to_rgba(Some(c), [1.0, 1.0, 1.0, 1.0]))
            {
                let c = Color::srgba(rgba[0], rgba[1], rgba[2], 1.0);
                b.color_sequence = vec![(0.0, c), (1.0, c)];
            }
            if let Some(t) = toml_f32(sec.get("transparency")) {
                b.transparency_sequence = vec![(0.0, t), (1.0, t)];
            }
            ec.insert(b);
        }
        _ => {}
    }
}

// NOTE: Instance loading is handled by SpaceFileLoaderPlugin (file_loader.rs)
// which properly creates folder hierarchy with parent-child relationships.
// The load_instance_files_system was removed to avoid duplicate loading.

/// System to write instance changes back to .glb.toml files.
///
/// PERF: Uses `Changed<Transform>` BUT excludes `Added<Transform>`.
/// Bevy marks newly-inserted components as Changed, so without the exclusion
/// ALL 10K entities would trigger 20K disk I/O ops on the first frame after
/// spawn (read TOML + write TOML per entity = 1-second freeze).
/// Only entities whose Transform was **modified** (gizmo, properties panel)
/// after initial spawn will be written back.
/// Marker placed on an entity for the duration of a manipulator drag
/// (Move / Rotate / Scale gizmos). While present, [`write_instance_changes_system`]
/// skips disk writes for that entity — the tool's mouse-release branch is
/// the canonical single TOML write per drag. Without this, every mouse-move
/// frame during a drag would queue a TOML write, producing dozens of disk
/// writes per second + a file-watcher reload storm.
///
/// Tools MUST pair every `insert(BeingDragged)` with a `remove::<BeingDragged>()`
/// in all drag-exit paths (mouse-up, Escape cancel, numeric-input finalise,
/// tool switch). The cancel paths are easy to miss — keep one mental
/// invariant: `BeingDragged` should never outlive `state.dragged_axis` /
/// `dragged_plane` / `free_drag`.
#[derive(Component, Default)]
pub struct BeingDragged;

pub fn write_instance_changes_system(
    instances: Query<(
        Entity,
        &Transform,
        &InstanceFile,
        Option<&eustress_common::classes::BasePart>,
        Option<&eustress_common::units::MeasureUnit>,
    ), (
        Or<(Changed<Transform>, Changed<eustress_common::classes::BasePart>)>,
        // Defer TOML writes for entities currently held by a gizmo drag.
        // The tool itself writes once on mouse-release; this auto-system
        // is for non-drag changes (Properties panel edits, scripts, MCP).
        Without<BeingDragged>,
    )>,
    added_instances: Query<Entity, Added<Transform>>,
    mut recently_written: ResMut<super::file_watcher::RecentlyWrittenFiles>,
    load_in_progress: Res<super::file_loader::LoadInProgress>,
) {
    // Gate every disk write while the cold-load / rescan path is still
    // settling. Without this, mesh-handle resolution and class-default
    // backfill mark BasePart as Changed for every just-loaded entity,
    // and the writer rewrites all 50k TOMLs we just read from disk —
    // ~53 s of background I/O for zero useful work. The
    // `Added<Transform>` HashSet below catches the same-frame spawn;
    // this guard catches the long tail across the load-settle window.
    if load_in_progress.active {
        return;
    }

    // Collect entities that were just added this tick — skip them entirely.
    // Bevy marks newly-inserted components as Changed, so without this check
    // ALL 10K entities would trigger 20K disk I/O ops on their first frame.
    let just_added: std::collections::HashSet<Entity> = added_instances.iter().collect();

    // Collect all write jobs this frame, then dispatch to background thread.
    // Each job carries the TOML path, transform data, and optional BasePart
    // properties (material, color, transparency, reflectance, etc.) so the
    // background thread can persist all visual properties — not just position.
    struct WriteJob {
        path: std::path::PathBuf,
        transform: TransformData,
        material: Option<String>,
        color: Option<[f32; 4]>,
        transparency: Option<f32>,
        reflectance: Option<f32>,
        anchored: Option<bool>,
        can_collide: Option<bool>,
        locked: Option<bool>,
        /// True when the entity references a custom GLB mesh (e.g. V-Cell
        /// parts). For these, `scale` in the TOML is the user-set multiplier
        /// and must NOT be overwritten from `BasePart.size` (which comes from
        /// the mesh bounding box and would clobber the user's value with
        /// whatever the mesh happens to measure in scene units).
        is_custom_mesh: bool,
        /// Authored unit of the file we're writing into. Position and
        /// scale are converted from engine-native meters to this unit
        /// at serialisation (identity short-circuit on `Meter`).
        authored_unit: eustress_common::units::Unit,
    }
    let mut jobs: Vec<WriteJob> = Vec::new();

    for (entity, transform, instance_file, base_part, measure_unit) in instances.iter() {
        if just_added.contains(&entity) {
            continue;
        }
        if recently_written.was_recently_written(&instance_file.toml_path) {
            continue;
        }
        // Lighting-service entities (Star/Sun, Moon, Sky, Atmosphere) have
        // runtime-driven Transforms (sun direction from time_of_day, etc.)
        // that must not be persisted — their authoritative state lives in
        // LightingService, not the TOML. Writing them would produce a stutter
        // loop: transform write → file-watcher event → class_schema self-heal
        // → another file-watcher event, every ~2 s.
        if instance_file.toml_path.components().any(|c| c.as_os_str() == "Lighting") {
            continue;
        }

        // Auto-write must use BasePart.size for `scale`, matching save_space.
        //
        // Mid-drag the scale tool temporarily sets `Transform.scale = size /
        // mesh_baked_size` to defer primitive mesh regen (perf). Writing
        // that transient ratio to disk corrupts the TOML because reload
        // treats `scale` as the part's size. Reading BasePart.size (the
        // authoritative dimension) keeps the round-trip honest whether
        // the part is file-system-first (scale==size) or legacy
        // (scale==ONE, mesh baked at size). This was the "neat door came
        // back as a mess" regression the user hit 2026-04-23.
        //
        // Sanitize before serialising — if a NaN/Inf snuck into Transform
        // (e.g. a degenerate gizmo math edge case), persisting it to disk
        // poisons every future reload AND panics Avian's
        // `assert_components_finite` check on the next physics tick. The
        // clamp loses sub-pixel precision in the failure case but keeps
        // the engine alive.
        let safe_transform = sanitize_transform(*transform);
        let mut td = TransformData::from(safe_transform);
        // Detect custom mesh: a primitive mesh path ends with parts/*.glb;
        // anything else is a user-supplied GLB (V-Cell, CAD exports, etc.)
        let is_custom_mesh = {
            let p = instance_file.mesh_path.to_string_lossy();
            let fname = instance_file.mesh_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            // Primitives are block/ball/cylinder/wedge/corner_wedge/cone
            !matches!(fname, "block.glb"|"ball.glb"|"cylinder.glb"|"wedge.glb"|"corner_wedge.glb"|"cone.glb")
            && !p.is_empty()
        };
        let authored_unit = measure_unit
            .map(|m| m.0)
            .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT);
        let mut job = WriteJob {
            path: instance_file.toml_path.clone(),
            transform: td.clone(),
            material: None,
            color: None,
            transparency: None,
            reflectance: None,
            anchored: None,
            can_collide: None,
            locked: None,
            is_custom_mesh,
            authored_unit,
        };
        if let Some(bp) = base_part {
            // Same guard for `BasePart.size` — sanitize_size clamps each
            // axis to [0.1, +∞) and replaces NaN with the floor.
            let safe_size = sanitize_size(bp.size);
            // Custom-mesh parts: do NOT overwrite `scale` from BasePart.size.
            // The scale in the TOML is the user's multiplier; BasePart.size
            // comes from the mesh bounding box at spawn time. If the mesh
            // ever loads incorrectly (wrong GLB path → block.glb fallback),
            // persisting that bounding box would permanently corrupt the TOML.
            if !is_custom_mesh {
                job.transform.scale = [safe_size.x, safe_size.y, safe_size.z];
            }
            // Persist all BasePart visual properties so material changes,
            // color edits, transparency tweaks, etc. survive reload.
            let mat_name = if bp.material_name.is_empty() {
                bp.material.as_str().to_string()
            } else {
                bp.material_name.clone()
            };
            job.material = Some(mat_name);
            let srgba = bp.color.to_srgba();
            job.color = Some([srgba.red, srgba.green, srgba.blue, srgba.alpha]);
            job.transparency = Some(bp.transparency);
            job.reflectance = Some(bp.reflectance);
            job.anchored = Some(bp.anchored);
            job.can_collide = Some(bp.can_collide);
            job.locked = Some(bp.locked);
        }

        recently_written.mark_written(instance_file.toml_path.clone());
        jobs.push(job);
    }

    if jobs.is_empty() {
        return;
    }

    // Dispatch all writes to a background thread — never block the main frame.
    let job_count = jobs.len();
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        for job in &jobs {
            // Patch the raw TOML in-place rather than going through
            // load_instance_definition.  That path runs the self-heal pass which
            // (a) can rewrite the file while we're reading it and
            // (b) re-serialises through InstanceDefinition, silently dropping any
            // section the typed struct doesn't recognise ([material], [thermodynamic],
            // [electrochemical], [material.custom], etc.).  A surgical patch on the
            // raw toml::Value preserves every section we don't touch.
            let patch_result = (|| -> Result<(), String> {
                let text = std::fs::read_to_string(&job.path)
                    .map_err(|e| format!("read {:?}: {}", job.path, e))?;
                let mut doc: toml::Value = text.parse()
                    .map_err(|e: toml::de::Error| format!("parse {:?}: {}", job.path, e))?;

                let root = doc.as_table_mut()
                    .ok_or_else(|| format!("TOML root is not a table: {:?}", job.path))?;

                // ── [transform] ────────────────────────────────────────────────
                // For custom-mesh parts the scale lives in the TOML as the user's
                // size multiplier; BasePart.size comes from the mesh AABB and must
                // not clobber it.  For primitives scale == size, so always write.
                let tf = root.entry("transform")
                    .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
                    .as_table_mut()
                    .ok_or("transform is not a table")?;

                // Convert engine-native meters back to the file's
                // authored unit at the very edge. Identity short-circuit
                // when `authored_unit == Meter`, so meter-authored files
                // pay zero conversion cost. The conversion mirrors the
                // load path: any value entering the file is in the
                // unit symbol the file's `[metadata].unit` declares.
                let [px, py, pz] = eustress_common::units::engine_to_authored_vec3_f32(
                    job.transform.position, job.authored_unit,
                );
                tf.insert("position".into(), toml::Value::Array(vec![
                    toml::Value::Float(px as f64),
                    toml::Value::Float(py as f64),
                    toml::Value::Float(pz as f64),
                ]));
                let [rx, ry, rz, rw] = job.transform.rotation;
                tf.insert("rotation".into(), toml::Value::Array(vec![
                    toml::Value::Float(rx as f64),
                    toml::Value::Float(ry as f64),
                    toml::Value::Float(rz as f64),
                    toml::Value::Float(rw as f64),
                ]));
                if !job.is_custom_mesh {
                    let [sx, sy, sz] = eustress_common::units::engine_to_authored_vec3_f32(
                        job.transform.scale, job.authored_unit,
                    );
                    tf.insert("scale".into(), toml::Value::Array(vec![
                        toml::Value::Float(sx as f64),
                        toml::Value::Float(sy as f64),
                        toml::Value::Float(sz as f64),
                    ]));
                }
                // [asset] is never touched — mesh path is immutable from auto-save.

                // ── [properties] ───────────────────────────────────────────────
                let props = root.entry("properties")
                    .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
                    .as_table_mut()
                    .ok_or("properties is not a table")?;

                if let Some(ref mat) = job.material {
                    props.insert("material".into(), toml::Value::String(mat.clone()));
                }
                if let Some(color) = job.color {
                    props.insert("color".into(), toml::Value::Array(vec![
                        toml::Value::Float(color[0] as f64),
                        toml::Value::Float(color[1] as f64),
                        toml::Value::Float(color[2] as f64),
                        toml::Value::Float(color[3] as f64),
                    ]));
                }
                if let Some(t) = job.transparency {
                    props.insert("transparency".into(), toml::Value::Float(t as f64));
                }
                if let Some(r) = job.reflectance {
                    props.insert("reflectance".into(), toml::Value::Float(r as f64));
                }
                if let Some(a) = job.anchored {
                    props.insert("anchored".into(), toml::Value::Boolean(a));
                }
                if let Some(c) = job.can_collide {
                    props.insert("can_collide".into(), toml::Value::Boolean(c));
                }
                if let Some(l) = job.locked {
                    props.insert("locked".into(), toml::Value::Boolean(l));
                }

                // ── [metadata].last_modified ────────────────────────────────────
                if let Some(meta) = root.get_mut("metadata").and_then(|m| m.as_table_mut()) {
                    meta.insert("last_modified".into(),
                        toml::Value::String(chrono::Utc::now().to_rfc3339()));
                }

                let out = toml::to_string_pretty(&doc)
                    .map_err(|e| format!("serialize {:?}: {}", job.path, e))?;
                // Atomic write + retry so a transient file-lock from
                // an external reader (antivirus, text editor, the
                // engine's reload-after-write pass) doesn't silently
                // drop the user's edit (see `gui_loader::write_atomic`
                // for the full rationale).
                super::gui_loader::write_atomic(&job.path, out.as_bytes())
                    .map_err(|e| format!("write {:?}: {}", job.path, e))?;
                Ok(())
            })();
            if let Err(e) = patch_result {
                tracing::error!("Instance patch write failed: {}", e);
            }
        }
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 50 {
            tracing::warn!("🐌 Background instance writes: {:.1}ms ({} files)", elapsed.as_secs_f64() * 1000.0, job_count);
        }
    });
}

// ============================================================================
// Tags + Attributes write-back — applies to ALL classes, not just BaseParts
// ============================================================================
//
// `save_tags_and_attributes_changes` runs alongside `write_instance_changes_system`
// but is filtered to `Changed<Tags>` / `Changed<Attributes>` so any class
// (Part, Model, BillboardGui, Script, Folder, …) with an `InstanceFile`
// component gets its tag / attribute mutations persisted to disk. Without
// this system, tag changes from Rune scripts, Luau scripts, the
// Properties panel, or future MCP-ECS-mediated paths would live only in
// the ECS and disappear on restart.

/// Convert a rich in-memory `AttributeValue` into a plain `toml::Value`
/// suitable for the `[attributes]` section. Mirrors the inverse mapping
/// in `rich_toml_value_to_attribute` (which loads TOML → ECS). Types
/// outside the round-trip-safe set (Object / EntityRef / CFrame / …)
/// fall back to a string display so the file stays human-readable even
/// when the data isn't recoverable on load.
pub(crate) fn attribute_to_toml(value: &eustress_common::AttributeValue) -> toml::Value {
    use eustress_common::AttributeValue as A;
    match value {
        A::Bool(b)      => toml::Value::Boolean(*b),
        A::Int(i)       => toml::Value::Integer(*i),
        A::Number(n)    => toml::Value::Float(*n),
        A::String(s)    => toml::Value::String(s.clone()),
        A::Vector2(v)   => toml::Value::Array(vec![
            toml::Value::Float(v.x as f64),
            toml::Value::Float(v.y as f64),
        ]),
        A::Vector3(v)   => toml::Value::Array(vec![
            toml::Value::Float(v.x as f64),
            toml::Value::Float(v.y as f64),
            toml::Value::Float(v.z as f64),
        ]),
        A::Color(c) | A::Color3(c) => {
            let s = c.to_srgba();
            toml::Value::Array(vec![
                toml::Value::Float(s.red as f64),
                toml::Value::Float(s.green as f64),
                toml::Value::Float(s.blue as f64),
                toml::Value::Float(s.alpha as f64),
            ])
        }
        // Less common types fall through to display strings — readable
        // in a TOML file but not currently re-parseable on reload. Good
        // enough for diff-friendly snapshotting; full round-trip can
        // be plumbed when a user surface needs it.
        other => toml::Value::String(other.display_value()),
    }
}

/// Patch a single `_instance.toml` with the entity's current tags and
/// attributes. Pure on-disk operation — no Bevy types in or out. Runs
/// on a background thread.
fn patch_tags_attributes_toml(
    path: &std::path::Path,
    tags: Option<Vec<String>>,
    attributes: Option<std::collections::HashMap<String, toml::Value>>,
) -> Result<(), String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read {:?}: {}", path, e))?;
    let mut doc: toml::Value = raw.parse()
        .map_err(|e| format!("parse {:?}: {}", path, e))?;
    let Some(root) = doc.as_table_mut() else {
        return Err(format!("{:?}: top-level is not a table", path));
    };

    if let Some(tags) = tags {
        if tags.is_empty() {
            root.remove("tags");
        } else {
            root.insert(
                "tags".into(),
                toml::Value::Array(tags.into_iter().map(toml::Value::String).collect()),
            );
        }
    }

    if let Some(attrs) = attributes {
        if attrs.is_empty() {
            root.remove("attributes");
        } else {
            let mut tbl = toml::map::Map::new();
            for (k, v) in attrs {
                tbl.insert(k, v);
            }
            root.insert("attributes".into(), toml::Value::Table(tbl));
        }
    }

    // Touch [metadata].last_modified for parity with the transform
    // write path so external tools that diff on the timestamp pick up
    // tag-only edits.
    if let Some(meta) = root.get_mut("metadata").and_then(|m| m.as_table_mut()) {
        meta.insert(
            "last_modified".into(),
            toml::Value::String(chrono::Utc::now().to_rfc3339()),
        );
    }

    let out = toml::to_string_pretty(&doc)
        .map_err(|e| format!("serialize {:?}: {}", path, e))?;
    super::gui_loader::write_atomic(path, out.as_bytes())
        .map_err(|e| format!("write {:?}: {}", path, e))?;
    Ok(())
}

/// Ensure every entity with an `InstanceFile` carries default-empty
/// `Tags` and `Attributes` components, so script APIs / MCP tools /
/// Properties-panel edits always have a destination component to
/// mutate on any class — not just BaseParts.
///
/// `spawn_instance` covers Part / class_schema entities directly, but
/// services, folders, GUI, scripts, and future spawn paths each have
/// their own siloed spawn site. This catch-all system runs on
/// `Added<InstanceFile>` so it fires exactly once per entity, the
/// frame after spawn. The `Without<Tags>` / `Without<Attributes>`
/// filters keep it from re-inserting on entities that already have
/// them, and `save_tags_and_attributes_changes`'s `Added<>` skip-set
/// prevents the freshly-inserted-but-empty components from triggering
/// a no-op TOML write-back on cold load.
pub fn ensure_tags_and_attributes_components(
    mut commands: Commands,
    needs_tags: Query<
        Entity,
        (
            Added<InstanceFile>,
            Without<eustress_common::attributes::Tags>,
        ),
    >,
    needs_attrs: Query<
        Entity,
        (
            Added<InstanceFile>,
            Without<eustress_common::attributes::Attributes>,
        ),
    >,
) {
    for entity in needs_tags.iter() {
        commands
            .entity(entity)
            .insert(eustress_common::attributes::Tags::new());
    }
    for entity in needs_attrs.iter() {
        commands
            .entity(entity)
            .insert(eustress_common::attributes::Attributes::new());
    }
}

/// Ensure every entity that carries an `InstanceFile` OR `LoadedFromFile`
/// has a `MeasureUnit` component. Defaults to `MeasureUnit(Meter)` —
/// the engine-native unit.
///
/// ## Why an auto-attach system instead of editing every spawn site
///
/// There are 30+ `commands.spawn` sites across `instance_loader`,
/// `file_loader`, `gui_loader`, `service_loader`, `file_watcher`,
/// `spawn`, and `spawn_events`. Threading a `MeasureUnit(...)` into
/// every bundle invites missing one; missing one means the entity's
/// future disk writes go through the unit-aware path with `None` and
/// fall back to engine-native silently. The auto-attach catches every
/// path uniformly.
///
/// ## Stage 2 contract
///
/// In Stage 2, the cold/hot-load paths will read `metadata.unit` from
/// the TOML and insert `MeasureUnit(parsed_unit)` BEFORE this system
/// runs (or at least before any disk write). The `Without<MeasureUnit>`
/// filter here means the explicit insert wins; this is the fallback
/// for entities that genuinely had no authoring info on disk.
pub fn ensure_measure_unit(
    mut commands: Commands,
    needs_unit: Query<
        Entity,
        (
            Or<(Added<InstanceFile>, Added<super::file_loader::LoadedFromFile>)>,
            Without<eustress_common::units::MeasureUnit>,
        ),
    >,
) {
    for entity in needs_unit.iter() {
        commands
            .entity(entity)
            .insert(eustress_common::units::MeasureUnit::default());
    }
}

/// Persist `Changed<Tags>` / `Changed<Attributes>` mutations to disk.
/// Class-agnostic — every entity with an `InstanceFile` participates,
/// so a Folder, BillboardGui, Script, or custom class can carry tags
/// and attributes that survive a restart.
pub fn save_tags_and_attributes_changes(
    q: Query<
        (
            Entity,
            &InstanceFile,
            Option<&eustress_common::attributes::Tags>,
            Option<&eustress_common::attributes::Attributes>,
        ),
        (
            Or<(
                Changed<eustress_common::attributes::Tags>,
                Changed<eustress_common::attributes::Attributes>,
            )>,
            Without<BeingDragged>,
        ),
    >,
    added_tags: Query<Entity, Added<eustress_common::attributes::Tags>>,
    added_attrs: Query<Entity, Added<eustress_common::attributes::Attributes>>,
    mut recently_written: ResMut<super::file_watcher::RecentlyWrittenFiles>,
    load_in_progress: Res<super::file_loader::LoadInProgress>,
) {
    // Mirror the gate on `write_instance_changes_system`: tag /
    // attribute Changed flags fire during cold-load schema healing
    // and would re-write 50k TOMLs we just read.
    if load_in_progress.active {
        return;
    }

    // Just-added entities had their components inserted this tick (cold
    // load). Skipping them avoids a 1-per-entity TOML write on every
    // Space open — the data already matches what's on disk.
    let just_added: std::collections::HashSet<Entity> =
        added_tags.iter().chain(added_attrs.iter()).collect();

    struct Job {
        path: std::path::PathBuf,
        tags: Option<Vec<String>>,
        attrs: Option<std::collections::HashMap<String, toml::Value>>,
    }
    let mut jobs: Vec<Job> = Vec::new();

    for (entity, instance_file, tags, attrs) in q.iter() {
        if just_added.contains(&entity) { continue; }
        // Binary-ECS entities carry a SYNTHETIC `__bin_…` path that nothing
        // ever writes (their persistence is the world-db save mirror, whose
        // change filter includes `Changed<Attributes>`). Patching it here
        // would just log a read-failure every edit.
        if instance_file.toml_path.to_string_lossy().contains("__bin_") { continue; }
        // Deliberately don't skip on recently_written — see
        // `save_text_label_changes` for the rationale. The watcher's
        // hot-reload loop is broken by `mark_written` below; gating
        // the save itself on the same flag drops rapid edits.
        let tags_payload = tags.map(|t| t.0.clone());
        let attrs_payload = attrs.map(|a| {
            let mut out = std::collections::HashMap::new();
            for (k, v) in a.values.iter() {
                out.insert(k.clone(), attribute_to_toml(v));
            }
            out
        });
        recently_written.mark_written(instance_file.toml_path.clone());
        jobs.push(Job {
            path: instance_file.toml_path.clone(),
            tags: tags_payload,
            attrs: attrs_payload,
        });
    }

    if jobs.is_empty() { return; }

    let job_count = jobs.len();
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        for job in jobs {
            if let Err(e) = patch_tags_attributes_toml(&job.path, job.tags, job.attrs) {
                tracing::error!("Tags/Attributes patch write failed: {}", e);
            }
        }
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 50 {
            tracing::warn!(
                "🐌 Background tag/attr writes: {:.1}ms ({} files)",
                elapsed.as_secs_f64() * 1000.0, job_count,
            );
        }
    });
}
