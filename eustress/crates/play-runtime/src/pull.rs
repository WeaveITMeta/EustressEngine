//! The physics half of a Play frame's pull: ECS to DataModel, before scripts
//! run, for whoever simulates the session. Studio's Play runs it, and so does
//! a Player playing a world alone (`PlayRole::Authority`); a joined Player is
//! sent the host's poses and touches instead. Nothing here marks the tree
//! dirty, so none of it is written back.

use bevy::prelude::*;

use avian3d::prelude::{CollisionEnd, CollisionStart, LinearVelocity, Position, Rotation, TranslationInterpolation};

use eustress_common::classes::BasePart;
use eustress_common::datamodel::record::{cframe_of, world_cframe};
use eustress_common::datamodel::{DataModel, DmEvent, DmValue, InstanceId};
use eustress_common::play_session::{PlayDataModel, DRAWN_CFRAME, DRAWN_FROM_CFRAME};
use eustress_common::scripting::Vector3;
use eustress_common::terrain::TerrainChunkCollider;

use crate::assemblies::{drawn_pose, AssemblyMember};

/// Poses that physics (or anything else) changed since last frame.
///
/// A body eased between physics steps (play-runtime's `body_easing`), and a
/// part merged into its weld group's body (play-runtime's `assemblies`), is
/// drawn at an eased `Transform`; scripts read its physics pose, `Position`
/// and `Rotation`, and only when physics moved it.
#[allow(clippy::type_complexity)]
pub fn pull_poses(
    dm: Option<Res<PlayDataModel>>,
    moved: Query<
        (Entity, &Transform, Option<&ChildOf>, Option<&LinearVelocity>),
        (Changed<Transform>, With<BasePart>, Without<TranslationInterpolation>, Without<AssemblyMember>),
    >,
    eased: Query<
        (Entity, &Position, &Rotation, Option<&LinearVelocity>),
        (Or<(Changed<Position>, Changed<Rotation>)>, With<BasePart>, Or<(With<TranslationInterpolation>, With<AssemblyMember>)>),
    >,
    globals: Query<&GlobalTransform>,
) {
    let Some(dm) = dm else { return };
    if moved.is_empty() && eased.is_empty() {
        return;
    }
    let mut g = dm.dm.lock();
    for (e, t, parent, vel) in moved.iter() {
        let Some(id) = g.by_entity(e.to_bits()) else { continue };
        let world = match parent.and_then(|p| globals.get(p.parent()).ok()) {
            Some(pg) => pg.mul_transform(*t),
            None => GlobalTransform::from(*t),
        };
        g.set_prop_from_engine(id, "CFrame", DmValue::CFrame(world_cframe(&world)));
        // Drawn at its CFrame now: a pose it was drawn at while eased or merged,
        // or one a clone copied from its source, no longer stands.
        forget_drawn_pose(&mut g, id);
        if let Some(v) = vel {
            g.set_prop_from_engine(id, "AssemblyLinearVelocity", DmValue::Vector3(Vector3::from_vec3(v.0)));
        }
    }
    for (e, position, rotation, vel) in eased.iter() {
        let Some(id) = g.by_entity(e.to_bits()) else { continue };
        let world = GlobalTransform::from(Transform::from_translation(position.0).with_rotation(rotation.0));
        g.set_prop_from_engine(id, "CFrame", DmValue::CFrame(world_cframe(&world)));
        if let Some(v) = vel {
            g.set_prop_from_engine(id, "AssemblyLinearVelocity", DmValue::Vector3(Vector3::from_vec3(v.0)));
        }
    }
}

/// Where each part eased between physics steps, or merged into a weld group,
/// is drawn this frame ([`drawn_pose`]), kept for its `DrawnCFrame`: the pose
/// under [`DRAWN_CFRAME`], and the part's CFrame as `pull_poses` left it under
/// [`DRAWN_FROM_CFRAME`]. A script reads the pose for as long as the part's
/// CFrame is still that one, so a script that moves the part reads the move.
/// This runs before scripts, so a camera in `RenderStepped` follows the drawn
/// pose. A value is written only when it changed, and quietly: it changes
/// every frame a part moves, and no script hears of it. A part that leaves the
/// set (anchored, or split from its group), or whose pose cannot be taken, has
/// its pose forgotten; `pull_poses` forgets a moved part's.
#[allow(clippy::type_complexity)]
pub fn pull_drawn_poses(
    dm: Option<Res<PlayDataModel>>,
    drawn: Query<Entity, (With<BasePart>, Or<(With<TranslationInterpolation>, With<AssemblyMember>)>)>,
    transforms: Query<&Transform>,
    parents: Query<&ChildOf>,
    members: Query<&AssemblyMember>,
    mut uneased: RemovedComponents<TranslationInterpolation>,
    mut unmerged: RemovedComponents<AssemblyMember>,
) {
    let Some(dm) = dm else {
        uneased.clear();
        unmerged.clear();
        return;
    };
    let mut g = dm.dm.lock();
    for e in uneased.read().chain(unmerged.read()) {
        if drawn.contains(e) {
            continue;
        }
        if let Some(id) = g.by_entity(e.to_bits()) {
            forget_drawn_pose(&mut g, id);
        }
    }
    for e in drawn.iter() {
        let Some(id) = g.by_entity(e.to_bits()) else { continue };
        let (Some(pose), Some(cframe)) = (drawn_pose(e, &transforms, &parents, &members), g.get_prop(id, "CFrame")) else {
            forget_drawn_pose(&mut g, id);
            continue;
        };
        let at = DmValue::CFrame(cframe_of(Vec3::from(pose.translation), pose.rotation));
        set_if_changed(&mut g, id, DRAWN_CFRAME, at);
        set_if_changed(&mut g, id, DRAWN_FROM_CFRAME, cframe);
    }
}

/// Stores `value` under `key` when it differs from what is stored there,
/// without a `Changed` event.
fn set_if_changed(g: &mut DataModel, id: InstanceId, key: &str, value: DmValue) {
    if g.get_prop(id, key).as_ref() != Some(&value) {
        g.set_prop_quietly(id, key, value);
    }
}

/// Forgets the pose a part was drawn at, so its `DrawnCFrame` reads as its
/// CFrame. It writes only when a pose stands, so a part that never had one is
/// never written.
fn forget_drawn_pose(g: &mut DataModel, id: InstanceId) {
    if g.get_prop(id, DRAWN_FROM_CFRAME).is_some_and(|v| !v.is_nil()) {
        g.set_prop_quietly(id, DRAWN_FROM_CFRAME, DmValue::Nil);
    }
}

/// Avian contacts -> `Touched` / `TouchEnded` on both parts. A terrain chunk
/// collider has no instance of its own and touches as the Terrain.
pub fn pull_collisions(
    dm: Option<Res<PlayDataModel>>,
    mut started: MessageReader<CollisionStart>,
    mut ended: MessageReader<CollisionEnd>,
    terrain_colliders: Query<(), With<TerrainChunkCollider>>,
) {
    let Some(dm) = dm else {
        started.clear();
        ended.clear();
        return;
    };
    let mut g = dm.dm.lock();
    // `workspace.Terrain`, which a terrain chunk collider touches as.
    let terrain = g.find_service("Workspace").and_then(|ws| g.find_first_child_of_class(ws, "Terrain", false));
    let resolve = |g: &eustress_common::datamodel::DataModel, collider: Entity, body: Option<Entity>| -> Option<InstanceId> {
        g.by_entity(collider.to_bits())
            .or_else(|| body.and_then(|b| g.by_entity(b.to_bits())))
            .or_else(|| terrain.filter(|_| terrain_colliders.contains(collider)))
    };
    for ev in started.read() {
        let (Some(a), Some(b)) = (resolve(&g, ev.collider1, ev.body1), resolve(&g, ev.collider2, ev.body2)) else { continue };
        let touch_a = g.get_prop(a, "CanTouch").and_then(|v| v.as_bool()).unwrap_or(true);
        let touch_b = g.get_prop(b, "CanTouch").and_then(|v| v.as_bool()).unwrap_or(true);
        if touch_a && touch_b {
            g.push_event(DmEvent::Touched { part: a, other: b });
            g.push_event(DmEvent::Touched { part: b, other: a });
        }
    }
    for ev in ended.read() {
        let (Some(a), Some(b)) = (resolve(&g, ev.collider1, ev.body1), resolve(&g, ev.collider2, ev.body2)) else { continue };
        g.push_event(DmEvent::TouchEnded { part: a, other: b });
        g.push_event(DmEvent::TouchEnded { part: b, other: a });
    }
}
