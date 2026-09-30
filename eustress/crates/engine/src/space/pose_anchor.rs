//! Pose anchors: a part's size never scales what hangs from it.
//!
//! A part's `Transform.scale` is its size (its mesh is a unit mesh), and
//! Bevy composes a child under its parent's whole `GlobalTransform`, so a
//! part nested in a part, or an Attachment, would be scaled by its parent's
//! size. Under the `ParentPose` rule a file's `[transform]` is relative to
//! its parent's POSE (position and rotation), never its size
//! (`eustress_common::datamodel::record::compose_pose`). So each sized part
//! that holds posed children gets one hidden child, its anchor, whose scale
//! is the inverse of the part's size, and those children hang from the
//! anchor: their world pose is the part's pose composed with their own
//! `Transform`, which is exactly the file's.
//!
//! One system keeps it so, as McKale set it: when anything is reparented
//! onto a sized part, or a sized part is resized, recalculate the anchor.
//! It runs only while the open Space's rule is `ParentPose` (its
//! `space.toml` says `[space] transform_rule = "parent_pose"`); a Space
//! without the key keeps the legacy composition, and no anchors exist.
//!
//! An anchor has no `Instance`. Hierarchy walkers see through it with
//! [`logical_parent`], [`instance_children`] and [`InstanceHierarchy`]: an
//! anchor's children are its part's.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use eustress_common::classes::{BasePart, ClassName, Instance};

/// The hidden, unscaled child a sized part's posed children hang from.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PoseAnchor;

/// On a sized part: its anchor.
#[derive(Component, Debug, Clone, Copy)]
pub struct HasPoseAnchor(pub Entity);

/// Whether the open Space composes children relative to their parent's
/// pose (`ParentPose`) or under its whole transform (legacy). Set when a
/// Space loads, from its `space.toml`.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParentPoseRule(pub bool);

/// The anchor systems. They run in `PostUpdate`, before transform
/// propagation, so a child re-hung this frame is drawn in place the same
/// frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PoseAnchorSet;

pub struct PoseAnchorPlugin;

impl Plugin for PoseAnchorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParentPoseRule>()
            .configure_sets(
                PostUpdate,
                PoseAnchorSet.before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                (hang_children_on_anchors, sync_anchor_scale)
                    .chain()
                    .in_set(PoseAnchorSet)
                    .run_if(|rule: Res<ParentPoseRule>| rule.0),
            );
    }
}

/// Whether an instance hangs from its part's anchor: the placed classes,
/// whose `[transform]` is a pose relative to the part. The rest stay on the
/// part itself, because the part's size places them (a decal or texture quad
/// on a face, a light's emitting face, a particle emitter's volume, a
/// SurfaceGui or BillboardGui) or they have no pose (scripts, values, welds,
/// constraints, sounds).
pub fn hangs_from_anchor(class: ClassName, is_part: bool) -> bool {
    is_part
        || matches!(
            class,
            ClassName::Attachment
                | ClassName::Bone
                | ClassName::Model
                | ClassName::WorldModel
                | ClassName::Actor
                | ClassName::Folder
                | ClassName::Tool
                | ClassName::Accessory
        )
}

fn inverse_scale(scale: Vec3) -> Vec3 {
    let inv = |x: f32| if x.abs() > 1e-6 { 1.0 / x } else { 1.0 };
    Vec3::new(inv(scale.x), inv(scale.y), inv(scale.z))
}

/// Re-hang every placed child that was just parented to a sized part onto
/// that part's anchor, making the anchor when the part has none.
///
/// A child spawned this frame carries its file's pose, which is already
/// relative to the part's pose, so its `Transform` is kept. A child that was
/// already in the world (reparented in the Explorer, by a script, by undo)
/// keeps its world placement: its new `Transform` is its last world pose
/// relative to the part's pose, with its own world scale.
fn hang_children_on_anchors(
    mut commands: Commands,
    reparented: Query<
        (Entity, &ChildOf, &Instance, Has<BasePart>, Ref<Transform>, &GlobalTransform),
        (Changed<ChildOf>, Without<PoseAnchor>),
    >,
    parts: Query<(&Transform, &GlobalTransform, Option<&HasPoseAnchor>), With<BasePart>>,
    anchors: Query<(), With<PoseAnchor>>,
    mut made: Local<std::collections::HashMap<Entity, Entity>>,
) {
    made.clear();
    for (child, child_of, instance, is_part, transform, global) in &reparented {
        let part = child_of.parent();
        let Ok((part_transform, part_global, has_anchor)) = parts.get(part) else {
            continue; // not a sized part (an anchor, a Model, a service)
        };
        if !hangs_from_anchor(instance.class_name, is_part) {
            continue;
        }
        let existing = has_anchor
            .map(|h| h.0)
            .filter(|a| anchors.contains(*a))
            .or_else(|| made.get(&part).copied());
        let anchor = match existing {
            Some(anchor) => anchor,
            None => {
                let anchor = commands
                    .spawn((
                        PoseAnchor,
                        Name::new("PoseAnchor"),
                        Transform::from_scale(inverse_scale(part_transform.scale)),
                        Visibility::Inherited,
                        ChildOf(part),
                    ))
                    .id();
                commands.entity(part).insert(HasPoseAnchor(anchor));
                made.insert(part, anchor);
                anchor
            }
        };
        if !transform.is_added() {
            let (_, part_rotation, part_translation) = part_global.to_scale_rotation_translation();
            let (scale, rotation, translation) = global.to_scale_rotation_translation();
            let inverse = part_rotation.inverse();
            commands.entity(child).insert(Transform {
                translation: inverse * (translation - part_translation),
                rotation: (inverse * rotation).normalize(),
                scale,
            });
        }
        commands.entity(child).insert(ChildOf(anchor));
    }
}

/// Keep each anchor's scale the inverse of its part's size, so a resized
/// part moves nothing that hangs from it.
fn sync_anchor_scale(
    parts: Query<(&Transform, &HasPoseAnchor), (With<BasePart>, Changed<Transform>)>,
    mut anchors: Query<&mut Transform, (With<PoseAnchor>, Without<BasePart>)>,
) {
    for (transform, has_anchor) in &parts {
        if let Ok(mut anchor) = anchors.get_mut(has_anchor.0) {
            let scale = inverse_scale(transform.scale);
            if anchor.scale != scale {
                anchor.scale = scale;
            }
        }
    }
}

/// The instance an entity hangs under: its parent, or the part when the
/// parent is that part's anchor.
pub fn logical_parent(world: &World, entity: Entity) -> Option<Entity> {
    let parent = world.get::<ChildOf>(entity)?.parent();
    if world.get::<PoseAnchor>(parent).is_some() {
        world.get::<ChildOf>(parent).map(|c| c.parent())
    } else {
        Some(parent)
    }
}

/// An entity's children as the instance tree sees them: an anchor's
/// children in the anchor's place.
pub fn instance_children(world: &World, entity: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    if let Some(children) = world.get::<Children>(entity) {
        for child in children.iter() {
            if world.get::<PoseAnchor>(child).is_some() {
                if let Some(grand) = world.get::<Children>(child) {
                    out.extend(grand.iter());
                }
            } else {
                out.push(child);
            }
        }
    }
    out
}

/// [`logical_parent`] and [`instance_children`] for systems, as one
/// read-only parameter.
#[derive(SystemParam)]
pub struct InstanceHierarchy<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    parents: Query<'w, 's, &'static ChildOf>,
    anchors: Query<'w, 's, (), With<PoseAnchor>>,
}

impl InstanceHierarchy<'_, '_> {
    /// Whether this entity is a pose anchor.
    pub fn is_anchor(&self, entity: Entity) -> bool {
        self.anchors.contains(entity)
    }

    /// The instance an entity hangs under, seeing through an anchor.
    pub fn parent(&self, entity: Entity) -> Option<Entity> {
        let parent = self.parents.get(entity).ok()?.parent();
        if self.anchors.contains(parent) {
            self.parents.get(parent).ok().map(|c| c.parent())
        } else {
            Some(parent)
        }
    }

    /// An entity's children, an anchor's children in the anchor's place.
    pub fn children(&self, entity: Entity) -> Vec<Entity> {
        let mut out = Vec::new();
        if let Ok(children) = self.children.get(entity) {
            for child in children.iter() {
                if self.anchors.contains(child) {
                    if let Ok(grand) = self.children.get(child) {
                        out.extend(grand.iter());
                    }
                } else {
                    out.push(child);
                }
            }
        }
        out
    }
}
