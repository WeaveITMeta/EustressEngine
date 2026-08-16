//! # Runtime fracture — split a part into independent physics bodies
//!
//! Consumes [`FractureMeshEvent`] and turns one part into two free-standing
//! dynamic rigid bodies, cut along the fracture plane.
//!
//! The geometry is pure math and lives in
//! [`eustress_common::realism::deformation::fracture_mesh`]; everything that
//! needs Avian (colliders, mass, velocity) or engine entity conventions lives
//! here, mirroring the split already used by the deformation bridge.
//!
//! ## Why the original part is hidden, never despawned
//!
//! The part being fractured is authored, persisted content. Despawning it
//! mid-play would violate the engine's "stop always restores" invariant:
//! nothing in the play-stop path re-creates a deleted authored entity, so the
//! part would be permanently gone from the scene after a play session. Instead
//! the original is hidden and its collider disabled; Stop puts it back exactly
//! as it was. Fragments carry [`SpawnedDuringPlayMode`], which the existing
//! stop path already despawns and the world-DB mirror already refuses to
//! persist.
//!
//! ## Cutting in scaled-local space
//!
//! Parts are unit meshes scaled by their transform, so a fragment's geometry
//! must have that scale BAKED IN and be spawned at `scale = ONE`. Two traps
//! this avoids: Avian collider behaviour under non-uniform scale, and — more
//! subtly — the fact that a plane's normal does not transform like a direction
//! under non-uniform scale (it maps by the inverse transpose). Cutting the raw
//! unit mesh with a naively rotated normal silently cuts the WRONG plane on any
//! non-cube part.
//!
//! ## Guards
//!
//! Fracture spawns entities in response to contact, and the fragments it
//! spawns are themselves in contact on their first frame — the obvious
//! runaway. Four independent guards bound it: a depth cap, a minimum fragment
//! size, a per-fragment cooldown, and a global per-frame budget. The pieces are
//! also spawned slightly separated, because leaving them overlapping lets the
//! solver hurl them apart and immediately re-trigger fracture.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::mesh::{Mesh, VertexAttributeValues};

use eustress_common::classes::BasePart;
use eustress_common::realism::deformation::components::FractureMeshEvent;
use eustress_common::realism::deformation::fracture_mesh::split_mesh_by_plane;

use crate::play_mode::{PlayModeState, SpawnedDuringPlayMode};

// Avian and `eustress_common::classes` both export `LinearVelocity` /
// `AngularVelocity`; alias the Avian ones exactly as `physics::movers` does so
// the two can coexist in one module.
use avian3d::prelude::{AngularVelocity as AvAngularVelocity, LinearVelocity as AvLinearVelocity};

// ── tunables ─────────────────────────────────────────────────────────────

/// How many times a piece may itself be fractured again.
const MAX_FRACTURE_DEPTH: u8 = 2;
/// Fragments whose bounding box is smaller than this (metres, longest axis)
/// are not produced — the cut is refused instead.
const MIN_FRAGMENT_EXTENT: f32 = 0.05;
/// Seconds a freshly spawned fragment is immune from fracturing again.
const FRACTURE_COOLDOWN_SECS: f32 = 0.3;
/// Maximum fracture events serviced per frame.
const MAX_FRACTURES_PER_FRAME: usize = 4;
/// Gap (metres) each half is pushed along ±normal at spawn.
const SEPARATION_GAP: f32 = 0.0015;
/// Speed (m/s) each half is given along ±normal so the crack visibly opens.
const SEPARATION_SPEED: f32 = 0.35;

// ── components ───────────────────────────────────────────────────────────

/// How many fractures deep this body is. Absent = 0.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct FractureDepth(pub u8);

/// Seconds remaining before this body may fracture again.
///
/// Non-negotiable: the contact that spawns a fragment is still resolving on
/// the frame the fragment appears, so without a cooldown a single impact
/// cascades into unbounded splitting.
#[derive(Component, Clone, Copy, Debug)]
pub struct FractureCooldown(pub f32);

/// Marks an authored part that has been fractured and is therefore hidden.
///
/// Restored on play-stop; see [`restore_fractured_originals`].
#[derive(Component, Clone, Copy, Debug)]
pub struct FracturedOriginal;

/// Marks a piece this module spawned, so it can be cleaned up on Stop.
///
/// Fragments also carry [`SpawnedDuringPlayMode`], and the engine's
/// `handle_stop_play` / `restore_scene_on_enter_edit` DO despawn everything
/// with that marker — but only on the stop paths those systems run on. A
/// duration-based auto-stop exits `Playing` without them, so fragments
/// survived into Edit mode and the scene did not reset. Owning the cleanup
/// here makes it independent of which stop path fires.
#[derive(Component, Clone, Copy, Debug)]
pub struct FractureFragment;

// ── helpers ──────────────────────────────────────────────────────────────

/// Longest axis of a mesh's bounding box, and its centroid.
fn mesh_extent_and_centroid(mesh: &Mesh) -> Option<(f32, Vec3)> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    if positions.is_empty() {
        return None;
    }
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut sum = Vec3::ZERO;
    for p in positions.iter() {
        let v = Vec3::new(p[0], p[1], p[2]);
        min = min.min(v);
        max = max.max(v);
        sum += v;
    }
    let extent = (max - min).max_element();
    Some((extent, sum / positions.len() as f32))
}

/// Bake a uniform/non-uniform scale into a mesh's vertex positions.
///
/// Fragments are spawned at `scale = ONE`, so the scale the original part
/// carried has to be folded into the geometry itself.
fn bake_scale(mesh: &mut Mesh, scale: Vec3) {
    if let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in positions.iter_mut() {
            p[0] *= scale.x;
            p[1] *= scale.y;
            p[2] *= scale.z;
        }
    }
    // Normals do NOT scale like positions under non-uniform scale; recompute
    // instead of trying to correct them analytically.
    if mesh.primitive_topology() == bevy::mesh::PrimitiveTopology::TriangleList {
        mesh.compute_normals();
    }
}

// ── systems ──────────────────────────────────────────────────────────────

/// Tick down per-fragment fracture immunity.
pub fn tick_fracture_cooldowns(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut FractureCooldown)>,
) {
    let dt = time.delta_secs();
    for (entity, mut cd) in query.iter_mut() {
        cd.0 -= dt;
        if cd.0 <= 0.0 {
            commands.entity(entity).remove::<FractureCooldown>();
        }
    }
}

/// Split fractured parts into two independent dynamic bodies.
#[allow(clippy::too_many_arguments)]
pub fn apply_fracture(
    mut commands: Commands,
    mut events: MessageReader<FractureMeshEvent>,
    parts: Query<(
        &GlobalTransform,
        &Transform,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
        &BasePart,
        Option<&FractureDepth>,
        Option<&FractureCooldown>,
        Option<&AvLinearVelocity>,
        Option<&AvAngularVelocity>,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let mut budget = MAX_FRACTURES_PER_FRAME;

    for ev in events.read() {
        if budget == 0 {
            // Say so rather than silently dropping — a scene that constantly
            // hits this is fracturing far more than intended.
            debug!("fracture: per-frame budget exhausted, deferring remaining events");
            break;
        }

        let Ok((
            gt,
            transform,
            mesh3d,
            material,
            base_part,
            depth,
            cooldown,
            lin_vel,
            ang_vel,
        )) = parts.get(ev.entity)
        else {
            continue;
        };

        if cooldown.is_some() {
            continue;
        }
        let depth = depth.copied().unwrap_or_default();
        if depth.0 >= MAX_FRACTURE_DEPTH {
            continue;
        }

        let Some(source_mesh) = meshes.get(&mesh3d.0) else {
            continue;
        };

        // Work in the part's SCALED local space so the cut plane and the
        // geometry agree. `ev.origin` / `ev.normal` are world-space.
        let scale = transform.scale;
        let mut scaled = source_mesh.clone();
        bake_scale(&mut scaled, scale);

        // World → unscaled-local for the point, then scale it, so the plane
        // lands in the same space the baked geometry now lives in.
        let affine = gt.affine();
        let inv = affine.inverse();
        let local_origin = inv.transform_point3(ev.origin) * scale;

        // A normal maps by the inverse transpose of the linear part. Under
        // uniform scale this reduces to a plain rotation, but for a stretched
        // part the naive transform tilts the plane.
        let normal_matrix = Mat3::from(inv.matrix3).transpose();
        let local_normal = (normal_matrix * ev.normal).normalize_or_zero();
        if local_normal == Vec3::ZERO {
            continue;
        }

        let result = split_mesh_by_plane(&scaled, local_origin, local_normal);
        if !result.success {
            // Plane missed the mesh, or produced only one side.
            continue;
        }
        let (Some(pos_mesh), Some(neg_mesh)) = (result.positive, result.negative) else {
            continue;
        };

        // Refuse cuts that would produce a sliver — slivers have near-zero
        // volume, which makes their mass and inertia numerically awful.
        let Some((pos_extent, pos_centroid)) = mesh_extent_and_centroid(&pos_mesh) else {
            continue;
        };
        let Some((neg_extent, neg_centroid)) = mesh_extent_and_centroid(&neg_mesh) else {
            continue;
        };
        if pos_extent < MIN_FRAGMENT_EXTENT || neg_extent < MIN_FRAGMENT_EXTENT {
            debug!(
                "fracture: refused — fragment too small ({pos_extent:.4} / {neg_extent:.4} m)"
            );
            continue;
        }

        // Density from the original part so both halves keep the parent's
        // material heft. Avian derives mass, inertia and centre of mass from
        // the collider + density, which is exact and avoids splitting the
        // inertia tensor by hand (there is no correct way to do that from a
        // mass scalar alone).
        // `BasePart.density` is the authored kg/m³ (default 900 = plastic) and is
        // the honest source here. Deriving it from `mass / volume` instead would
        // be wrong: `BasePart.mass` also defaults to a flat 900 REGARDLESS of
        // volume, so a 14 m³ plate would come out at ~62 kg/m³ — lighter than
        // cork — and its fragments would drift like balloons.
        let density = if base_part.density > 0.0 {
            base_part.density
        } else {
            let volume = (base_part.size.x * base_part.size.y * base_part.size.z).max(1.0e-6);
            base_part.mass / volume
        }
        .clamp(0.01, 100_000.0);

        let world_rot = transform.rotation;
        let world_pos = transform.translation;
        let parent_lin = lin_vel.map(|v| v.0).unwrap_or(Vec3::ZERO);
        let parent_ang = ang_vel.map(|v| v.0).unwrap_or(Vec3::ZERO);

        // World-space cut normal, used for separation.
        let world_normal = ev.normal.normalize_or_zero();

        let mut spawn_half = |mesh: Mesh, centroid: Vec3, sign: f32| -> Option<Entity> {
            // Convex hull, NOT trimesh: a dynamic trimesh body in Parry has
            // unreliable volume integrals and poor contact behaviour, and a
            // thin fragment can tunnel. A hull slightly overfills a concave
            // fragment, which is the accepted trade here.
            let hull_points: Vec<Vec3> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
                Some(VertexAttributeValues::Float32x3(p)) => {
                    p.iter().map(|v| Vec3::new(v[0], v[1], v[2])).collect()
                }
                _ => return None,
            };
            let collider = Collider::convex_hull(hull_points)?;

            let mesh_handle = meshes.add(mesh);

            // Offset along the cut normal so the two halves do not start
            // interpenetrating. Overlap on the spawn frame makes the solver
            // fling them apart, which both looks like an explosion and
            // re-triggers fracture.
            let offset = world_rot * (world_normal * (SEPARATION_GAP * sign));

            // A rotating body's fragments do not inherit its linear velocity
            // unchanged: v = v_parent + ω × r, where r is the offset from the
            // parent's centre to this fragment's centre.
            let r = world_rot * centroid;
            let inherited = parent_lin + parent_ang.cross(r);
            let separation = world_normal * (SEPARATION_SPEED * sign);

            let entity = commands
                .spawn((
                    Mesh3d(mesh_handle),
                    MeshMaterial3d(material.0.clone()),
                    Transform {
                        translation: world_pos + offset,
                        rotation: world_rot,
                        // Scale is already baked into the vertices.
                        scale: Vec3::ONE,
                    },
                    Visibility::default(),
                    collider,
                    RigidBody::Dynamic,
                    ColliderDensity(density),
                    AvLinearVelocity(inherited + separation),
                    AvAngularVelocity(parent_ang),
                    FractureDepth(depth.0 + 1),
                    FractureCooldown(FRACTURE_COOLDOWN_SECS),
                    // Ephemeral: despawned on Stop, never persisted.
                    SpawnedDuringPlayMode,
                    FractureFragment,
                    Name::new("Fragment"),
                ))
                .id();
            Some(entity)
        };

        let a = spawn_half(pos_mesh, pos_centroid, 1.0);
        let b = spawn_half(neg_mesh, neg_centroid, -1.0);

        if a.is_none() || b.is_none() {
            // Could not build a hull for one side — abandon the whole split
            // rather than leave a part half-fractured with one orphan piece.
            if let Some(e) = a {
                commands.entity(e).despawn();
            }
            if let Some(e) = b {
                commands.entity(e).despawn();
            }
            warn!("fracture: convex hull failed for a fragment; split abandoned");
            continue;
        }

        // Neutralise the original WITHOUT destroying it — it is authored data
        // and must survive to Stop.
        commands
            .entity(ev.entity)
            .insert((Visibility::Hidden, ColliderDisabled, FracturedOriginal));

        budget -= 1;
        info!(
            "💥 Fractured {:?} into 2 bodies (depth {} → {})",
            ev.entity,
            depth.0,
            depth.0 + 1
        );
    }
}

/// Un-hide fractured originals on play-stop.
///
/// Pairs with the hide-don't-despawn choice above: the fragments are cleaned
/// up by the existing `SpawnedDuringPlayMode` sweep, and this puts the authored
/// part back so Edit mode is byte-identical to how play started.
pub fn restore_fractured_originals(
    mut commands: Commands,
    query: Query<Entity, With<FracturedOriginal>>,
    fragments: Query<Entity, With<FractureFragment>>,
) {
    let mut restored = 0usize;
    for entity in query.iter() {
        commands
            .entity(entity)
            .insert(Visibility::Inherited)
            .remove::<ColliderDisabled>()
            .remove::<FracturedOriginal>();
        restored += 1;
    }

    // Despawn our own fragments rather than trusting the engine's
    // `SpawnedDuringPlayMode` sweep, which only runs on SOME stop paths — a
    // duration-based auto-stop leaves `Playing` without it, and the fragments
    // were surviving into Edit mode next to the restored original.
    let mut removed = 0usize;
    for entity in fragments.iter() {
        if commands.get_entity(entity).is_ok() {
            commands.entity(entity).despawn();
            removed += 1;
        }
    }

    if restored > 0 || removed > 0 {
        info!("Restored {restored} fractured part(s) and despawned {removed} fragment(s) on stop");
    }
}

// ── plugin ───────────────────────────────────────────────────────────────

/// Runtime fracture: splits parts into independent dynamic bodies.
pub struct FractureBridgePlugin;

impl Plugin for FractureBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (tick_fracture_cooldowns, apply_fracture)
                .chain()
                .run_if(in_state(PlayModeState::Playing)),
        )
        .add_systems(
            OnExit(PlayModeState::Playing),
            restore_fractured_originals,
        );
    }
}
