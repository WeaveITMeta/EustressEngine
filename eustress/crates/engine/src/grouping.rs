//! Group (Ctrl+G) / Ungroup (Ctrl+U) — hierarchy operations.
//!
//! The keybinding dispatch already writes `Action::Group` / `Action::Ungroup`
//! on Ctrl+G/U; they simply had no consumer (the big
//! `handle_menu_action_events` match has no arm for them, so they fell
//! through to `_ => {}` — a silent no-op, the "Ctrl+G/U do nothing"
//! regression). Handling them in this dedicated module reading the SAME
//! `MenuActionEvent` keeps the change isolated: no edits to that large
//! system or its parameter budget, and a second `MessageReader` cursor on
//! `MenuActionEvent` does not interfere with the existing reader.
//!
//! ## Scope
//!
//! ECS-level grouping: create / remove a `Model` parent and reparent the
//! selection via `ChildOf`, preserving each member's WORLD transform so
//! nothing visually jumps. The group is live immediately in the viewport
//! and the Explorer (both are ECS-driven — Explorer reads `Instance` +
//! `ChildOf`).
//!
//! ## Reversibility
//!
//! Each operation pushes exactly ONE `UndoStack` entry for the whole
//! selection, never one per object. `Action::GroupEntities` records
//! every member's pre-group `ChildOf` parent and LOCAL transform plus
//! the wrapper's own class/name/placement, so undo detaches the members
//! and despawns the wrapper; `Action::UngroupEntities` records enough of
//! each dissolved container to re-spawn it and pull its children back
//! inside with the locals they had before the dissolve.
//!
//! Group has no disk footprint at all, so it round-trips exactly.
//! Ungroup does not delete the container's folder, and the container
//! undo re-spawns is a fresh entity with no `InstanceFile` link — so
//! ungrouping a folder-backed container is only PARTLY reversible. The
//! handler says so in a toast rather than pretending otherwise.

use bevy::prelude::*;

use eustress_common::classes::{ClassName, Instance};

use crate::keybindings::Action;
use crate::notifications::NotificationManager;
use crate::selection_sync::SelectionSyncManager;
use crate::space::service_loader::ServiceComponent;
use crate::ui::MenuActionEvent;

/// Per-session selection id string (`"{index}v{generation}"`) — the form
/// the `SelectionManager` stores.
fn entity_id_str(e: Entity) -> String {
    format!("{}v{}", e.index(), e.generation())
}

/// One groupable member captured BEFORE the reparent.
///
/// Group needs both frames of reference at once: the WORLD transform is
/// what the new local is derived from (so nothing visually jumps), while
/// the LOCAL transform + `ChildOf` parent are what undo has to put back.
/// Reading them in the same pass keeps the two consistent — a later
/// re-query would see the values the reparent already overwrote.
struct GroupCandidate {
    entity: Entity,
    world_translation: Vec3,
    world_rotation: Quat,
    world_scale: Vec3,
    /// `None` when the entity carries no `Transform` at all — undo can
    /// only restore identity for those, which the toast admits.
    old_local: Option<Transform>,
    /// `None` = the member was a scene root before the group.
    old_parent: Option<Entity>,
}

/// Ctrl+G — wrap the current selection in a new `Model` parent.
fn handle_group_action(
    mut events: MessageReader<MenuActionEvent>,
    mut commands: Commands,
    selection: Option<Res<SelectionSyncManager>>,
    q: Query<(
        Entity,
        &Instance,
        Option<&GlobalTransform>,
        Option<&ServiceComponent>,
        Option<&Transform>,
        Option<&ChildOf>,
    )>,
    mut notifications: ResMut<NotificationManager>,
    // `Option` because headless/tool binaries boot `GroupingPlugin`
    // without `UndoPlugin`; in the editor it is always present.
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
) {
    let Some(selection) = selection else {
        return;
    };
    for event in events.read() {
        if !matches!(event.action, Action::Group) {
            continue;
        }
        let selected: std::collections::HashSet<String> =
            selection.0.read().get_selected().into_iter().collect();
        if selected.is_empty() {
            notifications.warning("Select objects to group (Ctrl+G)");
            continue;
        }

        // Groupable members + their world transforms. Skip services
        // (Workspace / Lighting / …) and adornments (gizmo handles).
        let mut members: Vec<GroupCandidate> = Vec::new();
        for (e, inst, gt, svc, local, child_of) in q.iter() {
            if !selected.contains(&entity_id_str(e)) {
                continue;
            }
            if svc.is_some() || inst.class_name.is_adornment() {
                continue;
            }
            let (scale, rot, trans) = gt
                .map(|g| g.to_scale_rotation_translation())
                .unwrap_or((Vec3::ONE, Quat::IDENTITY, Vec3::ZERO));
            members.push(GroupCandidate {
                entity: e,
                world_translation: trans,
                world_rotation: rot,
                world_scale: scale,
                old_local: local.copied(),
                old_parent: child_of.map(|c| c.0),
            });
        }
        if members.len() < 2 {
            notifications.warning("Select at least 2 groupable objects (Ctrl+G)");
            continue;
        }

        // Group center = mean of member world positions. The Model is
        // spawned axis-aligned + unit-scaled there, so a member's
        // preserved world transform is just its world transform with the
        // translation offset by -center (no parent rotation/scale to undo).
        let center = members.iter().map(|m| m.world_translation).sum::<Vec3>()
            / members.len() as f32;

        let model = commands
            .spawn((
                Instance {
                    name: "Model".to_string(),
                    class_name: ClassName::Model,
                    archivable: true,
                    id: 0,
                    ai: false,
                    uuid: String::new(),
                },
                Transform::from_translation(center),
                Visibility::default(),
                Name::new("Model"),
            ))
            .id();

        // Apply the reparent AND build the undo payload in one pass, so
        // the recorded `new_*` locals are literally the values inserted.
        let mut undo_members: Vec<crate::undo::GroupMember> =
            Vec::with_capacity(members.len());
        let mut without_local = 0usize;
        for m in &members {
            let new_local = Transform {
                translation: m.world_translation - center,
                rotation: m.world_rotation,
                scale: m.world_scale,
            };
            commands.entity(m.entity).insert((ChildOf(model), new_local));

            let old_local = match m.old_local {
                Some(t) => t,
                None => {
                    without_local += 1;
                    Transform::IDENTITY
                }
            };
            undo_members.push(crate::undo::GroupMember {
                entity_bits: m.entity.to_bits(),
                old_parent_bits: m.old_parent.map(|p| p.to_bits()),
                old_translation: old_local.translation.to_array(),
                old_rotation: old_local.rotation.to_array(),
                old_scale: old_local.scale.to_array(),
                new_translation: new_local.translation.to_array(),
                new_rotation: new_local.rotation.to_array(),
                new_scale: new_local.scale.to_array(),
            });
        }

        // ONE entry for the whole selection — the History panel shows
        // "Group N objects", not N rows. Group touches no files, so the
        // ECS payload is the complete record.
        let member_count = undo_members.len();
        match undo {
            Some(ref mut u) => u.push_labeled(
                format!("Group {} objects", member_count),
                crate::undo::Action::GroupEntities {
                    model_name: "Model".to_string(),
                    model_class: ClassName::Model.as_str().to_string(),
                    // `grouping.rs` spawns the wrapper unparented, at
                    // the members' mean world position, axis-aligned and
                    // unit-scaled — translation alone round-trips it.
                    model_parent_bits: None,
                    model_translation: center.to_array(),
                    members: undo_members,
                },
            ),
            None => warn!(
                "⌨️ Group: no UndoStack resource — grouping {member_count} objects \
                 was NOT recorded and cannot be undone"
            ),
        }

        selection.0.write().set_selected(vec![entity_id_str(model)]);
        // Members with no `Transform` at all can only be restored to
        // identity, which is a visible jump on undo. Say so instead of
        // letting the user discover it by pressing Ctrl+Z.
        if without_local > 0 {
            notifications.warning(format!(
                "Grouped {} objects — {} had no Transform, so Ctrl+Z returns them to their parent's origin",
                member_count, without_local,
            ));
        } else {
            notifications.info(format!("Grouped {} objects into a Model", member_count));
        }
        info!("⌨️ Group: wrapped {} objects in a Model", member_count);
    }
}

/// Ctrl+U — dissolve each selected `Model` / `Folder`, raising its
/// children one level (to the container's own parent, or the root) and
/// preserving their world transforms.
fn handle_ungroup_action(
    mut events: MessageReader<MenuActionEvent>,
    mut commands: Commands,
    selection: Option<Res<SelectionSyncManager>>,
    // `InstanceFile` rides along in this query rather than a separate
    // param: it is only read to detect the partly-irreversible
    // folder-backed case for the toast below.
    instances: Query<(
        Entity,
        &Instance,
        Option<&crate::space::instance_loader::InstanceFile>,
    )>,
    children_q: Query<&Children>,
    child_of_q: Query<&ChildOf>,
    global_q: Query<&GlobalTransform>,
    // LOCAL transforms — `global_q` gives the world frame the reparent
    // math needs, this gives the frame undo has to restore. (Plain `//`:
    // rustc rejects `///` on a function parameter.)
    local_q: Query<&Transform>,
    mut notifications: ResMut<NotificationManager>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
) {
    let Some(selection) = selection else {
        return;
    };
    for event in events.read() {
        if !matches!(event.action, Action::Ungroup) {
            continue;
        }
        let selected: std::collections::HashSet<String> =
            selection.0.read().get_selected().into_iter().collect();
        if selected.is_empty() {
            notifications.warning("Select a Model/Folder to ungroup (Ctrl+U)");
            continue;
        }

        let mut freed: Vec<String> = Vec::new();
        // One `UngroupedContainer` per dissolved container — the whole
        // multi-select becomes ONE undo entry, not one per Model.
        let mut undo_containers: Vec<crate::undo::UngroupedContainer> = Vec::new();
        // Containers whose state also lives in a folder on disk. Ungroup
        // is pure-ECS, so their folder survives the dissolve and undo
        // re-spawns a container with no `InstanceFile` link — the toast
        // below says so rather than implying a clean round-trip.
        let mut folder_backed = 0usize;
        for (model_e, inst, inst_file) in instances.iter() {
            if !selected.contains(&entity_id_str(model_e)) {
                continue;
            }
            if !matches!(inst.class_name, ClassName::Model | ClassName::Folder) {
                continue;
            }
            let Ok(children) = children_q.get(model_e) else {
                continue;
            };
            // Bevy 0.18 `Children::iter()` yields `Entity` by value (not
            // `&Entity`), so no `.copied()`. Collect up-front so the
            // borrow on `children`/the query ends before we issue commands.
            let kids: Vec<Entity> = children.iter().collect();
            if kids.is_empty() {
                continue;
            }
            // Raise children to the container's own parent (Roblox-style
            // ungroup); `None` ⇒ they become roots.
            let grandparent = child_of_q.get(model_e).ok().map(|c| c.0);
            // Read the container's own LOCAL transform BEFORE the
            // despawn is queued — undo re-spawns it from exactly this.
            let container_local = local_q
                .get(model_e)
                .ok()
                .copied()
                .unwrap_or(Transform::IDENTITY);
            let mut undo_children: Vec<crate::undo::GroupMember> =
                Vec::with_capacity(kids.len());
            for child in kids {
                // The local the child had INSIDE the container — the
                // value undo has to put back. Read before the rewrite
                // below overwrites it.
                let old_local = local_q.get(child).ok().copied().unwrap_or(Transform::IDENTITY);
                // Preserve world transform across the re-parent. Bevy's
                // `reparented_to` / `compute_transform` do the math.
                // Falls back to the old local for children with no
                // `GlobalTransform` — they are not moved, so old == new.
                let mut new_local = old_local;
                if let Ok(child_gt) = global_q.get(child) {
                    new_local = match grandparent.and_then(|gp| global_q.get(gp).ok()) {
                        Some(gp_gt) => child_gt.reparented_to(gp_gt),
                        None => child_gt.compute_transform(),
                    };
                    commands.entity(child).insert(new_local);
                }
                match grandparent {
                    Some(gp) => {
                        commands.entity(child).insert(ChildOf(gp));
                    }
                    None => {
                        commands.entity(child).remove::<ChildOf>();
                    }
                }
                undo_children.push(crate::undo::GroupMember {
                    entity_bits: child.to_bits(),
                    // Recorded for completeness only: the pre-ungroup
                    // parent IS the container, and undo re-spawns that
                    // under a fresh id, so the `UngroupEntities` arm
                    // reparents to the entity it just created instead.
                    old_parent_bits: Some(model_e.to_bits()),
                    old_translation: old_local.translation.to_array(),
                    old_rotation: old_local.rotation.to_array(),
                    old_scale: old_local.scale.to_array(),
                    new_translation: new_local.translation.to_array(),
                    new_rotation: new_local.rotation.to_array(),
                    new_scale: new_local.scale.to_array(),
                });
                freed.push(entity_id_str(child));
            }
            if inst_file.is_some() {
                folder_backed += 1;
            }
            undo_containers.push(crate::undo::UngroupedContainer {
                class_name: inst.class_name.as_str().to_string(),
                name: inst.name.clone(),
                parent_bits: grandparent.map(|gp| gp.to_bits()),
                translation: container_local.translation.to_array(),
                rotation: container_local.rotation.to_array(),
                scale: container_local.scale.to_array(),
                children: undo_children,
            });
            // Children detached above (queued first); the Model is now
            // childless when this despawn applies.
            commands.entity(model_e).despawn();
        }

        if undo_containers.is_empty() {
            notifications.warning("Ungroup: select a Model or Folder (Ctrl+U)");
            continue;
        }

        // ONE entry for the whole selection, however many containers it
        // held. Ungroup touches no files, so the ECS payload is the
        // complete record of the scene-graph half.
        let container_count = undo_containers.len();
        let freed_count = freed.len();
        match undo {
            Some(ref mut u) => u.push_labeled(
                format!(
                    "Ungroup {} container{}",
                    container_count,
                    if container_count == 1 { "" } else { "s" },
                ),
                crate::undo::Action::UngroupEntities {
                    containers: undo_containers,
                },
            ),
            None => warn!(
                "⌨️ Ungroup: no UndoStack resource — dissolving {container_count} container(s) \
                 was NOT recorded and cannot be undone"
            ),
        }

        if !freed.is_empty() {
            selection.0.write().set_selected(freed);
        }
        // Ungroup never deletes the container's folder, and the
        // container undo re-spawns is a fresh entity with no
        // `InstanceFile` — so the scene comes back but the file link
        // does not. Admit that instead of letting the user find out on
        // the next Space reload.
        if folder_backed > 0 {
            notifications.warning(format!(
                "Ungrouped {} container(s) — {} had a saved folder that stays on disk, so Ctrl+Z restores the grouping in the scene but not its file link",
                container_count, folder_backed,
            ));
        } else {
            notifications.info(format!("Ungrouped {} container(s)", container_count));
        }
        info!(
            "⌨️ Ungroup: dissolved {} container(s), freed {} children",
            container_count, freed_count,
        );
    }
}

/// Plugin wiring Group/Ungroup. Add in `main`.
pub struct GroupingPlugin;

impl Plugin for GroupingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (handle_group_action, handle_ungroup_action));
    }
}
