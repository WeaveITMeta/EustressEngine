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
    // Guard against a degenerate collider, NOT against small parts.
    //
    // This was 0.1, which is a tenth of a METRE, so every dimension under
    // 100 mm was silently clamped UP to 100 mm. That made precision modelling
    // impossible: a V-Cell laminate authored at 12 um per separator loaded as
    // 2,276 overlapping 100 mm slabs, which renders as noise and drops the
    // frame rate to 2 FPS. Nothing warned, because the clamp is silent and the
    // entity count is correct.
    //
    // What the guard actually has to prevent is Avian asserting on a
    // zero-extent or NaN AABB (`b.min.cmple(b.max).all()`). A micron satisfies
    // that as well as a decimetre does. 1 um is below any dimension a real
    // assembly carries and still keeps every AABB strictly positive.
    const MIN: f32 = 1.0e-6;
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
pub(crate) fn safe_collider_from(
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
    // COLLIDER DIMENSIONS ARE IN *LOCAL* SPACE — Avian multiplies them by the
    // entity's `GlobalTransform.scale`.
    //
    // `collider/backend.rs` runs `set_scale(scale)` in the on-insert hook,
    // commented "This overwrites the scale set by the constructor". So the
    // final world-space size is `constructor_arg × transform.scale`, and a
    // constructor fed world-space dimensions gets SQUARED.
    //
    // Eustress parts are unit meshes with `Transform.scale = size`, so the
    // correct local extent is `size / transform.scale` — normally exactly 1.
    // Two earlier versions were both wrong for the same reason (neither
    // accounted for the hook), for a 0.8 m cube:
    //
    //   passed `size * 0.5` → 0.32 m collider  (parts sank in — the original)
    //   passed `size`       → 0.64 m collider  (still short)
    //   passed `size/scale` → 0.80 m collider  ✔
    //
    // Feeding world dimensions also made big parts monstrous: a 6 m plate
    // became a 36 m collider, so neighbouring test stations silently rested on
    // each other's colliders.
    //
    // Also note Avian's constructors take FULL lengths (they halve internally);
    // `sphere` takes a RADIUS and `cylinder` takes (radius, FULL height).
    // `spawn::collider_local_half` is the canonical implementation of that
    // cancellation (documented and verified against the live scene, but it had
    // no call sites — this is the first). It returns LOCAL half-extents;
    // Avian's constructors want FULL lengths, hence the ×2.
    let half = crate::spawn::collider_local_half(scale, transform.scale);
    if !half.is_finite() {
        return None;
    }
    let fx = (half.x * 2.0).abs().max(MIN_HALF);
    let fy = (half.y * 2.0).abs().max(MIN_HALF);
    let fz = (half.z * 2.0).abs().max(MIN_HALF);
    Some(match part_shape {
        eustress_common::classes::PartType::Ball => Collider::sphere(fx * 0.5),
        eustress_common::classes::PartType::Cylinder | eustress_common::classes::PartType::Cone => {
            Collider::cylinder(fx * 0.5, fy)
        }
        _ => Collider::cuboid(fx, fy, fz),
    })
}

/// EVERY `can_collide` part gets a REAL Avian collider, at any scale —
/// exact click-selection and script raycasts everywhere. (A count-based
/// deferral briefly lived here; it was removed per explicit direction:
/// "Avian needs colliders on all parts no matter what." The scaling cost
/// is handled where it belongs — `avian_prepare_needed` in app_core gates
/// Avian's per-tick collider sweeps to run only when the collider world
/// actually changed or physics is simulating, so static colliders idle at
/// ~zero regardless of count.) The `streaming_active()` deferral below
/// remains ONLY for world-db binary-streamed worlds, whose Play-mode
/// collider streaming was designed around it.
fn defer_collider_for_huge_scene() -> bool {
    crate::space::active_db::streaming_active()
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
pub(crate) fn apply_physics_material(
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
    if let Some(d) = p.density_kg_m3() {
        ec.insert(ColliderDensity(d));
    }
}

// A part's density override from its `[properties.physics]`, the conversion
// a Player's reader shares.
pub(crate) use eustress_common::datamodel::record::part_physical_override;

/// A part's density in kg/m3: its `[properties.physics]` density when the
/// file sets one, else its material's default (Wood 600, steel 7850).
pub(crate) fn part_density_kg_m3(
    material: &eustress_common::classes::Material,
    physics: Option<&PhysicsProperties>,
) -> f32 {
    physics
        .and_then(PhysicsProperties::density_kg_m3)
        .unwrap_or_else(|| eustress_common::classes::BasePart::material_default_density(material))
}

/// Write a part's own density (kg/m3, tagged) into its file's `[properties]`
/// table, or remove it when the part has none, so a Studio Density edit
/// survives a reopen. Only `density` and `density_unit` are touched.
fn patch_physics_density(
    props: &mut toml::map::Map<String, toml::Value>,
    density: Option<f32>,
) -> Result<(), String> {
    match density {
        Some(d) => {
            let physics = props
                .entry("physics")
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
                .as_table_mut()
                .ok_or("properties.physics is not a table")?;
            let value: f64 = d.to_string().parse().unwrap_or(d as f64);
            physics.insert("density".into(), toml::Value::Float(value));
            physics.insert("density_unit".into(), toml::Value::String("kg/m3".into()));
        }
        None => {
            if let Some(physics) = props.get_mut("physics").and_then(|p| p.as_table_mut()) {
                physics.remove("density");
                physics.remove("density_unit");
            }
        }
    }
    Ok(())
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
        // `Changed<Transform>` FILTER (identical semantics to the old
        // `Ref::is_changed()` body check — Changed includes Added, so a
        // freshly-loaded part is still validated once) but the filtering
        // happens at change-tick level BEFORE the row fetch. The old shape
        // fetched the full 3-tuple for all 131K instances every frame just
        // to discard the unchanged (~9 ms/frame on Mountain Ascension); a
        // static world now pays ~nothing.
        Query<
            (Entity, Ref<Transform>, Has<avian3d::prelude::RigidBody>),
            (With<eustress_common::classes::Instance>, Changed<Transform>),
        >,
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
            // (Changed<Transform> query filter already excludes unchanged
            // rows; the belt-and-braces check stays for clarity/safety.)
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
            // 1e-6, not 1e-3. This repair pass runs over spawned entities and
            // overwrites Transform.scale, so a 1 mm floor here silently undoes
            // any part authored thinner than that even when the loader got it
            // right: a 12 um separator loaded with a correct BasePart.size and
            // a transform repaired to 1 mm, and only the render was wrong.
            //
            // The guard is against zero, negative and NaN, which invert or
            // collapse Avian AABBs. A micron is strictly positive and satisfies
            // that; a millimetre just forbids precision assemblies.
            const MIN_SCALE: f32 = 1e-6;
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
    /// Opt in to runtime mesh deformation for this part.
    ///
    /// Surfaced to `BasePart.destructible`, which
    /// `realism::deformation::init_deformable_meshes` watches. Without this
    /// field the flag had no authoring route at all: it existed on `BasePart`
    /// but nothing read it from TOML, so every loaded part was hard-`false`
    /// and the whole deformation pipeline was unreachable outside of code.
    /// Default false → parts stay rigid unless they ask not to be.
    #[serde(default)]
    pub destructible: bool,
    /// Roblox `PhysicalProperties` decomposition written by the importer
    /// under `[properties.physics]`. Optional — absent for hand-authored
    /// parts. When present, the collider-insert path attaches the
    /// matching Avian `Friction` / `Restitution` / `ColliderDensity`
    /// components so imported parts bounce / slide / weigh correctly.
    #[serde(default)]
    pub physics: Option<PhysicsProperties>,
}

// The file's `[properties.physics]`, typed: defined once in common, where a
// Player's reader reads it too.
pub use eustress_common::datamodel::record::PhysicsProperties;

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

    // A bad colour is one bad property, not a bad part: failing here dropped
    // the whole instance from the scene over its colour.
    if values.len() < 3 {
        warn!("color {:?} has fewer than 3 components; using the default grey", values);
        return Ok(default_color());
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
            destructible: false,
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
    /// RFC 3339 timestamp of the edit; on a merged entry, of its latest save.
    pub timestamp: String,
    /// On a merged entry, the first save of the author's run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_timestamp: Option<String>,
    /// How many saves the entry stands for: 1 unless merged.
    #[serde(default = "one_save", skip_serializing_if = "is_one_save")]
    pub saves: u32,
}

fn one_save() -> u32 {
    1
}

fn is_one_save(saves: &u32) -> bool {
    *saves <= 1
}

/// One save's stamp in an audit chain. A save by the author of the last entry
/// extends that entry: its `timestamp` becomes this save's, `first_timestamp`
/// keeps the run's first, and `saves` counts them. A save by anyone else
/// appends. Who edited, and in what order, is kept, while a run of saves by
/// one author (an autosave after every edit) grows the file by nothing.
pub fn record_modification(chain: &mut Vec<CreatorStamp>, stamp: &CreatorStamp) {
    match chain.last_mut() {
        Some(last) if last.public_key == stamp.public_key => {
            if last.first_timestamp.is_none() {
                last.first_timestamp = Some(last.timestamp.clone());
            }
            last.timestamp = stamp.timestamp.clone();
            last.name = stamp.name.clone();
            last.saves = last.saves.max(1) + 1;
        }
        _ => chain.push(CreatorStamp { first_timestamp: None, saves: 1, ..stamp.clone() }),
    }
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
    /// The signed modifications in order, one entry per run of saves by the
    /// same author ([`record_modification`]). Never capped — the full
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

    /// Build the TOML view of a realism material without writing it anywhere.
    ///
    /// Used by the Properties panel to SHOW a destructible part the constants
    /// its `material` name already implies. Nothing is persisted: an explicit
    /// `[material]` block should appear on disk only when someone deliberately
    /// overrides a value, so the common case stays "name the material and get
    /// its physics" rather than accumulating a copy of the preset in every
    /// part file.
    pub fn from_component(
        m: &eustress_common::realism::materials::prelude::MaterialProperties,
    ) -> Self {
        Self {
            name: m.name.clone(),
            young_modulus: m.young_modulus,
            poisson_ratio: m.poisson_ratio,
            yield_strength: m.yield_strength,
            ultimate_strength: m.ultimate_strength,
            fracture_toughness: m.fracture_toughness,
            hardness: m.hardness,
            thermal_conductivity: m.thermal_conductivity,
            specific_heat: m.specific_heat,
            thermal_expansion: m.thermal_expansion,
            melting_point: m.melting_point,
            density: m.density,
            friction_static: m.friction_static,
            friction_kinetic: m.friction_kinetic,
            restitution: m.restitution,
            custom: HashMap::new(),
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
    /// TOTAL electrode area of the cell in m² — every layer summed, not one
    /// layer's footprint. Drives the dendrite current-density criterion.
    /// Defaults to a single ~300 cm² electrode; a stacked cell must set it.
    #[serde(default = "default_electrode_area")]
    pub electrode_area_m2: f32,
    /// Lumped heat capacity, J/K = cell mass x specific heat. 0.0 keeps the
    /// tick's legacy 625.5 J/K, which is right only for a 0.695 kg pouch.
    #[serde(default)]
    pub thermal_mass_j_per_k: f32,
    /// Cell-to-ambient thermal resistance, K/W. Sets the steady-state rise
    /// dT = Q x R. 0.0 keeps the tick's legacy 2.0 K/W.
    #[serde(default)]
    pub thermal_resistance_k_per_w: f32,
    /// Standard cell potential of this cell's couple, V. 0.0 keeps the tick's
    /// legacy Na-S 2.23 V, which is wrong for any cell that is not Na-S.
    #[serde(default)]
    pub standard_potential_v: f32,
    /// Entropic coefficient dE/dT of this couple, V/K. 0.0 keeps the legacy Na-S value.
    #[serde(default)]
    pub entropy_coefficient_v_per_k: f32,

    // ── Cycle life ────────────────────────────────────────────────────
    // These belong in the instance file rather than anywhere else, because a
    // Space is a git repository: authoring them here puts the design and the
    // telemetry it produces in the same commit, so a branch-per-variant sweep
    // diffs the change and its outcome as one artifact.
    /// Critical plating current density, A/m2. 0.0 falls back to a
    /// Monroe-Newman estimate built from Na/oxide constants.
    #[serde(default)]
    pub j_crit_a_per_m2: f32,
    /// Coulombic efficiency at reference conditions, 0-1. 0.0 keeps 0.995.
    #[serde(default)]
    pub coulombic_efficiency_ref: f32,
    /// Excess metal carried as a reservoir, fraction of nominal capacity.
    /// 0.0 is anode-free: the first metal lost to interphase is capacity lost.
    #[serde(default)]
    pub li_reservoir_frac: f32,
    /// Stack pressure, MPa. 0.0 keeps 2.0.
    #[serde(default)]
    pub stack_pressure_mpa: f32,
    /// Lithium consumed forming interphase per cycle on a smooth deposit, nm.
    /// Set this and coulombic loss is DERIVED from roughness, areal capacity
    /// and stack pressure instead of taken from a fitted efficiency.
    /// 0.0 falls back to `coulombic_efficiency_ref`.
    #[serde(default)]
    pub sei_thickness_nm: f32,
    /// Roughness prefactor. 0.0 keeps the value calibrated so that 2 MPa at the
    /// plating limit reproduces the measured 0.995.
    #[serde(default)]
    pub roughness_k: f32,
    /// Calendar fade coefficient: capacity lost per sqrt of equivalent hours
    /// at rest. 0.0 keeps ~2 %/yr at 25 C and half charge.
    #[serde(default)]
    pub calendar_k: f32,
    /// Cathode fatigue coefficient: structural damage per cycle at unit depth.
    /// 0.0 keeps ~20 % loss over 1000 full-depth cycles.
    #[serde(default)]
    pub crack_k: f32,
    /// Pressure above which lithium extrudes rather than densifies, MPa.
    /// 0.0 keeps the literature value of about 1 MPa.
    #[serde(default)]
    pub creep_threshold_mpa: f32,
    /// Creep strain per hour at twice the threshold. 0.0 keeps a prefactor that
    /// places the knee near 9 MPa, which is an assumption, not a measurement.
    #[serde(default)]
    pub creep_k: f32,
    /// Activation energy for ionic conduction, eV. 0.0 keeps the Li6PS5Cl
    /// value of 0.35, about one decade per 40 K.
    #[serde(default)]
    pub resistance_activation_ev: f32,
    /// Ambient temperature, K. 0.0 keeps 298.15.
    #[serde(default)]
    pub ambient_temperature_k: f32,
    /// Poisson ratio of the plated metal. 0.0 drives creep with the full stack
    /// pressure, as an unconfined billet; 0.36 is lithium and applies the
    /// oedometric correction, which at an exponent of 6.6 is worth 78x on rate.
    #[serde(default)]
    pub creep_poisson: f32,
    /// Void the deposit can creep into before it bears on the separator, as a
    /// fraction of plated thickness. 0.0 charges every micron from hour one.
    #[serde(default)]
    pub creep_accommodation_frac: f32,
    /// Activation energy for creep, eV. 0.0 keeps the shared legacy ramp.
    #[serde(default)]
    pub creep_activation_ev: f32,
    /// Activation energy for calendar interphase growth, eV. 0.0 keeps the
    /// shared legacy ramp.
    #[serde(default)]
    pub calendar_activation_ev: f32,
    /// Separator thickness, um. Enables both the bridging channel and the
    /// rate-dependent transport limit. 0.0 disables both.
    #[serde(default)]
    pub separator_thickness_um: f32,
    /// Plated metal thickness at full charge, um.
    #[serde(default)]
    pub plated_thickness_um: f32,
    /// Fraction of extruded metal that penetrates the separator. 0.0 keeps 0.25.
    #[serde(default)]
    pub bridge_chi: f32,
    /// Weibull shape on penetration fraction. 0.0 keeps 4.0.
    #[serde(default)]
    pub bridge_weibull_m: f32,
    /// Penetration fraction at which a layer is expected to bridge. 0.0 keeps 0.35.
    #[serde(default)]
    pub bridge_scale: f32,
    /// Layers in a bipolar series stack. 0 models the cell as its parallel
    /// equivalent and skips every series-only mechanism.
    #[serde(default)]
    pub layer_count: u32,
    /// Exponent on stack pressure in the cathode-fatigue term. Negative means
    /// pressure protects the cathode. 0.0 keeps it pressure-blind.
    #[serde(default)]
    pub crack_pressure_exponent: f32,
    /// How strongly interphase age raises the plating loss rate. 0.0 keeps
    /// calendar fade and plating loss independent of one another.
    #[serde(default)]
    pub calendar_roughness_beta: f32,
    /// Total cell mass, kg. Set it and the tick publishes specific energy from
    /// the same state that produces the life numbers.
    #[serde(default)]
    pub cell_mass_kg: f32,
}

fn default_voltage() -> f32 { 2.23 }
fn default_electrode_area() -> f32 { 0.03 }

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
            electrode_area_m2: self.electrode_area_m2,
            cycle_accum: 0.0,
            j_crit_a_per_m2: self.j_crit_a_per_m2,
            coulombic_efficiency_ref: self.coulombic_efficiency_ref,
            li_reservoir_frac: self.li_reservoir_frac,
            stack_pressure_mpa: self.stack_pressure_mpa,
            sei_thickness_nm: self.sei_thickness_nm,
            roughness_k: self.roughness_k,
            calendar_k: self.calendar_k,
            calendar_hours_equiv: 0.0,
            crack_k: self.crack_k,
            crack_damage: 0.0,
            creep_threshold_mpa: self.creep_threshold_mpa,
            creep_k: self.creep_k,
            creep_strain: 0.0,
            resistance_activation_ev: self.resistance_activation_ev,
            resistance_effective: 0.0,
            ambient_temperature_k: self.ambient_temperature_k,
            creep_poisson: self.creep_poisson,
            creep_accommodation_frac: self.creep_accommodation_frac,
            creep_accommodated: 0.0,
            creep_activation_ev: self.creep_activation_ev,
            calendar_activation_ev: self.calendar_activation_ev,
            separator_thickness_um: self.separator_thickness_um,
            plated_thickness_um: self.plated_thickness_um,
            bridge_chi: self.bridge_chi,
            bridge_weibull_m: self.bridge_weibull_m,
            bridge_scale: self.bridge_scale,
            layer_count: self.layer_count,
            shorted_layers: 0.0,
            crack_pressure_exponent: self.crack_pressure_exponent,
            calendar_roughness_beta: self.calendar_roughness_beta,
            cell_mass_kg: self.cell_mass_kg,
            reservoir_mass_kg: 0.0,
            li_inventory_lost: 0.0,
            soc_turn: 1.0,
            excursion_depth: 0.0,
            thermal_mass_j_per_k: self.thermal_mass_j_per_k,
            thermal_resistance_k_per_w: self.thermal_resistance_k_per_w,
            standard_potential_v: self.standard_potential_v,
            entropy_coefficient_v_per_k: self.entropy_coefficient_v_per_k,
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
    let registry = eustress_common::class_schema::ClassSchemaRegistry::builtin();
    let healed = eustress_common::class_schema::load_and_heal_instance(toml_path, registry)
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
/// Typed definition from an already-parsed document (the bulk loader's
/// pre-parse pool produces the value; this is the only other work left).
pub fn load_instance_definition_from_value(
    value: toml::Value,
) -> Result<InstanceDefinition, String> {
    let registry = eustress_common::class_schema::ClassSchemaRegistry::builtin();
    let healed = eustress_common::class_schema::heal_instance_value(value, registry)
        .map_err(|e| format!("schema heal (from value): {}", e))?;
    let instance: InstanceDefinition = healed
        .value
        .try_into()
        .map_err(|e: toml::de::Error| format!("deserialize merged (from value): {}", e))?;
    Ok(instance)
}

pub fn load_instance_definition_from_str(
    content: &str,
) -> Result<InstanceDefinition, String> {
    let registry = eustress_common::class_schema::ClassSchemaRegistry::builtin();
    let healed = eustress_common::class_schema::heal_instance_from_str(content, registry)
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

/// The unit a file's numbers are in, as the loader reads it at spawn: its
/// `[metadata] unit`, metres when that is missing or unknown.
pub fn file_unit(symbol: Option<&str>) -> eustress_common::units::Unit {
    symbol
        .and_then(eustress_common::units::Unit::from_symbol)
        .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT)
}

/// A length in engine metres as a file written in `unit` holds it: the
/// inverse of the loader's conversion at spawn, under the same `units_v1`
/// gate, so the file reads back to the same place.
pub fn authored_vec3(v: Vec3, unit: eustress_common::units::Unit) -> [f32; 3] {
    #[cfg(feature = "units_v1")]
    {
        eustress_common::units::engine_to_authored_vec3_f32(v.to_array(), unit)
    }
    #[cfg(not(feature = "units_v1"))]
    {
        let _ = unit;
        v.to_array()
    }
}

/// Set a loaded definition's `[transform]` from an entity's pose (engine
/// metres) and, when given, its size, in the unit its file is written in
/// (`[metadata] unit`). Every save of a transform into a definition goes
/// through here: `load_instance_definition` returns a file's numbers as
/// written, and `write_instance_definition` writes them as they are, so a
/// pose assigned in metres into a file in feet would read back 3.28 times too
/// small. `size` `None` keeps the file's `scale` (a custom mesh's multiplier).
pub fn set_authored_transform(def: &mut InstanceDefinition, translation: Vec3, rotation: Quat, size: Option<Vec3>) {
    let unit = file_unit(def.metadata.unit.as_deref());
    def.transform.position = authored_vec3(translation, unit);
    def.transform.rotation = [rotation.x, rotation.y, rotation.z, rotation.w];
    if let Some(size) = size {
        def.transform.scale = authored_vec3(size, unit);
    }
}

/// [`set_authored_transform`] for a file held as a TOML document (a pasted
/// copy, a placed quad): its own `[metadata] unit` decides, and every other
/// key is kept.
pub fn set_authored_transform_toml(
    doc: &mut toml::Value,
    translation: Vec3,
    rotation: Quat,
    size: Option<Vec3>,
) -> Result<(), String> {
    let unit = file_unit(doc.get("metadata").and_then(|m| m.get("unit")).and_then(|u| u.as_str()));
    let float = |x: f32| toml::Value::Float(format!("{x}").parse::<f64>().unwrap_or(x as f64));
    let floats = |v: &[f32]| toml::Value::Array(v.iter().map(|x| float(*x)).collect());
    let tf = doc
        .as_table_mut()
        .ok_or("the document is not a table")?
        .entry("transform".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or("[transform] is not a table")?;
    tf.insert("position".into(), floats(&authored_vec3(translation, unit)));
    tf.insert("rotation".into(), floats(&[rotation.x, rotation.y, rotation.z, rotation.w]));
    if let Some(size) = size {
        tf.insert("scale".into(), floats(&authored_vec3(size, unit)));
    }
    Ok(())
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

    // Only what changed, in the file's own layout (`planned_write`).
    let toml_str = match planned_write(toml_path, instance)? {
        PlannedWrite::Unchanged => return Ok(()),
        PlannedWrite::Text(text) => text,
    };

    // Atomic write + retry on Windows file-lock races (file watcher
    // reload pass, antivirus scanning, text-editor reads).
    super::gui_loader::write_atomic(toml_path, toml_str.as_bytes())
        .map_err(|e| format!("Failed to write {}: {}", toml_path.display(), e))?;

    Ok(())
}

/// What a typed write does to its file (see [`planned_write`]).
enum PlannedWrite {
    /// The file already holds this instance: nothing is written.
    Unchanged,
    /// The file's new text.
    Text(String),
}

/// The text a typed write leaves in `toml_path`. A file the instance was read
/// from is edited in place: only the values that differ from how the file
/// loads now are written, in the file's own layout (its comments, key order,
/// inline arrays, a colour's float or 0 to 255 form), each float in its
/// shortest `f32` form. So saving an unchanged instance leaves the file byte
/// for byte, and moving one changes only its position. The typed model is
/// authoritative for what it models, both ways: a value it clears is deleted.
/// A key it does not carry is never touched, and a key the file lacks is
/// added only when its value differs from what the file loads as. A new or
/// unreadable file gets the whole instance.
fn planned_write(toml_path: &Path, instance: &InstanceDefinition) -> Result<PlannedWrite, String> {
    let typed = typed_document(instance)?;
    if let Ok(text) = std::fs::read_to_string(toml_path) {
        let loaded = load_instance_definition_from_str(&text).ok().and_then(|d| typed_document(&d).ok());
        let colour = Some(instance.properties.color);
        if let Some(edit) = loaded.and_then(|old| edit_document(&text, &old, &typed, NAMED_SECTIONS, colour)) {
            return Ok(edit.map_or(PlannedWrite::Unchanged, PlannedWrite::Text));
        }
    }
    toml::to_string(&typed)
        .map(PlannedWrite::Text)
        .map_err(|e| format!("Failed to serialize instance: {}", e))
}

/// An instance as the table the writer compares and writes, each float in
/// its shortest `f32` form.
fn typed_document(instance: &InstanceDefinition) -> Result<toml::Table, String> {
    let value = toml::Value::try_from(instance).map_err(|e| format!("Failed to serialize instance: {}", e))?;
    match narrow_floats(value) {
        toml::Value::Table(table) => Ok(table),
        _ => Err("an instance serializes as a table".to_string()),
    }
}

/// The sections [`InstanceDefinition`] names as fields; every other top-level
/// key rides in its flattened `extra`. Only these are ever deleted from a
/// file, so a definition built without its file's unknown sections leaves
/// them. Keep in step with the struct (`the_named_sections_are_the_structs_fields`).
const NAMED_SECTIONS: &[&str] = &[
    "asset",
    "transform",
    "properties",
    "metadata",
    "material",
    "thermodynamic",
    "electrochemical",
    "plasma",
    "ui",
    "attributes",
    "tags",
    "parameters",
];

/// A float that is exactly an `f32` as the shortest decimal that reads back
/// as that `f32` (`113.6`, never `113.5999984741211`); any other float as it
/// is.
fn shortest_f32(f: f64) -> f64 {
    let narrow = f as f32;
    if narrow.is_finite() && narrow as f64 == f {
        narrow.to_string().parse().unwrap_or(f)
    } else {
        f
    }
}

/// Every float in `value` as [`shortest_f32`] makes it.
fn narrow_floats(value: toml::Value) -> toml::Value {
    match value {
        toml::Value::Float(f) => toml::Value::Float(shortest_f32(f)),
        toml::Value::Array(a) => toml::Value::Array(a.into_iter().map(narrow_floats).collect()),
        toml::Value::Table(t) => toml::Value::Table(t.into_iter().map(|(k, v)| (k, narrow_floats(v))).collect()),
        other => other,
    }
}

/// Whether two typed values are the same to the precision the model holds:
/// floats within one part in a million (an `f32` round trip through a unit
/// conversion), everything else exactly.
pub(crate) fn same_value(a: &toml::Value, b: &toml::Value) -> bool {
    match (a, b) {
        (toml::Value::Float(x), toml::Value::Float(y)) => x == y || (x - y).abs() <= 1e-6 * x.abs().max(y.abs()),
        (toml::Value::Array(x), toml::Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_value(p, q))
        }
        (toml::Value::Table(x), toml::Value::Table(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same_value(v, w)))
        }
        _ => a == b,
    }
}

/// Whether two keys name the same thing: equal but for case and underscores
/// (`class_name`, `ClassName`, `Class_Name`).
pub(crate) fn same_key(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.chars().filter(|c| *c != '_').flat_map(char::to_lowercase).collect::<String>();
    norm(a) == norm(b)
}

/// A typed value as a `toml_edit` value: inline, arrays on one line.
pub(crate) fn to_value(value: &toml::Value) -> toml_edit::Value {
    match value {
        toml::Value::String(s) => s.as_str().into(),
        toml::Value::Integer(i) => (*i).into(),
        toml::Value::Float(f) => (*f).into(),
        toml::Value::Boolean(b) => (*b).into(),
        toml::Value::Datetime(d) => d.to_string().parse().unwrap_or_else(|_| d.to_string().as_str().into()),
        toml::Value::Array(a) => toml_edit::Value::Array(a.iter().map(to_value).collect()),
        toml::Value::Table(t) => {
            toml_edit::Value::InlineTable(t.iter().map(|(k, v)| (k.as_str(), to_value(v))).collect())
        }
    }
}

/// A typed value as the item it becomes under a key: a table as a
/// `[section]` and a list of tables as `[[sections]]` in a standard table,
/// everything else (and everything inside an inline table) a value.
pub(crate) fn to_item(value: &toml::Value, inline: bool) -> toml_edit::Item {
    let table = |t: &toml::Table| {
        let mut out = toml_edit::Table::new();
        for (k, v) in t {
            out.insert(k, to_item(v, false));
        }
        out
    };
    match value {
        toml::Value::Table(t) if !inline => toml_edit::Item::Table(table(t)),
        toml::Value::Array(a) if !inline && !a.is_empty() && a.iter().all(toml::Value::is_table) => {
            let mut list = toml_edit::ArrayOfTables::new();
            for v in a {
                if let toml::Value::Table(t) = v {
                    list.push(table(t));
                }
            }
            toml_edit::Item::ArrayOfTables(list)
        }
        other => toml_edit::Item::Value(to_value(other)),
    }
}

/// A part's colour in the form its file already writes it: floats from 0 to
/// 1 when the file has floats (four channels when it had four, or when the
/// colour is not opaque), else the model's 0 to 255 integers.
fn colour_value(existing: Option<&toml_edit::Item>, typed: &toml::Value, rgba: [f32; 4]) -> toml_edit::Value {
    let array = existing.and_then(|i| i.as_array());
    let floats = array.is_some_and(|a| a.iter().any(|v| v.as_float().is_some()));
    if !floats {
        return to_value(typed);
    }
    let channels = if array.is_some_and(|a| a.len() == 4) || rgba[3] != 1.0 { 4 } else { 3 };
    toml_edit::Value::Array(rgba[..channels].iter().map(|c| shortest_f32(*c as f64)).collect())
}

/// Writes into `file` every value where `new` differs from `old` (how the
/// file loads now), recursing into tables, and deletes what `new` cleared.
/// Unchanged values keep their text. A table the file lacks is added only
/// when a value in it changed, holding only the changed values. Keys match
/// the file's in any case or underscore style. At the top level only a
/// section in `named` is ever deleted, since a definition built without its
/// file's unknown sections must not remove them. Returns whether anything
/// changed.
fn apply_changes(
    file: &mut dyn toml_edit::TableLike,
    old: &toml::Table,
    new: &toml::Table,
    path: &str,
    inline: bool,
    named: &[&str],
    colour: Option<[f32; 4]>,
) -> bool {
    let mut keys: Vec<&String> = old.keys().chain(new.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut changed = false;
    for key in keys {
        let here = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
        let file_key = file.iter().map(|(k, _)| k.to_string()).find(|k| same_key(k, key));
        match (old.get(key), new.get(key)) {
            (Some(o), Some(n)) if same_value(o, n) => {}
            (Some(toml::Value::Table(o)), Some(toml::Value::Table(n))) => {
                let name = file_key.unwrap_or_else(|| key.clone());
                match file.get_mut(&name) {
                    Some(item) if item.is_table_like() => {
                        let sub_inline = inline || item.is_inline_table();
                        if let Some(sub) = item.as_table_like_mut() {
                            changed |= apply_changes(sub, o, n, &here, sub_inline, named, colour);
                        }
                    }
                    _ => {
                        // The file lacks it: a new table holding only what changed.
                        let mut fresh = toml_edit::Table::new();
                        if apply_changes(&mut fresh, o, n, &here, inline, named, colour) {
                            let item = if inline {
                                toml_edit::Item::Value(toml_edit::Value::InlineTable(fresh.into_inline_table()))
                            } else {
                                toml_edit::Item::Table(fresh)
                            };
                            file.insert(&name, item);
                            changed = true;
                        }
                    }
                }
            }
            (_, Some(n)) => {
                let name = file_key.unwrap_or_else(|| key.clone());
                let existing = file.get(&name);
                let mut item = match colour {
                    Some(rgba) if here == "properties.color" => toml_edit::Item::Value(colour_value(existing, n, rgba)),
                    _ => to_item(n, inline),
                };
                // A replaced value keeps its comments.
                if let (Some(old_value), Some(new_value)) = (existing.and_then(|i| i.as_value()), item.as_value_mut()) {
                    *new_value.decor_mut() = old_value.decor().clone();
                }
                file.insert(&name, item);
                changed = true;
            }
            (Some(_), None) => {
                let deletable = !path.is_empty() || named.iter().any(|s| same_key(s, key));
                if let (true, Some(name)) = (deletable, file_key) {
                    file.remove(&name);
                    changed = true;
                }
            }
            (None, None) => {}
        }
    }
    changed
}

/// `text` edited to hold `new`, where `old` is how `text` loads now:
/// `Some(None)` when nothing differs (the file is left byte for byte),
/// `Some(Some(text))` with only the changed values rewritten, `None` when
/// `text` is not a TOML document.
fn edit_document(
    text: &str,
    old: &toml::Table,
    new: &toml::Table,
    named: &[&str],
    colour: Option<[f32; 4]>,
) -> Option<Option<String>> {
    let mut doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let changed = apply_changes(doc.as_table_mut(), old, new, "", false, named, colour);
    Some(changed.then(|| doc.to_string()))
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
        first_timestamp: None,
        saves: 1,
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
    // An instance its file already holds is not written and gains no stamp:
    // a save without an edit changes nothing, and the audit chain records
    // only real modifications.
    if matches!(planned_write(toml_path, instance)?, PlannedWrite::Unchanged) {
        return Ok(());
    }
    match stamp {
        Some(s) => {
            if instance.metadata.created_by.is_none() {
                instance.metadata.created_by = Some(s.clone());
            }
            record_modification(&mut instance.metadata.modifications, s);
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
    // One reading, shared with a Player's reader of the same records.
    eustress_common::datamodel::record::toml_to_attribute(v)
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
// ORDER MATTERS: the lookup takes the FIRST hint that appears anywhere in the
// mesh filename, so any hint that is a substring of another must come first.
// `corner_wedge` contains `wedge`, so listing `wedge` first would classify
// every `corner_wedge.glb` as `PartType::Wedge` — wrong mesh semantics, wrong
// Avian collider, and wrong replacement mesh when the scale tool rebuilds it.
const PRIMITIVE_MESHES: &[(&str, &str, eustress_common::classes::PartType)] = &[
    ("corner_wedge", "parts/corner_wedge.glb", eustress_common::classes::PartType::CornerWedge),
    ("block", "parts/block.glb", eustress_common::classes::PartType::Block),
    ("ball", "parts/ball.glb", eustress_common::classes::PartType::Ball),
    ("cylinder", "parts/cylinder.glb", eustress_common::classes::PartType::Cylinder),
    ("wedge", "parts/wedge.glb", eustress_common::classes::PartType::Wedge),
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

    // Parse the class name early; the no-mesh branch needs it too. A class
    // the engine does not know loads as a Part, and the log says so once per
    // class name. A file with no class_name reads as "Part" and never gets here.
    let class_name = eustress_common::classes::ClassName::from_str(&instance.metadata.class_name)
        .unwrap_or_else(|_| {
            eustress_common::datamodel::record::warn_unknown_class(
                &instance.metadata.class_name,
                eustress_common::classes::ClassName::Part,
            );
            eustress_common::classes::ClassName::Part
        });

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
    // + [properties] but no [asset], and the Roblox importer writes none for a
    // block-shaped Seat, VehicleSeat or SpawnLocation. Without this, those
    // entities hit the non-visual branch and are invisible. Every part class
    // (`record::loads_as_part`), as a Player's reader defaults them.
    let mut instance = instance;
    if instance.asset.is_none() && eustress_common::datamodel::record::loads_as_part(class_name) {
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
    // Converted to meters below when `units_v1` is on (it is a length).
    #[allow(unused_mut)]
    let mut mesh_visual_offset = take_vec3_extra(&mut instance.extra, "mesh_offset");

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
            // `mesh_offset` (a DataMesh `Offset`) is a length in the file's
            // authored unit, and it is added to the already-converted
            // translation below, so it must be converted too. Left in feet,
            // every imported SpecialMesh offset landed 3.28x too far.
            // `mesh_scale` is a ratio and needs no conversion.
            if let Some(mo) = mesh_visual_offset {
                mesh_visual_offset = Some(Vec3::from_array(
                    eustress_common::units::convert_vec3_f32(mo.to_array(), from_unit, to_unit),
                ));
            }
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
            // A class's own field table is read into its component (below),
            // never folded into user attributes: `[gaussian_splats]` (a
            // SplatCloud, surfaced as built-in Appearance properties), the
            // particle and terrain-layer sections, a light's `[light]` and a
            // KeyframeSequence's `[keyframe_sequence]`. One list, which a
            // Player's tree reader shares, so both trees agree.
            if eustress_common::datamodel::record::class_owned_section(class_name, _section_name) {
                continue;
            }
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
        // A Sound's component from its `[sound]` section.
        crate::spawners::audio_vfx::sound::attach_sound_component(&mut commands.entity(entity), class_name, &instance.extra);
        attach_class_props(
            &mut commands.entity(entity),
            class_name,
            false,
            &instance.extra,
            instance.metadata.unit.as_deref(),
        );
        crate::particles::bridge::attach_class_component(
            &mut commands.entity(entity),
            class_name,
            &instance.extra,
            &toml_path,
        );
        crate::terrain_layers::attach_class_component(
            &mut commands.entity(entity),
            class_name,
            &instance.extra,
            &toml_path,
        );
        // PointLight / SpotLight / SurfaceLight / DirectionalLight: the class
        // component from `[light]`, which `light_classes` turns into a Bevy
        // light. Without it a light created at runtime (Insert, Toolbox, MCP)
        // stayed dark until the Space was reopened.
        if eustress_common::plugins::light_classes::is_light_class(class_name) {
            let section = instance
                .extra
                .get("light")
                .or_else(|| instance.extra.get("Light"));
            let light = eustress_common::plugins::light_classes::LightSection::from_parts(section, None);
            if let Some(component) =
                eustress_common::plugins::light_classes::LightComponent::from_section(class_name, &light)
            {
                component.insert(&mut commands.entity(entity));
            }
        }
        // Star / Moon / Sky / Atmosphere / Clouds: the class component from
        // its own section (`[star]`, `[moon]`, `[sky]`, `[atmosphere]`,
        // `[clouds]`), so the
        // renderer and the Properties panel read what the file says. The
        // lighting hydration adds a Star's and a Moon's light and marker
        // around the component and keeps its values.
        if let Some(own) = eustress_common::plugins::celestial_sections::section_name(class_name) {
            let section = instance.extra.get(own).or_else(|| {
                instance
                    .extra
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(own))
                    .map(|(_, v)| v)
            });
            eustress_common::plugins::celestial_sections::insert_class_component(
                &mut commands.entity(entity),
                class_name,
                section,
            );
            if matches!(
                class_name,
                eustress_common::classes::ClassName::Sky
                    | eustress_common::classes::ClassName::Atmosphere
                    | eustress_common::classes::ClassName::Clouds
            ) {
                commands
                    .entity(entity)
                    .insert(crate::plugins::lighting_plugin::LightingServiceOwner);
            }
        }
        // A Decal without a mesh of its own projects from its own Transform
        // (one placed on the terrain). `decal_place_tool::sync_standalone_decals`
        // draws it from this component with the live decal material store,
        // which not every caller of this spawn path can hand over.
        if matches!(class_name, eustress_common::classes::ClassName::Decal) {
            if let Some(sec) = section_table(&instance.extra, "decal") {
                commands.entity(entity).insert(decal_from_section(sec));
            }
        }
        // A Texture has no mesh of its own either: it tiles its image over a
        // face of its parent part, drawn by `decal_place_tool::
        // sync_texture_surfaces` from this component. Only the mesh branches
        // attached it, and a Texture never has an `[asset]` mesh, so every
        // texture loaded from disk, placed in Studio or imported, drew
        // nothing.
        if matches!(class_name, eustress_common::classes::ClassName::Texture) {
            if let Some(sec) = section_table(&instance.extra, "texture") {
                commands.entity(entity).insert(texture_from_section(sec));
            }
        }
        // GaussianSplats: attach the real radiance-field rendering components
        // (splats have no [asset], so they land in this no-mesh branch too).
        #[cfg(feature = "gaussian-splatting")]
        if matches!(class_name, eustress_common::classes::ClassName::GaussianSplats) {
            attach_gaussian_splat_component(
                &mut commands.entity(entity),
                asset_server,
                &toml_path,
                &instance.extra,
            );
        }
        // A DataMesh (SpecialMesh, BlockMesh, CylinderMesh, FileMesh) never has
        // an `[asset]` of its own, so it always lands here: its component from
        // its `[mesh]`, by the builder a Player's reader shares.
        if eustress_common::datamodel::record::is_data_mesh_class(class_name) {
            let section = instance.extra.iter().find(|(k, _)| k.eq_ignore_ascii_case("mesh")).map(|(_, v)| v);
            eustress_common::datamodel::record::insert_data_mesh(&mut commands.entity(entity), class_name, section);
        }
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

    // Build BasePart so the Properties panel can read/display part properties.
    // Its density (kg/m3) is the one every collider's mass comes from: the
    // file's own override, else the material's.
    let part_material = eustress_common::classes::Material::from_string(&instance.properties.material);
    let part_density = part_density_kg_m3(&part_material, instance.properties.physics.as_ref());
    let base_part = eustress_common::classes::BasePart {
        size: scale,
        color: Color::srgba(r, g, b, a),
        transparency,
        reflectance: instance.properties.reflectance,
        anchored: instance.properties.anchored,
        can_collide: instance.properties.can_collide,
        locked: instance.properties.locked,
        cast_shadow: instance.properties.cast_shadow,
        material: part_material,
        material_name: instance.properties.material.clone(),
        density: part_density,
        mass: part_density * scale.x * scale.y * scale.z,
        custom_physical_properties: part_physical_override(instance.properties.physics.as_ref()),
        cframe: Transform::from(safe_instance_transform.clone()),
        respect_gltf_materials: instance.properties.respect_gltf_materials,
        destructible: instance.properties.destructible,
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
        // Domain-scoped Parameters, mirrored from `[parameters]` on disk so the
        // live component is the faithful in-memory copy — the same contract
        // `attributes_from_toml_table` establishes for Attributes. Inserted
        // here rather than in the spawn bundle to stay clear of Bevy's tuple
        // arity limit.
        ec.insert(super::parameters_runtime::loaded_parameters(
            instance.parameters.as_ref(),
            instance.extra.get("parameter_bindings"),
        ));
        ec.insert(part_visibility_range(part_half_extent(scale)));

        // Only add physics collider when can_collide is true — avoids broadphase
        // overhead for thousands of static decorative parts.
        // GLB meshes are unit meshes ([-0.5, 0.5]), so Transform.scale = part size in studs.
        // Avian3D colliders take HALF-extents for cuboid and HALF-height for cylinder.
        //
        // Perf QW5 → collider-streaming tier (same huge-scene gate as the
        // primitive branch below): on a "huge" Space (`streaming_active()`
        // true) don't attach the Static body + Collider to these anchored
        // decorative custom-mesh parts up front — a 387K+ import would flood
        // Avian's broadphase. Instead stash a `DeferredCollider` descriptor;
        // `crate::physics::collider_streaming` materializes the real
        // Collider + RigidBody::Static only while the part is near physics
        // activity (an awake dynamic body, the player, or the camera),
        // keeping the resident collider count bounded regardless of scene
        // size. Reversible + scoped: no-op (gate is false) for normal scenes
        // and when `world-db` is off — those keep the eager path unchanged.
        // All parts get real colliders (defer only on world-db streamed
        // worlds — see defer_collider_for_huge_scene).
        if instance.properties.can_collide {
            if defer_collider_for_huge_scene() {
                ec.insert(crate::physics::collider_streaming::DeferredCollider {
                    part_shape,
                    size: scale,
                    is_static: true,
                    physics: instance.properties.physics.clone(),
                });
            } else if let Some(collider) = safe_collider_from(part_shape, scale, &render_transform) {
                // Validate `render_transform` — the transform the ENTITY actually
                // carries (and thus what Avian reads via GlobalTransform), NOT the
                // unadjusted `transform`. `render_transform` folds in a possibly-
                // negative `mesh_visual_scale`; validating it here is what catches
                // the mirrored-mesh collider AABB panic.
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
    // See the sibling spawn path above: Parameters mirror `[parameters]` from
    // disk into the live component.
    ec.insert(super::parameters_runtime::loaded_parameters(
        instance.parameters.as_ref(),
        instance.extra.get("parameter_bindings"),
    ));
    ec.insert(part_visibility_range(part_half_extent(scale)));

    // Only add physics collider when can_collide is true — avoids broadphase
    // overhead for thousands of static decorative parts.
    // Avian3D colliders take HALF-extents for cuboid and HALF-height for cylinder.
    //
    // Perf QW5 → collider-streaming tier. Every part spawned here is
    // anchored (`RigidBody::Static`), i.e. decorative collision geometry. On
    // a "huge" Space (the residency boot-load flipped `streaming_active()`
    // true for a large binary-ECS / streamed place — e.g. a 387K-part
    // Roblox import), eagerly attaching a Static body + Collider to hundreds
    // of thousands of parts floods Avian's broadphase and the per-frame
    // rigid-body transform walk for no gameplay benefit most of the time.
    // Instead of skipping physics for these parts entirely (the old
    // all-or-nothing gate), stash a `DeferredCollider` descriptor —
    // `crate::physics::collider_streaming` materializes the real collider
    // only while the part is near physics activity (an awake dynamic body,
    // the player, or the camera) and dematerializes it once it drifts far
    // away, keeping the resident collider count bounded (~10K) regardless
    // of scene size. The gate is reversible and scoped: `streaming_active()`
    // is `false` for normal-sized scenes and whenever the `world-db` feature
    // is off, so non-huge worlds attach colliders exactly as before (zero
    // behavior change).
    // All parts get real colliders (defer only on world-db streamed
    // worlds — see defer_collider_for_huge_scene).
    if instance.properties.can_collide {
        if defer_collider_for_huge_scene() {
            ec.insert(crate::physics::collider_streaming::DeferredCollider {
                part_shape,
                size: scale,
                is_static: true,
                physics: instance.properties.physics.clone(),
            });
        } else if let Some(collider) = safe_collider_from(part_shape, scale, &render_transform) {
            // Validate `render_transform` (the entity's actual transform / what
            // Avian reads), not the unadjusted `transform` — see the custom-mesh
            // branch above for why a negative folded scale panics Avian at insert.
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
    } else if instance.properties.destructible {
        // A part that opted in to deformation but authored no `[material]`
        // block still needs mechanical constants, and the ones that matter are
        // already implied by the material it says it is made of. Deriving them
        // here is what lets an agent write `material = "Concrete"` and get
        // concrete's behaviour, instead of having to supply a fracture
        // toughness — a number it has no business guessing, since the crack
        // threshold goes as K_IC squared.
        //
        // Without this the contact model fell back to generic plastic
        // constants for EVERY part with no explicit block, so a concrete slab
        // and a steel plate deformed identically.
        if let Some(props) = eustress_common::realism::materials::properties::MaterialProperties
            ::from_name(&instance.properties.material)
        {
            debug!(
                "  + MaterialProperties: {} (derived from material = {:?})",
                props.name, instance.properties.material
            );
            ec.insert(props);
        }
    }
    if let Some(ref thermo) = instance.thermodynamic {
        ec.insert(thermo.to_component());
        debug!("  + ThermodynamicState: T={:.1}K P={:.0}Pa", thermo.temperature, thermo.pressure);
    }
    if let Some(ref echem) = instance.electrochemical {
        ec.insert(echem.to_component());
        debug!("  + ElectrochemicalState: V={:.2}V SOC={:.1}%", echem.voltage, echem.soc * 100.0);
    }
    if let Some(ref plasma) = instance.plasma {
        ec.insert(plasma.to_component());
        debug!("  + PlasmaState: ne={:.1e} Te={:.1e}K", plasma.electron_density, plasma.electron_temperature_k);
    }
    // Attach UI ECS component if this is a UI class
    attach_ui_component(&mut ec, class_name, instance.ui.as_ref());
    attach_spawn_location(&mut ec, class_name, &instance.extra);
    attach_class_props(&mut ec, class_name, is_custom_mesh, &instance.extra, instance.metadata.unit.as_deref());
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
/// A SpawnLocation is a Part plus the `SpawnLocation` component Play looks
/// for when it places the player, read from the `[spawn]` section.
fn attach_spawn_location(
    ec: &mut bevy::ecs::system::EntityCommands,
    class_name: eustress_common::classes::ClassName,
    extra: &std::collections::HashMap<String, toml::Value>,
) {
    if class_name != eustress_common::classes::ClassName::SpawnLocation {
        return;
    }
    let mut spawn = eustress_common::classes::SpawnLocation::default();
    if let Some(sec) = section_table(extra, "spawn") {
        if let Some(v) = sec.get("enabled").and_then(|v| v.as_bool()) {
            spawn.enabled = v;
        }
        if let Some(v) = sec.get("neutral").and_then(|v| v.as_bool()) {
            spawn.neutral = v;
        }
        if let Some(v) = sec.get("allow_team_change_on_touch").and_then(|v| v.as_bool()) {
            spawn.allow_team_change = v;
        }
        if let Some(v) = toml_f32(sec.get("duration")) {
            spawn.spawn_protection_duration = v.max(0.0);
        }
        if let Some(v) = sec.get("team_color").and_then(|v| v.as_str()) {
            spawn.team_name = v.to_string();
        }
    }
    ec.insert(spawn);
}

/// A Seat's or VehicleSeat's own settings (`[seat]`, `[vehicle]`), read by the
/// conversion a Player's tree reader uses too and kept for Studio's Play seed.
/// A class's own properties in the tree (`RecordClassProps`), read from its
/// file's class sections by the rules a Player's reader shares
/// (`record::record_class_props`, keyed by the class's tree name): a seat's,
/// a KeyframeSequence's, a Luau script's `ScriptOrigin`. Every flat file goes
/// through here, so a class added to `record_class_props` needs nothing on
/// Studio's side. A file with no section of its own has none.
fn attach_class_props(
    ec: &mut bevy::ecs::system::EntityCommands,
    class_name: eustress_common::classes::ClassName,
    custom_mesh: bool,
    extra: &std::collections::HashMap<String, toml::Value>,
    unit: Option<&str>,
) {
    use eustress_common::datamodel::record::{record_class_props, tree_class, RecordClassProps};
    if extra.is_empty() {
        return;
    }
    // The file's unit too: a seat's speed is in its length unit a second.
    let mut doc: toml::Table = extra.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    if let Some(unit) = unit {
        let mut metadata = toml::Table::new();
        metadata.insert("unit".to_string(), toml::Value::String(unit.to_string()));
        doc.insert("metadata".to_string(), toml::Value::Table(metadata));
    }
    let props = record_class_props(&tree_class(class_name, custom_mesh), &toml::Value::Table(doc));
    if !props.is_empty() {
        ec.insert(RecordClassProps(props));
    }
}

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

/// A `Decal` from its `[decal]` section, the importer's and the Studio's
/// alike. A missing key keeps the class default; `depth_fade_factor` is
/// written by the terrain placement, which sizes it to the ground's relief.
pub(crate) fn decal_from_section(sec: &toml::value::Table) -> eustress_common::classes::Decal {
    let defaults = eustress_common::classes::Decal::default();
    eustress_common::classes::Decal {
        texture: sec
            .get("texture")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        face: sec
            .get("face")
            .and_then(|v| v.as_str())
            .map(face_from_str)
            .unwrap_or(defaults.face),
        transparency: toml_f32(sec.get("transparency")).unwrap_or(defaults.transparency),
        depth_fade_factor: toml_f32(sec.get("depth_fade_factor")).unwrap_or(defaults.depth_fade_factor),
        color: color_u8_array_to_rgba(sec.get("color"), defaults.color),
        z_index: sec
            .get("z_index")
            .and_then(|v| v.as_integer())
            .map_or(defaults.z_index, |z| z as i32),
    }
}

/// A `Texture` from its `[texture]` section. A missing key keeps the class
/// default. Shared by the no-mesh spawn path and the mesh branches.
pub(crate) fn texture_from_section(sec: &toml::value::Table) -> eustress_common::classes::Texture {
    let mut t = eustress_common::classes::Texture::default();
    if let Some(s) = sec.get("texture").and_then(|v| v.as_str()) { t.texture = s.to_string(); }
    if let Some(s) = sec.get("face").and_then(|v| v.as_str()) { t.face = s.to_string(); }
    if let Some(v) = toml_f32(sec.get("studs_per_tile_u")) { t.studs_per_tile_u = v; }
    if let Some(v) = toml_f32(sec.get("studs_per_tile_v")) { t.studs_per_tile_v = v; }
    if let Some(v) = toml_f32(sec.get("offset_studs_u")) { t.offset_studs_u = v; }
    if let Some(v) = toml_f32(sec.get("offset_studs_v")) { t.offset_studs_v = v; }
    if let Some(rgb) = sec.get("color3").or_else(|| sec.get("color")) {
        let c = color_u8_array_to_rgba(Some(rgb), [1.0, 1.0, 1.0, 1.0]);
        t.color3 = [c[0], c[1], c[2]];
    }
    if let Some(v) = toml_f32(sec.get("transparency")) { t.transparency = v; }
    t
}

/// Attach the Decal / DataMesh component from the importer-written
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
    use eustress_common::classes::{ClassName, Instance};
    match class_name {
        ClassName::Decal => {
            let Some(sec) = section_table(extra, "decal") else { return; };
            let decal = decal_from_section(sec);
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
        ClassName::Texture => {
            // A Texture tiles an image across one face of its parent part.
            // We only attach the `Texture` component here (onto the instance
            // entity itself, which is already `ChildOf` the part); the
            // dedicated `sync_texture_surfaces` system lazily builds the
            // tiled quad visual + drives the live UV mapping every frame, so
            // this loader path needs no mesh/material assets. See
            // `decal_place_tool::sync_texture_surfaces`.
            let Some(sec) = section_table(extra, "texture") else { return; };
            commands.entity(host).insert(texture_from_section(sec));
            extra.remove("texture");
            extra.remove("Texture");
        }
        c if eustress_common::datamodel::record::is_data_mesh_class(c) => {
            // SpecialMesh, BlockMesh, CylinderMesh or FileMesh: its component
            // from its `[mesh]`, by the builder a Player's reader shares.
            let key = extra.keys().find(|k| k.eq_ignore_ascii_case("mesh")).cloned();
            let section = key.and_then(|k| extra.remove(&k));
            eustress_common::datamodel::record::insert_data_mesh(&mut commands.entity(host), c, section.as_ref());
        }
        _ => {}
    }
}

/// Attach the actual radiance-field rendering components to a
/// `GaussianSplats` entity. Reads the Universe-relative splat path from the
/// importer's `[gaussian_splats].path` section (see
/// `file_event_handler::do_import_gaussian_splat` for why this is a
/// dedicated section rather than the generic `[asset]`/`AssetReference` —
/// `AssetReference.mesh` is required, so a path-only `[asset]` table would
/// fail to deserialize). `GaussianSplats` has no `[asset]`, so it hits the
/// same no-mesh branch as ParticleEmitter/Beam above; this is that branch's
/// twin, attaching REAL rendering (not stub data).
#[cfg(feature = "gaussian-splatting")]
fn attach_gaussian_splat_component(
    ec: &mut bevy::ecs::system::EntityCommands,
    asset_server: &AssetServer,
    toml_path: &Path,
    extra: &std::collections::HashMap<String, toml::Value>,
) {
    let Some(gs) = extra.get("gaussian_splats") else {
        // DIAG: a GaussianSplats reached the attach with NO gaussian_splats
        // section in its `extra` — this is the "empty reload" smoking gun (the
        // cloud path was stripped somewhere upstream, e.g. a component-rebuilt
        // core). `warn!` so it survives the engine's hardcoded log filter.
        warn!(
            "gaussian_splats ATTACH SKIPPED — no [gaussian_splats] in extra (path stripped upstream) for {:?}; extra keys = {:?}",
            toml_path,
            extra.keys().collect::<Vec<_>>()
        );
        return;
    };
    let Some(rel_path) = gs
        .get("path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    else {
        warn!("gaussian_splats ATTACH SKIPPED — [gaussian_splats] present but no usable path for {:?}", toml_path);
        return;
    };
    // Per-cloud correction toggles (Properties booleans). Absent ⇒ default ON,
    // matching `SplatCloud::default()`, so pre-existing imports light up both
    // passes without a schema migration.
    let cull_floaters = gs.get("cull_floaters").and_then(|v| v.as_bool()).unwrap_or(true);
    let ppisp = gs.get("ppisp").and_then(|v| v.as_bool()).unwrap_or(true);
    let Some(universe_root) = crate::space::universe_root_for_path(toml_path) else {
        warn!("gaussian_splats: could not resolve Universe root for {:?}", toml_path);
        return;
    };
    let abs_path = universe_root.join(rel_path);
    eustress_radiance::attach_splat_cloud(
        ec,
        asset_server,
        abs_path.to_string_lossy().to_string(),
        cull_floaters,
        ppisp,
    );
}

/// One-shot marker: the entity's radiance collider proxy has been converted to
/// a real Avian collider (so the conversion runs once).
#[cfg(feature = "gaussian-splatting")]
#[derive(bevy::prelude::Component)]
pub struct SplatColliderAttached;

/// Convert a splat cloud's radiance-extracted [`eustress_radiance::SplatColliderProxy`]
/// (Avian-free voxel boxes, in cloud-LOCAL space) into a REAL Avian compound
/// collider + a static body. This is the physics-engine-specific half that
/// radiance deliberately leaves to the engine.
///
/// Once attached, the cloud is a first-class physical object: things rest on
/// it, and — crucially — the click-pick's Avian `ray_hits` strikes THIS collider
/// (the precise voxel shell), so selection uses the real geometry, not the
/// coarse Aabb box (which stays only as a load-time fallback: `part_selection`
/// skips the OBB pass for any entity that has a collider). Runs once per cloud.
#[cfg(feature = "gaussian-splatting")]
pub fn apply_splat_colliders(
    mut commands: Commands,
    query: Query<
        (Entity, &eustress_radiance::SplatColliderProxy),
        Without<SplatColliderAttached>,
    >,
) {
    use avian3d::prelude::*;
    for (entity, sp) in &query {
        // Mark done up front — even an empty proxy must not re-run every frame.
        commands.entity(entity).insert(SplatColliderAttached);
        let shapes: Vec<(Vec3, Quat, Collider)> = sp
            .proxy
            .primitives
            .iter()
            .map(|p| match p {
                eustress_radiance::ColliderPrimitive::Box { center, half_extents } => (
                    Vec3::from(*center),
                    Quat::IDENTITY,
                    // Avian `cuboid` takes FULL side lengths (parry half-extents
                    // × 2). radiance emits half-extents.
                    Collider::cuboid(half_extents[0] * 2.0, half_extents[1] * 2.0, half_extents[2] * 2.0),
                ),
                eustress_radiance::ColliderPrimitive::Sphere { center, radius } => {
                    (Vec3::from(*center), Quat::IDENTITY, Collider::sphere(*radius))
                }
            })
            .collect();
        if shapes.is_empty() {
            continue;
        }
        let n = shapes.len();
        commands
            .entity(entity)
            .insert((Collider::compound(shapes), RigidBody::Static));
        warn!("splat collider: attached {} Avian voxel boxes (static) to {:?}", n, entity);
    }
}

/// Re-spawn imported GaussianSplats on Space open — closes the DB-primary
/// persistence gap for splat clouds.
///
/// A DB-primary Space cold-loads its instances from binary cores in the Fjall
/// `entities` partition. GaussianSplats are file-natured (their
/// `[gaussian_splats]` path can't survive the core's bincode round-trip — the
/// `#[serde(flatten)] extra` fails to serialise), so they get NO core, and the
/// `FjallSource` file walk doesn't re-materialise their disk folders either.
/// Net effect the user hit: an imported splat renders the session it was
/// imported (the file-watcher's hot-create path) but VANISHES on every later
/// launch — "3DGS fail to load unless you launch it from here".
///
/// This system re-spawns them, once per Space open, WITHOUT disturbing the
/// delicate DB load order: after the file loader has registered the
/// `Workspace` service root, it walks the DISK `Workspace` tree for
/// GaussianSplats `_instance.toml` folders and spawns any not already live —
/// through the exact same [`spawn_instance`] funnel the watcher uses, so the
/// cloud (and, downstream, the [`apply_splat_colliders`] collider) attach
/// identically. The [`super::file_loader::SpaceFileRegistry::is_loaded`] guard
/// makes it idempotent and prevents a double-spawn if any other path did
/// materialise the folder. The `.ply` is read straight off disk, so it works
/// for pre-existing imports with no schema migration.
/// Result of the off-thread splat discovery: every `_instance.toml` under
/// `Workspace` whose text carries a `[gaussian_splats]` section, as
/// `(disk path, text)`, plus the number of instance files inspected.
#[cfg(feature = "gaussian-splatting")]
pub struct GsDiscovery {
    hits: Vec<(std::path::PathBuf, String)>,
    inspected: usize,
    /// `"fjall tree"` or `"disk walk"`, for the summary line.
    source: &'static str,
}

#[cfg(feature = "gaussian-splatting")]
pub fn load_disk_gaussian_splats_on_open(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material_registry: ResMut<super::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<PrimitiveMeshCache>,
    mut decal_materials: ResMut<Assets<ForwardDecalMaterial<StandardMaterial>>>,
    mut registry: ResMut<super::file_loader::SpaceFileRegistry>,
    space_root: Res<super::SpaceRoot>,
    mut last_space: Local<Option<PathBuf>>,
    // Discovery in flight for `last_space`, if any. Polled each frame.
    mut in_flight: Local<Option<std::sync::mpsc::Receiver<GsDiscovery>>>,
) {
    // ── Phase B (main thread): a finished discovery → spawn its hits ─────
    //
    // PERF (load time): discovery used to run synchronously inside this
    // system. On Super Station it walked 106,356 `_instance.toml` files for
    // 39.4 s on the main thread — and found zero splats. It now runs on a
    // worker (Phase A below) and this system only polls; the window keeps
    // rendering and the priority spawn overlaps with the scan instead of
    // waiting behind it.
    if let Some(rx) = in_flight.as_ref() {
        match rx.try_recv() {
            Ok(found) => {
                *in_flight = None;
                spawn_discovered_splats(
                    found,
                    &space_root.0,
                    &mut commands,
                    &asset_server,
                    &mut materials,
                    &mut material_registry,
                    &mut mesh_cache,
                    &mut decal_materials,
                    &mut registry,
                );
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                *in_flight = None;
                warn!("🌫️ GS persistence scan: discovery worker vanished without a result");
            }
        }
        return;
    }
    // Run once per genuine Space open (mirrors the binary boot-load latch).
    if last_space.as_deref() == Some(space_root.0.as_path()) {
        return;
    }
    let workspace = space_root.0.join("Workspace");
    // Gate ONLY on the Space's Workspace existing on disk — skips the pre-load
    // default SpaceRoot without depending on the file loader's service
    // registration (which keys differently in DB-primary mode, so an earlier
    // service-entity gate never fired). We read disk directly, so DB/loader
    // ordering is irrelevant; the `is_loaded` guard below still prevents any
    // double-spawn.
    if !workspace.is_dir() {
        return;
    }
    if super::skip_disk_scans() {
        // Stamp the Space so this does not re-attempt every frame.
        *last_space = Some(space_root.0.clone());
        info!(
            "🌫️ GS persistence scan SKIPPED (EUSTRESS_SKIP_DISK_SCANS) — disk-authored \
             splat clouds will not be re-spawned for this open"
        );
        return;
    }
    *last_space = Some(space_root.0.clone());

    // ── Phase A (worker thread): discovery ──────────────────────────────
    //
    // Two sources, same output. When a Fjall DB is active the `tree`
    // partition already holds every `_instance.toml`'s bytes (the reconcile
    // keeps it current), so the scan is ONE sequential pass over the LSM
    // tree instead of 100K+ directory opens on NTFS. A legacy disk Space
    // keeps the rayon level-order walk. Either way it runs off the main
    // thread and the result is polled above.
    let (tx, rx) = std::sync::mpsc::channel::<GsDiscovery>();
    let space_root_for_worker = space_root.0.clone();
    let db = super::active_db::db_arc();
    let spawned = std::thread::Builder::new()
        .name("eustress-gs-discovery".into())
        .spawn(move || {
            let found = match db {
                Some(db) => discover_splats_in_tree(db.as_ref(), &space_root_for_worker),
                None => discover_splats_on_disk(&space_root_for_worker.join("Workspace")),
            };
            let _ = tx.send(found);
        });
    match spawned {
        Ok(_) => *in_flight = Some(rx),
        Err(e) => warn!("🌫️ GS persistence scan: could not spawn discovery worker: {e}"),
    }
}

/// Discovery against the Fjall `tree` partition (DB-primary Space).
#[cfg(feature = "gaussian-splatting")]
fn discover_splats_in_tree(db: &dyn eustress_worlddb::WorldDb, space_root: &Path) -> GsDiscovery {
    let mut out = GsDiscovery { hits: Vec::new(), inspected: 0, source: "fjall tree" };
    let it = match db.iter_tree() {
        Ok(it) => it,
        Err(e) => {
            warn!("🌫️ GS persistence scan: iter_tree failed ({e}); falling back to the disk walk");
            return discover_splats_on_disk(&space_root.join("Workspace"));
        }
    };
    for entry in it {
        let Ok((rel, bytes)) = entry else { continue };
        // Only `Workspace/**/_instance.toml` can hold a splat cloud.
        if !(rel.starts_with("Workspace/") || rel.starts_with("Workspace\\"))
            || !rel.ends_with("_instance.toml")
        {
            continue;
        }
        out.inspected += 1;
        // Byte-level pre-filter: a `[gaussian_splats]` section is the only
        // marker, and most Spaces have none.
        if !bytes.windows(15).any(|w| w == b"gaussian_splats") {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else { continue };
        out.hits.push((space_root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)), content));
    }
    out
}

/// Discovery by walking the disk `Workspace` tree (legacy, non-DB Space).
/// Rayon level-order: a whole directory level is read in parallel, and
/// `file_type()` avoids the extra stat that `is_dir()` costs per entry.
#[cfg(feature = "gaussian-splatting")]
fn discover_splats_on_disk(workspace: &Path) -> GsDiscovery {
    use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
    let mut out = GsDiscovery { hits: Vec::new(), inspected: 0, source: "disk walk" };
    let mut frontier: Vec<std::path::PathBuf> = vec![workspace.to_path_buf()];
    while !frontier.is_empty() {
        struct LevelScan {
            subdirs: Vec<std::path::PathBuf>,
            toml_seen: usize,
            hits: Vec<(std::path::PathBuf, String)>,
        }
        let scans: Vec<LevelScan> = frontier
            .par_iter()
            .map(|dir| {
                let mut lvl = LevelScan { subdirs: Vec::new(), toml_seen: 0, hits: Vec::new() };
                let Ok(read_dir) = std::fs::read_dir(dir) else {
                    return lvl;
                };
                for entry in read_dir.flatten() {
                    let path = entry.path();
                    let is_dir = entry
                        .file_type()
                        .map(|t| t.is_dir())
                        .unwrap_or_else(|_| path.is_dir());
                    if is_dir {
                        // Skip `.eustress` (trash + world.fjalldb): only the
                        // human tree.
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if !name.starts_with('.') {
                            lvl.subdirs.push(path);
                        }
                        continue;
                    }
                    if path.file_name().and_then(|n| n.to_str()) != Some("_instance.toml") {
                        continue;
                    }
                    lvl.toml_seen += 1;
                    let Ok(content) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    if content.contains("gaussian_splats") {
                        lvl.hits.push((path, content));
                    }
                }
                lvl
            })
            .collect();
        frontier = Vec::new();
        for scan in scans {
            out.inspected += scan.toml_seen;
            out.hits.extend(scan.hits);
            frontier.extend(scan.subdirs);
        }
    }
    out
}

/// Phase B (main thread): spawn every discovered splat cloud. Body unchanged
/// from the synchronous version; only what feeds it moved to a worker.
#[cfg(feature = "gaussian-splatting")]
#[allow(clippy::too_many_arguments)]
fn spawn_discovered_splats(
    found: GsDiscovery,
    space_root: &Path,
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    material_registry: &mut super::material_loader::MaterialRegistry,
    mesh_cache: &mut PrimitiveMeshCache,
    decal_materials: &mut Assets<ForwardDecalMaterial<StandardMaterial>>,
    registry: &mut super::file_loader::SpaceFileRegistry,
) {
    let workspace = space_root.join("Workspace");
    // Best-effort parent: the Workspace service entity if the file loader has
    // registered it (by `_service.toml` path or by directory path); else spawn
    // at the root. `tag_splats_for_explorer` still nests the cloud under
    // Workspace in the Explorer, and rendering needs no parent.
    let workspace_entity = registry
        .get_entity(&workspace.join("_service.toml"))
        .or_else(|| registry.get_entity(&workspace));
    let found_gs = found.hits.len();
    let mut replaced = 0usize;
    let mut spawned = 0usize;
    for (path, content) in found.hits {
        // In DB-primary mode the file loader DOES spawn this GS folder, but
        // WITHOUT a cloud: the `[gaussian_splats]` section is lost through the
        // Fjall core/tree, and the folder-spawn never re-reads it from disk
        // (confirmed at runtime: the entity is registered, yet no cloud or
        // collider attaches and nothing renders). Despawn that cloudless husk,
        // if present, then re-spawn a COMPLETE instance straight from the text
        // below, which routes through `spawn_instance`'s GaussianSplats arm
        // and attaches the real radiance-field cloud + collider.
        if let Some(old) = registry.get_entity(&path) {
            commands.entity(old).despawn();
            replaced += 1;
        }
        match spawn_instance_from_toml_str(
            commands,
            asset_server,
            materials,
            material_registry,
            mesh_cache,
            decal_materials,
            path.clone(),
            &content,
        ) {
            Ok(entity) => {
                // Mirror the watcher's post-spawn bookkeeping so the Explorer
                // classifies the entity and disk move/delete keep working.
                commands.entity(entity).insert(super::file_loader::LoadedFromFile {
                    path: path.clone(),
                    file_type: super::file_loader::FileType::Toml,
                    service: "Workspace".to_string(),
                });
                // Best-effort parent (see above). No parent still renders and
                // Explorer-nests via `tag_splats_for_explorer`.
                if let Some(pe) = workspace_entity {
                    commands.entity(entity).insert(ChildOf(pe));
                }
                let name = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("SplatCloud")
                    .to_string();
                registry.register(
                    path.clone(),
                    entity,
                    super::file_loader::FileMetadata {
                        path: path.clone(),
                        file_type: super::file_loader::FileType::Toml,
                        service: "Workspace".to_string(),
                        name,
                        size: 0,
                        modified: std::time::SystemTime::now(),
                        children: Vec::new(),
                    },
                );
                spawned += 1;
                info!("🌫️ GS persistence: re-spawned disk GaussianSplats on open: {:?}", path);
            }
            Err(e) => {
                warn!("GS persistence: failed to re-spawn disk GaussianSplats {:?}: {}", path, e);
            }
        }
    }
    // One-shot scan summary (always, even at zero): the definitive diagnostic
    // of what the open-time GS pass saw.
    info!(
        "🌫️ GS persistence scan ({}): {} _instance.toml, {} gaussian_splats, {} cloudless-replaced, {} spawned (parent={:?})",
        found.source, found.inspected, found_gs, replaced, spawned, workspace_entity
    );
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
            // The two nodes it joins, by `Instance.uuid`: what
            // `sync_beam_transforms` stretches it between.
            let uuid = |key: &str| {
                sec.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string)
            };
            ec.insert(crate::spawners::audio_vfx::BeamSegmentLink {
                attachment0_uuid: uuid("attachment0_uuid"),
                attachment1_uuid: uuid("attachment1_uuid"),
                ..Default::default()
            });
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
    // The same instances without the change filter: entities picked up
    // from the deferred set or from a drag that just ended are written from
    // their CURRENT state.
    current: Query<(
        Entity,
        &Transform,
        &InstanceFile,
        Option<&eustress_common::classes::BasePart>,
        Option<&eustress_common::units::MeasureUnit>,
    ), Without<BeingDragged>>,
    added_instances: Query<Entity, Added<Transform>>,
    mut recently_written: ResMut<super::file_watcher::RecentlyWrittenFiles>,
    load_in_progress: Res<super::file_loader::LoadInProgress>,
    // A drag ends by removing `BeingDragged`, and by then the transform's
    // change tick is older than this system's last run, so the change
    // filter never sees it. Without this, a tool that relies on this writer
    // (Rotate does) left the dragged pose unsaved.
    mut undragged: RemovedComponents<BeingDragged>,
    // Changes that arrived while their file was inside the recent-write
    // window. They are written once the window passes; they used to be
    // dropped, which lost an undo made within two seconds of a drag: the
    // file kept the moved pose and the part came back moved on reload.
    mut deferred: Local<std::collections::HashSet<Entity>>,
    // Light-class instances. They may live under `Lighting/` (the Toolbox
    // put them there), but their pose is authored, unlike the sky's.
    light_classes: Query<
        (),
        Or<(
            With<eustress_common::classes::EustressPointLight>,
            With<eustress_common::classes::EustressSpotLight>,
            With<eustress_common::classes::SurfaceLight>,
            With<eustress_common::classes::EustressDirectionalLight>,
        )>,
    >,
) {
    deferred.extend(undragged.read());
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
        /// The part's own density (kg/m3), `Some(None)` when it has none.
        physics_density: Option<Option<f32>>,
        /// True when the entity references a custom GLB mesh (e.g. V-Cell
        /// parts). For these, `scale` in the TOML is the user-set multiplier
        /// and must NOT be overwritten from `BasePart.size` (which comes from
        /// the mesh bounding box and would clobber the user's value with
        /// whatever the mesh happens to measure in scene units).
        is_custom_mesh: bool,
    }
    let mut jobs: Vec<WriteJob> = Vec::new();

    // This frame's changes plus everything still waiting from earlier ones.
    let mut candidates: Vec<Entity> = instances.iter().map(|(e, ..)| e).collect();
    candidates.extend(deferred.drain());
    candidates.sort_unstable();
    candidates.dedup();

    for candidate in candidates {
        let Ok((entity, transform, instance_file, base_part, _measure_unit)) = current.get(candidate) else {
            // Despawned, or picked up by a new drag (whose own release
            // writes it): nothing to do here.
            continue;
        };
        if just_added.contains(&entity) {
            continue;
        }
        // Writing inside the window would re-arm the watcher reload loop the
        // window exists to break. Hold the change and write the latest state
        // once the window has passed; never drop it.
        if recently_written.was_recently_written(&instance_file.toml_path) {
            deferred.insert(entity);
            continue;
        }
        // Lighting-service entities (Star/Sun, Moon, Sky, Atmosphere) have
        // runtime-driven Transforms (sun direction from time_of_day, etc.)
        // that must not be persisted — their authoritative state lives in
        // LightingService, not the TOML. Writing them would produce a stutter
        // loop: transform write → file-watcher event → class_schema self-heal
        // → another file-watcher event, every ~2 s.
        // A light-class instance there is not one of them: its position and
        // aim are authored, and dropping them lost every move or rotation.
        if instance_file.toml_path.components().any(|c| c.as_os_str() == "Lighting")
            && !light_classes.contains(entity)
        {
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
            physics_density: None,
            is_custom_mesh,
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
            job.physics_density = Some(bp.custom_physical_properties.as_ref().map(|p| p.density));
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
                // Source the document from the DB first when a DB is active.
                //
                // On a MIGRATED Space the DB is authoritative and the loose
                // `_instance.toml` may not exist at all (see
                // `space_ops::space_is_migrated` — "the DB owns integrity").
                // Reading straight from disk therefore failed on the very
                // first line and abandoned the whole write, so Transform /
                // BasePart edits never reached ANY store and silently
                // vanished on restart. Prefer the DB copy, fall back to disk.
                let text = match crate::space::active_db::get_instance_text(&job.path) {
                    Some(t) => t,
                    None => std::fs::read_to_string(&job.path)
                        .map_err(|e| format!("read {:?}: {}", job.path, e))?,
                };
                let mut doc: toml::Value = text.parse()
                    .map_err(|e: toml::de::Error| format!("parse {:?}: {}", job.path, e))?;
                // The unit the file declares (`[metadata] unit`), which the
                // loader converts from at spawn: values enter the file in it.
                let unit = file_unit(
                    doc.get("metadata").and_then(|m| m.get("unit")).and_then(|u| u.as_str()),
                );

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

                // Engine metres into the file's own unit at the very edge
                // (`authored_vec3`), mirroring the load path.
                let [px, py, pz] = authored_vec3(Vec3::from_array(job.transform.position), unit);
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
                    let [sx, sy, sz] = authored_vec3(Vec3::from_array(job.transform.scale), unit);
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
                if let Some(density) = job.physics_density {
                    patch_physics_density(props, density)?;
                }

                // ── [metadata].last_modified ────────────────────────────────────
                if let Some(meta) = root.get_mut("metadata").and_then(|m| m.as_table_mut()) {
                    meta.insert("last_modified".into(),
                        toml::Value::String(chrono::Utc::now().to_rfc3339()));
                }

                let out = toml::to_string_pretty(&doc)
                    .map_err(|e| format!("serialize {:?}: {}", job.path, e))?;

                // Mirror into the DB whenever one is active. This system
                // patches raw TOML on purpose (to preserve sections the typed
                // `InstanceDefinition` would drop), so it bypassed the
                // canonical `write_instance_definition` — which DOES
                // dual-write via `active_db::put_instance`. That bypass is
                // why gizmo/Properties/script edits persisted to disk but
                // were invisible to a DB-primary load, and came back stale
                // after a restart. Writing the patched TEXT keeps every
                // unknown section intact in the DB copy too.
                let db_ok = crate::space::active_db::put_instance_text(&job.path, &out);

                // Atomic write + retry so a transient file-lock from
                // an external reader (antivirus, text editor, the
                // engine's reload-after-write pass) doesn't silently
                // drop the user's edit (see `gui_loader::write_atomic`
                // for the full rationale).
                //
                // Still written when the DB accepted it: disk stays a
                // readable mirror, and a non-migrated Space has no DB at all.
                // A disk failure is only fatal if the DB did not take it.
                if let Err(e) = super::gui_loader::write_atomic(&job.path, out.as_bytes()) {
                    if !db_ok {
                        return Err(format!("write {:?}: {}", job.path, e));
                    }
                }
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

/// Patch a single `_instance.toml` with the entity's current tags and
/// attributes. Pure on-disk operation — no Bevy types in or out. Runs
/// on a background thread.
pub(crate) fn patch_tags_attributes_toml(
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
        // The root `tags` is the one copy. A `[metadata] tags` an older
        // importer wrote would come back as the fallback once the root is
        // gone, so it goes too.
        if let Some(meta) = root.get_mut("metadata").and_then(|m| m.as_table_mut()) {
            meta.remove("tags");
        }
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
        // Merged into what the file holds: an entry the engine cannot read (a
        // kind the reader does not model) stays as written, and a readable
        // entry missing from the component is one the user deleted.
        let existing = root.get("attributes").and_then(|a| a.as_table()).cloned();
        match eustress_common::datamodel::record::merge_attribute_table(existing.as_ref(), attrs) {
            Some(table) => {
                root.insert("attributes".into(), toml::Value::Table(table));
            }
            None => {
                root.remove("attributes");
            }
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
        (Entity, &InstanceFile),
        (
            Added<InstanceFile>,
            Without<eustress_common::attributes::Tags>,
        ),
    >,
    needs_attrs: Query<
        (Entity, &InstanceFile),
        (
            Added<InstanceFile>,
            Without<eustress_common::attributes::Attributes>,
        ),
    >,
    // Parameters get the same treatment as Attributes. They are a DIFFERENT
    // concept — an Attribute is a static value on the part, a Parameter binds
    // the part into a domain and, once sourced, to external data — but the
    // panel affordance must be identical: the section is always present so its
    // "+" is reachable on an entity that has no parameters yet.
    needs_params: Query<
        Entity,
        (
            Added<InstanceFile>,
            Without<eustress_common::parameters::InstanceParameters>,
        ),
    >,
) {
    // A spawn path that did not fill them gets them from the entity's file, so
    // a later tag or attribute save keeps the file's others. A file that
    // cannot be read (a synthetic binary-core path, a Fjall-only world) gets
    // empty ones, as before.
    let mut docs: std::collections::HashMap<Entity, Option<toml::Value>> = std::collections::HashMap::new();
    let mut doc_of = |entity: Entity, file: &InstanceFile| -> Option<toml::Value> {
        docs.entry(entity)
            .or_insert_with(|| {
                std::fs::read_to_string(&file.toml_path)
                    .ok()
                    .and_then(|text| text.parse::<toml::Value>().ok())
            })
            .clone()
    };
    for (entity, file) in needs_tags.iter() {
        let tags = doc_of(entity, file)
            .map(|doc| eustress_common::datamodel::record::record_tags(&doc))
            .unwrap_or_default();
        commands
            .entity(entity)
            .insert(eustress_common::attributes::Tags(tags));
    }
    for (entity, file) in needs_attrs.iter() {
        let mut attributes = eustress_common::attributes::Attributes::new();
        if let Some(doc) = doc_of(entity, file) {
            for (key, value) in eustress_common::datamodel::record::record_attribute_values(&doc) {
                attributes.set(&key, value);
            }
        }
        commands.entity(entity).insert(attributes);
    }
    for entity in needs_params.iter() {
        commands
            .entity(entity)
            .insert(eustress_common::parameters::InstanceParameters::new());
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
                // A runtime reference (an Object) never persists.
                if let Some(value) = eustress_common::datamodel::record::attribute_to_toml(v) {
                    out.insert(k.clone(), value);
                }
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

#[cfg(test)]
mod authored_transform_tests {
    use super::*;

    fn temp_file(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_authored_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("_instance.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Where the loader spawns a file's numbers: its unit to metres, as
    /// `spawn_instance` converts them.
    fn spawned(v: [f32; 3], unit: Option<&str>) -> Vec3 {
        #[cfg(feature = "units_v1")]
        {
            Vec3::from_array(eustress_common::units::convert_vec3_f32(
                v,
                file_unit(unit),
                eustress_common::units::ENGINE_NATIVE_UNIT,
            ))
        }
        #[cfg(not(feature = "units_v1"))]
        {
            let _ = unit;
            Vec3::from_array(v)
        }
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).abs().max_element() < 1e-4
    }

    /// A part imported in feet, moved and resized in metres, saved, and read
    /// back: its file stays in feet and it spawns where it was left.
    #[test]
    fn a_file_in_feet_reads_back_where_it_was_saved() {
        let path = temp_file(
            "feet",
            "[metadata]\nclass_name = \"Part\"\nunit = \"ft\"\n\n[transform]\nposition = [10.0, 0.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [4.0, 1.0, 2.0]\n",
        );
        let mut def = load_instance_definition(&path).unwrap();
        let moved = Vec3::new(3.048 + 1.0, 0.5, -2.0);
        let size = Vec3::new(1.2192, 0.3048, 0.9144);
        set_authored_transform(&mut def, moved, Quat::from_rotation_y(0.5), Some(size));
        write_instance_definition(&path, &def).unwrap();
        let back = load_instance_definition(&path).unwrap();
        assert_eq!(back.metadata.unit.as_deref(), Some("ft"));
        let unit = back.metadata.unit.as_deref();
        assert!(close(spawned(back.transform.position, unit), moved), "{:?}", back.transform.position);
        assert!(close(spawned(back.transform.scale, unit), size), "{:?}", back.transform.scale);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A file with no unit is in metres and takes the numbers as they are;
    /// a size of `None` keeps the file's scale.
    #[test]
    fn a_file_in_metres_takes_metres_and_keeps_its_scale() {
        let path = temp_file(
            "metres",
            "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [1.0, 2.0, 3.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [2.0, 2.0, 2.0]\n",
        );
        let mut def = load_instance_definition(&path).unwrap();
        set_authored_transform(&mut def, Vec3::new(4.0, 5.0, 6.0), Quat::IDENTITY, None);
        assert_eq!(def.transform.position, [4.0, 5.0, 6.0]);
        assert_eq!(def.transform.scale, [2.0, 2.0, 2.0]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A pasted copy of a part in feet gets its new pose and size in feet.
    #[test]
    fn a_toml_document_in_feet_takes_feet() {
        let mut doc: toml::Value = "[metadata]\nclass_name = \"Part\"\nunit = \"ft\"\n\n[properties]\nanchored = true\n"
            .parse()
            .unwrap();
        set_authored_transform_toml(&mut doc, Vec3::new(3.048, 0.0, 0.0), Quat::IDENTITY, Some(Vec3::splat(0.6096))).unwrap();
        let get = |k: &str| -> Vec<f64> {
            doc["transform"][k].as_array().unwrap().iter().map(|v| v.as_float().unwrap()).collect()
        };
        #[cfg(feature = "units_v1")]
        {
            assert!((get("position")[0] - 10.0).abs() < 1e-4, "{:?}", get("position"));
            assert!((get("scale")[1] - 2.0).abs() < 1e-4, "{:?}", get("scale"));
        }
        assert_eq!(doc["properties"]["anchored"].as_bool(), Some(true), "other keys are kept");
    }
}

/// The typed writer against the file it writes onto (`planned_write`).
#[cfg(test)]
mod density_tests {
    use super::*;

    /// A Part file: its `[metadata]` (every instance file has one), then `rest`.
    fn part(rest: &str) -> String {
        format!("[metadata]\nclass_name = \"Part\"\n\n{rest}")
    }

    fn resolved(text: &str) -> f32 {
        let def = load_instance_definition_from_str(text).unwrap();
        let material = eustress_common::classes::Material::from_string(&def.properties.material);
        part_density_kg_m3(&material, def.properties.physics.as_ref())
    }

    fn density(rest: &str) -> f32 {
        resolved(&part(rest))
    }

    /// An older Roblox import's custom physics section, as the importer wrote
    /// it before densities were tagged: Roblox's g/cm3 beside its weights.
    const OLDER_IMPORT: &str = "[properties]\nmaterial = \"Plastic\"\n\n[properties.physics]\ndensity = 0.7\nelasticity_weight = 1.0\nfriction_kinetic = 0.3\nfriction_static = 0.3\nfriction_weight = 1.0\nrestitution = 0.5\n";

    /// A part weighs its material, or its own authored density. An untagged
    /// native density is kg/m3; an older import's untagged Roblox number
    /// (g/cm3, beside Roblox's weights) is the same body as its tagged value.
    #[test]
    fn a_part_weighs_its_material_or_its_authored_density() {
        assert_eq!(density("[properties]\nmaterial = \"Wood\"\n"), 600.0, "Wood's default");
        assert_eq!(density("[properties]\n"), 900.0, "no material named: Plastic's");
        let native = "[properties]\nmaterial = \"Wood\"\n\n[properties.physics]\ndensity = 600.0\n";
        assert_eq!(density(native), 600.0, "an untagged native density is kg/m3");
        assert!((density(OLDER_IMPORT) - 700.0).abs() < 1e-3, "an older import's g/cm3");
        let tagged = "[properties]\nmaterial = \"Plastic\"\n\n[properties.physics]\ndensity = 700.0\ndensity_unit = \"kg/m3\"\n";
        assert!((density(tagged) - 700.0).abs() < 1e-3, "kg/m3");
        let grams = "[properties.physics]\ndensity = 0.7\ndensity_unit = \"g/cm3\"\n";
        assert!((density(grams) - 700.0).abs() < 1e-3, "g/cm3, said");
        // An imported Wood part (Roblox's Wood, 350 kg/m3) weighs what the
        // same authored density weighs on a native part.
        let imported = "[properties]\nmaterial = \"Wood\"\n\n[properties.physics]\npreset = \"Default\"\ndensity = 350.0\ndensity_unit = \"kg/m3\"\n";
        let native = "[properties]\nmaterial = \"Wood\"\n\n[properties.physics]\ndensity = 350.0\ndensity_unit = \"kg/m3\"\n";
        assert_eq!(density(imported), 350.0);
        assert_eq!(density(imported), density(native));
        // A zero density is none: the material's.
        assert_eq!(density("[properties]\nmaterial = \"Metal\"\n\n[properties.physics]\ndensity = 0.0\n"), 7850.0);
    }

    /// The file's density is the part's override, with its friction and
    /// bounce; a file without one has none.
    #[test]
    fn the_files_density_is_the_parts_override() {
        let def = load_instance_definition_from_str(&part(OLDER_IMPORT)).unwrap();
        let physics = def.properties.physics.as_ref().unwrap();
        assert_eq!(physics.density, Some(0.7), "loading keeps the file's own number");
        assert_eq!(physics.density_unit, None);
        let over = part_physical_override(Some(physics)).expect("an override");
        assert!((over.density - 700.0).abs() < 1e-3);
        assert_eq!(over.friction, 0.3);
        assert_eq!(over.elasticity, 0.5);
        let none = load_instance_definition_from_str(&part("[properties]\nmaterial = \"Wood\"\n")).unwrap();
        assert!(part_physical_override(none.properties.physics.as_ref()).is_none());
    }

    /// Save round trip: a part's override is written back tagged and reloads
    /// as the same density; a part with none has the file's density removed
    /// and takes its material's. Other physics keys are kept.
    #[test]
    fn a_saved_density_reloads_as_the_same_mass() {
        let file = part(&format!("{OLDER_IMPORT}roblox_note = \"kept\"\n"));
        let def = load_instance_definition_from_str(&file).unwrap();
        let over = part_physical_override(def.properties.physics.as_ref()).unwrap();
        let mut doc: toml::Value = file.parse().unwrap();
        let props = doc.get_mut("properties").and_then(|p| p.as_table_mut()).unwrap();
        patch_physics_density(props, Some(over.density)).unwrap();
        let saved = toml::to_string(&doc).unwrap();
        assert!((resolved(&saved) - 700.0).abs() < 1e-3, "{saved}");
        assert!(saved.contains("density_unit = \"kg/m3\""), "{saved}");
        assert!(saved.contains("roblox_note"), "{saved}");
        // A Studio edit to 1234 kg/m3 survives the same way.
        let props = doc.get_mut("properties").and_then(|p| p.as_table_mut()).unwrap();
        patch_physics_density(props, Some(1234.0)).unwrap();
        assert_eq!(resolved(&toml::to_string(&doc).unwrap()), 1234.0);
        // No override: the density goes, and the material's applies.
        let props = doc.get_mut("properties").and_then(|p| p.as_table_mut()).unwrap();
        patch_physics_density(props, None).unwrap();
        let cleared = toml::to_string(&doc).unwrap();
        assert_eq!(resolved(&cleared), 900.0, "{cleared}");
        assert!(cleared.contains("roblox_note"), "{cleared}");
    }
}

#[cfg(test)]
mod typed_write_tests {
    use super::*;

    const PART: &str = r#"# A crate
[metadata]
class_name = "Part"
name = "Crate"
uuid = "0123456789abcdef0123456789abcdef"
unit = "ft"
roblox_brick_color = 194

[transform]
position = [113.6, 0.071, 95.0] # studs
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [4.0, 1.0, 2.0]
pivot_note = "kept"

[properties]
anchored = true
color = [0.8627, 0.2745, 0.2353, 1.0]
material = "Wood"
collision_group = "Crates"

[properties.extras]
CanQuery = true
BackSurface = 0

[properties.physics]
density = 0.7
roblox_note = "kept"

[asset]
mesh = "parts/block.glb"
scene = "Scene0"

[attributes]
Health = 100
Owner = "npc"

[gui_hint]
label = "a top-level section no struct models"
"#;

    /// Box Head's pistol as an earlier writer left it (multi-line arrays).
    const PISTOL: &str = r#"[asset]
mesh = "../../../meshes/gun_pistol.glb"
scene = "Scene0"

[metadata]
archivable = true
class_name = "Part"
name = "Pistol"
uuid = "0d03f50cf79683cb104d7faf1650491f"

[properties]
anchored = true
can_collide = false
cast_shadow = true
color = [
    1.0,
    1.0,
    1.0,
    1.0,
]
locked = false
material = "SmoothPlastic"
reflectance = 0.0
transparency = 0.0

[transform]
position = [
    0.0,
    -50.0,
    0.0,
]
rotation = [
    0.0,
    0.0,
    0.0,
    1.0,
]
scale = [
    1.0,
    1.0,
    1.0,
]
"#;

    const FOLDER: &str = "[metadata]\nclass_name = \"Folder\"\nname = \"Stuff\"\n\n[attributes]\nCount = 3\n";

    fn file(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_typed_write_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("_instance.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn text(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn done(path: &Path) {
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Saving without an edit leaves every file byte for byte, signed or not,
    /// and adds no stamp.
    #[test]
    fn an_unchanged_save_leaves_the_file_byte_for_byte() {
        let stamp = CreatorStamp {
            name: "Tester".to_string(),
            public_key: "key".to_string(),
            timestamp: "2026-09-25T12:00:00Z".to_string(),
            first_timestamp: None,
            saves: 1,
        };
        for (name, original) in [("part", PART), ("pistol", PISTOL), ("folder", FOLDER)] {
            let path = file(name, original);
            let mut def = load_instance_definition_from_str(original).unwrap();
            write_instance_definition(&path, &def).unwrap();
            assert_eq!(text(&path), original, "{name}: unsigned");
            write_instance_definition_signed(&path, &mut def, Some(&stamp)).unwrap();
            assert_eq!(text(&path), original, "{name}: signed");
            assert!(def.metadata.created_by.is_none() && def.metadata.modifications.is_empty(), "{name}: no stamp");
            done(&path);
        }
    }

    /// A run of saves by one author is one entry; another author appends.
    #[test]
    fn a_run_of_saves_by_one_author_is_one_entry() {
        let stamp = |who: &str, at: &str| CreatorStamp {
            name: who.to_string(),
            public_key: format!("{who}-key"),
            timestamp: at.to_string(),
            first_timestamp: None,
            saves: 1,
        };
        let mut chain = Vec::new();
        for (who, at) in [("a", "t1"), ("a", "t2"), ("b", "t3"), ("a", "t4")] {
            record_modification(&mut chain, &stamp(who, at));
        }
        assert_eq!(chain.len(), 3);
        assert_eq!((chain[0].first_timestamp.as_deref(), chain[0].timestamp.as_str(), chain[0].saves), (Some("t1"), "t2", 2));
        assert_eq!((chain[1].first_timestamp.as_deref(), chain[1].timestamp.as_str(), chain[1].saves), (None, "t3", 1));
        assert_eq!((chain[2].timestamp.as_str(), chain[2].saves), ("t4", 1));

        // In a file: two signed moves by one author leave one entry, saves = 2.
        let path = file("stamps", PART);
        let mut def = load_instance_definition_from_str(PART).unwrap();
        def.transform.position = [1.0, 2.0, 3.0];
        write_instance_definition_signed(&path, &mut def, Some(&stamp("a", "t1"))).unwrap();
        let mut def = load_instance_definition_from_str(&text(&path)).unwrap();
        def.transform.position = [4.0, 5.0, 6.0];
        write_instance_definition_signed(&path, &mut def, Some(&stamp("a", "t2"))).unwrap();
        let doc: toml::Value = text(&path).parse().unwrap();
        let chain = doc["metadata"]["modifications"].as_array().unwrap();
        assert_eq!(chain.len(), 1, "{doc}");
        assert_eq!(chain[0]["saves"].as_integer(), Some(2));
        assert_eq!(chain[0]["first_timestamp"].as_str(), Some("t1"));
        assert_eq!(chain[0]["timestamp"].as_str(), Some("t2"));
        done(&path);
    }

    /// A move rewrites the position line and nothing else, in the shortest
    /// f32 form, keeping its comment.
    #[test]
    fn a_move_changes_only_the_position_line() {
        let path = file("move", PART);
        let mut def = load_instance_definition_from_str(PART).unwrap();
        def.transform.position = [5.1, 6.0, 7.0];
        write_instance_definition(&path, &def).unwrap();
        let after = text(&path);
        let changed: Vec<(&str, &str)> = PART.lines().zip(after.lines()).filter(|(a, b)| a != b).collect();
        assert_eq!(
            changed,
            vec![("position = [113.6, 0.071, 95.0] # studs", "position = [5.1, 6.0, 7.0] # studs")],
            "{after}"
        );
        assert_eq!(PART.lines().count(), after.lines().count());
        done(&path);
    }

    /// A changed colour keeps the file's float form and its alpha.
    #[test]
    fn a_colour_change_keeps_the_files_float_form() {
        let path = file("colour", PART);
        let mut def = load_instance_definition_from_str(PART).unwrap();
        def.properties.color = [0.5, 0.25, 0.125, 1.0];
        write_instance_definition(&path, &def).unwrap();
        let after = text(&path);
        assert!(after.contains("color = [0.5, 0.25, 0.125, 1.0]\n"), "{after}");
        done(&path);
    }

    /// A value the model clears is deleted; every key it does not know stays.
    #[test]
    fn a_cleared_value_stays_cleared_and_unmodeled_keys_stay() {
        let path = file("clear", PART);
        let before: toml::Value = PART.parse().unwrap();
        let mut def = load_instance_definition_from_str(PART).unwrap();
        def.asset = None;
        def.metadata.name = None;
        def.attributes.as_mut().unwrap().remove("Owner");
        if let Some(physics) = def.properties.physics.as_mut() {
            physics.density = None;
        }
        write_instance_definition(&path, &def).unwrap();
        let after: toml::Value = text(&path).parse().unwrap();
        assert!(after.get("asset").is_none(), "a removed asset mesh stays removed: {after}");
        assert!(after["metadata"].get("name").is_none());
        assert!(after["attributes"].get("Owner").is_none());
        assert_eq!(after["attributes"]["Health"].as_integer(), Some(100));
        assert!(after["properties"]["physics"].get("density").is_none());
        for (section, key) in [("transform", "pivot_note"), ("metadata", "roblox_brick_color"), ("properties", "collision_group"), ("properties", "extras")] {
            assert_eq!(after[section].get(key), before[section].get(key), "[{section}] {key}");
        }
        assert_eq!(after["properties"]["physics"]["roblox_note"].as_str(), Some("kept"));
        assert_eq!(after["gui_hint"], before["gui_hint"]);
        done(&path);
    }

    /// A class with no pose gains no part sections or defaults from a save.
    #[test]
    fn a_folder_gains_no_part_sections() {
        let path = file("folder_edit", FOLDER);
        let mut def = load_instance_definition_from_str(FOLDER).unwrap();
        def.metadata.name = Some("Things".to_string());
        write_instance_definition(&path, &def).unwrap();
        assert_eq!(text(&path), FOLDER.replace("name = \"Stuff\"", "name = \"Things\""));
        done(&path);
    }

    /// Every named section is a field, never `extra`, and a definition built
    /// without its file's unknown sections leaves them on disk.
    #[test]
    fn the_named_sections_are_the_structs_fields() {
        let mut def = load_instance_definition_from_str(PART).unwrap();
        for named in NAMED_SECTIONS {
            assert!(!def.extra.contains_key(*named), "{named} is a field of InstanceDefinition");
        }
        assert!(def.extra.contains_key("gui_hint"));
        let path = file("named", PART);
        def.extra.clear();
        write_instance_definition(&path, &def).unwrap();
        assert_eq!(text(&path), PART);
        done(&path);
    }

    /// A real Space, copied: saving each of its files without an edit changes
    /// none. Set `EUSTRESS_SAVE_SAMPLE` to a Space folder.
    #[test]
    #[ignore = "real data: set EUSTRESS_SAVE_SAMPLE to a Space folder"]
    fn a_real_space_saves_without_changing_a_file() {
        let Some(sample) = std::env::var_os("EUSTRESS_SAVE_SAMPLE").map(PathBuf::from) else { return };
        let temp = std::env::temp_dir().join(format!("eustress_save_sample_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let (mut checked, mut changed) = (0usize, Vec::new());
        let mut dirs = vec![sample.clone()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if path.is_dir() {
                    if !name.starts_with('.') {
                        dirs.push(path);
                    }
                    continue;
                }
                if !name.ends_with(".toml") || name == "space.toml" || name == "_service.toml" {
                    continue;
                }
                let Ok(original) = std::fs::read_to_string(&path) else { continue };
                let Ok(def) = load_instance_definition_from_str(&original) else { continue };
                let copy = temp.join(format!("{checked}.toml"));
                std::fs::write(&copy, &original).unwrap();
                write_instance_definition(&copy, &def).unwrap();
                if std::fs::read_to_string(&copy).unwrap() != original {
                    changed.push(path.display().to_string());
                }
                checked += 1;
            }
        }
        let _ = std::fs::remove_dir_all(&temp);
        eprintln!("{}: {checked} files saved without an edit, {} changed", sample.display(), changed.len());
        assert!(changed.is_empty(), "changed: {:?}", &changed[..changed.len().min(10)]);
    }
}
