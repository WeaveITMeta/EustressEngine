//! The engine bridge's whole-instance edits: `instance.transform_subtree`
//! (move and turn an instance and everything under it about a pivot). The
//! MCP `transform_subtree` tool calls it.
//!
//! An edit here is an agent's content edit, held to `properties.set`'s
//! rules: Edit only ([`super::properties::editing_only`]), only under a
//! content service ([`super::attributes::content_root`]), all or nothing, and
//! one undo step per call. It saves through the same record write the Move
//! tool uses (`set_authored_transform` + `write_instance_definition_signed`).
//!
//! What moves: each record's `[transform]` is local to its parent, so moving
//! the topmost instances that have a record of their own carries everything
//! under them. A Model or Folder spawned without a record (no InstanceFile)
//! keeps no pose of its own, as with the Move tool; its children are moved
//! instead. An instance with neither a record nor recorded descendants could
//! not be saved, so the call is refused rather than moving it in memory only.

use bevy::prelude::*;
use serde_json::{json, Value};

use eustress_common::classes::Instance;

use super::parameters::{resolve, target_json};
use super::{BridgeError, BridgeRequest, BridgeResponse};
use crate::space::InstanceFile;

/// Most instances one call may move.
const MAX_MOVERS: usize = 50_000;

fn answer(req: &BridgeRequest, result: Result<Value, String>) -> BridgeResponse {
    match result {
        Ok(v) => BridgeResponse::ok(req.id.clone(), v),
        Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
    }
}

/// An instance stored in the world database rather than in a record of its
/// own (its InstanceFile names a synthetic `__bin_` path). The database
/// mirror saves its pose.
fn is_binary(world: &World, entity: Entity) -> bool {
    #[cfg(feature = "world-db")]
    if world.get::<crate::space::world_db_binary::BinaryEcsInstance>(entity).is_some() {
        return true;
    }
    world.get::<InstanceFile>(entity).is_some_and(|f| f.toml_path.to_string_lossy().contains("__bin_"))
}

/// The entity's record (or a binary core's synthetic path), relative to the
/// open Space with `/`.
fn record_rel(world: &World, entity: Entity) -> Option<String> {
    let file = world.get::<InstanceFile>(entity)?;
    let root = world.get_resource::<crate::space::SpaceRoot>()?;
    let rel = file.toml_path.strip_prefix(&root.0).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// The instances a subtree move changes: the topmost ones under (and
/// including) `root` that keep a pose of their own. `Err` names an instance
/// that keeps none and has nothing under it that does.
fn movers(world: &World, root: Entity) -> Result<Vec<Entity>, String> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(e) = stack.pop() {
        if world.get::<InstanceFile>(e).is_some() || is_binary(world, e) {
            out.push(e);
            if out.len() > MAX_MOVERS {
                return Err(format!("more than {MAX_MOVERS} instances would move; move a smaller part of the tree"));
            }
            continue;
        }
        let children: Vec<Entity> = world.get::<Children>(e).map(|c| c.iter().collect()).unwrap_or_default();
        if children.is_empty() {
            let name = world.get::<Instance>(e).map_or_else(|| format!("{e:?}"), |i| i.name.clone());
            return Err(format!(
                "{name} has no record of its own on disk and nothing under it that has one, so its pose could not be saved"
            ));
        }
        stack.extend(children.into_iter().rev());
    }
    Ok(out)
}

/// `[x, y, z]` under `key`, finite, if given.
fn vec3_param(params: &Value, key: &str) -> Result<Option<Vec3>, String> {
    let Some(v) = params.get(key) else { return Ok(None) };
    let a = v.as_array().filter(|a| a.len() == 3).ok_or(format!("`{key}` must be [x, y, z]"))?;
    let mut out = [0f32; 3];
    for (slot, x) in out.iter_mut().zip(a) {
        match x.as_f64() {
            Some(f) if f.is_finite() && f.abs() < 1.0e7 => *slot = f as f32,
            _ => return Err(format!("`{key}` must hold finite numbers under 10,000 km")),
        }
    }
    Ok(Some(Vec3::from_array(out)))
}

/// A mover's new local pose: its world pose turned by `turn` about `pivot`,
/// then moved by `shift`, expressed in its parent's frame.
pub(super) fn moved_local(
    world_pose: (Vec3, Quat),
    parent: Option<&GlobalTransform>,
    pivot: Vec3,
    turn: Quat,
    shift: Vec3,
) -> (Vec3, Quat) {
    let (pos, rot) = world_pose;
    let new_pos = pivot + turn * (pos - pivot) + shift;
    let new_rot = (turn * rot).normalize();
    match parent {
        Some(p) => {
            let (_, parent_rot, _) = p.to_scale_rotation_translation();
            (p.affine().inverse().transform_point3(new_pos), (parent_rot.inverse() * new_rot).normalize())
        }
        None => (new_pos, new_rot),
    }
}

/// Save `entity`'s pose into its record, as a Move-tool drag does. A binary
/// core is saved by the database mirror.
fn save_pose(world: &World, entity: Entity) -> Result<(), String> {
    if is_binary(world, entity) {
        return Ok(());
    }
    let (Some(file), Some(t)) = (world.get::<InstanceFile>(entity), world.get::<Transform>(entity)) else {
        return Err("no record to save the pose into".to_string());
    };
    let stamp = world.get_resource::<crate::auth::AuthState>().and_then(crate::space::instance_loader::current_stamp);
    let mut def = crate::space::instance_loader::load_instance_definition(&file.toml_path)?;
    crate::space::instance_loader::set_authored_transform(&mut def, t.translation, t.rotation, None);
    crate::space::instance_loader::write_instance_definition_signed(&file.toml_path, &mut def, stamp.as_ref())
}

fn set_local(world: &mut World, entity: Entity, (translation, rotation): (Vec3, Quat)) {
    if let Some(mut t) = world.get_mut::<Transform>(entity) {
        t.translation = translation;
        t.rotation = rotation;
    }
    if let Some(mut bp) = world.get_mut::<crate::classes::BasePart>(entity) {
        bp.cframe.translation = translation;
        bp.cframe.rotation = rotation;
    }
}

// ---------------------------------------------------------------------------
// instance.transform_subtree
// ---------------------------------------------------------------------------

/// `instance.transform_subtree`: `uuid`, `translate` [x, y, z] metres in
/// world space, `rotate_degrees` [x, y, z] (Euler XYZ, world axes), `pivot`
/// "root" (default: the instance's own position) or [x, y, z].
pub fn transform_subtree(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    answer(req, transform(world, &req.params))
}

fn transform(world: &mut World, params: &Value) -> Result<Value, String> {
    super::properties::editing_only(world)?;
    let uuid = params
        .get("uuid")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .ok_or("instance.transform_subtree names its instance by `uuid`")?;
    let t = resolve(world, &json!({ "uuid": uuid }))?;
    if super::attributes::CONTENT_ROOTS.contains(&t.class.as_str()) {
        return Err(format!("{} is a service; move an instance under it", t.class));
    }
    let shift = vec3_param(params, "translate")?.unwrap_or(Vec3::ZERO);
    let turn_deg = vec3_param(params, "rotate_degrees")?.unwrap_or(Vec3::ZERO);
    if shift == Vec3::ZERO && turn_deg == Vec3::ZERO {
        return Err("nothing to do: pass `translate` and/or `rotate_degrees`".to_string());
    }
    let turn = Quat::from_euler(EulerRot::XYZ, turn_deg.x.to_radians(), turn_deg.y.to_radians(), turn_deg.z.to_radians());
    let root_world = world.get::<GlobalTransform>(t.entity).ok_or("the instance has no world pose")?.translation();
    let pivot = match params.get("pivot") {
        None => root_world,
        Some(Value::String(s)) if s == "root" => root_world,
        Some(_) => vec3_param(params, "pivot")?.ok_or("`pivot` is \"root\" or [x, y, z]")?,
    };

    let movers = movers(world, t.entity)?;
    // Every mover must lie under a content service, checked before any change.
    let mut paths = Vec::with_capacity(movers.len());
    for &m in &movers {
        let rel = record_rel(world, m).ok_or("an instance's record is outside the open Space")?;
        super::attributes::content_root(&rel)?;
        paths.push(rel);
    }

    // Plan every new pose first.
    let mut plan = Vec::with_capacity(movers.len());
    for &m in &movers {
        let (Some(gt), Some(local)) = (world.get::<GlobalTransform>(m), world.get::<Transform>(m)) else {
            return Err("an instance in the subtree has no pose".to_string());
        };
        let (_, rot, pos) = gt.to_scale_rotation_translation();
        let parent = world.get::<ChildOf>(m).and_then(|c| world.get::<GlobalTransform>(c.parent()));
        let (lp, lr) = moved_local((pos, rot), parent, pivot, turn, shift);
        let lp = crate::space::instance_loader::safe_translation(lp, local.translation);
        plan.push((m, (local.translation, local.rotation), (lp, lr)));
    }

    // Apply and save; a failed save puts back everything done so far.
    for (i, (m, _, new)) in plan.iter().enumerate() {
        set_local(world, *m, *new);
        if let Err(e) = save_pose(world, *m) {
            for (done, old, _) in plan[..=i].iter() {
                set_local(world, *done, *old);
                let _ = save_pose(world, *done);
            }
            return Err(format!("{}: {e}; nothing was moved", paths[i]));
        }
    }
    let (old_transforms, new_transforms) = plan
        .iter()
        .map(|(m, (op, or), (np, nr))| ((m.to_bits(), op.to_array(), or.to_array()), (m.to_bits(), np.to_array(), nr.to_array())))
        .unzip();
    if let Some(mut stack) = world.get_resource_mut::<crate::undo::UndoStack>() {
        stack.push_labeled(
            format!("Transform {}", t.name),
            crate::undo::Action::TransformEntities { old_transforms, new_transforms },
        );
    }
    info!(
        "bridge instance.transform_subtree uuid={uuid} movers={} translate={:?} rotate_degrees={:?}",
        plan.len(),
        shift.to_array(),
        turn_deg.to_array()
    );
    Ok(json!({
        "instance": target_json(&t),
        "moved": paths,
        "pivot": pivot.to_array(),
        "undo_steps": 1,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn a_turn_about_the_pivot_then_a_shift() {
        // Yaw +90 degrees about the origin takes +X to -Z.
        let turn = Quat::from_rotation_y(90f32.to_radians());
        let (p, r) = moved_local((Vec3::X, Quat::IDENTITY), None, Vec3::ZERO, turn, Vec3::new(0.0, 0.0, 10.0));
        assert!(close(p, Vec3::new(0.0, 0.0, 9.0)), "{p}");
        assert!(r.angle_between(turn) < 1e-4);
        // About a pivot at (1, 0, 0) the point there stays put.
        let (p, _) = moved_local((Vec3::X, Quat::IDENTITY), None, Vec3::X, turn, Vec3::ZERO);
        assert!(close(p, Vec3::X), "{p}");
    }

    #[test]
    fn the_new_pose_is_expressed_in_the_parents_frame() {
        // A parent at (5, 0, 0) turned +90 about Y; the child sits at world
        // (5, 0, -1), i.e. local (1, 0, 0).
        let parent = GlobalTransform::from(Transform::from_xyz(5.0, 0.0, 0.0).with_rotation(Quat::from_rotation_y(90f32.to_radians())));
        let world_pos = parent.transform_point(Vec3::X);
        let (p, r) = moved_local((world_pos, parent.rotation()), Some(&parent), Vec3::ZERO, Quat::IDENTITY, Vec3::new(0.0, 3.0, 0.0));
        assert!(close(p, Vec3::new(1.0, 3.0, 0.0)), "a world shift up stays up in the parent's frame: {p}");
        assert!(r.angle_between(Quat::IDENTITY) < 1e-4, "no turn: the same local rotation");
    }
}

/// The bridge path on a bare World with the resources it reads.
#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::play_mode::PlayModeState;
    use crate::space::SpaceRoot;
    use crate::undo::{apply_undo_action, UndoStack};
    use eustress_common::classes::ClassName;
    use std::path::PathBuf;

    /// A Model with no record (as a folder-form Model spawns) holding two
    /// Parts with records, under ServerStorage, at local (1,0,0) and (-1,0,0).
    fn car(tag: &str, service: &str) -> (World, Entity, [Entity; 2], PathBuf) {
        let root = std::env::temp_dir().join(format!("eus_xform_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut world = World::new();
        world.insert_resource(SpaceRoot(root.clone()));
        world.insert_resource(UndoStack::default());
        world.insert_resource(State::new(PlayModeState::Editing));
        let instance = |name: &str, class: ClassName, id: u32, uuid: &str| Instance {
            name: name.into(),
            class_name: class,
            archivable: true,
            id,
            ai: false,
            uuid: uuid.into(),
        };
        let model = world
            .spawn((instance("Coupe", ClassName::Model, 1, "model-uuid"), Transform::IDENTITY, GlobalTransform::IDENTITY))
            .id();
        let mut parts = [Entity::PLACEHOLDER; 2];
        for (i, (name, x)) in [("Spine", 1.0f32), ("Bumper", -1.0f32)].into_iter().enumerate() {
            let record = root.join(format!("{service}/Cars/Coupe/{name}/_instance.toml"));
            std::fs::create_dir_all(record.parent().unwrap()).unwrap();
            std::fs::write(
                &record,
                format!("[metadata]\nclass_name = \"Part\"\nname = \"{name}\"\n\n[transform]\nposition = [{x}, 0.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [1.0, 1.0, 1.0]\n"),
            )
            .unwrap();
            let t = Transform::from_xyz(x, 0.0, 0.0);
            parts[i] = world
                .spawn((
                    instance(name, ClassName::Part, 2 + i as u32, &format!("{name}-uuid")),
                    InstanceFile { toml_path: record, mesh_path: PathBuf::new(), name: name.into() },
                    t,
                    GlobalTransform::from(t),
                    ChildOf(model),
                ))
                .id();
        }
        (world, model, parts, root)
    }

    fn saved_x(world: &World, part: Entity) -> f32 {
        let file = world.get::<InstanceFile>(part).unwrap();
        crate::space::instance_loader::load_instance_definition(&file.toml_path).unwrap().transform.position[0]
    }

    #[test]
    fn a_model_without_a_record_moves_by_its_parts_and_saves_them() {
        let (mut world, _, parts, root) = car("move", "ServerStorage");
        let reply = transform(&mut world, &json!({ "uuid": "model-uuid", "translate": [10.0, 0.0, 0.0] })).unwrap();
        assert_eq!(reply["moved"].as_array().unwrap().len(), 2);
        assert_eq!(world.get::<Transform>(parts[0]).unwrap().translation, Vec3::new(11.0, 0.0, 0.0));
        assert!((saved_x(&world, parts[0]) - 11.0).abs() < 1e-4, "saved into the record");
        assert!((saved_x(&world, parts[1]) - 9.0).abs() < 1e-4);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_turn_and_its_undo() {
        let (mut world, _, parts, root) = car("turn", "ServerStorage");
        transform(&mut world, &json!({ "uuid": "model-uuid", "rotate_degrees": [0.0, 90.0, 0.0], "pivot": [0.0, 0.0, 0.0] })).unwrap();
        let p = world.get::<Transform>(parts[0]).unwrap().translation;
        assert!((p - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4, "+X turned +90 about Y is -Z: {p}");
        let action = world.resource_mut::<UndoStack>().undo().expect("one undo step");
        apply_undo_action(&action, &mut world);
        assert_eq!(world.get::<Transform>(parts[0]).unwrap().translation, Vec3::X, "undo restores the pose");
        assert!(world.resource_mut::<UndoStack>().undo().is_none(), "one step for the whole subtree");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_move_during_play_or_outside_a_content_service_is_refused() {
        let (mut world, _, parts, root) = car("play", "ServerStorage");
        world.insert_resource(State::new(PlayModeState::Playing));
        assert!(transform(&mut world, &json!({ "uuid": "model-uuid", "translate": [1.0, 0.0, 0.0] })).unwrap_err().contains("Play"));
        assert_eq!(world.get::<Transform>(parts[0]).unwrap().translation, Vec3::X);
        let _ = std::fs::remove_dir_all(root);
        let (mut world, _, parts, root) = car("core", "CoreGui");
        assert!(transform(&mut world, &json!({ "uuid": "model-uuid", "translate": [1.0, 0.0, 0.0] })).is_err());
        assert_eq!(world.get::<Transform>(parts[0]).unwrap().translation, Vec3::X, "nothing moved");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_subtree_with_an_unsaveable_leaf_is_refused_whole() {
        let (mut world, model, parts, root) = car("leaf", "ServerStorage");
        let leaf = world.spawn((Transform::IDENTITY, GlobalTransform::IDENTITY, ChildOf(model))).id();
        let _ = leaf;
        let err = transform(&mut world, &json!({ "uuid": "model-uuid", "translate": [1.0, 0.0, 0.0] })).unwrap_err();
        assert!(err.contains("no record"), "{err}");
        assert_eq!(world.get::<Transform>(parts[0]).unwrap().translation, Vec3::X, "nothing moved");
        let _ = std::fs::remove_dir_all(root);
    }
}
