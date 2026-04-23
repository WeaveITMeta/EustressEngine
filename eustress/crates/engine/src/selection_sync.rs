use bevy::prelude::*;
use crate::selection_box::Selected;
use crate::rendering::PartEntity;
use crate::classes::{Instance, ClassName};
use crate::commands::SelectionManager;
use std::sync::Arc;
use parking_lot::RwLock;

/// Classes that are abstract/non-visual and should not show selection boxes
const ABSTRACT_CLASSES: &[ClassName] = &[
    ClassName::Atmosphere,
    ClassName::Star,
    ClassName::Moon,
    ClassName::Sky,
    ClassName::SoulScript,
    ClassName::Folder,
];

/// Resource wrapper for SelectionManager in selection sync
#[derive(Resource)]
pub struct SelectionSyncManager(pub Arc<RwLock<SelectionManager>>);

/// Tracks the last-seen generation so we can skip frames where nothing changed.
#[derive(Resource, Default)]
struct SelectionGeneration(u64);

/// Tracks previous selection state for undo/redo recording.
#[derive(Resource, Default)]
struct PreviousSelection(Vec<String>);

/// Plugin to synchronize SelectionManager state with Bevy Selected components.
/// When Selected is added/removed, SelectionBoxPlugin spawns/despawns adornment meshes.
pub struct SelectionSyncPlugin {
    pub selection_manager: Arc<RwLock<SelectionManager>>,
}

impl Plugin for SelectionSyncPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(SelectionSyncManager(self.selection_manager.clone()))
            .init_resource::<SelectionGeneration>()
            .init_resource::<PreviousSelection>()
            .add_message::<SelectChildrenEvent>()
            .add_message::<SelectDescendantsEvent>()
            .add_message::<SelectParentEvent>()
            .add_message::<SelectSiblingsEvent>()
            .add_message::<InvertSelectionEvent>()
            .add_message::<SelectByClassEvent>()
            .add_message::<SelectByTagEvent>()
            .add_message::<SelectByMaterialEvent>()
            .add_systems(Update, (
                record_selection_history,
                sync_selection_components,
                handle_select_children_event,
                handle_select_descendants_event,
                handle_select_parent_event,
                handle_select_siblings_event,
                handle_invert_selection_event,
                handle_select_by_class_event,
                handle_select_by_tag_event,
                handle_select_by_material_event,
            ).chain());
    }
}

/// Fired by a keybinding / context-menu / MCP tool to add the direct
/// children of currently-selected entities to the selection.
#[derive(Event, Message, Debug, Clone, Default)]
pub struct SelectChildrenEvent;

/// Fired to recursively add all descendants of the current selection.
/// Idempotent — deduped by `SelectionManager::add_to_selection`.
#[derive(Event, Message, Debug, Clone, Default)]
pub struct SelectDescendantsEvent;

/// Replace selection with the parents of currently-selected entities.
#[derive(Event, Message, Debug, Clone, Default)]
pub struct SelectParentEvent;

/// Add entities sharing the same ChildOf parent as currently-selected
/// entities. Useful for "select the whole row" in mechanical assemblies.
#[derive(Event, Message, Debug, Clone, Default)]
pub struct SelectSiblingsEvent;

/// Replace selection with everything NOT currently selected (respecting
/// abstract-class filter — Folder/ScreenGui etc. stay excluded).
#[derive(Event, Message, Debug, Clone, Default)]
pub struct InvertSelectionEvent;

/// Add all entities of the same `ClassName` as the active selection.
/// `class_name` is the string form (`"Part"`, `"MeshPart"`, `"Light"`);
/// when empty, uses the class of the first selected entity.
#[derive(Event, Message, Debug, Clone, Default)]
pub struct SelectByClassEvent {
    pub class_name: String,
}

/// Add all entities carrying the specified CollectionService tag.
/// When empty, uses the union of tags on the current selection.
#[derive(Event, Message, Debug, Clone, Default)]
pub struct SelectByTagEvent {
    pub tag: String,
}

/// Add all entities whose `BasePart.material` matches. When empty,
/// uses the material of the first selected entity.
#[derive(Event, Message, Debug, Clone, Default)]
pub struct SelectByMaterialEvent {
    pub material: String,
}

/// Get the part_id for an entity (from PartEntity or Instance)
fn get_part_id(entity: Entity, _part_entity: Option<&PartEntity>, instance: Option<&Instance>) -> Option<String> {
    if _part_entity.is_some() || instance.is_some() {
        return Some(format!("{}v{}", entity.index(), entity.generation()));
    }
    None
}

/// Check if an instance is an abstract/non-visual class that shouldn't show selection
fn is_abstract_celestial(instance: Option<&Instance>) -> bool {
    if let Some(inst) = instance {
        ABSTRACT_CLASSES.contains(&inst.class_name)
    } else {
        false
    }
}

/// Record selection changes into CommandHistory for Ctrl+Z/Y cycling.
/// Runs BEFORE sync_selection_components so the "previous" state is captured first.
fn record_selection_history(
    selection_manager: Option<Res<SelectionSyncManager>>,
    last_gen: Res<SelectionGeneration>,
    mut prev: ResMut<PreviousSelection>,
    history: Option<ResMut<crate::commands::CommandHistory>>,
) {
    let Some(selection_manager) = selection_manager else { return };
    let mgr = selection_manager.0.read();
    let current_gen = mgr.generation();

    // Only record when generation changes — early exit BEFORE touching history
    if current_gen == last_gen.0 {
        return;
    }

    let current_ids = mgr.get_selected();
    drop(mgr);

    // Skip recording if previous and current are identical
    if prev.0 == current_ids {
        return;
    }

    // Only now access history mutably (triggers Bevy change detection)
    let Some(mut history) = history else { return };
    let cmd = crate::commands::SelectionCommand::new(prev.0.clone(), current_ids.clone());
    history.push_selection(cmd);

    // Update previous state
    prev.0 = current_ids;
}

/// System to add/remove `Selected` components based on SelectionManager state.
/// Uses generation tracking to skip the O(n) entity scan when selection hasn't changed.
/// When `Selected` is added, SelectionBoxPlugin reacts via `Added<Selected>` to spawn meshes.
/// When `Selected` is removed, `RemovedComponents<Selected>` triggers despawn.
fn sync_selection_components(
    mut commands: Commands,
    selection_manager: Option<Res<SelectionSyncManager>>,
    mut last_gen: ResMut<SelectionGeneration>,
    unselected_query: Query<(Entity, Option<&PartEntity>, Option<&Instance>), (Without<Selected>, Or<(With<eustress_common::default_scene::PartEntityMarker>, With<PartEntity>, With<Instance>)>)>,
    selected_query: Query<(Entity, Option<&PartEntity>, Option<&Instance>), With<Selected>>,
) {
    let Some(selection_manager) = selection_manager else {
        return;
    };
    let mgr = selection_manager.0.read();
    let current_gen = mgr.generation();

    // Fast path: nothing changed since last frame
    if current_gen == last_gen.0 {
        return;
    }
    last_gen.0 = current_gen;

    let selected_ids = mgr.get_selected();
    drop(mgr);
    let selected_set: std::collections::HashSet<String> = selected_ids.into_iter().collect();

    // Add Selected to newly selected entities
    for (entity, part_entity, instance) in &unselected_query {
        if is_abstract_celestial(instance) {
            continue;
        }
        if let Some(part_id) = get_part_id(entity, part_entity, instance) {
            if selected_set.contains(&part_id) {
                commands.entity(entity).insert(Selected);
            }
        }
    }

    // Remove Selected from deselected entities
    for (entity, part_entity, instance) in &selected_query {
        if let Some(part_id) = get_part_id(entity, part_entity, instance) {
            if !selected_set.contains(&part_id) {
                commands.entity(entity).remove::<Selected>();
            }
        }
    }
}

// ============================================================================
// Selection-command event handlers
// ============================================================================

/// Build a part_id the same way `part_selection::part_selection_system`
/// does, matching the contract SelectionManager expects.
fn make_part_id(entity: Entity) -> String {
    format!("{}v{}", entity.index(), entity.generation())
}

/// Handle a single-level "select children" command — adds direct
/// children of every currently-Selected entity to the selection.
pub fn handle_select_children_event(
    mut events: MessageReader<SelectChildrenEvent>,
    selected_entities: Query<Entity, With<Selected>>,
    children_query: Query<&Children>,
    instance_query: Query<Option<&Instance>>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    if events.read().next().is_none() { return; }
    let Some(mgr_res) = selection_manager else { return };

    // Snapshot entities to add (avoid mutating during query iteration).
    let mut to_add: Vec<String> = Vec::new();
    for parent in selected_entities.iter() {
        let Ok(children) = children_query.get(parent) else { continue };
        for child in children.iter() {
            // Filter abstract classes (Folder etc. stay non-visual).
            let inst = instance_query.get(child).ok().flatten();
            if is_abstract_celestial(inst) { continue; }
            to_add.push(make_part_id(child));
        }
    }

    if to_add.is_empty() { return; }
    let mgr = mgr_res.0.write();
    for id in to_add {
        mgr.add_to_selection(id);
    }
}

/// Handle recursive "select descendants" — walks the hierarchy via
/// breadth-first traversal from each currently-Selected entity.
/// Idempotent: re-running when all descendants are already selected
/// does nothing (SelectionManager dedupes).
pub fn handle_select_descendants_event(
    mut events: MessageReader<SelectDescendantsEvent>,
    selected_entities: Query<Entity, With<Selected>>,
    children_query: Query<&Children>,
    instance_query: Query<Option<&Instance>>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    if events.read().next().is_none() { return; }
    let Some(mgr_res) = selection_manager else { return };

    let mut to_add: Vec<String> = Vec::new();
    let mut visited: std::collections::HashSet<Entity> =
        selected_entities.iter().collect();
    let mut frontier: Vec<Entity> = visited.iter().copied().collect();

    // BFS until we've traversed every reachable descendant.
    while let Some(parent) = frontier.pop() {
        let Ok(children) = children_query.get(parent) else { continue };
        for child in children.iter() {
            if visited.insert(child) {
                let inst = instance_query.get(child).ok().flatten();
                if !is_abstract_celestial(inst) {
                    to_add.push(make_part_id(child));
                }
                frontier.push(child);
            }
        }
    }

    if to_add.is_empty() { return; }
    let mgr = mgr_res.0.write();
    for id in to_add {
        mgr.add_to_selection(id);
    }
}

/// Replace selection with parents. "Select Parent" is a classic
/// Unity / Maya shortcut — walk up one level in the hierarchy.
pub fn handle_select_parent_event(
    mut events: MessageReader<SelectParentEvent>,
    selected_entities: Query<Entity, With<Selected>>,
    parent_query: Query<&ChildOf>,
    instance_query: Query<Option<&Instance>>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    if events.read().next().is_none() { return; }
    let Some(mgr_res) = selection_manager else { return };

    let mut new_selection: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for entity in selected_entities.iter() {
        let Ok(child_of) = parent_query.get(entity) else { continue };
        let parent = child_of.parent();
        if !seen.insert(parent) { continue; }
        let inst = instance_query.get(parent).ok().flatten();
        if is_abstract_celestial(inst) { continue; }
        new_selection.push(make_part_id(parent));
    }

    if new_selection.is_empty() { return; }
    let mgr = mgr_res.0.write();
    mgr.set_selected(new_selection);
}

/// Add siblings (everything sharing the same ChildOf parent) of the
/// current selection.
pub fn handle_select_siblings_event(
    mut events: MessageReader<SelectSiblingsEvent>,
    selected_entities: Query<Entity, With<Selected>>,
    parent_query: Query<&ChildOf>,
    children_query: Query<&Children>,
    instance_query: Query<Option<&Instance>>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    if events.read().next().is_none() { return; }
    let Some(mgr_res) = selection_manager else { return };

    // Collect unique parents of the current selection.
    let mut parents: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for entity in selected_entities.iter() {
        if let Ok(child_of) = parent_query.get(entity) {
            parents.insert(child_of.parent());
        }
    }

    let mut to_add: Vec<String> = Vec::new();
    for parent in parents {
        let Ok(children) = children_query.get(parent) else { continue };
        for sibling in children.iter() {
            let inst = instance_query.get(sibling).ok().flatten();
            if is_abstract_celestial(inst) { continue; }
            to_add.push(make_part_id(sibling));
        }
    }

    if to_add.is_empty() { return; }
    let mgr = mgr_res.0.write();
    for id in to_add {
        mgr.add_to_selection(id);
    }
}

/// Replace selection with every selectable entity that's currently NOT
/// in the selection (respecting abstract-class filtering).
pub fn handle_invert_selection_event(
    mut events: MessageReader<InvertSelectionEvent>,
    // Broad query — match part_selection's selectable-entity filter so
    // invert doesn't accidentally pick up abstract entities.
    all_selectable: Query<(Entity, Option<&Instance>), With<Instance>>,
    selected_entities: Query<Entity, With<Selected>>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    if events.read().next().is_none() { return; }
    let Some(mgr_res) = selection_manager else { return };

    let currently_selected: std::collections::HashSet<Entity> =
        selected_entities.iter().collect();

    let new_selection: Vec<String> = all_selectable
        .iter()
        .filter(|(e, inst)| {
            !currently_selected.contains(e) && !is_abstract_celestial(*inst)
        })
        .map(|(e, _)| make_part_id(e))
        .collect();

    let mgr = mgr_res.0.write();
    mgr.set_selected(new_selection);
}

/// Select every entity whose `Instance.class_name` matches. When the
/// event string is empty, uses the class of the first selected entity.
pub fn handle_select_by_class_event(
    mut events: MessageReader<SelectByClassEvent>,
    selected_entities: Query<Entity, With<Selected>>,
    all_entities: Query<(Entity, &Instance)>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    let Some(evt) = events.read().next().cloned() else { return };
    let Some(mgr_res) = selection_manager else { return };

    let target_class = if !evt.class_name.is_empty() {
        evt.class_name
    } else {
        let Some(first) = selected_entities.iter().next() else { return };
        let Ok((_, inst)) = all_entities.get(first) else { return };
        inst.class_name.as_str().to_string()
    };

    let matches: Vec<String> = all_entities
        .iter()
        .filter(|(_, inst)| {
            inst.class_name.as_str() == target_class
                && !is_abstract_celestial(Some(*inst))
        })
        .map(|(e, _)| make_part_id(e))
        .collect();

    if matches.is_empty() { return; }
    let n = matches.len();
    let mgr = mgr_res.0.write();
    mgr.set_selected(matches);
    info!("🔎 Select by Class '{}': matched {} entities", target_class, n);
}

/// Select every entity whose `Tags` contains the event tag. When empty,
/// uses the union of tags on the current selection.
pub fn handle_select_by_tag_event(
    mut events: MessageReader<SelectByTagEvent>,
    selected_tagged: Query<&eustress_common::attributes::Tags, With<Selected>>,
    all_entities: Query<(Entity, &Instance, &eustress_common::attributes::Tags)>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    let Some(evt) = events.read().next().cloned() else { return };
    let Some(mgr_res) = selection_manager else { return };

    let mut targets: std::collections::HashSet<String> = std::collections::HashSet::new();
    if !evt.tag.is_empty() {
        targets.insert(evt.tag);
    } else {
        for tags in selected_tagged.iter() {
            for t in &tags.0 { targets.insert(t.clone()); }
        }
    }
    if targets.is_empty() { return; }

    let matches: Vec<String> = all_entities
        .iter()
        .filter(|(_, inst, tags)| {
            !is_abstract_celestial(Some(*inst))
                && tags.0.iter().any(|t| targets.contains(t))
        })
        .map(|(e, _, _)| make_part_id(e))
        .collect();

    if matches.is_empty() { return; }
    let tag_count = targets.len();
    let n = matches.len();
    let mgr = mgr_res.0.write();
    mgr.set_selected(matches);
    info!("🔎 Select by Tag ({} tag(s)): matched {} entities", tag_count, n);
}

/// Select every entity whose `BasePart.material`/`material_name`
/// matches. When empty, uses the material of the first selected entity.
pub fn handle_select_by_material_event(
    mut events: MessageReader<SelectByMaterialEvent>,
    selected_entities: Query<Entity, With<Selected>>,
    all_parts: Query<(Entity, &Instance, &eustress_common::classes::BasePart)>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    let Some(evt) = events.read().next().cloned() else { return };
    let Some(mgr_res) = selection_manager else { return };

    let target = if !evt.material.is_empty() {
        evt.material
    } else {
        let Some(first) = selected_entities.iter().next() else { return };
        let Ok((_, _, bp)) = all_parts.get(first) else { return };
        material_label(bp)
    };

    let matches: Vec<String> = all_parts
        .iter()
        .filter(|(_, inst, bp)| !is_abstract_celestial(Some(*inst)) && material_label(bp) == target)
        .map(|(e, _, _)| make_part_id(e))
        .collect();

    if matches.is_empty() { return; }
    let n = matches.len();
    let mgr = mgr_res.0.write();
    mgr.set_selected(matches);
    info!("🔎 Select by Material '{}': matched {} entities", target, n);
}

/// Canonical material label: prefer the `material_name` override when
/// set, else the enum's string form.
fn material_label(bp: &eustress_common::classes::BasePart) -> String {
    if !bp.material_name.is_empty() { bp.material_name.clone() }
    else { bp.material.as_str().to_string() }
}
