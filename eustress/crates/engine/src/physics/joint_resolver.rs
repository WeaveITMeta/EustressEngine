//! # Constraint joint body resolver — Wave F1d (+ UUID-binding revision)
//!
//! The constraint spawners in [`crate::spawners::constraints`] attach the
//! correct Avian joint (`FixedJoint` / `RevoluteJoint` / `DistanceJoint` /
//! `PrismaticJoint` / `SphericalJoint`) to a freshly-spawned constraint
//! entity, but with **placeholder bodies** — both `joint.body1` and
//! `joint.body2` are [`Entity::PLACEHOLDER`]. They cannot resolve the real
//! bodies at spawn time because [`ClassSpawner::spawn`] sees only
//! `Commands`, not a `World` query (see `spawners/constraints/mod.rs` §
//! "Entity ref resolution").
//!
//! This module is the promised downstream resolver. It runs two passes:
//!
//! ## Pass 1 — load-path binder ([`cache_loaded_constraint_refs`] +
//! [`resolve_pending_constraint_joints`])
//!
//! The dominant runtime path for an imported / TOML-authored constraint
//! is NOT the spawner: `space/file_loader.rs` loads a `_instance.toml`
//! whose `[metadata].class_name` is a constraint class as a **bare**
//! folder-shaped entity (`Instance{class_name: WeldConstraint, …}` +
//! [`LoadedFromFile`]). No constraint component, no Avian joint is
//! attached on that path. So before anything can be *resolved*, the joint
//! has to be *materialised* from disk.
//!
//! `cache_loaded_constraint_refs` finds those bare loaded-constraint
//! entities and reads each one's `[constraint]` (joint parameters) +
//! `[references]` (the importer's resolved part/attachment **UUIDs**,
//! keyed by the Roblox property name `Part0`/`Part1`/`Attachment0`/
//! `Attachment1`) tables from disk **once**, staging them onto a
//! [`PendingConstraintJoint`] component. `resolve_pending_constraint_joints`
//! then resolves each staged UUID → live `Entity` via the `Instance.uuid`
//! index (no disk, retried each frame until the parts stream in) and, once
//! both ends resolve, inserts the bound Avian joint + a [`ConstraintBound`]
//! marker.
//!
//! ## Pass 2 — placeholder fix-up (per-constraint systems)
//!
//! For constraints created via [`ClassSpawner::spawn`] (the binary-ECS
//! round-trip + future registry-driven Insert), the entity already
//! carries the Eustress constraint component + an Avian joint with
//! placeholder bodies, plus a [`ConstraintRefs`] component the spawner
//! populated from `[references]`. These systems walk every joint still
//! holding a placeholder and patch `body1`/`body2`:
//!
//! - **Part-referenced constraints** (`WeldConstraint`, `Motor6D`,
//!   `HingeConstraint`, `DistanceConstraint`, `PrismaticConstraint`,
//!   `BallSocketConstraint`, `RopeConstraint`) — bind directly to the
//!   referenced part entity.
//! - **Attachment-referenced constraints** (`RodConstraint`,
//!   `CylindricalConstraint`, `TorsionSpringConstraint`,
//!   `UniversalConstraint`) — resolve to the `Attachment` entity, then
//!   walk up its [`ChildOf`] chain to the owning Avian [`RigidBody`].
//!
//! Both ends are resolved **by UUID** against an `Instance.uuid` index
//! built once per pass (the importer stamps a deterministic uuid in
//! `[metadata].uuid`; the loader reads it into `Instance.uuid`). The
//! legacy `Instance.id` path is kept only as a fallback for the JSON
//! scene loader, which still keys part refs by the live entity id.
//!
//! ## Why UUID, not instance-id
//!
//! TOML-loaded parts spawn with `Instance.id == 0` (the loader hardcodes
//! the sentinel), so an id-keyed match can never bind an imported
//! constraint and — worse — would mis-bind every id-0 entity. `Instance.uuid`
//! is the only stable cross-reference identity present after load.
//!
//! ## Self-limiting, idempotent, and safe
//!
//! - Pass 1 inserts [`ConstraintBound`] once the joint is materialised, so
//!   each loaded constraint is processed exactly once.
//! - Pass 2 inspects a joint only while it still holds a placeholder; once
//!   both ends are patched it is skipped. We never match the empty UUID or
//!   instance-id `0`, so an unpopulated ref can't mis-bind.
//! - We patch each side independently and only when resolution succeeds,
//!   so a half-resolvable joint still binds the side it can.
//!
//! ## Gating
//!
//! Systems run only `in_state(PlayModeState::Playing)`, matching the
//! mover runtime in [`crate::physics::movers`] — joints only need binding
//! once the Avian solver is live.

use std::collections::HashMap;

use bevy::prelude::*;

use avian3d::prelude::{
    AngleLimit, DistanceJoint, FixedJoint, JointDisabled, PrismaticJoint, RevoluteJoint, RigidBody,
    SphericalJoint,
};

use eustress_common::classes::{
    BallSocketConstraint, ClassName, CylindricalConstraint, DistanceConstraint, HingeConstraint,
    Instance, Motor6D, PrismaticConstraint, RodConstraint, RopeConstraint, TorsionSpringConstraint,
    UniversalConstraint, WeldConstraint,
};

use crate::play_mode::PlayModeState;
use crate::space::file_loader::LoadedFromFile;

// ─────────────────────────────────────────────────────────────────────────
// Reference-carrier component (surfaced by the spawners)
// ─────────────────────────────────────────────────────────────────────────

/// Resolved part/attachment **UUIDs** carried onto a constraint entity so
/// the resolver can bind its Avian joint by stable identity. Mirrors the
/// `BeamSegmentLink` precedent (`spawners/audio_vfx/beam.rs`): the spawner
/// has only `Commands` (no `World`), so it records the references here and
/// a runtime system resolves them.
///
/// The constraint spawners populate this from the `[references]` table the
/// Roblox importer writes (`Part0`/`Part1` for part-referenced joints,
/// `Attachment0`/`Attachment1` for attachment-referenced ones). Empty
/// strings / `None` mean "no ref authored on that side".
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct ConstraintRefs {
    /// UUID of the entity referenced by `Part0` / `Attachment0`.
    pub ref0_uuid: Option<String>,
    /// UUID of the entity referenced by `Part1` / `Attachment1`.
    pub ref1_uuid: Option<String>,
}

impl ConstraintRefs {
    /// Build from two optional uuid strings, normalising empty strings to
    /// `None` so an unauthored `Part0 = ""` ref never tries to resolve.
    pub fn new(ref0: Option<String>, ref1: Option<String>) -> Self {
        Self {
            ref0_uuid: ref0.filter(|s| !s.is_empty()),
            ref1_uuid: ref1.filter(|s| !s.is_empty()),
        }
    }
}

/// Idempotency marker — placed on a loaded constraint entity once its
/// Avian joint has been materialised + bound, so the binder never touches
/// it again.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct ConstraintBound;

/// "Pass 1a has looked at this entity" marker. Inserted on EVERY
/// `LoadedFromFile` entity the disk-read pass inspects — constraint or not
/// — so the pass never re-scans it. This bounds Pass 1a to O(loaded
/// entities) once, instead of a per-frame full scan of every loaded
/// folder/part (the codebase's "no continuous checks that destroy FPS at
/// scale" rule). Constraints additionally get a [`PendingConstraintJoint`];
/// non-constraints get only this marker and drop out forever.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct ConstraintScanned;

/// Load-path staging component: everything needed to materialise a loaded
/// constraint's Avian joint, parsed from its `_instance.toml` **once** by
/// [`cache_loaded_constraint_refs`]. A second system
/// ([`resolve_pending_constraint_joints`]) resolves the ref UUIDs → bodies
/// against the live `Instance.uuid` index each frame (cheap, no disk) and,
/// once both ends resolve, inserts the joint + [`ConstraintBound`] and
/// removes this component.
///
/// Splitting "read disk once" from "resolve every frame until the parts
/// stream in" keeps the per-frame retry allocation- and I/O-free — the
/// exact "no continuous checks that destroy FPS at scale" requirement.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct PendingConstraintJoint {
    /// UUID referenced by `Part0` / `Attachment0` (empty-normalised).
    pub ref0_uuid: Option<String>,
    /// UUID referenced by `Part1` / `Attachment1` (empty-normalised).
    pub ref1_uuid: Option<String>,
    /// True ⇒ refs point at `Attachment` entities (walk to the owning
    /// body); false ⇒ refs point directly at part bodies.
    pub attachment_kind: bool,
    /// `[constraint].enabled` — when false the joint binds but the solver
    /// skips it (`JointDisabled`).
    pub enabled: bool,
    /// Per-class joint scalars read from `[constraint]` (lower/upper angle,
    /// min/max distance, length, rest_length). Stored raw; the builder
    /// picks the ones its class needs. `f32::NAN` ⇒ "absent, use default".
    pub lower: f32,
    pub upper: f32,
    pub length: f32,
}

impl PendingConstraintJoint {
    fn scalar(v: f32) -> Option<f32> {
        if v.is_nan() {
            None
        } else {
            Some(v)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Shared resolution helpers
// ─────────────────────────────────────────────────────────────────────────

/// Build a `uuid → Entity` index from the live `Instance` query. Keyed by
/// the 32-hex `Instance.uuid`; entities with an empty uuid are skipped (an
/// empty key can never be a valid lookup target). Linear build over all
/// `Instance`s — run once per resolve pass, not per joint.
fn build_uuid_index<'a>(
    instances: impl Iterator<Item = (Entity, &'a Instance)>,
) -> HashMap<&'a str, Entity> {
    let mut map = HashMap::new();
    for (e, inst) in instances {
        if !inst.uuid.is_empty() {
            // First-writer-wins: uuids are unique by construction, so a
            // duplicate would be a data error; keeping the first is the
            // safe, deterministic choice.
            map.entry(inst.uuid.as_str()).or_insert(e);
        }
    }
    map
}

/// Resolve a Eustress instance-id to its Bevy `Entity` by scanning the
/// `Instance` query. Returns `None` for the unassigned sentinel (`0`) or
/// when no live entity carries that id. Legacy fallback for the JSON scene
/// loader, which still keys refs by live entity id.
fn entity_for_instance_id(id: u32, instances: &Query<(Entity, &Instance)>) -> Option<Entity> {
    if id == 0 {
        return None;
    }
    instances
        .iter()
        .find(|(_, inst)| inst.id == id)
        .map(|(e, _)| e)
}

/// Walk up the [`ChildOf`] chain from `start` (inclusive) and return the
/// first ancestor that is an Avian rigid body. Mirrors
/// [`crate::physics::movers::resolve_target_body`] — bounded to 8 hops to
/// defend against malformed cyclic hierarchies. Used by the
/// attachment-referenced constraints, whose refs point at the
/// `Attachment` entity rather than the part body.
fn rigid_body_ancestor(
    start: Entity,
    child_of: &Query<&ChildOf>,
    is_body: &impl Fn(Entity) -> bool,
) -> Option<Entity> {
    let mut current = start;
    for _ in 0..8 {
        if is_body(current) {
            return Some(current);
        }
        match child_of.get(current) {
            Ok(parent) => current = parent.0,
            Err(_) => return None,
        }
    }
    None
}

/// Resolve a part-referenced side: prefer the UUID carried on
/// [`ConstraintRefs`], fall back to the legacy instance-id on the
/// component. The part entity binds the joint directly.
fn resolve_part_side(
    uuid: Option<&str>,
    legacy_id: Option<u32>,
    uuid_index: &HashMap<&str, Entity>,
    instances: &Query<(Entity, &Instance)>,
) -> Option<Entity> {
    if let Some(u) = uuid.filter(|s| !s.is_empty()) {
        if let Some(&e) = uuid_index.get(u) {
            return Some(e);
        }
    }
    entity_for_instance_id(legacy_id?, instances)
}

/// Resolve an attachment-referenced side: map UUID (or legacy id) → the
/// `Attachment` entity, then climb to its owning rigid body.
fn resolve_attachment_side(
    uuid: Option<&str>,
    legacy_id: Option<u32>,
    uuid_index: &HashMap<&str, Entity>,
    instances: &Query<(Entity, &Instance)>,
    child_of: &Query<&ChildOf>,
    is_body: &impl Fn(Entity) -> bool,
) -> Option<Entity> {
    let attachment = if let Some(u) = uuid.filter(|s| !s.is_empty()) {
        uuid_index
            .get(u)
            .copied()
            .or_else(|| entity_for_instance_id(legacy_id?, instances))?
    } else {
        entity_for_instance_id(legacy_id?, instances)?
    };
    rigid_body_ancestor(attachment, child_of, is_body)
}

// ─────────────────────────────────────────────────────────────────────────
// Pass 1 — load-path binder
// ─────────────────────────────────────────────────────────────────────────

/// Classify a constraint class by the **Roblox reference property** its
/// `[references]` table carries (which the importer keys by the raw Roblox
/// name), and by how that ref resolves to a body:
///
/// - [`RefKind::Part`] — `Part0`/`Part1` point **directly at part bodies**.
///   In Roblox only `WeldConstraint` and `Motor6D` are part-referenced.
/// - [`RefKind::Attachment`] — `Attachment0`/`Attachment1` point at
///   `Attachment` entities; the joint binds the attachment's **owning
///   RigidBody** (walk the `ChildOf` chain). Every other modern Roblox
///   constraint (`HingeConstraint`, `DistanceConstraint`,
///   `PrismaticConstraint`, `BallSocketConstraint`, `SpringConstraint`,
///   `RopeConstraint`, `RodConstraint`, `CylindricalConstraint`,
///   `TorsionSpringConstraint`, `UniversalConstraint`) is attachment-based.
///
/// NOTE: the Eustress component field happens to be named `part0`/`part1`
/// for several attachment-based classes (Hinge/Distance/…), but that is a
/// component-naming artifact — the *on-disk reference property* the
/// importer writes is `Attachment0`/`Attachment1` for those, so the binder
/// keys off the Roblox name here, not the component field.
fn constraint_ref_kind(class: ClassName) -> Option<RefKind> {
    use ClassName::*;
    match class {
        // Part-referenced (refs point straight at bodies).
        WeldConstraint | Motor6D => Some(RefKind::Part),
        // Attachment-referenced (refs point at Attachments → owning body).
        HingeConstraint
        | DistanceConstraint
        | PrismaticConstraint
        | BallSocketConstraint
        | SpringConstraint
        | RopeConstraint
        | RodConstraint
        | CylindricalConstraint
        | TorsionSpringConstraint
        | UniversalConstraint => Some(RefKind::Attachment),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefKind {
    Part,
    Attachment,
}

/// Read the two reference UUIDs from a parsed `_instance.toml`'s
/// `[references]` table for the given [`RefKind`]. The importer keys these
/// by the raw Roblox property name (`Part0`/`Part1` or
/// `Attachment0`/`Attachment1`); we also accept lowercase for
/// hand-authored files.
fn read_reference_uuids(doc: &toml::Value, kind: RefKind) -> (Option<String>, Option<String>) {
    let refs = match doc.get("references").and_then(|v| v.as_table()) {
        Some(t) => t,
        None => return (None, None),
    };
    let (k0, k0_lc, k1, k1_lc) = match kind {
        RefKind::Part => ("Part0", "part0", "Part1", "part1"),
        RefKind::Attachment => ("Attachment0", "attachment0", "Attachment1", "attachment1"),
    };
    let read = |upper: &str, lower: &str| -> Option<String> {
        refs.get(upper)
            .or_else(|| refs.get(lower))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    };
    (read(k0, k0_lc), read(k1, k1_lc))
}

/// Read a constraint's enabled flag from the `[constraint]` table (the
/// class-schema template's home for it). Defaults to `true` when absent.
fn read_constraint_enabled(doc: &toml::Value) -> bool {
    doc.get("constraint")
        .and_then(|c| c.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// Read a `f32` field from the `[constraint]` table.
fn read_constraint_f32(doc: &toml::Value, key: &str) -> Option<f32> {
    doc.get("constraint")
        .and_then(|c| c.get(key))
        .and_then(|v| {
            v.as_float()
                .map(|f| f as f32)
                .or_else(|| v.as_integer().map(|i| i as f32))
        })
}

/// Pass 1a: read each freshly-loaded constraint's `_instance.toml` **once**
/// and stage everything needed to build its joint onto a
/// [`PendingConstraintJoint`] component. This is the only system that
/// touches the disk.
///
/// Every `LoadedFromFile` entity it inspects — constraint or not — gets a
/// [`ConstraintScanned`] marker so the pass never re-scans it; this bounds
/// total work to O(loaded entities) once rather than a per-frame full
/// scan. Spawner-created constraints carry no `LoadedFromFile`, so they
/// never match here — they already have their Avian joint and are handled
/// by Pass 2.
#[allow(clippy::type_complexity)]
fn cache_loaded_constraint_refs(
    mut commands: Commands,
    pending: Query<(Entity, &Instance, &LoadedFromFile), Without<ConstraintScanned>>,
) {
    for (entity, inst, loaded) in pending.iter() {
        // Mark scanned no matter the outcome so this entity is never
        // re-iterated by this pass.
        let kind = match constraint_ref_kind(inst.class_name) {
            Some(k) => k,
            None => {
                commands.entity(entity).insert(ConstraintScanned);
                continue;
            }
        };

        // The loader stores the FOLDER path on `LoadedFromFile`; the
        // constraint payload lives in the `_instance.toml` inside it.
        let toml_path = if loaded.path.is_dir() {
            loaded.path.join("_instance.toml")
        } else {
            loaded.path.clone()
        };
        let doc: toml::Value = match std::fs::read_to_string(&toml_path).map(|s| s.parse()) {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                warn!("joint_resolver: parse {}: {e}", toml_path.display());
                commands.entity(entity).insert(ConstraintScanned);
                continue;
            }
            Err(e) => {
                warn!("joint_resolver: read {}: {e}", toml_path.display());
                commands.entity(entity).insert(ConstraintScanned);
                continue;
            }
        };

        let (r0, r1) = read_reference_uuids(&doc, kind);
        // No refs authored on either side ⇒ nothing this binder can bind.
        if r0.is_none() && r1.is_none() {
            commands.entity(entity).insert(ConstraintScanned);
            continue;
        }

        let nan = f32::NAN;
        commands.entity(entity).insert((
            ConstraintScanned,
            PendingConstraintJoint {
                ref0_uuid: r0,
                ref1_uuid: r1,
                attachment_kind: kind == RefKind::Attachment,
                enabled: read_constraint_enabled(&doc),
                lower: read_constraint_f32(&doc, "lower_angle").unwrap_or(nan),
                upper: read_constraint_f32(&doc, "upper_angle").unwrap_or(nan),
                // `length` covers rope/rod; distance max + spring rest reuse
                // this slot (see `insert_joint_for_class`).
                length: read_constraint_f32(&doc, "length")
                    .or_else(|| read_constraint_f32(&doc, "max_distance"))
                    .or_else(|| read_constraint_f32(&doc, "rest_length"))
                    .unwrap_or(nan),
            },
        ));
    }
}

/// Pass 1b: resolve each [`PendingConstraintJoint`]'s ref UUIDs → live
/// bodies against the `Instance.uuid` index (cheap, no disk) and, once
/// BOTH ends resolve, insert the bound Avian joint + [`ConstraintBound`]
/// and drop the pending component. Entities whose parts haven't streamed
/// in yet are left pending and retried next frame — allocation- and
/// I/O-free.
fn resolve_pending_constraint_joints(
    mut commands: Commands,
    pending: Query<(Entity, &Instance, &PendingConstraintJoint)>,
    instances: Query<(Entity, &Instance)>,
    child_of: Query<&ChildOf>,
    bodies: Query<(), With<RigidBody>>,
) {
    if pending.is_empty() {
        return;
    }
    let uuid_index = build_uuid_index(instances.iter());
    let is_body = |e: Entity| bodies.get(e).is_ok();

    for (entity, inst, p) in pending.iter() {
        let (body0, body1) = if p.attachment_kind {
            (
                resolve_attachment_side(
                    p.ref0_uuid.as_deref(),
                    None,
                    &uuid_index,
                    &instances,
                    &child_of,
                    &is_body,
                ),
                resolve_attachment_side(
                    p.ref1_uuid.as_deref(),
                    None,
                    &uuid_index,
                    &instances,
                    &child_of,
                    &is_body,
                ),
            )
        } else {
            (
                resolve_part_side(p.ref0_uuid.as_deref(), None, &uuid_index, &instances),
                resolve_part_side(p.ref1_uuid.as_deref(), None, &uuid_index, &instances),
            )
        };

        // Require BOTH sides — a one-ended joint is meaningless and a
        // placeholder body would warn every solver step. Retry next frame
        // when a side is still unresolved (streaming).
        let (b0, b1) = match (body0, body1) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };

        // Local anchors (`c0`/`c1`) are not applied here — same scope as
        // the original placeholder resolver, which bound bodies only.
        insert_joint_for_class(&mut commands, entity, inst.class_name, b0, b1, p);
        let mut ec = commands.entity(entity);
        ec.insert(ConstraintBound)
            .remove::<PendingConstraintJoint>();
        // `enabled = false` ⇒ bind bodies but let Avian's solver skip the
        // joint (mirrors the spawner's `JointDisabled` insert).
        if !p.enabled {
            ec.insert(JointDisabled);
        }
    }
}

/// Insert the Avian joint component matching `class`, bound to `b0`/`b1`,
/// using the per-class scalars staged on [`PendingConstraintJoint`]. A
/// `NaN` slot means "absent on disk — use the component default".
fn insert_joint_for_class(
    commands: &mut Commands,
    entity: Entity,
    class: ClassName,
    b0: Entity,
    b1: Entity,
    p: &PendingConstraintJoint,
) {
    use ClassName::*;
    match class {
        WeldConstraint => {
            commands.entity(entity).insert(FixedJoint::new(b0, b1));
        }
        Motor6D | HingeConstraint => {
            // Avian sets angle limits via the `angle_limit` field (see the
            // hinge spawner) — there is no `with_angle_limits` builder. The
            // Eustress `HingeConstraint` component documents the angles as
            // radians and the spawner passes them through unconverted, so we
            // do the same (no deg→rad scaling) for round-trip consistency.
            let mut joint = RevoluteJoint::new(b0, b1);
            if let (Some(lo), Some(hi)) = (
                PendingConstraintJoint::scalar(p.lower),
                PendingConstraintJoint::scalar(p.upper),
            ) {
                joint.angle_limit = Some(AngleLimit::new(lo, hi));
            }
            commands.entity(entity).insert(joint);
        }
        DistanceConstraint => {
            // `length` slot carries `max_distance` for this class; min has
            // no schema slot here, so default to 0 (matches the component).
            let max = PendingConstraintJoint::scalar(p.length).unwrap_or(5.0);
            commands
                .entity(entity)
                .insert(DistanceJoint::new(b0, b1).with_limits(0.0, max));
        }
        RopeConstraint => {
            let length = PendingConstraintJoint::scalar(p.length).unwrap_or(10.0);
            commands
                .entity(entity)
                .insert(DistanceJoint::new(b0, b1).with_limits(0.0, length));
        }
        SpringConstraint => {
            // Match the spring spawner: a near-rigid distance joint pinned
            // at the rest length (compliance lives on the spawner path).
            let rest = PendingConstraintJoint::scalar(p.length).unwrap_or(5.0);
            commands
                .entity(entity)
                .insert(DistanceJoint::new(b0, b1).with_limits(rest, rest));
        }
        PrismaticConstraint => {
            commands.entity(entity).insert(PrismaticJoint::new(b0, b1));
        }
        BallSocketConstraint => {
            commands.entity(entity).insert(SphericalJoint::new(b0, b1));
        }
        // ── Attachment-referenced ──
        RodConstraint => {
            let length = PendingConstraintJoint::scalar(p.length).unwrap_or(2.0);
            commands
                .entity(entity)
                .insert(DistanceJoint::new(b0, b1).with_limits(length, length));
        }
        CylindricalConstraint => {
            commands.entity(entity).insert(PrismaticJoint::new(b0, b1));
        }
        TorsionSpringConstraint => {
            commands.entity(entity).insert(RevoluteJoint::new(b0, b1));
        }
        UniversalConstraint => {
            commands.entity(entity).insert(SphericalJoint::new(b0, b1));
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Pass 2 — per-(constraint, joint) placeholder fix-up systems
// ─────────────────────────────────────────────────────────────────────────

/// Generate a placeholder fix-up system for a part-referenced constraint.
///
/// Resolves each placeholder body by UUID (via [`ConstraintRefs`], when
/// present) and falls back to the legacy instance-id on the Eustress
/// component (the JSON scene loader still sets those).
///
/// `$sys`   — system fn name
/// `$cons`  — Eustress constraint component (provides `part0` / `part1`)
/// `$joint` — Avian joint component (provides `body1` / `body2`)
macro_rules! part_resolver_system {
    ($sys:ident, $cons:ty, $joint:ty) => {
        fn $sys(
            mut joints: Query<(&$cons, Option<&ConstraintRefs>, &mut $joint)>,
            instances: Query<(Entity, &Instance)>,
        ) {
            // Only build the index if at least one joint still needs work.
            if !joints
                .iter()
                .any(|(_, _, j)| j.body1 == Entity::PLACEHOLDER || j.body2 == Entity::PLACEHOLDER)
            {
                return;
            }
            let uuid_index = build_uuid_index(instances.iter());
            for (cons, refs, mut joint) in joints.iter_mut() {
                if joint.body1 == Entity::PLACEHOLDER {
                    let u = refs.and_then(|r| r.ref0_uuid.as_deref());
                    if let Some(e) = resolve_part_side(u, cons.part0, &uuid_index, &instances) {
                        joint.body1 = e;
                    }
                }
                if joint.body2 == Entity::PLACEHOLDER {
                    let u = refs.and_then(|r| r.ref1_uuid.as_deref());
                    if let Some(e) = resolve_part_side(u, cons.part1, &uuid_index, &instances) {
                        joint.body2 = e;
                    }
                }
            }
        }
    };
}

/// Generate a placeholder fix-up system for an attachment-referenced
/// constraint. Resolves UUID (or legacy id) → attachment → owning body.
///
/// `$sys`   — system fn name
/// `$cons`  — Eustress constraint component (provides `attachment0` / `attachment1`)
/// `$joint` — Avian joint component (provides `body1` / `body2`)
macro_rules! attachment_resolver_system {
    ($sys:ident, $cons:ty, $joint:ty) => {
        fn $sys(
            mut joints: Query<(&$cons, Option<&ConstraintRefs>, &mut $joint)>,
            instances: Query<(Entity, &Instance)>,
            child_of: Query<&ChildOf>,
            bodies: Query<(), With<RigidBody>>,
        ) {
            if !joints
                .iter()
                .any(|(_, _, j)| j.body1 == Entity::PLACEHOLDER || j.body2 == Entity::PLACEHOLDER)
            {
                return;
            }
            let uuid_index = build_uuid_index(instances.iter());
            let is_body = |e: Entity| bodies.get(e).is_ok();
            for (cons, refs, mut joint) in joints.iter_mut() {
                if joint.body1 == Entity::PLACEHOLDER {
                    let u = refs.and_then(|r| r.ref0_uuid.as_deref());
                    if let Some(e) = resolve_attachment_side(
                        u,
                        cons.attachment0,
                        &uuid_index,
                        &instances,
                        &child_of,
                        &is_body,
                    ) {
                        joint.body1 = e;
                    }
                }
                if joint.body2 == Entity::PLACEHOLDER {
                    let u = refs.and_then(|r| r.ref1_uuid.as_deref());
                    if let Some(e) = resolve_attachment_side(
                        u,
                        cons.attachment1,
                        &uuid_index,
                        &instances,
                        &child_of,
                        &is_body,
                    ) {
                        joint.body2 = e;
                    }
                }
            }
        }
    };
}

// Part-referenced constraints → joint type.
part_resolver_system!(resolve_weld_bodies, WeldConstraint, FixedJoint);
part_resolver_system!(resolve_motor6d_bodies, Motor6D, RevoluteJoint);
part_resolver_system!(resolve_hinge_bodies, HingeConstraint, RevoluteJoint);
part_resolver_system!(resolve_distance_bodies, DistanceConstraint, DistanceJoint);
part_resolver_system!(
    resolve_prismatic_bodies,
    PrismaticConstraint,
    PrismaticJoint
);
part_resolver_system!(
    resolve_ball_socket_bodies,
    BallSocketConstraint,
    SphericalJoint
);
part_resolver_system!(resolve_rope_bodies, RopeConstraint, DistanceJoint);

// Attachment-referenced constraints → joint type.
attachment_resolver_system!(resolve_rod_bodies, RodConstraint, DistanceJoint);
attachment_resolver_system!(
    resolve_cylindrical_bodies,
    CylindricalConstraint,
    PrismaticJoint
);
attachment_resolver_system!(
    resolve_torsion_spring_bodies,
    TorsionSpringConstraint,
    RevoluteJoint
);
attachment_resolver_system!(
    resolve_universal_bodies,
    UniversalConstraint,
    SphericalJoint
);

// ─────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────

/// Registers the constraint joint body resolver systems. Gated to
/// `PlayModeState::Playing`, matching [`crate::physics::movers::MoversPlugin`].
///
/// Mount via `app.add_plugins(JointResolverPlugin)` next to `MoversPlugin`.
pub struct JointResolverPlugin;

impl Plugin for JointResolverPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<ConstraintRefs>()
            .register_type::<ConstraintBound>()
            .register_type::<ConstraintScanned>()
            .register_type::<PendingConstraintJoint>()
            .add_systems(
                Update,
                (
                    // Pass 1a: read each loaded constraint's TOML once →
                    // stage `PendingConstraintJoint`.
                    cache_loaded_constraint_refs,
                    // Pass 1b: resolve staged refs → bodies and materialise
                    // the joint once both ends exist.
                    resolve_pending_constraint_joints,
                    // Pass 2: patch any remaining placeholder bodies on
                    // spawner-created joints.
                    resolve_weld_bodies,
                    resolve_motor6d_bodies,
                    resolve_hinge_bodies,
                    resolve_distance_bodies,
                    resolve_prismatic_bodies,
                    resolve_ball_socket_bodies,
                    resolve_rope_bodies,
                    resolve_rod_bodies,
                    resolve_cylindrical_bodies,
                    resolve_torsion_spring_bodies,
                    resolve_universal_bodies,
                )
                    .chain()
                    .run_if(in_state(PlayModeState::Playing)),
            );
    }
}
