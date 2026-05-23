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
//! ## Scope (2026-05-22)
//!
//! ECS-level grouping: create / remove a `Model` parent and reparent the
//! selection via `ChildOf`, preserving each member's WORLD transform so
//! nothing visually jumps. The group is live immediately in the viewport
//! and the Explorer (both are ECS-driven — Explorer reads `Instance` +
//! `ChildOf`). Disk-folder persistence and undo integration land with the
//! unified-undo + representation pass: the undo system currently keys
//! entities by `Instance.id`, which `spawn_instance` sets to `0` for
//! loaded entities, so it needs the unification before Group/Ungroup can
//! record a reliable, reversible history entry.

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
    )>,
    mut notifications: ResMut<NotificationManager>,
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
        let mut members: Vec<(Entity, Vec3, Quat, Vec3)> = Vec::new();
        for (e, inst, gt, svc) in q.iter() {
            if !selected.contains(&entity_id_str(e)) {
                continue;
            }
            if svc.is_some() || inst.class_name.is_adornment() {
                continue;
            }
            let (scale, rot, trans) = gt
                .map(|g| g.to_scale_rotation_translation())
                .unwrap_or((Vec3::ONE, Quat::IDENTITY, Vec3::ZERO));
            members.push((e, trans, rot, scale));
        }
        if members.len() < 2 {
            notifications.warning("Select at least 2 groupable objects (Ctrl+G)");
            continue;
        }

        // Group center = mean of member world positions. The Model is
        // spawned axis-aligned + unit-scaled there, so a member's
        // preserved world transform is just its world transform with the
        // translation offset by -center (no parent rotation/scale to undo).
        let center =
            members.iter().map(|(_, t, _, _)| *t).sum::<Vec3>() / members.len() as f32;

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

        for (e, trans, rot, scale) in &members {
            commands.entity(*e).insert((
                ChildOf(model),
                Transform {
                    translation: *trans - center,
                    rotation: *rot,
                    scale: *scale,
                },
            ));
        }

        selection.0.write().set_selected(vec![entity_id_str(model)]);
        notifications.info(format!("Grouped {} objects into a Model", members.len()));
        info!("⌨️ Group: wrapped {} objects in a Model", members.len());
    }
}

/// Ctrl+U — dissolve each selected `Model` / `Folder`, raising its
/// children one level (to the container's own parent, or the root) and
/// preserving their world transforms.
fn handle_ungroup_action(
    mut events: MessageReader<MenuActionEvent>,
    mut commands: Commands,
    selection: Option<Res<SelectionSyncManager>>,
    instances: Query<(Entity, &Instance)>,
    children_q: Query<&Children>,
    child_of_q: Query<&ChildOf>,
    global_q: Query<&GlobalTransform>,
    mut notifications: ResMut<NotificationManager>,
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
        let mut containers = 0u32;
        for (model_e, inst) in instances.iter() {
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
            for child in kids {
                // Preserve world transform across the re-parent. Bevy's
                // `reparented_to` / `compute_transform` do the math.
                if let Ok(child_gt) = global_q.get(child) {
                    let new_local = match grandparent.and_then(|gp| global_q.get(gp).ok()) {
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
                freed.push(entity_id_str(child));
            }
            // Children detached above (queued first); the Model is now
            // childless when this despawn applies.
            commands.entity(model_e).despawn();
            containers += 1;
        }

        if containers == 0 {
            notifications.warning("Ungroup: select a Model or Folder (Ctrl+U)");
            continue;
        }
        if !freed.is_empty() {
            selection.0.write().set_selected(freed.clone());
        }
        notifications.info(format!("Ungrouped {} container(s)", containers));
        info!(
            "⌨️ Ungroup: dissolved {} container(s), freed {} children",
            containers,
            freed.len()
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
