//! # Hands on the wheel
//!
//! A seated avatar's arms reach for its seat's two grips: the attachments
//! `LeftHandGrip` and `RightHandGrip` in the seat's model (a steering wheel
//! carries both). Each arm is a two-bone chain pulled to its grip after the
//! Animator has posed it and after the foot IK has run, so the hands hold where
//! the wheel is this frame. Only the arms move. The legs keep the Animate pose,
//! and a model with one grip reaches with that arm alone.
//!
//! The grip is read from the tree: the attachment's position in its part. The
//! part's pose is composed fresh through [`TransformHelper`], as the seat's own
//! pose is, so the hand and the rider move together. The arm's bones are
//! composed from the avatar's transform down the chain, so the solve sees this
//! frame's pose, not last frame's.

use std::collections::{HashMap, HashSet};

use bevy::math::{Quat, Vec3};
use bevy::prelude::*;
use tracing::info;

use super::LiveTree;
use crate::avatar::ik::solve_two_bone;
use crate::avatar::rig::{AvatarRig, HumanoidBone};
use crate::avatar::seat::AvatarSeated;
use crate::avatar::SpawnedByAvatarRuntime;
use crate::datamodel::{DataModel, DmValue, InstanceId};

/// How fast an arm's hold eases in, per second: a grip is reached in about a
/// quarter of a second.
const REACH_RATE: f32 = 4.0;

/// Where the elbows point while the hands are on the wheel: back and down, in
/// metres from the shoulder.
const ELBOW_BACK: f32 = 0.4;
const ELBOW_DOWN: f32 = 0.3;

/// One arm: its upper arm, forearm and hand bones, and the grip it reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arm {
    pub upper: HumanoidBone,
    pub lower: HumanoidBone,
    pub hand: HumanoidBone,
    pub grip: &'static str,
}

/// The two arms, left then right.
pub const ARMS: [Arm; 2] = [
    Arm {
        upper: HumanoidBone::LeftArm,
        lower: HumanoidBone::LeftForeArm,
        hand: HumanoidBone::LeftHand,
        grip: "LeftHandGrip",
    },
    Arm {
        upper: HumanoidBone::RightArm,
        lower: HumanoidBone::RightForeArm,
        hand: HumanoidBone::RightHand,
        grip: "RightHandGrip",
    },
];

/// An arm pulled to a grip: where the elbow and the wrist land, and the world
/// rotations of the upper arm and the forearm that put them there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmSolve {
    pub elbow: Vec3,
    pub wrist: Vec3,
    pub upper: Quat,
    pub lower: Quat,
}

/// Pull an arm from its current pose to a grip at `target`, the elbow bending
/// toward `pole`. Each segment turns by the smallest rotation that takes it from
/// its present direction to its new one, so the arm keeps its twist. A grip out
/// of reach stretches the arm straight toward it. `None` when a bone has no
/// length or a number is not finite.
pub fn solve_arm(
    shoulder: Vec3,
    elbow: Vec3,
    wrist: Vec3,
    upper: Quat,
    lower: Quat,
    target: Vec3,
    pole: Vec3,
) -> Option<ArmSolve> {
    let finite = [shoulder, elbow, wrist, target, pole].iter().all(|v| v.is_finite());
    if !finite || !upper.is_finite() || !lower.is_finite() {
        return None;
    }
    let upper_vec = elbow - shoulder;
    let lower_vec = wrist - elbow;
    let (upper_len, lower_len) = (upper_vec.length(), lower_vec.length());
    if upper_len < 1e-4 || lower_len < 1e-4 {
        return None;
    }
    let (up_dir, lo_dir) = solve_two_bone(shoulder, target, upper_len, lower_len, pole);
    let new_elbow = shoulder + up_dir * upper_len;
    let delta_up = Quat::from_rotation_arc(upper_vec / upper_len, up_dir);
    let swung = delta_up * (lower_vec / lower_len);
    let delta_lo = Quat::from_rotation_arc(swung, lo_dir);
    Some(ArmSolve {
        elbow: new_elbow,
        wrist: new_elbow + lo_dir * lower_len,
        upper: (delta_up * upper).normalize(),
        lower: (delta_lo * delta_up * lower).normalize(),
    })
}

/// A grip: the part it is on, and its attachment in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Grip {
    part: Entity,
    attachment: InstanceId,
}

/// The grips of a seat's model, by arm. A side is `None` when the model has no
/// attachment of that name on a part that is drawn.
fn find_grips(g: &DataModel, seat: Entity) -> [Option<Grip>; 2] {
    let mut found = [None, None];
    let Some(seat_part) = g.by_entity(seat.to_bits()) else { return found };
    let Some(model) = g.find_first_ancestor_which_is_a(seat_part, "Model") else { return found };
    for id in g.descendants(model) {
        if g.class_of(id) != Some("Attachment") {
            continue;
        }
        let Some(side) = ARMS.iter().position(|arm| g.name_of(id) == Some(arm.grip)) else { continue };
        if found[side].is_some() {
            continue;
        }
        let Some(part) = g.parent(id) else { continue };
        let Some(part_entity) = g.entity_of(part).map(Entity::from_bits) else { continue };
        found[side] = Some(Grip { part: part_entity, attachment: id });
    }
    found
}

/// A grip's position in its part, from the attachment's `CFrame`.
fn grip_offset(g: &DataModel, grip: Grip) -> Option<Vec3> {
    match g.get_prop(grip.attachment, "CFrame") {
        Some(DmValue::CFrame(cf)) => {
            let at = cf.position.to_vec3();
            at.is_finite().then_some(at)
        }
        _ => None,
    }
}

/// Each seat's grips, kept until the tree's structure changes.
#[derive(Default)]
pub struct GripCache {
    seats: HashMap<Entity, (u64, [Option<Grip>; 2])>,
}

impl GripCache {
    fn grips(&mut self, g: &DataModel, seat: Entity) -> [Option<Grip>; 2] {
        let version = g.structure_version;
        match self.seats.get(&seat) {
            Some((known, grips)) if *known == version => *grips,
            _ => {
                let grips = find_grips(g, seat);
                self.seats.insert(seat, (version, grips));
                grips
            }
        }
    }
}

/// A seated avatar as the hands need it this frame.
struct Rider {
    avatar: Entity,
    seat: Entity,
    root: Transform,
    /// Per arm: the upper arm, forearm and hand bones, when the rig has them.
    bones: [Option<[Entity; 3]>; 2],
}

fn arm_bones(rig: &AvatarRig, arm: &Arm) -> Option<[Entity; 3]> {
    Some([rig.bone(arm.upper)?, rig.bone(arm.lower)?, rig.bone(arm.hand)?])
}

/// A bone's transform in the avatar's frame, composed from the avatar's own
/// transform down the chain: this frame's pose, not last frame's.
fn world_of(
    e: Entity,
    root: Entity,
    root_tf: &Transform,
    parents: &Query<&ChildOf>,
    locals: &Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
) -> Option<Transform> {
    let mut chain = Vec::with_capacity(16);
    let mut cur = e;
    while cur != root {
        if chain.len() > 32 {
            return None;
        }
        chain.push(cur);
        cur = parents.get(cur).ok()?.parent();
    }
    let mut acc = *root_tf;
    for node in chain.iter().rev() {
        acc = acc.mul_transform(*locals.get(*node).ok()?);
    }
    Some(acc)
}

/// Each frame: pull each seated avatar's arms to its seat's grips.
pub fn seat_hands(
    mut set: ParamSet<(
        TransformHelper,
        Query<(Entity, &AvatarSeated, &AvatarRig, &Transform), With<SpawnedByAvatarRuntime>>,
        Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
    )>,
    parents: Query<&ChildOf>,
    tree: Option<Res<LiveTree>>,
    time: Res<Time>,
    mut grips: Local<GripCache>,
    mut reach: Local<HashMap<(Entity, usize), f32>>,
    mut told: Local<HashSet<Entity>>,
) {
    let dt = time.delta_secs();
    let Some(tree) = tree else {
        reach.clear();
        return;
    };

    let riders: Vec<Rider> = set
        .p1()
        .iter()
        .map(|(avatar, seated, rig, root)| Rider {
            avatar,
            seat: seated.seat,
            root: *root,
            bones: ARMS.map(|arm| arm_bones(rig, &arm)),
        })
        .collect();

    // Each rider's grips, and their offsets in their parts, from the tree.
    let mut offsets: Vec<[Option<(Grip, Vec3)>; 2]> = Vec::with_capacity(riders.len());
    {
        let g = tree.dm.lock();
        for rider in &riders {
            let found = grips.grips(&g, rider.seat);
            offsets.push(found.map(|side| side.and_then(|grip| grip_offset(&g, grip).map(|at| (grip, at)))));
        }
    }

    // Where each grip is in the world this frame, composed the way the seat is.
    let mut targets: Vec<[Option<Vec3>; 2]> = Vec::with_capacity(riders.len());
    {
        let helper = set.p0();
        for found in &offsets {
            targets.push(found.map(|side| {
                let (grip, at) = side?;
                let part = helper.compute_global_transform(grip.part).ok()?;
                let target = part.transform_point(at);
                target.is_finite().then_some(target)
            }));
        }
    }

    let mut writes = set.p2();
    let mut active: HashSet<(Entity, usize)> = HashSet::new();
    for (rider, sides) in riders.iter().zip(&targets) {
        let forward = rider.root.rotation * Vec3::NEG_Z;
        for side in 0..ARMS.len() {
            let Some(target) = sides[side] else { continue };
            let Some([upper, lower, hand]) = rider.bones[side] else { continue };
            // The forearm must hang from the upper arm, or this is not an arm.
            if parents.get(lower).map(|p| p.parent()).ok() != Some(upper) {
                continue;
            }
            let Some(parent) = parents.get(upper).ok().map(|p| p.parent()) else { continue };
            let pose = |e: Entity| world_of(e, rider.avatar, &rider.root, &parents, &writes);
            let (Some(u), Some(l), Some(h), Some(p)) = (pose(upper), pose(lower), pose(hand), pose(parent)) else {
                continue;
            };
            let pole = u.translation - forward * ELBOW_BACK + Vec3::NEG_Y * ELBOW_DOWN;
            let Some(solve) = solve_arm(u.translation, l.translation, h.translation, u.rotation, l.rotation, target, pole)
            else {
                continue;
            };

            let key = (rider.avatar, side);
            let w = (reach.get(&key).copied().unwrap_or(0.0) + dt * REACH_RATE).min(1.0);
            reach.insert(key, w);
            active.insert(key);

            // Local rotations: the upper arm's against its parent, the forearm's
            // against the upper arm as it now is.
            let upper_local = p.rotation.inverse() * solve.upper;
            let lower_local = solve.upper.inverse() * solve.lower;
            if let Ok(mut t) = writes.get_mut(upper) {
                t.rotation = t.rotation.slerp(upper_local, w).normalize();
            }
            if let Ok(mut t) = writes.get_mut(lower) {
                t.rotation = t.rotation.slerp(lower_local, w).normalize();
            }
        }
        if sides.iter().all(Option::is_some) && told.insert(rider.avatar) {
            info!("animation: a seated avatar's hands reach its seat's grips ({:?})", rider.avatar);
        }
    }
    reach.retain(|key, _| active.contains(key));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::CFrame;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1.0e-3
    }

    /// A reachable grip is reached by the wrist, and the rotations the solve
    /// returns put the elbow and wrist where the solve says.
    #[test]
    fn a_reachable_grip_is_reached_and_the_rotations_agree_with_the_solve() {
        let shoulder = Vec3::new(0.0, 1.4, 0.0);
        let upper = Quat::from_rotation_z(0.3);
        let lower = Quat::from_rotation_x(-0.5);
        let elbow = shoulder + upper * Vec3::new(0.0, -0.3, 0.0);
        let wrist = elbow + lower * Vec3::new(0.0, -0.3, 0.0);
        let target = Vec3::new(0.25, 1.15, 0.2);
        let pole = shoulder + Vec3::new(0.0, -0.3, -0.5);
        let s = solve_arm(shoulder, elbow, wrist, upper, lower, target, pole).expect("solves");
        assert!(close(s.wrist, target), "the wrist reaches the grip: {:?}", s.wrist);
        let elbow_fk = shoulder + s.upper * (upper.inverse() * (elbow - shoulder));
        let wrist_fk = elbow_fk + s.lower * (lower.inverse() * (wrist - elbow));
        assert!(close(elbow_fk, s.elbow), "forward kinematics puts the elbow where the solve says");
        assert!(close(wrist_fk, s.wrist), "and the wrist too");
        assert!((s.upper.length() - 1.0).abs() < 1.0e-4 && (s.lower.length() - 1.0).abs() < 1.0e-4);
    }

    /// Out of reach, the arm stretches straight toward the grip.
    #[test]
    fn a_grip_out_of_reach_stretches_the_arm_toward_it() {
        let shoulder = Vec3::ZERO;
        let elbow = Vec3::new(0.0, -0.4, 0.0);
        let wrist = Vec3::new(0.0, -0.8, 0.0);
        let target = Vec3::new(0.0, 0.0, -3.0);
        let s = solve_arm(shoulder, elbow, wrist, Quat::IDENTITY, Quat::IDENTITY, target, Vec3::new(0.0, -1.0, 1.0))
            .expect("solves");
        assert!(((s.wrist - shoulder).length() - 0.8).abs() < 1.0e-3, "at full reach");
        assert!((s.wrist.normalize() - Vec3::NEG_Z).length() < 1.0e-3, "toward the grip");
    }

    /// The pole decides which way the elbow bends.
    #[test]
    fn the_elbow_bends_toward_the_pole() {
        let shoulder = Vec3::new(0.0, 1.4, 0.0);
        let elbow = shoulder + Vec3::new(0.0, -0.3, 0.0);
        let wrist = elbow + Vec3::new(0.0, -0.3, 0.0);
        let target = Vec3::new(0.0, 1.1, -0.2);
        let back = shoulder + Vec3::new(0.0, -0.3, 0.6);
        let front = shoulder + Vec3::new(0.0, -0.3, -0.6);
        let toward_back = solve_arm(shoulder, elbow, wrist, Quat::IDENTITY, Quat::IDENTITY, target, back).unwrap();
        let toward_front = solve_arm(shoulder, elbow, wrist, Quat::IDENTITY, Quat::IDENTITY, target, front).unwrap();
        assert!(
            (toward_back.elbow - back).length() < (toward_front.elbow - back).length(),
            "the elbow sits nearer the pole that asked for it"
        );
    }

    #[test]
    fn a_bone_without_length_or_a_number_that_is_not_finite_is_refused() {
        let p = Vec3::ZERO;
        assert!(solve_arm(p, p, Vec3::Y, Quat::IDENTITY, Quat::IDENTITY, Vec3::X, Vec3::Z).is_none(), "no upper arm");
        let bad = Vec3::splat(f32::NAN);
        assert!(solve_arm(p, Vec3::Y, Vec3::Y * 2.0, Quat::IDENTITY, Quat::IDENTITY, bad, Vec3::Z).is_none());
    }

    /// A seat's grips are the attachments of its model that carry the names,
    /// on the parts that are drawn; a seat outside any model has none.
    #[test]
    fn a_seats_grips_are_the_named_attachments_of_its_model() {
        let mut g = DataModel::new();
        let ws = g.get_service("Workspace").unwrap();
        let model = g.create_virtual("Model", "Coupe", Some(ws));
        let seat = g.create_virtual("Seat", "DriverSeat", Some(model));
        let wheel = g.create_virtual("Part", "SteeringWheel", Some(model));
        let left = g.create_virtual("Attachment", "LeftHandGrip", Some(wheel));
        let right = g.create_virtual("Attachment", "RightHandGrip", Some(wheel));
        g.bind_entity(seat, 11);
        g.bind_entity(wheel, 12);
        let found = find_grips(&g, Entity::from_bits(11));
        assert_eq!(found[0], Some(Grip { part: Entity::from_bits(12), attachment: left }));
        assert_eq!(found[1], Some(Grip { part: Entity::from_bits(12), attachment: right }));

        // A model with one grip reaches with that arm alone.
        let model2 = g.create_virtual("Model", "Kart", Some(ws));
        let seat2 = g.create_virtual("Seat", "KartSeat", Some(model2));
        let wheel2 = g.create_virtual("Part", "Bar", Some(model2));
        g.create_virtual("Attachment", "LeftHandGrip", Some(wheel2));
        g.bind_entity(seat2, 21);
        g.bind_entity(wheel2, 22);
        let one = find_grips(&g, Entity::from_bits(21));
        assert!(one[0].is_some() && one[1].is_none());

        // A seat outside any model has no grips.
        let loose = g.create_virtual("Seat", "Loose", Some(ws));
        g.bind_entity(loose, 31);
        assert_eq!(find_grips(&g, Entity::from_bits(31)), [None, None]);
    }

    #[test]
    fn a_grip_offset_is_the_attachment_position_in_its_part() {
        let mut g = DataModel::new();
        let ws = g.get_service("Workspace").unwrap();
        let part = g.create_virtual("Part", "Wheel", Some(ws));
        let att = g.create_virtual("Attachment", "LeftHandGrip", Some(part));
        g.set_prop_from_engine(att, "CFrame", DmValue::CFrame(CFrame::new(0.1, 0.2, 0.3)));
        let at = grip_offset(&g, Grip { part: Entity::from_bits(1), attachment: att });
        assert_eq!(at, Some(Vec3::new(0.1, 0.2, 0.3)));
    }
}
